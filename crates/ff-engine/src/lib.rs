//! ff-engine — engine xử lý PDF của FoFreeXit.
//!
//! Phase 1: bọc PDFium (qua crate `pdfium-render`, tải động `pdfium.dll`)
//! để mở tài liệu, đếm trang và render trang ra ảnh. Các module edit/io/font/
//! ocr... sẽ thêm dần ở các phase sau (xem docs/03-roadmap.md, 04-architecture.md).

pub mod annot;
pub mod bookmarks;
pub mod annot_ext;
pub mod compare;
pub mod compare_report;
pub mod convert;
pub mod edit;
pub mod editobj;
mod cffwrap;
mod fontfix;
mod formsurgery;
mod pagemark_cos;
mod tounicode;
pub(crate) mod fontmatch;
pub mod form;
pub mod links;
pub mod meta;
pub mod ocr;
pub mod organize;
pub mod qpdf;
pub mod redact;
pub mod redact_search;
pub mod sanitize;
pub mod attachments;
pub mod docprops;
pub mod a11y;
pub(crate) mod pdfobj;
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
pub use annot_ext::{
    apply_shape_annotations, delete_annotations, list_annotations_detailed, render_page_hiding,
    save_annotations, update_annotations, AnnotDetail, AnnotMeta, AnnotRef, AnnotSaveRequest,
    AnnotUpdate, ReplySpec, ShapeKind, ShapeSpec, StampSpec,
};
pub use compare::{
    compare_documents, diff_regions, Change, ChangeKind, CompareMode, CompareOptions, CompareProgress,
    CompareResult, CompareSummary, PagePair, RawImage, COMPARE_CANCELLED,
};
pub use compare_report::{export_compare_report, ReportInfo, ReportLabels, ReportStats};
pub use edit::{apply_edits, extract_image, flatten_form_xobjects, font_data, list_objects, EditOp, ObjectInfo, ObjectKind, RichSeg};
// ShapeKind/ShapeSpec của editobj (hình vẽ trong nội dung trang) trùng tên với
// annot_ext (chú thích) → re-export với tiền tố Edit.
pub use editobj::{ArrangeMode, PathStyle, ShapeKind as EditShapeKind, ShapeSpec as EditShapeSpec};
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
pub use redact::{redact_areas, redact_areas_styled, RedactAlign, RedactStyle};
pub use redact_search::{iban_valid, luhn_valid, search_redact, RedactHit, RedactPattern, RedactSearchSpec};
pub use sanitize::{examine_document, sanitize_document, FormAction, SanitizeOptions, SanitizeReport};
pub use attachments::{
    apply_attachment_ops, extract_attachment, is_risky_file_name, list_attachments, Attachment, AttachmentOp,
};
pub use docprops::{
    read_properties, write_properties, DocProperties, DocPropsUpdate, FontInfo, InitialView, SecurityInfo,
};
pub use a11y::{
    check_accessibility, fix_accessibility, list_figures, set_alt_texts, A11yCheck, A11yFixes, CheckStatus, FigureInfo,
};
pub use sign::{generate_self_signed_id, sign_pdf, verify_signatures, SignatureCheck};
pub use render::{bind_pdfium, page_count, page_render_mismatch, render_page_png, PageImage};
pub use text::{extract_text, page_char_boxes, search, CharBox, Rect, SearchHit};
pub use watermark::{
    add_background, add_bates, add_header_footer, add_watermark, preview_page_mark, remove_page_marks,
    scan_page_marks, Anchor, BatesFormat, BatesRange, FontChoice, HeaderFooterSpec, MarkCounts, MarkKind,
    PageMarkJob, PageSubset, StampSource, TextStyle, WatermarkSpec,
};

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
