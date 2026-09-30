//! Thao tác OBJECT chuẩn Foxit "Edit Object" (bổ sung cho `edit.rs`): xoay/lật
//! object quanh tâm, cắt ảnh, độ mờ, vẽ hình (Add Shapes: chữ nhật / bo góc /
//! elip / đường thẳng / mũi tên), đổi viền-nền-độ dày path, nhân bản object
//! (Ctrl+D / Copy-Paste), sắp xếp lớp (Arrange: lên trên cùng / xuống dưới
//! cùng / lên 1 lớp / xuống 1 lớp).
//!
//! Mọi hàm ở đây thao tác trên page object của PDFium ĐÃ MỞ — `apply_edits`
//! gọi trong pha (C) và lo regenerate + lưu. Quy ước ma trận giống `edit.rs`:
//! hàng p' = p·M, `apply_matrix(X)` = M rồi X (PDFium `Concat`).
//!
//! Ghi chú độ tin cậy:
//! - CẮT ẢNH: KHÔNG dùng clip path (PDFium không ghi lại clip của image object
//!   khi regenerate content) — mã hoá lại phần điểm ảnh giữ lại + đặt lại ma
//!   trận để phần giữ lại nằm đúng chỗ cũ. Kết quả xác định, kiểm được bằng pixel.
//! - ĐỘ MỜ ẢNH: PDFium chỉ ghi ExtGState (ca/CA) cho text/path; image object
//!   ghi "q cm /Im Do Q" trần → nướng alpha vào ảnh (SMask) khi mã hoá lại.
//! - SẮP LỚP: PDFium ghi lại object theo THỨ TỰ DANH SÁCH nhưng trong từng
//!   content stream riêng → trang nhiều stream phải gộp về 1 stream trước
//!   (`merge_page_contents`), nếu không object ở stream sau luôn nằm trên.

use std::path::Path;

use image::{DynamicImage, GenericImageView, RgbaImage};
use pdfium_render::prelude::*;

use crate::text::Rect;
use crate::EngineError;

/// Loại hình vẽ mới (Foxit Edit > Add Shapes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    RoundRect,
    Ellipse,
    Line,
    Arrow,
}

impl ShapeKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "rect" => ShapeKind::Rect,
            "roundRect" => ShapeKind::RoundRect,
            "ellipse" => ShapeKind::Ellipse,
            "line" => ShapeKind::Line,
            "arrow" => ShapeKind::Arrow,
            _ => return None,
        })
    }
}

/// Đặc tả 1 hình vẽ mới (điểm PDF, gốc dưới-trái trang).
#[derive(Clone, Debug)]
pub struct ShapeSpec {
    pub kind: ShapeKind,
    /// Rect/RoundRect/Ellipse: 2 góc đối (thứ tự tuỳ ý). Line/Arrow: đầu → cuối.
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Màu viền; None = không viền (Line/Arrow luôn có viền — None ≙ đen).
    pub stroke: Option<[u8; 3]>,
    /// Màu nền; None = không nền. Arrow: đầu mũi tên tô bằng màu viền.
    pub fill: Option<[u8; 3]>,
    pub width: f32,
    /// Mẫu nét đứt (điểm PDF); rỗng = nét liền.
    pub dash: Vec<f32>,
    /// Độ mờ 0..1 (1 = đục).
    pub opacity: f32,
}

/// Kiểu sắp lớp (Foxit Arrange).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeMode {
    Front,
    Back,
    Forward,
    Backward,
}

impl ArrangeMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "front" => ArrangeMode::Front,
            "back" => ArrangeMode::Back,
            "forward" => ArrangeMode::Forward,
            "backward" => ArrangeMode::Backward,
            _ => return None,
        })
    }
}

/// Đổi kiểu vẽ của path object — mọi field None/false = giữ nguyên.
#[derive(Clone, Debug, Default)]
pub struct PathStyle {
    pub stroke: Option<[u8; 3]>,
    pub no_stroke: bool,
    pub fill: Option<[u8; 3]>,
    pub no_fill: bool,
    pub width: Option<f32>,
    /// Some(vec![]) = đổi về nét liền.
    pub dash: Option<Vec<f32>>,
}

/// Thuộc tính hiển thị của 1 object cho panel Format của UI.
#[derive(Clone, Debug, Default)]
pub struct ObjStyle {
    pub stroke_color: Option<[u8; 4]>,
    pub fill_color: Option<[u8; 4]>,
    pub stroke_width: Option<f32>,
    pub opacity: Option<f32>,
    pub rotation: Option<f32>,
    pub image_px: Option<(u32, u32)>,
}

type Mat6 = (f32, f32, f32, f32, f32, f32);

fn m6(m: &PdfMatrix) -> Mat6 {
    (m.a(), m.b(), m.c(), m.d(), m.e(), m.f())
}

fn pm(m: Mat6) -> PdfMatrix {
    PdfMatrix::new(m.0, m.1, m.2, m.3, m.4, m.5)
}

