//! Page Marks kiểu Foxit (Trang > Hình mờ / Nền / Đầu & chân trang / Đánh số Bates).
//!
//! Hai lượt:
//! 1. PDFium dựng một tài liệu NHÁP chứa phần chữ (PDFium lo mã hoá CID cho
//!    font nhúng → tiếng Việt đúng; đo bề rộng chữ chính xác).
//! 2. lopdf biến từng trang nháp (hoặc ảnh, hoặc 1 trang của PDF khác) thành
//!    Form XObject rồi đóng lên trang đích trong khối
//!    `/Artifact <</Subtype /Watermark /Type /Pagination>> BDC … EMC` — đúng
//!    cách Acrobat/Foxit đánh dấu, nên "Gỡ" tìm lại được (cả dấu do Acrobat/
//!    Foxit thêm) và đặt được DƯỚI hoặc TRÊN nội dung gốc (xem pagemark_cos.rs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId, Stream};
use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::pagemark_cos::{
    self as cos, invert, lerr, mul, rotate, scale, translate, Body, Mat, PageGeom,
};
pub use crate::pagemark_cos::MarkKind;
use crate::EngineError;

/// Vị trí neo theo lưới 9 điểm (= căn dọc × căn ngang của dialog Foxit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft, TopCenter, TopRight,
    MiddleLeft, Center, MiddleRight,
    BottomLeft, BottomCenter, BottomRight,
}

impl Anchor {
    /// (ngang, dọc): -1 = trái/dưới, 0 = giữa, 1 = phải/trên.
    fn axes(self) -> (i8, i8) {
        use Anchor::*;
        match self {
            TopLeft => (-1, 1), TopCenter => (0, 1), TopRight => (1, 1),
            MiddleLeft => (-1, 0), Center => (0, 0), MiddleRight => (1, 0),
            BottomLeft => (-1, -1), BottomCenter => (0, -1), BottomRight => (1, -1),
        }
    }
}

/// Tập con trang trong phạm vi (Foxit: "Tất cả trang / Trang chẵn / Trang lẻ").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PageSubset {
    #[default]
    All,
    Even,
    Odd,
}

/// Font chữ. Sans/Serif/Mono = font nhúng (Noto đóng gói theo app — đủ tiếng
/// Việt); Helvetica/Times/Courier = font chuẩn base-14 (không nhúng, nhẹ) —
/// tự chuyển sang font nhúng cùng kiểu nếu chữ có ký tự ngoài bảng mã WinAnsi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontChoice {
    #[default]
    Sans,
    Serif,
    Mono,
    Helvetica,
    Times,
    Courier,
}

/// Kiểu chữ của 1 khối văn bản.
#[derive(Clone, Debug)]
pub struct TextStyle {
    pub font: FontChoice,
    pub font_size: f32,
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font: FontChoice::Sans, font_size: 36.0, color: [0, 0, 0], bold: false, italic: false }
    }
}

/// Nguồn của hình mờ / nền.
#[derive(Clone, Debug)]
pub enum StampSource {
    /// Văn bản (nhiều dòng tách bằng '\n', căn giữa).
    Text { text: String, style: TextStyle },
    /// Ảnh PNG/JPG/BMP/GIF/WebP (1 px = 1 pt ở tỉ lệ 100%).
    Image { path: PathBuf },
    /// Một trang của tệp PDF khác (0-based).
    Page { path: PathBuf, page: u16 },
    /// Màu đặc phủ kín trang (chỉ có nghĩa với Nền).
    Color { color: [u8; 3] },
}

/// Hình mờ / nền (dialog "Thêm hình mờ" / "Thêm nền" của Foxit).
#[derive(Clone, Debug)]
pub struct WatermarkSpec {
    pub source: StampSource,
    /// Độ xoay (độ), dương = ngược chiều kim đồng hồ.
    pub rotation_deg: f32,
    /// Độ mờ đục 0.0–1.0 (1 = đậm hẳn).
    pub opacity: f32,
    /// Tỉ lệ tuyệt đối so với cỡ gốc của ảnh/trang nguồn (1.0 = 100%).
    pub scale: f32,
    /// Foxit "Tỉ lệ so với trang đích": Some(0.5) = vừa khít 50% trang (bỏ qua `scale`).
    pub relative_scale: Option<f32>,
    pub anchor: Anchor,
    /// Khoảng cách (pt) từ mép/tâm theo căn ngang/dọc đã chọn.
    pub offset_x: f32,
    pub offset_y: f32,
    /// true = nằm dưới nội dung trang, false = đè lên trên.
    pub behind: bool,
    /// Gỡ dấu cùng loại có sẵn trước khi thêm (Foxit "Cập nhật").
    pub replace_existing: bool,
    /// 0-based; rỗng = mọi trang.
    pub pages: Vec<u16>,
    pub subset: PageSubset,
}

