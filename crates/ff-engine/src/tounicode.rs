//! Đọc /ToUnicode của font trong PDF (lopdf): map MÃ trong content stream →
//! Unicode. Dùng cho fontfix: subset kiểu LibreOffice gán các mã thấp 1–31
//! cho ký tự ngoài WinAnsi (chữ Việt có dấu…) — chỉ /ToUnicode giải nghĩa
//! được các mã đó; ASCII giữ nguyên mã nên không cần.

use std::collections::HashMap;
use std::path::Path;

use lopdf::{Dictionary, Document as LoDoc, Object};

use crate::formsurgery::{deref, stream_bytes};

/// Map mã→Unicode của font có BaseFont khớp `font_name` (so phần sau prefix
/// subset "ABCDEF+") trên trang `page_index`; tìm cả trong resources của Form
/// XObject (sâu tối đa 3). Không thấy/không parse được → None (fontfix rơi về
/// ASCII identity + MacRoman).
pub(crate) fn code_to_unicode(
    input: &Path,
    page_index: u16,
    font_name: &str,
) -> Option<HashMap<u32, u32>> {
    let want = font_name.rsplit('+').next().unwrap_or(font_name).trim();
    if want.is_empty() {
        return None;
    }
    let doc = LoDoc::load(input).ok()?;
    let pages = doc.get_pages();
    let page_id = *pages.get(&(u32::from(page_index) + 1))?;
    let page = doc.get_dictionary(page_id).ok()?;
    let res = deref(&doc, page.get(b"Resources").ok()?).as_dict().ok()?;
    scan_resources(&doc, res, want, 0)
}

