//! Test redaction THẬT (Phase 5): nội dung phải BIẾN MẤT khỏi file (extract
//! không còn, pixel ảnh gốc bị bôi đen thật) — không phải chỉ vẽ đè.

use std::path::PathBuf;

use ff_engine::{redact_areas, EditOp, Rect};

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

/// Rect của text object đầu tiên chứa `needle` trên trang 0.
fn find_text_rect(pdf: &pdfium_render::prelude::Pdfium, input: &std::path::Path, needle: &str) -> Rect {
    ff_engine::list_objects(pdf, input, 0, None)
        .expect("list")
        .into_iter()
        .find(|o| o.text.as_deref().map(|t| t.contains(needle)).unwrap_or(false))
        .unwrap_or_else(|| panic!("không thấy text {needle:?}"))
        .rect
}

/// Redact text: chuỗi mật phải biến mất khỏi extract, chuỗi công khai giữ
/// nguyên, và vùng redact hiển thị ĐEN khi render.
#[test]
fn redact_removes_text_content_for_real() {
    let pdf = pdfium();
    let fx = tmp("ff_redact_fx.pdf");
    let out = tmp("ff_redact_out.pdf");
    let mk = |x: f32, y: f32, s: &str| EditOp::AddText {
        x,
        y,
        text: s.into(),
        font_size: 14.0,
        color: [0, 0, 0, 255],
        font_family: None,
        bold: false,
        italic: false,
    };
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[mk(60.0, 700.0, "PUBLICINFO vẫn còn"), mk(60.0, 600.0, "TOPSECRET42 tuyệt mật")],
        &fx,
        None,
    )
    .expect("fixture");

    // Vùng redact = ĐÚNG cụm ký tự "TOPSECRET42" (sub-rect của run, để kiểm tỉa
    // theo ký tự: phần " tuyệt mật" cùng object phải còn).
    let boxes = ff_engine::page_char_boxes(&pdf, &fx, 0, None).expect("char boxes");
    let secret_boxes: Vec<_> = boxes.iter().filter(|b| b.top < 620.0 && b.bottom > 580.0).collect();
    // Lấy 11 ký tự đầu của dòng (TOPSECRET42) theo thứ tự trái→phải.
    let mut sorted = secret_boxes.clone();
    sorted.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap());
    let head = &sorted[..sorted.len().min(11)];
    let area = Rect {
        left: head.iter().map(|b| b.left).fold(f32::INFINITY, f32::min) - 1.0,
        right: head.iter().map(|b| b.right).fold(f32::NEG_INFINITY, f32::max) + 1.0,
        bottom: head.iter().map(|b| b.bottom).fold(f32::INFINITY, f32::min) - 2.0,
        top: head.iter().map(|b| b.top).fold(f32::NEG_INFINITY, f32::max) + 2.0,
    };

    let touched = redact_areas(&pdf, &fx, 0, &[area], &out, None).expect("redact");
    assert!(touched >= 1, "phải xoá ít nhất 1 object");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(!text.contains("TOPSECRET42"), "nội dung mật phải BIẾN MẤT: {text:?}");
    assert!(text.contains("PUBLICINFO"), "nội dung ngoài vùng phải còn: {text:?}");
    // Cùng dòng với chuỗi mật vẫn còn phần "tuyệt mật" (tỉa theo ký tự, không
    // xoá cả dòng) — chỉ TOPSECRET42 bị chạm.
    assert!(text.contains("mật"), "phần ngoài vùng của cùng dòng phải giữ: {text:?}");

    // Render: tâm vùng redact phải ĐEN.
    let png = tmp("ff_redact_out.png");
    ff_engine::render_page_png(&pdf, &out, 0, &png, 600, None).expect("render");
    let img = image::open(&png).expect("mở png").to_rgba8();
    let dims = ff_engine::page_dims(&pdf, &out, None).expect("dims");
    let scale = 600.0 / dims[0].width_pt;
    let cx = ((area.left + area.right) / 2.0 * scale) as u32;
    let cy = ((dims[0].height_pt - (area.bottom + area.top) / 2.0) * scale) as u32;
    let p = img.get_pixel(cx, cy);
    assert!(p[0] < 40 && p[1] < 40 && p[2] < 40, "tâm vùng redact phải đen, được {p:?}");
}

/// Redact ảnh: pixel vùng giao bị BÔI ĐEN TRONG CHÍNH DỮ LIỆU ẢNH (đọc lại
/// raw image từ file kết quả), phần ngoài vùng giữ nguyên màu gốc.
#[test]
fn redact_blacks_out_image_pixels_for_real() {
    use pdfium_render::prelude::*;
    let pdf = pdfium();
    let fx = tmp("ff_redact_img_fx.pdf");
    let out = tmp("ff_redact_img_out.pdf");

    // Ảnh đỏ 40×32 đặt tại (300,500) khung 100×80pt → bounds (300,500)-(400,580).
    let png = tmp("ff_redact_red.png");
    let mut img = image::RgbImage::new(40, 32);
    for p in img.pixels_mut() {
        *p = image::Rgb([220, 30, 30]);
    }
    img.save(&png).expect("save png");
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[EditOp::AddImage {
            x: 300.0,
            y: 500.0,
            width_pt: 100.0,
            height_pt: 80.0,
            image_path: png.to_string_lossy().into_owned(),
        }],
        &fx,
        None,
    )
    .expect("fixture ảnh");

    // Redact NỬA TRÁI của ảnh.
    let area = Rect { left: 300.0, bottom: 500.0, right: 350.0, top: 580.0 };
    redact_areas(&pdf, &fx, 0, &[area], &out, None).expect("redact ảnh");

    // Đọc lại raw image từ file kết quả — kiểm content thật, không phải hình vẽ đè.
    let document = pdf.load_pdf_from_file(&out, None).expect("mở out");
    let page = document.pages().get(0).expect("trang 0");
    let mut checked = false;
    for obj in page.objects().iter() {
        let Some(io) = obj.as_image_object() else { continue };
        let Ok(raw) = io.get_raw_image() else { continue };
        let rgba = raw.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        if w < 4 {
            continue;
        }
        // Nửa trái đen, nửa phải vẫn đỏ.
        let left_px = rgba.get_pixel(w / 4, h / 2);
        let right_px = rgba.get_pixel(3 * w / 4, h / 2);
        assert!(
            left_px[0] < 40 && left_px[1] < 40 && left_px[2] < 40,
            "pixel nửa trái phải ĐEN trong dữ liệu ảnh: {left_px:?}"
        );
        assert!(right_px[0] > 150 && right_px[1] < 90, "nửa phải vẫn đỏ: {right_px:?}");
        checked = true;
    }
    assert!(checked, "phải còn image object (đã thay bằng bản bôi đen) để kiểm pixel");
}