impl WatermarkSpec {
    /// Hình mờ chữ với mặc định giống Foxit (giữa trang, xoay 45°, mờ 50%).
    pub fn text(text: &str) -> Self {
        WatermarkSpec {
            source: StampSource::Text {
                text: text.to_string(),
                style: TextStyle { font_size: 48.0, color: [255, 0, 0], ..Default::default() },
            },
            rotation_deg: 45.0,
            opacity: 0.5,
            scale: 1.0,
            relative_scale: None,
            anchor: Anchor::Center,
            offset_x: 0.0,
            offset_y: 0.0,
            behind: false,
            replace_existing: false,
            pages: vec![],
            subset: PageSubset::All,
        }
    }
}

/// Định dạng số Bates: `prefix` + số `start…` đệm 0 đủ `digits` chữ số + `suffix`.
#[derive(Clone, Debug)]
pub struct BatesFormat {
    pub prefix: String,
    pub suffix: String,
    pub start: u64,
    pub digits: u8,
}

impl BatesFormat {
    pub fn format(&self, n: u64) -> String {
        format!("{}{:0width$}{}", self.prefix, n, self.suffix, width = self.digits as usize)
    }
}

/// Đầu/chân trang — 6 ô (trái/giữa/phải × trên/dưới), token:
/// `{page}` (số trang = vị trí + `start_number`), `{total}`, `{date}` (lấy từ
/// `date` do caller định dạng sẵn), `{bates}` (khi `bates` có giá trị).
#[derive(Clone, Debug)]
pub struct HeaderFooterSpec {
    pub top_left: String,
    pub top_center: String,
    pub top_right: String,
    pub bottom_left: String,
    pub bottom_center: String,
    pub bottom_right: String,
    pub font: FontChoice,
    pub font_size: f32,
    /// RGBA — alpha = độ mờ đục.
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub date: String,
    /// Số của trang đầu tài liệu (Foxit "Bắt đầu đánh số từ").
    pub start_number: u32,
    pub bates: Option<BatesFormat>,
    pub replace_existing: bool,
    /// 0-based; rỗng = mọi trang.
    pub pages: Vec<u16>,
    pub subset: PageSubset,
}

impl Default for HeaderFooterSpec {
    fn default() -> Self {
        HeaderFooterSpec {
            top_left: String::new(),
            top_center: String::new(),
            top_right: String::new(),
            bottom_left: String::new(),
            bottom_center: String::new(),
            bottom_right: String::new(),
            font: FontChoice::Sans,
            font_size: 10.0,
            color: [0, 0, 0, 255],
            bold: false,
            italic: false,
            // Mặc định Foxit: trên/dưới 0,5 in, trái/phải 1 in.
            margin_top: 36.0,
            margin_bottom: 36.0,
            margin_left: 72.0,
            margin_right: 72.0,
            date: String::new(),
            start_number: 1,
            bates: None,
            replace_existing: false,
            pages: vec![],
            subset: PageSubset::All,
        }
    }
}

/// Kết quả đánh Bates cho 1 tệp.
#[derive(Clone, Debug)]
pub struct BatesRange {
    pub output: PathBuf,
    pub first: String,
    pub last: String,
    pub pages: u32,
}

/// Số trang (có dấu) theo từng loại — dùng cho "Gỡ …" (báo "không có" như Foxit).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkCounts {
    pub watermark: u32,
    pub header_footer: u32,
    pub background: u32,
    pub bates: u32,
}

fn applies(pages: &[u16], subset: PageSubset, index: u16) -> bool {
    if !(pages.is_empty() || pages.contains(&index)) {
        return false;
    }
    let n = index as u32 + 1;
    match subset {
        PageSubset::All => true,
        PageSubset::Even => n % 2 == 0,
        PageSubset::Odd => n % 2 == 1,
    }
}

fn perr(ctx: &'static str) -> impl Fn(PdfiumError) -> EngineError {
    move |e| EngineError::Pdfium(format!("{ctx}: {}", crate::pdfium_msg(&e)))
}

// ---------------------------------------------------------------- font

fn winansi_ok(s: &str) -> bool {
    s.chars().all(|c| c == '\n' || (' '..='~').contains(&c) || ('\u{a0}'..='\u{ff}').contains(&c))
}

