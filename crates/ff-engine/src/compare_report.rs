//! Xuất "Báo cáo so sánh" (như Foxit "Compare Result"): bản sao tài liệu MỚI
//! có chú thích đánh dấu mọi thay đổi (chèn = tô xanh lá, thay = tô cam, xoá =
//! ghi chú đỏ chứa chữ bị xoá, đồ hoạ = khung xanh dương, trang chèn = khung
//! xanh lá) + trang bìa tóm tắt (số lượng theo loại, danh sách thay đổi có
//! liên kết nhảy tới trang). Chú thích dựng qua `annot::apply_annotations`.

use std::collections::BTreeMap;
use std::path::Path;

use lopdf::{Dictionary, Document as LoDoc, Object as LoObj, ObjectId, StringFormat};
use pdfium_render::prelude::*;

use crate::annot::{apply_annotations, embed_type0_font, encode_cid, find_font_bytes, AnnotKind, AnnotSpec};
use crate::compare::{Change, ChangeKind, CompareResult};
use crate::text::Rect;
use crate::EngineError;

/// Nhãn (đã dịch theo ngôn ngữ giao diện) dùng trong trang bìa & nội dung chú thích.
#[derive(Clone, Debug)]
pub struct ReportLabels {
    pub title: String,
    pub old_file: String,
    pub new_file: String,
    /// Dòng ngày giờ hoàn chỉnh (UI định dạng), vd "Date: 2026-09-30 10:15".
    pub date_line: String,
    pub summary: String,
    pub total: String,
    pub details: String,
    pub col_no: String,
    pub col_type: String,
    pub col_old_page: String,
    pub col_new_page: String,
    pub col_content: String,
    pub kind_inserted: String,
    pub kind_deleted: String,
    pub kind_replaced: String,
    pub kind_page_inserted: String,
    pub kind_page_deleted: String,
    pub kind_graphics: String,
    /// "… và {n} thay đổi khác" — `{n}` được thay.
    pub more: String,
    pub no_diff: String,
    /// Tác giả (/T) của chú thích.
    pub author: String,
}

impl Default for ReportLabels {
    fn default() -> Self {
        let s = |x: &str| x.to_string();
        ReportLabels {
            title: s("Compare Result"),
            old_file: s("Old file"),
            new_file: s("New file"),
            date_line: String::new(),
            summary: s("Summary"),
            total: s("Total"),
            details: s("Differences"),
            col_no: s("#"),
            col_type: s("Type"),
            col_old_page: s("Old p."),
            col_new_page: s("New p."),
            col_content: s("Content"),
            kind_inserted: s("Inserted"),
            kind_deleted: s("Deleted"),
            kind_replaced: s("Replaced"),
            kind_page_inserted: s("Page inserted"),
            kind_page_deleted: s("Page deleted"),
            kind_graphics: s("Graphics changed"),
            more: s("… and {n} more"),
            no_diff: s("No differences found."),
            author: s("FoFreeXit Compare"),
        }
    }
}

