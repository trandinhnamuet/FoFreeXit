//! ff-engine — engine xử lý PDF của FoFreeXit.
//!
//! Phase 1: bọc PDFium (qua crate `pdfium-render`, tải động `pdfium.dll`)
//! để mở tài liệu, đếm trang và render trang ra ảnh. Các module edit/io/font/
//! ocr... sẽ thêm dần ở các phase sau (xem docs/03-roadmap.md, 04-architecture.md).

pub mod annot;
pub mod bookmarks;
pub mod convert;
pub mod edit;
mod cffwrap;
mod fontfix;
mod formsurgery;
mod tounicode;
pub(crate) mod fontmatch;
pub mod form;
pub mod links;
pub mod meta;
pub mod ocr;
pub mod organize;
pub mod qpdf;
pub mod redact;
pub mod render;
pub mod sign;
pub mod split;
pub mod text;
pub mod watermark;

pub use annot::{
    apply_annotations, count_annotations, list_annotations, AnnotInfo, AnnotKind, AnnotSpec,
};
pub use bookmarks::{auto_bookmarks_from_headings, get_outline, set_outline, Bookmark};
pub use links::{edit_links, list_links, save_outline_and_links, LinkInfo, NewLink};
pub use split::{
    parse_page_ranges, plan_split_by_size, sanitize_file_name, split_by_bookmarks, split_by_ranges, split_by_size,
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
    export_docx, export_images, export_text, find_soffice, office_to_pdf, pdf_to_docx_via_soffice,
};
pub use form::{
    create_form_fields, export_csv, export_fdf, fill_form_fields, flatten_form, import_fdf,
    list_form_fields, parse_fdf, FieldKind, FieldValue, FormField, NewField,
};
pub use ocr::{find_tesseract, ocr_add_text_layer, ocr_page_words, OcrWord};
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