/// K = M rồi N.
fn mul(m: Mat6, n: Mat6) -> Mat6 {
    (
        m.0 * n.0 + m.1 * n.2,
        m.0 * n.1 + m.1 * n.3,
        m.2 * n.0 + m.3 * n.2,
        m.2 * n.1 + m.3 * n.3,
        m.4 * n.0 + m.5 * n.2 + n.4,
        m.4 * n.1 + m.5 * n.3 + n.5,
    )
}

fn inv(m: Mat6) -> Option<Mat6> {
    let det = m.0 * m.3 - m.1 * m.2;
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b, c, d) = (m.3 / det, -m.1 / det, -m.2 / det, m.0 / det);
    Some((a, b, c, d, -(m.4 * a + m.5 * c), -(m.4 * b + m.5 * d)))
}

fn apply(m: Mat6, x: f32, y: f32) -> (f32, f32) {
    (m.0 * x + m.2 * y + m.4, m.1 * x + m.3 * y + m.5)
}

fn err(e: PdfiumError) -> EngineError {
    EngineError::Pdfium(format!("thao tác object: {e}"))
}

fn bounds_rect(obj: &PdfPageObject<'_>) -> Result<Rect, EngineError> {
    let q = obj.bounds().map_err(err)?;
    Ok(Rect {
        left: q.left().value,
        bottom: q.bottom().value,
        right: q.right().value,
        top: q.top().value,
    })
}

/// Tâm quay/lật: `center` nếu có, ngược lại tâm khung bao của object.
fn pivot(obj: &PdfPageObject<'_>, center: Option<(f32, f32)>) -> Result<(f32, f32), EngineError> {
    if let Some(c) = center {
        return Ok(c);
    }
    let r = bounds_rect(obj)?;
    Ok(((r.left + r.right) / 2.0, (r.bottom + r.top) / 2.0))
}

/// Xoay object `degrees` độ NGƯỢC chiều kim đồng hồ (dương = xoay trái, như
/// hệ toạ độ PDF y-lên) quanh `center` (None = TÂM khung bao hiện tại).
pub(crate) fn rotate_object(
    obj: &mut PdfPageObject<'_>,
    degrees: f32,
    center: Option<(f32, f32)>,
) -> Result<(), EngineError> {
    if degrees.abs() < 1e-4 {
        return Ok(());
    }
    let (cx, cy) = pivot(obj, center)?;
    let t = degrees.to_radians();
    let (s, c) = (t.sin(), t.cos());
    // T(-c) · R · T(c) theo quy ước hàng.
    let x = (c, s, -s, c, -cx * c + cy * s + cx, -cx * s - cy * c + cy);
    obj.apply_matrix(pm(x)).map_err(err)
}

/// Lật object quanh trục dọc (horizontal=true, trái↔phải) hoặc trục ngang qua
/// `center` (None = tâm khung bao).
pub(crate) fn flip_object(
    obj: &mut PdfPageObject<'_>,
    horizontal: bool,
    center: Option<(f32, f32)>,
) -> Result<(), EngineError> {
    let (cx, cy) = pivot(obj, center)?;
    let x = if horizontal {
        (-1.0, 0.0, 0.0, 1.0, 2.0 * cx, 0.0)
    } else {
        (1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * cy)
    };
    obj.apply_matrix(pm(x)).map_err(err)
}

/// Ảnh gốc của image object dạng RGBA (điểm ảnh thô, chưa áp ma trận).
fn raw_rgba(obj: &PdfPageObject<'_>) -> Result<RgbaImage, EngineError> {
    let img = obj
        .as_image_object()
        .ok_or_else(|| EngineError::Pdfium("object không phải ảnh".into()))?
        .get_raw_image()
        .map_err(|e| EngineError::Pdfium(format!("đọc điểm ảnh: {e}")))?;
    Ok(img.to_rgba8())
}

