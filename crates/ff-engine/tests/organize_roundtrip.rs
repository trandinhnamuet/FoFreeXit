//! Round-trip test cho thao tác tổ chức trang (Phase 3): xoá/xoay/trích/
//! merge/split/crop/chèn trang trắng — đường GHI file rủi ro nhất nên test
//! kỹ bằng cách thao tác rồi đọc lại, so khớp số trang/thứ tự/nội dung.

use std::path::PathBuf;

use ff_engine::{PagePlanEntry, Rect};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn delete_pages_keeps_remaining_content_and_order() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_delete.pdf");

    // sample-multipage.pdf: 3 trang, outline Chapter 1/2/3 -> trang 0/1/2.
    ff_engine::delete_pages(&pdf, &input, &[1], &output, None).expect("delete_pages");

    assert_eq!(ff_engine::page_count(&pdf, &output, None).expect("count"), 2);
    let p0 = ff_engine::extract_text(&pdf, &output, 0, None).expect("text p0");
    let p1 = ff_engine::extract_text(&pdf, &output, 1, None).expect("text p1");
    assert!(p0.contains("1") || !p0.is_empty(), "trang 0 gốc phải còn nguyên: {p0:?}");
    assert!(!p1.is_empty(), "trang 2 gốc (giờ là trang 1) phải còn nguyên: {p1:?}");
    // Trang giữa (index 1 gốc) không còn xuất hiện ở bất kỳ đâu trong output.
    assert_ne!(p0, p1);
}

#[test]
fn delete_all_pages_is_rejected() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_delete_all.pdf");
    let err = ff_engine::delete_pages(&pdf, &input, &[0, 1, 2], &output, None);
    assert!(err.is_err(), "không được cho xoá hết toàn bộ trang");
}

#[test]
fn rotate_pages_persists_in_saved_file() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_rotate.pdf");

    ff_engine::rotate_pages(&pdf, &input, &[0], 90, &output, None).expect("rotate_pages");

    let doc = pdf.load_pdf_from_file(&output, None).expect("mở lại");
    let page0 = doc.pages().get(0).expect("trang 0");
    let page1 = doc.pages().get(1).expect("trang 1");
    assert_eq!(
        page0.rotation().expect("rotation p0"),
        pdfium_render::prelude::PdfPageRenderRotation::Degrees90,
        "trang 0 phải xoay 90 độ"
    );
    assert_eq!(
        page1.rotation().expect("rotation p1"),
        pdfium_render::prelude::PdfPageRenderRotation::None,
        "trang 1 không bị xoay"
    );
}

#[test]
fn extract_pages_creates_new_file_without_touching_input() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_extract.pdf");

    ff_engine::extract_pages(&pdf, &input, &[2, 0], &output, None).expect("extract_pages");

    assert_eq!(ff_engine::page_count(&pdf, &output, None).expect("count"), 2);
    assert_eq!(ff_engine::page_count(&pdf, &input, None).expect("count input"), 3, "input không bị sửa");

    // Thứ tự đúng theo `pages` truyền vào: [2, 0] -> output trang 0 = input trang 2.
    let out_p0 = ff_engine::extract_text(&pdf, &output, 0, None).expect("text");
    let in_p2 = ff_engine::extract_text(&pdf, &input, 2, None).expect("text");
    assert_eq!(out_p0, in_p2, "trích đúng thứ tự đã chọn");
}

#[test]
fn merge_files_concatenates_in_order() {
    let pdf = pdfium();
    let a = workspace_root().join("corpus").join("hello.pdf");
    let b = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_merge.pdf");

    ff_engine::merge_files(&pdf, &[a.clone(), b.clone()], &output).expect("merge_files");

    let count_a = ff_engine::page_count(&pdf, &a, None).expect("count a");
    let count_b = ff_engine::page_count(&pdf, &b, None).expect("count b");
    assert_eq!(
        ff_engine::page_count(&pdf, &output, None).expect("count out"),
        count_a + count_b
    );

    let merged_first = ff_engine::extract_text(&pdf, &output, 0, None).expect("text");
    let a_first = ff_engine::extract_text(&pdf, &a, 0, None).expect("text");
    assert_eq!(merged_first, a_first, "trang đầu output phải khớp file a");
}

#[test]
fn split_by_page_count_produces_correct_chunks() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf"); // 3 trang
    let out_dir = std::env::temp_dir();

    let parts = ff_engine::split_by_page_count(&pdf, &input, 2, &out_dir, "ff_org_split", None)
        .expect("split_by_page_count");

    assert_eq!(parts.len(), 2, "3 trang / 2 mỗi file = 2 phần (2+1)");
    assert_eq!(ff_engine::page_count(&pdf, &parts[0], None).expect("count"), 2);
    assert_eq!(ff_engine::page_count(&pdf, &parts[1], None).expect("count"), 1);

    for p in &parts {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn crop_sets_cropbox_on_saved_file() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_crop.pdf");

    let mut plan = ff_engine::identity_plan(&pdf, &input, None).expect("identity_plan");
    plan[0].crop = Some(Rect { left: 50.0, bottom: 60.0, right: 500.0, top: 700.0 });
    ff_engine::build_document(&pdf, &input, &plan, &output, None).expect("build_document crop");

    let doc = pdf.load_pdf_from_file(&output, None).expect("mở lại");
    let page0 = doc.pages().get(0).expect("trang 0");
    let crop = page0.boundaries().crop().expect("crop box");
    assert!((crop.bounds.left().value - 50.0).abs() < 0.5);
    assert!((crop.bounds.bottom().value - 60.0).abs() < 0.5);
    assert!((crop.bounds.right().value - 500.0).abs() < 0.5);
    assert!((crop.bounds.top().value - 700.0).abs() < 0.5);
}

#[test]
fn insert_blank_page_has_correct_size_and_no_text() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_org_blank.pdf");

    let mut plan = ff_engine::identity_plan(&pdf, &input, None).expect("identity_plan");
    plan.insert(1, PagePlanEntry::blank(300.0, 400.0));
    ff_engine::build_document(&pdf, &input, &plan, &output, None).expect("build_document blank");

    assert_eq!(ff_engine::page_count(&pdf, &output, None).expect("count"), 4);
    let doc = pdf.load_pdf_from_file(&output, None).expect("mở lại");
    let blank = doc.pages().get(1).expect("trang trắng");
    assert!((blank.width().value - 300.0).abs() < 0.5);
    assert!((blank.height().value - 400.0).abs() < 0.5);
    drop(doc);
    let text = ff_engine::extract_text(&pdf, &output, 1, None).expect("text trang trắng");
    assert!(text.trim().is_empty(), "trang trắng không được có text: {text:?}");
}
