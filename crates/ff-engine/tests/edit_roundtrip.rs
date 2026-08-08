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

/// Dựng "đoạn văn" 3 dòng bằng font nhúng (AddText) tại x=60, baseline cách 15pt.
fn make_paragraph_fixture(pdf: &pdfium_render::prelude::Pdfium, out: &std::path::Path) {
    let mk = |x: f32, y: f32, s: &str| EditOp::AddText {
        x,
        y,
        text: s.into(),
        font_size: 12.0,
        color: [0, 0, 0, 255],
        font_family: None,
        bold: false,
        italic: false,
    };
    ff_engine::apply_edits(
        pdf,
        &sample(),
        0,
        &[
            mk(60.0, 500.0, "Dòng một của đoạn văn mẫu"),
            mk(60.0, 485.0, "dòng hai nối tiếp nội dung"),
            mk(60.0, 470.0, "dòng ba kết thúc đoạn."),
        ],
        out,
        None,
    )
    .expect("dựng đoạn 3 dòng");
}

/// Index (và ObjectInfo) các run thuộc đoạn fixture (baseline 470–500).
fn paragraph_runs(pdf: &pdfium_render::prelude::Pdfium, path: &std::path::Path) -> Vec<ff_engine::ObjectInfo> {
    ff_engine::list_objects(pdf, path, 0, None)
        .expect("list")
        .into_iter()
        .filter(|o| {
            o.kind == ObjectKind::Text && o.rect.bottom > 455.0 && o.rect.bottom < 505.0 && o.rect.left > 50.0
        })
        .collect()
}

/// REFLOW (iteration 3): text dài hơn hẳn → tự bẻ thành NHIỀU dòng hơn, mọi
/// dòng nằm trong bề rộng khối, GIỮ NGUYÊN font nhúng + baseline spacing 15pt.
#[test]
fn reflow_wraps_and_keeps_embedded_font() {
    let pdf = pdfium();
    let fx = tmp("ff_reflow_fx.pdf");
    let out = tmp("ff_reflow_out.pdf");
    make_paragraph_fixture(&pdf, &fx);
    let runs = paragraph_runs(&pdf, &fx);
    assert_eq!(runs.len(), 3, "fixture phải có 3 dòng");
    let font_before = runs[0].font_name.clone().expect("font fixture");
    let left = runs.iter().map(|r| r.rect.left).fold(f32::INFINITY, f32::min);
    let right = runs.iter().map(|r| r.rect.right).fold(f32::NEG_INFINITY, f32::max);
    let width = right - left;

    let long_text = "Nội dung hoàn toàn mới và dài hơn hẳn bản gốc, đủ nhiều từ tiếng Việt \
để buộc thuật toán reflow phải bẻ lại thành nhiều dòng khác nhau trong đúng bề rộng \
của khối đoạn văn ban đầu, giữ nguyên phông chữ nhúng và khoảng cách dòng.";
    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText { indices: runs.iter().map(|r| r.index).collect(), text: long_text.into() }],
        &out,
        None,
    )
    .expect("reflow");

    // Run mới = mọi text object chứa mảnh của text dài (reflow kéo dài xuống dưới
    // ngoài cửa sổ baseline của fixture nên không dùng paragraph_runs).
    let new_runs: Vec<_> = ff_engine::list_objects(&pdf, &out, 0, None)
        .expect("list out")
        .into_iter()
        .filter(|o| {
            o.kind == ObjectKind::Text
                && o.text.as_deref().map(|t| long_text.contains(t.trim()) && !t.trim().is_empty()).unwrap_or(false)
        })
        .collect();
    assert!(new_runs.len() > 3, "text dài phải bẻ thành >3 dòng, được {}", new_runs.len());
    for r in &new_runs {
        assert_eq!(r.font_name.as_deref(), Some(font_before.as_str()), "reflow phải giữ font nhúng");
        assert!(r.rect.left >= left - 2.0, "dòng không được tràn trái");
        assert!(r.rect.right <= left + width + width * 0.05 + 3.0, "dòng không được tràn phải: right={} limit={}", r.rect.right, left + width);
    }
    // Baseline cách đều 15pt.
    let mut bottoms: Vec<f32> = new_runs.iter().map(|r| r.rect.bottom).collect();
    bottoms.sort_by(|a, b| b.partial_cmp(a).unwrap());
    for w in bottoms.windows(2) {
        let d = w[0] - w[1];
        assert!((d - 15.0).abs() < 2.0, "khoảng baseline phải ~15pt, được {d}");
    }
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("thuật toán reflow"), "nội dung round-trip: {norm:?}");
    assert!(!norm.contains("Dòng một của đoạn"), "text cũ phải biến mất");
}