/// Cắt ảnh: giữ phần giao của `keep` (điểm PDF trên trang) với ảnh. Điểm ảnh
/// ngoài vùng giữ bị bỏ HẲN khỏi file (mã hoá lại), ma trận đặt lại để phần
/// còn lại nằm đúng vị trí cũ (ảnh xoay/lật vẫn đúng: quy về không gian ảnh).
pub(crate) fn crop_image(
    obj: &mut PdfPageObject<'_>,
    keep: &Rect,
    document: &PdfDocument<'_>,
) -> Result<(), EngineError> {
    let m = m6(&obj.matrix().map_err(err)?);
    let mi = inv(m).ok_or_else(|| EngineError::Pdfium("ma trận ảnh suy biến".into()))?;
    // 4 góc vùng giữ → không gian đơn vị của ảnh → khung bao, kẹp [0,1].
    let pts = [
        apply(mi, keep.left, keep.bottom),
        apply(mi, keep.right, keep.bottom),
        apply(mi, keep.left, keep.top),
        apply(mi, keep.right, keep.top),
    ];
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    let u0 = clamp(pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min));
    let u1 = clamp(pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max));
    let v0 = clamp(pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min));
    let v1 = clamp(pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max));

    let src = raw_rgba(obj)?;
    let (w, h) = src.dimensions();
    // Hàng 0 của dữ liệu ảnh = MÉP TRÊN (v = 1) của không gian ảnh.
    let px0 = (u0 * w as f32).floor() as u32;
    let px1 = ((u1 * w as f32).ceil() as u32).min(w);
    let py0 = ((1.0 - v1) * h as f32).floor() as u32;
    let py1 = (((1.0 - v0) * h as f32).ceil() as u32).min(h);
    if px1 <= px0 || py1 <= py0 {
        return Err(EngineError::Pdfium("vùng cắt không giao với ảnh".into()));
    }
    if px0 == 0 && py0 == 0 && px1 == w && py1 == h {
        return Ok(()); // giữ nguyên cả ảnh
    }
    let mut cropped = image::imageops::crop_imm(&src, px0, py0, px1 - px0, py1 - py0).to_image();
    // Điểm ảnh thô không mang SMask → giữ độ đục đã đặt trước đó (nướng lại).
    if let Some(op) = image_opacity(obj, document).filter(|o| *o < 0.995) {
        let a = (op * 255.0).round() as u16;
        for p in cropped.pixels_mut() {
            p.0[3] = ((p.0[3] as u16 * a + 127) / 255) as u8;
        }
    }
    // Toạ độ đơn vị CHÍNH XÁC theo biên pixel đã làm tròn.
    let (fu0, fu1) = (px0 as f32 / w as f32, px1 as f32 / w as f32);
    let (fv0, fv1) = (1.0 - py1 as f32 / h as f32, 1.0 - py0 as f32 / h as f32);
    let s: Mat6 = (fu1 - fu0, 0.0, 0.0, fv1 - fv0, fu0, fv0);
    {
        let img = obj
            .as_image_object_mut()
            .ok_or_else(|| EngineError::Pdfium("object không phải ảnh".into()))?;
        img.set_image(&DynamicImage::ImageRgba8(cropped)).map_err(err)?;
    }
    obj.reset_matrix(pm(mul(s, m))).map_err(err)?;
    Ok(())
}

/// Độ mờ 0..1 cho object. Text/path: alpha của màu tô + màu viền (PDFium
/// ghi ExtGState ca/CA). Ảnh: nướng alpha vào điểm ảnh (SMask) khi mã hoá
/// lại — alpha mới = alpha điểm ảnh gốc × độ mờ.
pub(crate) fn set_opacity(obj: &mut PdfPageObject<'_>, opacity: f32) -> Result<(), EngineError> {
    let a = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    if obj.object_type() == PdfPageObjectType::Image {
        let src = raw_rgba(obj)?;
        let mut out = src.clone();
        for p in out.pixels_mut() {
            p.0[3] = ((p.0[3] as u16 * a as u16 + 127) / 255) as u8;
        }
        let m = obj.matrix().map_err(err)?;
        {
            let img = obj
                .as_image_object_mut()
                .ok_or_else(|| EngineError::Pdfium("object không phải ảnh".into()))?;
            img.set_image(&DynamicImage::ImageRgba8(out)).map_err(err)?;
        }
        obj.reset_matrix(m).map_err(err)?;
        return Ok(());
    }
    if let Ok(c) = obj.fill_color() {
        obj.set_fill_color(PdfColor::new(c.red(), c.green(), c.blue(), a)).map_err(err)?;
    }
    if let Ok(c) = obj.stroke_color() {
        obj.set_stroke_color(PdfColor::new(c.red(), c.green(), c.blue(), a)).map_err(err)?;
    }
    Ok(())
}

/// Đổi viền/nền/độ dày/nét đứt của path object.
pub(crate) fn set_path_style(obj: &mut PdfPageObject<'_>, st: &PathStyle) -> Result<(), EngineError> {
    let Some(path) = obj.as_path_object_mut() else {
        return Err(EngineError::Pdfium("object không phải hình (path)".into()));
    };
    let mut fill_mode = path.fill_mode().unwrap_or(PdfPathFillMode::None);
    let mut stroked = path.is_stroked().unwrap_or(false);
    if let Some(c) = st.stroke {
        let a = path.stroke_color().map(|c| c.alpha()).unwrap_or(255);
        path.set_stroke_color(PdfColor::new(c[0], c[1], c[2], a)).map_err(err)?;
        if !stroked && path.stroke_width().map(|w| w.value <= 0.0).unwrap_or(true) {
            path.set_stroke_width(PdfPoints::new(1.0)).map_err(err)?;
        }
        stroked = true;
    }
    if st.no_stroke {
        stroked = false;
    }
    if let Some(c) = st.fill {
        let a = path.fill_color().map(|c| c.alpha()).unwrap_or(255);
        path.set_fill_color(PdfColor::new(c[0], c[1], c[2], a)).map_err(err)?;
        if fill_mode == PdfPathFillMode::None {
            fill_mode = PdfPathFillMode::Winding;
        }
    }
    if st.no_fill {
        fill_mode = PdfPathFillMode::None;
    }
    if let Some(w) = st.width {
        path.set_stroke_width(PdfPoints::new(w.max(0.0))).map_err(err)?;
        if w > 0.0 && st.stroke.is_none() && !st.no_stroke {
            stroked = true;
        }
    }
    if let Some(d) = &st.dash {
        let arr: Vec<PdfPoints> = d.iter().map(|v| PdfPoints::new(v.max(0.0))).collect();
        path.set_dash_array(&arr, PdfPoints::ZERO).map_err(err)?;
    }
    path.set_fill_and_stroke_mode(fill_mode, stroked).map_err(err)
}