impl ReportLabels {
    fn kind(&self, k: ChangeKind) -> &str {
        match k {
            ChangeKind::Inserted => &self.kind_inserted,
            ChangeKind::Deleted => &self.kind_deleted,
            ChangeKind::Replaced => &self.kind_replaced,
            ChangeKind::PageInserted => &self.kind_page_inserted,
            ChangeKind::PageDeleted => &self.kind_page_deleted,
            ChangeKind::Graphics => &self.kind_graphics,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReportInfo {
    pub old_name: String,
    pub new_name: String,
    pub labels: ReportLabels,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportStats {
    pub cover_pages: usize,
    /// Số chú thích đánh dấu thay đổi đã thêm vào các trang của tài liệu mới.
    pub annotations: usize,
    /// Số liên kết (dòng danh sách → trang) trên trang bìa.
    pub links: usize,
}

/// Màu thống nhất với UI so sánh (RGB).
pub fn kind_rgb(k: ChangeKind) -> [u8; 3] {
    match k {
        ChangeKind::Inserted | ChangeKind::PageInserted => [46, 160, 67],
        ChangeKind::Deleted | ChangeKind::PageDeleted => [218, 54, 51],
        ChangeKind::Replaced => [240, 136, 0],
        ChangeKind::Graphics => [47, 129, 247],
    }
}

fn text_string(s: &str) -> LoObj {
    if s.is_ascii() {
        LoObj::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        LoObj::String(bytes, StringFormat::Hexadecimal)
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n).collect();
        t.push('…');
        t
    }
}

fn union_all(rs: &[Rect]) -> Rect {
    let mut u = rs[0];
    for r in &rs[1..] {
        u = Rect { left: u.left.min(r.left), bottom: u.bottom.min(r.bottom), right: u.right.max(r.right), top: u.top.max(r.top) };
    }
    u
}

/// Nội dung popup của chú thích cho một thay đổi.
fn annot_contents(c: &Change, l: &ReportLabels) -> String {
    let k = l.kind(c.kind);
    match c.kind {
        ChangeKind::Replaced => format!("{k}: \"{}\" → \"{}\"", clip(&c.old_text, 500), clip(&c.new_text, 500)),
        ChangeKind::Inserted | ChangeKind::PageInserted => {
            if c.new_text.is_empty() { k.to_string() } else { format!("{k}: \"{}\"", clip(&c.new_text, 800)) }
        }
        ChangeKind::Deleted | ChangeKind::PageDeleted => {
            let pg = c.old_page.map(|p| format!(" (p.{})", p + 1)).unwrap_or_default();
            if c.old_text.is_empty() { format!("{k}{pg}") } else { format!("{k}{pg}: \"{}\"", clip(&c.old_text, 800)) }
        }
        ChangeKind::Graphics => k.to_string(),
    }
}

/// Số phần tử /Annots của trang (PDFium ghi annotation dạng dict TRỰC TIẾP
/// trong mảng, lopdf ghi dạng tham chiếu — đếm cả hai).
fn annot_count(doc: &LoDoc, page_id: ObjectId) -> usize {
    let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict()) else { return 0 };
    match page.get(b"Annots") {
        Ok(LoObj::Array(a)) => a.len(),
        Ok(LoObj::Reference(r)) => doc.get_object(*r).and_then(|o| o.as_array()).map(|a| a.len()).unwrap_or(0),
        _ => 0,
    }
}

/// Gọi `f(dict)` cho mọi annotation từ vị trí `skip` trở đi của trang — dù là
/// dict trực tiếp trong mảng /Annots hay tham chiếu tới object riêng.
fn edit_annots(doc: &mut LoDoc, page_id: ObjectId, skip: usize, mut f: impl FnMut(&mut Dictionary)) {
    let arr_ref = match doc.get_object(page_id).and_then(|o| o.as_dict()).and_then(|p| p.get(b"Annots")) {
        Ok(LoObj::Reference(r)) => Some(*r),
        Ok(LoObj::Array(_)) => None,
        _ => return,
    };
    let mut arr = match arr_ref {
        Some(r) => doc.get_object(r).and_then(|o| o.as_array()).cloned().unwrap_or_default(),
        None => doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .and_then(|p| p.get(b"Annots"))
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default(),
    };
    let mut inline_changed = false;
    for item in arr.iter_mut().skip(skip) {
        match item {
            LoObj::Reference(id) => {
                if let Ok(d) = doc.get_object_mut(*id).and_then(|o| o.as_dict_mut()) {
                    f(d);
                }
            }
            LoObj::Dictionary(d) => {
                f(d);
                inline_changed = true;
            }
            _ => {}
        }
    }
    if inline_changed {
        match arr_ref {
            Some(r) => {
                if let Ok(o) = doc.get_object_mut(r) {
                    *o = LoObj::Array(arr);
                }
            }
            None => {
                if let Ok(p) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                    p.set("Annots", LoObj::Array(arr));
                }
            }
        }
    }
}

fn subtype_is(d: &Dictionary, names: &[&[u8]]) -> bool {
    d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| names.contains(&n)).unwrap_or(false)
}

// ---- Trang bìa ----

enum Draw {
    Text { x: f32, y: f32, size: f32, bold: bool, rgb: [u8; 3], s: String },
    Fill { x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3] },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, rgb: [u8; 3] },
}

#[derive(Default)]
struct CoverPage {
    draws: Vec<Draw>,
    /// (hộp dòng, trang đích trong tài liệu mới, toạ độ top để nhảy tới)
    links: Vec<(Rect, u16, Option<f32>)>,
}

struct Fonts<'a> {
    reg: Option<ttf_parser::Face<'a>>,
    bold: Option<ttf_parser::Face<'a>>,
}