fn read_first(paths: &[String]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Font nhúng theo kiểu chữ: Noto đóng gói theo app trước, rồi font hệ thống.
fn family_font_bytes(font: FontChoice, bold: bool, italic: bool) -> Option<Vec<u8>> {
    let v = match (bold, italic) { (false, false) => 0, (true, false) => 1, (false, true) => 2, (true, true) => 3 };
    let (noto, win, linux): ([&str; 4], [&str; 4], [&str; 4]) = match font {
        FontChoice::Serif | FontChoice::Times => (
            ["NotoSerif-Regular.ttf", "NotoSerif-Bold.ttf", "NotoSerif-Italic.ttf", "NotoSerif-BoldItalic.ttf"],
            ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
            ["dejavu/DejaVuSerif.ttf", "dejavu/DejaVuSerif-Bold.ttf", "dejavu/DejaVuSerif-Italic.ttf", "dejavu/DejaVuSerif-BoldItalic.ttf"],
        ),
        FontChoice::Mono | FontChoice::Courier => (
            ["NotoSansMono-Regular.ttf", "NotoSansMono-Bold.ttf", "NotoSansMono-Regular.ttf", "NotoSansMono-Bold.ttf"],
            ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"],
            ["dejavu/DejaVuSansMono.ttf", "dejavu/DejaVuSansMono-Bold.ttf", "dejavu/DejaVuSansMono-Oblique.ttf", "dejavu/DejaVuSansMono-BoldOblique.ttf"],
        ),
        _ => return find_font_bytes(bold, italic),
    };
    let mut cands = Vec::new();
    if let Ok(dir) = std::env::var("FOFREEXIT_FONTS_PATH") {
        cands.push(Path::new(&dir).join(noto[v]).to_string_lossy().into_owned());
    }
    if cfg!(windows) {
        cands.push(format!(r"C:\Windows\Fonts\{}", win[v]));
    } else {
        cands.push(format!("/usr/share/fonts/truetype/{}", linux[v]));
        cands.push(format!("/usr/share/fonts/truetype/{}", linux[0]));
    }
    read_first(&cands).or_else(|| find_font_bytes(bold, italic))
}

fn load_font(doc: &mut PdfDocument, st: &TextStyle, sample: &str) -> Result<PdfFontToken, EngineError> {
    let (b, i) = (st.bold, st.italic);
    let builtin = winansi_ok(sample);
    let fonts = doc.fonts_mut();
    let tok = match st.font {
        FontChoice::Helvetica if builtin => Some(match (b, i) {
            (false, false) => fonts.helvetica(),
            (true, false) => fonts.helvetica_bold(),
            (false, true) => fonts.helvetica_oblique(),
            (true, true) => fonts.helvetica_bold_oblique(),
        }),
        FontChoice::Times if builtin => Some(match (b, i) {
            (false, false) => fonts.times_roman(),
            (true, false) => fonts.times_bold(),
            (false, true) => fonts.times_italic(),
            (true, true) => fonts.times_bold_italic(),
        }),
        FontChoice::Courier if builtin => Some(match (b, i) {
            (false, false) => fonts.courier(),
            (true, false) => fonts.courier_bold(),
            (false, true) => fonts.courier_oblique(),
            (true, true) => fonts.courier_bold_oblique(),
        }),
        _ => None,
    };
    if let Some(t) = tok {
        return Ok(t);
    }
    let bytes = family_font_bytes(st.font, b, i)
        .ok_or_else(|| EngineError::Pdfium("không tìm được font để ghi chữ".into()))?;
    doc.fonts_mut()
        .load_true_type_from_bytes(&bytes, true)
        .map_err(perr("nạp font"))
}

/// Tạo các dòng chữ (chưa gắn trang), trả về (object, dx, dy) đã căn trong
/// khối kích thước (w, h) có gốc dưới-trái; `align` -1/0/1 = trái/giữa/phải.
fn text_block<'a>(
    doc: &PdfDocument<'a>,
    text: &str,
    token: PdfFontToken,
    st: &TextStyle,
    align: i8,
) -> Result<(Vec<(PdfPageTextObject<'a>, f32, f32)>, f32, f32), EngineError> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let lh = st.font_size * 1.2;
    // (object — None cho dòng trống, lề trái glyph, bề rộng)
    let mut made = Vec::new();
    let mut w = 0f32;
    for line in &lines {
        if line.trim().is_empty() {
            made.push((None, 0.0, 0.0));
            continue;
        }
        let mut obj = PdfPageTextObject::new(doc, line.to_string(), token, PdfPoints::new(st.font_size))
            .map_err(perr("tạo chữ"))?;
        obj.set_fill_color(PdfColor::new(st.color[0], st.color[1], st.color[2], 255)).map_err(perr("màu chữ"))?;
        let (left, right) = match obj.bounds() {
            Ok(q) => (q.left().value, q.right().value),
            _ => (0.0, 0.0),
        };
        w = w.max(right - left);
        made.push((Some(obj), left, right - left));
    }
    let n = made.len();
    let h = n as f32 * lh;
    let mut out = Vec::new();
    for (i, (obj, left, lw)) in made.into_iter().enumerate() {
        let Some(obj) = obj else { continue };
        let x = match align { -1 => 0.0, 1 => w - lw, _ => (w - lw) / 2.0 } - left;
        let y = (n - 1 - i) as f32 * lh + st.font_size * 0.25;
        out.push((obj, x, y));
    }
    Ok((out, w, h))
}

fn place_block<'a>(
    page: &mut PdfPage<'a>,
    objs: Vec<(PdfPageTextObject<'a>, f32, f32)>,
    x0: f32,
    y0: f32,
) -> Result<(), EngineError> {
    for (mut obj, dx, dy) in objs {
        obj.translate(PdfPoints::new(x0 + dx), PdfPoints::new(y0 + dy)).map_err(perr("đặt chữ"))?;
        page.objects_mut().add_text_object(obj).map_err(perr("thêm chữ"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- con dấu (stamp)

/// Con dấu = Form XObject trong `doc`, vẽ trong khung [0,w]×[0,h].
struct Stamp {
    id: ObjectId,
    w: f32,
    h: f32,
}

fn stamp_from_text(pdfium: &Pdfium, doc: &mut LoDoc, text: &str, st: &TextStyle) -> Result<Stamp, EngineError> {
    if text.trim().is_empty() {
        return Err(EngineError::Pdfium("nội dung hình mờ đang trống".into()));
    }
    let mut sdoc = pdfium.create_new_pdf().map_err(perr("tạo tài liệu nháp"))?;
    let token = load_font(&mut sdoc, st, text)?;
    let (objs, w, h) = text_block(&sdoc, text, token, st, 0)?;
    let (w, h) = (w.max(1.0), h.max(1.0));
    let mut page = sdoc
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(w), PdfPoints::new(h)))
        .map_err(perr("tạo trang nháp"))?;
    place_block(&mut page, objs, 0.0, 0.0)?;
    page.regenerate_content().map_err(perr("ghi trang nháp"))?;
    drop(page);
    let bytes = sdoc.save_to_bytes().map_err(perr("lưu nháp"))?;
    let scratch = LoDoc::load_mem(&bytes).map_err(lerr("đọc nháp"))?;
    let pid = *scratch.get_pages().values().next().ok_or_else(|| EngineError::Pdfium("nháp rỗng".into()))?;
    let mut map = HashMap::new();
    let id = cos::page_to_form(&scratch, pid, doc, &mut map, [0.0, 0.0, w, h], None);
    Ok(Stamp { id, w, h })
}

fn stamp_from_image(doc: &mut LoDoc, path: &Path) -> Result<Stamp, EngineError> {
    let bytes = std::fs::read(path)?;
    let fmt = image::guess_format(&bytes).ok();
    let img = image::load_from_memory(&bytes)
        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh {}: {e}", path.display())))?;
    let (pw, ph) = (img.width(), img.height());
    let mut idict = Dictionary::new();
    idict.set("Type", Object::Name(b"XObject".to_vec()));
    idict.set("Subtype", Object::Name(b"Image".to_vec()));
    idict.set("Width", pw as i64);
    idict.set("Height", ph as i64);
    idict.set("BitsPerComponent", 8i64);
    let jpeg_ok = fmt == Some(image::ImageFormat::Jpeg)
        && matches!(img.color(), image::ColorType::Rgb8 | image::ColorType::L8);
    let img_id = if jpeg_ok {
        // JPEG RGB/xám: nhúng nguyên (DCTDecode) — không phình dung lượng.
        let cs: &[u8] = if img.color() == image::ColorType::L8 { b"DeviceGray" } else { b"DeviceRGB" };
        idict.set("ColorSpace", Object::Name(cs.to_vec()));
        idict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        let mut s = Stream::new(idict, bytes);
        s.allows_compression = false;
        doc.add_object(Object::Stream(s))
    } else {
        let rgba = img.to_rgba8();
        let mut rgb = Vec::with_capacity((pw * ph * 3) as usize);
        let mut alpha = Vec::with_capacity((pw * ph) as usize);
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            alpha.push(p.0[3]);
        }
        idict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
        if alpha.iter().any(|a| *a < 255) {
            let mut md = Dictionary::new();
            md.set("Type", Object::Name(b"XObject".to_vec()));
            md.set("Subtype", Object::Name(b"Image".to_vec()));
            md.set("Width", pw as i64);
            md.set("Height", ph as i64);
            md.set("BitsPerComponent", 8i64);
            md.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
            let mut ms = Stream::new(md, alpha);
            let _ = ms.compress();
            let mid = doc.add_object(Object::Stream(ms));
            idict.set("SMask", Object::Reference(mid));
        }
        let mut s = Stream::new(idict, rgb);
        let _ = s.compress();
        doc.add_object(Object::Stream(s))
    };
    let (w, h) = (pw as f32, ph as f32);
    let mut xo = Dictionary::new();
    xo.set("Im0", Object::Reference(img_id));
    let mut res = Dictionary::new();
    res.set("XObject", Object::Dictionary(xo));
    let mut fd = Dictionary::new();
    fd.set("Type", Object::Name(b"XObject".to_vec()));
    fd.set("Subtype", Object::Name(b"Form".to_vec()));
    fd.set("BBox", Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Real(w), Object::Real(h)]));
    fd.set("Resources", Object::Dictionary(res));
    let content = format!("q {} 0 0 {} 0 0 cm /Im0 Do Q", cos::num(w), cos::num(h)).into_bytes();
    let mut s = Stream::new(fd, content);
    let _ = s.compress();
    Ok(Stamp { id: doc.add_object(Object::Stream(s)), w, h })
}

