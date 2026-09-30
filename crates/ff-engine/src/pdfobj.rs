//! Tiện ích tầng PDF object (lopdf) dùng chung cho sanitize / đính kèm /
//! thuộc tính / trợ năng: giải mã/mã hoá text string, deref, name tree,
//! đọc/ghi stream, ngày PDF.

use std::collections::BTreeSet;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::EngineError;

pub(crate) fn lerr<E: std::fmt::Display>(ctx: &'static str) -> impl Fn(E) -> EngineError {
    move |e| EngineError::Pdfium(format!("{ctx}: {e}"))
}

/// Mở tài liệu bằng lopdf; file có trailer phi chuẩn → chuẩn hoá qua qpdf
/// (file tạm) rồi mở lại.
pub(crate) fn load(input: &Path) -> Result<Document, EngineError> {
    match Document::load(input) {
        Ok(d) => Ok(d),
        Err(first) => {
            let norm = std::env::temp_dir().join(format!(
                "ff_lopdf_norm_{}_{}.pdf",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            if crate::qpdf::repair(input, &norm).is_err() {
                return Err(EngineError::Pdfium(format!("không đọc được cấu trúc PDF: {first}")));
            }
            let r = Document::load(&norm).map_err(lerr("lopdf load"));
            let _ = std::fs::remove_file(&norm);
            r
        }
    }
}

pub(crate) fn save(doc: &mut Document, output: &Path) -> Result<(), EngineError> {
    doc.save(output).map_err(lerr("lopdf save"))?;
    Ok(())
}

/// Theo tham chiếu (nhiều tầng) tới object thật.
pub(crate) fn deref<'a>(doc: &'a Document, mut o: &'a Object) -> &'a Object {
    for _ in 0..16 {
        match o {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(t) => o = t,
                Err(_) => return &Object::Null,
            },
            _ => return o,
        }
    }
    o
}

pub(crate) fn dict_get<'a>(doc: &'a Document, d: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    d.get(key).ok().map(|o| deref(doc, o)).filter(|o| !matches!(o, Object::Null))
}

pub(crate) fn dict_of<'a>(doc: &'a Document, d: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    match dict_get(doc, d, key)? {
        Object::Dictionary(x) => Some(x),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub(crate) fn name_of(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<String> {
    match dict_get(doc, d, key)? {
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

pub(crate) fn obj_dict<'a>(o: &'a Object) -> Option<&'a Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub(crate) fn obj_dict_mut(o: &mut Object) -> Option<&mut Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&mut s.dict),
        _ => None,
    }
}

/// Giải mã text string PDF: UTF-16BE (BOM FE FF), UTF-8 (BOM EF BB BF) hoặc
/// PDFDocEncoding (≈ Latin-1 cho phần in được).
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16_lossy(&u)
    } else if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        bytes.iter().map(|&b| pdfdoc_char(b)).collect()
    }
}

/// PDFDocEncoding: khác Latin-1 ở dải 0x80–0x9F (dấu câu kiểu chữ).
fn pdfdoc_char(b: u8) -> char {
    const HI: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚', '™', 'ﬁ',
        'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\u{FFFD}',
    ];
    if (0x80..=0x9F).contains(&b) {
        HI[(b - 0x80) as usize]
    } else if b == 0xA0 {
        '€'
    } else {
        b as char
    }
}

