//! Vá font subset trích từ PDF cho WebView dùng được qua FontFace.
//!
//! OTS (OpenType Sanitizer của Chromium) BẮT BUỘC font TrueType phải có bảng
//! `OS/2`, nhưng subset nhúng trong PDF thường bị trình tạo file bỏ bảng này
//! (PDF không cần) → `new FontFace(...)` ném "OTS parsing error: OS/2:
//! missing required table". Đã bắt được đúng thông điệp đó qua CDP Log trên
//! font NotoSansMono-Bold subset (12 bảng, đủ name/post/cmap, thiếu mỗi
//! OS/2).
//!
//! Ở đây dựng OS/2 version 4 tối thiểu từ `head` (unitsPerEm) + `hhea`
//! (ascender/descender/lineGap) + cờ đậm/nghiêng lấy từ chính text object,
//! rồi ghép lại sfnt: danh mục bảng sắp theo tag, offset pad 4 byte,
//! checksum từng bảng + head.checkSumAdjustment chuẩn. Sfnt tag `true`
//! (TrueType kiểu Apple) được chuẩn hoá về 0x00010000. Dữ liệu không phải
//! sfnt (CFF trần của font Type1C...) trả nguyên trạng — UI tự rơi về stack
//! font hệ thống.

use std::collections::HashMap;

use crate::tounicode::FontMapping;

/// Bytes font sẵn sàng cho FontFace: vá nếu cần, còn lại giữ nguyên.
/// `mapping`: giải mã từ PDF (/ToUnicode + CIDToGIDMap) — cho mã tuỳ biến của
/// subset LibreOffice lẫn font CID/Identity-H (Word/InDesign) KHÔNG có cmap.
pub(crate) fn web_compatible_font(
    data: &[u8],
    bold: bool,
    italic: bool,
    mapping: Option<&FontMapping>,
) -> Vec<u8> {
    // CFF trần (Type1C/FontFile3 — không có vỏ sfnt, header major=1): bọc
    // thành OTF để FontFace nạp được. Không bọc nổi thì trả nguyên trạng
    // (UI rơi về font đóng gói).
    if data.len() >= 4 && data[0] == 1 && data[1] == 0 {
        return wrap_bare_cff(data, bold, italic, mapping).unwrap_or_else(|| data.to_vec());
    }
    try_fix(data, bold, italic, mapping).unwrap_or_else(|| data.to_vec())
}

/// Bọc CFF trần thành OTF (tag OTTO): bảng `CFF ` giữ nguyên bytes gốc, các
/// bảng sfnt bắt buộc tổng hợp từ CFF (số glyph, encoding/charset, upm) +
/// PDF (/Widths, /Ascent, /ToUnicode). hmtx đúng bề rộng gốc → chữ trong ô
/// sửa chiếm chỗ y như trên trang.
fn wrap_bare_cff(
    data: &[u8],
    bold: bool,
    italic: bool,
    mapping: Option<&FontMapping>,
) -> Option<Vec<u8>> {
    let m = mapping?;
    if m.code_to_uni.is_empty() {
        return None;
    }
    let cff = crate::cffwrap::parse(data)?;
    let upm_f = cff.upm.clamp(16.0, 16384.0);
    let upm = upm_f.round() as u16;
    let to_units = |w: f32| (w * upm_f / 1000.0).round().clamp(0.0, 65535.0) as u16;
    let code_gid = |code: u32| -> Option<u16> {
        let g = if cff.is_cid {
            cff.cid_to_gid.get(&code).copied()
        } else {
            cff.code_to_gid.get(&code).copied()
        }?;
        (g != 0 && g < cff.num_glyphs).then_some(g)
    };
    let default_w = to_units(if m.default_width > 0.0 { m.default_width } else { 500.0 });
    let mut advances = vec![default_w; cff.num_glyphs as usize];
    let mut pairs: Vec<(u32, u16)> = Vec::new();
    for (&code, &uni) in &m.code_to_uni {
        let Some(gid) = code_gid(code) else { continue };
        if uni != 0 && uni <= 0xFFFF {
            pairs.push((uni, gid));
        }
        if let Some(&w) = m.widths.get(&code) {
            advances[gid as usize] = to_units(w);
        }
    }
    if pairs.is_empty() {
        return None; // không map được glyph nào — bọc cũng vô dụng
    }
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);

    let asc = if m.ascent > 1.0 { (m.ascent * upm_f / 1000.0) as i16 } else { (upm_f * 0.8) as i16 };
    let desc =
        if m.descent < -1.0 { (m.descent * upm_f / 1000.0) as i16 } else { -((upm_f * 0.2) as i16) };
    let adv_max = advances.iter().copied().max().unwrap_or(default_w);

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"CFF ", data.to_vec()),
        (*b"cmap", build_cmap_format4(&pairs)),
        (*b"head", build_head(upm, desc, adv_max as i16, asc)),
        (*b"hhea", build_hhea(asc, desc, adv_max, cff.num_glyphs)),
        (*b"hmtx", build_hmtx(&advances)),
        (*b"maxp", build_maxp05(cff.num_glyphs)),
    ];
    let os2 = build_os2(&tables, bold, italic)?;
    tables.push((*b"OS/2", os2));
    tables.push((*b"name", build_name()));
    tables.push((*b"post", build_post()));
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    Some(assemble(TAG_OTTO, tables))
}

