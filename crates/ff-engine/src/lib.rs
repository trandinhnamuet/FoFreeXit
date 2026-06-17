//! ff-engine — engine xử lý PDF của FoFreeXit.
//!
//! Phase 1: bọc PDFium (qua crate `pdfium-render`, tải động `pdfium.dll`)
//! để mở tài liệu, đếm trang và render trang ra ảnh. Các module edit/io/font/
//! ocr... sẽ thêm dần ở các phase sau (xem docs/03-roadmap.md, 04-architecture.md).

pub mod annot;
pub mod meta;
pub mod render;
pub mod text;

pub use annot::{
    apply_annotations, count_annotations, list_annotations, AnnotInfo, AnnotKind, AnnotSpec,
};
pub use meta::{outline, page_dims, OutlineItem, PageDim};
pub use render::{bind_pdfium, page_count, render_page_png, PageImage};
pub use text::{extract_text, page_char_boxes, search, CharBox, Rect, SearchHit};

/// Lỗi cấp engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("không tìm thấy thư viện PDFium. Đặt biến môi trường FOFREEXIT_PDFIUM_PATH trỏ tới thư mục chứa pdfium.dll, hoặc chạy `scripts/fetch-pdfium`. Chi tiết: {0}")]
    PdfiumNotFound(String),

    #[error("lỗi PDFium: {0}")]
    Pdfium(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
