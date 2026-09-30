//! Form AcroForm mức Foxit: liệt kê WIDGET (để điền trực tiếp trên trang),
//! điền kèm dựng lại /AP, tạo mọi loại field (text, checkbox, radio nhóm,
//! combo, list đa chọn, push button có action, date có JS định dạng, chữ ký),
//! sửa thuộc tính / di chuyển / xoá field có sẵn, reset / clear form.
//!
//! Mọi thao tác ở tầng PDF object bằng `lopdf`; appearance dựng ở `formap`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::form::{bit, decode_pdf_text, encode_pdf_text, text_of, widget_page_map, FieldKind};
use crate::formap::{self, ApFonts, Border, Color, TextOpts, WStyle};
use crate::EngineError;

fn le<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("form {ctx}: {e}"))
}

// Cờ /Ff (bit 1-based theo PDF 32000 §12.7).
const FF_READONLY: u32 = 1;
const FF_REQUIRED: u32 = 2;
const FF_MULTILINE: u32 = 13;
const FF_PASSWORD: u32 = 14;
const FF_NO_TOGGLE_OFF: u32 = 15;
const FF_RADIO: u32 = 16;
const FF_PUSH: u32 = 17;
const FF_COMBO: u32 = 18;
const FF_EDIT: u32 = 19;
const FF_MULTISELECT: u32 = 22;
const FF_DONOTSCROLL: u32 = 24;
const FF_COMB: u32 = 25;

fn mask(i: u32) -> i64 {
    1 << (i - 1)
}

pub fn id_str(id: ObjectId) -> String {
    format!("{} {}", id.0, id.1)
}

pub fn parse_id(s: &str) -> Option<ObjectId> {
    let mut it = s.split_whitespace();
    let n = it.next()?.parse().ok()?;
    let g = it.next().unwrap_or("0").parse().ok()?;
    Some((n, g))
}

// ---------- Cây field ----------

/// Field lá (terminal) + các widget của nó.
#[derive(Clone, Debug)]
pub(crate) struct FieldNode {
    pub id: ObjectId,
    pub name: String,
    pub widgets: Vec<ObjectId>,
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        _ => o,
    }
}

fn dict_of(doc: &Document, id: ObjectId) -> Option<&Dictionary> {
    doc.get_object(id).and_then(Object::as_dict).ok()
}

fn is_widget(d: &Dictionary) -> bool {
    d.get(b"Subtype").and_then(Object::as_name).map(|n| n == b"Widget").unwrap_or(false)
}

pub(crate) fn terminal_fields(doc: &Document) -> Vec<FieldNode> {
    fn walk(doc: &Document, id: ObjectId, prefix: &str, out: &mut Vec<FieldNode>, seen: &mut BTreeSet<ObjectId>) {
        if !seen.insert(id) || seen.len() > 100_000 {
            return;
        }
        let Some(dict) = dict_of(doc, id) else { return };
        let partial = dict.get(b"T").ok().map(|o| deref(doc, o)).and_then(text_of);
        let full = match &partial {
            Some(t) if prefix.is_empty() => t.clone(),
            Some(t) => format!("{prefix}.{t}"),
            None => prefix.to_string(),
        };
        let mut kid_fields = Vec::new();
        let mut widgets = Vec::new();
        if let Ok(kids) = dict.get(b"Kids").map(|o| deref(doc, o)).and_then(Object::as_array) {
            for k in kids {
                let Ok(kid) = k.as_reference() else { continue };
                let Some(kd) = dict_of(doc, kid) else { continue };
                if kd.has(b"T") || (!is_widget(kd) && kd.has(b"Kids")) {
                    kid_fields.push(kid);
                } else {
                    widgets.push(kid);
                }
            }
        }
        if is_widget(dict) {
            widgets.insert(0, id);
        }
        for kid in kid_fields.iter() {
            walk(doc, *kid, &full, out, seen);
        }
        if !widgets.is_empty() || (kid_fields.is_empty() && (partial.is_some() || dict.has(b"FT"))) {
            out.push(FieldNode { id, name: full, widgets });
        }
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(acro) = acroform(doc) {
        if let Ok(fields) = acro.get(b"Fields").map(|o| deref(doc, o)).and_then(Object::as_array) {
            for f in fields {
                if let Ok(id) = f.as_reference() {
                    walk(doc, id, "", &mut out, &mut seen);
                }
            }
        }
    }
    out
}

fn acroform(doc: &Document) -> Option<&Dictionary> {
    let cat = doc.catalog().ok()?;
    match cat.get(b"AcroForm").ok()? {
        Object::Reference(id) => dict_of(doc, *id),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// Đảm bảo /AcroForm là object gián tiếp có /Fields, /DA; trả id.
pub(crate) fn ensure_acroform(doc: &mut Document) -> Result<ObjectId, EngineError> {
    let root_id = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(le("root"))?;
    let cur = doc
        .get_object(root_id)
        .and_then(Object::as_dict)
        .map_err(le("catalog"))?
        .get(b"AcroForm")
        .ok()
        .cloned();
    let id = match cur {
        Some(Object::Reference(id)) if dict_of(doc, id).is_some() => id,
        Some(Object::Dictionary(d)) => {
            let id = doc.add_object(Object::Dictionary(d));
            if let Ok(cat) = doc.get_object_mut(root_id).and_then(Object::as_dict_mut) {
                cat.set("AcroForm", Object::Reference(id));
            }
            id
        }
        _ => {
            let mut d = Dictionary::new();
            d.set("Fields", Object::Array(vec![]));
            let id = doc.add_object(Object::Dictionary(d));
            if let Ok(cat) = doc.get_object_mut(root_id).and_then(Object::as_dict_mut) {
                cat.set("AcroForm", Object::Reference(id));
            }
            id
        }
    };
    // /Fields gián tiếp → kéo về trực tiếp để sửa dễ.
    let fields = dict_of(doc, id).and_then(|a| a.get(b"Fields").ok()).cloned();
    let arr = match fields {
        Some(Object::Reference(r)) => doc.get_object(r).and_then(Object::as_array).cloned().unwrap_or_default(),
        Some(Object::Array(a)) => a,
        _ => vec![],
    };
    if let Ok(a) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
        a.set("Fields", Object::Array(arr));
        if !a.has(b"DA") {
            a.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
        }
    }
    Ok(id)
}

/// Thuộc tính kế thừa (đi ngược /Parent).
fn inh<'a>(doc: &'a Document, id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = Some(id);
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let d = dict_of(doc, c)?;
        if let Ok(v) = d.get(key) {
            return Some(deref(doc, v));
        }
        cur = d.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

fn inh_ff(doc: &Document, id: ObjectId) -> i64 {
    inh(doc, id, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0)
}

fn kind_of(doc: &Document, id: ObjectId) -> FieldKind {
    let ft = inh(doc, id, b"FT").and_then(|o| o.as_name().ok()).map(|n| n.to_vec());
    let ff = inh_ff(doc, id);
    match ft.as_deref() {
        Some(b"Tx") => FieldKind::Text,
        Some(b"Btn") if bit(ff, FF_PUSH) => FieldKind::Button,
        Some(b"Btn") if bit(ff, FF_RADIO) => FieldKind::Radio,
        Some(b"Btn") => FieldKind::Checkbox,
        Some(b"Ch") if bit(ff, FF_COMBO) => FieldKind::Combo,
        Some(b"Ch") => FieldKind::List,
        Some(b"Sig") => FieldKind::Signature,
        _ => FieldKind::Unknown,
    }
}

fn rect_of(d: &Dictionary) -> Option<[f32; 4]> {
    let arr = d.get(b"Rect").and_then(Object::as_array).ok()?;
    let v: Vec<f32> = arr.iter().filter_map(|o| o.as_float().ok()).collect();
    if v.len() != 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

/// Trạng thái BẬT của 1 widget (khoá ≠ Off trong /AP /N hoặc /D).
fn widget_on_state(doc: &Document, wid: ObjectId) -> Option<String> {
    let w = dict_of(doc, wid)?;
    let ap = w.get(b"AP").ok().map(|o| deref(doc, o))?.as_dict().ok()?;
    for key in [&b"N"[..], &b"D"[..]] {
        if let Some(n) = ap.get(key).ok().map(|o| deref(doc, o)).and_then(|o| o.as_dict().ok()) {
            if let Some(k) = n.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).find(|k| k != "Off") {
                return Some(k);
            }
        }
    }
    None
}

fn name_str(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => text_of(o),
    }
}

/// Lựa chọn (export, hiển thị) của field choice.
fn choice_opts(doc: &Document, id: ObjectId) -> Vec<(String, String)> {
    let Some(arr) = inh(doc, id, b"Opt").and_then(|o| o.as_array().ok()) else { return vec![] };
    arr.iter()
        .filter_map(|o| match deref(doc, o) {
            Object::Array(pair) => {
                let e = pair.first().map(|x| deref(doc, x)).and_then(text_of)?;
                let d = pair.get(1).map(|x| deref(doc, x)).and_then(text_of).unwrap_or_else(|| e.clone());
                Some((e, d))
            }
            other => text_of(other).map(|s| (s.clone(), s)),
        })
        .collect()
}

fn values_of(o: Option<&Object>, doc: &Document) -> Vec<String> {
    match o {
        Some(Object::Array(a)) => a.iter().filter_map(|x| name_str(deref(doc, x))).collect(),
        Some(Object::Stream(s)) => vec![decode_pdf_text(&s.decompressed_content().unwrap_or_else(|_| s.content.clone()))],
        Some(x) => name_str(x).into_iter().collect(),
        None => vec![],
    }
}

/// Mô tả DA đã tách: (font, cỡ, màu).
fn parse_da(da: &str) -> (String, f32, Color) {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let mut font = "Helv".to_string();
    let mut size = 0.0f32;
    let mut color = Color::black();
    for (i, t) in toks.iter().enumerate() {
        match *t {
            "Tf" if i >= 2 => {
                font = toks[i - 2].trim_start_matches('/').to_string();
                size = toks[i - 1].parse().unwrap_or(0.0);
            }
            "g" if i >= 1 => color = Color(vec![toks[i - 1].parse().unwrap_or(0.0)]),
            "rg" if i >= 3 => {
                color = Color((1..=3).rev().map(|k| toks[i - k].parse().unwrap_or(0.0)).collect())
            }
            "k" if i >= 4 => {
                color = Color((1..=4).rev().map(|k| toks[i - k].parse().unwrap_or(0.0)).collect())
            }
            _ => {}
        }
    }
    (font, size, color)
}

fn build_da(font: &str, size: f32, color: &Color) -> String {
    let c = &color.0;
    let col = match c.len() {
        1 => format!("{} g", formap::fmt(c[0])),
        4 => format!("{} {} {} {} k", formap::fmt(c[0]), formap::fmt(c[1]), formap::fmt(c[2]), formap::fmt(c[3])),
        _ => format!("{} {} {} rg", formap::fmt(c[0]), formap::fmt(c[1]), formap::fmt(c[2])),
    };
    format!("/{} {} Tf {}", font, formap::fmt(size), col)
}

fn da_of(doc: &Document, field: ObjectId, widget: ObjectId) -> String {
    let from = |o: Option<&Object>| o.and_then(text_of);
    from(dict_of(doc, widget).and_then(|w| w.get(b"DA").ok()))
        .or_else(|| from(inh(doc, field, b"DA")))
        .or_else(|| from(acroform(doc).and_then(|a| a.get(b"DA").ok())))
        .unwrap_or_else(|| "/Helv 0 Tf 0 g".into())
}

fn mk_of(doc: &Document, widget: ObjectId) -> Option<&Dictionary> {
    dict_of(doc, widget)?.get(b"MK").ok().map(|o| deref(doc, o))?.as_dict().ok()
}

/// Kiểu dáng widget để dựng AP.
fn widget_style(doc: &Document, field: ObjectId, widget: ObjectId) -> WStyle {
    let w = dict_of(doc, widget);
    let r = w.and_then(rect_of).unwrap_or([0.0, 0.0, 100.0, 20.0]);
    let mk = mk_of(doc, widget);
    let col = |k: &[u8]| mk.and_then(|m| m.get(k).ok()).map(|o| deref(doc, o)).and_then(|o| o.as_array().ok()).and_then(|a| Color::from_array(a));
    let bs = w.and_then(|w| w.get(b"BS").ok()).map(|o| deref(doc, o)).and_then(|o| o.as_dict().ok());
    let bw = bs
        .and_then(|b| b.get(b"W").ok())
        .and_then(|o| o.as_float().ok())
        .or_else(|| {
            w.and_then(|w| w.get(b"Border").ok())
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.get(2))
                .and_then(|o| o.as_float().ok())
        })
        .unwrap_or(1.0);
    let border = bs
        .and_then(|b| b.get(b"S").ok())
        .and_then(|o| o.as_name().ok())
        .map(Border::from_name)
        .unwrap_or(Border::Solid);
    let (font, size, color) = parse_da(&da_of(doc, field, widget));
    let align = inh(doc, field, b"Q").and_then(|o| o.as_i64().ok()).unwrap_or(0).clamp(0, 2) as u8;
    WStyle {
        w: (r[2] - r[0]).max(1.0),
        h: (r[3] - r[1]).max(1.0),
        bg: col(b"BG"),
        bc: col(b"BC"),
        bw,
        border,
        font,
        font_size: size,
        color,
        align,
    }
}