/// Thuộc tính Format của object (viền/nền/độ dày/độ mờ/góc xoay/kích thước ảnh).
/// Độ đục hiện tại của ảnh = alpha LỚN NHẤT của ảnh đã áp mask (SMask) — ảnh
/// đã nướng độ đục có alpha đồng đều; ảnh PNG trong suốt vẫn có điểm đục 255.
pub(crate) fn image_opacity(obj: &PdfPageObject<'_>, document: &PdfDocument<'_>) -> Option<f32> {
    let img = obj.as_image_object()?.get_processed_image(document).ok()?;
    let max = img.to_rgba8().pixels().map(|p| p.0[3]).max()?;
    Some(max as f32 / 255.0)
}

/// `document`: có thì đo độ đục ảnh (render ảnh đã áp mask — chỉ gọi cho số ít ảnh).
pub(crate) fn object_style(
    obj: &PdfPageObject<'_>,
    acc: (f32, f32, f32, f32, f32, f32),
    document: Option<&PdfDocument<'_>>,
) -> ObjStyle {
    let mut st = ObjStyle::default();
    let col = |c: PdfColor| [c.red(), c.green(), c.blue(), c.alpha()];
    let m = obj.matrix().map(|m| mul(m6(&m), acc)).unwrap_or(acc);
    let rot = m.1.atan2(m.0).to_degrees();
    st.rotation = Some(if rot.abs() < 0.05 { 0.0 } else { rot });
    match obj {
        PdfPageObject::Path(p) => {
            let stroked = p.is_stroked().unwrap_or(false);
            let filled = p.fill_mode().map(|f| f != PdfPathFillMode::None).unwrap_or(false);
            if stroked {
                st.stroke_color = obj.stroke_color().ok().map(col);
                st.stroke_width = obj.stroke_width().ok().map(|w| w.value);
            }
            if filled {
                st.fill_color = obj.fill_color().ok().map(col);
            }
            let a = if filled {
                st.fill_color.map(|c| c[3])
            } else {
                st.stroke_color.map(|c| c[3])
            };
            st.opacity = Some(a.unwrap_or(255) as f32 / 255.0);
        }
        PdfPageObject::Image(i) => {
            if let (Ok(w), Ok(h)) = (i.width(), i.height()) {
                st.image_px = Some((w as u32, h as u32));
            }
            st.opacity = document.and_then(|d| image_opacity(obj, d));
        }
        _ => {
            st.opacity = obj.fill_color().ok().map(|c| c.alpha() as f32 / 255.0);
        }
    }
    st
}

// ---------- Hình vẽ mới ----------

fn color(c: [u8; 3], a: u8) -> PdfColor {
    PdfColor::new(c[0], c[1], c[2], a)
}