fn stamp_from_pdf_page(pdfium: &Pdfium, doc: &mut LoDoc, path: &Path, page: u16) -> Result<Stamp, EngineError> {
    let src = cos::load_lodoc(pdfium, path, None)?.doc;
    let pages = src.get_pages();
    let pid = *pages
        .values()
        .nth(page as usize)
        .ok_or_else(|| EngineError::Pdfium(format!("tệp nguồn không có trang {}", page + 1)))?;
    let g = cos::page_geom(&src, pid);
    let (w, h) = g.disp();
    let mut map = HashMap::new();
    let bbox = [g.x0, g.y0, g.x0 + g.w, g.y0 + g.h];
    let id = cos::page_to_form(&src, pid, doc, &mut map, bbox, Some(invert(g.disp_to_user())));
    Ok(Stamp { id, w, h })
}

/// Ma trận đặt con dấu (w×h) lên trang theo spec: tâm con dấu → xoay → tỉ lệ →
/// căn + lệch trong vùng hiển thị → user space của trang.
fn placement(g: &PageGeom, sw: f32, sh: f32, spec: &WatermarkSpec) -> Mat {
    let (dw, dh) = g.disp();
    let (s, c) = spec.rotation_deg.to_radians().sin_cos();
    let (s, c) = (s.abs(), c.abs());
    let (rw, rh) = ((sw * c + sh * s).max(0.01), (sw * s + sh * c).max(0.01));
    let k = match spec.relative_scale {
        Some(p) if p > 0.0 => p * (dw / rw).min(dh / rh),
        _ => spec.scale.max(0.001),
    };
    let (bw, bh) = (rw * k, rh * k);
    let (ax, ay) = spec.anchor.axes();
    let cx = match ax { -1 => spec.offset_x + bw / 2.0, 1 => dw - spec.offset_x - bw / 2.0, _ => dw / 2.0 + spec.offset_x };
    let cy = match ay { 1 => dh - spec.offset_y - bh / 2.0, -1 => spec.offset_y + bh / 2.0, _ => dh / 2.0 + spec.offset_y };
    let m = mul(translate(-sw / 2.0, -sh / 2.0), scale(k));
    let m = mul(m, rotate(spec.rotation_deg));
    let m = mul(m, translate(cx, cy));
    mul(m, g.disp_to_user())
}