/// head 54B cho vỏ OTF (magic 0x5F0F3CF5 bắt buộc; checkSumAdjustment do
/// assemble điền).
fn build_head(upm: u16, y_min: i16, x_max: i16, y_max: i16) -> Vec<u8> {
    let mut v = vec![0u8; 54];
    v[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    v[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // fontRevision
    v[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    v[18..20].copy_from_slice(&upm.to_be_bytes());
    v[38..40].copy_from_slice(&y_min.to_be_bytes());
    v[40..42].copy_from_slice(&x_max.to_be_bytes());
    v[42..44].copy_from_slice(&y_max.to_be_bytes());
    v[46..48].copy_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    v[48..50].copy_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    v
}

fn build_hhea(asc: i16, desc: i16, adv_max: u16, num_h_metrics: u16) -> Vec<u8> {
    let mut v = vec![0u8; 36];
    v[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    v[4..6].copy_from_slice(&asc.to_be_bytes());
    v[6..8].copy_from_slice(&desc.to_be_bytes());
    v[10..12].copy_from_slice(&adv_max.to_be_bytes());
    v[18..20].copy_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    v[34..36].copy_from_slice(&num_h_metrics.to_be_bytes());
    v
}

/// hmtx đầy đủ: mỗi glyph advance(u16) + lsb(i16=0).
fn build_hmtx(advances: &[u16]) -> Vec<u8> {
    let mut v = Vec::with_capacity(advances.len() * 4);
    for &a in advances {
        v.extend_from_slice(&a.to_be_bytes());
        v.extend_from_slice(&0i16.to_be_bytes());
    }
    v
}

/// maxp version 0.5 (6B) — chuẩn cho font CFF (không có glyf/loca).
fn build_maxp05(num_glyphs: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(6);
    v.extend_from_slice(&0x0000_5000u32.to_be_bytes());
    v.extend_from_slice(&num_glyphs.to_be_bytes());
    v
}

const TAG_TRUE: u32 = u32::from_be_bytes(*b"true");
const TAG_OTTO: u32 = u32::from_be_bytes(*b"OTTO");

fn try_fix(
    data: &[u8],
    bold: bool,
    italic: bool,
    mapping: Option<&FontMapping>,
) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }
    let ver = u32::from_be_bytes(data[0..4].try_into().ok()?);
    if ver != 0x0001_0000 && ver != TAG_TRUE && ver != TAG_OTTO {
        return None; // không phải sfnt — CFF trần đi đường wrap_bare_cff
    }
    let num = u16::from_be_bytes(data[4..6].try_into().ok()?) as usize;
    if num == 0 || data.len() < 12 + num * 16 {
        return None;
    }
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(num + 4);
    let mut has_os2 = false;
    let mut has_name = false;
    let mut has_post = false;
    for i in 0..num {
        let o = 12 + i * 16;
        let tag: [u8; 4] = data[o..o + 4].try_into().ok()?;
        let off = u32::from_be_bytes(data[o + 8..o + 12].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(data[o + 12..o + 16].try_into().ok()?) as usize;
        if off.checked_add(len)? > data.len() {
            return None; // danh mục hỏng — trả nguyên trạng cho lành
        }
        match &tag {
            b"OS/2" => has_os2 = true,
            b"name" => has_name = true,
            b"post" => has_post = true,
            _ => {}
        }
        tables.push((tag, data[off..off + len].to_vec()));
    }
    let num_glyphs = read_num_glyphs(&tables);
    // cmap: (1,0) fmt 0/6 → chuyển sang (3,1) fmt 4; KHÔNG có cmap (font
    // CID/Identity-H của Word) → dựng mới từ /ToUnicode × CIDToGIDMap.
    let mut cmap_changed = false;
    match tables.iter().position(|(t, _)| t == b"cmap") {
        Some(ci) => {
            if let Some(converted) =
                convert_cmap(&tables[ci].1, mapping.map(|m| &m.code_to_uni))
            {
                tables[ci].1 = converted;
                cmap_changed = true;
            } else if !cmap_has_unicode_subtable(&tables[ci].1) {
                if let Some(cm) = mapping.and_then(|m| build_cid_cmap(m, num_glyphs)) {
                    tables[ci].1 = cm;
                    cmap_changed = true;
                }
            }
        }
        None => {
            if let Some(cm) = mapping.and_then(|m| build_cid_cmap(m, num_glyphs)) {
                tables.push((*b"cmap", cm));
                cmap_changed = true;
            }
        }
    }
    if has_os2 && has_name && has_post && ver == 0x0001_0000 && !cmap_changed {
        return None; // đã chuẩn sẵn — giữ nguyên bytes gốc
    }
    if !has_os2 {
        let os2 = build_os2(&tables, bold, italic)?;
        tables.push((*b"OS/2", os2));
    }
    // OTS cũng BẮT BUỘC name + post — subset CID hay bỏ cả hai.
    if !has_name {
        tables.push((*b"name", build_name()));
    }
    if !has_post {
        tables.push((*b"post", build_post()));
    }
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    Some(assemble(ver, tables))
}

/// numGlyphs từ maxp (chặn GID rác khi dựng cmap); 0 = không rõ, không chặn.
fn read_num_glyphs(tables: &[([u8; 4], Vec<u8>)]) -> u16 {
    tables
        .iter()
        .find(|(t, _)| t == b"maxp")
        .and_then(|(_, b)| b.get(4..6))
        .and_then(|s| s.try_into().ok())
        .map(u16::from_be_bytes)
        .unwrap_or(0)
}

/// cmap đã có subtable Unicode OTS chấp nhận chưa ((0,*), (3,1), (3,10))?
fn cmap_has_unicode_subtable(cmap: &[u8]) -> bool {
    let Some(n) = cmap.get(2..4).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes) else {
        return false;
    };
    for i in 0..n as usize {
        let o = 4 + i * 8;
        let Some(plat) = cmap.get(o..o + 2).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes)
        else {
            return false;
        };
        let Some(enc) =
            cmap.get(o + 2..o + 4).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes)
        else {
            return false;
        };
        if plat == 0 || (plat == 3 && (enc == 1 || enc == 10)) {
            return true;
        }
    }
    false
}