/// Dựng path object cho hình mới (chưa thêm vào trang).
pub(crate) fn build_shape<'a>(
    document: &PdfDocument<'a>,
    s: &ShapeSpec,
) -> Result<PdfPagePathObject<'a>, EngineError> {
    let p = |v: f32| PdfPoints::new(v);
    let a = (s.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (l, r) = (s.x0.min(s.x1), s.x0.max(s.x1));
    let (b, t) = (s.y0.min(s.y1), s.y0.max(s.y1));
    let is_line = matches!(s.kind, ShapeKind::Line | ShapeKind::Arrow);
    let width = s.width.max(0.0);
    let stroke = if is_line { Some(s.stroke.unwrap_or([0, 0, 0])) } else { s.stroke };
    let fill = match s.kind {
        ShapeKind::Line => None,
        ShapeKind::Arrow => stroke,
        _ => s.fill,
    };
    let k = 0.552_284_8_f32;
    let (sx, sy) = match s.kind {
        ShapeKind::Line | ShapeKind::Arrow => (s.x0, s.y0),
        ShapeKind::Rect => (l, b),
        ShapeKind::RoundRect => {
            let rr = ((r - l).min(t - b) * 0.18).max(0.0);
            (l + rr, b)
        }
        ShapeKind::Ellipse => (r, (b + t) / 2.0),
    };
    let mut path = PdfPagePathObject::new(document, p(sx), p(sy), None, None, None).map_err(err)?;
    match s.kind {
        ShapeKind::Rect => {
            path.line_to(p(r), p(b)).map_err(err)?;
            path.line_to(p(r), p(t)).map_err(err)?;
            path.line_to(p(l), p(t)).map_err(err)?;
            path.close_path().map_err(err)?;
        }
        ShapeKind::RoundRect => {
            let rr = ((r - l).min(t - b) * 0.18).max(0.0);
            let kr = k * rr;
            path.line_to(p(r - rr), p(b)).map_err(err)?;
            path.bezier_to(p(r), p(b + rr), p(r - rr + kr), p(b), p(r), p(b + rr - kr)).map_err(err)?;
            path.line_to(p(r), p(t - rr)).map_err(err)?;
            path.bezier_to(p(r - rr), p(t), p(r), p(t - rr + kr), p(r - rr + kr), p(t)).map_err(err)?;
            path.line_to(p(l + rr), p(t)).map_err(err)?;
            path.bezier_to(p(l), p(t - rr), p(l + rr - kr), p(t), p(l), p(t - rr + kr)).map_err(err)?;
            path.line_to(p(l), p(b + rr)).map_err(err)?;
            path.bezier_to(p(l + rr), p(b), p(l), p(b + rr - kr), p(l + rr - kr), p(b)).map_err(err)?;
            path.close_path().map_err(err)?;
        }
        ShapeKind::Ellipse => {
            let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
            let (rx, ry) = ((r - l) / 2.0, (t - b) / 2.0);
            let (kx, ky) = (k * rx, k * ry);
            path.bezier_to(p(cx), p(t), p(r), p(cy + ky), p(cx + kx), p(t)).map_err(err)?;
            path.bezier_to(p(l), p(cy), p(cx - kx), p(t), p(l), p(cy + ky)).map_err(err)?;
            path.bezier_to(p(cx), p(b), p(l), p(cy - ky), p(cx - kx), p(b)).map_err(err)?;
            path.bezier_to(p(r), p(cy), p(cx + kx), p(b), p(r), p(cy - ky)).map_err(err)?;
            path.close_path().map_err(err)?;
        }
        ShapeKind::Line => {
            path.line_to(p(s.x1), p(s.y1)).map_err(err)?;
        }
        ShapeKind::Arrow => {
            let (dx, dy) = (s.x1 - s.x0, s.y1 - s.y0);
            let len = (dx * dx + dy * dy).sqrt().max(0.01);
            let (ux, uy) = (dx / len, dy / len);
            let (nx, ny) = (-uy, ux);
            let head = (width * 4.0).max(8.0).min(len * 0.6);
            let hw = head * 0.45;
            // Nét viền nối góc MITER ở mũi nhọn lòi ra (w/2)/sin(nửa góc mũi)
            // → lùi mũi vào đúng lượng đó để đầu nhọn chạm ĐÚNG điểm cuối.
            let ext = (width / 2.0) / (hw / head).atan().sin().max(0.1);
            let (tx, ty) = (s.x1 - ux * ext, s.y1 - uy * ext);
            let (bx, by) = (tx - ux * head, ty - uy * head);
            // Thân dừng ở CHÂN đầu mũi tên — nét dày không lòi qua mũi nhọn.
            path.line_to(p(bx), p(by)).map_err(err)?;
            path.move_to(p(tx), p(ty)).map_err(err)?;
            path.line_to(p(bx + nx * hw), p(by + ny * hw)).map_err(err)?;
            path.line_to(p(bx - nx * hw), p(by - ny * hw)).map_err(err)?;
            path.close_path().map_err(err)?;
        }
    }
    let stroked = stroke.is_some() && width > 0.0;
    if let Some(c) = stroke {
        path.set_stroke_color(color(c, a)).map_err(err)?;
        path.set_stroke_width(p(width.max(0.1))).map_err(err)?;
    }
    if let Some(c) = fill {
        path.set_fill_color(color(c, a)).map_err(err)?;
    }
    if !s.dash.is_empty() && !matches!(s.kind, ShapeKind::Arrow) {
        let arr: Vec<PdfPoints> = s.dash.iter().map(|v| p(v.max(0.1))).collect();
        path.set_dash_array(&arr, PdfPoints::ZERO).map_err(err)?;
    }
    if s.kind == ShapeKind::Arrow {
        path.set_line_join(PdfPageObjectLineJoin::Miter).map_err(err)?;
    }
    let mode = if fill.is_some() { PdfPathFillMode::Winding } else { PdfPathFillMode::None };
    path.set_fill_and_stroke_mode(mode, stroked).map_err(err)?;
    Ok(path)
}

// ---------- Nhân bản object ----------

