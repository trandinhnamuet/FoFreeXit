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
//! appearance (cách portable nhất, mọi viewer hiểu). Đồng thời TỰ DỰNG
//! appearance stream (/AP /N) cho text/combo (và checkbox chưa có /AP) — vì
//! Flatten của PDFium chỉ "in" appearance có sẵn, thiếu /AP thì giá trị mất.

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
    // /AS trên widget: field tự là widget, hoặc các widget con.
    for wid in widget_ids(dict, id) {
        // /AS chỉ đặt tên trạng thái nếu widget có appearance tương ứng, ngược
        // lại vẫn đặt (viewer tự xử lý qua NeedAppearances).
        if let Ok(w) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
            w.set("AS", Object::Name(state.as_bytes().to_vec()));
        }
    }
}

/// Widget của field: chính field (nếu nó là widget — có /AP hoặc /Rect), hoặc
/// các phần tử /Kids (widget con không tên).
fn widget_ids(dict: &Dictionary, id: ObjectId) -> Vec<ObjectId> {
    if dict.has(b"AP") || dict.has(b"Rect") {
        vec![id]
    } else {
        dict.get(b"Kids")
            .and_then(Object::as_array)
            .ok()
            .map(|kids| kids.iter().filter_map(|k| k.as_reference().ok()).collect())
            .unwrap_or_default()
    }
}

fn widget_has_ap(doc: &Document, wid: ObjectId) -> bool {
    doc.get_object(wid)
        .and_then(Object::as_dict)
        .map(|d| d.has(b"AP"))
        .unwrap_or(false)
}

// ---- Appearance stream tự dựng (để Flatten giữ giá trị đã điền) ----

/// Mã WinAnsi của 1 ký tự nếu có (0x20–0x7E và 0xA0–0xFF trùng Latin-1).
fn winansi_byte(c: char) -> Option<u8> {
    let u = c as u32;
    if (0x20..=0x7E).contains(&u) || (0xA0..=0xFF).contains(&u) {
        Some(u as u8)
    } else {
        None
    }
}

/// Chuỗi có ký tự ngoài WinAnsi (tiếng Việt, CJK…) → cần font Unicode.
fn needs_unicode(s: &str) -> bool {
    s.chars()
        .any(|c| !matches!(c, '\n' | '\r' | '\t') && winansi_byte(c).is_none())
}

/// Font cho appearance tự dựng: Helvetica (WinAnsi, không nhúng) cho giá trị
/// Latin; font Unicode nhúng (Type0/Identity-H + /ToUnicode) khi có ký tự
/// ngoài WinAnsi. Mỗi font tạo tối đa 1 lần cho cả tài liệu.
struct ApFonts {
    helv: Option<ObjectId>,
    uni: Option<(ObjectId, Vec<u8>)>,
}

impl ApFonts {
    fn new(doc: &mut Document, all_text: &str) -> ApFonts {
        let uni = if needs_unicode(all_text) {
            crate::annot::find_font_bytes(false, false).and_then(|bytes| {
                let id = crate::annot::embed_type0_font(doc, &bytes, all_text).ok()?;
                add_to_unicode(doc, id, &bytes, all_text);
                Some((id, bytes))
            })
        } else {
            None
        };
        ApFonts { helv: None, uni }
    }

    fn helv(&mut self, doc: &mut Document) -> ObjectId {
        if let Some(id) = self.helv {
            return id;
        }
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Font".to_vec()));
        d.set("Subtype", Object::Name(b"Type1".to_vec()));
        d.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        d.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        let id = doc.add_object(Object::Dictionary(d));
        self.helv = Some(id);
        id
    }
}