fn js_of(doc: &Document, action: Option<&Object>) -> Option<String> {
    let a = action.map(|o| deref(doc, o))?.as_dict().ok()?;
    let js = a.get(b"JS").ok().map(|o| deref(doc, o))?;
    match js {
        Object::Stream(s) => Some(String::from_utf8_lossy(&s.decompressed_content().unwrap_or_else(|_| s.content.clone())).into_owned()),
        other => text_of(other),
    }
}

/// Định dạng ngày trong `AFDate_FormatEx("dd/mm/yyyy")` của /AA /F (hoặc /K).
fn date_format_of(doc: &Document, field: ObjectId, widget: ObjectId) -> Option<String> {
    for src in [widget, field] {
        let Some(aa) = dict_of(doc, src).and_then(|d| d.get(b"AA").ok()).map(|o| deref(doc, o)).and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        for k in [&b"F"[..], &b"K"[..]] {
            if let Some(js) = js_of(doc, aa.get(k).ok()) {
                for fnname in ["AFDate_FormatEx(", "AFDate_KeystrokeEx(", "AFDate_Format(", "AFDate_Keystroke("] {
                    if let Some(p) = js.find(fnname) {
                        let rest = &js[p + fnname.len()..];
                        let rest = rest.trim_start();
                        if let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                            if let Some(end) = rest[1..].find(q) {
                                return Some(rest[1..1 + end].to_string());
                            }
                        }
                        // AFDate_Format(n): chỉ số định dạng cũ → mặc định.
                        return Some("mm/dd/yyyy".into());
                    }
                }
            }
        }
    }
    None
}

/// Action của push button.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ButtonAction {
    /// "none" | "reset" | "print" | "url" | "submit" | "js"
    pub kind: String,
    pub target: String,
}

fn action_of(doc: &Document, widget: ObjectId, field: ObjectId) -> ButtonAction {
    let a = dict_of(doc, widget)
        .and_then(|w| w.get(b"A").ok())
        .or_else(|| dict_of(doc, field).and_then(|f| f.get(b"A").ok()))
        .map(|o| deref(doc, o))
        .and_then(|o| o.as_dict().ok());
    let Some(a) = a else { return ButtonAction { kind: "none".into(), target: String::new() } };
    let s = a.get(b"S").and_then(Object::as_name).unwrap_or(b"");
    match s {
        b"ResetForm" => ButtonAction { kind: "reset".into(), target: String::new() },
        b"URI" => ButtonAction {
            kind: "url".into(),
            target: a.get(b"URI").ok().map(|o| deref(doc, o)).and_then(text_of).unwrap_or_default(),
        },
        b"SubmitForm" => {
            let f = a.get(b"F").ok().map(|o| deref(doc, o));
            let url = match f {
                Some(Object::Dictionary(d)) => d.get(b"F").ok().and_then(text_of).unwrap_or_default(),
                Some(o) => text_of(o).unwrap_or_default(),
                None => String::new(),
            };
            ButtonAction { kind: "submit".into(), target: url }
        }
        b"Named" => {
            let n = a.get(b"N").and_then(Object::as_name).unwrap_or(b"");
            if n == b"Print" {
                ButtonAction { kind: "print".into(), target: String::new() }
            } else {
                ButtonAction { kind: "none".into(), target: String::new() }
            }
        }
        b"JavaScript" => {
            let js = js_of(doc, Some(&Object::Dictionary(a.clone()))).unwrap_or_default();
            if js.contains("print(") {
                ButtonAction { kind: "print".into(), target: String::new() }
            } else if js.contains("resetForm(") {
                ButtonAction { kind: "reset".into(), target: String::new() }
            } else {
                ButtonAction { kind: "js".into(), target: js }
            }
        }
        _ => ButtonAction { kind: "none".into(), target: String::new() },
    }
}