/// Bản chụp đủ dữ liệu để TẠO LẠI 1 object (cùng trang hoặc trang khác của
/// cùng document) — chụp trước rồi tạo sau để không phải mượn 2 object cùng lúc.
pub(crate) enum CloneSpec {
    Text {
        text: String,
        token: PdfFontToken,
        tf: f32,
        matrix: PdfMatrix,
        fill: PdfColor,
        stroke: PdfColor,
        stroke_width: f32,
        mode: PdfPageTextRenderMode,
    },
    Path {
        /// (loại, x, y, đóng subpath) — Bezier = 3 đoạn BezierTo liên tiếp.
        segs: Vec<(PdfPathSegmentType, f32, f32, bool)>,
        matrix: PdfMatrix,
        fill_mode: PdfPathFillMode,
        stroked: bool,
        fill: PdfColor,
        stroke: PdfColor,
        width: f32,
        cap: PdfPageObjectLineCap,
        join: PdfPageObjectLineJoin,
        dash: Vec<PdfPoints>,
        dash_phase: PdfPoints,
    },
    Image {
        img: DynamicImage,
        matrix: PdfMatrix,
    },
}

/// `acc` = tích ma trận form tổ tiên (object trong Form XObject) — bản sao
/// tạo ở cấp trang nên mang ma trận đã quy về trang.
pub(crate) fn capture_clone(obj: &PdfPageObject<'_>, acc: Mat6) -> Result<CloneSpec, EngineError> {
    let matrix = pm(mul(m6(&obj.matrix().map_err(err)?), acc));
    match obj {
        PdfPageObject::Text(t) => Ok(CloneSpec::Text {
            text: t.text(),
            token: t.font().token(),
            tf: t.unscaled_font_size().value,
            matrix,
            fill: t.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
            stroke: t.stroke_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
            stroke_width: t.stroke_width().map(|w| w.value).unwrap_or(1.0),
            mode: t.render_mode(),
        }),
        PdfPageObject::Path(p) => {
            let segs = p
                .segments()
                .raw()
                .iter()
                .map(|s| {
                    let (x, y) = s.point();
                    (s.segment_type(), x.value, y.value, s.is_close())
                })
                .collect();
            Ok(CloneSpec::Path {
                segs,
                matrix,
                fill_mode: p.fill_mode().unwrap_or(PdfPathFillMode::None),
                stroked: p.is_stroked().unwrap_or(false),
                fill: p.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
                stroke: p.stroke_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
                width: p.stroke_width().map(|w| w.value).unwrap_or(1.0),
                cap: p.line_cap().unwrap_or(PdfPageObjectLineCap::Butt),
                join: p.line_join().unwrap_or(PdfPageObjectLineJoin::Miter),
                dash: p.dash_array().unwrap_or_default(),
                dash_phase: p.dash_phase().unwrap_or(PdfPoints::ZERO),
            })
        }
        PdfPageObject::Image(i) => Ok(CloneSpec::Image {
            img: i
                .get_raw_image()
                .map_err(|e| EngineError::Pdfium(format!("đọc điểm ảnh: {e}")))?,
            matrix,
        }),
        _ => Err(EngineError::Pdfium(
            "chỉ nhân bản được chữ / ảnh / hình (path)".into(),
        )),
    }
}

/// Tạo object từ bản chụp, dịch (dx,dy) rồi thêm vào CUỐI trang (trên cùng).
pub(crate) fn create_clone<'a>(
    document: &PdfDocument<'a>,
    page: &mut PdfPage<'a>,
    spec: &CloneSpec,
    dx: f32,
    dy: f32,
) -> Result<(), EngineError> {
    let shift = |m: &PdfMatrix| PdfMatrix::new(m.a(), m.b(), m.c(), m.d(), m.e() + dx, m.f() + dy);
    match spec {
        CloneSpec::Text { text, token, tf, matrix, fill, stroke, stroke_width, mode } => {
            let mut obj = page
                .objects_mut()
                .create_text_object(PdfPoints::ZERO, PdfPoints::ZERO, text.clone(), *token, PdfPoints::new(*tf))
                .map_err(err)?;
            obj.reset_matrix(shift(matrix)).map_err(err)?;
            obj.set_fill_color(*fill).map_err(err)?;
            obj.set_stroke_color(*stroke).map_err(err)?;
            obj.set_stroke_width(PdfPoints::new(*stroke_width)).map_err(err)?;
            if let Some(t) = obj.as_text_object_mut() {
                let _ = t.set_render_mode(*mode);
            }
        }
        CloneSpec::Path { segs, matrix, fill_mode, stroked, fill, stroke, width, cap, join, dash, dash_phase } => {
            let (x0, y0) = segs.first().map(|s| (s.1, s.2)).unwrap_or((0.0, 0.0));
            let p = |v: f32| PdfPoints::new(v);
            let mut path = PdfPagePathObject::new(document, p(x0), p(y0), None, None, None).map_err(err)?;
            let mut i = 1usize;
            if segs.first().map(|s| s.3).unwrap_or(false) {
                path.close_path().map_err(err)?;
            }
            while i < segs.len() {
                let (kind, x, y, close) = segs[i];
                match kind {
                    PdfPathSegmentType::MoveTo => path.move_to(p(x), p(y)).map_err(err)?,
                    PdfPathSegmentType::LineTo => path.line_to(p(x), p(y)).map_err(err)?,
                    PdfPathSegmentType::BezierTo if i + 2 < segs.len() => {
                        let (c1, c2, e) = (segs[i], segs[i + 1], segs[i + 2]);
                        path.bezier_to(p(e.1), p(e.2), p(c1.1), p(c1.2), p(c2.1), p(c2.2)).map_err(err)?;
                        i += 2;
                        if e.3 {
                            path.close_path().map_err(err)?;
                        }
                        i += 1;
                        continue;
                    }
                    _ => path.line_to(p(x), p(y)).map_err(err)?,
                }
                if close {
                    path.close_path().map_err(err)?;
                }
                i += 1;
            }
            path.set_fill_color(*fill).map_err(err)?;
            path.set_stroke_color(*stroke).map_err(err)?;
            path.set_stroke_width(p(*width)).map_err(err)?;
            path.set_line_cap(*cap).map_err(err)?;
            path.set_line_join(*join).map_err(err)?;
            if !dash.is_empty() {
                path.set_dash_array(dash, *dash_phase).map_err(err)?;
            }
            path.set_fill_and_stroke_mode(*fill_mode, *stroked).map_err(err)?;
            let mut obj = page.objects_mut().add_path_object(path).map_err(err)?;
            obj.reset_matrix(shift(matrix)).map_err(err)?;
        }
        CloneSpec::Image { img, matrix } => {
            let mut obj = page
                .objects_mut()
                .create_image_object(PdfPoints::ZERO, PdfPoints::ZERO, img, None, None)
                .map_err(err)?;
            obj.reset_matrix(shift(matrix)).map_err(err)?;
        }
    }
    Ok(())
}

