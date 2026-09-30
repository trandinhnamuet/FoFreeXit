//! Trợ năng (Foxit: tab Accessibility › Full Check / Quick Check / Set
//! Alternate Text). Bộ kiểm tra theo mẫu Accessibility Checker của Acrobat:
//! Tài liệu, Nội dung trang, Biểu mẫu, Văn bản thay thế, Bảng, Danh sách,
//! Tiêu đề. Mỗi mục: Đạt / Không đạt / Cần kiểm tra thủ công (+ chi tiết), kèm
//! cờ "sửa tự động được". `fix_accessibility` áp các sửa tự động; `list_figures`
//! / `set_alt_texts` phục vụ hộp thoại Đặt văn bản thay thế.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::*;

use crate::docprops::{self, DocPropsUpdate};
use crate::pdfobj::{self, deref, dict_get, dict_of, dict_text, name_of, obj_dict};
use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed,
    Manual,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Passed => "passed",
            CheckStatus::Failed => "failed",
            CheckStatus::Manual => "manual",
        }
    }
}

/// Một mục kiểm tra.
#[derive(Debug, Clone, PartialEq)]
pub struct A11yCheck {
    /// Khoá ổn định (UI dịch theo khoá): vd "document.title".
    pub id: &'static str,
    pub category: &'static str,
    pub status: CheckStatus,
    /// Số phần tử vi phạm (0 nếu đạt).
    pub count: usize,
    /// Chi tiết (tên font, số trang, tên field…).
    pub details: Vec<String>,
    /// Sửa tự động được bằng `fix_accessibility`.
    pub fixable: bool,
}

fn chk(id: &'static str, category: &'static str, status: CheckStatus, details: Vec<String>, fixable: bool) -> A11yCheck {
    let count = if status == CheckStatus::Passed { 0 } else { details.len() };
    A11yCheck { id, category, status, count, details, fixable: fixable && status == CheckStatus::Failed }
}

fn pass_fail(ok: bool) -> CheckStatus {
    if ok { CheckStatus::Passed } else { CheckStatus::Failed }
}

// ---------------------------------------------------------------- struct tree

/// Phần tử cấu trúc đã làm phẳng.
#[derive(Debug, Clone)]
struct Elem {
    id: Option<ObjectId>,
    role: String,
    alt: Option<String>,
    actual: Option<String>,
    parent: Option<usize>,
    children: Vec<usize>,
    /// Có nội dung trực tiếp (MCID / MCR / OBJR).
    has_content: bool,
    mcids: Vec<(Option<ObjectId>, i64)>,
    page: Option<ObjectId>,
    attrs: Vec<Dictionary>,
}

const STD_ROLES: &[&str] = &[
    "Document", "Part", "Art", "Sect", "Div", "BlockQuote", "Caption", "TOC", "TOCI", "Index", "NonStruct", "Private",
    "P", "H", "H1", "H2", "H3", "H4", "H5", "H6", "L", "LI", "Lbl", "LBody", "Table", "TR", "TH", "TD", "THead",
    "TBody", "TFoot", "Span", "Quote", "Note", "Reference", "BibEntry", "Code", "Link", "Annot", "Ruby", "RB", "RT",
    "RP", "Warichu", "WT", "WP", "Figure", "Formula", "Form", "DocumentFragment", "Aside", "Title", "FENote", "Sub",
    "Em", "Strong", "Artifact",
];

