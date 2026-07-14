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
            font_family: None,
            bold: None,
            italic: None,
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
            font_family: None,
            bold: None,
            italic: None,
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
            font_family: None,
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

/// CHUẨN FOXIT (iteration 2): sửa text ASCII trên font base-14 (Helvetica,
/// không nhúng) phải GIỮ NGUYÊN BaseFont — không đổi font. Text mới cố ý dùng
/// ký tự ngoài text cũ để không "ăn may" qua luật charset-subset.
#[test]
fn set_text_keeps_original_font() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_keepfont.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");
    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list");
    let font_before = before
        .iter()
        .find(|o| o.index == idx)
        .and_then(|o| o.font_name.clone())
        .expect("font gốc");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Fixed by editor 2026 JQXZ!".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &out,
        None,
    )
    .expect("apply_edits SetText");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let edited = after
        .iter()
        .find(|o| o.text.as_deref().map(|t| t.contains("JQXZ")).unwrap_or(false))
        .expect("run đã sửa");
    assert_eq!(
        edited.font_name.as_deref(),
        Some(font_before.as_str()),
        "sửa text ASCII phải GIỮ NGUYÊN font (chuẩn Foxit)"
    );
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Fixed by editor"), "text mới phải có mặt: {text:?}");
}

/// Tiếng Việt trên font base-14 (KHÔNG có glyph Việt ở bất kỳ đâu để giữ):
/// phải thay bằng font hệ thống CÙNG HỌ metric-compatible (Helvetica→Arial/
/// LiberationSans), tuyệt đối không rơi bừa về font generic.
#[test]
fn vietnamese_on_base14_uses_matched_family() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_vnmatch.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Thay thế hoàn chỉnh".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &out,
        None,
    )
    .expect("apply_edits VI");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let edited = after
        .iter()
        .find(|o| o.text.as_deref().map(|t| t.contains("hoàn chỉnh")).unwrap_or(false))
        .expect("run đã sửa");
    let font = edited.font_name.clone().unwrap_or_default();
    #[cfg(not(any(windows, target_os = "macos")))]
    assert!(
        font.contains("Liberation"),
        "Helvetica+VI phải match LiberationSans (metric-compatible), được {font:?}"
    );
    #[cfg(windows)]
    assert!(font.to_lowercase().contains("arial"), "Helvetica+VI phải match Arial, được {font:?}");
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("hoàn chỉnh"), "tiếng Việt đúng dấu: {text:?}");
}

/// Case quan trọng nhất với tài liệu Việt thực tế: font NHÚNG đầy đủ glyph →
/// sửa sang nội dung Việt hoàn toàn khác phải GIỮ NGUYÊN font nhúng (sửa tại
/// chỗ, không tạo lại bằng font khác).
#[test]
fn set_text_preserves_embedded_font_vietnamese() {
    let pdf = pdfium();
    let input = sample();
    let step1 = tmp("ff_edit_emb1.pdf");
    let step2 = tmp("ff_edit_emb2.pdf");

    // B1: tạo run với font nhúng FULL (AddText nhúng nguyên font hệ thống).
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::AddText {
            x: 60.0,
            y: 300.0,
            text: "Chào FoFreeXit".into(),
            font_size: 18.0,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        }],
        &step1,
        None,
    )
    .expect("add embedded run");
    let idx = find_text_index(&pdf, &step1, "Chào");
    let mid = ff_engine::list_objects(&pdf, &step1, 0, None).expect("list mid");
    let run = mid.iter().find(|o| o.index == idx).expect("run");
    assert_eq!(run.font_embedded, Some(true), "fixture phải là font nhúng");
    let font_before = run.font_name.clone().expect("tên font nhúng");

    // B2: sửa sang câu Việt khác hẳn → phải giữ nguyên font nhúng.
    ff_engine::apply_edits(
        &pdf,
        &step1,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Đã kiểm định — sửa giữ font nhúng".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &step2,
        None,
    )
    .expect("edit embedded run");

    let after = ff_engine::list_objects(&pdf, &step2, 0, None).expect("list out");
    let edited = after
        .iter()
        .find(|o| o.text.as_deref().map(|t| t.contains("kiểm định")).unwrap_or(false))
        .expect("run đã sửa");
    assert_eq!(
        edited.font_name.as_deref(),
        Some(font_before.as_str()),
        "font NHÚNG phải được giữ nguyên khi sửa tiếng Việt"
    );
    let text = ff_engine::extract_text(&pdf, &step2, 0, None).expect("extract");
    assert!(text.contains("sửa giữ font nhúng"), "text round-trip: {text:?}");
}

/// Đổi CỠ CHỮ (không đổi nội dung) phải giữ nguyên font + text, cỡ mới đúng
/// theo nghĩa hiển thị và vị trí neo (left) không trôi.
#[test]
fn font_size_change_keeps_font_and_anchors() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_size.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");
    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list");
    let orig = before.iter().find(|o| o.index == idx).expect("obj").clone();

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: orig.text.clone().unwrap_or_default(),
            font_size: Some(30.0),
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &out,
        None,
    )
    .expect("apply_edits size");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let got = after.iter().find(|o| o.index == idx).expect("obj out");
    assert_eq!(got.font_name, orig.font_name, "đổi cỡ không được đổi font");
    assert_eq!(got.text, orig.text, "đổi cỡ không được đổi nội dung");
    let sz = got.font_size.expect("size");
    assert!((sz - 30.0).abs() < 0.5, "cỡ hiển thị phải ≈30, được {sz}");
    assert!(
        (got.rect.left - orig.rect.left).abs() < 2.0,
        "điểm neo trái không được trôi: {} -> {}",
        orig.rect.left,
        got.rect.left
    );
}