// ---------- Liệt kê widget ----------

/// 1 widget (ô hiển thị) của 1 field — đơn vị UI điền/sửa trên trang.
#[derive(Clone, Debug, Default)]
pub struct WidgetInfo {
    pub id: String,
    pub field_id: String,
    pub name: String,
    pub kind: FieldKind,
    pub page_index: u16,
    pub rect: [f32; 4],
    /// Giá trị (text; tên trạng thái của checkbox/radio; lựa chọn đầu của list).
    pub value: String,
    /// Mọi giá trị (list đa chọn).
    pub values: Vec<String>,
    pub default_value: String,
    /// Trạng thái bật của RIÊNG widget này (checkbox/radio).
    pub export_value: String,
    pub checked: bool,
    pub options: Vec<String>,
    pub option_exports: Vec<String>,
    pub read_only: bool,
    pub required: bool,
    pub multiline: bool,
    pub password: bool,
    pub comb: bool,
    pub do_not_scroll: bool,
    pub multi_select: bool,
    pub editable: bool,
    pub max_len: u32,
    pub align: u8,
    pub font_size: f32,
    pub text_color: [f32; 3],
    pub border_color: Option<[f32; 3]>,
    pub fill_color: Option<[f32; 3]>,
    pub border_width: f32,
    /// "solid" | "dashed" | "beveled" | "inset" | "underline"
    pub border_style: String,
    pub tooltip: String,
    pub date_format: String,
    pub caption: String,
    pub check_style: String,
    pub action: ButtonAction,
    pub hidden: bool,
    pub tab_order: u32,
}

fn border_name(b: Border) -> &'static str {
    match b {
        Border::Solid => "solid",
        Border::Dashed => "dashed",
        Border::Beveled => "beveled",
        Border::Inset => "inset",
        Border::Underline => "underline",
    }
}

fn border_from_str(s: &str) -> Border {
    match s {
        "dashed" => Border::Dashed,
        "beveled" => Border::Beveled,
        "inset" => Border::Inset,
        "underline" => Border::Underline,
        _ => Border::Solid,
    }
}

/// Liệt kê mọi widget của form, theo thứ tự Tab (trang → /Tabs → /Annots).
pub fn list_widgets(input: &Path) -> Result<Vec<WidgetInfo>, EngineError> {
    let doc = Document::load(input).map_err(le("load"))?;
    Ok(widgets_of(&doc))
}