/// cmap (3,1) format 4 cho font CID/Identity-H: Unicode ← /ToUnicode(mã),
/// GID ← CIDToGIDMap(mã) (Identity = chính mã). Bỏ mapping ngoài BMP/GID rác.
fn build_cid_cmap(m: &FontMapping, num_glyphs: u16) -> Option<Vec<u8>> {
    if !m.is_cid || m.code_to_uni.is_empty() {
        return None;
    }
    let mut pairs: Vec<(u32, u16)> = Vec::new();
    for (&code, &uni) in &m.code_to_uni {
        if uni == 0 || uni > 0xFFFF {
            continue;
        }
        let gid = match &m.cid_to_gid {
            Some(map) => *map.get(code as usize).unwrap_or(&0),
            None => {
                if code > 0xFFFF {
                    continue;
                }
                code as u16
            }
        };
        if gid == 0 || (num_glyphs != 0 && gid >= num_glyphs) {
            continue;
        }
        pairs.push((uni, gid));
    }
    if pairs.is_empty() {
        return None;
    }
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);
    Some(build_cmap_format4(&pairs))
}

/// name tối thiểu (format 0, family + subfamily, Windows Unicode en-US) — OTS
/// bắt buộc có bảng name; nội dung không ảnh hưởng hiển thị (CSS đặt alias).
fn build_name() -> Vec<u8> {
    let entries: [(u16, &str); 2] = [(1, "FFX Embedded"), (2, "Regular")];
    let mut strings: Vec<u8> = Vec::new();
    let mut records: Vec<u8> = Vec::new();
    for (id, s) in entries {
        let start = strings.len() as u16;
        for u in s.encode_utf16() {
            strings.extend_from_slice(&u.to_be_bytes());
        }
        let len = strings.len() as u16 - start;
        records.extend_from_slice(&3u16.to_be_bytes()); // platform Windows
        records.extend_from_slice(&1u16.to_be_bytes()); // encoding Unicode BMP
        records.extend_from_slice(&0x0409u16.to_be_bytes()); // en-US
        records.extend_from_slice(&id.to_be_bytes());
        records.extend_from_slice(&len.to_be_bytes());
        records.extend_from_slice(&start.to_be_bytes());
    }
    let mut out = Vec::with_capacity(6 + records.len() + strings.len());
    out.extend_from_slice(&0u16.to_be_bytes()); // format 0
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    out.extend_from_slice(&((6 + records.len()) as u16).to_be_bytes()); // stringOffset
    out.extend_from_slice(&records);
    out.extend_from_slice(&strings);
    out
}

/// post version 3.0 (32 byte, không tên glyph) — đủ cho OTS.
fn build_post() -> Vec<u8> {
    let mut v = Vec::with_capacity(32);
    v.extend_from_slice(&0x0003_0000u32.to_be_bytes()); // version 3.0
    v.extend_from_slice(&0i32.to_be_bytes()); // italicAngle (Fixed)
    v.extend_from_slice(&(-100i16).to_be_bytes()); // underlinePosition
    v.extend_from_slice(&50i16.to_be_bytes()); // underlineThickness
    v.extend_from_slice(&0u32.to_be_bytes()); // isFixedPitch
    v.extend_from_slice(&[0u8; 16]); // min/max mem Type42/Type1
    v
}

/// MacRoman 0x80–0xFF → Unicode (bảng chuẩn Apple) — fallback cho mã cao khi
/// không có /ToUnicode.
const MACROMAN_HI: [u16; 128] = [
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4,
    0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8, 0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF,
    0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020,
    0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9, 0x2122, 0x00B4,
    0x00A8, 0x2260, 0x00C6, 0x00D8, 0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202,
    0x2211, 0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8, 0x00BF, 0x00A1,
    0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB, 0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3,
    0x00D5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA,
    0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02, 0x2021, 0x00B7, 0x201A,
    0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF, 0x00CC,
    0x00D3, 0x00D4, 0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF,
    0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
];

