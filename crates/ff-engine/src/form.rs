//! Form AcroForm (Phase 6): liệt kê / điền / tạo field, flatten, và
//! export/import FDF. Thao tác tầng PDF object qua `lopdf` cho phần đọc/ghi
//! cấu trúc (đáng tin & portable), dùng PDFium chỉ để flatten.
//!
//! Kiểu field (khoá `/FT`):
//! - `/Tx`  — text
//! - `/Btn` — nút: checkbox / radio / push-button (phân biệt bằng cờ `/Ff`)
//! - `/Ch`  — choice: combo-box / list-box
//! - `/Sig` — chữ ký (xử lý ở `sign.rs`)
//!
//! Điền field: đặt `/V`; với checkbox/radio đặt cả `/AS` của widget về đúng
//! tên trạng thái bật; bật `NeedAppearances=true` để viewer tự dựng lại
//! appearance (cách portable nhất, mọi viewer hiểu).

use std::collections::BTreeMap;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::EngineError;

fn le<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("form {ctx}: {e}"))
}

/// Loại field rút gọn cho UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FieldKind {
    #[default]
    Text,
    Checkbox,
    Radio,
    Combo,
    List,
    Button,
    Signature,
    Unknown,
}

impl FieldKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Checkbox => "checkbox",
            FieldKind::Radio => "radio",
            FieldKind::Combo => "combo",
            FieldKind::List => "list",
            FieldKind::Button => "button",
            FieldKind::Signature => "signature",
            FieldKind::Unknown => "unknown",
        }
    }
}

/// Thông tin 1 field cho UI.
#[derive(Clone, Debug)]
pub struct FormField {
    /// Tên đầy đủ (fully-qualified) của field.
    pub name: String,
    pub kind: FieldKind,
    /// Giá trị hiện tại (text; hoặc tên trạng thái bật của checkbox/radio).
    pub value: Option<String>,
    /// Trang chứa widget đầu tiên (0-based) nếu xác định được.
    pub page_index: Option<u16>,
    /// Khung widget đầu tiên [left, bottom, right, top] (điểm PDF).
    pub rect: Option<[f32; 4]>,
    /// Với checkbox/radio: tên trạng thái BẬT (để đặt /V và /AS khi tick).
    pub on_state: Option<String>,
    /// Với combo/list: các lựa chọn.
    pub options: Vec<String>,
    pub read_only: bool,
}

pub(crate) fn bit(flags: i64, i: u32) -> bool {
    flags & (1 << (i - 1)) != 0
}

/// Bản đồ ObjectId của widget → chỉ số trang (0-based), dựng từ /Annots mỗi trang.
pub(crate) fn widget_page_map(doc: &Document) -> BTreeMap<ObjectId, u16> {
    let mut map = BTreeMap::new();
    for (page_no, page_id) in doc.get_pages() {
        if let Ok(page) = doc.get_object(page_id).and_then(Object::as_dict) {
            if let Ok(annots) = page.get(b"Annots").and_then(Object::as_array) {
                for a in annots {
                    if let Ok(id) = a.as_reference() {
                        map.insert(id, (page_no - 1) as u16);
                    }
                }
            }
        }
    }
    map
}

pub(crate) fn text_of(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes, _) => Some(decode_pdf_text(bytes)),
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

/// Giải mã chuỗi text PDF: UTF-16BE (BOM FE FF) hoặc PDFDocEncoding≈Latin-1.
pub(crate) fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Mã hoá chuỗi thành text PDF: ASCII → literal; có ký tự ngoài Latin-1 →
/// UTF-16BE (để tiếng Việt/CJK đúng).
pub(crate) fn encode_pdf_text(s: &str) -> Object {
    if s.chars().all(|c| (c as u32) < 128) {
        Object::string_literal(s.to_string())
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(bytes, lopdf::StringFormat::Hexadecimal)
    }
}

/// Kiểu + trạng thái bật (on-state) của 1 field, suy từ /FT, /Ff và các /AP.
fn classify(doc: &Document, dict: &Dictionary) -> (FieldKind, Option<String>) {
    let ft = dict.get(b"FT").and_then(Object::as_name).ok().map(|n| n.to_vec());
    let ft = ft.as_deref();
    let flags = dict.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
    match ft {
        Some(b"Tx") => (FieldKind::Text, None),
        Some(b"Btn") => {
            if bit(flags, 17) {
                (FieldKind::Button, None) // pushbutton
            } else if bit(flags, 16) {
                (FieldKind::Radio, on_state(doc, dict))
            } else {
                (FieldKind::Checkbox, on_state(doc, dict))
            }
        }
        Some(b"Ch") => {
            if bit(flags, 18) {
                (FieldKind::Combo, None)
            } else {
                (FieldKind::List, None)
            }
        }
        Some(b"Sig") => (FieldKind::Signature, None),
        _ => (FieldKind::Unknown, None),
    }
}