pub(crate) fn widgets_of(doc: &Document) -> Vec<WidgetInfo> {
    let wmap = widget_page_map(doc);
    // Thứ tự tab: vị trí trong /Annots (hoặc hàng/cột theo /Tabs).
    let mut order: HashMap<ObjectId, u32> = HashMap::new();
    let mut seq = 0u32;
    for (_, page_id) in doc.get_pages() {
        let Some(page) = dict_of(doc, page_id) else { continue };
        let tabs = page.get(b"Tabs").and_then(Object::as_name).unwrap_or(b"").to_vec();
        let Ok(annots) = page.get(b"Annots").map(|o| deref(doc, o)).and_then(Object::as_array) else { continue };
        let mut ids: Vec<(ObjectId, [f32; 4])> = annots
            .iter()
            .filter_map(|a| a.as_reference().ok())
            .filter_map(|id| {
                let d = dict_of(doc, id)?;
                if !is_widget(d) {
                    return None;
                }
                Some((id, rect_of(d).unwrap_or([0.0; 4])))
            })
            .collect();
        if tabs == b"R" {
            ids.sort_by(|a, b| {
                let ra = (-(a.1[3] / 4.0).round(), a.1[0]);
                let rb = (-(b.1[3] / 4.0).round(), b.1[0]);
                ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if tabs == b"C" {
            ids.sort_by(|a, b| {
                let ra = ((a.1[0] / 4.0).round(), -a.1[3]);
                let rb = ((b.1[0] / 4.0).round(), -b.1[3]);
                ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        for (id, _) in ids {
            order.insert(id, seq);
            seq += 1;
        }
    }

    let mut out = Vec::new();
    for node in terminal_fields(doc) {
        let kind = kind_of(doc, node.id);
        let ff = inh_ff(doc, node.id);
        let fdict = dict_of(doc, node.id);
        let vals = values_of(inh(doc, node.id, b"V"), doc);
        let dvals = values_of(inh(doc, node.id, b"DV"), doc);
        let opts = choice_opts(doc, node.id);
        let tooltip = fdict.and_then(|d| d.get(b"TU").ok()).map(|o| deref(doc, o)).and_then(text_of).unwrap_or_default();
        let max_len = inh(doc, node.id, b"MaxLen").and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as u32;
        for &wid in &node.widgets {
            let Some(w) = dict_of(doc, wid) else { continue };
            let Some(rect) = rect_of(w) else { continue };
            let Some(&page) = wmap.get(&wid) else { continue };
            let st = widget_style(doc, node.id, wid);
            let bs = w.get(b"BS").ok().map(|o| deref(doc, o)).and_then(|o| o.as_dict().ok());
            let has_bs_w = bs.map(|b| b.has(b"W")).unwrap_or(false) || w.has(b"Border");
            let mk = mk_of(doc, wid);
            let ca = mk.and_then(|m| m.get(b"CA").ok()).map(|o| deref(doc, o)).and_then(text_of).unwrap_or_default();
            let export = if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
                widget_on_state(doc, wid).unwrap_or_else(|| "Yes".into())
            } else {
                String::new()
            };
            let as_state = w.get(b"AS").and_then(Object::as_name).ok().map(|n| String::from_utf8_lossy(n).into_owned());
            let checked = match &as_state {
                Some(s) => s != "Off" && (s == &export || export.is_empty()),
                None => vals.first().map(|v| v == &export).unwrap_or(false),
            };
            let f = w.get(b"F").and_then(Object::as_i64).unwrap_or(0);
            out.push(WidgetInfo {
                id: id_str(wid),
                field_id: id_str(node.id),
                name: node.name.clone(),
                kind,
                page_index: page,
                rect,
                value: vals.first().cloned().unwrap_or_default(),
                values: vals.clone(),
                default_value: dvals.first().cloned().unwrap_or_default(),
                export_value: export,
                checked,
                options: opts.iter().map(|o| o.1.clone()).collect(),
                option_exports: opts.iter().map(|o| o.0.clone()).collect(),
                read_only: bit(ff, FF_READONLY),
                required: bit(ff, FF_REQUIRED),
                multiline: bit(ff, FF_MULTILINE),
                password: bit(ff, FF_PASSWORD),
                comb: bit(ff, FF_COMB),
                do_not_scroll: bit(ff, FF_DONOTSCROLL),
                multi_select: bit(ff, FF_MULTISELECT),
                editable: bit(ff, FF_EDIT),
                max_len,
                align: st.align,
                font_size: st.font_size,
                text_color: st.color.to_rgb(),
                border_color: st.bc.as_ref().map(|c| c.to_rgb()),
                fill_color: st.bg.as_ref().map(|c| c.to_rgb()),
                border_width: if has_bs_w || st.bc.is_some() { st.bw } else { 0.0 },
                border_style: border_name(st.border).into(),
                tooltip: tooltip.clone(),
                date_format: if kind == FieldKind::Text { date_format_of(doc, node.id, wid).unwrap_or_default() } else { String::new() },
                caption: if kind == FieldKind::Button { ca.clone() } else { String::new() },
                check_style: if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
                    if ca.is_empty() {
                        if kind == FieldKind::Radio { "circle".into() } else { "check".into() }
                    } else {
                        formap::style_from_char(&ca).into()
                    }
                } else {
                    String::new()
                },
                action: if kind == FieldKind::Button { action_of(doc, wid, node.id) } else { ButtonAction::default() },
                hidden: f & 2 != 0 || f & 32 != 0,
                tab_order: order.get(&wid).copied().unwrap_or(u32::MAX),
            });
        }
    }
    out.sort_by_key(|w| (w.page_index, w.tab_order));
    out
}

// ---------- Dựng lại appearance ----------

/// Dựng lại /AP mọi widget của 1 field theo giá trị hiện tại.
/// Trả false nếu loại field không dựng được (để giữ NeedAppearances).
pub(crate) fn regen_field(doc: &mut Document, acro: ObjectId, fonts: &mut ApFonts, node: &FieldNode) -> bool {
    let kind = kind_of(doc, node.id);
    let ff = inh_ff(doc, node.id);
    let vals = values_of(inh(doc, node.id, b"V"), doc);
    let value = vals.first().cloned().unwrap_or_default();
    let opts = choice_opts(doc, node.id);
    for &wid in &node.widgets {
        // Widget xoay (/MK /R ≠ 0): chưa hỗ trợ → để viewer tự dựng.
        let rot = mk_of(doc, wid).and_then(|m| m.get(b"R").ok()).and_then(|o| o.as_i64().ok()).unwrap_or(0);
        if rot % 360 != 0 {
            return false;
        }
        let st = widget_style(doc, node.id, wid);
        let mut ap = Dictionary::new();
        match kind {
            FieldKind::Text | FieldKind::Combo => {
                let max_len = inh(doc, node.id, b"MaxLen").and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as u32;
                let shown = if kind == FieldKind::Combo {
                    opts.iter().find(|o| o.0 == value).map(|o| o.1.clone()).unwrap_or(value.clone())
                } else {
                    value.clone()
                };
                let opts_t = TextOpts {
                    text: &shown,
                    multiline: kind == FieldKind::Text && bit(ff, FF_MULTILINE),
                    comb: if kind == FieldKind::Text && bit(ff, FF_COMB) && max_len > 0 { Some(max_len) } else { None },
                    password: kind == FieldKind::Text && bit(ff, FF_PASSWORD),
                };
                let (c, r) = formap::text_ap(doc, acro, fonts, &st, &opts_t);
                let x = formap::make_xobject(doc, st.w, st.h, c, r);
                ap.set("N", Object::Reference(x));
            }
            FieldKind::List => {
                let sel: Vec<usize> = {
                    let from_i: Vec<usize> = inh(doc, node.id, b"I")
                        .and_then(|o| o.as_array().ok())
                        .map(|a| a.iter().filter_map(|x| x.as_i64().ok()).map(|x| x as usize).collect())
                        .unwrap_or_default();
                    if !from_i.is_empty() {
                        from_i
                    } else {
                        opts.iter().enumerate().filter(|(_, o)| vals.contains(&o.0)).map(|(i, _)| i).collect()
                    }
                };
                let top = inh(doc, node.id, b"TI").and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as usize;
                let items: Vec<String> = opts.iter().map(|o| o.1.clone()).collect();
                let (c, r) = formap::list_ap(doc, acro, fonts, &st, &items, &sel, top);
                let x = formap::make_xobject(doc, st.w, st.h, c, r);
                ap.set("N", Object::Reference(x));
            }
            FieldKind::Checkbox | FieldKind::Radio => {
                let round = kind == FieldKind::Radio;
                let export = widget_on_state(doc, wid).unwrap_or_else(|| "Yes".into());
                let ca = mk_of(doc, wid).and_then(|m| m.get(b"CA").ok()).and_then(text_of);
                let style = match ca.as_deref() {
                    Some(c) if !c.is_empty() => formap::style_from_char(c),
                    _ if round => "circle",
                    _ => "check",
                };
                let mut n = Dictionary::new();
                let mut d = Dictionary::new();
                for (state, on) in [(export.as_str(), true), ("Off", false)] {
                    let xn = formap::make_xobject(doc, st.w, st.h, formap::check_ap(&st, style, on, round, false), Dictionary::new());
                    let xd = formap::make_xobject(doc, st.w, st.h, formap::check_ap(&st, style, on, round, true), Dictionary::new());
                    n.set(state.as_bytes().to_vec(), Object::Reference(xn));
                    d.set(state.as_bytes().to_vec(), Object::Reference(xd));
                }
                ap.set("N", Object::Dictionary(n));
                ap.set("D", Object::Dictionary(d));
                let on = value == export;
                if let Ok(w) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
                    w.set("AS", Object::Name(if on { export.into_bytes() } else { b"Off".to_vec() }));
                }
            }
            FieldKind::Button => {
                let cap = mk_of(doc, wid).and_then(|m| m.get(b"CA").ok()).and_then(text_of).unwrap_or_default();
                let (c, r) = formap::button_ap(doc, acro, fonts, &st, &cap, false);
                let xn = formap::make_xobject(doc, st.w, st.h, c, r);
                let (c2, r2) = formap::button_ap(doc, acro, fonts, &st, &cap, true);
                let xd = formap::make_xobject(doc, st.w, st.h, c2, r2);
                ap.set("N", Object::Reference(xn));
                ap.set("D", Object::Reference(xd));
            }
            FieldKind::Signature => {
                // Chữ ký đã ký có AP riêng — không đè.
                if inh(doc, node.id, b"V").is_some() {
                    continue;
                }
                let xn = formap::make_xobject(doc, st.w, st.h, formap::sig_ap(&st), Dictionary::new());
                ap.set("N", Object::Reference(xn));
            }
            FieldKind::Unknown => return false,
        }
        if let Ok(w) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
            w.set("AP", Object::Dictionary(ap));
        }
    }
    true
}

fn need_appearances(doc: &Document) -> bool {
    acroform(doc)
        .and_then(|a| a.get(b"NeedAppearances").ok())
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
}

fn set_need_appearances(doc: &mut Document, acro: ObjectId, v: bool) {
    if let Ok(a) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
        if v {
            a.set("NeedAppearances", Object::Boolean(true));
        } else {
            a.remove(b"NeedAppearances");
        }
    }
}

/// Sau khi dựng AP cho các field đã đổi: nếu tài liệu đang dựa vào
/// NeedAppearances, dựng nốt field thiếu AP rồi tắt cờ (mọi viewer hiển thị
/// giống nhau); dựng hỏng field nào thì giữ cờ.
fn finalize_appearances(doc: &mut Document, acro: ObjectId, fonts: &mut ApFonts, done: &BTreeSet<ObjectId>) {
    if !need_appearances(doc) {
        return;
    }
    let mut ok = true;
    for node in terminal_fields(doc) {
        if done.contains(&node.id) {
            continue;
        }
        let missing = node.widgets.iter().any(|w| {
            dict_of(doc, *w).map(|d| !d.has(b"AP")).unwrap_or(false)
        });
        if missing || kind_of(doc, node.id) == FieldKind::Text || matches!(kind_of(doc, node.id), FieldKind::Combo | FieldKind::List) {
            ok &= regen_field(doc, acro, fonts, &node);
        }
    }
    set_need_appearances(doc, acro, !ok);
}

// ---------- Điền ----------

/// Giá trị điền cho 1 field. `values` (≥1 phần tử) dùng cho list đa chọn.
#[derive(Clone, Debug, Default)]
pub struct FillValue {
    pub name: String,
    pub value: String,
    pub values: Vec<String>,
}

fn is_on_word(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "on" | "true" | "yes" | "1" | "checked")
}