/// `\n` trong text reflow = ngắt dòng cứng (đoạn mới).
#[test]
fn reflow_hard_break_creates_new_line() {
    let pdf = pdfium();
    let fx = tmp("ff_reflow_hb_fx.pdf");
    let out = tmp("ff_reflow_hb_out.pdf");
    make_paragraph_fixture(&pdf, &fx);
    let runs = paragraph_runs(&pdf, &fx);

    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText {
            indices: runs.iter().map(|r| r.index).collect(),
            text: "Đoạn một ngắn.\nĐoạn hai riêng.".into(),
        }],
        &out,
        None,
    )
    .expect("reflow hard break");

    let new_runs: Vec<_> = ff_engine::list_objects(&pdf, &out, 0, None)
        .expect("list out")
        .into_iter()
        .filter(|o| o.text.as_deref().map(|t| t.contains("Đoạn")).unwrap_or(false))
        .collect();
    assert_eq!(new_runs.len(), 2, "2 đoạn = 2 dòng riêng");
    let d = (new_runs[0].rect.bottom - new_runs[1].rect.bottom).abs();
    assert!((d - 15.0).abs() < 2.0, "2 dòng cách đúng nhịp baseline: {d}");
}

/// Reflow trên font base-14 (Helvetica, không nhúng) + text ASCII: các dòng
/// mới dùng font CHUẨN PDF — BaseFont vẫn là Helvetica, file không phình.
#[test]
fn reflow_base14_ascii_keeps_standard_font() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ff_reflow_b14.pdf");
    let idx = find_text_index(&pdf, &input, "Page one");

    let long_ascii = "This replacement paragraph is intentionally much longer than the \
original single line so the reflow engine must wrap it into several lines while keeping \
the declared standard Helvetica base font untouched.";
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::ReflowText { indices: vec![idx], text: long_ascii.into() }],
        &out,
        None,
    )
    .expect("reflow base14");

    let new_runs: Vec<_> = ff_engine::list_objects(&pdf, &out, 0, None)
        .expect("list out")
        .into_iter()
        .filter(|o| o.text.as_deref().map(|t| long_ascii.contains(t.trim()) && !t.trim().is_empty()).unwrap_or(false))
        .collect();
    assert!(new_runs.len() >= 2, "text dài phải bẻ ≥2 dòng, được {}", new_runs.len());
    for r in &new_runs {
        let f = r.font_name.clone().unwrap_or_default();
        assert!(f.contains("Helvetica"), "BaseFont phải giữ Helvetica chuẩn, được {f:?}");
    }
    // Extract chèn ngắt dòng tại điểm wrap → chuẩn hoá khoảng trắng trước khi so.
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("standard Helvetica base font untouched"), "round-trip: {norm:?}");
}

/// Engine tự NỞ danh sách run theo bbox khối: dòng bị cắt làm 3 run, UI chỉ
/// gửi run ĐẦU + CUỐI (sót run giữa — như run rỗng/lệch bbox của Word) —
/// run giữa vẫn bị thay sạch, không còn chữ cũ đè dưới chữ mới.
#[test]
fn reflow_expands_indices_to_cover_block() {
    let pdf = pdfium();
    let fx = tmp("ff_reflow_exp_fx.pdf");
    let out = tmp("ff_reflow_exp_out.pdf");
    let mk = |x: f32, s: &str| EditOp::AddText {
        x,
        y: 400.0,
        text: s.into(),
        font_size: 14.0,
        color: [0, 0, 0, 255],
        font_family: None,
        bold: false,
        italic: false,
    };
    // 1 dòng thị giác bị cắt thành 3 run sát nhau (kiểu Word).
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[mk(60.0, "Mảnh một"), mk(125.0, "mảnh hai"), mk(188.0, "mảnh ba.")],
        &fx,
        None,
    )
    .expect("fixture 3 mảnh");
    let first = find_text_index(&pdf, &fx, "Mảnh một");
    let third = find_text_index(&pdf, &fx, "mảnh ba");

    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText { indices: vec![first, third], text: "Dòng đã thay hoàn toàn".into() }],
        &out,
        None,
    )
    .expect("reflow partial");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("Dòng đã thay hoàn toàn"), "text mới: {norm:?}");
    assert!(!norm.contains("mảnh hai"), "run GIỮA bị sót phải được nở vào và xoá: {norm:?}");
    assert!(!norm.contains("Mảnh một"), "run đã đưa phải bị thay: {norm:?}");
}

