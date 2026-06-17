//! ff-pdmodel — PD (Page Description) layer.
//!
//! Tầng tài liệu cao: Document/Page/Resources/ContentStream/TextRun/
//! ImageObject/Annotation/AcroForm... Tương ứng "PD layer" của Adobe.
//! Khung trống ở Phase 1; phát triển từ Phase 2 (annotation) trở đi.
//!
//! Xem docs/04-architecture.md.

/// Kích thước trang theo điểm (points, 1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}