/// Đặt giá trị cho 1 field (chưa dựng AP). Trả true nếu đổi được.
fn set_value(doc: &mut Document, node: &FieldNode, fv: &FillValue) -> bool {
    let kind = kind_of(doc, node.id);
    let ff = inh_ff(doc, node.id);
    match kind {
        FieldKind::Checkbox | FieldKind::Radio => {
            let states: Vec<String> = node.widgets.iter().map(|w| widget_on_state(doc, *w).unwrap_or_else(|| "Yes".into())).collect();
            let v = fv.value.trim();
            let target = if v.is_empty() || v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("false") || v == "0" {
                "Off".to_string()
            } else if states.iter().any(|s| s == v) {
                v.to_string()
            } else if is_on_word(v) || kind == FieldKind::Checkbox {
                states.first().cloned().unwrap_or_else(|| "Yes".into())
            } else {
                return false;
            };
            // Radio "không bỏ chọn được" vẫn cho reset về Off qua điền rỗng (như Foxit Reset).
            let _ = bit(ff, FF_NO_TOGGLE_OFF);
            let tname = Object::Name(target.clone().into_bytes());
            if let Ok(f) = doc.get_object_mut(node.id).and_then(Object::as_dict_mut) {
                f.set("V", tname);
            }
            for (w, s) in node.widgets.iter().zip(states.iter()) {
                if let Ok(wd) = doc.get_object_mut(*w).and_then(Object::as_dict_mut) {
                    wd.set("AS", Object::Name(if *s == target { target.clone().into_bytes() } else { b"Off".to_vec() }));
                }
            }
            true
        }
        FieldKind::Text => {
            let mut v = fv.value.clone();
            let max_len = inh(doc, node.id, b"MaxLen").and_then(|o| o.as_i64().ok()).unwrap_or(0);
            if max_len > 0 {
                v = v.chars().take(max_len as usize).collect();
            }
            if !bit(ff, FF_MULTILINE) {
                v = v.replace("\r\n", " ").replace(['\r', '\n'], " ");
            }
            if let Ok(f) = doc.get_object_mut(node.id).and_then(Object::as_dict_mut) {
                if v.is_empty() {
                    f.remove(b"V");
                } else {
                    f.set("V", encode_pdf_text(&v));
                }
            }
            true
        }
        FieldKind::Combo | FieldKind::List => {
            let opts = choice_opts(doc, node.id);
            // Nhận cả export lẫn nhãn hiển thị.
            let to_export = |s: &str| -> String {
                if opts.iter().any(|o| o.0 == s) {
                    s.to_string()
                } else if let Some(o) = opts.iter().find(|o| o.1 == s) {
                    o.0.clone()
                } else {
                    s.to_string()
                }
            };
            let mut list: Vec<String> = if !fv.values.is_empty() { fv.values.clone() } else if fv.value.is_empty() { vec![] } else { vec![fv.value.clone()] };
            list = list.iter().map(|s| to_export(s)).collect();
            if kind == FieldKind::List && !bit(ff, FF_MULTISELECT) {
                list.truncate(1);
            }
            let idx: Vec<Object> = list
                .iter()
                .filter_map(|s| opts.iter().position(|o| &o.0 == s))
                .map(|i| Object::Integer(i as i64))
                .collect();
            if let Ok(f) = doc.get_object_mut(node.id).and_then(Object::as_dict_mut) {
                match list.len() {
                    0 => {
                        f.remove(b"V");
                        f.remove(b"I");
                    }
                    1 => {
                        f.set("V", encode_pdf_text(&list[0]));
                        if idx.is_empty() { f.remove(b"I"); } else { f.set("I", Object::Array(idx)); }
                    }
                    _ => {
                        f.set("V", Object::Array(list.iter().map(|s| encode_pdf_text(s)).collect()));
                        f.set("I", Object::Array(idx));
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// Điền form + dựng lại appearance (tiếng Việt dùng font nhúng). Trả số field đã điền.
pub fn fill_form(input: &Path, values: &[FillValue], output: &Path) -> Result<usize, EngineError> {
    let mut doc = Document::load(input).map_err(le("load"))?;
    let n = fill_doc(&mut doc, values)?;
    doc.prune_objects();
    doc.save(output).map_err(le("save"))?;
    Ok(n)
}

pub(crate) fn fill_doc(doc: &mut Document, values: &[FillValue]) -> Result<usize, EngineError> {
    if values.is_empty() {
        return Ok(0);
    }
    let acro = ensure_acroform(doc)?;
    let mut fonts = ApFonts::prepare(doc, acro)?;
    let lookup: HashMap<&str, &FillValue> = values.iter().map(|v| (v.name.as_str(), v)).collect();
    let mut done = BTreeSet::new();
    let mut n = 0;
    for node in terminal_fields(doc) {
        let Some(fv) = lookup.get(node.name.as_str()) else { continue };
        if set_value(doc, &node, fv) {
            regen_field(doc, acro, &mut fonts, &node);
            done.insert(node.id);
            n += 1;
        }
    }
    finalize_appearances(doc, acro, &mut fonts, &done);
    Ok(n)
}

/// Reset form: đưa mọi field về giá trị mặc định (/DV); `clear` = xoá trắng
/// hẳn (bỏ qua /DV). `names` rỗng = mọi field. Trả số field đã đặt lại.
pub fn reset_form(input: &Path, output: &Path, names: &[String], clear: bool) -> Result<usize, EngineError> {
    let mut doc = Document::load(input).map_err(le("load"))?;
    let acro = ensure_acroform(&mut doc)?;
    let mut fonts = ApFonts::prepare(&mut doc, acro)?;
    let mut done = BTreeSet::new();
    let mut n = 0;
    for node in terminal_fields(&doc) {
        if !names.is_empty() && !names.contains(&node.name) {
            continue;
        }
        let kind = kind_of(&doc, node.id);
        if matches!(kind, FieldKind::Button | FieldKind::Signature | FieldKind::Unknown) {
            continue;
        }
        let dv = if clear { None } else { inh(&doc, node.id, b"DV").cloned() };
        let fv = FillValue {
            name: node.name.clone(),
            value: match &dv {
                Some(Object::Array(_)) | None => String::new(),
                Some(o) => name_str(o).unwrap_or_default(),
            },
            values: match &dv {
                Some(o @ Object::Array(_)) => values_of(Some(o), &doc),
                _ => vec![],
            },
        };
        if set_value(&mut doc, &node, &fv) {
            regen_field(&mut doc, acro, &mut fonts, &node);
            done.insert(node.id);
            n += 1;
        }
    }
    finalize_appearances(&mut doc, acro, &mut fonts, &done);
    doc.prune_objects(); // bỏ AP cũ mồ côi (file không phình dần sau mỗi lần lưu)
    doc.save(output).map_err(le("save"))?;
    Ok(n)
}

// ---------- Tạo / sửa field ----------

/// Đặc tả đầy đủ 1 field (tạo mới, hoặc thuộc tính mới khi sửa).
#[derive(Clone, Debug)]
pub struct FieldSpec {
    pub name: String,
    pub kind: FieldKind,
    pub page_index: u16,
    /// [left, bottom, right, top] điểm PDF.
    pub rect: [f32; 4],
    pub tooltip: String,
    pub required: bool,
    pub read_only: bool,
    /// Giá trị mặc định (text/combo/list: chuỗi; checkbox/radio: dùng `checked`).
    pub value: String,
    /// List đa chọn: các giá trị mặc định.
    pub values: Vec<String>,
    /// 0 = tự co.
    pub font_size: f32,
    pub align: u8,
    pub text_color: [f32; 3],
    pub border_color: Option<[f32; 3]>,
    pub fill_color: Option<[f32; 3]>,
    pub border_width: f32,
    pub border_style: String,
    pub options: Vec<String>,
    pub option_exports: Vec<String>,
    /// Checkbox/radio: tên trạng thái bật (export value).
    pub export_value: String,
    pub checked: bool,
    pub check_style: String,
    pub multiline: bool,
    pub max_len: u32,
    pub comb: bool,
    pub password: bool,
    pub do_not_scroll: bool,
    pub multi_select: bool,
    pub editable: bool,
    pub caption: String,
    pub action: ButtonAction,
    /// Text: định dạng ngày (vd "dd/mm/yyyy") → field ngày có JS AFDate_*.
    pub date_format: String,
}

impl Default for FieldSpec {
    fn default() -> Self {
        FieldSpec {
            name: String::new(),
            kind: FieldKind::Text,
            page_index: 0,
            rect: [0.0, 0.0, 100.0, 20.0],
            tooltip: String::new(),
            required: false,
            read_only: false,
            value: String::new(),
            values: vec![],
            font_size: 0.0,
            align: 0,
            text_color: [0.0, 0.0, 0.0],
            border_color: Some([0.0, 0.0, 0.0]),
            fill_color: None,
            border_width: 1.0,
            border_style: "solid".into(),
            options: vec![],
            option_exports: vec![],
            export_value: String::new(),
            checked: false,
            check_style: String::new(),
            multiline: false,
            max_len: 0,
            comb: false,
            password: false,
            do_not_scroll: false,
            multi_select: false,
            editable: false,
            caption: String::new(),
            action: ButtonAction::default(),
            date_format: String::new(),
        }
    }
}

fn js_action(js: &str) -> Object {
    let mut d = Dictionary::new();
    d.set("S", Object::Name(b"JavaScript".to_vec()));
    d.set("JS", Object::string_literal(js.to_string()));
    Object::Dictionary(d)
}

fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn button_action_obj(a: &ButtonAction) -> Option<Object> {
    let mut d = Dictionary::new();
    match a.kind.as_str() {
        "reset" => d.set("S", Object::Name(b"ResetForm".to_vec())),
        "print" => {
            d.set("S", Object::Name(b"Named".to_vec()));
            d.set("N", Object::Name(b"Print".to_vec()));
        }
        "url" => {
            d.set("S", Object::Name(b"URI".to_vec()));
            d.set("URI", Object::string_literal(a.target.clone()));
        }
        "submit" => {
            d.set("S", Object::Name(b"SubmitForm".to_vec()));
            let mut fs = Dictionary::new();
            fs.set("FS", Object::Name(b"URL".to_vec()));
            fs.set("F", Object::string_literal(a.target.clone()));
            d.set("F", Object::Dictionary(fs));
            d.set("Flags", Object::Integer(4)); // HTML form (ExportFormat)
        }
        "js" if !a.target.is_empty() => return Some(js_action(&a.target)),
        _ => return None,
    }
    Some(Object::Dictionary(d))
}

fn rect_obj(r: [f32; 4]) -> Object {
    Object::Array(vec![
        Object::Real(r[0].min(r[2])),
        Object::Real(r[1].min(r[3])),
        Object::Real(r[0].max(r[2])),
        Object::Real(r[1].max(r[3])),
    ])
}

/// Tên trạng thái PDF hợp lệ (name không chứa khoảng trắng/ký tự đặc biệt).
fn state_name(s: &str, fallback: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let t = t.trim_matches('_').to_string();
    if t.is_empty() || t.eq_ignore_ascii_case("off") {
        fallback.into()
    } else {
        t
    }
}

/// Áp thuộc tính `spec` lên field + widget (dùng cho cả tạo mới lẫn sửa).
fn apply_props(doc: &mut Document, field: ObjectId, widget: ObjectId, spec: &FieldSpec, kind: FieldKind, is_new: bool) {
    let old_export = widget_on_state(doc, widget);
    let old_dv = values_of(inh(doc, field, b"DV"), doc);
    let old_v = values_of(inh(doc, field, b"V"), doc);
    let date = kind == FieldKind::Text && !spec.date_format.trim().is_empty();

    // --- field ---
    if let Ok(f) = doc.get_object_mut(field).and_then(Object::as_dict_mut) {
        if !spec.name.trim().is_empty() {
            f.set("T", encode_pdf_text(spec.name.trim()));
        }
        if spec.tooltip.is_empty() {
            f.remove(b"TU");
        } else {
            f.set("TU", encode_pdf_text(&spec.tooltip));
        }
        let mut ff = f.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
        let mut setb = |b: u32, on: bool| {
            if on { ff |= mask(b) } else { ff &= !mask(b) }
        };
        setb(FF_READONLY, spec.read_only);
        setb(FF_REQUIRED, spec.required);
        match kind {
            FieldKind::Text => {
                setb(FF_MULTILINE, spec.multiline && !spec.comb);
                setb(FF_PASSWORD, spec.password);
                setb(FF_DONOTSCROLL, spec.do_not_scroll || spec.comb);
                setb(FF_COMB, spec.comb && spec.max_len > 0);
            }
            FieldKind::Combo => {
                setb(FF_COMBO, true);
                setb(FF_EDIT, spec.editable);
            }
            FieldKind::List => {
                setb(FF_COMBO, false);
                setb(FF_MULTISELECT, spec.multi_select);
            }
            FieldKind::Radio => {
                setb(FF_RADIO, true);
                setb(FF_NO_TOGGLE_OFF, true);
            }
            FieldKind::Button => setb(FF_PUSH, true),
            _ => {}
        }
        if ff == 0 { f.remove(b"Ff"); } else { f.set("Ff", Object::Integer(ff)); }

        if kind == FieldKind::Text {
            if spec.max_len > 0 { f.set("MaxLen", Object::Integer(spec.max_len as i64)); } else { f.remove(b"MaxLen"); }
        }
        if matches!(kind, FieldKind::Text | FieldKind::Combo | FieldKind::List | FieldKind::Button) {
            if spec.align > 0 { f.set("Q", Object::Integer(spec.align.min(2) as i64)); } else { f.remove(b"Q"); }
        }
        if matches!(kind, FieldKind::Combo | FieldKind::List) {
            let opts: Vec<Object> = spec
                .options
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let e = spec.option_exports.get(i).filter(|e| !e.is_empty() && *e != d);
                    match e {
                        Some(e) => Object::Array(vec![encode_pdf_text(e), encode_pdf_text(d)]),
                        None => encode_pdf_text(d),
                    }
                })
                .collect();
            f.set("Opt", Object::Array(opts));
        }
        if date {
            let fmtj = js_escape(spec.date_format.trim());
            let mut aa = Dictionary::new();
            aa.set("F", js_action(&format!("AFDate_FormatEx(\"{fmtj}\");")));
            aa.set("K", js_action(&format!("AFDate_KeystrokeEx(\"{fmtj}\");")));
            f.set("AA", Object::Dictionary(aa));
        } else if kind == FieldKind::Text {
            // Bỏ định dạng ngày cũ (nếu từng là field ngày).
            let is_date_aa = f
                .get(b"AA")
                .ok()
                .and_then(|o| o.as_dict().ok())
                .and_then(|aa| aa.get(b"F").ok())
                .and_then(|o| o.as_dict().ok())
                .and_then(|a| a.get(b"JS").ok())
                .and_then(text_of)
                .map(|js| js.contains("AFDate_"))
                .unwrap_or(false);
            if is_date_aa {
                f.remove(b"AA");
            }
        }
        let da_font = if spec.value.chars().any(|c| (c as u32) > 126) { formap::UNI_FONT } else { "Helv" };
        if matches!(kind, FieldKind::Text | FieldKind::Combo | FieldKind::List | FieldKind::Button) {
            f.set("DA", Object::string_literal(build_da(da_font, spec.font_size.max(0.0), &Color::rgb(spec.text_color))));
        } else if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
            f.set("DA", Object::string_literal(build_da("ZaDb", spec.font_size.max(0.0), &Color::rgb(spec.text_color))));
        }
    }

    // --- giá trị mặc định ---
    match kind {
        FieldKind::Text | FieldKind::Combo | FieldKind::List => {
            let list: Vec<String> = if kind == FieldKind::List && !spec.values.is_empty() {
                spec.values.clone()
            } else if spec.value.is_empty() {
                vec![]
            } else {
                vec![spec.value.clone()]
            };
            let obj = match list.len() {
                0 => None,
                1 => Some(encode_pdf_text(&list[0])),
                _ => Some(Object::Array(list.iter().map(|s| encode_pdf_text(s)).collect())),
            };
            // Giá trị hiện tại theo mặc định khi tạo mới, hoặc khi đang bằng mặc định cũ.
            let follow = is_new || old_v.is_empty() || old_v == old_dv;
            if let Ok(f) = doc.get_object_mut(field).and_then(Object::as_dict_mut) {
                match &obj {
                    Some(o) => f.set("DV", o.clone()),
                    None => {
                        f.remove(b"DV");
                    }
                }
            }
            if follow {
                let node = FieldNode { id: field, name: String::new(), widgets: vec![] };
                let fv = FillValue { name: String::new(), value: list.first().cloned().unwrap_or_default(), values: list.clone() };
                set_value(doc, &node, &fv);
            }
        }
        _ => {}
    }

    // --- widget ---
    let export = if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
        let fb = old_export.clone().unwrap_or_else(|| {
            if kind == FieldKind::Radio {
                // Nút mới trong nhóm: Choice<n> không trùng nút đã có.
                let n = dict_of(doc, field).and_then(|f| f.get(b"Kids").ok()).and_then(|o| o.as_array().ok()).map(|k| k.len()).unwrap_or(1);
                format!("Choice{}", n.max(1))
            } else {
                "Yes".into()
            }
        });
        Some(state_name(&spec.export_value, &fb))
    } else {
        None
    };
    if let Ok(w) = doc.get_object_mut(widget).and_then(Object::as_dict_mut) {
        let mut mk = w.get(b"MK").ok().and_then(|o| o.as_dict().ok()).cloned().unwrap_or_default();
        match spec.border_color {
            Some(c) if spec.border_width > 0.0 => mk.set("BC", Color::rgb(c).to_object()),
            _ => {
                mk.remove(b"BC");
            }
        }
        match spec.fill_color {
            Some(c) => mk.set("BG", Color::rgb(c).to_object()),
            None => {
                mk.remove(b"BG");
            }
        }
        match kind {
            FieldKind::Checkbox | FieldKind::Radio => {
                let style = if spec.check_style.is_empty() {
                    if kind == FieldKind::Radio { "circle" } else { "check" }
                } else {
                    spec.check_style.as_str()
                };
                mk.set("CA", Object::string_literal(formap::style_char(style)));
            }
            FieldKind::Button => {
                mk.set("CA", encode_pdf_text(&spec.caption));
            }
            _ => {}
        }
        w.set("MK", Object::Dictionary(mk));
        let mut bs = Dictionary::new();
        bs.set("W", Object::Real(spec.border_width.max(0.0)));
        let b = border_from_str(&spec.border_style);
        bs.set("S", Object::Name(b.name().to_vec()));
        if b == Border::Dashed {
            bs.set("D", Object::Array(vec![Object::Integer(3)]));
        }
        w.set("BS", Object::Dictionary(bs));
        w.remove(b"Border");
        if kind == FieldKind::Button {
            match button_action_obj(&spec.action) {
                Some(a) => w.set("A", a),
                None => {
                    w.remove(b"A");
                }
            }
        }
        if let Some(e) = &export {
            // AP tạm chỉ để mang tên trạng thái — regen_field dựng thật sau.
            let mut n = Dictionary::new();
            n.set(e.as_bytes().to_vec(), Object::Null);
            n.set("Off", Object::Null);
            let mut ap = Dictionary::new();
            ap.set("N", Object::Dictionary(n));
            w.set("AP", Object::Dictionary(ap));
        }
    }

    // --- trạng thái checkbox/radio ---
    // `checked` = bật MẶC ĐỊNH (/DV); giá trị hiện tại theo mặc định khi tạo
    // mới hoặc khi chưa ai đổi (V == DV). Đổi export value → đổi tên trạng thái.
    if let Some(e) = export {
        let old_e = old_export.clone().unwrap_or_default();
        let cur_v = values_of(inh(doc, field, b"V"), doc).first().cloned().unwrap_or_else(|| "Off".into());
        let cur_dv = values_of(inh(doc, field, b"DV"), doc).first().cloned().unwrap_or_else(|| "Off".into());
        let rename = |s: &str| if !old_e.is_empty() && s == old_e { e.clone() } else { s.to_string() };
        let untouched = is_new || cur_v == cur_dv;
        let new_dv = if spec.checked {
            e.clone()
        } else if rename(&cur_dv) == e {
            "Off".to_string()
        } else {
            rename(&cur_dv)
        };
        let new_v = if untouched { new_dv.clone() } else { rename(&cur_v) };
        if let Ok(f) = doc.get_object_mut(field).and_then(Object::as_dict_mut) {
            f.set("V", Object::Name(new_v.clone().into_bytes()));
            f.set("DV", Object::Name(new_dv.into_bytes()));
        }
        if let Ok(w) = doc.get_object_mut(widget).and_then(Object::as_dict_mut) {
            w.set("AS", Object::Name(if new_v == e { e.into_bytes() } else { b"Off".to_vec() }));
        }
    }
}

fn page_id_of(doc: &Document, page_index: u16) -> Option<ObjectId> {
    doc.get_pages().get(&(page_index as u32 + 1)).copied()
}

fn add_annot(doc: &mut Document, page_id: ObjectId, id: ObjectId) {
    let cur = dict_of(doc, page_id).and_then(|p| p.get(b"Annots").ok()).cloned();
    let mut arr = match cur {
        Some(Object::Reference(r)) => doc.get_object(r).and_then(Object::as_array).cloned().unwrap_or_default(),
        Some(Object::Array(a)) => a,
        _ => vec![],
    };
    arr.push(Object::Reference(id));
    if let Ok(p) = doc.get_object_mut(page_id).and_then(Object::as_dict_mut) {
        p.set("Annots", Object::Array(arr));
        if !p.has(b"Tabs") {
            p.set("Tabs", Object::Name(b"R".to_vec()));
        }
    }
}

fn remove_annot(doc: &mut Document, wid: ObjectId) {
    for (_, pid) in doc.get_pages() {
        let cur = dict_of(doc, pid).and_then(|p| p.get(b"Annots").ok()).cloned();
        let (arr, holder) = match cur {
            Some(Object::Reference(r)) => (doc.get_object(r).and_then(Object::as_array).cloned().unwrap_or_default(), Some(r)),
            Some(Object::Array(a)) => (a, None),
            _ => continue,
        };
        if !arr.iter().any(|o| o.as_reference().ok() == Some(wid)) {
            continue;
        }
        let kept: Vec<Object> = arr.into_iter().filter(|o| o.as_reference().ok() != Some(wid)).collect();
        match holder {
            Some(r) => {
                if let Ok(a) = doc.get_object_mut(r) {
                    *a = Object::Array(kept);
                }
            }
            None => {
                if let Ok(p) = doc.get_object_mut(pid).and_then(Object::as_dict_mut) {
                    p.set("Annots", Object::Array(kept));
                }
            }
        }
    }
}

fn push_field(doc: &mut Document, acro: ObjectId, id: ObjectId) {
    if let Ok(a) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
        let mut f = a.get(b"Fields").and_then(Object::as_array).cloned().unwrap_or_default();
        f.push(Object::Reference(id));
        a.set("Fields", Object::Array(f));
    }
}

/// Field gộp (field = widget) → tách thành field cha + widget con, để thêm
/// widget thứ hai (nhóm radio, bản sao cùng tên). Trả id field (giữ nguyên
/// id cũ cho widget để /Annots không đổi).
fn split_merged(doc: &mut Document, acro: ObjectId, id: ObjectId) -> ObjectId {
    let Some(d) = dict_of(doc, id).cloned() else { return id };
    if !is_widget(&d) {
        return id;
    }
    const FIELD_KEYS: &[&[u8]] = &[b"FT", b"T", b"TU", b"TM", b"Ff", b"V", b"DV", b"AA", b"DA", b"Q", b"Opt", b"MaxLen", b"I", b"TI", b"Kids", b"Parent", b"DS", b"RV"];
    let mut field = Dictionary::new();
    let mut widget = d.clone();
    for k in FIELD_KEYS {
        if let Ok(v) = d.get(k) {
            if *k == b"AA" {
                // /AA của field (định dạng/keystroke) theo field; widget giữ /AA sự kiện chuột nếu có.
                field.set(k.to_vec(), v.clone());
                widget.remove(k);
                continue;
            }
            field.set(k.to_vec(), v.clone());
            widget.remove(k);
        }
    }
    let fid = doc.add_object(Object::Dictionary(field));
    widget.set("Parent", Object::Reference(fid));
    if let Ok(o) = doc.get_object_mut(id) {
        *o = Object::Dictionary(widget);
    }
    if let Ok(f) = doc.get_object_mut(fid).and_then(Object::as_dict_mut) {
        f.set("Kids", Object::Array(vec![Object::Reference(id)]));
    }
    // Thay id cũ bằng field mới trong /Fields hoặc /Kids của cha.
    let parent = d.get(b"Parent").and_then(Object::as_reference).ok();
    let replace = |arr: &mut Vec<Object>| {
        for o in arr.iter_mut() {
            if o.as_reference().ok() == Some(id) {
                *o = Object::Reference(fid);
            }
        }
    };
    match parent {
        Some(p) => {
            if let Ok(pd) = doc.get_object_mut(p).and_then(Object::as_dict_mut) {
                if let Ok(Object::Array(k)) = pd.get_mut(b"Kids") {
                    replace(k);
                }
            }
        }
        None => {
            if let Ok(a) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
                if let Ok(Object::Array(k)) = a.get_mut(b"Fields") {
                    replace(k);
                }
            }
        }
    }
    fid
}

