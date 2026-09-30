//! Xuất PDF sang định dạng khác (như "Convert/Export" của Foxit): Excel .xlsx
//! (tự dựng bảng từ vị trí chữ), PowerPoint .pptx (mỗi trang 1 slide: ảnh nền +
//! hộp chữ sửa được), ảnh PNG/JPEG, TIFF nhiều trang, HTML (bố cục cố định hoặc
//! văn bản trôi) và RTF. Mọi hàm nhận `pages` (0-based; rỗng = mọi trang).
//!
//! Gói OOXML tự ghi (zip STORE của convert.rs) theo đúng bộ phần tối thiểu mà
//! Excel/PowerPoint yêu cầu: [Content_Types].xml, _rels, docProps, workbook +
//! rels + styles + sharedStrings / presentation + slideMaster + slideLayout +
//! theme + presProps/viewProps/tableStyles.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use image::DynamicImage;
use pdfium_render::prelude::*;

use crate::convert::build_zip_stored;
use crate::EngineError;

fn err(msg: impl Into<String>) -> EngineError {
    EngineError::Pdfium(msg.into())
}

/// Lọc danh sách trang hợp lệ (bỏ trùng, giữ thứ tự); rỗng → mọi trang.
pub fn resolve_pages(total: u16, pages: &[u16]) -> Vec<u16> {
    if pages.is_empty() {
        return (0..total).collect();
    }
    let mut seen = std::collections::HashSet::new();
    pages.iter().copied().filter(|&p| p < total && seen.insert(p)).collect()
}

fn open<'a>(pdfium: &'a Pdfium, input: &Path, password: Option<&'a str>) -> Result<PdfDocument<'a>, EngineError> {
    pdfium.load_pdf_from_file(input, password).map_err(|e| err(e.to_string()))
}

fn page_list(doc: &PdfDocument, pages: &[u16]) -> Result<Vec<u16>, EngineError> {
    let list = resolve_pages(doc.pages().len(), pages);
    if list.is_empty() {
        return Err(err("không có trang nào trong phạm vi đã chọn"));
    }
    Ok(list)
}

/// XML text/attribute an toàn: escape + bỏ ký tự điều khiển XML 1.0 không cho
/// phép (Excel/PowerPoint từ chối mở file chứa chúng).
pub(crate) fn xml_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => o.push(c),
        }
    }
    o
}

// ---------------------------------------------------------------------------
// Chữ trên trang: ký tự → dòng → đoạn (segment) có vị trí + kiểu chữ
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Glyph {
    ch: String,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    baseline: f32,
    size: f32,
    bold: bool,
    italic: bool,
    color: [u8; 3],
    invisible: bool,
    font: String,
}

/// Một đoạn chữ cùng kiểu.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
    pub invisible: bool,
    pub font: String,
}

/// Cụm chữ liền nhau trên 1 dòng (tách nhau bởi khoảng hở lớn → ô bảng/cột).
#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
    pub baseline: f32,
    pub size: f32,
    pub runs: Vec<Run>,
}

impl Segment {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect::<String>()
    }
    fn all_bold(&self) -> bool {
        self.runs.iter().filter(|r| !r.text.trim().is_empty()).all(|r| r.bold)
    }
}

/// Dòng thị giác (trên → dưới), các segment trái → phải.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub baseline: f32,
    pub size: f32,
    pub segments: Vec<Segment>,
}

impl Line {
    fn left(&self) -> f32 {
        self.segments.first().map(|s| s.left).unwrap_or(0.0)
    }
}

/// Tên font gọn: bỏ tiền tố subset "ABCDEF+" và hậu tố kiểu ",Bold"/"-Bold".
fn clean_font_name(name: &str) -> String {
    let n = match name.split_once('+') {
        Some((pre, rest)) if pre.len() == 6 && pre.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    };
    let n = n.split([',', '-']).next().unwrap_or(n);
    let n = n.trim_end_matches("MT").trim_end_matches("PS");
    match n {
        "" => "Arial".into(),
        "ArialMT" | "Helvetica" => "Arial".into(),
        "TimesNewRoman" | "Times" | "TimesNewRomanPS" => "Times New Roman".into(),
        "CourierNew" | "Courier" => "Courier New".into(),
        other => other.to_string(),
    }
}

fn page_glyphs(page: &PdfPage) -> Vec<Glyph> {
    let Ok(text) = page.text() else { return Vec::new() };
    let mut out = Vec::new();
    for ch in text.chars().iter() {
        let Some(s) = ch.unicode_string() else { continue };
        if s.chars().all(|c| c.is_whitespace() || c.is_control()) {
            continue; // khoảng trắng suy ra từ hình học
        }
        let Ok(b) = ch.loose_bounds().or_else(|_| ch.tight_bounds()) else { continue };
        let (left, right, bottom, top) = (b.left().value, b.right().value, b.bottom().value, b.top().value);
        let mut size = ch.scaled_font_size().value;
        if !(1.0..=500.0).contains(&size) {
            size = (top - bottom).max(4.0);
        }
        let baseline = ch.origin_y().map(|v| v.value).unwrap_or(bottom);
        let raw_font = ch.font_name();
        let weight_bold = matches!(
            ch.font_weight(),
            Some(PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900)
        );
        let lower = raw_font.to_ascii_lowercase();
        let bold = weight_bold || lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
        let italic = ch.font_is_italic() || lower.contains("italic") || lower.contains("oblique");
        let color = ch.fill_color().map(|c| [c.red(), c.green(), c.blue()]).unwrap_or([0, 0, 0]);
        let invisible = matches!(ch.render_mode(), Ok(PdfPageTextRenderMode::Invisible));
        out.push(Glyph {
            ch: s,
            left,
            right: right.max(left),
            bottom,
            top: top.max(bottom),
            baseline,
            size,
            bold,
            italic,
            color,
            invisible,
            font: clean_font_name(&raw_font),
        });
    }
    out
}

