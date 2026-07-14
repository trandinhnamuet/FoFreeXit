//! Round-trip test cho chỉnh sửa nội dung (Phase 4 — moat chính): liệt kê
//! object, sửa text (gồm tiếng Việt), xoá, di chuyển, thêm chữ/ảnh. Đường GHI
//! file rủi ro nhất nên thao tác rồi đọc lại để so khớp.
//!
//! Lưu ý bẫy mutex PDFium (đã ghi memory): mọi assert chạy SAU khi ff_engine
//! đã trả về (doc đã drop), nên panic assert không poison mutex toàn cục.

use std::path::PathBuf;

use ff_engine::{EditOp, ObjectKind};

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

fn sample() -> PathBuf {
    workspace_root().join("corpus").join("sample-multipage.pdf")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Index của text object đầu tiên trên trang 0 có nội dung chứa `needle`.
fn find_text_index(pdf: &pdfium_render::prelude::Pdfium, input: &std::path::Path, needle: &str) -> u16 {
    let objs = ff_engine::list_objects(pdf, input, 0, None).expect("list_objects");
    objs.into_iter()
        .find(|o| o.kind == ObjectKind::Text && o.text.as_deref().map(|t| t.contains(needle)).unwrap_or(false))
        .unwrap_or_else(|| panic!("không tìm thấy text object chứa {needle:?}"))
        .index
}

#[test]
fn list_objects_finds_page_text() {
    let pdf = pdfium();
    let objs = ff_engine::list_objects(&pdf, &sample(), 0, None).expect("list_objects");
    assert!(!objs.is_empty(), "trang 0 phải có object");
    let has_text = objs
        .iter()
        .any(|o| o.kind == ObjectKind::Text && o.text.as_deref().map(|t| t.contains("Page one") || t.contains("FoFreeXit")).unwrap_or(false));
    assert!(has_text, "phải thấy text object của trang 1: {objs:?}");
}

#[test]
fn set_text_replaces_run_content() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_settext.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Edited line ABC".into(),
            font_size: None,
            color: None,
            bold: false,
            italic: false,
        }],
        &out,
        None,
    )
    .expect("apply_edits SetText");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Edited line ABC"), "phải có text mới: {text:?}");
    assert!(!text.contains("Page one content alpha"), "text cũ phải biến mất: {text:?}");
}

#[test]
fn set_text_vietnamese_round_trips() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_vi.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");
    let vi = "Sửa: nội dung Tiếng Việt";

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: vi.into(),
            font_size: None,
            color: None,
            bold: false,
            italic: false,
        }],
        &out,
        None,
    )
    .expect("apply_edits SetText VI");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Tiếng Việt"), "tiếng Việt phải đúng dấu: {text:?}");
}

#[test]
fn delete_object_reduces_count() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_delete.pdf");
    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list").len();
    assert!(before >= 1);

    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Delete { index: 0 }], &out, None)
        .expect("apply_edits Delete");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out").len();
    assert_eq!(after, before - 1, "xoá 1 object phải giảm đúng 1");
}

#[test]
fn transform_translate_moves_object() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_move.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");
    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list");
    let orig_left = before.iter().find(|o| o.index == idx).expect("obj").rect.left;

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::Transform { index: idx, dx: 50.0, dy: 0.0, sx: 1.0, sy: 1.0 }],
        &out,
        None,
    )
    .expect("apply_edits Transform");

    // Object đã dịch giữ nguyên thứ tự index (không xoá/thêm) → so cùng index.
    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let new_left = after.iter().find(|o| o.index == idx).expect("obj out").rect.left;
    assert!((new_left - (orig_left + 50.0)).abs() < 2.0, "left phải dịch ~+50: {orig_left} -> {new_left}");
}

#[test]
fn add_text_appears_in_extract() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_addtext.pdf");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::AddText {
            x: 100.0,
            y: 100.0,
            text: "ADDEDXYZ".into(),
            font_size: 14.0,
            color: [10, 20, 30, 255],
            bold: false,
            italic: false,
        }],
        &out,
        None,
    )
    .expect("apply_edits AddText");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("ADDEDXYZ"), "text thêm mới phải xuất hiện: {text:?}");
}

#[test]
fn add_image_adds_image_object() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_addimg.pdf");

    // Tạo 1 PNG đỏ 24x24 làm fixture.
    let png = tmp("ff_edit_fixture.png");
    let mut img = image::RgbImage::new(24, 24);
    for p in img.pixels_mut() {
        *p = image::Rgb([220, 30, 30]);
    }
    img.save(&png).expect("save png");

    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list").len();

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::AddImage {
            x: 50.0,
            y: 50.0,
            width_pt: 80.0,
            height_pt: 60.0,
            image_path: png.to_string_lossy().into_owned(),
        }],
        &out,
        None,
    )
    .expect("apply_edits AddImage");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    assert_eq!(after.len(), before + 1, "thêm ảnh phải tăng 1 object");
    assert!(after.iter().any(|o| o.kind == ObjectKind::Image), "phải có object kind Image");
}
