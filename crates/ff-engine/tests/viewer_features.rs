//! Test các năng lực viewer ở tầng engine: kích thước trang, trích text,
//! tìm kiếm, outline. Dựa trên fixture corpus/sample-multipage.pdf có nội dung
//! biết trước:
//!   trang 0: "FoFreeXit Test Document" / "Page one content alpha"
//!   trang 1: "Page two content beta searchterm"
//!   trang 2: "Page three content gamma"
//!   outline: Chapter 1/2/3 -> trang 0/1/2

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture() -> PathBuf {
    workspace_root().join("corpus").join("sample-multipage.pdf")
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

#[test]
fn page_dims_letter_3pages() {
    let dims = ff_engine::page_dims(&pdfium(), &fixture(), None).expect("page_dims");
    assert_eq!(dims.len(), 3, "phải có 3 trang");
    for d in &dims {
        assert!((d.width_pt - 612.0).abs() < 0.5, "rộng sai: {}", d.width_pt);
        assert!((d.height_pt - 792.0).abs() < 0.5, "cao sai: {}", d.height_pt);
    }
}

#[test]
fn extract_text_per_page() {
    let pdf = pdfium();
    let p0 = ff_engine::extract_text(&pdf, &fixture(), 0, None).expect("text trang 0");
    assert!(p0.contains("FoFreeXit Test Document"), "thiếu tiêu đề: {p0:?}");
    assert!(p0.contains("alpha"), "thiếu 'alpha': {p0:?}");

    let p1 = ff_engine::extract_text(&pdf, &fixture(), 1, None).expect("text trang 1");
    assert!(p1.contains("searchterm"), "thiếu 'searchterm': {p1:?}");
}

#[test]
fn search_counts_and_locations() {
    let pdf = pdfium();

    // "content" xuất hiện 1 lần/trang -> 3 kết quả, ở trang 0,1,2.
    let hits = ff_engine::search(&pdf, &fixture(), "content", false, None).expect("search");
    assert_eq!(hits.len(), 3, "'content' phải có 3 kết quả: {hits:?}");
    let pages: Vec<u16> = hits.iter().map(|h| h.page_index).collect();
    assert_eq!(pages, vec![0, 1, 2], "vị trí trang sai: {pages:?}");

    // Ít nhất một kết quả có khung bao (rect) hợp lệ.
    assert!(
        hits.iter().any(|h| h.rect.is_some()),
        "không kết quả nào có rect"
    );

    // "searchterm" chỉ ở trang 1, đúng 1 kết quả.
    let st = ff_engine::search(&pdf, &fixture(), "searchterm", false, None).expect("search");
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].page_index, 1);
}

#[test]
fn search_case_sensitivity() {
    let pdf = pdfium();
    // Không phân biệt hoa thường: khớp.
    let ci = ff_engine::search(&pdf, &fixture(), "SEARCHTERM", false, None).expect("search ci");
    assert_eq!(ci.len(), 1, "case-insensitive phải khớp");
    // Phân biệt hoa thường: không khớp (text gốc là chữ thường).
    let cs = ff_engine::search(&pdf, &fixture(), "SEARCHTERM", true, None).expect("search cs");
    assert_eq!(cs.len(), 0, "case-sensitive không được khớp");
}

#[test]
fn char_boxes_match_text_and_bounds() {
    let boxes = ff_engine::page_char_boxes(&pdfium(), &fixture(), 0, None).expect("char boxes");
    assert!(!boxes.is_empty(), "phải có ký tự");

    // Ghép ký tự theo thứ tự đọc -> chứa đúng nội dung trang.
    let concat: String = boxes.iter().map(|b| b.ch.as_str()).collect();
    assert!(concat.contains("FoFreeXit"), "thiếu 'FoFreeXit': {concat:?}");
    assert!(concat.contains("Document"), "thiếu 'Document'");
    assert!(concat.contains("content"), "thiếu 'content'");
    assert!(concat.contains("alpha"), "thiếu 'alpha'");

    // Mọi hộp nằm trong khổ trang Letter (612x792), cho dung sai nhỏ.
    for b in &boxes {
        assert!(b.left >= -1.0 && b.right <= 613.0, "x ngoài trang: {b:?}");
        assert!(b.bottom >= -1.0 && b.top <= 793.0, "y ngoài trang: {b:?}");
    }
}

#[test]
fn outline_three_chapters() {
    let items = ff_engine::outline(&pdfium(), &fixture(), None).expect("outline");
    assert_eq!(items.len(), 3, "phải có 3 mục outline: {items:?}");
    assert_eq!(items[0].title, "Chapter 1");
    assert_eq!(items[1].title, "Chapter 2");
    assert_eq!(items[2].title, "Chapter 3");
    assert_eq!(items[0].page_index, Some(0));
    assert_eq!(items[1].page_index, Some(1));
    assert_eq!(items[2].page_index, Some(2));
}