/// Ngữ cảnh áp dụng: xem trước chỉ có 1 trang (bản trích) nhưng phải đánh số
/// như trong tài liệu thật.
#[derive(Clone, Copy)]
struct Ctx {
    /// Vị trí thật (0-based) của trang đầu trong `doc`.
    index_base: u16,
    /// Tổng số trang thật (None = số trang của `doc`).
    total: Option<u16>,
    /// Bỏ qua phạm vi trang (xem trước luôn hiện dấu).
    force_all: bool,
}

const FULL: Ctx = Ctx { index_base: 0, total: None, force_all: false };

fn apply_stamp(pdfium: &Pdfium, doc: &mut LoDoc, spec: &WatermarkSpec, kind: MarkKind, ctx: Ctx) -> Result<u32, EngineError> {
    if spec.replace_existing {
        let ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        for id in ids {
            cos::strip_page(doc, id, &[kind])?;
        }
    }
    let geoms = cos::page_geoms(doc);
    let targets: Vec<&PageGeom> = geoms
        .iter()
        .enumerate()
        .filter(|(i, _)| ctx.force_all || applies(&spec.pages, spec.subset, ctx.index_base + *i as u16))
        .map(|(_, g)| g)
        .collect();
    if targets.is_empty() {
        return Ok(0);
    }
    let behind = spec.behind || kind == MarkKind::Background;
    if let StampSource::Color { color } = &spec.source {
        for g in &targets {
            let (w, h) = g.disp();
            let body = Body::Fill { rgb: *color, cm: g.disp_to_user(), w, h };
            cos::stamp_page(doc, g.id, kind, behind, spec.opacity, &body)?;
        }
        return Ok(targets.len() as u32);
    }
    let stamp = match &spec.source {
        StampSource::Text { text, style } => stamp_from_text(pdfium, doc, text, style)?,
        StampSource::Image { path } => stamp_from_image(doc, path)?,
        StampSource::Page { path, page } => stamp_from_pdf_page(pdfium, doc, path, *page)?,
        StampSource::Color { .. } => unreachable!(),
    };
    for g in &targets {
        let body = Body::XObject { id: stamp.id, cm: placement(g, stamp.w, stamp.h, spec) };
        cos::stamp_page(doc, g.id, kind, behind, spec.opacity, &body)?;
    }
    Ok(targets.len() as u32)
}