/// Gom ký tự thành dòng/segment. `split_gap_em`: khoảng hở (theo cỡ chữ) để
/// tách segment mới (≈ ranh giới cột/ô bảng).
fn build_lines(mut glyphs: Vec<Glyph>, split_gap_em: f32) -> Vec<Line> {
    glyphs.sort_by(|a, b| b.baseline.partial_cmp(&a.baseline).unwrap_or(std::cmp::Ordering::Equal));
    let mut groups: Vec<Vec<Glyph>> = Vec::new();
    for g in glyphs {
        match groups.last_mut() {
            Some(cur) => {
                let base = cur[0].baseline;
                let size = cur.iter().map(|x| x.size).fold(0.0f32, f32::max).max(g.size);
                if (base - g.baseline).abs() <= size * 0.4 {
                    cur.push(g);
                } else {
                    groups.push(vec![g]);
                }
            }
            None => groups.push(vec![g]),
        }
    }
    let mut lines = Vec::new();
    for mut gs in groups {
        gs.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal));
        // Bỏ glyph vẽ chồng (giả đậm: cùng ký tự vẽ 2 lần lệch nhẹ).
        let mut dedup: Vec<Glyph> = Vec::with_capacity(gs.len());
        for g in gs {
            if let Some(p) = dedup.last() {
                if p.ch == g.ch && (p.left - g.left).abs() < g.size * 0.12 && (p.baseline - g.baseline).abs() < 0.5 {
                    continue;
                }
            }
            dedup.push(g);
        }
        let mut segments: Vec<Segment> = Vec::new();
        let mut prev: Option<Glyph> = None;
        for g in dedup {
            let gap = prev.as_ref().map(|p| g.left - p.right).unwrap_or(0.0);
            let em = g.size.max(prev.as_ref().map(|p| p.size).unwrap_or(0.0));
            let new_seg = prev.is_none() || gap > em * split_gap_em;
            if new_seg {
                segments.push(Segment {
                    left: g.left,
                    right: g.right,
                    bottom: g.bottom,
                    top: g.top,
                    baseline: g.baseline,
                    size: g.size,
                    runs: Vec::new(),
                });
            }
            let seg = segments.last_mut().expect("có segment");
            let space = !new_seg && gap > em * 0.15;
            seg.left = seg.left.min(g.left);
            seg.right = seg.right.max(g.right);
            seg.bottom = seg.bottom.min(g.bottom);
            seg.top = seg.top.max(g.top);
            seg.size = seg.size.max(g.size);
            let same = seg.runs.last().map(|r| {
                (r.size - g.size).abs() < 0.6
                    && r.bold == g.bold
                    && r.italic == g.italic
                    && r.color == g.color
                    && r.invisible == g.invisible
                    && r.font == g.font
            });
            if space {
                if let Some(r) = seg.runs.last_mut() {
                    r.text.push(' ');
                }
            }
            if same == Some(true) {
                seg.runs.last_mut().expect("run").text.push_str(&g.ch);
            } else {
                seg.runs.push(Run {
                    text: g.ch.clone(),
                    size: g.size,
                    bold: g.bold,
                    italic: g.italic,
                    color: g.color,
                    invisible: g.invisible,
                    font: g.font.clone(),
                });
            }
            prev = Some(g);
        }
        if segments.is_empty() {
            continue;
        }
        let size = segments.iter().map(|s| s.size).fold(0.0f32, f32::max);
        let baseline = segments[0].baseline;
        lines.push(Line { baseline, size, segments });
    }
    lines
}

