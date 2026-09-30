//! Trang đã XOAY / CROP (qua chức năng Organize): text/tìm kiếm không được mất,
//! metadata trang (rotation + hộp trang) đúng, watermark/header-footer nằm
//! trong vùng hiển thị. Fixture corpus/sample-multipage.pdf (Letter 612x792):
//!   trang 0: "FoFreeXit Test Document" / "Page one content alpha"

use std::path::{Path, PathBuf};

use ff_engine::{Anchor, HeaderFooterSpec, Rect, WatermarkSpec};

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

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Crop gốc KHÁC (0,0) — phần trên trang, nơi có tiêu đề.
const CROP: Rect = Rect { left: 30.0, bottom: 300.0, right: 612.0, top: 792.0 };

// Nhận Pdfium của test: bind lần 2 trong cùng tiến trình (thread_safe) sẽ chờ
// khoá toàn cục mà instance đầu đang giữ → treo vĩnh viễn.
fn make_rotated(pdf: &pdfium_render::prelude::Pdfium, out: &Path) {
    ff_engine::rotate_pages(pdf, &fixture(), &[0], 90, out, None).expect("rotate_pages");
}

fn make_cropped(pdf: &pdfium_render::prelude::Pdfium, out: &Path, crop: Rect, rotation_delta: i32) {
    let mut plan = ff_engine::identity_plan(pdf, &fixture(), None).expect("identity plan");
    plan[0].crop = Some(crop);
    plan[0].rotation_delta = rotation_delta;
    ff_engine::build_document(pdf, &fixture(), &plan, out, None).expect("build_document crop");
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn rotated_page_keeps_text_and_search() {
    let pdf = pdfium();
    let out = tmp("ff_rot_text.pdf");
    make_rotated(&pdf, &out);

    let t = ff_engine::extract_text(&pdf, &out, 0, None).expect("text");
    assert!(t.contains("FoFreeXit Test Document"), "trang xoay mất tiêu đề: {t:?}");
    assert!(t.contains("alpha"), "trang xoay mất 'alpha': {t:?}");

    let hits = ff_engine::search(&pdf, &out, "alpha", false, None).expect("search");
    assert_eq!(hits.len(), 1, "tìm 'alpha' trên trang xoay: {hits:?}");
    assert_eq!(hits[0].page_index, 0);
    assert!(hits[0].rect.is_some(), "kết quả phải có khung");
}

#[test]
fn cropped_page_keeps_text_and_search() {
    let pdf = pdfium();
    let out = tmp("ff_crop_text.pdf");
    make_cropped(&pdf, &out, CROP, 0);

    let t = ff_engine::extract_text(&pdf, &out, 0, None).expect("text");
    assert!(t.contains("FoFreeXit Test Document"), "trang crop mất tiêu đề: {t:?}");
    assert!(t.contains("alpha"), "trang crop mất 'alpha': {t:?}");

    let hits = ff_engine::search(&pdf, &out, "FoFreeXit", false, None).expect("search");
    assert!(hits.iter().any(|h| h.page_index == 0), "tìm trên trang crop: {hits:?}");

    // Trang thường không đổi hành vi: vẫn đọc đủ text.
    let normal = ff_engine::extract_text(&pdf, &fixture(), 0, None).expect("text gốc");
    assert!(normal.contains("FoFreeXit Test Document") && normal.contains("alpha"));
}

#[test]
fn page_dims_report_rotation_and_box() {
    let pdf = pdfium();

    // Trang thường: rotation 0, hộp (0,0,612,792).
    let d = ff_engine::page_dims(&pdf, &fixture(), None).expect("dims");
    assert_eq!(d[0].rotation, 0);
    assert!(close(d[0].box_left, 0.0) && close(d[0].box_bottom, 0.0));
    assert!(close(d[0].box_width, 612.0) && close(d[0].box_height, 792.0));

    // Xoay 90: width/height hiển thị đổi chỗ, hộp CHƯA xoay giữ nguyên.
    let rot = tmp("ff_rot_dims.pdf");
    make_rotated(&pdf, &rot);
    let d = ff_engine::page_dims(&pdf, &rot, None).expect("dims rot");
    assert_eq!(d[0].rotation, 90);
    assert!(close(d[0].width_pt, 792.0) && close(d[0].height_pt, 612.0), "{:?}", d[0]);
    assert!(close(d[0].box_width, 612.0) && close(d[0].box_height, 792.0), "{:?}", d[0]);
    assert_eq!(d[1].rotation, 0, "trang 1 không xoay");

    // Crop: hộp = CropBox (gốc lệch), width/height hiển thị = kích thước crop.
    let crop = tmp("ff_crop_dims.pdf");
    make_cropped(&pdf, &crop, CROP, 0);
    let d = ff_engine::page_dims(&pdf, &crop, None).expect("dims crop");
    assert_eq!(d[0].rotation, 0);
    assert!(close(d[0].box_left, 30.0) && close(d[0].box_bottom, 300.0), "{:?}", d[0]);
    assert!(close(d[0].box_width, 582.0) && close(d[0].box_height, 492.0), "{:?}", d[0]);
    assert!(close(d[0].width_pt, 582.0) && close(d[0].height_pt, 492.0), "{:?}", d[0]);

    // Crop + xoay 90.
    let both = tmp("ff_crop_rot_dims.pdf");
    make_cropped(&pdf, &both, CROP, 90);
    let d = ff_engine::page_dims(&pdf, &both, None).expect("dims both");
    assert_eq!(d[0].rotation, 90);
    assert!(close(d[0].box_left, 30.0) && close(d[0].box_bottom, 300.0), "{:?}", d[0]);
    assert!(close(d[0].box_width, 582.0) && close(d[0].box_height, 492.0), "{:?}", d[0]);
    assert!(close(d[0].width_pt, 492.0) && close(d[0].height_pt, 582.0), "{:?}", d[0]);
}

/// Khung hợp (left, bottom, right, top) của chuỗi `needle` trong char boxes trang.
fn text_union(pdf: &pdfium_render::prelude::Pdfium, path: &Path, needle: &str) -> (f32, f32, f32, f32) {
    let boxes = ff_engine::page_char_boxes(pdf, path, 0, None).expect("char boxes");
    let want: Vec<char> = needle.chars().collect();
    let chars: Vec<String> = boxes.iter().map(|b| b.ch.clone()).collect();
    let start = (0..chars.len().saturating_sub(want.len() - 1))
        .find(|&i| (0..want.len()).all(|k| chars[i + k] == want[k].to_string()))
        .unwrap_or_else(|| panic!("không thấy {needle:?} trong char boxes: {:?}", chars.concat()));
    let seg = &boxes[start..start + want.len()];
    let l = seg.iter().map(|b| b.left).fold(f32::MAX, f32::min);
    let bt = seg.iter().map(|b| b.bottom).fold(f32::MAX, f32::min);
    let r = seg.iter().map(|b| b.right).fold(f32::MIN, f32::max);
    let t = seg.iter().map(|b| b.top).fold(f32::MIN, f32::max);
    (l, bt, r, t)
}

fn watermark(text: &str) -> WatermarkSpec {
    WatermarkSpec {
        text: text.to_string(),
        font_size: 24.0,
        color: [200, 0, 0, 255],
        bold: false,
        italic: false,
        rotation_deg: 0.0,
        anchor: Anchor::Center,
        pages: vec![0],
    }
}

#[test]
fn watermark_centered_inside_crop_box() {
    let pdf = pdfium();
    let crop = Rect { left: 300.0, bottom: 400.0, right: 592.0, top: 772.0 };
    let src = tmp("ff_wm_crop_src.pdf");
    let out = tmp("ff_wm_crop_out.pdf");
    make_cropped(&pdf, &src, crop, 0);
    ff_engine::add_watermark(&pdf, &src, &watermark("WMCROP"), &out, None).expect("watermark");

    let (l, b, r, t) = text_union(&pdf, &out, "WMCROP");
    assert!(
        l >= crop.left - 1.0 && r <= crop.right + 1.0 && b >= crop.bottom - 1.0 && t <= crop.top + 1.0,
        "watermark phải nằm trong crop box {crop:?}, được ({l},{b},{r},{t})"
    );
    let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
    assert!((cx - 446.0).abs() < 15.0 && (cy - 586.0).abs() < 15.0, "tâm watermark lệch: ({cx},{cy})");
}

#[test]
fn watermark_upright_and_centered_on_rotated_page() {
    let pdf = pdfium();
    let src = tmp("ff_wm_rot_src.pdf");
    let out = tmp("ff_wm_rot_out.pdf");
    make_rotated(&pdf, &src);
    ff_engine::add_watermark(&pdf, &src, &watermark("WMROTATED"), &out, None).expect("watermark");

    let (l, b, r, t) = text_union(&pdf, &out, "WMROTATED");
    let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
    assert!((cx - 306.0).abs() < 15.0 && (cy - 396.0).abs() < 15.0, "tâm watermark lệch: ({cx},{cy})");
    // Trang xoay 90° (hiển thị ngang): chữ đứng thẳng với người xem nghĩa là
    // chạy DỌC trong không gian PDF chưa xoay.
    assert!(t - b > r - l, "watermark phải xoay theo trang: ({l},{b},{r},{t})");
}

#[test]
fn header_footer_inside_crop_box() {
    let pdf = pdfium();
    let src = tmp("ff_hf_crop_src.pdf");
    let out = tmp("ff_hf_crop_out.pdf");
    make_cropped(&pdf, &src, CROP, 0);
    let spec = HeaderFooterSpec {
        bottom_center: "HFCROP".into(),
        font_size: 12.0,
        color: [0, 0, 0, 255],
        margin_pt: 20.0,
        pages: vec![0],
        ..Default::default()
    };
    ff_engine::add_header_footer(&pdf, &src, &spec, &out, None).expect("header/footer");

    let (l, b, r, t) = text_union(&pdf, &out, "HFCROP");
    assert!(
        l >= CROP.left - 1.0 && r <= CROP.right + 1.0 && b >= CROP.bottom - 1.0 && t <= CROP.top + 1.0,
        "footer phải nằm trong crop box, được ({l},{b},{r},{t})"
    );
    // Footer ở đáy vùng hiển thị (cách mép dưới crop ~margin).
    assert!(b < CROP.bottom + 60.0, "footer phải ở đáy crop box: bottom={b}");
}