/// Kết quả thêm đầu/chân trang: số trang đã đóng + số Bates cuối cùng.
struct HfResult {
    stamped: u32,
    first_bates: Option<u64>,
    last_bates: Option<u64>,
}

fn apply_header_footer(
    pdfium: &Pdfium,
    doc: &mut LoDoc,
    spec: &HeaderFooterSpec,
    ctx: Ctx,
    bates_next: u64,
) -> Result<HfResult, EngineError> {
    let (top_kind, bottom_kind) = if spec.bates.is_some() {
        (MarkKind::Bates, MarkKind::Bates)
    } else {
        (MarkKind::Header, MarkKind::Footer)
    };
    if spec.replace_existing {
        let ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        for id in ids {
            cos::strip_page(doc, id, &[top_kind, bottom_kind])?;
        }
    }
    let geoms = cos::page_geoms(doc);
    let total = ctx.total.unwrap_or(geoms.len() as u16);
    let slots: [(&str, i8, bool); 6] = [
        (&spec.top_left, -1, true),
        (&spec.top_center, 0, true),
        (&spec.top_right, 1, true),
        (&spec.bottom_left, -1, false),
        (&spec.bottom_center, 0, false),
        (&spec.bottom_right, 1, false),
    ];
    if slots.iter().all(|(s, _, _)| s.trim().is_empty()) {
        return Err(EngineError::Pdfium("chưa nhập nội dung đầu/chân trang".into()));
    }
    let style = TextStyle {
        font: spec.font,
        font_size: spec.font_size.max(1.0),
        color: [spec.color[0], spec.color[1], spec.color[2]],
        bold: spec.bold,
        italic: spec.italic,
    };
    let bates_sample = spec.bates.as_ref().map(|b| b.format(0)).unwrap_or_default();
    let sample: String = slots.iter().map(|(s, _, _)| *s).collect::<Vec<_>>().join(" ")
        + &spec.date
        + &bates_sample;

    let mut sdoc = pdfium.create_new_pdf().map_err(perr("tạo tài liệu nháp"))?;
    let token = load_font(&mut sdoc, &style, &sample)?;
    // (trang đích, là đầu trang?)
    let mut layers: Vec<(usize, bool)> = Vec::new();
    let mut bates_n = bates_next;
    let mut first_bates = None;
    let mut stamped = 0u32;
    for (i, g) in geoms.iter().enumerate() {
        let index = ctx.index_base + i as u16;
        if !(ctx.force_all || applies(&spec.pages, spec.subset, index)) {
            continue;
        }
        stamped += 1;
        let bates_str = spec.bates.as_ref().map(|b| {
            first_bates.get_or_insert(bates_n);
            let s = b.format(bates_n);
            bates_n += 1;
            s
        });
        let (dw, dh) = g.disp();
        for top in [true, false] {
            let mut page = None;
            for (tpl, align, is_top) in slots {
                if is_top != top || tpl.trim().is_empty() {
                    continue;
                }
                let text = tpl
                    .replace("{page}", &(index as u64 + spec.start_number as u64).to_string())
                    .replace("{total}", &(total as u64 + spec.start_number as u64 - 1).to_string())
                    .replace("{date}", &spec.date)
                    .replace("{bates}", bates_str.as_deref().unwrap_or(""));
                let (objs, bw, bh) = text_block(&sdoc, &text, token, &style, align)?;
                let x = match align { -1 => spec.margin_left, 1 => dw - spec.margin_right - bw, _ => (dw - bw) / 2.0 };
                let y = if top { dh - spec.margin_top - bh } else { spec.margin_bottom };
                if page.is_none() {
                    page = Some(
                        sdoc.pages_mut()
                            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(dw), PdfPoints::new(dh)))
                            .map_err(perr("tạo trang nháp"))?,
                    );
                    layers.push((i, top));
                }
                place_block(page.as_mut().unwrap(), objs, x, y)?;
            }
            if let Some(mut p) = page {
                p.regenerate_content().map_err(perr("ghi trang nháp"))?;
            }
        }
    }
    let last_bates = first_bates.map(|_| bates_n - 1);
    if layers.is_empty() {
        return Ok(HfResult { stamped, first_bates, last_bates });
    }
    let bytes = sdoc.save_to_bytes().map_err(perr("lưu nháp"))?;
    drop(sdoc);
    let scratch = LoDoc::load_mem(&bytes).map_err(lerr("đọc nháp"))?;
    let spages: Vec<ObjectId> = scratch.get_pages().values().copied().collect();
    let mut map = HashMap::new();
    let opacity = spec.color[3] as f32 / 255.0;
    for (k, (i, top)) in layers.iter().enumerate() {
        let g = &geoms[*i];
        let (dw, dh) = g.disp();
        let form = cos::page_to_form(&scratch, spages[k], doc, &mut map, [0.0, 0.0, dw, dh], None);
        let kind = if *top { top_kind } else { bottom_kind };
        cos::stamp_page(doc, g.id, kind, false, opacity, &Body::XObject { id: form, cm: g.disp_to_user() })?;
    }
    Ok(HfResult { stamped, first_bates, last_bates })
}