impl Fonts<'_> {
    fn face(&self, bold: bool) -> Option<&ttf_parser::Face<'_>> {
        if bold { self.bold.as_ref().or(self.reg.as_ref()) } else { self.reg.as_ref() }
    }
    fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        match self.face(bold) {
            Some(f) => {
                let upem = f.units_per_em() as f32;
                s.chars()
                    .map(|c| {
                        let g = f.glyph_index(c).or_else(|| f.glyph_index('?')).unwrap_or(ttf_parser::GlyphId(0));
                        f.glyph_hor_advance(g).unwrap_or(500) as f32 * size / upem
                    })
                    .sum()
            }
            None => s.chars().count() as f32 * size * if bold { 0.58 } else { 0.53 },
        }
    }
    fn fit(&self, s: &str, size: f32, bold: bool, max_w: f32) -> String {
        if self.width(s, size, bold) <= max_w {
            return s.to_string();
        }
        let chars: Vec<char> = s.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let t: String = chars[..mid].iter().collect::<String>() + "…";
            if self.width(&t, size, bold) <= max_w { lo = mid } else { hi = mid - 1 }
        }
        chars[..lo].iter().collect::<String>() + "…"
    }
    fn arrow(&self) -> &'static str {
        match self.reg.as_ref() {
            Some(f) if f.glyph_index('→').is_some() => " → ",
            _ => " -> ",
        }
    }
}