/// Hồi quy bug phóng đại kép: text có matrix scale (Tf nhỏ × matrix lớn) —
/// đặt cỡ hiển thị 20pt phải ra đúng ~20pt, không nhân đôi theo matrix.
#[test]
fn font_size_change_respects_matrix_scale() {
    let pdf = pdfium();
    let input = sample();
    let step1 = tmp("ff_edit_mtx1.pdf");
    let step2 = tmp("ff_edit_mtx2.pdf");
    let step3 = tmp("ff_edit_mtx3.pdf");

    // Tạo run 16pt rồi scale ×2 qua Transform → cỡ hiển thị 32pt, matrix scale 2.
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::AddText {
            x: 80.0,
            y: 200.0,
            text: "MATRIXCASE".into(),
            font_size: 16.0,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        }],
        &step1,
        None,
    )
    .expect("add");
    let idx = find_text_index(&pdf, &step1, "MATRIXCASE");
    ff_engine::apply_edits(
        &pdf,
        &step1,
        0,
        &[EditOp::Transform { index: idx, dx: 0.0, dy: 0.0, sx: 2.0, sy: 2.0 }],
        &step2,
        None,
    )
    .expect("scale");
    let mid = ff_engine::list_objects(&pdf, &step2, 0, None).expect("list mid");
    let scaled = mid.iter().find(|o| o.index == idx).and_then(|o| o.font_size).expect("size mid");
    assert!((scaled - 32.0).abs() < 1.0, "sau scale ×2 phải ~32pt, được {scaled}");

    // Đặt cỡ hiển thị 20 — phải ra ~20 (bug cũ: tạo Tf=20 rồi nhân matrix 2 → 40).
    ff_engine::apply_edits(
        &pdf,
        &step2,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "MATRIXCASE".into(),
            font_size: Some(20.0),
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &step3,
        None,
    )
    .expect("resize");
    let fin = ff_engine::list_objects(&pdf, &step3, 0, None).expect("list fin");
    let got = fin.iter().find(|o| o.index == idx).and_then(|o| o.font_size).expect("size fin");
    assert!((got - 20.0).abs() < 0.5, "cỡ hiển thị phải ≈20, được {got}");
}

/// Ép ĐẬM qua override: font phải đổi sang biến thể đậm (khác font gốc) nhưng
/// nội dung (kể cả tiếng Việt) round-trip nguyên vẹn.
#[test]
fn bold_override_substitutes_font_and_keeps_text() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_edit_bold.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");
    let before = ff_engine::list_objects(&pdf, &input, 0, None).expect("list");
    let font_before = before
        .iter()
        .find(|o| o.index == idx)
        .and_then(|o| o.font_name.clone())
        .expect("font gốc");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Chữ đậm kiểm thử".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: Some(true),
            italic: None,
        }],
        &out,
        None,
    )
    .expect("apply_edits bold");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let edited = after
        .iter()
        .find(|o| o.text.as_deref().map(|t| t.contains("đậm")).unwrap_or(false))
        .expect("run đã sửa");
    assert_ne!(
        edited.font_name.as_deref(),
        Some(font_before.as_str()),
        "ép đậm phải chuyển sang biến thể font khác"
    );
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Chữ đậm kiểm thử"), "text round-trip: {text:?}");
}

/// Luồng UI "sửa cả dòng": 1 batch = SetText(run đầu, text gộp) + Delete(các
/// run còn lại). Chốt hành vi: text mới có mặt, các run kia biến mất, tổng
/// object giảm đúng số run bị gộp.
#[test]
fn line_merge_batch_set_text_plus_delete() {
    let pdf = pdfium();
    let input = sample();
    let step1 = tmp("ff_edit_line1.pdf");
    let step2 = tmp("ff_edit_line2.pdf");

    // Dựng "1 dòng bị cắt làm 2 run" bằng 2 AddText cạnh nhau.
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[
            EditOp::AddText {
                x: 60.0,
                y: 260.0,
                text: "Nửa đầu".into(),
                font_size: 14.0,
                color: [0, 0, 0, 255],
                font_family: None,
                bold: false,
                italic: false,
            },
            EditOp::AddText {
                x: 130.0,
                y: 260.0,
                text: "nửa sau".into(),
                font_size: 14.0,
                color: [0, 0, 0, 255],
                font_family: None,
                bold: false,
                italic: false,
            },
        ],
        &step1,
        None,
    )
    .expect("dựng 2 run");

    let objs = ff_engine::list_objects(&pdf, &step1, 0, None).expect("list");
    let first = find_text_index(&pdf, &step1, "Nửa đầu");
    let second = find_text_index(&pdf, &step1, "nửa sau");
    let count_before = objs.len();

    ff_engine::apply_edits(
        &pdf,
        &step1,
        0,
        &[
            EditOp::SetText {
                index: first,
                text: "Cả dòng đã gộp và sửa".into(),
                font_size: None,
                color: None,
                font_family: None,
                bold: None,
                italic: None,
            },
            EditOp::Delete { index: second },
        ],
        &step2,
        None,
    )
    .expect("batch gộp dòng");

    let after = ff_engine::list_objects(&pdf, &step2, 0, None).expect("list out");
    assert_eq!(after.len(), count_before - 1, "gộp 2 run còn 1 → tổng giảm 1");
    let text = ff_engine::extract_text(&pdf, &step2, 0, None).expect("extract");
    assert!(text.contains("Cả dòng đã gộp và sửa"), "text gộp phải có: {text:?}");
    assert!(!text.contains("nửa sau"), "run bị gộp phải biến mất: {text:?}");
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