/// cmap chỉ có subtable OTS không hỗ trợ ((1,0) format 0/6 của subset PDF) →
/// dựng lại thành (3,1) format 4 theo Unicode. Đã có subtable Unicode/Windows
/// → None (giữ nguyên). Mã→Unicode: ưu tiên /ToUnicode; thiếu thì ASCII
/// identity (0x20–0x7E) + MacRoman (≥0x80); mã thấp không giải nghĩa được thì
/// bỏ — ký tự đó rơi xuống font kế trong stack CSS.
fn convert_cmap(cmap: &[u8], code_to_uni: Option<&HashMap<u32, u32>>) -> Option<Vec<u8>> {
    if cmap.len() < 4 {
        return None;
    }
    let n = u16::from_be_bytes(cmap[2..4].try_into().ok()?) as usize;
    if cmap.len() < 4 + n * 8 {
        return None;
    }
    let mut src: Option<(usize, u16)> = None;
    for i in 0..n {
        let o = 4 + i * 8;
        let plat = u16::from_be_bytes(cmap[o..o + 2].try_into().ok()?);
        let enc = u16::from_be_bytes(cmap[o + 2..o + 4].try_into().ok()?);
        let off = u32::from_be_bytes(cmap[o + 4..o + 8].try_into().ok()?) as usize;
        if off + 2 > cmap.len() {
            return None;
        }
        let fmt = u16::from_be_bytes(cmap[off..off + 2].try_into().ok()?);
        if plat == 0 || (plat == 3 && (enc == 1 || enc == 10)) {
            return None; // đã có subtable Unicode — OTS chấp nhận
        }
        if plat == 1 && enc == 0 && (fmt == 0 || fmt == 6) {
            src = Some((off, fmt));
        }
    }
    let (off, fmt) = src?;
    let mut pairs: Vec<(u32, u16)> = Vec::new();
    let mut push = |code: u32, glyph: u16| {
        if glyph == 0 {
            return;
        }
        let uni = match code_to_uni.and_then(|m| m.get(&code)) {
            Some(&u) => u,
            None => {
                if (0x20..=0x7E).contains(&code) {
                    code
                } else if (0x80..=0xFF).contains(&code) {
                    u32::from(MACROMAN_HI[(code - 0x80) as usize])
                } else {
                    return; // mã thấp không giải nghĩa được
                }
            }
        };
        if uni == 0 || uni > 0xFFFF {
            return; // format 4 chỉ chứa BMP
        }
        pairs.push((uni, glyph));
    };
    if fmt == 0 {
        if off + 6 + 256 > cmap.len() {
            return None;
        }
        for c in 0..256u32 {
            push(c, u16::from(cmap[off + 6 + c as usize]));
        }
    } else {
        if off + 10 > cmap.len() {
            return None;
        }
        let first = u32::from(u16::from_be_bytes(cmap[off + 6..off + 8].try_into().ok()?));
        let cnt = u16::from_be_bytes(cmap[off + 8..off + 10].try_into().ok()?) as usize;
        if off + 10 + cnt * 2 > cmap.len() {
            return None;
        }
        for k in 0..cnt {
            let g = u16::from_be_bytes(cmap[off + 10 + k * 2..off + 12 + k * 2].try_into().ok()?);
            push(first + k as u32, g);
        }
    }
    if pairs.is_empty() {
        return None;
    }
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);
    Some(build_cmap_format4(&pairs))
}

/// cmap trọn vẹn: 1 subtable (3,1) format 4 từ các cặp (unicode, glyph) đã
/// sort + khử trùng. Cặp liên tiếp cả 2 chiều gộp thành 1 segment.
fn build_cmap_format4(pairs: &[(u32, u16)]) -> Vec<u8> {
    struct Seg {
        start: u32,
        end: u32,
        g_start: u16,
        delta_fixed: Option<u16>,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for &(u, g) in pairs {
        if let Some(last) = segs.last_mut() {
            let next_g = last.g_start as u32 + (u - last.start);
            if u == last.end + 1 && next_g <= 0xFFFF && next_g == u32::from(g) {
                last.end = u;
                continue;
            }
        }
        segs.push(Seg { start: u, end: u, g_start: g, delta_fixed: None });
    }
    segs.push(Seg { start: 0xFFFF, end: 0xFFFF, g_start: 0, delta_fixed: Some(1) });

    let n = segs.len();
    let sub_len = 16 + n * 8;
    let mut pow = 1usize;
    let mut es = 0u16;
    while pow * 2 <= n {
        pow *= 2;
        es += 1;
    }
    let mut sub = vec![0u8; sub_len];
    let w16 = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_be_bytes());
    w16(&mut sub, 0, 4);
    w16(&mut sub, 2, sub_len as u16);
    w16(&mut sub, 4, 0); // language
    w16(&mut sub, 6, (n * 2) as u16);
    w16(&mut sub, 8, (pow * 2) as u16);
    w16(&mut sub, 10, es);
    w16(&mut sub, 12, (n * 2 - pow * 2) as u16);
    for (i, s) in segs.iter().enumerate() {
        w16(&mut sub, 14 + i * 2, s.end as u16); // endCode
        w16(&mut sub, 16 + n * 2 + i * 2, s.start as u16); // startCode (sau pad)
        let delta = s
            .delta_fixed
            .unwrap_or_else(|| (u32::from(s.g_start).wrapping_sub(s.start) & 0xFFFF) as u16);
        w16(&mut sub, 16 + n * 4 + i * 2, delta); // idDelta
        w16(&mut sub, 16 + n * 6 + i * 2, 0); // idRangeOffset
    }
    let mut cmap = vec![0u8; 12 + sub_len];
    w16(&mut cmap, 0, 0); // version
    w16(&mut cmap, 2, 1); // numTables
    w16(&mut cmap, 4, 3); // platformID Windows
    w16(&mut cmap, 6, 1); // encodingID Unicode BMP
    cmap[8..12].copy_from_slice(&12u32.to_be_bytes());
    cmap[12..].copy_from_slice(&sub);
    cmap
}