pub(crate) fn text_of(doc: &Document, o: &Object) -> Option<String> {
    match deref(doc, o) {
        Object::String(b, _) => Some(decode_text(b)),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

pub(crate) fn dict_text(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<String> {
    d.get(key).ok().and_then(|o| text_of(doc, o))
}

/// Mã hoá text string: ASCII → literal; còn lại UTF-16BE có BOM (hex).
pub(crate) fn text_obj(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(b, StringFormat::Hexadecimal)
    }
}

/// Nội dung đã giải nén của stream (lỗi giải nén → bytes thô).
pub(crate) fn stream_data(s: &lopdf::Stream) -> Vec<u8> {
    if s.dict.get(b"Filter").is_ok() {
        s.decompressed_content().unwrap_or_else(|_| s.content.clone())
    } else {
        s.content.clone()
    }
}

/// ID catalog.
pub(crate) fn root_id(doc: &Document) -> Option<ObjectId> {
    doc.trailer.get(b"Root").ok().and_then(|o| o.as_reference().ok())
}

pub(crate) fn info_dict(doc: &Document) -> Option<&Dictionary> {
    let o = doc.trailer.get(b"Info").ok()?;
    obj_dict(deref(doc, o))
}

/// Ngày PDF "D:YYYYMMDDHHmmSSOHH'mm'" → ISO 8601 (thiếu phần nào bỏ qua).
pub(crate) fn pdf_date_to_iso(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s.strip_prefix("D:").unwrap_or(s);
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |a: usize, b: usize, def: &str| digits.get(a..b).map(|x| x.to_string()).unwrap_or_else(|| def.into());
    let (y, mo, d, h, mi, se) = (part(0, 4, ""), part(4, 6, "01"), part(6, 8, "01"), part(8, 10, "00"), part(10, 12, "00"), part(12, 14, "00"));
    let rest = &s[digits.len()..];
    let tz = match rest.chars().next() {
        Some('Z') => "Z".to_string(),
        Some(c @ ('+' | '-')) => {
            let t: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            let hh = t.get(0..2).unwrap_or("00");
            let mm = t.get(2..4).unwrap_or("00");
            format!("{c}{hh}:{mm}")
        }
        _ => String::new(),
    };
    Some(format!("{y}-{mo}-{d}T{h}:{mi}:{se}{tz}"))
}

/// Thời điểm hiện tại dạng ngày PDF (UTC).
pub(crate) fn pdf_date_now() -> String {
    let (y, mo, d, h, mi, s) = civil_now();
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

/// Thời điểm hiện tại dạng ISO (XMP).
pub(crate) fn iso_now() -> String {
    let (y, mo, d, h, mi, s) = civil_now();
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

pub(crate) fn system_time_to_pdf(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_secs(secs);
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

fn civil_now() -> (i64, u32, u32, u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_secs(secs)
}

/// Giây Unix → (năm, tháng, ngày, giờ, phút, giây) UTC (thuật toán Howard Hinnant).
fn civil_from_secs(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, (tod / 3600) as u32, ((tod % 3600) / 60) as u32, (tod % 60) as u32)
}

/// Làm phẳng name tree (/Names + /Kids) thành danh sách (key bytes, value).
pub(crate) fn name_tree_entries(doc: &Document, node: &Dictionary) -> Vec<(Vec<u8>, Object)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    walk_name_tree(doc, node, &mut out, &mut seen, 0);
    out
}

fn walk_name_tree(
    doc: &Document,
    node: &Dictionary,
    out: &mut Vec<(Vec<u8>, Object)>,
    seen: &mut BTreeSet<ObjectId>,
    depth: u32,
) {
    if depth > 32 {
        return;
    }
    if let Some(Object::Array(names)) = dict_get(doc, node, b"Names") {
        let mut it = names.iter();
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            if let Object::String(kb, _) = deref(doc, k) {
                out.push((kb.clone(), v.clone()));
            }
        }
    }
    if let Some(Object::Array(kids)) = dict_get(doc, node, b"Kids") {
        for k in kids {
            if let Object::Reference(id) = k {
                if !seen.insert(*id) {
                    continue;
                }
            }
            if let Some(d) = obj_dict(deref(doc, k)) {
                walk_name_tree(doc, d, out, seen, depth + 1);
            }
        }
    }
}

/// Mọi page object id theo thứ tự trang.
pub(crate) fn page_ids(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().values().copied().collect()
}

/// Mảng /Annots của trang (đã deref), kèm ID từng annot (nếu là tham chiếu).
pub(crate) fn page_annots(doc: &Document, page_id: ObjectId) -> Vec<(Option<ObjectId>, Dictionary)> {
    let Ok(page) = doc.get_dictionary(page_id) else { return Vec::new() };
    let Some(Object::Array(arr)) = dict_get(doc, page, b"Annots") else { return Vec::new() };
    arr.iter()
        .filter_map(|o| {
            let id = o.as_reference().ok();
            obj_dict(deref(doc, o)).map(|d| (id, d.clone()))
        })
        .collect()
}

/// Rect của annot [l,b,r,t] (chuẩn hoá thứ tự).
pub(crate) fn rect_of(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<[f32; 4]> {
    let Some(Object::Array(a)) = dict_get(doc, d, key) else { return None };
    if a.len() < 4 {
        return None;
    }
    let v: Vec<f32> = a.iter().take(4).filter_map(|o| num(deref(doc, o))).collect();
    if v.len() < 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

pub(crate) fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Hàm ghi đè /Annots của trang bằng danh sách mới (mảng trực tiếp).
pub(crate) fn set_page_annots(doc: &mut Document, page_id: ObjectId, annots: Vec<Object>) {
    if let Ok(page) = doc.get_dictionary_mut(page_id) {
        if annots.is_empty() {
            page.remove(b"Annots");
        } else {
            page.set("Annots", Object::Array(annots));
        }
    }
}

/// Mảng /Annots thô (chưa deref) của trang.
pub(crate) fn page_annots_raw(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    let Ok(page) = doc.get_dictionary(page_id) else { return Vec::new() };
    match dict_get(doc, page, b"Annots") {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}
