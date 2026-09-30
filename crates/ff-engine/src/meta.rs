//! Metadata tài liệu: kích thước trang & outline (bookmarks) + xoá metadata
//! nhận dạng (Phase 5).

use std::path::Path;

use pdfium_render::prelude::*;

use crate::EngineError;

/// Xoá metadata nhận dạng khỏi file (Phase 5 — riêng tư): /Info trong trailer
/// (Author/Producer/CreationDate…) và stream XMP `/Metadata` trong catalog —
/// xoá cả THAM CHIẾU lẫn OBJECT đích để nội dung không còn nằm trong file.
/// Thao tác tầng PDF object qua lopdf; file mã hoá cần gỡ mật khẩu trước.
pub fn strip_metadata(input: &Path, output: &Path) -> Result<(), EngineError> {
    let mut doc = lopdf::Document::load(input)
        .map_err(|e| EngineError::Pdfium(format!("lopdf load: {e}")))?;

    // /Info: gỡ tham chiếu trong trailer + xoá object đích.
    if let Ok(id) = doc.trailer.get(b"Info").and_then(|o| o.as_reference()) {
        doc.objects.remove(&id);
    }
    doc.trailer.remove(b"Info");

    // /Metadata (XMP) trong catalog: gỡ tham chiếu + xoá stream đích.
    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference()) {
        let meta_id = doc
            .get_object(root_id)
            .ok()
            .and_then(|c| c.as_dict().ok())
            .and_then(|d| d.get(b"Metadata").ok().and_then(|o| o.as_reference().ok()));
        if let Ok(lopdf::Object::Dictionary(dict)) = doc.get_object_mut(root_id) {
            dict.remove(b"Metadata");
        }
        if let Some(id) = meta_id {
            doc.objects.remove(&id);
        }
    }

    doc.save(output)
        .map_err(|e| EngineError::Pdfium(format!("lopdf save: {e}")))?;
    Ok(())
}

/// Kích thước một trang theo điểm PDF (points, 1/72 inch).
///
/// Quy ước toạ độ:
/// - `width_pt`/`height_pt`: kích thước KHI HIỂN THỊ (đã áp /Rotate — trang
///   xoay 90/270 thì hai giá trị đổi chỗ so với hộp trang) = khung ảnh render.
/// - `box_*`: hộp trang hiển thị (CropBox ∩ MediaBox, hoặc MediaBox nếu không
///   có CropBox) trong không gian người dùng PDF CHƯA xoay — đúng không gian
///   của char box (`page_char_boxes`), rect tìm kiếm, annotation, redaction.
///   Gốc có thể khác (0,0) (trang đã crop) và có thể âm.
/// - `rotation`: /Rotate hiệu lực, chuẩn hoá về 0/90/180/270 (theo chiều kim đồng hồ).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDim {
    pub index: u16,
    pub width_pt: f32,
    pub height_pt: f32,
    pub rotation: u16,
    pub box_left: f32,
    pub box_bottom: f32,
    pub box_width: f32,
    pub box_height: f32,
}

/// Hộp trang hiển thị như PDFium tính (`CPDF_Page::UpdateDimensions`):
/// CropBox giao MediaBox; thiếu CropBox (hoặc giao rỗng) thì dùng MediaBox.
fn visible_box(page: &PdfPage) -> Option<(f32, f32, f32, f32)> {
    let media = page.boundaries().media().ok().map(|b| b.bounds);
    let crop = page.boundaries().crop().ok().map(|b| b.bounds);
    let as_tuple = |r: PdfRect| (r.left().value, r.bottom().value, r.right().value, r.top().value);
    match (media.map(as_tuple), crop.map(as_tuple)) {
        (Some(m), Some(c)) => {
            let (l, b, r, t) = (m.0.max(c.0), m.1.max(c.1), m.2.min(c.2), m.3.min(c.3));
            if r > l && t > b {
                Some((l, b, r, t))
            } else {
                Some(m)
            }
        }
        (Some(m), None) => Some(m),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    }
}

/// Một mục trong outline (bookmark).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    /// Trang đích (0-based) nếu xác định được.
    pub page_index: Option<u16>,
    /// Độ sâu trong cây outline (0 = cấp cao nhất).
    pub level: u32,
}

/// Lấy kích thước mọi trang.
pub fn page_dims(
    pdfium: &Pdfium,
    input: &Path,
    password: Option<&str>,
) -> Result<Vec<PageDim>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let mut dims = Vec::new();
    for (i, page) in document.pages().iter().enumerate() {
        let width_pt = page.width().value;
        let height_pt = page.height().value;
        let rotation: u16 = match page.rotation() {
            Ok(PdfPageRenderRotation::Degrees90) => 90,
            Ok(PdfPageRenderRotation::Degrees180) => 180,
            Ok(PdfPageRenderRotation::Degrees270) => 270,
            _ => 0,
        };
        // Dự phòng khi không đọc được hộp: gốc (0,0), kích thước chưa xoay.
        let (box_left, box_bottom, box_right, box_top) = visible_box(&page).unwrap_or_else(|| {
            if rotation % 180 == 90 {
                (0.0, 0.0, height_pt, width_pt)
            } else {
                (0.0, 0.0, width_pt, height_pt)
            }
        });
        dims.push(PageDim {
            index: i as u16,
            width_pt,
            height_pt,
            rotation,
            box_left,
            box_bottom,
            box_width: box_right - box_left,
            box_height: box_top - box_bottom,
        });
    }
    Ok(dims)
}

/// Lấy outline (bookmarks) dạng phẳng theo thứ tự duyệt sâu (DFS).
pub fn outline(
    pdfium: &Pdfium,
    input: &Path,
    password: Option<&str>,
) -> Result<Vec<OutlineItem>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let mut items = Vec::new();
    for bookmark in document.bookmarks().iter() {
        let title = bookmark.title().unwrap_or_default();
        let page_index = bookmark
            .destination()
            .and_then(|dest| dest.page_index().ok())
            .map(|idx| idx as u16);
        items.push(OutlineItem {
            title,
            page_index,
            level: 0,
        });
    }
    Ok(items)
}
