//! ff-cos — COS (Carousel Object System) layer.
//!
//! Tầng object thấp của PDF: Object/Dictionary/Array/Stream/Name/Ref/XRef.
//! Tương ứng "Cos layer" của Adobe và QPDF. Hiện là khung trống; sẽ được
//! xây dựng dần từ Phase 3 (ghi file vững) trở đi. Phase 1 chưa cần.
//!
//! Xem docs/04-architecture.md.

/// Phiên bản đặc tả PDF tối thiểu mà engine nhắm tới.
pub const TARGET_PDF_VERSION: &str = "1.7";