/// Khối gốc CĂN GIỮA (dòng ngắn thụt vào, tâm trùng nhau) → các dòng mới cũng
/// đặt căn giữa theo tâm khối (không dồn hết về lề trái).
#[test]
fn reflow_preserves_centered_alignment() {
    let pdf = pdfium();
    let probe = tmp("ff_center_probe.pdf");
    let fx = tmp("ff_center_fx.pdf");
    let out = tmp("ff_center_out.pdf");
    let mk = |x: f32, y: f32, s: &str| EditOp::AddText {
        x,
        y,
        text: s.into(),
        font_size: 16.0,
        color: [0, 0, 0, 255],
        font_family: None,
        bold: false,
        italic: false,
    };
    // Đo bề rộng thật 2 dòng để dựng fixture căn giữa quanh tâm 300.
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[mk(10.0, 500.0, "TIÊU ĐỀ DÀI Ở DÒNG TRÊN"), mk(10.0, 460.0, "DÒNG DƯỚI NGẮN")],
        &probe,
        None,
    )
    .expect("probe");
    let objs = ff_engine::list_objects(&pdf, &probe, 0, None).expect("list probe");
    let w = |needle: &str| {
        let o = objs
            .iter()
            .find(|o| o.text.as_deref().map(|t| t.contains(needle)).unwrap_or(false))
            .expect(needle);
        o.rect.right - o.rect.left
    };
    let (w1, w2) = (w("DÒNG TRÊN"), w("DÒNG DƯỚI"));
    let cx = 300.0;
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[
            mk(cx - w1 / 2.0, 500.0, "TIÊU ĐỀ DÀI Ở DÒNG TRÊN"),
            mk(cx - w2 / 2.0, 480.0, "DÒNG DƯỚI NGẮN"),
        ],
        &fx,
        None,
    )
    .expect("fixture centered");
    let idxs: Vec<u16> = ff_engine::list_objects(&pdf, &fx, 0, None)
        .expect("list fx")
        .into_iter()
        .filter(|o| o.text.as_deref().map(|t| t.contains("DÒNG")).unwrap_or(false))
        .map(|o| o.index)
        .collect();
    assert_eq!(idxs.len(), 2);

    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText {
            indices: idxs,
            text: "TIÊU ĐỀ MỚI DÀI HƠN CHÚT\nNGẮN".into(),
        }],
        &out,
        None,
    )
    .expect("reflow centered");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let mut checked = 0;
    for o in &after {
        let Some(t) = &o.text else { continue };
        if t.contains("TIÊU ĐỀ MỚI") || t.trim() == "NGẮN" {
            let c = (o.rect.left + o.rect.right) / 2.0;
            assert!((c - cx).abs() < 6.0, "dòng mới phải căn giữa quanh {cx}: center={c} text={t:?}");
            checked += 1;
        }
    }
    assert_eq!(checked, 2, "phải kiểm được 2 dòng mới");
}

/// Mỗi DÒNG CỨNG (theo `\n` từ ô sửa) giữ đúng CỠ CHỮ của dòng gốc cùng thứ
/// tự — tiêu đề 22/16pt không bị ép cả khối về 1 cỡ.
#[test]
fn reflow_keeps_per_line_font_sizes() {
    let pdf = pdfium();
    let fx = tmp("ff_reflow_sz_fx.pdf");
    let out = tmp("ff_reflow_sz_out.pdf");
    let mk = |y: f32, s: &str, size: f32| EditOp::AddText {
        x: 80.0,
        y,
        text: s.into(),
        font_size: size,
        color: [0, 0, 0, 255],
        font_family: None,
        bold: false,
        italic: false,
    };
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[mk(500.0, "TIÊU ĐỀ CHÍNH CỠ LỚN", 22.0), mk(474.0, "phụ đề bên dưới cỡ nhỏ", 16.0)],
        &fx,
        None,
    )
    .expect("fixture 2 cỡ chữ");
    let idxs: Vec<u16> = ff_engine::list_objects(&pdf, &fx, 0, None)
        .expect("list fx")
        .into_iter()
        .filter(|o| o.text.as_deref().map(|t| t.contains("CỠ LỚN") || t.contains("cỡ nhỏ")).unwrap_or(false))
        .map(|o| o.index)
        .collect();
    assert_eq!(idxs.len(), 2);

    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText { indices: idxs, text: "TIÊU ĐỀ MỚI\nphụ đề mới".into() }],
        &out,
        None,
    )
    .expect("reflow 2 cỡ");

    let after = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let size_of = |needle: &str| {
        after
            .iter()
            .find(|o| o.text.as_deref().map(|t| t.contains(needle)).unwrap_or(false))
            .unwrap_or_else(|| panic!("thiếu dòng {needle:?}"))
            .font_size
            .expect("font_size")
    };
    let s1 = size_of("TIÊU ĐỀ MỚI");
    let s2 = size_of("phụ đề mới");
    assert!((s1 - 22.0).abs() < 0.6, "dòng 1 phải giữ ~22pt, được {s1}");
    assert!((s2 - 16.0).abs() < 0.6, "dòng 2 phải giữ ~16pt, được {s2}");
}