fn layout_cover(result: &CompareResult, info: &ReportInfo, fonts: &Fonts, pw: f32, ph: f32) -> Vec<CoverPage> {
    let l = &info.labels;
    let m = 48.0;
    let ink = [30, 30, 30];
    let muted = [110, 110, 110];
    let mut pages = vec![CoverPage::default()];
    let mut y = ph - m - 20.0;
    let cur = |pages: &mut Vec<CoverPage>| pages.len() - 1;
    let txt = |x: f32, y: f32, size: f32, bold: bool, rgb: [u8; 3], s: String| Draw::Text { x, y, size, bold, rgb, s };

    // Tiêu đề + tệp.
    {
        let p = &mut pages[0].draws;
        p.push(Draw::Fill { x: m, y: y + 26.0, w: pw - 2.0 * m, h: 3.0, rgb: [47, 129, 247] });
        p.push(txt(m, y, 20.0, true, ink, fonts.fit(&l.title, 20.0, true, pw - 2.0 * m)));
        y -= 28.0;
        for (label, name) in [(&l.old_file, &info.old_name), (&l.new_file, &info.new_name)] {
            let lab = format!("{label}: ");
            let lw = fonts.width(&lab, 10.0, true);
            p.push(txt(m, y, 10.0, true, ink, lab));
            p.push(txt(m + lw, y, 10.0, false, ink, fonts.fit(name, 10.0, false, pw - 2.0 * m - lw)));
            y -= 15.0;
        }
        if !l.date_line.is_empty() {
            p.push(txt(m, y, 10.0, false, muted, fonts.fit(&l.date_line, 10.0, false, pw - 2.0 * m)));
            y -= 15.0;
        }
        y -= 14.0;
        // Tóm tắt.
        p.push(txt(m, y, 13.0, true, ink, l.summary.clone()));
        y -= 20.0;
        let s = &result.summary;
        let rows = [
            (ChangeKind::Replaced, s.replaced),
            (ChangeKind::Inserted, s.inserted),
            (ChangeKind::Deleted, s.deleted),
            (ChangeKind::PageInserted, s.page_inserted),
            (ChangeKind::PageDeleted, s.page_deleted),
            (ChangeKind::Graphics, s.graphics),
        ];
        let col = m + 230.0;
        for (k, n) in rows {
            p.push(Draw::Fill { x: m, y: y - 1.0, w: 10.0, h: 10.0, rgb: kind_rgb(k) });
            p.push(txt(m + 18.0, y, 10.0, false, ink, l.kind(k).to_string()));
            let ns = n.to_string();
            p.push(txt(col - fonts.width(&ns, 10.0, false), y, 10.0, false, ink, ns));
            y -= 16.0;
        }
        p.push(Draw::Line { x0: m, y0: y + 11.0, x1: col, y1: y + 11.0, rgb: muted });
        p.push(txt(m + 18.0, y - 2.0, 10.0, true, ink, l.total.clone()));
        let ts = s.total().to_string();
        p.push(txt(col - fonts.width(&ts, 10.0, true), y - 2.0, 10.0, true, ink, ts));
        y -= 34.0;
        p.push(txt(m, y, 13.0, true, ink, l.details.clone()));
        y -= 20.0;
    }

    if result.changes.is_empty() {
        pages[0].draws.push(txt(m, y, 10.0, false, muted, l.no_diff.clone()));
        return pages;
    }

    // Bảng danh sách thay đổi (phân trang).
    let (x_no, x_type, x_old, x_new, x_ct) = (m, m + 30.0, m + 138.0, m + 184.0, m + 232.0);
    let right = pw - m;
    let fs = 8.5;
    let row_h = 13.5;
    let header = |p: &mut CoverPage, y: f32| {
        for (x, s) in [(x_no, &l.col_no), (x_type, &l.col_type), (x_old, &l.col_old_page), (x_new, &l.col_new_page), (x_ct, &l.col_content)] {
            p.draws.push(Draw::Text { x, y, size: fs, bold: true, rgb: ink, s: s.clone() });
        }
        p.draws.push(Draw::Line { x0: m, y0: y - 4.0, x1: right, y1: y - 4.0, rgb: muted });
    };
    let c0 = cur(&mut pages);
    header(&mut pages[c0], y);
    y -= row_h + 2.0;
    const MAX_ROWS: usize = 3000;
    let arrow = fonts.arrow();
    for (i, c) in result.changes.iter().enumerate() {
        if i >= MAX_ROWS {
            let rest = result.changes.len() - MAX_ROWS;
            let ci = cur(&mut pages);
            pages[ci].draws.push(txt(m, y, fs, false, muted, l.more.replace("{n}", &rest.to_string())));
            break;
        }
        if y < m {
            pages.push(CoverPage::default());
            y = ph - m - 10.0;
            let ci = cur(&mut pages);
            header(&mut pages[ci], y);
            y -= row_h + 2.0;
        }
        let ci = cur(&mut pages);
        let p = &mut pages[ci];
        if i % 2 == 1 {
            p.draws.push(Draw::Fill { x: m - 2.0, y: y - 3.5, w: right - m + 4.0, h: row_h, rgb: [244, 246, 250] });
        }
        let pg = |v: Option<u16>| v.map(|n| (n + 1).to_string()).unwrap_or_else(|| "—".into());
        let content = match c.kind {
            ChangeKind::Replaced => format!("{}{arrow}{}", c.old_text, c.new_text),
            ChangeKind::Inserted | ChangeKind::PageInserted => c.new_text.clone(),
            ChangeKind::Deleted | ChangeKind::PageDeleted => c.old_text.clone(),
            ChangeKind::Graphics => String::new(),
        };
        let content: String = content.chars().take(400).collect();
        p.draws.push(txt(x_no, y, fs, false, muted, (i + 1).to_string()));
        p.draws.push(Draw::Fill { x: x_type, y: y - 0.5, w: 7.0, h: 7.0, rgb: kind_rgb(c.kind) });
        p.draws.push(txt(x_type + 11.0, y, fs, false, ink, fonts.fit(l.kind(c.kind), fs, false, x_old - x_type - 14.0)));
        let old_pg = if matches!(c.kind, ChangeKind::PageInserted) { "—".to_string() } else { pg(c.old_page) };
        let new_pg = if matches!(c.kind, ChangeKind::PageDeleted) { "—".to_string() } else { pg(c.new_page) };
        p.draws.push(txt(x_old, y, fs, false, ink, old_pg));
        p.draws.push(txt(x_new, y, fs, false, ink, new_pg));
        p.draws.push(txt(x_ct, y, fs, false, ink, fonts.fit(&content, fs, false, right - x_ct)));
        if let Some(np) = c.new_page {
            let top = c.new_rects.first().map(|r| r.top + 36.0);
            p.links.push((Rect { left: m, bottom: y - 3.5, right, top: y - 3.5 + row_h }, np, top));
        }
        y -= row_h;
    }
    pages
}