// ---------- Sắp lớp (z-order) ----------

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.bottom < b.top && b.bottom < a.top
}

/// Thứ tự mới của danh sách object cấp trang `n` phần tử khi áp `mode` cho
/// tập `sel` (index hiện tại). Object không chọn giữ nguyên thứ tự tương đối.
/// Lên/xuống 1 lớp: vượt qua object KHÔNG chọn kế tiếp CÓ CHỒNG LÊN object
/// (như Foxit — vượt 1 object không chồng thì không thấy khác gì); không có
/// object chồng nào → vượt object kế tiếp.
pub(crate) fn arrange_order(n: usize, sel: &[usize], rects: &[Rect], mode: ArrangeMode) -> Vec<usize> {
    let is_sel = |i: usize| sel.contains(&i);
    let mut order: Vec<usize> = (0..n).collect();
    match mode {
        ArrangeMode::Front => {
            order.retain(|&i| !is_sel(i));
            let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < n).collect();
            s.sort_unstable();
            s.dedup();
            order.extend(s);
        }
        ArrangeMode::Back => {
            let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < n).collect();
            s.sort_unstable();
            s.dedup();
            order.retain(|&i| !is_sel(i));
            s.extend(order);
            order = s;
        }
        ArrangeMode::Forward | ArrangeMode::Backward => {
            let fwd = mode == ArrangeMode::Forward;
            let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < n).collect();
            s.sort_unstable();
            s.dedup();
            if fwd {
                s.reverse(); // xử lý object trên cùng trước để không vượt lẫn nhau
            }
            for id in s {
                let pos = order.iter().position(|&x| x == id).unwrap_or(0);
                let cands: Vec<usize> = if fwd {
                    (pos + 1..order.len()).filter(|&j| !is_sel(order[j])).collect()
                } else {
                    (0..pos).rev().filter(|&j| !is_sel(order[j])).collect()
                };
                let target = cands
                    .iter()
                    .copied()
                    .find(|&j| rects.get(order[j]).zip(rects.get(id)).map_or(false, |(a, b)| overlap(a, b)))
                    .or_else(|| cands.first().copied());
                let Some(j) = target else { continue };
                let item = order.remove(pos);
                // Tiến: sau khi gỡ, object đích lùi 1 chỗ → chèn tại j đặt NGAY TRÊN nó.
                // Lùi: chèn tại j đặt NGAY DƯỚI object đích.
                order.insert(j, item);
            }
        }
    }
    order
}