/// Dòng cứng chỉ DÀI THÊM CHÚT (≤35% bề rộng khối) thì NỞ ra chứ không bị bẻ
/// lại thành 2 dòng (hộp text tự nở như Foxit) — sửa "HỢP"→"HỢPP" phải giữ
/// nguyên số dòng. Dấu cách cũng phải sống sót qua đường ghi.
#[test]
fn reflow_hard_line_grows_without_rewrap() {
    let pdf = pdfium();
    let fx = tmp("ff_reflow_grow_fx.pdf");
    let out = tmp("ff_reflow_grow_out.pdf");
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[EditOp::AddText {
            x: 90.0,
            y: 520.0,
            text: "Dòng tiêu đề dài vừa khít khung gốc".into(),
            font_size: 18.0,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        }],
        &fx,
        None,
    )
    .expect("fixture 1 dòng");
    let idx = find_text_index(&pdf, &fx, "vừa khít");

    let new_text = "Dòng tiêu đề dài vừa khít khung gốc nở";
    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText { indices: vec![idx], text: new_text.into() }],
        &out,
        None,
    )
    .expect("reflow nở dòng");

    let new_runs: Vec<_> = ff_engine::list_objects(&pdf, &out, 0, None)
        .expect("list out")
        .into_iter()
        .filter(|o| o.text.as_deref().map(|t| t.contains("tiêu đề") || t.contains("nở thêm")).unwrap_or(false))
        .collect();
    assert_eq!(new_runs.len(), 1, "dòng chỉ dài thêm ~20% phải GIỮ 1 dòng: {new_runs:?}");
    assert_eq!(new_runs[0].text.as_deref(), Some(new_text), "nội dung (gồm dấu cách) phải tròn trịa");
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


// ---- Phase 4 iteration 4: sửa text NẰM TRONG Form XObject (file Canva/
// Illustrator gói cả trang vào form — trước đây không thấy gì để sửa) ----

/// Dựng PDF 1 trang mà TOÀN BỘ chữ nằm trong 1 Form XObject (như Canva xuất):
/// trang chỉ có 1 object "form", bên trong có 1 text run Helvetica 24pt.
fn build_form_xobject_pdf(path: &std::path::Path) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        // 1 path đỏ + 2 dòng text — đủ để kiểm phẫu thuật giữ nguyên phần khác.
        b"q 0.9 0.2 0.2 rg 40 40 100 20 re f Q BT /F1 24 Tf 72 700 Td (Hello inside form) Tj ET BT /F1 18 Tf 72 650 Td (Second line stays) Tj ET".to_vec(),
    ));
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"q /Fx1 Do Q".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Fx1" => form_id } },
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("lưu fixture form xobject");
}

fn form_fixture(name: &str) -> PathBuf {
    let p = tmp(name);
    build_form_xobject_pdf(&p);
    p
}

#[test]
fn form_xobject_children_are_listed() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_list.pdf");
    let objs = ff_engine::list_objects(&pdf, &input, 0, None).expect("list");
    // Form cấp trang phải được đánh dấu expanded, con text phải lộ ra.
    let form = objs.iter().find(|o| o.kind == ObjectKind::XObjectForm).expect("phải thấy form");
    assert!(form.expanded, "form có con phải expanded: {form:?}");
    let text = objs
        .iter()
        .find(|o| o.kind == ObjectKind::Text && o.text.as_deref().map(|t| t.contains("Hello inside form")).unwrap_or(false))
        .unwrap_or_else(|| panic!("phải thấy text trong form: {objs:?}"));
    assert!(text.nested, "text trong form phải nested: {text:?}");
    // Toạ độ đã quy về trang: baseline 700, cỡ 24 → bbox quanh (72, ~694..~724).
    assert!(text.rect.left > 60.0 && text.rect.left < 84.0, "left ~72: {:?}", text.rect);
    assert!(text.rect.bottom > 680.0 && text.rect.top < 740.0, "bbox quanh y=700: {:?}", text.rect);
    assert!((text.font_size.unwrap_or(0.0) - 24.0).abs() < 2.0, "cỡ hiển thị ~24: {:?}", text.font_size);
}