/// Gắn /ToUnicode (glyph id → Unicode) cho font Type0 Identity-H vừa nhúng,
/// để text sau khi flatten vẫn copy/tìm kiếm được.
fn add_to_unicode(doc: &mut Document, font0_id: ObjectId, font_bytes: &[u8], text: &str) {
    let Ok(face) = ttf_parser::Face::parse(font_bytes, 0) else { return };
    let mut map: BTreeMap<u16, char> = BTreeMap::new();
    for ch in text.chars().chain([' ', '?']) {
        if ch.is_control() {
            continue;
        }
        if let Some(g) = face.glyph_index(ch) {
            map.entry(g.0).or_insert(ch);
        }
    }
    let entries: Vec<(u16, char)> = map.into_iter().collect();
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            let mut buf = [0u16; 2];
            let hex: String = ch.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            cmap.push_str(&format!("<{gid:04X}> <{hex}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let sid = doc.add_object(Object::Stream(lopdf::Stream::new(Dictionary::new(), cmap.into_bytes())));
    if let Ok(f) = doc.get_object_mut(font0_id).and_then(Object::as_dict_mut) {
        f.set("ToUnicode", Object::Reference(sid));
    }
}

/// Kích thước (rộng, cao) của widget theo /Rect.
fn widget_size(doc: &Document, wid: ObjectId) -> Option<(f32, f32)> {
    let d = doc.get_object(wid).and_then(Object::as_dict).ok()?;
    let arr = d.get(b"Rect").and_then(Object::as_array).ok()?;
    if arr.len() != 4 {
        return None;
    }
    let v: Vec<f32> = arr.iter().filter_map(|o| o.as_float().ok()).collect();
    if v.len() != 4 {
        return None;
    }
    let (w, h) = ((v[2] - v[0]).abs(), (v[3] - v[1]).abs());
    if w > 0.0 && h > 0.0 {
        Some((w, h))
    } else {
        None
    }
}

fn pdf_string_text(o: &Object) -> Option<String> {
    match o {
        Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

fn acroform_dict(doc: &Document) -> Option<&Dictionary> {
    let acro = doc.catalog().ok()?.get(b"AcroForm").ok()?;
    match acro {
        Object::Reference(id) => doc.get_object(*id).and_then(Object::as_dict).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// /DA hiệu lực của widget: widget → field → các field cha → /AcroForm.
fn effective_da(doc: &Document, wid: ObjectId, field: &Dictionary) -> String {
    let da_of = |d: &Dictionary| d.get(b"DA").ok().and_then(pdf_string_text);
    if let Some(s) = doc.get_object(wid).and_then(Object::as_dict).ok().and_then(da_of) {
        return s;
    }
    if let Some(s) = da_of(field) {
        return s;
    }
    let mut parent = field.get(b"Parent").and_then(Object::as_reference).ok();
    for _ in 0..32 {
        let Some(pid) = parent else { break };
        let Ok(p) = doc.get_object(pid).and_then(Object::as_dict) else { break };
        if let Some(s) = da_of(p) {
            return s;
        }
        parent = p.get(b"Parent").and_then(Object::as_reference).ok();
    }
    acroform_dict(doc)
        .and_then(da_of)
        .unwrap_or_else(|| "/Helv 0 Tf 0 g".to_string())
}

/// Từ /DA lấy cỡ chữ (0 = tự co) và lệnh màu tô (`g`/`rg`/`k`, mặc định đen).
/// Chỉ nhận toán hạng là số để không chèn rác vào content stream.
fn parse_da(da: &str) -> (f32, String) {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let nums = |ts: &[&str]| -> Option<String> {
        let v: Option<Vec<f32>> = ts.iter().map(|t| t.parse::<f32>().ok()).collect();
        v.map(|v| v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(" "))
    };
    let mut size = 0.0f32;
    let mut color = String::from("0 g");
    for (i, t) in toks.iter().enumerate() {
        let (n, op) = match *t {
            "Tf" => (1, ""),
            "g" => (1, "g"),
            "rg" => (3, "rg"),
            "k" => (4, "k"),
            _ => continue,
        };
        if i < n {
            continue;
        }
        let Some(args) = nums(&toks[i - n..i]) else { continue };
        if op.is_empty() {
            size = args.parse().unwrap_or(0.0);
        } else {
            color = format!("{args} {op}");
        }
    }
    (size.max(0.0), color)
}

/// Độ rộng (pt) của `s` ở cỡ `fs`: đo theo font nhúng nếu có, không thì xấp xỉ
/// Helvetica (~0.55 em/ký tự).
fn text_width(s: &str, fs: f32, face: Option<&ttf_parser::Face>) -> f32 {
    match face {
        Some(f) => {
            let upem = f.units_per_em().max(1) as f32;
            s.chars()
                .map(|c| {
                    f.glyph_index(c)
                        .and_then(|g| f.glyph_hor_advance(g))
                        .map_or(fs * 0.5, |a| a as f32 / upem * fs)
                })
                .sum()
        }
        None => s.chars().count() as f32 * fs * 0.55,
    }
}

/// Toán hạng Tj cho font Identity-H: `<gid gid …>` (2 byte/ký tự).
fn cid_hex(s: &str, face: &ttf_parser::Face) -> String {
    let mut out = String::from("<");
    for c in s.chars() {
        let g = face
            .glyph_index(c)
            .or_else(|| face.glyph_index('?'))
            .unwrap_or(ttf_parser::GlyphId(0));
        out.push_str(&format!("{:04X}", g.0));
    }
    out.push('>');
    out
}

/// Toán hạng Tj cho Helvetica WinAnsi: literal `(...)`, ký tự không mã hoá được
/// thành `?`, byte ≥ 0x80 viết dạng octal để content stream thuần ASCII.
fn winansi_literal(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let b = winansi_byte(c).unwrap_or(b'?');
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// Thêm Form XObject `[0 0 w h]` với content + resources, trả id.
fn add_form_xobject(doc: &mut Document, w: f32, h: f32, resources: Dictionary, content: String) -> ObjectId {
    let mut xd = Dictionary::new();
    xd.set("Type", Object::Name(b"XObject".to_vec()));
    xd.set("Subtype", Object::Name(b"Form".to_vec()));
    xd.set("BBox", Object::Array(vec![Object::Real(0.0), Object::Real(0.0), Object::Real(w), Object::Real(h)]));
    xd.set("Resources", Object::Dictionary(resources));
    doc.add_object(Object::Stream(lopdf::Stream::new(xd, content.into_bytes())))
}

fn set_widget_normal_ap(doc: &mut Document, wid: ObjectId, normal: Object) {
    if let Ok(wd) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
        let mut ap = Dictionary::new();
        ap.set("N", normal);
        wd.set("AP", Object::Dictionary(ap));
    }
}

/// Dựng /AP /N cho widget text/combo hiển thị `value` (thay /AP cũ). Hỗ trợ
/// /DA (cỡ chữ, màu; cỡ 0 = tự co), /Q (căn trái/giữa/phải), multiline (/Ff bit 13).
fn build_text_ap(
    doc: &mut Document,
    fonts: &mut ApFonts,
    wid: ObjectId,
    field: &Dictionary,
    value: &str,
) -> Result<(), EngineError> {
    let (w, h) = widget_size(doc, wid)
        .ok_or_else(|| EngineError::Pdfium("form: widget không có /Rect hợp lệ".into()))?;
    let (da_size, color) = parse_da(&effective_da(doc, wid, field));
    let flags = field.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
    let multiline = bit(flags, 13);
    let widget_q = doc
        .get_object(wid)
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Q"))
        .and_then(Object::as_i64)
        .ok();
    let quadding = field.get(b"Q").and_then(Object::as_i64).ok().or(widget_q).unwrap_or(0);

    let use_uni = needs_unicode(value) && fonts.uni.is_some();
    let helv_id = if use_uni { None } else { Some(fonts.helv(doc)) };
    let uni = if use_uni { fonts.uni.as_ref() } else { None };
    let face = uni.and_then(|u| ttf_parser::Face::parse(&u.1, 0).ok());
    let font_id = match (uni, helv_id) {
        (Some(u), _) => u.0,
        (None, Some(hid)) => hid,
        (None, None) => return Err(EngineError::Pdfium("form: không có font cho appearance".into())),
    };
    // Font nhúng không đọc được → không dựng (tránh Tj sai mã với Type0).
    if use_uni && face.is_none() {
        return Err(EngineError::Pdfium("form: không đọc được font Unicode".into()));
    }

    let lines: Vec<String> = if multiline {
        value
            .replace("\r\n", "\n")
            .split(|c: char| c == '\n' || c == '\r')
            .map(str::to_string)
            .collect()
    } else {
        vec![value.replace(|c: char| c == '\r' || c == '\n', " ")]
    };

    let pad = 2.0f32;
    let avail_w = (w - 2.0 * pad).max(1.0);
    let mut fs = if da_size > 0.0 {
        da_size
    } else if multiline {
        10.0
    } else {
        (h * 0.65).clamp(4.0, 12.0)
    };
    if da_size <= 0.0 && !multiline {
        let tw = text_width(&lines[0], fs, face.as_ref());
        if tw > avail_w {
            fs = (fs * avail_w / tw).max(4.0);
        }
    }

    let mut cs = String::from("/Tx BMC\nq\n");
    cs.push_str(&format!("1 1 {:.2} {:.2} re W n\n", (w - 2.0).max(0.0), (h - 2.0).max(0.0)));
    cs.push_str(&format!("BT\n/FfF0 {fs:.2} Tf\n{color}\n"));
    let line_h = fs * 1.15;
    let mut y = if multiline { h - pad - fs * 0.9 } else { (h - fs) / 2.0 + fs * 0.22 };
    for line in &lines {
        let tw = text_width(line, fs, face.as_ref());
        let x = match quadding {
            1 => (w - tw) / 2.0,
            2 => w - pad - tw,
            _ => pad,
        }
        .max(pad);
        let shown = match &face {
            Some(f) => cid_hex(line, f),
            None => winansi_literal(line),
        };
        cs.push_str(&format!("1 0 0 1 {x:.2} {y:.2} Tm\n{shown} Tj\n"));
        y -= line_h;
    }
    cs.push_str("ET\nQ\nEMC\n");

    let mut font_res = Dictionary::new();
    font_res.set("FfF0", Object::Reference(font_id));
    let mut res = Dictionary::new();
    res.set("Font", Object::Dictionary(font_res));
    let ap_id = add_form_xobject(doc, w, h, res, cs);
    set_widget_normal_ap(doc, wid, Object::Reference(ap_id));
    Ok(())
}

/// Dựng /AP /N cho checkbox: trạng thái `on_name` = dấu tick (vẽ bằng path,
/// không phụ thuộc font ZapfDingbats), `Off` = rỗng.
fn build_checkbox_ap(doc: &mut Document, wid: ObjectId, on_name: &str) -> Result<(), EngineError> {
    let (w, h) = widget_size(doc, wid)
        .ok_or_else(|| EngineError::Pdfium("form: widget không có /Rect hợp lệ".into()))?;
    let s = w.min(h);
    let (ox, oy) = ((w - s) / 2.0, (h - s) / 2.0);
    let lw = (s * 0.12).max(0.8);
    let on_cs = format!(
        "q\n0 g 0 G\n{lw:.2} w\n1 J\n1 j\n{:.2} {:.2} m\n{:.2} {:.2} l\n{:.2} {:.2} l\nS\nQ\n",
        ox + s * 0.2,
        oy + s * 0.52,
        ox + s * 0.42,
        oy + s * 0.25,
        ox + s * 0.8,
        oy + s * 0.78,
    );
    let on_id = add_form_xobject(doc, w, h, Dictionary::new(), on_cs);
    let off_id = add_form_xobject(doc, w, h, Dictionary::new(), String::new());
    let mut n = Dictionary::new();
    n.set(on_name.as_bytes().to_vec(), Object::Reference(on_id));
    if on_name != "Off" {
        n.set("Off", Object::Reference(off_id));
    }
    set_widget_normal_ap(doc, wid, Object::Dictionary(n));
    Ok(())
}

/// Dựng appearance cho widget ĐÃ có giá trị nhưng THIẾU /AP (file điền bởi
/// công cụ khác / bản cũ chỉ bật NeedAppearances). Trả số widget đã dựng.
fn generate_missing_appearances(doc: &mut Document) -> usize {
    let mut jobs: Vec<(Dictionary, FieldKind, String, Vec<ObjectId>)> = Vec::new();
    let mut all_text = String::new();
    for (id, _name, mut dict) in collect_fields(doc) {
        if !dict.has(b"FT") {
            if let Some(ft) = effective_ft(doc, &dict) {
                dict.set("FT", Object::Name(ft));
            }
        }
        let (kind, _) = classify(doc, &dict);
        if !matches!(kind, FieldKind::Text | FieldKind::Combo | FieldKind::Checkbox) {
            continue;
        }
        let value = dict.get(b"V").ok().and_then(text_of).unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        let missing: Vec<ObjectId> = widget_ids(&dict, id)
            .into_iter()
            .filter(|w| !widget_has_ap(doc, *w))
            .collect();
        if missing.is_empty() {
            continue;
        }
        if kind != FieldKind::Checkbox {
            all_text.push_str(&value);
            all_text.push('\n');
        }
        jobs.push((dict, kind, value, missing));
    }
    if jobs.is_empty() {
        return 0;
    }
    let mut fonts = ApFonts::new(doc, &all_text);
    let mut built = 0usize;
    for (dict, kind, value, missing) in jobs {
        for wid in missing {
            let ok = if kind == FieldKind::Checkbox {
                // Trạng thái hiển thị = /AS của widget (thiếu thì theo /V).
                let state = doc
                    .get_object(wid)
                    .and_then(Object::as_dict)
                    .ok()
                    .and_then(|d| d.get(b"AS").ok())
                    .and_then(text_of)
                    .unwrap_or_else(|| value.clone());
                state != "Off" && build_checkbox_ap(doc, wid, &state).is_ok()
            } else {
                build_text_ap(doc, &mut fonts, wid, &dict, &value).is_ok()
            };
            if ok {
                built += 1;
            }
        }
    }
    built
}

/// Bản tạm của `input` đã dựng appearance còn thiếu, hoặc None nếu không cần /
/// không làm được (file mã hoá, lopdf không đọc được…) → flatten file gốc.
fn with_generated_appearances(input: &Path) -> Option<std::path::PathBuf> {
    let bytes = std::fs::read(input).ok()?;
    // lopdf tự giải mã file mật khẩu rỗng rồi bỏ /Encrypt khi ghi → không đụng
    // tới file có mã hoá để khỏi làm mất bảo vệ.
    if bytes.windows(8).any(|w| w == b"/Encrypt") {
        return None;
    }
    let mut doc = Document::load_mem(&bytes).ok()?;
    if generate_missing_appearances(&mut doc) == 0 {
        return None;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("ff_flatten_ap_{}_{nanos}.pdf", std::process::id()));
    doc.save(&tmp).ok()?;
    Some(tmp)
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
/// Dùng flatten của PDFium — nó chỉ in appearance (/AP) CÓ SẴN, nên trước đó
/// dựng appearance cho widget có giá trị mà thiếu /AP (xem
/// `generate_missing_appearances`; file có mật khẩu thì bỏ qua bước này).
pub fn flatten_form(
    pdfium: &pdfium_render::prelude::Pdfium,
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    // Bước phụ trợ, best-effort: lopdf lỗi/panic trên file lạ thì flatten file gốc.
    let prepared = if password.is_none() {
        std::panic::catch_unwind(|| with_generated_appearances(input)).ok().flatten()
    } else {
        None
    };
    let src: &Path = prepared.as_deref().unwrap_or(input);
    let result = flatten_pdfium(pdfium, src, output, password);
    if let Some(tmp) = &prepared {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

fn flatten_pdfium(
    pdfium: &pdfium_render::prelude::Pdfium,
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(format!("form flatten load: {}", crate::pdfium_msg(&e))))?;
    for (i, mut page) in document.pages().iter().enumerate() {
        page.flatten()
            .map_err(|e| EngineError::Pdfium(format!("flatten trang {i}: {e}")))?;
    }
    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("form flatten save: {}", crate::pdfium_msg(&e))))?;
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