fn struct_elems(doc: &Document) -> Vec<Elem> {
    let mut out = Vec::new();
    let Ok(cat) = doc.catalog() else { return out };
    let Some(root) = dict_of(doc, cat, b"StructTreeRoot") else { return out };
    let rolemap: BTreeMap<String, String> = dict_of(doc, root, b"RoleMap")
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    deref(doc, v).as_name().ok().map(|n| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(n).into_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let resolve = |r: &str| -> String {
        let mut cur = r.to_string();
        for _ in 0..8 {
            if STD_ROLES.contains(&cur.as_str()) {
                break;
            }
            match rolemap.get(&cur) {
                Some(n) => cur = n.clone(),
                None => break,
            }
        }
        cur
    };
    let mut seen = BTreeSet::new();
    let kids = root.get(b"K").ok().cloned();
    if let Some(k) = kids {
        walk_kids(doc, &k, None, None, &mut out, &mut seen, &resolve, 0);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn walk_kids(
    doc: &Document,
    k: &Object,
    parent: Option<usize>,
    page: Option<ObjectId>,
    out: &mut Vec<Elem>,
    seen: &mut BTreeSet<ObjectId>,
    resolve: &dyn Fn(&str) -> String,
    depth: u32,
) {
    if depth > 128 {
        return;
    }
    match k {
        Object::Array(a) => {
            for x in a {
                walk_kids(doc, x, parent, page, out, seen, resolve, depth + 1);
            }
        }
        Object::Integer(mcid) => {
            if let Some(p) = parent {
                out[p].has_content = true;
                out[p].mcids.push((page, *mcid));
            }
        }
        Object::Reference(id) => {
            if !seen.insert(*id) {
                return;
            }
            if let Ok(o) = doc.get_object(*id) {
                if let Some(d) = obj_dict(o) {
                    visit_dict(doc, Some(*id), d, parent, page, out, seen, resolve, depth);
                }
            }
        }
        Object::Dictionary(d) => visit_dict(doc, None, d, parent, page, out, seen, resolve, depth),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_dict(
    doc: &Document,
    id: Option<ObjectId>,
    d: &Dictionary,
    parent: Option<usize>,
    page: Option<ObjectId>,
    out: &mut Vec<Elem>,
    seen: &mut BTreeSet<ObjectId>,
    resolve: &dyn Fn(&str) -> String,
    depth: u32,
) {
    let pg = d.get(b"Pg").ok().and_then(|o| o.as_reference().ok()).or(page);
    if d.has_type(b"MCR") {
        if let Some(p) = parent {
            out[p].has_content = true;
            let mcid = d.get(b"MCID").ok().and_then(|o| o.as_i64().ok()).unwrap_or(-1);
            out[p].mcids.push((pg, mcid));
        }
        return;
    }
    if d.has_type(b"OBJR") {
        if let Some(p) = parent {
            out[p].has_content = true;
        }
        return;
    }
    let Some(role) = name_of(doc, d, b"S") else { return };
    let attrs: Vec<Dictionary> = match dict_get(doc, d, b"A") {
        Some(Object::Array(a)) => a.iter().filter_map(|x| obj_dict(deref(doc, x)).cloned()).collect(),
        Some(Object::Dictionary(x)) => vec![x.clone()],
        _ => Vec::new(),
    };
    let idx = out.len();
    out.push(Elem {
        id,
        role: resolve(&role),
        alt: dict_text(doc, d, b"Alt"),
        actual: dict_text(doc, d, b"ActualText"),
        parent,
        children: Vec::new(),
        has_content: false,
        mcids: Vec::new(),
        page: pg,
        attrs,
    });
    if let Some(p) = parent {
        out[p].children.push(idx);
    }
    if let Ok(k) = d.get(b"K") {
        let k = k.clone();
        walk_kids(doc, &k, Some(idx), pg, out, seen, resolve, depth + 1);
    }
}

fn subtree_has_content(e: &[Elem], i: usize) -> bool {
    e[i].has_content || e[i].children.iter().any(|&c| subtree_has_content(e, c))
}

// ---------------------------------------------------------------- checks

/// Kiểm tra trợ năng. `quick` = chỉ nhóm Tài liệu (như Quick Check).
pub fn check_accessibility(pdfium: &Pdfium, input: &Path, quick: bool) -> Result<Vec<A11yCheck>, EngineError> {
    let doc = pdfobj::load(input)?;
    let cat = doc.catalog().map_err(pdfobj::lerr("catalog"))?.clone();
    let pages = pdfobj::page_ids(&doc);
    let elems = struct_elems(&doc);
    let tagged = dict_of(&doc, &cat, b"MarkInfo")
        .and_then(|m| dict_get(&doc, m, b"Marked"))
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
        && cat.has(b"StructTreeRoot");
    let mut out = Vec::new();
    const D: &str = "document";

    // --- Tài liệu
    let sec = docprops::read_properties(Some(pdfium), input, None).ok().and_then(|p| p.security);
    let access_ok = sec.as_ref().map(|s| s.allow_accessibility).unwrap_or(true);
    out.push(chk("document.accessPermission", D, pass_fail(access_ok), if access_ok { vec![] } else { vec![String::new()] }, false));

    // Trang chỉ có ảnh (không có lớp chữ) → gợi ý OCR.
    let pd = pdfium.load_pdf_from_file(input, None).map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let mut image_only = Vec::new();
    for (i, page) in pd.pages().iter().enumerate() {
        let n_chars = page.text().map(|t| t.chars().len()).unwrap_or(0);
        let has_img = page.objects().iter().any(|o| o.object_type() == PdfPageObjectType::Image);
        if n_chars == 0 && has_img {
            image_only.push(format!("{}", i + 1));
        }
    }
    out.push(chk("document.imageOnly", D, pass_fail(image_only.is_empty()), image_only, false));
    out.push(chk("document.tagged", D, pass_fail(tagged), if tagged { vec![] } else { vec![String::new()] }, false));
    out.push(chk("document.readingOrder", D, CheckStatus::Manual, vec![], false));
    let lang = dict_text(&doc, &cat, b"Lang").unwrap_or_default();
    out.push(chk("document.language", D, pass_fail(!lang.trim().is_empty()), if lang.trim().is_empty() { vec![String::new()] } else { vec![] }, true));
    let title = pdfobj::info_dict(&doc)
        .and_then(|i| dict_text(&doc, i, b"Title"))
        .filter(|t| !t.trim().is_empty())
        .or_else(|| docprops::read_xmp_fields(input).ok().flatten().map(|x| x.0).filter(|t| !t.trim().is_empty()));
    out.push(chk("document.title", D, pass_fail(title.is_some()), if title.is_some() { vec![] } else { vec![String::new()] }, true));
    let ddt = dict_of(&doc, &cat, b"ViewerPreferences")
        .and_then(|v| dict_get(&doc, v, b"DisplayDocTitle"))
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    out.push(chk("document.displayDocTitle", D, pass_fail(ddt), if ddt { vec![] } else { vec![String::new()] }, true));
    let n_bm = dict_of(&doc, &cat, b"Outlines").map(|o| crate::sanitize::count_outline(&doc, o, 0, &mut BTreeSet::new())).unwrap_or(0);
    let bm_ok = pages.len() <= 20 || n_bm > 0;
    let has_headings = elems.iter().any(|e| is_heading(&e.role));
    out.push(chk("document.bookmarks", D, pass_fail(bm_ok), if bm_ok { vec![] } else { vec![String::new()] }, has_headings));
    out.push(chk("document.colorContrast", D, CheckStatus::Manual, vec![], false));
    if quick {
        return Ok(out);
    }

    // --- Nội dung trang
    const P: &str = "page";
    let mut untagged_pages = Vec::new();
    for (i, pid) in pages.iter().enumerate() {
        let Ok(bytes) = doc.get_page_content(*pid) else { continue };
        let Ok(c) = Content::decode(&bytes) else { continue };
        let paints = c.operations.iter().any(|o| matches!(o.operator.as_str(), "Tj" | "TJ" | "'" | "\"" | "Do" | "f" | "S" | "B" | "sh" | "BI"));
        let marked = c.operations.iter().any(|o| {
            (o.operator == "BDC" || o.operator == "BMC")
                && (o.operands.first().and_then(|x| x.as_name().ok()) == Some(b"Artifact")
                    || matches!(o.operands.get(1), Some(Object::Dictionary(d)) if d.has(b"MCID"))
                    || matches!(o.operands.get(1), Some(Object::Name(_))))
        });
        if paints && !(tagged && marked) {
            untagged_pages.push(format!("{}", i + 1));
        }
    }
    out.push(chk("page.taggedContent", P, pass_fail(untagged_pages.is_empty()), untagged_pages, false));
    let mut untagged_annots = Vec::new();
    let mut tab_pages = Vec::new();
    let mut multimedia = 0usize;
    let mut links_no_contents = Vec::new();
    let mut widgets_untagged = 0usize;
    for (i, pid) in pages.iter().enumerate() {
        let annots = pdfobj::page_annots(&doc, *pid);
        let visible: Vec<&Dictionary> = annots
            .iter()
            .map(|(_, d)| d)
            .filter(|d| {
                let st = name_of(&doc, d, b"Subtype").unwrap_or_default();
                st != "Popup" && d.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0) & 2 == 0
            })
            .collect();
        if visible.is_empty() {
            continue;
        }
        let tabs = doc.get_dictionary(*pid).ok().and_then(|p| name_of(&doc, p, b"Tabs"));
        if tabs.as_deref() != Some("S") {
            tab_pages.push(format!("{}", i + 1));
        }
        for d in visible {
            let st = name_of(&doc, d, b"Subtype").unwrap_or_default();
            if matches!(st.as_str(), "Screen" | "Movie" | "Sound" | "RichMedia" | "3D") {
                multimedia += 1;
            }
            if !d.has(b"StructParent") {
                if st == "Widget" {
                    widgets_untagged += 1;
                } else {
                    untagged_annots.push(format!("{} ({})", st, i + 1));
                }
            }
            if st == "Link" && dict_text(&doc, d, b"Contents").map(|s| s.trim().is_empty()).unwrap_or(true) {
                links_no_contents.push(format!("{}", i + 1));
            }
        }
    }
    out.push(chk("page.taggedAnnotations", P, pass_fail(untagged_annots.is_empty()), untagged_annots, false));
    out.push(chk("page.tabOrder", P, pass_fail(tab_pages.is_empty()), tab_pages, true));
    // Mã hoá ký tự: font Type0/Type3 thiếu /ToUnicode.
    let mut bad_enc = Vec::new();
    let fonts = docprops::list_fonts(&doc);
    for (name, fd) in collect_font_dicts(&doc) {
        let st = name_of(&doc, &fd, b"Subtype").unwrap_or_default();
        if (st == "Type0" || st == "Type3") && !fd.has(b"ToUnicode") {
            bad_enc.push(name);
        }
    }
    bad_enc.sort();
    bad_enc.dedup();
    out.push(chk("page.characterEncoding", P, pass_fail(bad_enc.is_empty()), bad_enc, false));
    let not_embedded: Vec<String> = fonts.iter().filter(|f| !f.embedded).map(|f| f.name.clone()).collect();
    out.push(chk("page.fontsEmbedded", P, pass_fail(not_embedded.is_empty()), not_embedded, false));
    let has_js = has_javascript(&doc);
    let manual_if = |b: bool| if b { CheckStatus::Manual } else { CheckStatus::Passed };
    out.push(chk("page.multimedia", P, manual_if(multimedia > 0), vec![], false));
    out.push(chk("page.screenFlicker", P, manual_if(multimedia > 0 || has_js), vec![], false));
    out.push(chk("page.scripts", P, manual_if(has_js), vec![], false));
    out.push(chk("page.timedResponses", P, manual_if(has_js), vec![], false));
    out.push(chk("page.navigationLinks", P, CheckStatus::Manual, vec![], false));
    out.push(chk("page.linkContents", P, pass_fail(links_no_contents.is_empty()), links_no_contents, true));

    // --- Biểu mẫu
    const F: &str = "forms";
    let fields = terminal_fields(&doc);
    out.push(chk(
        "forms.tagged",
        F,
        pass_fail(widgets_untagged == 0),
        (0..widgets_untagged).map(|_| String::new()).collect(),
        false,
    ));
    let no_tu: Vec<String> = fields.iter().filter(|(_, _, tu)| tu.is_none()).map(|(_, n, _)| n.clone()).collect();
    out.push(chk("forms.descriptions", F, pass_fail(no_tu.is_empty()), no_tu, true));

    // --- Văn bản thay thế
    const A: &str = "altText";
    let figs: Vec<&Elem> = elems.iter().filter(|e| e.role == "Figure").collect();
    let no_alt: Vec<String> = figs
        .iter()
        .filter(|e| e.alt.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) && e.actual.is_none())
        .map(|e| elem_label(&doc, e))
        .collect();
    // Tài liệu chưa gắn thẻ mà có ảnh → hình không thể có Alt.
    let untagged_imgs = !tagged && pd.pages().iter().any(|p| p.objects().iter().any(|o| o.object_type() == PdfPageObjectType::Image));
    if untagged_imgs {
        out.push(chk("altText.figures", A, CheckStatus::Failed, vec![String::new()], false));
    } else {
        out.push(chk("altText.figures", A, pass_fail(no_alt.is_empty()), no_alt, false));
    }
    let mut nested = Vec::new();
    let mut orphan_alt = Vec::new();
    for (i, e) in elems.iter().enumerate() {
        if e.alt.is_some() {
            let mut p = e.parent;
            while let Some(pi) = p {
                if elems[pi].alt.is_some() {
                    nested.push(elem_label(&doc, e));
                    break;
                }
                p = elems[pi].parent;
            }
            if !subtree_has_content(&elems, i) {
                orphan_alt.push(elem_label(&doc, e));
            }
        }
    }
    out.push(chk("altText.nested", A, pass_fail(nested.is_empty()), nested, false));
    out.push(chk("altText.associated", A, pass_fail(orphan_alt.is_empty()), orphan_alt, false));
    let other_no_alt: Vec<String> = elems
        .iter()
        .filter(|e| e.role == "Formula" && e.alt.is_none() && e.actual.is_none())
        .map(|e| elem_label(&doc, e))
        .collect();
    out.push(chk("altText.otherElements", A, pass_fail(other_no_alt.is_empty()), other_no_alt, false));

    // --- Bảng
    const T: &str = "tables";
    let mut rows_bad = Vec::new();
    let mut cells_bad = Vec::new();
    let mut no_headers = Vec::new();
    let mut irregular = Vec::new();
    let mut no_summary = Vec::new();
    for (i, e) in elems.iter().enumerate() {
        match e.role.as_str() {
            "Table" => {
                let label = elem_label(&doc, e);
                let rows = table_rows(&elems, i);
                if e.children.iter().any(|&c| !matches!(elems[c].role.as_str(), "TR" | "THead" | "TBody" | "TFoot" | "Caption")) {
                    rows_bad.push(label.clone());
                }
                let any_th = rows.iter().any(|&r| elems[r].children.iter().any(|&c| elems[c].role == "TH"));
                if !any_th {
                    no_headers.push(label.clone());
                }
                let widths: BTreeSet<i64> = rows
                    .iter()
                    .map(|&r| elems[r].children.iter().filter(|&&c| matches!(elems[c].role.as_str(), "TH" | "TD")).map(|&c| col_span(&elems[c])).sum())
                    .collect();
                if widths.len() > 1 {
                    irregular.push(label.clone());
                }
                let has_summary = e.attrs.iter().any(|a| a.has(b"Summary")) || e.alt.is_some();
                if !has_summary {
                    no_summary.push(label);
                }
            }
            "TR" => {
                let p = e.parent.map(|p| elems[p].role.as_str()).unwrap_or("");
                if !matches!(p, "Table" | "THead" | "TBody" | "TFoot") {
                    rows_bad.push(elem_label(&doc, e));
                }
            }
            "TH" | "TD" => {
                let p = e.parent.map(|p| elems[p].role.as_str()).unwrap_or("");
                if p != "TR" {
                    cells_bad.push(elem_label(&doc, e));
                }
            }
            _ => {}
        }
    }
    out.push(chk("tables.rows", T, pass_fail(rows_bad.is_empty()), rows_bad, false));
    out.push(chk("tables.cells", T, pass_fail(cells_bad.is_empty()), cells_bad, false));
    out.push(chk("tables.headers", T, pass_fail(no_headers.is_empty()), no_headers, false));
    out.push(chk("tables.regularity", T, pass_fail(irregular.is_empty()), irregular, false));
    out.push(chk("tables.summary", T, pass_fail(no_summary.is_empty()), no_summary, false));

    // --- Danh sách
    const L: &str = "lists";
    let li_bad: Vec<String> = elems
        .iter()
        .filter(|e| e.role == "LI" && e.parent.map(|p| elems[p].role != "L").unwrap_or(true))
        .map(|e| elem_label(&doc, e))
        .collect();
    let lbl_bad: Vec<String> = elems
        .iter()
        .filter(|e| matches!(e.role.as_str(), "Lbl" | "LBody") && e.parent.map(|p| elems[p].role != "LI").unwrap_or(true))
        .map(|e| elem_label(&doc, e))
        .collect();
    out.push(chk("lists.items", L, pass_fail(li_bad.is_empty()), li_bad, false));
    out.push(chk("lists.lblLBody", L, pass_fail(lbl_bad.is_empty()), lbl_bad, false));

    // --- Tiêu đề: H1..H6 không nhảy cấp khi đi xuống.
    let mut prev = 0u32;
    let mut skips = Vec::new();
    for e in &elems {
        if let Some(l) = heading_level(&e.role) {
            if l > prev + 1 {
                skips.push(format!("{} → {}", if prev == 0 { "—".into() } else { format!("H{prev}") }, e.role));
            }
            prev = l;
        }
    }
    out.push(chk("headings.nesting", "headings", pass_fail(skips.is_empty()), skips, false));
    Ok(out)
}

fn is_heading(r: &str) -> bool {
    r == "H" || heading_level(r).is_some()
}

fn heading_level(r: &str) -> Option<u32> {
    let n = r.strip_prefix('H')?;
    let v: u32 = n.parse().ok()?;
    (1..=6).contains(&v).then_some(v)
}

fn col_span(e: &Elem) -> i64 {
    e.attrs.iter().find_map(|a| a.get(b"ColSpan").ok().and_then(|o| o.as_i64().ok())).unwrap_or(1).max(1)
}

fn table_rows(e: &[Elem], table: usize) -> Vec<usize> {
    let mut rows = Vec::new();
    for &c in &e[table].children {
        match e[c].role.as_str() {
            "TR" => rows.push(c),
            "THead" | "TBody" | "TFoot" => rows.extend(e[c].children.iter().copied().filter(|&r| e[r].role == "TR")),
            _ => {}
        }
    }
    rows
}

/// Nhãn hiển thị của phần tử: "Figure (trang 3)" → UI dịch; ở đây "Role@page".
fn elem_label(doc: &Document, e: &Elem) -> String {
    let pages = pdfobj::page_ids(doc);
    match e.page.and_then(|p| pages.iter().position(|x| *x == p)) {
        Some(i) => format!("{}@{}", e.role, i + 1),
        None => e.role.clone(),
    }
}

fn collect_font_dicts(doc: &Document) -> Vec<(String, Dictionary)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for obj in doc.objects.iter() {
        let (id, o) = obj;
        if let Some(d) = obj_dict(o) {
            if d.has_type(b"Font") && seen.insert(*id) {
                let name = name_of(doc, d, b"BaseFont").unwrap_or_else(|| "(Type 3)".into());
                out.push((name, d.clone()));
            }
        }
    }
    out
}

fn has_javascript(doc: &Document) -> bool {
    doc.objects.values().any(|o| {
        obj_dict(o)
            .map(|d| d.get(b"S").ok().and_then(|s| s.as_name().ok()) == Some(b"JavaScript") || d.has(b"JS"))
            .unwrap_or(false)
    })
}

/// Field cuối (có widget): (id, tên đầy đủ, /TU).
fn terminal_fields(doc: &Document) -> Vec<(ObjectId, String, Option<String>)> {
    let mut out = Vec::new();
    let Ok(cat) = doc.catalog() else { return out };
    let Some(acro) = dict_of(doc, cat, b"AcroForm") else { return out };
    let Some(Object::Array(fields)) = dict_get(doc, acro, b"Fields") else { return out };
    let mut seen = BTreeSet::new();
    for f in fields {
        if let Ok(id) = f.as_reference() {
            walk_field(doc, id, String::new(), None, &mut out, &mut seen, 0);
        }
    }
    out
}

fn walk_field(
    doc: &Document,
    id: ObjectId,
    prefix: String,
    parent_tu: Option<String>,
    out: &mut Vec<(ObjectId, String, Option<String>)>,
    seen: &mut BTreeSet<ObjectId>,
    depth: u32,
) {
    if depth > 32 || !seen.insert(id) {
        return;
    }
    let Ok(d) = doc.get_dictionary(id) else { return };
    let t = dict_text(doc, d, b"T");
    let name = match (&t, prefix.is_empty()) {
        (Some(t), true) => t.clone(),
        (Some(t), false) => format!("{prefix}.{t}"),
        (None, _) => prefix.clone(),
    };
    let tu = dict_text(doc, d, b"TU").filter(|s| !s.trim().is_empty()).or(parent_tu);
    let kids: Vec<ObjectId> = match dict_get(doc, d, b"Kids") {
        Some(Object::Array(a)) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
        _ => Vec::new(),
    };
    // Kid có /T = field con; không có /T = widget của field này.
    let field_kids: Vec<ObjectId> = kids
        .iter()
        .copied()
        .filter(|k| doc.get_dictionary(*k).map(|kd| kd.has(b"T")).unwrap_or(false))
        .collect();
    if field_kids.is_empty() {
        out.push((id, name, tu));
    } else {
        for k in field_kids {
            walk_field(doc, k, name.clone(), tu.clone(), out, seen, depth + 1);
        }
    }
}

// ---------------------------------------------------------------- fixes

/// Các sửa tự động.
#[derive(Debug, Clone, Default)]
pub struct A11yFixes {
    pub title: Option<String>,
    pub lang: Option<String>,
    pub display_doc_title: bool,
    pub tab_order: bool,
    pub field_tooltips: bool,
    /// Đặt /Contents cho liên kết thiếu mô tả. Mẫu cho liên kết nội bộ, vd
    /// "Đi tới trang {n}".
    pub link_contents: Option<String>,
    pub bookmarks_from_headings: bool,
}

/// "ho_ten" / "hoTen" / "form.ngay-sinh" → "ho ten" / "ho Ten" / "ngay sinh".
fn humanize(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    let mut s = String::new();
    let mut prev_lower = false;
    for c in last.chars() {
        if c == '_' || c == '-' {
            s.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            s.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        s.push(c);
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut ch = s.chars();
    match ch.next() {
        Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
        None => last.to_string(),
    }
}

/// Áp sửa tự động, ghi `output`. Trả số thay đổi.
pub fn fix_accessibility(input: &Path, output: &Path, fx: &A11yFixes) -> Result<usize, EngineError> {
    let mut doc = pdfobj::load(input)?;
    let mut n = 0;
    if let Some(t) = fx.title.as_ref().filter(|t| !t.trim().is_empty()) {
        docprops::set_info_and_xmp(&mut doc, &DocPropsUpdate { title: Some(t.trim().to_string()), ..Default::default() })?;
        n += 1;
    }
    let rid = pdfobj::root_id(&doc).ok_or_else(|| EngineError::Pdfium("thiếu catalog".into()))?;
    if let Some(l) = fx.lang.as_ref().filter(|l| !l.trim().is_empty()) {
        doc.get_dictionary_mut(rid).map_err(pdfobj::lerr("catalog"))?.set("Lang", pdfobj::text_obj(l.trim()));
        n += 1;
    }
    if fx.display_doc_title {
        let vp_ref = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"ViewerPreferences").ok()).and_then(|o| o.as_reference().ok());
        match vp_ref {
            Some(id) => doc.get_dictionary_mut(id).map_err(pdfobj::lerr("vp"))?.set("DisplayDocTitle", Object::Boolean(true)),
            None => {
                let mut vp = doc.get_dictionary(rid).ok().and_then(|c| dict_of(&doc, c, b"ViewerPreferences").cloned()).unwrap_or_default();
                vp.set("DisplayDocTitle", Object::Boolean(true));
                doc.get_dictionary_mut(rid).map_err(pdfobj::lerr("catalog"))?.set("ViewerPreferences", Object::Dictionary(vp));
            }
        }
        n += 1;
    }
    let pages = pdfobj::page_ids(&doc);
    if fx.tab_order {
        for pid in &pages {
            if !pdfobj::page_annots_raw(&doc, *pid).is_empty() {
                if let Ok(p) = doc.get_dictionary_mut(*pid) {
                    if p.get(b"Tabs").and_then(|o| o.as_name()).ok() != Some(b"S") {
                        p.set("Tabs", Object::Name(b"S".to_vec()));
                        n += 1;
                    }
                }
            }
        }
    }
    if fx.field_tooltips {
        for (id, name, tu) in terminal_fields(&doc) {
            if tu.is_none() && !name.is_empty() {
                if let Ok(d) = doc.get_dictionary_mut(id) {
                    d.set("TU", pdfobj::text_obj(&humanize(&name)));
                    n += 1;
                }
            }
        }
    }
    if let Some(tpl) = &fx.link_contents {
        let mut edits: Vec<(Option<ObjectId>, ObjectId, usize, String)> = Vec::new();
        for pid in &pages {
            for (ai, (aid, d)) in pdfobj::page_annots(&doc, *pid).into_iter().enumerate() {
                if name_of(&doc, &d, b"Subtype").as_deref() != Some("Link") {
                    continue;
                }
                if dict_text(&doc, &d, b"Contents").map(|s| !s.trim().is_empty()).unwrap_or(false) {
                    continue;
                }
                let desc = link_description(&doc, &d, &pages, tpl);
                edits.push((aid, *pid, ai, desc));
            }
        }
        for (aid, pid, ai, desc) in edits {
            match aid {
                Some(id) => {
                    if let Ok(d) = doc.get_dictionary_mut(id) {
                        d.set("Contents", pdfobj::text_obj(&desc));
                    }
                }
                None => {
                    if let Ok(Object::Array(a)) = doc.get_dictionary_mut(pid).and_then(|p| p.get_mut(b"Annots")) {
                        if let Some(Object::Dictionary(d)) = a.get_mut(ai) {
                            d.set("Contents", pdfobj::text_obj(&desc));
                        }
                    }
                }
            }
            n += 1;
        }
    }
    if fx.bookmarks_from_headings {
        n += bookmarks_from_headings(&mut doc);
    }
    pdfobj::save(&mut doc, output)?;
    Ok(n)
}

fn link_description(doc: &Document, d: &Dictionary, pages: &[ObjectId], tpl: &str) -> String {
    if let Some(a) = dict_of(doc, d, b"A") {
        if let Some(uri) = dict_text(doc, a, b"URI") {
            return uri;
        }
        if let Some(f) = dict_get(doc, a, b"F") {
            if let Some(s) = pdfobj::text_of(doc, f).or_else(|| obj_dict(f).and_then(|fd| dict_text(doc, fd, b"F"))) {
                return s;
            }
        }
    }
    let dest = dict_get(doc, d, b"Dest").or_else(|| dict_of(doc, d, b"A").and_then(|a| dict_get(doc, a, b"D")));
    if let Some(Object::Array(arr)) = dest {
        if let Some(Object::Reference(p)) = arr.first() {
            if let Some(i) = pages.iter().position(|x| x == p) {
                return tpl.replace("{n}", &(i + 1).to_string());
            }
        }
    }
    tpl.replace("{n}", "?")
}

/// Tạo dấu trang từ H1..H6 của cây cấu trúc (lồng theo cấp). Trả số mục.
fn bookmarks_from_headings(doc: &mut Document) -> usize {
    let elems = struct_elems(doc);
    let pages = pdfobj::page_ids(doc);
    let mut items: Vec<(u32, String, Option<ObjectId>)> = Vec::new();
    for e in &elems {
        let lvl = heading_level(&e.role).or(if e.role == "H" { Some(1) } else { None });
        let Some(l) = lvl else { continue };
        let title = e.actual.clone().or_else(|| e.alt.clone()).unwrap_or_else(|| heading_text(doc, e));
        if title.trim().is_empty() {
            continue;
        }
        items.push((l, title.trim().to_string(), e.page.or_else(|| pages.first().copied())));
    }
    if items.is_empty() {
        return 0;
    }
    // Dựng cây: cha = mục gần nhất trước đó có cấp nhỏ hơn.
    let root = doc.new_object_id();
    let ids: Vec<ObjectId> = items.iter().map(|_| doc.new_object_id()).collect();
    let mut parent: Vec<Option<usize>> = vec![None; items.len()];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..items.len() {
        while let Some(&top) = stack.last() {
            if items[top].0 < items[i].0 {
                break;
            }
            stack.pop();
        }
        parent[i] = stack.last().copied();
        stack.push(i);
    }
    let children = |p: Option<usize>| -> Vec<usize> { (0..items.len()).filter(|&i| parent[i] == p).collect() };
    let mut objs: Vec<(ObjectId, Dictionary)> = Vec::new();
    for i in 0..items.len() {
        let mut d = Dictionary::new();
        d.set("Title", pdfobj::text_obj(&items[i].1));
        d.set("Parent", Object::Reference(parent[i].map(|p| ids[p]).unwrap_or(root)));
        if let Some(pg) = items[i].2 {
            d.set("Dest", Object::Array(vec![Object::Reference(pg), Object::Name(b"XYZ".to_vec()), Object::Null, Object::Null, Object::Null]));
        }
        let sibs = children(parent[i]);
        let pos = sibs.iter().position(|&x| x == i).unwrap();
        if pos > 0 {
            d.set("Prev", Object::Reference(ids[sibs[pos - 1]]));
        }
        if pos + 1 < sibs.len() {
            d.set("Next", Object::Reference(ids[sibs[pos + 1]]));
        }
        let kids = children(Some(i));
        if let (Some(f), Some(l)) = (kids.first(), kids.last()) {
            d.set("First", Object::Reference(ids[*f]));
            d.set("Last", Object::Reference(ids[*l]));
            d.set("Count", Object::Integer(-(kids.len() as i64)));
        }
        objs.push((ids[i], d));
    }
    let top = children(None);
    let mut r = Dictionary::new();
    r.set("Type", Object::Name(b"Outlines".to_vec()));
    r.set("First", Object::Reference(ids[top[0]]));
    r.set("Last", Object::Reference(ids[*top.last().unwrap()]));
    r.set("Count", Object::Integer(top.len() as i64));
    doc.objects.insert(root, Object::Dictionary(r));
    for (id, d) in objs {
        doc.objects.insert(id, Object::Dictionary(d));
    }
    if let Ok(c) = doc.catalog_mut() {
        c.set("Outlines", Object::Reference(root));
    }
    items.len()
}

/// Chữ của tiêu đề: gom chuỗi Tj/TJ trong các MCID của phần tử (font đơn
/// giản — đủ cho tiêu đề ASCII/Latin; không giải được thì để trống).
fn heading_text(doc: &Document, e: &Elem) -> String {
    let Some(pg) = e.page else { return String::new() };
    let Ok(bytes) = doc.get_page_content(pg) else { return String::new() };
    let Ok(c) = Content::decode(&bytes) else { return String::new() };
    let want: BTreeSet<i64> = e.mcids.iter().map(|(_, m)| *m).collect();
    let mut stack: Vec<Option<i64>> = Vec::new();
    let mut s = String::new();
    for op in &c.operations {
        match op.operator.as_str() {
            "BDC" => stack.push(match op.operands.get(1) {
                Some(Object::Dictionary(d)) => d.get(b"MCID").ok().and_then(|o| o.as_i64().ok()),
                _ => None,
            }),
            "BMC" => stack.push(None),
            "EMC" => {
                stack.pop();
            }
            "Tj" | "'" | "\"" | "TJ" if stack.iter().flatten().any(|m| want.contains(m)) => {
                for o in &op.operands {
                    match o {
                        Object::String(b, _) => s.push_str(&pdfobj::decode_text(b)),
                        Object::Array(a) => {
                            for x in a {
                                match x {
                                    Object::String(b, _) => s.push_str(&pdfobj::decode_text(b)),
                                    Object::Integer(v) if *v < -200 => s.push(' '),
                                    Object::Real(v) if *v < -200.0 => s.push(' '),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    s.chars().filter(|c| !c.is_control()).collect()
}

// ---------------------------------------------------------------- figures

/// Một hình (phần tử Figure) trong cây cấu trúc.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureInfo {
    /// "số thế_hệ" của object phần tử cấu trúc.
    pub id: String,
    pub page_index: Option<u16>,
    /// Hộp bao nội dung của hình trên trang (điểm PDF) nếu tính được.
    pub rect: Option<[f32; 4]>,
    pub alt: String,
    pub actual_text: String,
}

pub fn list_figures(input: &Path) -> Result<Vec<FigureInfo>, EngineError> {
    let doc = pdfobj::load(input)?;
    let elems = struct_elems(&doc);
    let pages = pdfobj::page_ids(&doc);
    let mut boxes_cache: BTreeMap<ObjectId, BTreeMap<i64, [f32; 4]>> = BTreeMap::new();
    let mut out = Vec::new();
    for e in elems.iter().filter(|e| e.role == "Figure") {
        let Some(id) = e.id else { continue };
        // MCID có thể nằm ở phần tử con.
        let mut mcids = Vec::new();
        collect_mcids(&elems, &elems.iter().position(|x| x.id == Some(id)).unwrap(), &mut mcids);
        let pg = e.page.or_else(|| mcids.iter().find_map(|(p, _)| *p));
        let mut rect: Option<[f32; 4]> = None;
        if let Some(p) = pg {
            let boxes = boxes_cache.entry(p).or_insert_with(|| mcid_boxes(&doc, p));
            for (mp, m) in &mcids {
                if mp.unwrap_or(p) != p {
                    continue;
                }
                if let Some(b) = boxes.get(m) {
                    rect = Some(match rect {
                        None => *b,
                        Some(r) => [r[0].min(b[0]), r[1].min(b[1]), r[2].max(b[2]), r[3].max(b[3])],
                    });
                }
            }
        }
        out.push(FigureInfo {
            id: format!("{} {}", id.0, id.1),
            page_index: pg.and_then(|p| pages.iter().position(|x| *x == p)).map(|i| i as u16),
            rect,
            alt: e.alt.clone().unwrap_or_default(),
            actual_text: e.actual.clone().unwrap_or_default(),
        });
    }
    Ok(out)
}

fn collect_mcids(e: &[Elem], i: &usize, out: &mut Vec<(Option<ObjectId>, i64)>) {
    out.extend(e[*i].mcids.iter().copied());
    for c in &e[*i].children {
        collect_mcids(e, c, out);
    }
}

type M = [f32; 6];
fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}
fn apply(m: &M, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Hộp bao theo MCID trên 1 trang: theo dõi CTM, cộng dồn ảnh/Form (Do,
/// BI), path và gốc chữ trong từng vùng marked-content có MCID.
fn mcid_boxes(doc: &Document, pid: ObjectId) -> BTreeMap<i64, [f32; 4]> {
    let mut out: BTreeMap<i64, [f32; 4]> = BTreeMap::new();
    let Ok(bytes) = doc.get_page_content(pid) else { return out };
    let Ok(c) = Content::decode(&bytes) else { return out };
    let res = {
        let mut cur = Some(pid);
        let mut r = None;
        for _ in 0..32 {
            let Some(id) = cur else { break };
            let Ok(d) = doc.get_dictionary(id) else { break };
            if let Some(x) = dict_of(doc, d, b"Resources") {
                r = Some(x.clone());
                break;
            }
            cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        }
        r
    };
    let xobjs = res.as_ref().and_then(|r| dict_of(doc, r, b"XObject")).cloned();
    let props = res.as_ref().and_then(|r| dict_of(doc, r, b"Properties")).cloned();
    let mut ctm: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut gstack: Vec<M> = Vec::new();
    let mut mc: Vec<Option<i64>> = Vec::new();
    let mut path: Vec<(f32, f32)> = Vec::new();
    let mut tm: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut tlm: M = tm;
    let mut fsize = 12.0f32;
    let add = |mc: &[Option<i64>], pts: &[(f32, f32)], out: &mut BTreeMap<i64, [f32; 4]>| {
        let Some(m) = mc.iter().rev().flatten().next() else { return };
        for &(x, y) in pts {
            let e = out.entry(*m).or_insert([x, y, x, y]);
            e[0] = e[0].min(x);
            e[1] = e[1].min(y);
            e[2] = e[2].max(x);
            e[3] = e[3].max(y);
        }
    };
    let nums = |ops: &[Object]| -> Vec<f32> { ops.iter().filter_map(pdfobj::num).collect() };
    for op in &c.operations {
        let v = nums(&op.operands);
        match op.operator.as_str() {
            "q" => gstack.push(ctm),
            "Q" => ctm = gstack.pop().unwrap_or(ctm),
            "cm" if v.len() == 6 => ctm = mul(&[v[0], v[1], v[2], v[3], v[4], v[5]], &ctm),
            "BDC" => {
                let mcid = match op.operands.get(1) {
                    Some(Object::Dictionary(d)) => d.get(b"MCID").ok().and_then(|o| o.as_i64().ok()),
                    Some(Object::Name(n)) => props
                        .as_ref()
                        .and_then(|p| p.get(n).ok())
                        .and_then(|o| obj_dict(deref(doc, o)))
                        .and_then(|d| d.get(b"MCID").ok().and_then(|o| o.as_i64().ok())),
                    _ => None,
                };
                mc.push(mcid);
            }
            "BMC" => mc.push(None),
            "EMC" => {
                mc.pop();
            }
            "m" | "l" if v.len() >= 2 => path.push(apply(&ctm, v[0], v[1])),
            "c" if v.len() >= 6 => {
                for k in 0..3 {
                    path.push(apply(&ctm, v[2 * k], v[2 * k + 1]));
                }
            }
            "v" | "y" if v.len() >= 4 => {
                path.push(apply(&ctm, v[0], v[1]));
                path.push(apply(&ctm, v[2], v[3]));
            }
            "re" if v.len() >= 4 => {
                for (x, y) in [(v[0], v[1]), (v[0] + v[2], v[1]), (v[0], v[1] + v[3]), (v[0] + v[2], v[1] + v[3])] {
                    path.push(apply(&ctm, x, y));
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                add(&mc, &path, &mut out);
                path.clear();
            }
            "n" => path.clear(),
            "BI" => {
                let pts: Vec<(f32, f32)> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].iter().map(|&(x, y)| apply(&ctm, x, y)).collect();
                add(&mc, &pts, &mut out);
            }
            "Do" => {
                let xd = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .and_then(|n| xobjs.as_ref().and_then(|x| x.get(n).ok()))
                    .and_then(|o| obj_dict(deref(doc, o)))
                    .cloned();
                let Some(xd) = xd else { continue };
                let is_form = xd.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Form");
                let (bb, m): ([f32; 4], M) = if is_form {
                    let bb = pdfobj::rect_of(doc, &xd, b"BBox").unwrap_or([0.0, 0.0, 1.0, 1.0]);
                    let mm = match dict_get(doc, &xd, b"Matrix") {
                        Some(Object::Array(a)) => {
                            let n: Vec<f32> = a.iter().filter_map(pdfobj::num).collect();
                            if n.len() == 6 { [n[0], n[1], n[2], n[3], n[4], n[5]] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
                        }
                        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    };
                    (bb, mul(&mm, &ctm))
                } else {
                    ([0.0, 0.0, 1.0, 1.0], ctm)
                };
                let pts: Vec<(f32, f32)> = [(bb[0], bb[1]), (bb[2], bb[1]), (bb[0], bb[3]), (bb[2], bb[3])]
                    .iter()
                    .map(|&(x, y)| apply(&m, x, y))
                    .collect();
                add(&mc, &pts, &mut out);
            }
            "BT" => {
                tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                tlm = tm;
            }
            "Tf" if v.len() == 1 => fsize = v[0],
            "Tm" if v.len() == 6 => {
                tm = [v[0], v[1], v[2], v[3], v[4], v[5]];
                tlm = tm;
            }
            "Td" | "TD" if v.len() == 2 => {
                tlm = mul(&[1.0, 0.0, 0.0, 1.0, v[0], v[1]], &tlm);
                tm = tlm;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                let m = mul(&tm, &ctm);
                let pts = [apply(&m, 0.0, 0.0), apply(&m, 0.0, fsize), apply(&m, fsize, 0.0)];
                add(&mc, &pts, &mut out);
            }
            _ => {}
        }
    }
    out
}

/// Ghi /Alt cho các phần tử (id "n g"); chuỗi rỗng = gỡ /Alt.
pub fn set_alt_texts(input: &Path, output: &Path, alts: &[(String, String)]) -> Result<usize, EngineError> {
    let mut doc = pdfobj::load(input)?;
    let mut n = 0;
    for (id, alt) in alts {
        let mut it = id.split_whitespace();
        let (Some(a), Some(b)) = (it.next().and_then(|x| x.parse::<u32>().ok()), it.next().and_then(|x| x.parse::<u16>().ok())) else {
            continue;
        };
        if let Ok(d) = doc.get_dictionary_mut((a, b)) {
            if alt.trim().is_empty() {
                d.remove(b"Alt");
            } else {
                d.set("Alt", pdfobj::text_obj(alt.trim()));
            }
            n += 1;
        }
    }
    pdfobj::save(&mut doc, output)?;
    Ok(n)
}