// ---------------------------------------------------------------- API công khai

/// Thêm hình mờ vào `input`, ghi ra `output`.
pub fn add_watermark(
    pdfium: &Pdfium,
    input: &Path,
    spec: &WatermarkSpec,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut l = cos::load_lodoc(pdfium, input, password)?;
    if apply_stamp(pdfium, &mut l.doc, spec, MarkKind::Watermark, FULL)? == 0 {
        return Err(EngineError::Pdfium("phạm vi trang không có trang nào".into()));
    }
    cos::save_lodoc(&mut l, output)
}

/// Thêm nền (màu hoặc ảnh/trang PDF) DƯỚI nội dung trang.
pub fn add_background(
    pdfium: &Pdfium,
    input: &Path,
    spec: &WatermarkSpec,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut l = cos::load_lodoc(pdfium, input, password)?;
    if apply_stamp(pdfium, &mut l.doc, spec, MarkKind::Background, FULL)? == 0 {
        return Err(EngineError::Pdfium("phạm vi trang không có trang nào".into()));
    }
    cos::save_lodoc(&mut l, output)
}

/// Thêm đầu/chân trang (đánh số trang, ngày…) vào `input`, ghi ra `output`.
pub fn add_header_footer(
    pdfium: &Pdfium,
    input: &Path,
    spec: &HeaderFooterSpec,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut l = cos::load_lodoc(pdfium, input, password)?;
    let start = spec.bates.as_ref().map_or(0, |b| b.start);
    let r = apply_header_footer(pdfium, &mut l.doc, spec, FULL, start)?;
    if r.stamped == 0 {
        return Err(EngineError::Pdfium("phạm vi trang không có trang nào".into()));
    }
    cos::save_lodoc(&mut l, output)
}