fn widget_dict(page_id: ObjectId, rect: [f32; 4]) -> Dictionary {
    let mut w = Dictionary::new();
    w.set("Type", Object::Name(b"Annot".to_vec()));
    w.set("Subtype", Object::Name(b"Widget".to_vec()));
    w.set("Rect", rect_obj(rect));
    w.set("P", Object::Reference(page_id));
    w.set("F", Object::Integer(4)); // Print
    w
}

fn ft_of(kind: FieldKind) -> &'static [u8] {
    match kind {
        FieldKind::Text => b"Tx",
        FieldKind::Checkbox | FieldKind::Radio | FieldKind::Button => b"Btn",
        FieldKind::Combo | FieldKind::List => b"Ch",
        FieldKind::Signature => b"Sig",
        FieldKind::Unknown => b"Tx",
    }
}

/// Tạo 1 field trong doc (đã mở). Trả (field id, widget id).
fn create_one(doc: &mut Document, acro: ObjectId, spec: &FieldSpec) -> Result<(ObjectId, ObjectId), EngineError> {
    let page_id = page_id_of(doc, spec.page_index)
        .ok_or_else(|| EngineError::Pdfium(format!("form: không có trang {}", spec.page_index + 1)))?;
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(EngineError::Pdfium("form: tên field rỗng".into()));
    }
    if spec.kind == FieldKind::Unknown {
        return Err(EngineError::Pdfium("form: loại field không hợp lệ".into()));
    }
    let existing = terminal_fields(doc).into_iter().find(|n| n.name == name);
    let (field, widget) = match existing {
        Some(node) => {
            let k = kind_of(doc, node.id);
            if k != spec.kind {
                return Err(EngineError::Pdfium(format!("form: tên \"{name}\" đã dùng cho field loại khác")));
            }
            if k == FieldKind::Signature {
                return Err(EngineError::Pdfium(format!("form: tên \"{name}\" đã tồn tại")));
            }
            // Cùng tên cùng loại = thêm widget (radio: thêm nút vào nhóm).
            let fid = split_merged(doc, acro, node.id);
            let mut w = widget_dict(page_id, spec.rect);
            w.set("Parent", Object::Reference(fid));
            let wid = doc.add_object(Object::Dictionary(w));
            if let Ok(f) = doc.get_object_mut(fid).and_then(Object::as_dict_mut) {
                let mut kids = f.get(b"Kids").and_then(Object::as_array).cloned().unwrap_or_default();
                kids.push(Object::Reference(wid));
                f.set("Kids", Object::Array(kids));
            }
            add_annot(doc, page_id, wid);
            (fid, wid)
        }
        None if spec.kind == FieldKind::Radio => {
            // Radio: field cha (nhóm) + widget con.
            let mut f = Dictionary::new();
            f.set("FT", Object::Name(b"Btn".to_vec()));
            f.set("T", encode_pdf_text(name));
            f.set("Ff", Object::Integer(mask(FF_RADIO) | mask(FF_NO_TOGGLE_OFF)));
            f.set("V", Object::Name(b"Off".to_vec()));
            let fid = doc.add_object(Object::Dictionary(f));
            let mut w = widget_dict(page_id, spec.rect);
            w.set("Parent", Object::Reference(fid));
            let wid = doc.add_object(Object::Dictionary(w));
            if let Ok(f) = doc.get_object_mut(fid).and_then(Object::as_dict_mut) {
                f.set("Kids", Object::Array(vec![Object::Reference(wid)]));
            }
            push_field(doc, acro, fid);
            add_annot(doc, page_id, wid);
            (fid, wid)
        }
        None => {
            let mut w = widget_dict(page_id, spec.rect);
            w.set("FT", Object::Name(ft_of(spec.kind).to_vec()));
            w.set("T", encode_pdf_text(name));
            if spec.kind == FieldKind::Checkbox {
                w.set("V", Object::Name(b"Off".to_vec()));
            }
            let id = doc.add_object(Object::Dictionary(w));
            push_field(doc, acro, id);
            add_annot(doc, page_id, id);
            (id, id)
        }
    };
    apply_props(doc, field, widget, spec, spec.kind, true);
    if spec.kind == FieldKind::Signature {
        if let Ok(a) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
            // SigFlags 1 = tài liệu có field chữ ký.
            let cur = a.get(b"SigFlags").and_then(Object::as_i64).unwrap_or(0);
            a.set("SigFlags", Object::Integer(cur | 1));
        }
    }
    Ok((field, widget))
}

