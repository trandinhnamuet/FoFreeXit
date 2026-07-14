//! Watermark + Header/Footer + đánh số trang (Phase 3).
//!
//! Dùng API gốc của PDFium (`PdfPages::watermark`) để thêm page object vào
//! mọi trang — không tự dựng content stream bằng tay như đã làm cho FreeText
//! (annot.rs): PDFium tự lo việc mã hoá CID khi font nạp bằng
//! `load_true_type_from_bytes(..., is_cid_font: true)`, nên tiếng Việt hiển
//! thị đúng mà không cần build Type0 dict thủ công.

use std::path::Path;

use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::EngineError;

/// Vị trí neo theo lưới 9 điểm, đúng kiểu dialog Watermark của Foxit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft, TopCenter, TopRight,
    MiddleLeft, Center, MiddleRight,
    BottomLeft, BottomCenter, BottomRight,
}

/// Watermark văn bản áp cho 1+ trang.
#[derive(Clone, Debug)]
pub struct WatermarkSpec {
    pub text: String,
    pub font_size: f32,
    /// RGBA — alpha (0–255) chính là độ mờ (255 = đậm hẳn, ví dụ Foxit mặc định ~watermark mờ dùng alpha thấp).
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    /// Độ xoay (độ), dương = ngược chiều kim đồng hồ — khớp quy ước PDFium `rotate_counter_clockwise_degrees`.
    pub rotation_deg: f32,
    pub anchor: Anchor,
    /// 0-based; rỗng = mọi trang.
    pub pages: Vec<u16>,
}

/// Header/Footer — 6 ô text (trái/giữa/phải × trên/dưới), hỗ trợ token
/// `{page}` (số trang 1-based), `{total}` (tổng số trang). Token `{date}` do
/// caller tự thay trước (truyền sẵn trong field `date`, rỗng = bỏ token đó).
#[derive(Clone, Debug, Default)]
pub struct HeaderFooterSpec {
    pub top_left: String,
    pub top_center: String,
    pub top_right: String,
    pub bottom_left: String,
    pub bottom_center: String,
    pub bottom_right: String,
    pub font_size: f32,
    pub color: [u8; 4],
    pub margin_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub date: String,
    /// 0-based; rỗng = mọi trang.
    pub pages: Vec<u16>,
}

fn substitute(template: &str, page_1based: u16, total: u16, date: &str) -> String {
    template
        .replace("{page}", &page_1based.to_string())
        .replace("{total}", &total.to_string())
        .replace("{date}", date)
}

fn applies_to(pages: &[u16], index: u16) -> bool {
    pages.is_empty() || pages.contains(&index)
}

fn anchor_xy(anchor: Anchor, page_w: f32, page_h: f32, text_w: f32, text_h: f32, margin: f32) -> (f32, f32) {
    let (left, center_x, right) = (margin, (page_w - text_w) / 2.0, page_w - text_w - margin);
    let (bottom, center_y, top) = (margin, (page_h - text_h) / 2.0, page_h - text_h - margin);
    match anchor {
        Anchor::TopLeft => (left, top),
        Anchor::TopCenter => (center_x, top),
        Anchor::TopRight => (right, top),
        Anchor::MiddleLeft => (left, center_y),
        Anchor::Center => (center_x, center_y),
        Anchor::MiddleRight => (right, center_y),
        Anchor::BottomLeft => (left, bottom),
        Anchor::BottomCenter => (center_x, bottom),
        Anchor::BottomRight => (right, bottom),
    }
}

/// Thêm watermark văn bản vào `input`, ghi ra `output`.
pub fn add_watermark(
    pdfium: &Pdfium,
    input: &Path,
    spec: &WatermarkSpec,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let font_bytes = find_font_bytes(spec.bold, spec.italic)
        .ok_or_else(|| EngineError::Pdfium("không tìm được font hệ thống cho watermark".into()))?;
    let token = document
        .fonts_mut()
        .load_true_type_from_bytes(&font_bytes, true)
        .map_err(|e| EngineError::Pdfium(format!("nạp font watermark: {e}")))?;

    let err = |e: PdfiumError| EngineError::Pdfium(format!("watermark: {e}"));

    document
        .pages()
        .watermark(|group, index, width, height| {
            if !applies_to(&spec.pages, index) {
                return Ok(());
            }
            let mut obj = PdfPageTextObject::new(
                &document,
                spec.text.clone(),
                token,
                PdfPoints::new(spec.font_size),
            )?;
            obj.set_fill_color(PdfColor::new(
                spec.color[0], spec.color[1], spec.color[2], spec.color[3],
            ))?;
            if spec.rotation_deg != 0.0 {
                obj.rotate_counter_clockwise_degrees(spec.rotation_deg)?;
            }
            let (tw, th) = (obj.width()?.value, obj.height()?.value);
            let (x, y) = anchor_xy(spec.anchor, width.value, height.value, tw, th, 0.0);
            obj.translate(PdfPoints::new(x), PdfPoints::new(y))?;
            group.push(&mut obj.into())
        })
        .map_err(err)?;

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(())
}

/// Thêm header/footer (gồm cả đánh số trang qua token `{page}`/`{total}`)
/// vào `input`, ghi ra `output`.
pub fn add_header_footer(
    pdfium: &Pdfium,
    input: &Path,
    spec: &HeaderFooterSpec,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let font_bytes = find_font_bytes(spec.bold, spec.italic)
        .ok_or_else(|| EngineError::Pdfium("không tìm được font hệ thống cho header/footer".into()))?;
    let token = document
        .fonts_mut()
        .load_true_type_from_bytes(&font_bytes, true)
        .map_err(|e| EngineError::Pdfium(format!("nạp font header/footer: {e}")))?;

    let total = document.pages().len();
    let err = |e: PdfiumError| EngineError::Pdfium(format!("header/footer: {e}"));

    let slots: [(&str, Anchor); 6] = [
        (spec.top_left.as_str(), Anchor::TopLeft),
        (spec.top_center.as_str(), Anchor::TopCenter),
        (spec.top_right.as_str(), Anchor::TopRight),
        (spec.bottom_left.as_str(), Anchor::BottomLeft),
        (spec.bottom_center.as_str(), Anchor::BottomCenter),
        (spec.bottom_right.as_str(), Anchor::BottomRight),
    ];

    document
        .pages()
        .watermark(|group, index, width, height| {
            if !applies_to(&spec.pages, index) {
                return Ok(());
            }
            for (template, anchor) in slots {
                if template.is_empty() {
                    continue;
                }
                let text = substitute(template, index + 1, total, &spec.date);
                let mut obj = PdfPageTextObject::new(
                    &document,
                    text,
                    token,
                    PdfPoints::new(spec.font_size),
                )?;
                obj.set_fill_color(PdfColor::new(
                    spec.color[0], spec.color[1], spec.color[2], spec.color[3],
                ))?;
                let (tw, th) = (obj.width()?.value, obj.height()?.value);
                let (x, y) = anchor_xy(anchor, width.value, height.value, tw, th, spec.margin_pt);
                obj.translate(PdfPoints::new(x), PdfPoints::new(y))?;
                group.push(&mut obj.into())?;
            }
            Ok(())
        })
        .map_err(err)?;

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(())
}