fn scan_resources(
    doc: &LoDoc,
    res: &Dictionary,
    want: &str,
    depth: usize,
) -> Option<HashMap<u32, u32>> {
    if depth > 3 {
        return None;
    }
    if let Ok(fonts) = res.get(b"Font") {
        if let Ok(fonts) = deref(doc, fonts).as_dict() {
            for (_, fref) in fonts.iter() {
                let Ok(fd) = deref(doc, fref).as_dict() else { continue };
                let base = fd
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|o| deref(doc, o).as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();
                let suffix = base.rsplit('+').next().unwrap_or(&base);
                if suffix != want {
                    continue;
                }
                let Ok(tu) = fd.get(b"ToUnicode") else { continue };
                if let Object::Stream(s) = deref(doc, tu) {
                    if let Some(m) = parse_cmap(&stream_bytes(s)) {
                        if !m.is_empty() {
                            return Some(m);
                        }
                    }
                }
            }
        }
    }
    // Font có thể nằm trong resources của Form XObject (file Canva…).
    if let Ok(xo) = res.get(b"XObject") {
        if let Ok(xo) = deref(doc, xo).as_dict() {
            for (_, oref) in xo.iter() {
                if let Object::Stream(s) = deref(doc, oref) {
                    if let Ok(r2) = s.dict.get(b"Resources") {
                        if let Ok(r2) = deref(doc, r2).as_dict() {
                            if let Some(m) = scan_resources(doc, r2, want, depth + 1) {
                                return Some(m);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

enum Tok {
    Hex(Vec<u8>),
    Kw(String),
    ArrOpen,
    ArrClose,
}

fn tokenize(data: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'<' => {
                let mut j = i + 1;
                let mut hex: Vec<u8> = Vec::new();
                let mut hi: Option<u8> = None;
                while j < data.len() && data[j] != b'>' {
                    let c = data[j];
                    let d = match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    };
                    if let Some(d) = d {
                        match hi {
                            None => hi = Some(d),
                            Some(h) => {
                                hex.push(h * 16 + d);
                                hi = None;
                            }
                        }
                    }
                    j += 1;
                }
                if let Some(h) = hi {
                    hex.push(h * 16); // số hex lẻ digit: pad 0 theo spec PDF
                }
                out.push(Tok::Hex(hex));
                i = j + 1;
            }
            b'[' => {
                out.push(Tok::ArrOpen);
                i += 1;
            }
            b']' => {
                out.push(Tok::ArrClose);
                i += 1;
            }
            c if c.is_ascii_alphabetic() => {
                let mut j = i;
                while j < data.len() && data[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                out.push(Tok::Kw(String::from_utf8_lossy(&data[i..j]).to_string()));
                i = j;
            }
            _ => i += 1,
        }
    }
    out
}

/// Mã nguồn (≤4 byte hex) → u32 big-endian.
fn hex_code(b: &[u8]) -> Option<u32> {
    if b.is_empty() || b.len() > 4 {
        return None;
    }
    Some(b.iter().fold(0u32, |v, &x| (v << 8) | u32::from(x)))
}

/// Giá trị UTF-16BE → đúng 1 scalar Unicode (surrogate pair ok); mapping ra
/// nhiều ký tự (ligature) → None — cmap chỉ chứa 1 codepoint/glyph.
fn utf16be_single(b: &[u8]) -> Option<u32> {
    if b.len() < 2 || b.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    let mut it = char::decode_utf16(units.iter().copied());
    let first = it.next()?.ok()?;
    if it.next().is_some() {
        return None;
    }
    Some(first as u32)
}

/// Parse bfchar/bfrange của CMap ToUnicode — đủ cho file thực tế
/// (LibreOffice/Word/Canva). Lỗi cục bộ thì bỏ entry đó, không fail cả map.
fn parse_cmap(data: &[u8]) -> Option<HashMap<u32, u32>> {
    let toks = tokenize(data);
    let mut map = HashMap::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Kw(k) if k == "beginbfchar" => {
                i += 1;
                while let (Some(Tok::Hex(src)), Some(Tok::Hex(dst))) = (toks.get(i), toks.get(i + 1)) {
                    if let (Some(c), Some(u)) = (hex_code(src), utf16be_single(dst)) {
                        map.insert(c, u);
                    }
                    i += 2;
                }
            }
            Tok::Kw(k) if k == "beginbfrange" => {
                i += 1;
                loop {
                    let (Some(Tok::Hex(lo)), Some(Tok::Hex(hi))) = (toks.get(i), toks.get(i + 1)) else {
                        break;
                    };
                    let (Some(lo), Some(hi)) = (hex_code(lo), hex_code(hi)) else { break };
                    match toks.get(i + 2) {
                        Some(Tok::Hex(d)) => {
                            if let Some(u0) = utf16be_single(d) {
                                for k in 0..=hi.saturating_sub(lo).min(0xFFFF) {
                                    map.insert(lo + k, u0 + k);
                                }
                            }
                            i += 3;
                        }
                        Some(Tok::ArrOpen) => {
                            i += 3;
                            let mut c = lo;
                            while let Some(Tok::Hex(d)) = toks.get(i) {
                                if let Some(u) = utf16be_single(d) {
                                    map.insert(c, u);
                                }
                                c += 1;
                                i += 1;
                            }
                            if matches!(toks.get(i), Some(Tok::ArrClose)) {
                                i += 1;
                            }
                        }
                        _ => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bfchar_and_bfrange() {
        let cmap = br#"
/CIDInit /ProcSet findresource begin
begincmap
1 begincodespacerange <00> <FF> endcodespacerange
2 beginbfchar
<01> <1EA3>
<0E> <D83D DE00>
endbfchar
2 beginbfrange
<20> <7E> <0020>
<02> <04> [<1EBF> <1EC7> <0110>]
endbfrange
endcmap
"#;
        let m = parse_cmap(cmap).unwrap();
        assert_eq!(m.get(&0x01), Some(&0x1EA3)); // ả
        assert_eq!(m.get(&0x0E), Some(&0x1F600)); // surrogate pair → emoji
        assert_eq!(m.get(&0x20), Some(&0x20)); // range dạng 1
        assert_eq!(m.get(&0x41), Some(&0x41));
        assert_eq!(m.get(&0x7E), Some(&0x7E));
        assert_eq!(m.get(&0x03), Some(&0x1EC7)); // range dạng mảng
        assert_eq!(m.get(&0x04), Some(&0x0110));
    }
}
