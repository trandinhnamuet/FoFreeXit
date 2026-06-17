//! Metadata tài liệu: kích thước trang & outline (bookmarks).

use std::path::Path;

use pdfium_render::prelude::*;

use crate::EngineError;

/// Kích thước một trang theo điểm PDF (points, 1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDim {
    pub index: u16,
    pub width_pt: f32,
    pub height_pt: f32,
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
        dims.push(PageDim {
            index: i as u16,
            width_pt: page.width().value,
            height_pt: page.height().value,
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