/// Dòng chữ của 1 trang (dùng cho bộ xuất và test).
pub(crate) fn page_lines_rich(page: &PdfPage, split_gap_em: f32) -> Vec<Line> {
    build_lines(page_glyphs(page), split_gap_em)
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

fn render(page: &PdfPage, dpi: f32) -> Result<DynamicImage, EngineError> {
    let w = (page.width().value / 72.0 * dpi).round().clamp(16.0, 12000.0) as i32;
    let cfg = PdfRenderConfig::new()
        .set_target_width(w)
        .set_maximum_height(w.saturating_mul(6))
        .render_form_data(true);
    let bmp = page.render_with_config(&cfg).map_err(|e| err(format!("render trang: {e}")))?;
    Ok(bmp.as_image())
}

/// Gỡ mọi text object (cả trong Form XObject cấp 1) khỏi trang ĐANG MỞ trong
/// bộ nhớ (không ghi file) — để render "nền không chữ" rồi đặt chữ sửa được lên.
fn strip_text_objects(page: &mut PdfPage) {
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    let n = page.objects().len();
    for i in (0..n).rev() {
        let kind = match page.objects().get(i) {
            Ok(o) => o.object_type(),
            Err(_) => continue,
        };
        match kind {
            PdfPageObjectType::Text => {
                if let Ok(o) = page.objects_mut().remove_object_at_index(i) {
                    // Bẫy Drop của PDFium (xem edit.rs) → forget.
                    std::mem::forget(o);
                }
            }
            PdfPageObjectType::XObjectForm => {
                let Ok(mut parent) = page.objects().get(i) else { continue };
                if let PdfPageObject::XObjectForm(form) = &mut parent {
                    for j in (0..form.len()).rev() {
                        let is_text = form.get(j).map(|c| c.object_type() == PdfPageObjectType::Text).unwrap_or(false);
                        if is_text {
                            if let Ok(c) = form.get(j) {
                                if let Ok(removed) = form.remove_object(c) {
                                    std::mem::forget(removed);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, EngineError> {
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100))
        .encode_image(&rgb)
        .map_err(|e| err(format!("mã hoá JPEG: {e}")))?;
    Ok(buf)
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, EngineError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| err(format!("mã hoá PNG: {e}")))?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Ảnh: PNG / JPEG / TIFF nhiều trang
// ---------------------------------------------------------------------------

/// Định dạng ảnh khi xuất từng trang.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterFormat {
    Png,
    Jpeg { quality: u8 },
}

/// PDF → mỗi trang 1 ảnh `<stem>-p<N>.<ext>` trong `out_dir` (N = số trang gốc).
pub fn export_page_images(
    pdfium: &Pdfium,
    input: &Path,
    out_dir: &Path,
    pages: &[u16],
    dpi: f32,
    format: RasterFormat,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    std::fs::create_dir_all(out_dir)?;
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".into());
    let doc = open(pdfium, input, password)?;
    let mut out = Vec::new();
    for p in page_list(&doc, pages)? {
        let page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        let img = render(&page, dpi.clamp(36.0, 1200.0))?;
        let (ext, bytes) = match format {
            RasterFormat::Png => ("png", encode_png(&img)?),
            RasterFormat::Jpeg { quality } => ("jpg", encode_jpeg(&img, quality)?),
        };
        let path = out_dir.join(format!("{stem}-p{}.{ext}", p + 1));
        std::fs::write(&path, bytes)?;
        out.push(path);
    }
    Ok(out)
}

/// PDF → 1 tệp TIFF nhiều trang (RGB, nén Deflate, ghi DPI). Trả số trang.
pub fn export_tiff(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    dpi: f32,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    use tiff::encoder::{colortype, compression::DeflateLevel, Compression, Rational, TiffEncoder};
    use tiff::tags::ResolutionUnit;
    let dpi = dpi.clamp(36.0, 1200.0);
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let file = std::fs::File::create(output)?;
    let mut enc = TiffEncoder::new(std::io::BufWriter::new(file))
        .map_err(|e| err(format!("TIFF: {e}")))?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced));
    for &p in &list {
        let page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        let rgb = render(&page, dpi)?.to_rgb8();
        let mut im = enc
            .new_image::<colortype::RGB8>(rgb.width(), rgb.height())
            .map_err(|e| err(format!("TIFF: {e}")))?;
        im.resolution(ResolutionUnit::Inch, Rational { n: dpi.round() as u32, d: 1 });
        im.write_data(rgb.as_raw()).map_err(|e| err(format!("TIFF: {e}")))?;
    }
    Ok(list.len())
}

// ---------------------------------------------------------------------------
// Excel .xlsx
// ---------------------------------------------------------------------------

/// Tên cột Excel: 0 → A, 25 → Z, 26 → AA.
fn col_name(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// Nhận diện số trong ô (1,234.5 | 1.234,5 | -12 | (12) | 12%). Giữ dạng chuỗi
/// cho mã có số 0 đứng đầu ("0123") để không mất dữ liệu.
pub fn parse_number_cell(s: &str) -> Option<f64> {
    let mut t = s.trim().replace(['\u{a0}', ' '], "");
    if t.is_empty() || t.len() > 30 {
        return None;
    }
    let mut neg = false;
    if t.starts_with('(') && t.ends_with(')') {
        neg = true;
        t = t[1..t.len() - 1].to_string();
    }
    let mut percent = false;
    if let Some(x) = t.strip_suffix('%') {
        percent = true;
        t = x.to_string();
    }
    if let Some(x) = t.strip_prefix('-').or_else(|| t.strip_prefix('\u{2212}')) {
        neg = !neg;
        t = x.to_string();
    } else if let Some(x) = t.strip_prefix('+') {
        t = x.to_string();
    }
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        return None;
    }
    if !t.chars().next()?.is_ascii_digit() || !t.chars().last()?.is_ascii_digit() {
        return None;
    }
    let digits_only = t.chars().all(|c| c.is_ascii_digit());
    if digits_only && t.len() > 1 && t.starts_with('0') {
        return None; // mã số (0123) — giữ chuỗi
    }
    let dots = t.matches('.').count();
    let commas = t.matches(',').count();
    let grouped = |sep: char| -> bool {
        let parts: Vec<&str> = t.split(sep).collect();
        parts.len() > 1 && parts[0].len() <= 3 && parts[1..].iter().all(|p| p.len() == 3)
    };
    let normalized = if dots > 0 && commas > 0 {
        // Dấu xuất hiện SAU CÙNG là dấu thập phân.
        let last_dot = t.rfind('.')?;
        let last_comma = t.rfind(',')?;
        if last_dot > last_comma {
            if dots > 1 { return None; }
            t.replace(',', "")
        } else {
            if commas > 1 { return None; }
            t.replace('.', "").replace(',', ".")
        }
    } else if commas > 0 {
        if commas > 1 || grouped(',') {
            if !grouped(',') { return None; }
            t.replace(',', "")
        } else {
            t.replace(',', ".")
        }
    } else if dots > 1 {
        if !grouped('.') { return None; }
        t.replace('.', "")
    } else {
        t.clone()
    };
    let mut v: f64 = normalized.parse().ok()?;
    if percent {
        v /= 100.0;
    }
    if neg {
        v = -v;
    }
    Some(v)
}

/// 1 ô của bảng dựng từ trang.
#[derive(Clone, Debug)]
pub struct TableCell {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub bold: bool,
}

/// Dựng bảng từ các dòng: hàng = dòng thị giác; cột = khoảng x hợp nhất của
/// các segment trên những dòng "dạng bảng" (nhiều segment).
fn table_from_lines(lines: &[Line]) -> Vec<TableCell> {
    // Số segment phổ biến nhất trong các dòng có ≥ 2 segment.
    let mut freq: std::collections::BTreeMap<usize, usize> = Default::default();
    for l in lines {
        if l.segments.len() >= 2 {
            *freq.entry(l.segments.len()).or_default() += 1;
        }
    }
    let k = freq.iter().max_by_key(|(n, c)| (**c, **n)).map(|(n, _)| *n).unwrap_or(usize::MAX);
    let mut intervals: Vec<(f32, f32)> = lines
        .iter()
        .filter(|l| l.segments.len() >= 2 && l.segments.len() >= k)
        .flat_map(|l| l.segments.iter().map(|s| (s.left, s.right)))
        .collect();
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cols: Vec<(f32, f32)> = Vec::new();
    for (l, r) in intervals {
        match cols.last_mut() {
            Some(c) if l <= c.1 + 1.5 => c.1 = c.1.max(r),
            _ => cols.push((l, r)),
        }
    }
    let col_of = |s: &Segment| -> usize {
        if cols.is_empty() {
            return 0;
        }
        // Cột chứa mép trái; không có thì cột giao nhiều nhất; rồi cột gần nhất.
        if let Some(i) = cols.iter().position(|c| s.left >= c.0 - 2.0 && s.left <= c.1 + 2.0) {
            return i;
        }
        let mut best = (0usize, 0.0f32);
        for (i, c) in cols.iter().enumerate() {
            let ov = (s.right.min(c.1) - s.left.max(c.0)).max(0.0);
            if ov > best.1 {
                best = (i, ov);
            }
        }
        if best.1 > 0.0 {
            return best.0;
        }
        cols.iter()
            .enumerate()
            .min_by(|a, b| {
                let da = (a.1 .0 - s.left).abs();
                let db = (b.1 .0 - s.left).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let mut cells: Vec<TableCell> = Vec::new();
    for (r, l) in lines.iter().enumerate() {
        for s in &l.segments {
            let c = col_of(s);
            let text = s.text().trim().to_string();
            if text.is_empty() {
                continue;
            }
            match cells.iter_mut().find(|x| x.row == r && x.col == c) {
                Some(x) => {
                    x.text.push(' ');
                    x.text.push_str(&text);
                    x.bold &= s.all_bold();
                }
                None => cells.push(TableCell { row: r, col: c, text, bold: s.all_bold() }),
            }
        }
    }
    cells
}

/// Bảng ô của 1 trang (công khai cho test/xem trước).
pub fn page_table(pdfium: &Pdfium, input: &Path, page_index: u16, password: Option<&str>) -> Result<Vec<TableCell>, EngineError> {
    let doc = open(pdfium, input, password)?;
    let page = doc.pages().get(page_index).map_err(|e| err(format!("trang {}: {e}", page_index + 1)))?;
    Ok(table_from_lines(&page_lines_rich(&page, 1.0)))
}

const CT_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
"#;

fn core_xml() -> String {
    let now = crate::create::iso_now();
    format!(
        r#"{CT_HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:creator>FoFreeXit</dc:creator><cp:lastModifiedBy>FoFreeXit</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified></cp:coreProperties>"#
    )
}

fn app_xml() -> String {
    format!(
        r#"{CT_HEAD}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>FoFreeXit</Application></Properties>"#
    )
}

const ROOT_RELS_TAIL: &str = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>"#;

/// Tên sheet hợp lệ Excel (≤31 ký tự, không []:*?/\\, duy nhất).
fn sheet_name(prefix: &str, page: u16, used: &mut Vec<String>) -> String {
    let base: String = format!("{} {}", prefix.trim(), page + 1)
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    let base = if base.trim().is_empty() { format!("{}", page + 1) } else { base };
    let mut name = base.clone();
    let mut k = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
        name = format!("{}_{k}", base.chars().take(28).collect::<String>());
        k += 1;
    }
    used.push(name.clone());
    name
}

/// PDF → Excel: mỗi trang 1 sheet; bảng dựng từ vị trí chữ; số → ô số.
/// `sheet_prefix`: tiền tố tên sheet (vd "Trang" → "Trang 1").
pub fn export_xlsx(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    sheet_prefix: &str,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let mut strings: Vec<String> = Vec::new();
    let mut string_idx: std::collections::HashMap<String, usize> = Default::default();
    let mut sheets: Vec<(String, String)> = Vec::new(); // (tên, xml)
    let mut used_names = Vec::new();
    for &p in &list {
        let page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        let cells = table_from_lines(&page_lines_rich(&page, 1.0));
        let max_row = cells.iter().map(|c| c.row).max().unwrap_or(0);
        let max_col = cells.iter().map(|c| c.col).max().unwrap_or(0);
        let mut widths = vec![8.0f32; max_col + 1];
        let mut rows = String::new();
        for r in 0..=max_row {
            let mut row_cells: Vec<&TableCell> = cells.iter().filter(|c| c.row == r).collect();
            if row_cells.is_empty() {
                continue;
            }
            row_cells.sort_by_key(|c| c.col);
            rows.push_str(&format!(r#"<row r="{}">"#, r + 1));
            for c in row_cells {
                let reference = format!("{}{}", col_name(c.col), r + 1);
                let style = if c.bold { r#" s="1""# } else { "" };
                let text: String = c.text.chars().take(32000).collect();
                widths[c.col] = widths[c.col].max((text.chars().count() as f32 * 1.1 + 2.0).min(80.0));
                match parse_number_cell(&text) {
                    Some(v) if v.is_finite() => {
                        rows.push_str(&format!(r#"<c r="{reference}"{style}><v>{v}</v></c>"#));
                    }
                    _ => {
                        let idx = *string_idx.entry(text.clone()).or_insert_with(|| {
                            strings.push(text.clone());
                            strings.len() - 1
                        });
                        rows.push_str(&format!(r#"<c r="{reference}"{style} t="s"><v>{idx}</v></c>"#));
                    }
                }
            }
            rows.push_str("</row>");
        }
        let dim = if cells.is_empty() {
            "A1".to_string()
        } else {
            format!("A1:{}{}", col_name(max_col), max_row + 1)
        };
        let cols: String = widths
            .iter()
            .enumerate()
            .map(|(i, w)| format!(r#"<col min="{n}" max="{n}" width="{w:.1}" customWidth="1"/>"#, n = i + 1))
            .collect();
        let data = if rows.is_empty() { "<sheetData/>".to_string() } else { format!("<sheetData>{rows}</sheetData>") };
        let xml = format!(
            r#"{CT_HEAD}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="{dim}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols>{cols}</cols>{data}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#
        );
        sheets.push((sheet_name(sheet_prefix, p, &mut used_names), xml));
    }

    let n = sheets.len();
    let mut ct = format!(
        r#"{CT_HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#
    );
    for i in 1..=n {
        ct.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
    }
    ct.push_str(r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#);

    let root_rels = format!(
        r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>{ROOT_RELS_TAIL}</Relationships>"#
    );
    let mut wb_sheets = String::new();
    let mut wb_rels = String::new();
    for (i, (name, _)) in sheets.iter().enumerate() {
        wb_sheets.push_str(&format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, xml_text(name), i + 1, i + 1));
        wb_rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.xml"/>"#,
            i + 1,
            i + 1
        ));
    }
    wb_rels.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#,
        n + 1,
        n + 2
    ));
    let workbook = format!(
        r#"{CT_HEAD}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView/></bookViews><sheets>{wb_sheets}</sheets></workbook>"#
    );
    let wb_rels = format!(r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{wb_rels}</Relationships>"#);
    let styles = format!(
        r#"{CT_HEAD}<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    );
    let total_refs: usize = sheets.iter().map(|(_, x)| x.matches(r#" t="s""#).count()).sum();
    let mut sst = format!(
        r#"{CT_HEAD}<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{total_refs}" uniqueCount="{}">"#,
        strings.len()
    );
    for s in &strings {
        sst.push_str(&format!(r#"<si><t xml:space="preserve">{}</t></si>"#, xml_text(s)));
    }
    sst.push_str("</sst>");

    let core = core_xml();
    let app = app_xml();
    let sheet_names: Vec<String> = (1..=n).map(|i| format!("xl/worksheets/sheet{i}.xml")).collect();
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("docProps/app.xml", app.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/sharedStrings.xml", sst.as_bytes()),
    ];
    for (i, (_, xml)) in sheets.iter().enumerate() {
        entries.push((sheet_names[i].as_str(), xml.as_bytes()));
    }
    std::fs::write(output, build_zip_stored(&entries))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// PowerPoint .pptx
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PptxOptions {
    /// true: nền = trang KHÔNG chữ + hộp chữ sửa được đúng vị trí;
    /// false: mỗi slide chỉ là ảnh trang (giữ nguyên hình, không sửa chữ).
    pub editable_text: bool,
    /// DPI render ảnh nền.
    pub dpi: f32,
}

impl Default for PptxOptions {
    fn default() -> Self {
        PptxOptions { editable_text: true, dpi: 150.0 }
    }
}

const NS_PML: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const GRP_HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

fn theme_xml() -> String {
    let fill3 = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#.repeat(3);
    format!(
        r#"{CT_HEAD}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst>{fill3}</a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{fill3}</a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#
    )
}

const EMU_PER_PT: f64 = 12700.0;

/// PDF → PowerPoint: mỗi trang 1 slide (kích thước slide = trang đầu).
pub fn export_pptx(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    opts: &PptxOptions,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let first = doc.pages().get(list[0]).map_err(|e| err(format!("trang: {e}")))?;
    let clamp_emu = |v: f64| v.round().clamp(914_400.0, 51_206_400.0) as i64;
    let slide_cx = clamp_emu(first.width().value as f64 * EMU_PER_PT);
    let slide_cy = clamp_emu(first.height().value as f64 * EMU_PER_PT);
    drop(first);

    let mut slides: Vec<(String, String)> = Vec::new(); // (slide xml, rels)
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    for (si, &p) in list.iter().enumerate() {
        let mut page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        let (pw, ph) = (page.width().value as f64, page.height().value as f64);
        let lines = if opts.editable_text { page_lines_rich(&page, 1.6) } else { Vec::new() };
        if opts.editable_text && !lines.is_empty() {
            strip_text_objects(&mut page);
        }
        let img = render(&page, opts.dpi.clamp(50.0, 400.0))?;
        drop(page);
        let jpg = encode_jpeg(&img, 88)?;
        let media_name = format!("image{}.jpeg", si + 1);
        media.push((format!("ppt/media/{media_name}"), jpg));

        // Trang khác khổ slide → thu vừa + căn giữa.
        let s = (slide_cx as f64 / (pw * EMU_PER_PT)).min(slide_cy as f64 / (ph * EMU_PER_PT));
        let (ox, oy) = (
            (slide_cx as f64 - pw * EMU_PER_PT * s) / 2.0,
            (slide_cy as f64 - ph * EMU_PER_PT * s) / 2.0,
        );
        let emu = |pt: f64| (pt * EMU_PER_PT * s).round() as i64;
        let mut tree = String::new();
        tree.push_str(&format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="Page {n}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            n = p + 1,
            x = ox.round() as i64,
            y = oy.round() as i64,
            cx = emu(pw),
            cy = emu(ph),
        ));
        let mut id = 3;
        for l in &lines {
            for seg in &l.segments {
                let text_all = seg.text();
                if text_all.trim().is_empty() {
                    continue;
                }
                let size = seg.size as f64;
                let top_pt = ph - (seg.baseline as f64 + size * 0.92);
                let x = ox.round() as i64 + emu(seg.left as f64);
                let y = oy.round() as i64 + emu(top_pt.max(0.0));
                let cx = emu(((seg.right - seg.left) as f64 + size * 0.6).max(size));
                let cy = emu(size * 1.25);
                let mut runs = String::new();
                for r in &seg.runs {
                    let sz = ((r.size as f64 * s * 100.0).round() as i64).clamp(100, 400_000);
                    let fill = if r.invisible {
                        r#"<a:solidFill><a:srgbClr val="000000"><a:alpha val="0"/></a:srgbClr></a:solidFill>"#.to_string()
                    } else {
                        format!(r#"<a:solidFill><a:srgbClr val="{:02X}{:02X}{:02X}"/></a:solidFill>"#, r.color[0], r.color[1], r.color[2])
                    };
                    let face = xml_text(&r.font);
                    runs.push_str(&format!(
                        r#"<a:r><a:rPr lang="vi-VN" sz="{sz}" b="{}" i="{}" dirty="0">{fill}<a:latin typeface="{face}"/><a:cs typeface="{face}"/></a:rPr><a:t>{}</a:t></a:r>"#,
                        r.bold as u8,
                        r.italic as u8,
                        xml_text(&r.text),
                    ));
                }
                tree.push_str(&format!(
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="none" lIns="0" tIns="0" rIns="0" bIns="0" rtlCol="0" anchor="t"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p>{runs}</a:p></p:txBody></p:sp>"#
                ));
                id += 1;
            }
        }
        let slide = format!(
            r#"{CT_HEAD}<p:sld {NS_PML}><p:cSld><p:spTree>{GRP_HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        let rels = format!(
            r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{media_name}"/></Relationships>"#
        );
        slides.push((slide, rels));
    }

    let n = slides.len();
    let mut ct = format!(
        r#"{CT_HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>"#
    );
    for i in 1..=n {
        ct.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
    }
    ct.push_str(r#"<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/presProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presProps+xml"/><Override PartName="/ppt/viewProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml"/><Override PartName="/ppt/tableStyles.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#);

    let root_rels = format!(
        r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>{ROOT_RELS_TAIL}</Relationships>"#
    );
    let mut sld_ids = String::new();
    let mut pres_rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
    );
    for i in 0..n {
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 2));
        pres_rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>"#,
            i + 2,
            i + 1
        ));
    }
    let k = n + 2;
    pres_rels.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps" Target="presProps.xml"/><Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps" Target="viewProps.xml"/><Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles" Target="tableStyles.xml"/>"#,
        k,
        k + 1,
        k + 2,
        k + 3
    ));
    let presentation = format!(
        r#"{CT_HEAD}<p:presentation {NS_PML} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{slide_cx}" cy="{slide_cy}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="vi-VN"/></a:defPPr></p:defaultTextStyle></p:presentation>"#
    );
    let pres_rels = format!(r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{pres_rels}</Relationships>"#);
    let master = format!(
        r#"{CT_HEAD}<p:sldMaster {NS_PML}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{GRP_HEAD}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz="4400"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"#
    );
    let master_rels = format!(
        r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#
    );
    let layout = format!(
        r#"{CT_HEAD}<p:sldLayout {NS_PML} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{GRP_HEAD}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    );
    let layout_rels = format!(
        r#"{CT_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#
    );
    let pres_props = format!(r#"{CT_HEAD}<p:presentationPr {NS_PML}/>"#);
    let view_props = format!(
        r#"{CT_HEAD}<p:viewPr {NS_PML}><p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>"#
    );
    let table_styles = format!(
        r#"{CT_HEAD}<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}"/>"#
    );
    let theme = theme_xml();
    let core = core_xml();
    let app = app_xml();

    let slide_paths: Vec<(String, String)> = (1..=n)
        .map(|i| (format!("ppt/slides/slide{i}.xml"), format!("ppt/slides/_rels/slide{i}.xml.rels")))
        .collect();
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
        ("docProps/app.xml", app.as_bytes()),
        ("ppt/presentation.xml", presentation.as_bytes()),
        ("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        ("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels.as_bytes()),
        ("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", layout_rels.as_bytes()),
        ("ppt/theme/theme1.xml", theme.as_bytes()),
        ("ppt/presProps.xml", pres_props.as_bytes()),
        ("ppt/viewProps.xml", view_props.as_bytes()),
        ("ppt/tableStyles.xml", table_styles.as_bytes()),
    ];
    for (i, (slide, rels)) in slides.iter().enumerate() {
        entries.push((slide_paths[i].0.as_str(), slide.as_bytes()));
        entries.push((slide_paths[i].1.as_str(), rels.as_bytes()));
    }
    for (name, data) in &media {
        entries.push((name.as_str(), data.as_slice()));
    }
    std::fs::write(output, build_zip_stored(&entries))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlMode {
    /// Giữ bố cục: nền trang (không chữ) + chữ đặt đúng vị trí.
    Positioned,
    /// Văn bản trôi: đoạn/tiêu đề + ảnh, đọc tốt trên mọi màn hình.
    Flowing,
}

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn css_family(font: &str) -> String {
    let l = font.to_ascii_lowercase();
    let generic = if l.contains("courier") || l.contains("mono") || l.contains("consol") {
        "monospace"
    } else if l.contains("times") || l.contains("serif") && !l.contains("sans") || l.contains("georgia") || l.contains("cambria") {
        "serif"
    } else {
        "sans-serif"
    };
    format!("'{}',{generic}", font.replace(['\'', '"', ';', '<', '>'], ""))
}

fn run_css(r: &Run, scale: f32) -> String {
    let mut css = format!("font-size:{:.1}pt;font-family:{}", r.size * scale, css_family(&r.font));
    if r.bold {
        css.push_str(";font-weight:bold");
    }
    if r.italic {
        css.push_str(";font-style:italic");
    }
    if r.invisible {
        css.push_str(";color:transparent");
    } else if r.color != [0, 0, 0] {
        css.push_str(&format!(";color:#{:02x}{:02x}{:02x}", r.color[0], r.color[1], r.color[2]));
    }
    css
}

/// PDF → 1 tệp HTML độc lập (ảnh nhúng base64).
pub fn export_html(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    mode: HtmlMode,
    dpi: f32,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let title = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut body = String::new();
    for &p in &list {
        let mut page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        let (pw, ph) = (page.width().value, page.height().value);
        match mode {
            HtmlMode::Positioned => {
                let lines = page_lines_rich(&page, 1.6);
                if !lines.is_empty() {
                    strip_text_objects(&mut page);
                }
                let bg = encode_jpeg(&render(&page, dpi.clamp(50.0, 300.0))?, 85)?;
                body.push_str(&format!(
                    r#"<div class="page" id="page{n}" style="width:{pw:.1}pt;height:{ph:.1}pt;background-image:url(data:image/jpeg;base64,{img})">"#,
                    n = p + 1,
                    img = b64.encode(&bg)
                ));
                for l in &lines {
                    for seg in &l.segments {
                        let top = ph - (seg.baseline + seg.size * 0.9);
                        body.push_str(&format!(r#"<div class="t" style="left:{:.1}pt;top:{:.1}pt">"#, seg.left, top.max(0.0)));
                        for r in &seg.runs {
                            body.push_str(&format!(r#"<span style="{}">{}</span>"#, run_css(r, 1.0), html_esc(&r.text)));
                        }
                        body.push_str("</div>");
                    }
                }
                body.push_str("</div>\n");
            }
            HtmlMode::Flowing => {
                // Mục trong luồng: (top, html) — sắp xếp theo vị trí dọc.
                let mut items: Vec<(f32, String)> = Vec::new();
                for obj in page.objects().iter() {
                    if obj.object_type() != PdfPageObjectType::Image {
                        continue;
                    }
                    let Ok(q) = obj.bounds() else { continue };
                    let (w, top) = (q.right().value - q.left().value, q.top().value);
                    if w < 8.0 {
                        continue;
                    }
                    let Some(io) = obj.as_image_object() else { continue };
                    let Ok(img) = io.get_processed_image(&doc).or_else(|_| io.get_raw_image()) else { continue };
                    let (mime, data) = if img.color().has_alpha() {
                        ("png", encode_png(&img)?)
                    } else {
                        ("jpeg", encode_jpeg(&img, 85)?)
                    };
                    items.push((
                        top,
                        format!(r#"<p class="img"><img style="width:{w:.0}pt" src="data:image/{mime};base64,{}" alt=""></p>"#, b64.encode(&data)),
                    ));
                }
                let lines = page_lines_rich(&page, 3.0);
                let mut sizes: Vec<f32> = lines.iter().map(|l| l.size).collect();
                sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let body_size = sizes.get(sizes.len() / 2).copied().unwrap_or(11.0).max(1.0);
                // Gom dòng thành đoạn: cỡ tương đương + khoảng cách dòng bình thường.
                let mut para: Vec<&Line> = Vec::new();
                let flush = |para: &mut Vec<&Line>, items: &mut Vec<(f32, String)>| {
                    if para.is_empty() {
                        return;
                    }
                    let size = para.iter().map(|l| l.size).fold(0.0f32, f32::max);
                    let tag = if size >= body_size * 1.8 {
                        "h1"
                    } else if size >= body_size * 1.35 {
                        "h2"
                    } else {
                        "p"
                    };
                    let mut html = String::new();
                    for (i, l) in para.iter().enumerate() {
                        for (j, seg) in l.segments.iter().enumerate() {
                            if j > 0 {
                                html.push(' ');
                            }
                            for r in &seg.runs {
                                let mut css = String::new();
                                if r.bold && tag == "p" {
                                    css.push_str("font-weight:bold;");
                                }
                                if r.italic {
                                    css.push_str("font-style:italic;");
                                }
                                if r.color != [0, 0, 0] && !r.invisible {
                                    css.push_str(&format!("color:#{:02x}{:02x}{:02x};", r.color[0], r.color[1], r.color[2]));
                                }
                                if css.is_empty() {
                                    html.push_str(&html_esc(&r.text));
                                } else {
                                    html.push_str(&format!(r#"<span style="{css}">{}</span>"#, html_esc(&r.text)));
                                }
                            }
                        }
                        if i + 1 < para.len() {
                            // Ghép dòng: bỏ gạch nối cuối dòng, còn lại chèn khoảng trắng.
                            if html.ends_with('-') && !html.ends_with(" -") {
                                html.pop();
                            } else {
                                html.push(' ');
                            }
                        }
                    }
                    items.push((para[0].baseline + para[0].size, format!("<{tag}>{html}</{tag}>")));
                    para.clear();
                };
                for l in &lines {
                    if let Some(prev) = para.last() {
                        let gap = prev.baseline - l.baseline;
                        let similar = (prev.size - l.size).abs() <= prev.size.max(l.size) * 0.15;
                        let indent = (l.left() - prev.left()).abs() > prev.size * 3.0 && l.left() > prev.left();
                        if gap > prev.size.max(l.size) * 1.75 || !similar || indent {
                            flush(&mut para, &mut items);
                        }
                    }
                    para.push(l);
                }
                flush(&mut para, &mut items);
                items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                body.push_str(&format!(r#"<section class="flow" id="page{}">"#, p + 1));
                for (_, h) in items {
                    body.push_str(&h);
                    body.push('\n');
                }
                body.push_str("</section>\n");
                let _ = &mut page;
            }
        }
    }
    let css = match mode {
        HtmlMode::Positioned => "body{margin:0;background:#8a8f98}.page{position:relative;margin:16px auto;background:#fff no-repeat;background-size:100% 100%;box-shadow:0 1px 6px rgba(0,0,0,.35);overflow:hidden}.t{position:absolute;white-space:pre;line-height:1.15}",
        HtmlMode::Flowing => "body{margin:0;background:#fff;color:#111;font-family:'Segoe UI',Arial,sans-serif;line-height:1.55}.flow{max-width:820px;margin:0 auto;padding:24px 20px;border-bottom:1px solid #ddd}.img{text-align:center}.img img{max-width:100%;height:auto}h1,h2{line-height:1.25}",
    };
    let html = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"FoFreeXit\">\n<title>{}</title>\n<style>{css}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        html_esc(&title)
    );
    std::fs::write(output, html)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// RTF
// ---------------------------------------------------------------------------

fn rtf_esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            c if (c as u32) < 0x80 => o.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    o.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
    o
}

/// PDF → RTF: mỗi dòng 1 đoạn (giữ cỡ/đậm/nghiêng), ngắt trang giữa các trang.
pub fn export_rtf(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    password: Option<&str>,
) -> Result<(), EngineError> {
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Arial;}}\\uc1\n");
    for (i, &p) in list.iter().enumerate() {
        if i > 0 {
            out.push_str("\\page\n");
        }
        let page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        for l in page_lines_rich(&page, 3.0) {
            out.push_str("\\pard ");
            for (j, seg) in l.segments.iter().enumerate() {
                if j > 0 {
                    out.push_str("\\tab ");
                }
                for r in &seg.runs {
                    let hp = (r.size * 2.0).round().clamp(8.0, 144.0) as u32;
                    out.push_str(&format!(
                        "{{\\fs{hp}{}{} {}}}",
                        if r.bold { "\\b" } else { "" },
                        if r.italic { "\\i" } else { "" },
                        rtf_esc(&r.text)
                    ));
                }
            }
            out.push_str("\\par\n");
        }
    }
    out.push('}');
    std::fs::write(output, out)?;
    Ok(())
}

/// Văn bản các trang (phạm vi) → tệp .txt.
pub fn export_text_range(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    pages: &[u16],
    password: Option<&str>,
) -> Result<(), EngineError> {
    let doc = open(pdfium, input, password)?;
    let list = page_list(&doc, pages)?;
    let mut all = String::new();
    for &p in &list {
        let page = doc.pages().get(p).map_err(|e| err(format!("trang {}: {e}", p + 1)))?;
        if let Ok(t) = page.text() {
            all.push_str(&t.all());
        }
        all.push_str("\n\n");
    }
    std::fs::write(output, all)?;
    Ok(())
}

/// Trích phạm vi trang ra PDF tạm (cho bộ chuyển ngoài như LibreOffice).
/// None = dùng nguyên tệp (mọi trang).
pub fn subset_to_temp(
    pdfium: &Pdfium,
    input: &Path,
    pages: &[u16],
    password: Option<&str>,
) -> Result<Option<PathBuf>, EngineError> {
    let total = crate::render::page_count(pdfium, input, password)?;
    let list = resolve_pages(total, pages);
    if list.is_empty() {
        return Err(err("không có trang nào trong phạm vi đã chọn"));
    }
    if list.len() == total as usize && list.iter().enumerate().all(|(i, &p)| i as u16 == p) && password.is_none() {
        return Ok(None);
    }
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "doc".into());
    let dir = std::env::temp_dir().join(format!("ff_subset_{}_{}", std::process::id(), crate::create::nanos()));
    std::fs::create_dir_all(&dir)?;
    let out = dir.join(format!("{stem}.pdf"));
    crate::organize::extract_pages(pdfium, input, &list, &out, password)?;
    Ok(Some(out))
}