/// Mã hoá chuỗi cho toán tử Tj theo font đang dùng.
fn encode(s: &str, face: Option<&ttf_parser::Face>) -> String {
    match face {
        Some(f) => {
            let b = encode_cid(s, f);
            let mut h = String::with_capacity(b.len() * 2 + 2);
            h.push('<');
            for x in b {
                h.push_str(&format!("{x:02X}"));
            }
            h.push('>');
            h
        }
        None => {
            // WinAnsi (Helvetica dự phòng): Latin-1 + dấu "…"; còn lại '?'.
            let mut h = String::from("<");
            for c in s.chars() {
                let b = match c {
                    '…' => 0x85,
                    '—' => 0x97,
                    c if (c as u32) < 0x80 || (0xA0..=0xFF).contains(&(c as u32)) => c as u32 as u8,
                    _ => b'?',
                };
                h.push_str(&format!("{b:02X}"));
            }
            h.push('>');
            h
        }
    }
}

fn rgb_op(c: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

fn cover_content(p: &CoverPage, fonts: &Fonts) -> Vec<u8> {
    let mut s = String::new();
    for d in &p.draws {
        match d {
            Draw::Fill { x, y, w, h, rgb } => s.push_str(&format!("{} rg {x:.2} {y:.2} {w:.2} {h:.2} re f\n", rgb_op(*rgb))),
            Draw::Line { x0, y0, x1, y1, rgb } => {
                s.push_str(&format!("{} RG 0.6 w {x0:.2} {y0:.2} m {x1:.2} {y1:.2} l S\n", rgb_op(*rgb)))
            }
            Draw::Text { x, y, size, bold, rgb, s: t } => {
                let font = if *bold { "F2" } else { "F1" };
                let face = fonts.face(*bold);
                s.push_str(&format!(
                    "BT /{font} {size:.2} Tf {} rg {x:.2} {y:.2} Td {} Tj ET\n",
                    rgb_op(*rgb),
                    encode(t, face)
                ));
            }
        }
    }
    s.into_bytes()
}

fn type1(doc: &mut LoDoc, name: &str) -> ObjectId {
    let mut d = Dictionary::new();
    d.set("Type", LoObj::Name(b"Font".to_vec()));
    d.set("Subtype", LoObj::Name(b"Type1".to_vec()));
    d.set("BaseFont", LoObj::Name(name.as_bytes().to_vec()));
    d.set("Encoding", LoObj::Name(b"WinAnsiEncoding".to_vec()));
    doc.add_object(LoObj::Dictionary(d))
}

fn rect_arr(r: &Rect) -> LoObj {
    LoObj::Array(vec![LoObj::Real(r.left), LoObj::Real(r.bottom), LoObj::Real(r.right), LoObj::Real(r.top)])
}

/// Xuất báo cáo so sánh ra `output` (không sửa tệp nguồn).
pub fn export_compare_report(
    pdfium: &Pdfium,
    result: &CompareResult,
    output: &Path,
    info: &ReportInfo,
) -> Result<ReportStats, EngineError> {
    let l = &info.labels;
    let lo_err = |e: lopdf::Error| EngineError::Pdfium(format!("lopdf: {e}"));

    // 1. Chú thích trên bản sao tài liệu mới.
    let mut specs: Vec<AnnotSpec> = Vec::new();
    // Nội dung popup của markup (PDFium pass không ghi Contents) theo trang, đúng thứ tự tạo.
    let mut markup_meta: BTreeMap<u16, Vec<(String, String, [u8; 3])>> = BTreeMap::new();
    let size_of = |p: u16| result.new_sizes.get(p as usize).copied().unwrap_or((612.0, 792.0));
    for c in &result.changes {
        let Some(np) = c.new_page else { continue };
        let (pw, ph) = size_of(np);
        let rgb = kind_rgb(c.kind);
        let contents = annot_contents(c, l);
        match c.kind {
            ChangeKind::Inserted | ChangeKind::Replaced => {
                if c.new_rects.is_empty() {
                    continue;
                }
                let fill = if c.kind == ChangeKind::Inserted { [120, 220, 130, 255] } else { [255, 185, 70, 255] };
                let mut s = AnnotSpec::markup(AnnotKind::Highlight, np, union_all(&c.new_rects), fill);
                s.quads = c.new_rects.clone();
                specs.push(s);
                markup_meta.entry(np).or_default().push((contents, l.kind(c.kind).to_string(), [fill[0], fill[1], fill[2]]));
            }
            ChangeKind::Graphics | ChangeKind::PageInserted => {
                let r = if c.kind == ChangeKind::PageInserted {
                    Rect { left: 3.0, bottom: 3.0, right: pw - 3.0, top: ph - 3.0 }
                } else {
                    match c.new_rects.first() {
                        Some(r) => *r,
                        None => continue,
                    }
                };
                specs.push(AnnotSpec::markup(AnnotKind::Square, np, r, [rgb[0], rgb[1], rgb[2], 255]));
                markup_meta.entry(np).or_default().push((contents, l.kind(c.kind).to_string(), rgb));
            }
            ChangeKind::Deleted | ChangeKind::PageDeleted => {
                let (x, top) = match c.new_rects.first() {
                    Some(r) if c.kind == ChangeKind::Deleted => (r.left - 9.0, r.top + 16.0),
                    _ => (8.0, ph - 8.0),
                };
                let x = x.clamp(0.0, (pw - 18.0).max(0.0));
                let top = top.clamp(18.0, ph);
                let mut s = AnnotSpec::markup(
                    AnnotKind::Note,
                    np,
                    Rect { left: x, bottom: top - 18.0, right: x + 18.0, top },
                    [rgb[0], rgb[1], rgb[2], 255],
                );
                s.contents = Some(contents);
                specs.push(s);
            }
        }
    }
    // Số chú thích sẵn có từng trang (để nhận ra chú thích mới tạo ở đuôi /Annots).
    let pre: BTreeMap<u32, usize> = match LoDoc::load(&result.new_path) {
        Ok(d) => d.get_pages().into_iter().map(|(n, id)| (n, annot_count(&d, id))).collect(),
        Err(_) => BTreeMap::new(),
    };
    apply_annotations(pdfium, &result.new_path, output, &specs)?;

    // 2. Hậu kỳ lopdf: Contents/T/Subj cho markup, rồi chèn trang bìa.
    let mut doc = LoDoc::load(output).map_err(lo_err)?;
    let page_ids = doc.get_pages();
    for (page, metas) in &markup_meta {
        let Some(&pid) = page_ids.get(&(*page as u32 + 1)) else { continue };
        let skip = pre.get(&(*page as u32 + 1)).copied().unwrap_or(0);
        let mut it = metas.iter();
        edit_annots(&mut doc, pid, skip, |d| {
            if !subtype_is(d, &[b"Highlight", b"Square"]) {
                return;
            }
            let Some((contents, subj, rgb)) = it.next() else { return };
            d.set("Contents", text_string(contents));
            d.set("T", text_string(&l.author));
            d.set("Subj", text_string(subj));
            // /C cho trình xem khác (Acrobat/Foxit dựng lại giao diện từ /C).
            let c: Vec<LoObj> = rgb.iter().map(|v| LoObj::Real(*v as f32 / 255.0)).collect();
            d.set("C", LoObj::Array(c));
        });
    }
    // Ghi chú (Note) do pass lopdf của annot.rs tạo: gắn tác giả + biểu tượng
    // ĐỎ tự vẽ (PDFium tự sinh icon Note màu vàng, bỏ qua /C) như Foxit.
    let note_ap = doc.add_object(LoObj::Stream(lopdf::Stream::new(
        {
            let mut d = Dictionary::new();
            d.set("Type", LoObj::Name(b"XObject".to_vec()));
            d.set("Subtype", LoObj::Name(b"Form".to_vec()));
            d.set("BBox", LoObj::Array(vec![0.into(), 0.into(), 18.into(), 18.into()]));
            d
        },
        format!(
            "q {} rg 0.35 0.08 0.08 RG 0.8 w 1 4.5 16 12.5 re B 4 4.5 m 5.5 0.8 l 9 4.5 l f              1 1 1 RG 1.3 w 4 13.5 m 14 13.5 l S 4 10.5 m 14 10.5 l S 4 7.5 m 11 7.5 l S Q",
            rgb_op(kind_rgb(ChangeKind::Deleted))
        )
        .into_bytes(),
    )));
    for (n, pid) in &page_ids {
        let skip = pre.get(n).copied().unwrap_or(0);
        edit_annots(&mut doc, *pid, skip, |d| {
            if subtype_is(d, &[b"Text"]) && !d.has(b"T") {
                d.set("T", text_string(&l.author));
                d.set("Subj", text_string(&l.kind_deleted));
                let mut ap = Dictionary::new();
                ap.set("N", LoObj::Reference(note_ap));
                d.set("AP", LoObj::Dictionary(ap));
            }
        });
    }

    // 3. Trang bìa.
    let (pw, ph) = match result.new_sizes.first() {
        Some(&(w, h)) if w >= 400.0 && h >= 500.0 => (w, h),
        _ => (595.0, 842.0),
    };
    let reg_bytes = find_font_bytes(false, false);
    let bold_bytes = find_font_bytes(true, false);
    let fonts = Fonts {
        reg: reg_bytes.as_deref().and_then(|b| ttf_parser::Face::parse(b, 0).ok()),
        bold: bold_bytes.as_deref().and_then(|b| ttf_parser::Face::parse(b, 0).ok()),
    };
    let cover = layout_cover(result, info, &fonts, pw, ph);
    let mut used = String::new();
    for p in &cover {
        for d in &p.draws {
            if let Draw::Text { s, .. } = d {
                used.push_str(s);
            }
        }
    }
    let (f1, f2) = match (&fonts.reg, &reg_bytes) {
        (Some(_), Some(rb)) => {
            let f1 = embed_type0_font(&mut doc, rb, &used)?;
            let f2 = match (&fonts.bold, &bold_bytes) {
                (Some(_), Some(bb)) => embed_type0_font(&mut doc, bb, &used)?,
                _ => f1,
            };
            (f1, f2)
        }
        _ => (type1(&mut doc, "Helvetica"), type1(&mut doc, "Helvetica-Bold")),
    };
    let pages_id = doc
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(|p| p.as_reference())
        .map_err(lo_err)?;
    let mut links = 0usize;
    let mut cover_ids = Vec::new();
    for p in &cover {
        let content_id = doc.add_object(LoObj::Stream(lopdf::Stream::new(Dictionary::new(), cover_content(p, &fonts))));
        let mut font_dict = Dictionary::new();
        font_dict.set("F1", LoObj::Reference(f1));
        font_dict.set("F2", LoObj::Reference(f2));
        let mut res = Dictionary::new();
        res.set("Font", LoObj::Dictionary(font_dict));
        let mut annots = Vec::new();
        for (r, target, top) in &p.links {
            let Some(&tid) = page_ids.get(&(*target as u32 + 1)) else { continue };
            let mut a = Dictionary::new();
            a.set("Type", LoObj::Name(b"Annot".to_vec()));
            a.set("Subtype", LoObj::Name(b"Link".to_vec()));
            a.set("Rect", rect_arr(r));
            a.set("Border", LoObj::Array(vec![0.into(), 0.into(), 0.into()]));
            let th = size_of(*target).1;
            a.set(
                "Dest",
                LoObj::Array(vec![
                    LoObj::Reference(tid),
                    LoObj::Name(b"XYZ".to_vec()),
                    LoObj::Null,
                    top.map(|t| LoObj::Real(t.min(th))).unwrap_or(LoObj::Null),
                    LoObj::Null,
                ]),
            );
            annots.push(LoObj::Reference(doc.add_object(LoObj::Dictionary(a))));
            links += 1;
        }
        let mut pd = Dictionary::new();
        pd.set("Type", LoObj::Name(b"Page".to_vec()));
        pd.set("Parent", LoObj::Reference(pages_id));
        pd.set("MediaBox", LoObj::Array(vec![0.into(), 0.into(), LoObj::Real(pw), LoObj::Real(ph)]));
        pd.set("Rotate", LoObj::Integer(0));
        pd.set("Resources", LoObj::Dictionary(res));
        pd.set("Contents", LoObj::Reference(content_id));
        if !annots.is_empty() {
            pd.set("Annots", LoObj::Array(annots));
        }
        cover_ids.push(doc.add_object(LoObj::Dictionary(pd)));
    }
    {
        let pages = doc.get_object_mut(pages_id).and_then(|o| o.as_dict_mut()).map_err(lo_err)?;
        let count = pages.get(b"Count").and_then(|c| c.as_i64()).unwrap_or(0);
        let kids = pages.get_mut(b"Kids").and_then(|k| k.as_array_mut()).map_err(lo_err)?;
        for (i, id) in cover_ids.iter().enumerate() {
            kids.insert(i, LoObj::Reference(*id));
        }
        pages.set("Count", LoObj::Integer(count + cover_ids.len() as i64));
    }
    doc.save(output).map_err(|e| EngineError::Pdfium(format!("lưu báo cáo: {e}")))?;

    Ok(ReportStats { cover_pages: cover.len(), annotations: specs.len(), links })
}