/// Đánh số Bates cho nhiều tệp theo thứ tự `jobs` (input → output): số nối
/// tiếp qua các tệp, bắt đầu từ `spec.bates.start`. `spec` phải có `bates` và
/// dùng token `{bates}` ở ít nhất 1 ô.
pub fn add_bates(
    pdfium: &Pdfium,
    jobs: &[(PathBuf, PathBuf)],
    spec: &HeaderFooterSpec,
) -> Result<Vec<BatesRange>, EngineError> {
    let fmt = spec
        .bates
        .as_ref()
        .ok_or_else(|| EngineError::Pdfium("thiếu định dạng số Bates".into()))?;
    let mut next = fmt.start;
    let mut out = Vec::new();
    for (input, output) in jobs {
        let mut l = cos::load_lodoc(pdfium, input, None)?;
        let r = apply_header_footer(pdfium, &mut l.doc, spec, FULL, next)?;
        cos::save_lodoc(&mut l, output)?;
        let (first, last) = match (r.first_bates, r.last_bates) {
            (Some(a), Some(b)) => {
                next = b + 1;
                (fmt.format(a), fmt.format(b))
            }
            _ => (String::new(), String::new()),
        };
        out.push(BatesRange { output: output.clone(), first, last, pages: r.stamped });
    }
    Ok(out)
}

/// Gỡ mọi dấu trang thuộc `kinds` (do FoFreeXit, Acrobat hay Foxit thêm, nhận
/// qua `/Artifact` Pagination; hình mờ gồm cả annotation /Watermark và lớp OCG
/// tên "Watermark"). Trả về số khối đã gỡ (0 = không có gì, vẫn ghi `output`).
pub fn remove_page_marks(
    pdfium: &Pdfium,
    input: &Path,
    kinds: &[MarkKind],
    output: &Path,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    let mut l = cos::load_lodoc(pdfium, input, password)?;
    let ids: Vec<ObjectId> = l.doc.get_pages().values().copied().collect();
    let mut n = 0;
    for id in ids {
        n += cos::strip_page(&mut l.doc, id, kinds)?;
    }
    if n > 0 {
        l.doc.prune_objects();
    }
    cos::save_lodoc(&mut l, output)?;
    Ok(n)
}

/// Đếm số trang có từng loại dấu.
pub fn scan_page_marks(pdfium: &Pdfium, input: &Path, password: Option<&str>) -> Result<MarkCounts, EngineError> {
    let doc = cos::load_lodoc(pdfium, input, password)?.doc;
    let mut c = MarkCounts::default();
    for id in doc.get_pages().values() {
        let kinds = cos::page_mark_kinds(&doc, *id);
        if kinds.contains(&MarkKind::Watermark) { c.watermark += 1; }
        if kinds.contains(&MarkKind::Header) || kinds.contains(&MarkKind::Footer) { c.header_footer += 1; }
        if kinds.contains(&MarkKind::Background) { c.background += 1; }
        if kinds.contains(&MarkKind::Bates) { c.bates += 1; }
    }
    Ok(c)
}

/// Việc cần xem trước.
pub enum PageMarkJob<'a> {
    Watermark(&'a WatermarkSpec),
    Background(&'a WatermarkSpec),
    HeaderFooter(&'a HeaderFooterSpec),
}

/// Xem trước: trích RIÊNG trang `page` (nhanh cả với tệp nghìn trang), áp dấu
/// với số trang/tổng như tài liệu thật, ghi PDF 1 trang ra `output` để render.
pub fn preview_page_mark(
    pdfium: &Pdfium,
    input: &Path,
    page: u16,
    job: PageMarkJob,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let src = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(format!("mở {}: {e}", input.display())))?;
    let total = src.pages().len();
    if page >= total {
        return Err(EngineError::Pdfium(format!("không có trang {}", page + 1)));
    }
    let mut one = pdfium.create_new_pdf().map_err(perr("tạo bản xem trước"))?;
    one.pages_mut().copy_page_from_document(&src, page, 0).map_err(perr("trích trang"))?;
    let bytes = one.save_to_bytes().map_err(perr("lưu bản xem trước"))?;
    drop(one);
    drop(src);
    let mut l = cos::Loaded::plain(LoDoc::load_mem(&bytes).map_err(lerr("đọc bản xem trước"))?);
    let ctx = Ctx { index_base: page, total: Some(total), force_all: true };
    match job {
        PageMarkJob::Watermark(s) => {
            apply_stamp(pdfium, &mut l.doc, s, MarkKind::Watermark, ctx)?;
        }
        PageMarkJob::Background(s) => {
            apply_stamp(pdfium, &mut l.doc, s, MarkKind::Background, ctx)?;
        }
        PageMarkJob::HeaderFooter(s) => {
            // Bates: giả định mọi trang trước đó đều được đánh số.
            let start = s.bates.as_ref().map_or(0, |b| b.start + page as u64);
            apply_header_footer(pdfium, &mut l.doc, s, ctx, start)?;
        }
    }
    cos::save_lodoc(&mut l, output)
}
