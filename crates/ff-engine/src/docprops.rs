//! Thuộc tính tài liệu (Foxit: File › Properties): Mô tả (Title/Author/
//! Subject/Keywords — ghi /Info VÀ đồng bộ XMP), thông tin chỉ đọc (Producer,
//! Creator, ngày tạo/sửa, phiên bản PDF, cỡ trang, dung lượng, mã hoá, gắn thẻ),
//! Bảo mật (phương thức + quyền), Phông chữ (tên, loại, nhúng/tập con), Chế độ
//! xem ban đầu (/PageLayout, /PageMode, /OpenAction, /ViewerPreferences).

use std::collections::BTreeSet;
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::pdfobj::{self, deref, dict_get, dict_of, dict_text, name_of, obj_dict};
use crate::EngineError;

#[derive(Debug, Clone, PartialEq)]
pub struct FontInfo {
    pub name: String,
    pub font_type: String,
    pub encoding: Option<String>,
    pub embedded: bool,
    pub subset: bool,
}

/// Chế độ xem ban đầu.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InitialView {
    /// Tên PDF (SinglePage, OneColumn, TwoPageLeft, TwoColumnLeft, TwoPageRight,
    /// TwoColumnRight) hoặc rỗng = mặc định.
    pub page_layout: String,
    /// Bảng điều hướng: UseNone, UseOutlines, UseThumbs, UseAttachments, UseOC
    /// (rỗng = mặc định).
    pub navigation: String,
    pub full_screen: bool,
    /// Trang mở đầu (0-based).
    pub open_page: u32,
    /// "default" | "actual" | "fitPage" | "fitWidth" | "fitHeight" | "fitVisible" | "<phần trăm>"
    pub magnification: String,
    pub fit_window: bool,
    pub center_window: bool,
    pub display_doc_title: bool,
    pub hide_menubar: bool,
    pub hide_toolbar: bool,
    pub hide_window_ui: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SecurityInfo {
    pub method: String,
    pub has_user_password: bool,
    pub allow_print: bool,
    pub allow_print_high: bool,
    pub allow_modify: bool,
    pub allow_copy: bool,
    pub allow_annotate: bool,
    pub allow_fill_forms: bool,
    pub allow_accessibility: bool,
    pub allow_assemble: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocProperties {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
    pub pdf_version: String,
    pub page_count: usize,
    pub page_width: f32,
    pub page_height: f32,
    pub file_size: u64,
    pub tagged: bool,
    pub has_xmp: bool,
    pub lang: String,
    pub security: Option<SecurityInfo>,
    pub fonts: Vec<FontInfo>,
    pub view: InitialView,
}

/// Thay đổi cần ghi (None = giữ nguyên).
#[derive(Debug, Clone, Default)]
pub struct DocPropsUpdate {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub lang: Option<String>,
    pub view: Option<InitialView>,
}

fn info_text(doc: &Document, key: &[u8]) -> String {
    pdfobj::info_dict(doc).and_then(|d| dict_text(doc, d, key)).unwrap_or_default()
}

/// Đọc thuộc tính. `original` = tệp gốc người dùng mở (có thể đã mã hoá) để
/// lấy dung lượng + thông tin bảo mật; `input` = bản đang hiển thị.
pub fn read_properties(
    pdfium: Option<&pdfium_render::prelude::Pdfium>,
    input: &Path,
    original: Option<&Path>,
) -> Result<DocProperties, EngineError> {
    let doc = pdfobj::load(input)?;
    let orig = original.unwrap_or(input);
    let file_size = std::fs::metadata(orig).map(|m| m.len()).unwrap_or(0);
    let xmp = xmp_text(&doc);
    let xmp_field = |f: fn(&str) -> Option<String>| xmp.as_deref().and_then(f).unwrap_or_default();
    let or_xmp = |v: String, f: fn(&str) -> Option<String>| if v.is_empty() { xmp_field(f) } else { v };
    let pages = pdfobj::page_ids(&doc);
    let (w, h) = pages
        .first()
        .and_then(|p| page_box(&doc, *p))
        .map(|b| (b[2] - b[0], b[3] - b[1]))
        .unwrap_or((0.0, 0.0));
    let cat = doc.catalog().ok();
    let tagged = cat
        .map(|c| {
            dict_of(&doc, c, b"MarkInfo")
                .and_then(|m| dict_get(&doc, m, b"Marked"))
                .and_then(|o| o.as_bool().ok())
                .unwrap_or(false)
                && c.has(b"StructTreeRoot")
        })
        .unwrap_or(false);
    let lang = cat.and_then(|c| dict_text(&doc, c, b"Lang")).unwrap_or_default();
    Ok(DocProperties {
        title: or_xmp(info_text(&doc, b"Title"), |x| xmp_alt(x, "dc:title")),
        author: or_xmp(info_text(&doc, b"Author"), |x| xmp_seq(x, "dc:creator")),
        subject: or_xmp(info_text(&doc, b"Subject"), |x| xmp_alt(x, "dc:description")),
        keywords: or_xmp(info_text(&doc, b"Keywords"), |x| xmp_simple(x, "pdf:Keywords")),
        creator: or_xmp(info_text(&doc, b"Creator"), |x| xmp_simple(x, "xmp:CreatorTool")),
        producer: or_xmp(info_text(&doc, b"Producer"), |x| xmp_simple(x, "pdf:Producer")),
        creation_date: pdfobj::pdf_date_to_iso(&info_text(&doc, b"CreationDate"))
            .or_else(|| xmp.as_deref().and_then(|x| xmp_simple(x, "xmp:CreateDate"))),
        mod_date: pdfobj::pdf_date_to_iso(&info_text(&doc, b"ModDate"))
            .or_else(|| xmp.as_deref().and_then(|x| xmp_simple(x, "xmp:ModifyDate"))),
        pdf_version: cat
            .and_then(|c| name_of(&doc, c, b"Version"))
            .filter(|v| v.as_str() > doc.version.as_str())
            .unwrap_or_else(|| doc.version.clone()),
        page_count: pages.len(),
        page_width: w,
        page_height: h,
        file_size,
        tagged,
        has_xmp: xmp.is_some(),
        lang,
        security: security_info(pdfium, orig),
        fonts: list_fonts(&doc),
        view: read_view(&doc),
    })
}

fn page_box(doc: &Document, pid: ObjectId) -> Option<[f32; 4]> {
    let mut cur = Some(pid);
    for _ in 0..32 {
        let d = doc.get_dictionary(cur?).ok()?;
        for k in [&b"CropBox"[..], b"MediaBox"] {
            if let Some(r) = pdfobj::rect_of(doc, d, k) {
                return Some(r);
            }
        }
        cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

// ---------------------------------------------------------------- security

fn security_info(pdfium: Option<&pdfium_render::prelude::Pdfium>, orig: &Path) -> Option<SecurityInfo> {
    let raw = std::fs::read(orig).ok()?;
    if !raw.windows(8).any(|w| w == b"/Encrypt") {
        return None;
    }
    // Đọc /Encrypt thô (Reader KHÔNG tự giải được khi có mật khẩu người dùng).
    let doc = Document::load_mem(&raw).ok();
    let (enc, decrypted_ok) = match &doc {
        Some(d) if d.trailer.has(b"Encrypt") => (obj_dict(deref(d, d.trailer.get(b"Encrypt").ok()?)).cloned(), false),
        Some(_) => (None, true), // lopdf đã giải bằng mật khẩu rỗng (Encrypt bị gỡ khỏi trailer)
        None => (None, false),
    };
    let mut s = SecurityInfo { method: "Standard".into(), has_user_password: !decrypted_ok, ..Default::default() };
    let p: i64 = match &enc {
        Some(e) => {
            let v = e.get(b"V").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
            let len = e.get(b"Length").ok().and_then(|o| o.as_i64().ok()).unwrap_or(40);
            let cfm = e
                .get(b"CF")
                .ok()
                .and_then(|o| o.as_dict().ok())
                .and_then(|cf| cf.iter().next().map(|(_, v)| v.clone()))
                .and_then(|v| v.as_dict().ok().and_then(|d| d.get(b"CFM").ok()).and_then(|o| o.as_name().ok()).map(|n| n.to_vec()));
            s.method = match v {
                5 => "AES 256-bit".into(),
                4 if cfm.as_deref() == Some(b"AESV2") => "AES 128-bit".into(),
                4 => "RC4 128-bit".into(),
                2 | 3 => format!("RC4 {len}-bit"),
                _ => "RC4 40-bit".into(),
            };
            e.get(b"P").ok().and_then(|o| o.as_i64().ok()).unwrap_or(-1)
        }
        None => {
            // Chỉ có mật khẩu chủ (mở được không cần mật khẩu): hỏi PDFium quyền.
            let doc = pdfium.and_then(|p| p.load_pdf_from_byte_slice(&raw, None).ok());
            let Some(d) = doc else { return Some(s) };
            let perm = d.permissions();
            if let Ok(rev) = perm.security_handler_revision() {
                s.method = match format!("{rev:?}").as_str() {
                    x if x.contains('6') || x.contains('5') => "AES 256-bit".into(),
                    x if x.contains('4') => "AES / RC4 128-bit".into(),
                    x if x.contains('3') => "RC4 128-bit".into(),
                    _ => "RC4 40-bit".into(),
                };
            }
            let print_hi = perm.can_print_high_quality().unwrap_or(true);
            let print_lo = perm.can_print_only_low_quality().unwrap_or(false);
            s.allow_print = print_hi || print_lo;
            s.allow_print_high = print_hi;
            s.allow_modify = perm.can_modify_document_content().unwrap_or(true);
            s.allow_copy = perm.can_extract_text_and_graphics().unwrap_or(true);
            s.allow_annotate = perm.can_add_or_modify_text_annotations().unwrap_or(true);
            s.allow_fill_forms = perm.can_fill_existing_interactive_form_fields().unwrap_or(true);
            s.allow_accessibility = s.allow_copy || perm.can_extract_text_and_graphics().unwrap_or(true);
            s.allow_assemble = perm.can_assemble_document().unwrap_or(true);
            return Some(s);
        }
    };
    let bit = |n: u32| (p >> (n - 1)) & 1 == 1;
    s.allow_print = bit(3);
    s.allow_modify = bit(4);
    s.allow_copy = bit(5);
    s.allow_annotate = bit(6);
    s.allow_fill_forms = bit(9) || bit(6);
    s.allow_accessibility = bit(10) || bit(5);
    s.allow_assemble = bit(11) || bit(4);
    s.allow_print_high = bit(12) && bit(3);
    Some(s)
}

// ---------------------------------------------------------------- fonts

pub(crate) fn list_fonts(doc: &Document) -> Vec<FontInfo> {
    let mut seen_fonts: BTreeSet<ObjectId> = BTreeSet::new();
    let mut seen_res: BTreeSet<ObjectId> = BTreeSet::new();
    let mut out: Vec<FontInfo> = Vec::new();
    for pid in pdfobj::page_ids(doc) {
        let mut res_stack: Vec<Dictionary> = Vec::new();
        let mut cur = Some(pid);
        for _ in 0..32 {
            let Some(id) = cur else { break };
            let Ok(d) = doc.get_dictionary(id) else { break };
            if let Some(r) = dict_of(doc, d, b"Resources") {
                res_stack.push(r.clone());
                break;
            }
            cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        }
        while let Some(res) = res_stack.pop() {
            if let Some(fonts) = dict_of(doc, &res, b"Font") {
                for (_, f) in fonts.iter() {
                    if let Ok(id) = f.as_reference() {
                        if !seen_fonts.insert(id) {
                            continue;
                        }
                    }
                    if let Some(fd) = obj_dict(deref(doc, f)) {
                        let info = font_info(doc, fd);
                        if !out.contains(&info) {
                            out.push(info);
                        }
                    }
                }
            }
            if let Some(xo) = dict_of(doc, &res, b"XObject") {
                for (_, x) in xo.iter() {
                    let Ok(id) = x.as_reference() else { continue };
                    if !seen_res.insert(id) {
                        continue;
                    }
                    if let Ok(Object::Stream(s)) = doc.get_object(id) {
                        if let Some(r) = dict_of(doc, &s.dict, b"Resources") {
                            res_stack.push(r.clone());
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn font_info(doc: &Document, f: &Dictionary) -> FontInfo {
    let base = name_of(doc, f, b"BaseFont").or_else(|| name_of(doc, f, b"Name")).unwrap_or_else(|| "(Type 3)".into());
    let sub = name_of(doc, f, b"Subtype").unwrap_or_default();
    let encoding = match dict_get(doc, f, b"Encoding") {
        Some(Object::Name(n)) => Some(String::from_utf8_lossy(n).into_owned()),
        Some(Object::Dictionary(_)) => Some("Custom".into()),
        Some(Object::Stream(_)) => Some("Embedded CMap".into()),
        _ => None,
    };
    let (desc, desc_sub) = if sub == "Type0" {
        let d = match dict_get(doc, f, b"DescendantFonts") {
            Some(Object::Array(a)) => a.first().and_then(|o| obj_dict(deref(doc, o))),
            _ => None,
        };
        (d.and_then(|d| dict_of(doc, d, b"FontDescriptor")), d.and_then(|d| name_of(doc, d, b"Subtype")))
    } else {
        (dict_of(doc, f, b"FontDescriptor"), None)
    };
    let file3_sub = desc.and_then(|d| match dict_get(doc, d, b"FontFile3") {
        Some(Object::Stream(s)) => name_of(doc, &s.dict, b"Subtype"),
        _ => None,
    });
    let embedded = sub == "Type3"
        || desc.map(|d| d.has(b"FontFile") || d.has(b"FontFile2") || d.has(b"FontFile3")).unwrap_or(false);
    let font_type = match (sub.as_str(), desc_sub.as_deref(), file3_sub.as_deref()) {
        (_, _, Some("OpenType")) => "OpenType".to_string(),
        ("Type0", Some("CIDFontType2"), _) => "TrueType (CID)".into(),
        ("Type0", Some("CIDFontType0"), Some("CIDFontType0C")) => "Type 1 (CID, CFF)".into(),
        ("Type0", Some("CIDFontType0"), _) => "Type 1 (CID)".into(),
        ("Type1", _, Some("Type1C")) => "Type 1C".into(),
        ("Type1", _, _) => "Type 1".into(),
        ("MMType1", _, _) => "Multiple Master Type 1".into(),
        ("TrueType", _, _) => "TrueType".into(),
        ("Type3", _, _) => "Type 3".into(),
        (s, _, _) => s.to_string(),
    };
    let subset = base.len() > 7 && base.as_bytes()[6] == b'+' && base[..6].chars().all(|c| c.is_ascii_uppercase());
    FontInfo { name: base, font_type, encoding, embedded, subset }
}

// ---------------------------------------------------------------- initial view

fn read_view(doc: &Document) -> InitialView {
    let mut v = InitialView { magnification: "default".into(), ..Default::default() };
    let Ok(cat) = doc.catalog() else { return v };
    v.page_layout = name_of(doc, cat, b"PageLayout").unwrap_or_default();
    let vp = dict_of(doc, cat, b"ViewerPreferences");
    let pm = name_of(doc, cat, b"PageMode").unwrap_or_default();
    if pm == "FullScreen" {
        v.full_screen = true;
        v.navigation = vp.and_then(|p| name_of(doc, p, b"NonFullScreenPageMode")).unwrap_or_default();
    } else {
        v.navigation = pm;
    }
    if v.navigation == "UseNone" {
        v.navigation.clear();
    }
    let flag = |k: &[u8]| vp.and_then(|p| dict_get(doc, p, k)).and_then(|o| o.as_bool().ok()).unwrap_or(false);
    v.fit_window = flag(b"FitWindow");
    v.center_window = flag(b"CenterWindow");
    v.display_doc_title = flag(b"DisplayDocTitle");
    v.hide_menubar = flag(b"HideMenubar");
    v.hide_toolbar = flag(b"HideToolbar");
    v.hide_window_ui = flag(b"HideWindowUI");
    // OpenAction: mảng đích hoặc action GoTo.
    let dest = match dict_get(doc, cat, b"OpenAction") {
        Some(Object::Array(a)) => Some(a.clone()),
        Some(Object::Dictionary(d)) if name_of(doc, d, b"S").as_deref() == Some("GoTo") => match dict_get(doc, d, b"D") {
            Some(Object::Array(a)) => Some(a.clone()),
            _ => None,
        },
        _ => None,
    };
    if let Some(a) = dest {
        let pages = pdfobj::page_ids(doc);
        if let Some(first) = a.first() {
            v.open_page = match first {
                Object::Reference(id) => pages.iter().position(|p| p == id).unwrap_or(0) as u32,
                Object::Integer(i) => (*i).max(0) as u32,
                _ => 0,
            };
        }
        let kind = a.get(1).and_then(|o| o.as_name().ok()).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
        v.magnification = match kind.as_str() {
            "Fit" => "fitPage".into(),
            "FitH" => "fitWidth".into(),
            "FitV" => "fitHeight".into(),
            "FitB" | "FitBH" | "FitBV" => "fitVisible".into(),
            "XYZ" => match a.get(4).and_then(pdfobj::num) {
                Some(z) if z > 0.0 => {
                    if (z - 1.0).abs() < 1e-4 {
                        "actual".into()
                    } else {
                        format!("{}", (z * 100.0).round() as i64)
                    }
                }
                _ => "default".into(),
            },
            _ => "default".into(),
        };
    }
    v
}

fn write_view(doc: &mut Document, v: &InitialView) -> Result<(), EngineError> {
    let pages = pdfobj::page_ids(doc);
    let rid = pdfobj::root_id(doc).ok_or_else(|| EngineError::Pdfium("thiếu catalog".into()))?;
    // ViewerPreferences có thể là tham chiếu.
    let vp_ref = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"ViewerPreferences").ok()).and_then(|o| o.as_reference().ok());
    let mut vp = match vp_ref {
        Some(id) => doc.get_dictionary(id).ok().cloned(),
        None => doc.get_dictionary(rid).ok().and_then(|c| dict_of(doc, c, b"ViewerPreferences").cloned()),
    }
    .unwrap_or_default();
    for (k, on) in [
        (&b"FitWindow"[..], v.fit_window),
        (b"CenterWindow", v.center_window),
        (b"DisplayDocTitle", v.display_doc_title),
        (b"HideMenubar", v.hide_menubar),
        (b"HideToolbar", v.hide_toolbar),
        (b"HideWindowUI", v.hide_window_ui),
    ] {
        if on {
            vp.set(k.to_vec(), Object::Boolean(true));
        } else {
            vp.remove(k);
        }
    }
    let nav = if v.navigation.is_empty() { None } else { Some(v.navigation.clone()) };
    if v.full_screen {
        match &nav {
            Some(n) => vp.set("NonFullScreenPageMode", Object::Name(n.as_bytes().to_vec())),
            None => {
                vp.remove(b"NonFullScreenPageMode");
            }
        }
    } else {
        vp.remove(b"NonFullScreenPageMode");
    }
    let open_existing = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"OpenAction").ok()).cloned();
    let replace_open = match open_existing.as_ref().map(|o| deref(doc, o)) {
        None | Some(Object::Array(_)) => true,
        Some(Object::Dictionary(d)) => name_of(doc, d, b"S").as_deref() == Some("GoTo"),
        _ => true,
    };
    let page_ref = pages.get(v.open_page as usize).or_else(|| pages.first()).copied();
    let dest: Option<Object> = if v.open_page == 0 && v.magnification == "default" {
        None
    } else {
        page_ref.map(|pr| {
            let p = Object::Reference(pr);
            let n = |s: &str| Object::Name(s.as_bytes().to_vec());
            Object::Array(match v.magnification.as_str() {
                "fitPage" => vec![p, n("Fit")],
                "fitWidth" => vec![p, n("FitH"), Object::Null],
                "fitHeight" => vec![p, n("FitV"), Object::Null],
                "fitVisible" => vec![p, n("FitB")],
                "actual" => vec![p, n("XYZ"), Object::Null, Object::Null, Object::Real(1.0)],
                "default" => vec![p, n("XYZ"), Object::Null, Object::Null, Object::Null],
                pct => {
                    let z = pct.parse::<f32>().unwrap_or(100.0) / 100.0;
                    vec![p, n("XYZ"), Object::Null, Object::Null, Object::Real(z)]
                }
            })
        })
    };
    let vp_empty = vp.is_empty();
    if let Some(id) = vp_ref {
        if let Ok(d) = doc.get_dictionary_mut(id) {
            *d = vp.clone();
        }
    }
    let c = doc.get_dictionary_mut(rid).map_err(pdfobj::lerr("catalog"))?;
    if vp_ref.is_none() {
        if vp_empty {
            c.remove(b"ViewerPreferences");
        } else {
            c.set("ViewerPreferences", Object::Dictionary(vp));
        }
    }
    if v.page_layout.is_empty() {
        c.remove(b"PageLayout");
    } else {
        c.set("PageLayout", Object::Name(v.page_layout.as_bytes().to_vec()));
    }
    if v.full_screen {
        c.set("PageMode", Object::Name(b"FullScreen".to_vec()));
    } else if let Some(n) = nav {
        c.set("PageMode", Object::Name(n.into_bytes()));
    } else {
        c.remove(b"PageMode");
    }
    // Đích mới do người dùng chọn thay mọi OpenAction; để "mặc định" thì chỉ
    // gỡ OpenAction dạng đích/GoTo (không đụng action khác, vd JavaScript).
    if replace_open || dest.is_some() {
        match dest {
            Some(d) => c.set("OpenAction", d),
            None => {
                c.remove(b"OpenAction");
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- write

/// Ghi thuộc tính ra tệp mới.
pub fn write_properties(input: &Path, output: &Path, upd: &DocPropsUpdate) -> Result<(), EngineError> {
    let mut doc = pdfobj::load(input)?;
    set_info_and_xmp(&mut doc, upd)?;
    if let Some(lang) = &upd.lang {
        let c = doc.catalog_mut().map_err(pdfobj::lerr("catalog"))?;
        if lang.trim().is_empty() {
            c.remove(b"Lang");
        } else {
            c.set("Lang", pdfobj::text_obj(lang.trim()));
        }
    }
    if let Some(v) = &upd.view {
        write_view(&mut doc, v)?;
    }
    pdfobj::save(&mut doc, output)
}

/// Ghi Title/Author/Subject/Keywords vào /Info (tạo nếu thiếu) + đồng bộ XMP
/// (nếu tài liệu có XMP). ModDate/xmp:ModifyDate cập nhật về hiện tại.
pub(crate) fn set_info_and_xmp(doc: &mut Document, upd: &DocPropsUpdate) -> Result<(), EngineError> {
    let fields: [(&[u8], &Option<String>); 4] =
        [(b"Title", &upd.title), (b"Author", &upd.author), (b"Subject", &upd.subject), (b"Keywords", &upd.keywords)];
    if fields.iter().all(|(_, v)| v.is_none()) {
        return Ok(());
    }
    let now = pdfobj::pdf_date_now();
    let info_ref = doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok());
    let mut info = pdfobj::info_dict(doc).cloned().unwrap_or_default();
    for (k, v) in fields {
        if let Some(s) = v {
            if s.is_empty() {
                info.remove(k);
            } else {
                info.set(k.to_vec(), pdfobj::text_obj(s));
            }
        }
    }
    info.set("ModDate", Object::string_literal(now));
    match info_ref {
        Some(id) if doc.objects.contains_key(&id) => {
            if let Ok(d) = doc.get_dictionary_mut(id) {
                *d = info;
            }
        }
        _ => {
            let id = doc.add_object(info);
            doc.trailer.set("Info", Object::Reference(id));
        }
    }
    // XMP.
    let Some(rid) = pdfobj::root_id(doc) else { return Ok(()) };
    let meta_id = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"Metadata").ok()).and_then(|o| o.as_reference().ok());
    let Some(mid) = meta_id else { return Ok(()) };
    let Ok(Object::Stream(s)) = doc.get_object(mid) else { return Ok(()) };
    let xml = String::from_utf8_lossy(&pdfobj::stream_data(s)).into_owned();
    let new_xml = update_xmp(&xml, upd);
    let mut dict = s.dict.clone();
    dict.remove(b"Filter");
    dict.remove(b"DecodeParms");
    dict.set("Type", Object::Name(b"Metadata".to_vec()));
    dict.set("Subtype", Object::Name(b"XML".to_vec()));
    let st = Stream::new(dict, new_xml.into_bytes()).with_compression(false);
    doc.objects.insert(mid, Object::Stream(st));
    Ok(())
}

// ---------------------------------------------------------------- XMP

fn xmp_text(doc: &Document) -> Option<String> {
    let cat = doc.catalog().ok()?;
    match dict_get(doc, cat, b"Metadata")? {
        Object::Stream(s) => Some(String::from_utf8_lossy(&pdfobj::stream_data(s)).into_owned()),
        _ => None,
    }
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Nội dung phần tử `<tag ...>…</tag>` đầu tiên.
fn element_body<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let i = from + i;
        let after = &xml[i + open.len()..];
        if after.starts_with(|c: char| c == '>' || c.is_whitespace() || c == '/') {
            let gt = after.find('>')?;
            if after[..gt].ends_with('/') {
                return Some("");
            }
            let body_start = i + open.len() + gt + 1;
            let close = format!("</{tag}>");
            let end = xml[body_start..].find(&close)?;
            return Some(&xml[body_start..body_start + end]);
        }
        from = i + open.len();
    }
    None
}

fn xmp_simple(xml: &str, tag: &str) -> Option<String> {
    if let Some(b) = element_body(xml, tag) {
        let t = xml_unescape(b.trim());
        return (!t.is_empty()).then_some(t);
    }
    // Dạng thuộc tính: tag="…"
    let pat = format!("{tag}=\"");
    let i = xml.find(&pat)? + pat.len();
    let j = xml[i..].find('"')?;
    Some(xml_unescape(&xml[i..i + j])).filter(|s| !s.is_empty())
}

fn xmp_alt(xml: &str, tag: &str) -> Option<String> {
    let body = element_body(xml, tag)?;
    let li = element_body(body, "rdf:li")?;
    Some(xml_unescape(li.trim())).filter(|s| !s.is_empty())
}

fn xmp_seq(xml: &str, tag: &str) -> Option<String> {
    let mut body = element_body(xml, tag)?;
    let mut items = Vec::new();
    while let Some(li) = element_body(body, "rdf:li") {
        items.push(xml_unescape(li.trim()));
        let close = body.find("</rdf:li>").map(|i| i + 9).unwrap_or(body.len());
        body = &body[close..];
    }
    Some(items.join("; ")).filter(|s| !s.is_empty())
}

/// Gỡ mọi dạng (phần tử hoặc thuộc tính) của `tag` khỏi XMP.
fn remove_prop(xml: &str, tag: &str) -> String {
    let mut s = xml.to_string();
    // Phần tử.
    loop {
        let open = format!("<{tag}");
        let Some(i) = s.find(&open) else { break };
        let after = &s[i + open.len()..];
        if !after.starts_with(|c: char| c == '>' || c.is_whitespace() || c == '/') {
            break;
        }
        let Some(gt) = after.find('>') else { break };
        let end = if after[..gt].ends_with('/') {
            i + open.len() + gt + 1
        } else {
            let close = format!("</{tag}>");
            match s[i..].find(&close) {
                Some(j) => i + j + close.len(),
                None => break,
            }
        };
        s.replace_range(i..end, "");
    }
    // Thuộc tính.
    loop {
        let pat = format!(" {tag}=\"");
        let Some(i) = s.find(&pat) else { break };
        let Some(j) = s[i + pat.len()..].find('"') else { break };
        s.replace_range(i..i + pat.len() + j + 1, "");
    }
    s
}

/// Cập nhật XMP: xoá dạng cũ của các trường thay đổi rồi thêm một
/// rdf:Description mới chứa giá trị mới (hợp lệ RDF: nhiều Description cùng about="").
fn update_xmp(xml: &str, upd: &DocPropsUpdate) -> String {
    let mut s = xml.to_string();
    let mut body = String::new();
    if let Some(t) = &upd.title {
        s = remove_prop(&s, "dc:title");
        if !t.is_empty() {
            body.push_str(&format!("<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>", xml_escape(t)));
        }
    }
    if let Some(a) = &upd.author {
        s = remove_prop(&s, "dc:creator");
        if !a.is_empty() {
            let items: String = a
                .split(';')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| format!("<rdf:li>{}</rdf:li>", xml_escape(x)))
                .collect();
            body.push_str(&format!("<dc:creator><rdf:Seq>{items}</rdf:Seq></dc:creator>"));
        }
    }
    if let Some(d) = &upd.subject {
        s = remove_prop(&s, "dc:description");
        if !d.is_empty() {
            body.push_str(&format!("<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>", xml_escape(d)));
        }
    }
    if let Some(k) = &upd.keywords {
        s = remove_prop(&s, "pdf:Keywords");
        if !k.is_empty() {
            body.push_str(&format!("<pdf:Keywords>{}</pdf:Keywords>", xml_escape(k)));
        }
    }
    s = remove_prop(&s, "xmp:ModifyDate");
    s = remove_prop(&s, "xmp:MetadataDate");
    let now = pdfobj::iso_now();
    body.push_str(&format!("<xmp:ModifyDate>{now}</xmp:ModifyDate><xmp:MetadataDate>{now}</xmp:MetadataDate>"));
    let desc = format!(
        "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">{body}</rdf:Description>"
    );
    match s.find("</rdf:RDF>") {
        Some(i) => {
            s.insert_str(i, &desc);
            s
        }
        None => {
            // XMP hỏng/rỗng → dựng mới tối thiểu.
            format!(
                "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{desc}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
            )
        }
    }
}

/// Đọc nhanh XMP (cho test): (title, author, subject, keywords).
pub fn read_xmp_fields(input: &Path) -> Result<Option<(String, String, String, String)>, EngineError> {
    let doc = pdfobj::load(input)?;
    Ok(xmp_text(&doc).map(|x| {
        (
            xmp_alt(&x, "dc:title").unwrap_or_default(),
            xmp_seq(&x, "dc:creator").unwrap_or_default(),
            xmp_alt(&x, "dc:description").unwrap_or_default(),
            xmp_simple(&x, "pdf:Keywords").unwrap_or_default(),
        )
    }))
}
