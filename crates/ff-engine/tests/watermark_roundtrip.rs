//! Round-trip test cho watermark + header/footer (Phase 3): áp rồi đọc lại
//! bằng extract_text — PDFium ghi watermark/header-footer thành CONTENT thật
//! của trang (không phải annotation) nên phải xuất hiện trong text trích xuất.

use std::path::PathBuf;

use ff_engine::{Anchor, HeaderFooterSpec, WatermarkSpec};

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

fn base_watermark(text: &str) -> WatermarkSpec {
    WatermarkSpec {
        text: text.to_string(),
        font_size: 36.0,
        color: [200, 0, 0, 120],
        bold: false,
        italic: false,
        rotation_deg: 45.0,
        anchor: Anchor::Center,
        pages: vec![],
    }
}

#[test]
fn watermark_text_appears_on_all_pages() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf"); // 3 trang
    let output = tmp("ff_wm_all.pdf");

    ff_engine::add_watermark(&pdf, &input, &base_watermark("CONFIDENTIAL"), &output, None)
        .expect("add_watermark");

    for i in 0..3u16 {
        let text = ff_engine::extract_text(&pdf, &output, i, None).expect("text");
        assert!(text.contains("CONFIDENTIAL"), "trang {i} phải có watermark: {text:?}");
    }
}

#[test]
fn watermark_respects_page_filter() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_wm_filtered.pdf");

    let mut spec = base_watermark("ONLY-PAGE-1");
    spec.pages = vec![1];
    ff_engine::add_watermark(&pdf, &input, &spec, &output, None).expect("add_watermark");

    let p0 = ff_engine::extract_text(&pdf, &output, 0, None).expect("text p0");
    let p1 = ff_engine::extract_text(&pdf, &output, 1, None).expect("text p1");
    assert!(!p0.contains("ONLY-PAGE-1"), "trang 0 không được có watermark");
    assert!(p1.contains("ONLY-PAGE-1"), "trang 1 phải có watermark");
}

#[test]
fn watermark_vietnamese_text_round_trips() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_wm_viet.pdf");

    ff_engine::add_watermark(&pdf, &input, &base_watermark("Bản nháp"), &output, None)
        .expect("add_watermark viet");

    let text = ff_engine::extract_text(&pdf, &output, 0, None).expect("text");
    assert!(text.contains("Bản nháp"), "watermark tiếng Việt phải đọc lại đúng: {text:?}");
}

#[test]
fn header_footer_inserts_page_numbers_and_total() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf"); // 3 trang
    let output = tmp("ff_hf_pagenum.pdf");

    let spec = HeaderFooterSpec {
        bottom_center: "Trang {page}/{total}".to_string(),
        top_right: "{date}".to_string(),
        font_size: 10.0,
        color: [0, 0, 0, 255],
        margin_pt: 20.0,
        date: "2026-06-17".to_string(),
        ..Default::default()
    };
    ff_engine::add_header_footer(&pdf, &input, &spec, &output, None).expect("add_header_footer");

    for i in 0..3u16 {
        let text = ff_engine::extract_text(&pdf, &output, i, None).expect("text");
        let expected = format!("Trang {}/3", i + 1);
        assert!(text.contains(&expected), "trang {i} phải có '{expected}': {text:?}");
        assert!(text.contains("2026-06-17"), "trang {i} phải có ngày: {text:?}");
    }
}

#[test]
fn header_footer_empty_slots_are_skipped() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = tmp("ff_hf_empty.pdf");

    let before = ff_engine::extract_text(&pdf, &input, 0, None).expect("text before");
    let spec = HeaderFooterSpec {
        bottom_left: "ONLY-THIS".to_string(),
        font_size: 10.0,
        color: [0, 0, 0, 255],
        margin_pt: 20.0,
        ..Default::default()
    };
    ff_engine::add_header_footer(&pdf, &input, &spec, &output, None).expect("add_header_footer");

    let after = ff_engine::extract_text(&pdf, &output, 0, None).expect("text after");
    assert!(after.contains("ONLY-THIS"));
    assert_eq!(
        after.replace("ONLY-THIS", "").trim_end(),
        before.trim_end(),
        "không thêm gì ngoài ô đã điền"
    );
}