/// Sửa 1 widget/field có sẵn.
#[derive(Clone, Debug, Default)]
pub struct FieldEdit {
    /// Id widget ("12 0") như `WidgetInfo::id`.
    pub widget_id: String,
    pub delete: bool,
    pub rect: Option<[f32; 4]>,
    pub props: Option<FieldSpec>,
}

fn node_of_widget(doc: &Document, wid: ObjectId) -> Option<FieldNode> {
    terminal_fields(doc).into_iter().find(|n| n.widgets.contains(&wid))
}

/// Xoá widget; field hết widget thì xoá khỏi cây (đệ quy lên cha rỗng).
fn delete_widget(doc: &mut Document, acro: ObjectId, wid: ObjectId) {
    remove_annot(doc, wid);
    let mut child = wid;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 64 {
            break;
        }
        let parent = dict_of(doc, child).and_then(|d| d.get(b"Parent").and_then(Object::as_reference).ok());
        let holder_empty = match parent {
            Some(p) => {
                let mut empty = false;
                if let Ok(pd) = doc.get_object_mut(p).and_then(Object::as_dict_mut) {
                    if let Ok(Object::Array(k)) = pd.get_mut(b"Kids") {
                        k.retain(|o| o.as_reference().ok() != Some(child));
                        empty = k.is_empty();
                    }
                }
                empty
            }
            None => {
                if let Ok(a) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
                    if let Ok(Object::Array(k)) = a.get_mut(b"Fields") {
                        k.retain(|o| o.as_reference().ok() != Some(child));
                    }
                    if let Ok(Object::Array(k)) = a.get_mut(b"CO") {
                        k.retain(|o| o.as_reference().ok() != Some(child));
                    }
                }
                false
            }
        };
        doc.objects.remove(&child);
        match parent {
            Some(p) if holder_empty => child = p,
            _ => break,
        }
    }
}