/// Tổng checksum kiểu sfnt: cộng u32 big-endian, phần dư pad 0.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        sum = sum.wrapping_add(u32::from_be_bytes(c.try_into().unwrap()));
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        let mut last = [0u8; 4];
        last[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(last));
    }
    sum
}

fn assemble(orig_ver: u32, tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    // 'true' → 0x00010000 (OTS chấp nhận cả hai nhưng chuẩn hoá cho chắc);
    // OTTO (CFF) giữ nguyên tag.
    let ver = if orig_ver == TAG_OTTO { orig_ver } else { 0x0001_0000 };
    let num = tables.len() as u16;
    let mut pow = 1u16;
    let mut entry_selector = 0u16;
    while pow * 2 <= num {
        pow *= 2;
        entry_selector += 1;
    }
    let search_range = pow * 16;
    let range_shift = num * 16 - search_range;

    let mut out = Vec::new();
    out.extend_from_slice(&ver.to_be_bytes());
    out.extend_from_slice(&num.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut off = 12 + tables.len() * 16;
    let mut dir: Vec<u8> = Vec::new();
    let mut bodies: Vec<u8> = Vec::new();
    let mut head_off = None;
    for (tag, mut body) in tables {
        if &tag == b"head" && body.len() >= 12 {
            // checkSumAdjustment phải = 0 khi tính checksum — điền lại ở cuối.
            body[8..12].fill(0);
            head_off = Some(off);
        }
        let cs = checksum(&body);
        dir.extend_from_slice(&tag);
        dir.extend_from_slice(&cs.to_be_bytes());
        dir.extend_from_slice(&(off as u32).to_be_bytes());
        dir.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let padded = (body.len() + 3) & !3;
        off += padded;
        body.resize(padded, 0);
        bodies.extend_from_slice(&body);
    }
    out.extend_from_slice(&dir);
    out.extend_from_slice(&bodies);
    if let Some(h) = head_off {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adj.to_be_bytes());
    }
    out
}