/// Áp sắp lớp cho các object cấp trang `sel` (index hiện tại) trên `page`.
pub(crate) fn arrange(page: &mut PdfPage<'_>, sel: &[usize], mode: ArrangeMode) -> Result<(), EngineError> {
    let n = page.objects().len();
    if n == 0 || sel.is_empty() {
        return Ok(());
    }
    let rects: Vec<Rect> = (0..n)
        .map(|i| {
            page.objects()
                .get(i)
                .ok()
                .and_then(|o| bounds_rect(&o).ok())
                .unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 })
        })
        .collect();
    let order = arrange_order(n, sel, &rects, mode);
    let mut moving: Vec<usize> = sel.iter().copied().filter(|&i| i < n).collect();
    moving.sort_unstable();
    moving.dedup();
    // Chỉ gỡ/chèn object ĐỔI chỗ — vị trí đích = chỗ trong thứ tự mới.
    let target: Vec<(usize, usize)> = moving
        .iter()
        .map(|&id| (id, order.iter().position(|&x| x == id).unwrap_or(id)))
        .collect();
    if target.iter().all(|&(id, pos)| id == pos) && order.iter().enumerate().all(|(i, &x)| i == x) {
        return Ok(());
    }
    let mut removed: Vec<(usize, PdfPageObject<'_>)> = Vec::new();
    for &id in moving.iter().rev() {
        let obj = page.objects_mut().remove_object_at_index(id).map_err(err)?;
        let pos = target.iter().find(|t| t.0 == id).map(|t| t.1).unwrap_or(id);
        removed.push((pos, obj));
    }
    removed.sort_by_key(|r| r.0);
    let mut iter = removed.into_iter();
    while let Some((pos, obj)) = iter.next() {
        if let Err(e) = page.objects_mut().insert_object_at_index(pos, obj) {
            // Object đã tách chưa gắn lại: forget để Drop không destroy (bẫy segfault).
            for (_, rest) in iter {
                std::mem::forget(rest);
            }
            return Err(err(e));
        }
    }
    Ok(())
}

/// Trang có /Contents là MẢNG nhiều stream → gộp về 1 stream (nối theo thứ
/// tự, chèn xuống dòng giữa các stream — ngữ nghĩa PDF y hệt) ghi ra `out`.
/// Trả về false (không ghi) nếu trang chỉ có 1 stream / không đọc được.
pub(crate) fn merge_page_contents(input: &Path, page_index: u16, out: &Path) -> Result<bool, EngineError> {
    let Ok(mut doc) = lopdf::Document::load(input) else { return Ok(false) };
    if doc.is_encrypted() {
        return Ok(false);
    }
    let pages = doc.get_pages();
    let Some(&page_id) = pages.get(&(page_index as u32 + 1)) else { return Ok(false) };
    let ids = doc.get_page_contents(page_id);
    if ids.len() <= 1 {
        return Ok(false);
    }
    let mut content: Vec<u8> = Vec::new();
    for id in &ids {
        if let Ok(s) = doc.get_object(*id).and_then(lopdf::Object::as_stream) {
            let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            content.extend_from_slice(&data);
            content.push(b'\n');
        }
    }
    let mut stream = lopdf::Stream::new(lopdf::Dictionary::new(), content);
    let _ = stream.compress();
    let sid = doc.add_object(stream);
    if let Ok(dict) = doc.get_object_mut(page_id).and_then(lopdf::Object::as_dict_mut) {
        dict.set("Contents", sid);
    } else {
        return Ok(false);
    }
    doc.save(out).map_err(|e| EngineError::Pdfium(format!("gộp content stream: {e}")))?;
    Ok(true)
}

/// Ghi điểm ảnh gốc của image object ra PNG (Foxit "Save Image As").
pub(crate) fn save_image_png(obj: &PdfPageObject<'_>, output: &Path) -> Result<(u32, u32), EngineError> {
    let img = obj
        .as_image_object()
        .ok_or_else(|| EngineError::Pdfium("object không phải ảnh".into()))?
        .get_raw_image()
        .map_err(|e| EngineError::Pdfium(format!("đọc điểm ảnh: {e}")))?;
    let (w, h) = img.dimensions();
    img.save_with_format(output, image::ImageFormat::Png)
        .map_err(|e| EngineError::Pdfium(format!("ghi PNG: {e}")))?;
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(l: f32, b: f32, rr: f32, t: f32) -> Rect {
        Rect { left: l, bottom: b, right: rr, top: t }
    }

    #[test]
    fn arrange_front_back() {
        let rects = vec![r(0., 0., 10., 10.); 4];
        assert_eq!(arrange_order(4, &[1], &rects, ArrangeMode::Front), vec![0, 2, 3, 1]);
        assert_eq!(arrange_order(4, &[2], &rects, ArrangeMode::Back), vec![2, 0, 1, 3]);
        assert_eq!(arrange_order(4, &[0, 2], &rects, ArrangeMode::Front), vec![1, 3, 0, 2]);
    }

    #[test]
    fn arrange_forward_skips_non_overlapping() {
        // 0 chồng 2, không chồng 1 → lên 1 lớp phải vượt qua 2.
        let rects = vec![r(0., 0., 10., 10.), r(50., 50., 60., 60.), r(5., 5., 15., 15.)];
        assert_eq!(arrange_order(3, &[0], &rects, ArrangeMode::Forward), vec![1, 2, 0]);
        assert_eq!(arrange_order(3, &[2], &rects, ArrangeMode::Backward), vec![2, 0, 1]);
    }

    #[test]
    fn inverse_roundtrip() {
        let m = (0.0, 2.0, -3.0, 0.0, 10.0, 20.0);
        let mi = inv(m).unwrap();
        let k = mul(m, mi);
        assert!((k.0 - 1.0).abs() < 1e-5 && k.1.abs() < 1e-5 && k.4.abs() < 1e-4 && k.5.abs() < 1e-4);
    }
}