/// Áp các thay đổi cấu trúc form trong 1 lần: tạo field mới, sửa/xoá field
/// có sẵn, rồi điền giá trị. Ghi ra `output`. Trả số field đã tạo.
pub fn apply_form_changes(
    input: &Path,
    creates: &[FieldSpec],
    edits: &[FieldEdit],
    fills: &[FillValue],
    output: &Path,
) -> Result<usize, EngineError> {
    let mut doc = Document::load(input).map_err(le("load"))?;
    let acro = ensure_acroform(&mut doc)?;
    let mut fonts = ApFonts::prepare(&mut doc, acro)?;
    let mut touched: BTreeSet<ObjectId> = BTreeSet::new();

    // 1. Sửa / xoá (trước khi tạo — id widget còn đúng như UI đọc).
    for e in edits {
        let Some(wid) = parse_id(&e.widget_id) else { continue };
        if dict_of(&doc, wid).is_none() {
            continue;
        }
        if e.delete {
            delete_widget(&mut doc, acro, wid);
            continue;
        }
        let Some(node) = node_of_widget(&doc, wid) else { continue };
        if let Some(r) = e.rect {
            if let Ok(w) = doc.get_object_mut(wid).and_then(Object::as_dict_mut) {
                w.set("Rect", rect_obj(r));
            }
        }
        if let Some(p) = &e.props {
            let kind = kind_of(&doc, node.id);
            // Đổi tên trùng field khác → báo lỗi thay vì âm thầm gộp.
            let new_name = p.name.trim();
            if !new_name.is_empty() && new_name != node.name {
                let clash = terminal_fields(&doc).into_iter().any(|n| n.name == new_name && n.id != node.id);
                if clash {
                    return Err(EngineError::Pdfium(format!("form: tên \"{new_name}\" đã tồn tại")));
                }
            }
            let mut p2 = p.clone();
            if new_name == node.name || new_name.is_empty() {
                p2.name = String::new(); // giữ /T (tên đầy đủ có thể gồm tiền tố cha)
            }
            apply_props(&mut doc, node.id, wid, &p2, kind, false);
        }
        touched.insert(wid);
    }

    // 2. Tạo mới.
    let mut created = 0;
    for spec in creates {
        let (_, wid) = create_one(&mut doc, acro, spec)?;
        touched.insert(wid);
        created += 1;
    }

    // 3. Điền.
    let lookup: HashMap<&str, &FillValue> = fills.iter().map(|v| (v.name.as_str(), v)).collect();
    let mut done = BTreeSet::new();
    for node in terminal_fields(&doc) {
        let mut regen = node.widgets.iter().any(|w| touched.contains(w));
        if let Some(fv) = lookup.get(node.name.as_str()) {
            regen |= set_value(&mut doc, &node, fv);
        }
        if regen {
            regen_field(&mut doc, acro, &mut fonts, &node);
            done.insert(node.id);
        }
    }
    finalize_appearances(&mut doc, acro, &mut fonts, &done);
    doc.prune_objects(); // bỏ AP cũ mồ côi (file không phình dần sau mỗi lần lưu)
    doc.save(output).map_err(le("save"))?;
    Ok(created)
}

/// Tạo các field mới (tiện dụng cho test/API). Trả số field đã tạo.
pub fn create_fields(input: &Path, specs: &[FieldSpec], output: &Path) -> Result<usize, EngineError> {
    apply_form_changes(input, specs, &[], &[], output)
}

// ---------- Giá trị cho export dữ liệu ----------

/// (tên, loại, các giá trị) của mọi field có dữ liệu (bỏ push button / chữ ký trống).
pub(crate) fn field_values(doc: &Document) -> Vec<(String, FieldKind, Vec<String>)> {
    let mut out: Vec<(String, FieldKind, Vec<String>)> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for node in terminal_fields(doc) {
        let kind = kind_of(doc, node.id);
        if matches!(kind, FieldKind::Button | FieldKind::Signature | FieldKind::Unknown) {
            continue;
        }
        let vals = values_of(inh(doc, node.id, b"V"), doc);
        if let Some(&i) = seen.get(&node.name) {
            if out[i].2.is_empty() {
                out[i].2 = vals;
            }
            continue;
        }
        seen.insert(node.name.clone(), out.len());
        out.push((node.name, kind, vals));
    }
    out
}