/// OS/2 version 4 (96 byte) với số liệu suy từ head/hhea — đủ cho OTS và cho
/// trình duyệt chọn đúng mặt đậm/nghiêng, không ảnh hưởng shaping.
fn build_os2(tables: &[([u8; 4], Vec<u8>)], bold: bool, italic: bool) -> Option<Vec<u8>> {
    let find = |tag: &[u8; 4]| tables.iter().find(|(t, _)| t == tag).map(|(_, b)| b.as_slice());
    let head = find(b"head")?;
    let upm = if head.len() >= 20 {
        i32::from(u16::from_be_bytes(head[18..20].try_into().ok()?))
    } else {
        1000
    };
    let (asc, desc, gap) = match find(b"hhea") {
        Some(h) if h.len() >= 10 => (
            i16::from_be_bytes(h[4..6].try_into().ok()?),
            i16::from_be_bytes(h[6..8].try_into().ok()?),
            i16::from_be_bytes(h[8..10].try_into().ok()?),
        ),
        _ => ((upm * 4 / 5) as i16, -((upm / 5) as i16), 0),
    };

    let mut v: Vec<u8> = Vec::with_capacity(96);
    fn u(v: &mut Vec<u8>, x: u16) { v.extend_from_slice(&x.to_be_bytes()); }
    fn i(v: &mut Vec<u8>, x: i16) { v.extend_from_slice(&x.to_be_bytes()); }
    fn d(v: &mut Vec<u8>, x: u32) { v.extend_from_slice(&x.to_be_bytes()); }

    u(&mut v, 4); // version
    i(&mut v, (upm / 2) as i16); // xAvgCharWidth (xấp xỉ — không dùng để shaping)
    u(&mut v, if bold { 700 } else { 400 }); // usWeightClass
    u(&mut v, 5); // usWidthClass = medium
    u(&mut v, 0); // fsType: không hạn chế nhúng
    let sub = (upm * 65 / 100) as i16;
    i(&mut v, sub); // ySubscriptXSize
    i(&mut v, sub); // ySubscriptYSize
    i(&mut v, 0); // ySubscriptXOffset
    i(&mut v, (upm * 14 / 100) as i16); // ySubscriptYOffset
    i(&mut v, sub); // ySuperscriptXSize
    i(&mut v, sub); // ySuperscriptYSize
    i(&mut v, 0); // ySuperscriptXOffset
    i(&mut v, (upm * 48 / 100) as i16); // ySuperscriptYOffset
    i(&mut v, (upm * 5 / 100) as i16); // yStrikeoutSize
    i(&mut v, (upm * 26 / 100) as i16); // yStrikeoutPosition
    i(&mut v, 0); // sFamilyClass
    v.extend_from_slice(&[0u8; 10]); // panose: any
    d(&mut v, 0); d(&mut v, 0); d(&mut v, 0); d(&mut v, 0); // ulUnicodeRange 1–4
    v.extend_from_slice(b"FFXT"); // achVendID
    let mut fs = 0u16;
    if italic { fs |= 0x0001; }
    if bold { fs |= 0x0020; }
    if fs == 0 { fs = 0x0040; } // REGULAR
    u(&mut v, fs); // fsSelection
    u(&mut v, 0x0020); // usFirstCharIndex
    u(&mut v, 0xFFFD); // usLastCharIndex
    i(&mut v, asc); // sTypoAscender
    i(&mut v, desc); // sTypoDescender
    i(&mut v, gap); // sTypoLineGap
    u(&mut v, asc.max(0) as u16); // usWinAscent
    u(&mut v, (-(desc.min(0))) as u16); // usWinDescent
    d(&mut v, 1); // ulCodePageRange1: Latin 1
    d(&mut v, 0); // ulCodePageRange2
    i(&mut v, (upm / 2) as i16); // sxHeight
    i(&mut v, (upm * 7 / 10) as i16); // sCapHeight
    u(&mut v, 0); // usDefaultChar
    u(&mut v, 0x0020); // usBreakChar
    u(&mut v, 0); // usMaxContext
    debug_assert_eq!(v.len(), 96);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sfnt giả tối thiểu: head (54B, upm=1000) + hhea (36B) + glyf giả,
    /// KHÔNG có OS/2 — đúng hình dạng subset PDF gây lỗi OTS.
    fn fake_subset() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes()); // ascender
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes()); // descender
        hhea[8..10].copy_from_slice(&90i16.to_be_bytes()); // lineGap
        let glyf = vec![1u8, 2, 3, 4, 5]; // lẻ 4 byte để thử pad
        let tables: Vec<([u8; 4], Vec<u8>)> =
            vec![(*b"glyf", glyf), (*b"head", head), (*b"hhea", hhea)];
        assemble(TAG_TRUE, tables)
    }

    fn table_dir(font: &[u8]) -> Vec<(String, usize, usize)> {
        let num = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
        (0..num)
            .map(|i| {
                let o = 12 + i * 16;
                (
                    String::from_utf8_lossy(&font[o..o + 4]).to_string(),
                    u32::from_be_bytes(font[o + 8..o + 12].try_into().unwrap()) as usize,
                    u32::from_be_bytes(font[o + 12..o + 16].try_into().unwrap()) as usize,
                )
            })
            .collect()
    }

    #[test]
    fn missing_os2_is_synthesized_with_valid_checksums() {
        let subset = fake_subset();
        let fixed = web_compatible_font(&subset, true, false, None);
        assert_ne!(fixed, subset, "phải được vá");
        // Tag chuẩn hoá + có OS/2 + name/post tổng hợp + danh mục sắp theo tag.
        assert_eq!(&fixed[0..4], &0x0001_0000u32.to_be_bytes());
        let dir = table_dir(&fixed);
        let tags: Vec<&str> = dir.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(
            tags,
            vec!["OS/2", "glyf", "head", "hhea", "name", "post"],
            "sắp theo tag: {tags:?}"
        );
        // OS/2 đọc lại đúng nội dung suy ra.
        let (_, off, len) = dir[0].clone();
        let os2 = &fixed[off..off + len];
        assert_eq!(len, 96);
        assert_eq!(u16::from_be_bytes(os2[4..6].try_into().unwrap()), 700, "usWeightClass bold");
        assert_eq!(u16::from_be_bytes(os2[62..64].try_into().unwrap()), 0x0020, "fsSelection BOLD");
        assert_eq!(
            i16::from_be_bytes(os2[68..70].try_into().unwrap()),
            800,
            "sTypoAscender từ hhea"
        );
        // Checksum toàn font phải ra hằng số ma thuật khi head.adjustment đã điền.
        assert_eq!(checksum(&fixed), 0xB1B0_AFBA, "checkSumAdjustment chuẩn");
        // Vá lần 2 phải là no-op (đã có OS/2 + tag chuẩn, không có cmap).
        assert_eq!(web_compatible_font(&fixed, true, false, None), fixed);
    }

    #[test]
    fn non_sfnt_data_passes_through() {
        let cff = vec![0x01, 0x00, 0x04, 0x04, 0xAA, 0xBB];
        assert_eq!(web_compatible_font(&cff, false, false, None), cff);
    }

    /// cmap (1,0) format 0 kiểu subset LibreOffice: mã ASCII giữ nguyên, mã
    /// thấp (1) là chữ có dấu chỉ /ToUnicode giải nghĩa được.
    fn fake_subset_with_cmap() -> Vec<u8> {
        let mut sub = vec![0u8; 6 + 256];
        sub[0..2].copy_from_slice(&0u16.to_be_bytes()); // format 0
        sub[2..4].copy_from_slice(&262u16.to_be_bytes());
        sub[6 + 0x41] = 5; // 'A' → glyph 5
        sub[6 + 0x42] = 6; // 'B' → glyph 6 (liên tiếp → gộp segment)
        sub[6 + 1] = 7; // mã 1 → glyph 7 (chữ 'ả' qua ToUnicode)
        let mut cmap = vec![0u8; 12];
        cmap[2..4].copy_from_slice(&1u16.to_be_bytes()); // numTables
        cmap[4..6].copy_from_slice(&1u16.to_be_bytes()); // platform Mac
        cmap[6..8].copy_from_slice(&0u16.to_be_bytes()); // encoding 0
        cmap[8..12].copy_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&sub);

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let tables: Vec<([u8; 4], Vec<u8>)> = vec![(*b"cmap", cmap), (*b"head", head)];
        assemble(TAG_TRUE, tables)
    }

    /// Tra format 4: quét segment tuyến tính.
    fn lookup_f4(cmap: &[u8], uni: u16) -> u16 {
        assert_eq!(u16::from_be_bytes(cmap[4..6].try_into().unwrap()), 3, "platform Windows");
        assert_eq!(u16::from_be_bytes(cmap[6..8].try_into().unwrap()), 1, "encoding BMP");
        let off = u32::from_be_bytes(cmap[8..12].try_into().unwrap()) as usize;
        assert_eq!(u16::from_be_bytes(cmap[off..off + 2].try_into().unwrap()), 4, "format 4");
        let n = (u16::from_be_bytes(cmap[off + 6..off + 8].try_into().unwrap()) / 2) as usize;
        for i in 0..n {
            let end = u16::from_be_bytes(cmap[off + 14 + i * 2..off + 16 + i * 2].try_into().unwrap());
            let start =
                u16::from_be_bytes(cmap[off + 16 + n * 2 + i * 2..off + 18 + n * 2 + i * 2].try_into().unwrap());
            if uni >= start && uni <= end {
                let delta =
                    u16::from_be_bytes(cmap[off + 16 + n * 4 + i * 2..off + 18 + n * 4 + i * 2].try_into().unwrap());
                return uni.wrapping_add(delta);
            }
        }
        0
    }

    fn mapping(uni: HashMap<u32, u32>, is_cid: bool, cid_to_gid: Option<Vec<u16>>) -> FontMapping {
        FontMapping {
            code_to_uni: uni,
            is_cid,
            cid_to_gid,
            widths: HashMap::new(),
            default_width: 1000.0,
            ascent: 0.0,
            descent: 0.0,
        }
    }

    #[test]
    fn unsupported_cmap_is_converted_to_windows_format4() {
        let subset = fake_subset_with_cmap();
        let mut uni = HashMap::new();
        uni.insert(1u32, 0x1EA3u32); // mã 1 = 'ả'
        let m = mapping(uni, false, None);
        let fixed = web_compatible_font(&subset, false, false, Some(&m));
        assert_ne!(fixed, subset);
        // Tìm bảng cmap mới.
        let num = u16::from_be_bytes(fixed[4..6].try_into().unwrap()) as usize;
        let mut cm = None;
        for i in 0..num {
            let o = 12 + i * 16;
            if &fixed[o..o + 4] == b"cmap" {
                let off = u32::from_be_bytes(fixed[o + 8..o + 12].try_into().unwrap()) as usize;
                let len = u32::from_be_bytes(fixed[o + 12..o + 16].try_into().unwrap()) as usize;
                cm = Some(fixed[off..off + len].to_vec());
            }
        }
        let cm = cm.expect("có cmap");
        assert_eq!(lookup_f4(&cm, 0x41), 5, "'A' giữ glyph cũ");
        assert_eq!(lookup_f4(&cm, 0x42), 6, "'B' gộp segment vẫn đúng");
        assert_eq!(lookup_f4(&cm, 0x1EA3), 7, "'ả' theo ToUnicode");
        assert_eq!(lookup_f4(&cm, 0x43), 0, "mã không map → notdef");
        // OS/2 cũng được thêm (fake subset không có).
        let tags: Vec<String> = (0..u16::from_be_bytes(fixed[4..6].try_into().unwrap()) as usize)
            .map(|i| String::from_utf8_lossy(&fixed[12 + i * 16..16 + i * 16]).to_string())
            .collect();
        assert!(tags.contains(&"OS/2".to_string()), "tags: {tags:?}");
    }

    /// Font CID/Identity-H kiểu Word: KHÔNG có cmap, mã = CID, GID = CID
    /// (Identity) hoặc qua bảng CIDToGIDMap → dựng cmap (3,1) fmt 4 mới.
    fn fake_cid_subset() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&2048u16.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&1638i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-410i16).to_be_bytes());
        let mut maxp = vec![0u8; 32];
        maxp[4..6].copy_from_slice(&50u16.to_be_bytes()); // numGlyphs = 50
        let tables: Vec<([u8; 4], Vec<u8>)> =
            vec![(*b"glyf", vec![0u8; 8]), (*b"head", head), (*b"hhea", hhea), (*b"maxp", maxp)];
        assemble(0x0001_0000, tables)
    }

    #[test]
    fn cid_font_without_cmap_gets_synthesized_cmap() {
        let subset = fake_cid_subset();
        let mut uni = HashMap::new();
        uni.insert(5u32, 0x1EA3u32); // CID 5 = 'ả'
        uni.insert(6u32, 0x0041u32); // CID 6 = 'A'
        uni.insert(99u32, 0x0042u32); // CID 99 ≥ numGlyphs → phải bị bỏ
        // Identity: GID = CID.
        let m = mapping(uni.clone(), true, None);
        let fixed = web_compatible_font(&subset, false, false, Some(&m));
        assert_ne!(fixed, subset, "phải được vá");
        let dir = table_dir(&fixed);
        let tags: Vec<&str> = dir.iter().map(|(t, _, _)| t.as_str()).collect();
        for need in ["cmap", "OS/2", "name", "post"] {
            assert!(tags.contains(&need), "thiếu {need}: {tags:?}");
        }
        let (_, off, len) = dir[tags.iter().position(|t| *t == "cmap").unwrap()].clone();
        let cm = fixed[off..off + len].to_vec();
        assert_eq!(lookup_f4(&cm, 0x1EA3), 5, "'ả' → GID 5 (Identity)");
        assert_eq!(lookup_f4(&cm, 0x41), 6, "'A' → GID 6");
        assert_eq!(lookup_f4(&cm, 0x42), 0, "GID ngoài numGlyphs bị bỏ");

        // Có bảng CIDToGIDMap: CID 5 → GID 7.
        let mut c2g = vec![0u16; 10];
        c2g[5] = 7;
        c2g[6] = 8;
        let m2 = mapping(uni, true, Some(c2g));
        let fixed2 = web_compatible_font(&subset, false, false, Some(&m2));
        let dir2 = table_dir(&fixed2);
        let tags2: Vec<&str> = dir2.iter().map(|(t, _, _)| t.as_str()).collect();
        let (_, o2, l2) = dir2[tags2.iter().position(|t| *t == "cmap").unwrap()].clone();
        let cm2 = fixed2[o2..o2 + l2].to_vec();
        assert_eq!(lookup_f4(&cm2, 0x1EA3), 7, "'ả' → GID 7 theo CIDToGIDMap");
        assert_eq!(lookup_f4(&cm2, 0x41), 8);
    }

    /// CFF trần (Type1C): bọc thành OTF — bảng CFF giữ nguyên bytes, cmap từ
    /// Encoding × ToUnicode, hmtx đúng /Widths của PDF.
    #[test]
    fn bare_cff_is_wrapped_into_otf() {
        let cff = crate::cffwrap::fake_cff();
        let mut uni = HashMap::new();
        uni.insert(65u32, 65u32); // 'A' giữ mã
        uni.insert(66u32, 0x1EA3u32); // mã 66 = 'ả' qua ToUnicode
        let mut m = mapping(uni, false, None);
        m.widths.insert(65, 600.0);
        m.widths.insert(66, 480.0);
        m.ascent = 800.0;
        m.descent = -200.0;
        let out = web_compatible_font(&cff, false, false, Some(&m));
        assert_ne!(out, cff, "phải được bọc");
        assert_eq!(&out[0..4], b"OTTO", "vỏ OTF cho CFF");
        let dir = table_dir(&out);
        let tags: Vec<&str> = dir.iter().map(|(t, _, _)| t.as_str()).collect();
        for need in ["CFF ", "OS/2", "cmap", "head", "hhea", "hmtx", "maxp", "name", "post"] {
            assert!(tags.contains(&need), "thiếu {need}: {tags:?}");
        }
        let (_, off, len) = dir[tags.iter().position(|t| *t == "cmap").unwrap()].clone();
        let cm = out[off..off + len].to_vec();
        assert_eq!(lookup_f4(&cm, 65), 1, "'A' → gid 1 theo Encoding");
        assert_eq!(lookup_f4(&cm, 0x1EA3), 2, "'ả' → gid 2");
        let (_, ho, _) = dir[tags.iter().position(|t| *t == "hmtx").unwrap()].clone();
        assert_eq!(
            u16::from_be_bytes(out[ho + 4..ho + 6].try_into().unwrap()),
            600,
            "advance gid 1 theo /Widths"
        );
        assert_eq!(u16::from_be_bytes(out[ho + 8..ho + 10].try_into().unwrap()), 480);
        let (_, co, cl) = dir[tags.iter().position(|t| *t == "CFF ").unwrap()].clone();
        assert_eq!(&out[co..co + cl], &cff[..], "bảng CFF giữ nguyên bytes gốc");
        assert_eq!(checksum(&out), 0xB1B0_AFBA);
        // Không có mapping → không bọc nổi, trả nguyên trạng (UI tự fallback).
        assert_eq!(web_compatible_font(&cff, false, false, None), cff);
    }
}