// PDFium KHÔNG ghi lại được stream của Form XObject (CPDF_PageContentManager
// chỉ biết /Contents của trang) → muốn sửa phải MỞ GÓI form ra cấp trang.
#[test]
fn flatten_form_xobjects_unwraps_page() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_flatten.pdf");
    let out = tmp("ff_edit_formx_flatten_out.pdf");

    let n = ff_engine::flatten_form_xobjects(&pdf, &input, 0, &out, None).expect("flatten");
    assert_eq!(n, 1, "phải mở đúng 1 form");

    let objs = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    assert!(
        objs.iter().all(|o| o.kind != ObjectKind::XObjectForm),
        "không còn form nào: {objs:?}"
    );
    let t = objs
        .iter()
        .find(|o| o.kind == ObjectKind::Text && o.text.as_deref().map(|s| s.contains("Hello inside form")).unwrap_or(false))
        .unwrap_or_else(|| panic!("text phải thành cấp trang: {objs:?}"));
    assert!(!t.nested, "text sau flatten phải hết nested: {t:?}");
    // Vị trí giữ nguyên (baseline 700, lề 72) và render vẫn ra chữ.
    assert!(t.rect.left > 60.0 && t.rect.left < 84.0, "left ~72: {:?}", t.rect);
    assert!(t.rect.bottom > 680.0 && t.rect.top < 740.0, "bbox quanh y=700: {:?}", t.rect);
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Hello inside form"), "nội dung không đổi: {text:?}");
}

// Sau flatten, sửa text (giữ font base-14) round-trip như trang thường.
#[test]
fn form_xobject_settext_after_flatten_roundtrip() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_settext.pdf");
    let flat = tmp("ff_edit_formx_settext_flat.pdf");
    let out = tmp("ff_edit_formx_settext_out.pdf");
    ff_engine::flatten_form_xobjects(&pdf, &input, 0, &flat, None).expect("flatten");
    let idx = find_text_index(&pdf, &flat, "Hello inside form");

    ff_engine::apply_edits(
        &pdf,
        &flat,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "Edited in form!".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &out,
        None,
    )
    .expect("apply_edits SetText sau flatten");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Edited in form!"), "text mới phải có mặt: {text:?}");
    assert!(!text.contains("Hello inside form"), "text cũ phải biến mất: {text:?}");
}

// Reflow tiếng Việt sau flatten: bẻ dòng quanh vị trí khối cũ.
#[test]
fn form_xobject_reflow_after_flatten_roundtrip() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_reflow.pdf");
    let flat = tmp("ff_edit_formx_reflow_flat.pdf");
    let out = tmp("ff_edit_formx_reflow_out.pdf");
    ff_engine::flatten_form_xobjects(&pdf, &input, 0, &flat, None).expect("flatten");
    let idx = find_text_index(&pdf, &flat, "Hello inside form");

    ff_engine::apply_edits(
        &pdf,
        &flat,
        0,
        &[EditOp::ReflowText {
            indices: vec![idx],
            text: "Đoạn văn tiếng Việt thay thế trong form, đủ dài để chắc chắn phải bẻ xuống dòng mới".into(),
        }],
        &out,
        None,
    )
    .expect("apply_edits ReflowText sau flatten");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    // Reflow bẻ dòng ở giữa cụm từ → so khớp trên bản CHUẨN HOÁ khoảng trắng.
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("Đoạn văn tiếng Việt thay thế"), "text mới phải có mặt: {text:?}");
    assert!(!norm.contains("Hello inside form"), "text cũ phải biến mất: {text:?}");
    let objs = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    let new_runs: Vec<_> = objs
        .iter()
        .filter(|o| o.kind == ObjectKind::Text && o.text.as_deref().map(|t| t.contains("Đoạn")).unwrap_or(false))
        .collect();
    assert!(!new_runs.is_empty(), "phải có run mới: {objs:?}");
    for r in &new_runs {
        assert!(
            r.rect.left > 50.0 && r.rect.top < 740.0 && r.rect.bottom > 500.0,
            "dòng mới phải quanh khối cũ: {:?}",
            r.rect
        );
    }
}

