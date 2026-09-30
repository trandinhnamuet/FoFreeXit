//! ff-engine — engine xử lý PDF của FoFreeXit.
//!
//! Phase 1: bọc PDFium (qua crate `pdfium-render`, tải động `pdfium.dll`)
//! để mở tài liệu, đếm trang và render trang ra ảnh. Các module edit/io/font/
//! ocr... sẽ thêm dần ở các phase sau (xem docs/03-roadmap.md, 04-architecture.md).

pub mod annot;
pub mod convert;
pub mod create;
pub mod edit;
pub mod export;
mod cffwrap;
mod fontfix;
mod formsurgery;
mod tounicode;
pub(crate) mod fontmatch;
pub mod form;
pub mod meta;
pub mod ocr;
pub mod organize;
pub mod qpdf;
pub mod redact;
pub mod render;
pub mod sign;
pub mod text;
pub mod watermark;

pub use annot::{
    apply_annotations, count_annotations, list_annotations, AnnotInfo, AnnotKind, AnnotSpec,
};
pub use edit::{apply_edits, flatten_form_xobjects, font_data, list_objects, EditOp, ObjectInfo, ObjectKind, RichSeg};
pub use meta::{outline, page_dims, strip_metadata, OutlineItem, PageDim};
pub use organize::{
    build_document, delete_pages, extract_pages, identity_plan, merge_files, rotate_pages,
    split_by_page_count, PagePlanEntry, PageSource,
};
pub use qpdf::{
    decrypt_remove_password, encrypt_with_password, encrypt_with_password_perms, ensure_openable,
    find_qpdf, optimize_save, repair, Permissions,
};
pub use convert::{
    export_docx, export_docx_pages, export_images, export_text, find_soffice, office_to_pdf,
    pdf_to_docx_via_soffice, pdf_to_pptx_via_soffice,
};
pub use form::{
    create_form_fields, export_csv, export_fdf, fill_form_fields, flatten_form, import_fdf,
    list_form_fields, parse_fdf, FieldKind, FieldValue, FormField, NewField,
};
pub use ocr::{
    find_tesseract, ocr_add_text_layer, ocr_document, ocr_images_to_pdf, ocr_page_words, ocr_page_words_ex,
    OcrOptions, OcrReport, OcrWord,
};
pub use create::{
    blank_pdf, combine_files, find_browser, html_to_pdf, image_page_count, images_to_pdf, source_kind,
    standard_page_size, text_to_pdf, CombineOptions, ImagePdfOptions, Orientation, PageSizeMode, TextPdfOptions,
};
pub use export::{
    export_html, export_page_images, export_pptx, export_rtf, export_text_range, export_tiff, export_xlsx,
    resolve_pages, subset_to_temp, HtmlMode, PptxOptions, RasterFormat,
};
pub use redact::redact_areas;
pub use sign::{generate_self_signed_id, sign_pdf, verify_signatures, SignatureCheck};
pub use render::{bind_pdfium, page_count, page_render_mismatch, render_page_png, PageImage};
pub use text::{extract_text, page_char_boxes, search, CharBox, Rect, SearchHit};
pub use watermark::{add_header_footer, add_watermark, Anchor, HeaderFooterSpec, WatermarkSpec};

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