/// Tên trạng thái BẬT của checkbox/radio: khoá khác "Off" trong /AP /N.
pub(crate) fn on_state(doc: &Document, dict: &Dictionary) -> Option<String> {
    // Widget có thể là chính field, hoặc nằm trong /Kids.
    let widget = if dict.has(b"AP") {
        Some(dict)
    } else {
        dict.get(b"Kids")
            .and_then(Object::as_array)
            .ok()
            .and_then(|kids| kids.first())
            .and_then(|k| k.as_reference().ok())
            .and_then(|id| doc.get_object(id).and_then(Object::as_dict).ok())
    }?;
    let ap = widget.get(b"AP").and_then(Object::as_dict).ok()?;
    let n = ap.get(b"N").and_then(Object::as_dict).ok()?;
    n.iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .find(|k| k != "Off")
}

fn field_rect(doc: &Document, dict: &Dictionary) -> Option<[f32; 4]> {
    let src = if dict.has(b"Rect") {
        Some(dict)
    } else {
        dict.get(b"Kids")
            .and_then(Object::as_array)
            .ok()
            .and_then(|kids| kids.first())
            .and_then(|k| k.as_reference().ok())
            .and_then(|id| doc.get_object(id).and_then(Object::as_dict).ok())
    }?;
    let arr = src.get(b"Rect").and_then(Object::as_array).ok()?;
    if arr.len() != 4 {
        return None;
    }
    let v: Vec<f32> = arr.iter().filter_map(|o| o.as_float().ok().or_else(|| o.as_i64().ok().map(|i| i as f32))).collect();
    if v.len() != 4 {
        return None;
    }
    // Chuẩn hoá left<right, bottom<top.
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

fn field_page(_doc: &Document, dict: &Dictionary, wmap: &BTreeMap<ObjectId, u16>, self_id: ObjectId) -> Option<u16> {
    // Nếu field TỰ là widget (có trong wmap).
    if let Some(p) = wmap.get(&self_id) {
        return Some(*p);
    }
    // Widget con.
    let kids = dict.get(b"Kids").and_then(Object::as_array).ok()?;
    for k in kids {
        if let Ok(id) = k.as_reference() {
            if let Some(p) = wmap.get(&id) {
                return Some(*p);
            }
        }
    }
    None
}

/// Duyệt cây field (đệ quy /Kids là field con — phân biệt widget con bằng việc
/// KHÔNG có /FT riêng và có /Rect). Trả (ObjectId, tên đầy đủ, dict).
fn collect_fields(doc: &Document) -> Vec<(ObjectId, String, Dictionary)> {
    fn walk(
        doc: &Document,
        id: ObjectId,
        prefix: &str,
        out: &mut Vec<(ObjectId, String, Dictionary)>,
    ) {
        let Ok(dict) = doc.get_object(id).and_then(Object::as_dict) else { return };
        let partial = dict.get(b"T").ok().and_then(text_of);
        let full = match &partial {
            Some(t) if prefix.is_empty() => t.clone(),
            Some(t) => format!("{prefix}.{t}"),
            None => prefix.to_string(),
        };
        // Kids là FIELD con khi phần tử có /T (tên riêng); widget con thì không.
        let kid_fields: Vec<ObjectId> = dict
            .get(b"Kids")
            .and_then(Object::as_array)
            .ok()
            .map(|kids| {
                kids.iter()
                    .filter_map(|k| k.as_reference().ok())
                    .filter(|kid| {
                        doc.get_object(*kid)
                            .and_then(Object::as_dict)
                            .map(|d| d.has(b"T"))
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default();

        if kid_fields.is_empty() {
            // Field lá (hoặc field có widget con không tên).
            if partial.is_some() || dict.has(b"FT") {
                out.push((id, full.clone(), dict.clone()));
            }
        } else {
            for kid in kid_fields {
                walk(doc, kid, &full, out);
            }
        }
    }

    let mut out = Vec::new();
    if let Some(fields) = acroform_fields(doc) {
        for f in fields {
            if let Ok(id) = f.as_reference() {
                walk(doc, id, "", &mut out);
            }
        }
    }
    out
}

fn acroform_fields(doc: &Document) -> Option<Vec<Object>> {
    let acro = doc.catalog().ok()?.get(b"AcroForm").ok()?;
    let acro = match acro {
        Object::Reference(id) => doc.get_object(*id).and_then(Object::as_dict).ok()?,
        Object::Dictionary(d) => d,
        _ => return None,
    };
    acro.get(b"Fields").and_then(Object::as_array).ok().cloned()
}

/// Kế thừa /FT từ cha nếu field lá không tự khai (radio/checkbox thường vậy).
pub(crate) fn effective_ft(doc: &Document, dict: &Dictionary) -> Option<Vec<u8>> {
    if let Ok(ft) = dict.get(b"FT").and_then(Object::as_name) {
        return Some(ft.to_vec());
    }
    let mut parent = dict.get(b"Parent").and_then(Object::as_reference).ok();
    while let Some(pid) = parent {
        let p = doc.get_object(pid).and_then(Object::as_dict).ok()?;
        if let Ok(ft) = p.get(b"FT").and_then(Object::as_name) {
            return Some(ft.to_vec());
        }
        parent = p.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

/// Liệt kê mọi field của form.
pub fn list_form_fields(input: &Path) -> Result<Vec<FormField>, EngineError> {
    let doc = Document::load(input).map_err(le("load"))?;
    let wmap = widget_page_map(&doc);
    let mut out = Vec::new();
    for (id, name, mut dict) in collect_fields(&doc) {
        // Bổ sung /FT kế thừa để classify đúng.
        if !dict.has(b"FT") {
            if let Some(ft) = effective_ft(&doc, &dict) {
                dict.set("FT", Object::Name(ft));
            }
        }
        let (kind, on) = classify(&doc, &dict);
        let value = dict.get(b"V").ok().and_then(text_of);
        let options = choice_options(&dict);
        let read_only = bit(dict.get(b"Ff").and_then(Object::as_i64).unwrap_or(0), 1);
        out.push(FormField {
            name,
            kind,
            value,
            page_index: field_page(&doc, &dict, &wmap, id),
            rect: field_rect(&doc, &dict),
            on_state: on,
            options,
            read_only,
        });
    }
    Ok(out)
}

fn choice_options(dict: &Dictionary) -> Vec<String> {
    let Ok(opt) = dict.get(b"Opt").and_then(Object::as_array) else { return Vec::new() };
    opt.iter()
        .filter_map(|o| match o {
            Object::Array(pair) => pair.get(1).and_then(text_of).or_else(|| pair.first().and_then(text_of)),
            other => text_of(other),
        })
        .collect()
}

/// Cặp (tên field, giá trị) để điền.
#[derive(Clone, Debug)]
pub struct FieldValue {
    pub name: String,
    pub value: String,
}

/// Điền các field theo tên. Với checkbox/radio: value="on"/"true"/"yes"/"1" →
/// bật (dùng on-state của field); "off"/rỗng → tắt. Text/combo/list: đặt /V.
pub fn fill_form_fields(input: &Path, values: &[FieldValue], output: &Path) -> Result<usize, EngineError> {
    // Điền kèm dựng lại appearance (formx) — hiển thị đúng mọi viewer, tiếng Việt
    // dùng font nhúng thay vì trông vào NeedAppearances.
    let vals: Vec<crate::formx::FillValue> = values
        .iter()
        .map(|v| crate::formx::FillValue { name: v.name.clone(), value: v.value.clone(), values: vec![] })
        .collect();
    crate::formx::fill_form(input, &vals, output)
}

#[allow(dead_code)]
/// Đặt /V của field + /AS của mọi widget (field hoặc /Kids) về `state`.
fn set_button_state(doc: &mut Document, id: ObjectId, dict: &Dictionary, state: &str) {
    let state_name = Object::Name(state.as_bytes().to_vec());
    // /V trên field.
    if let Ok(f) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
        f.set("V", state_name.clone());
    }
    // /AS trên widget: field tự là widget?
    let widget_ids: Vec<ObjectId> = if dict.has(b"AP") || dict.has(b"Rect") {
        vec![id]
    } else {
        dict.get(b"Kids")
            .and_then(Object::as_array)
            .ok()
            .map(|kids| kids.iter().filter_map(|k| k.as_reference().ok()).collect())
            .unwrap_or_default()
    };
    for wid in widget_ids {
        // /AS chỉ đặt tên trạng thái nếu widget có appearance tương ứng, ngược
        // lại vẫn đặt (viewer tự xử lý qua NeedAppearances).
        if let Ok(w) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
            w.set("AS", Object::Name(state.as_bytes().to_vec()));
        }
    }
}

#[allow(dead_code)]
fn set_need_appearances(doc: &mut Document, need: bool) {
    let acro_id = doc.catalog().ok().and_then(|c| c.get(b"AcroForm").ok()).and_then(|o| o.as_reference().ok());
    if let Some(id) = acro_id {
        if let Ok(acro) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
            acro.set("NeedAppearances", Object::Boolean(need));
        }
    }
}

/// Flatten form: "in" giá trị field vào nội dung trang, bỏ tính tương tác.
/// Dùng flatten của PDFium (cần appearance đã dựng → chạy sau khi điền + qpdf).
pub fn flatten_form(
    pdfium: &pdfium_render::prelude::Pdfium,
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(format!("form flatten load: {e}")))?;
    for (i, mut page) in document.pages().iter().enumerate() {
        page.flatten()
            .map_err(|e| EngineError::Pdfium(format!("flatten trang {i}: {e}")))?;
    }
    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("form flatten save: {e}")))?;
    Ok(())
}

// ---- FDF export / import ----

/// Xuất giá trị field ra FDF (Forms Data Format) — chuẩn trao đổi dữ liệu form.
pub fn export_fdf(input: &Path, output: &Path) -> Result<(), EngineError> {
    let fields = list_form_fields(input)?;
    let mut body = String::new();
    for f in &fields {
        if matches!(f.kind, FieldKind::Button | FieldKind::Signature) && f.value.is_none() {
            continue;
        }
        let v = f.value.clone().unwrap_or_default();
        body.push_str(&format!(
            "<< /T {} /V {} >>\n",
            serialize_pdf_text(&f.name),
            serialize_pdf_text(&v)
        ));
    }
    let fdf = format!(
        "%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [\n{body}] >> >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n"
    );
    std::fs::write(output, fdf)?;
    Ok(())
}

/// Chuỗi text PDF cho FDF: ASCII → literal `(...)`; có ký tự Unicode → hex
/// UTF-16BE `<FEFF...>` (portable, Acrobat đọc được, round-trip tiếng Việt).
pub(crate) fn serialize_pdf_text(s: &str) -> String {
    if s.chars().all(|c| (c as u32) < 128) {
        let mut out = String::from("(");
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for u in s.encode_utf16() {
            out.push_str(&format!("{u:04X}"));
        }
        out.push('>');
        out
    }
}

/// Đọc FDF → danh sách (tên, giá trị). Parser tối giản cho FDF do ta/Acrobat xuất.
pub fn parse_fdf(fdf_path: &Path) -> Result<Vec<FieldValue>, EngineError> {
    let text = std::fs::read_to_string(fdf_path)?;
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // Tìm từng cụm "/T <text> ... /V <text>" (literal hoặc hex).
    while let Some(t_rel) = find_from(bytes, b"/T", i) {
        let (name, after_t) = match read_pdf_value(bytes, t_rel + 2) {
            Some(x) => x,
            None => {
                i = t_rel + 2;
                continue;
            }
        };
        // /V có thể đứng sau, trước /T kế tiếp.
        let next_t = find_from(bytes, b"/T", after_t).unwrap_or(bytes.len());
        let value = if let Some(v_rel) = find_from(bytes, b"/V", after_t) {
            if v_rel < next_t {
                read_pdf_value(bytes, v_rel + 2).map(|(v, _)| v).unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        out.push(FieldValue { name, value });
        i = after_t;
    }
    Ok(out)
}

/// Import FDF vào PDF (điền các field tương ứng).
pub fn import_fdf(input: &Path, fdf_path: &Path, output: &Path) -> Result<usize, EngineError> {
    let values = parse_fdf(fdf_path)?;
    fill_form_fields(input, &values, output)
}

/// Xuất CSV 2 cột name,value.
pub fn export_csv(input: &Path, output: &Path) -> Result<(), EngineError> {
    let fields = list_form_fields(input)?;
    let mut s = String::from("name,value\n");
    for f in &fields {
        if matches!(f.kind, FieldKind::Button | FieldKind::Signature) && f.value.is_none() {
            continue;
        }
        s.push_str(&csv_cell(&f.name));
        s.push(',');
        s.push_str(&csv_cell(&f.value.clone().unwrap_or_default()));
        s.push('\n');
    }
    std::fs::write(output, s)?;
    Ok(())
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---- helpers cho parser FDF ----

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Đọc 1 giá trị PDF (literal `(..)` hoặc name `/..`) bắt đầu quét từ `from`.
fn read_pdf_value(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let mut i = from;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'(' {
        read_pdf_literal(bytes, i)
    } else if bytes[i] == b'<' {
        read_pdf_hex(bytes, i)
    } else if bytes[i] == b'/' {
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'/' && bytes[j] != b'>' {
            j += 1;
        }
        Some((String::from_utf8_lossy(&bytes[start..j]).into_owned(), j))
    } else {
        None
    }
}

/// Đọc hex string PDF `<...>` (đã loại nhầm `<<` dict ở caller). Trả (text, sau `>`).
fn read_pdf_hex(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let mut i = from;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'<' {
        return None;
    }
    i += 1;
    let mut hex = Vec::new();
    while i < bytes.len() && bytes[i] != b'>' {
        if !bytes[i].is_ascii_whitespace() {
            hex.push(bytes[i]);
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    i += 1; // qua '>'
    if hex.len() % 2 != 0 {
        hex.push(b'0');
    }
    let raw: Vec<u8> = hex
        .chunks_exact(2)
        .filter_map(|c| {
            let hi = (c[0] as char).to_digit(16)?;
            let lo = (c[1] as char).to_digit(16)?;
            Some(((hi << 4) | lo) as u8)
        })
        .collect();
    Some((decode_pdf_text(&raw), i))
}

/// Đọc literal string PDF `(...)` (xử lý escape + UTF-16BE BOM). `from` trỏ tại
/// hoặc trước dấu `(`. Trả (chuỗi, offset sau `)`).
fn read_pdf_literal(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'(' {
        if !bytes[i].is_ascii_whitespace() {
            return None;
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    i += 1; // qua '('
    let mut raw: Vec<u8> = Vec::new();
    let mut depth = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                raw.push(bytes[i + 1]);
                i += 2;
            }
            b'(' => {
                depth += 1;
                raw.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    break;
                }
                raw.push(b')');
                i += 1;
            }
            b => {
                raw.push(b);
                i += 1;
            }
        }
    }
    Some((decode_pdf_text(&raw), i))
}

// ---- Tạo field mới ----

/// Đặc tả tạo 1 field mới trên 1 trang.
#[derive(Clone, Debug)]
pub struct NewField {
    pub name: String,
    pub kind: FieldKind,
    pub page_index: u16,
    /// [left, bottom, right, top] điểm PDF.
    pub rect: [f32; 4],
    /// Text: giá trị mặc định; Combo: giá trị chọn; Checkbox: "on"/"off".
    pub value: String,
    /// Combo/list options.
    pub options: Vec<String>,
}

/// Tạo các field mới, ghi ra `output`. Widget cơ bản (viền mảnh) + appearance
/// nhờ NeedAppearances. Text/checkbox/combo.
pub fn create_form_fields(input: &Path, fields: &[NewField], output: &Path) -> Result<(), EngineError> {
    // Dùng bộ tạo field đầy đủ (formx): có /AP thật cho mọi loại field.
    let specs: Vec<crate::formx::FieldSpec> = fields
        .iter()
        .filter(|nf| !matches!(nf.kind, FieldKind::Unknown))
        .map(|nf| crate::formx::FieldSpec {
            name: nf.name.clone(),
            kind: nf.kind,
            page_index: nf.page_index,
            rect: nf.rect,
            value: if matches!(nf.kind, FieldKind::Checkbox | FieldKind::Radio) { String::new() } else { nf.value.clone() },
            checked: matches!(nf.value.to_ascii_lowercase().as_str(), "on" | "true" | "yes" | "1" | "checked"),
            options: nf.options.clone(),
            fill_color: Some([0.95, 0.95, 0.95]),
            ..Default::default()
        })
        .collect();
    crate::formx::create_fields(input, &specs, output)?;
    Ok(())
}