// Xoá sau flatten: text biến mất thật khỏi file lưu ra.
#[test]
fn form_xobject_delete_after_flatten_clears_text() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_delete.pdf");
    let flat = tmp("ff_edit_formx_delete_flat.pdf");
    let out = tmp("ff_edit_formx_delete_out.pdf");
    ff_engine::flatten_form_xobjects(&pdf, &input, 0, &flat, None).expect("flatten");
    let idx = find_text_index(&pdf, &flat, "Hello inside form");

    ff_engine::apply_edits(&pdf, &flat, 0, &[EditOp::Delete { index: idx }], &out, None)
        .expect("apply_edits Delete sau flatten");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(!text.contains("Hello inside form"), "text phải biến mất sau xoá: {text:?}");
}

// SetText tại chỗ vào object trong form vẫn bị TỪ CHỐI rõ ràng (toolbar đổi
// thuộc tính) — chỉ ReflowText/Delete đi đường phẫu thuật.
#[test]
fn nested_settext_is_rejected() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_reject.pdf");
    let out = tmp("ff_edit_formx_reject_out.pdf");
    let idx = find_text_index(&pdf, &input, "Hello inside form");

    let err = ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::SetText {
            index: idx,
            text: "abc".into(),
            font_size: None,
            color: None,
            font_family: None,
            bold: None,
            italic: None,
        }],
        &out,
        None,
    )
    .expect_err("SetText vào object trong form phải bị từ chối");
    assert!(format!("{err}").contains("mở gói"), "thông điệp phải rõ: {err}");
}

// PHẪU THUẬT: xoá 1 dòng TRONG form không cần mở gói — dòng kia + path đỏ
// giữ nguyên, form còn nguyên là form, cổng an toàn (trong apply_edits) pass.
#[test]
fn form_xobject_surgical_delete() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_surg_del.pdf");
    let out = tmp("ff_edit_formx_surg_del_out.pdf");
    let idx = find_text_index(&pdf, &input, "Hello inside form");

    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Delete { index: idx }], &out, None)
        .expect("phẫu thuật xoá trong form");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(!text.contains("Hello inside form"), "dòng xoá phải biến mất: {text:?}");
    assert!(text.contains("Second line stays"), "dòng còn lại phải nguyên: {text:?}");
    let objs = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    assert!(
        objs.iter().any(|o| o.kind == ObjectKind::XObjectForm),
        "form phải CÒN NGUYÊN (không mở gói): {objs:?}"
    );
}

// PHẪU THUẬT: sửa cả đoạn TRONG form (reflow) — chữ cũ mất, chữ mới hiện,
// phần khác của form giữ nguyên.
#[test]
fn form_xobject_surgical_reflow() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_surg_ref.pdf");
    let out = tmp("ff_edit_formx_surg_ref_out.pdf");
    let idx = find_text_index(&pdf, &input, "Hello inside form");

    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::ReflowText {
            indices: vec![idx],
            text: "Sửa trong form bằng phẫu thuật stream".into(),
        }],
        &out,
        None,
    )
    .expect("phẫu thuật reflow trong form");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("Sửa trong form"), "chữ mới phải có mặt: {text:?}");
    assert!(!norm.contains("Hello inside form"), "chữ cũ phải biến mất: {text:?}");
    assert!(norm.contains("Second line stays"), "dòng khác phải nguyên: {text:?}");
    let objs = ff_engine::list_objects(&pdf, &out, 0, None).expect("list out");
    assert!(
        objs.iter().any(|o| o.kind == ObjectKind::XObjectForm),
        "form phải còn nguyên: {objs:?}"
    );
}

// Cổng an toàn: mở gói fixture đơn giản phải giữ nguyên hiển thị (lệch ~0%).
#[test]
fn flatten_is_visually_lossless_on_simple_form() {
    let pdf = pdfium();
    let input = form_fixture("ff_edit_formx_gate.pdf");
    let out = tmp("ff_edit_formx_gate_out.pdf");
    ff_engine::flatten_form_xobjects(&pdf, &input, 0, &out, None).expect("flatten");
    let mm = ff_engine::page_render_mismatch(&pdf, &input, &out, 0, 500).expect("mismatch");
    assert!(mm < 0.005, "mở gói phải giữ nguyên hiển thị, lệch {:.3}%", mm * 100.0);
}
