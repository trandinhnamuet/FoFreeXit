//! Smoke test cho pipeline render Phase 1.
//!
//! Dùng golden test "ổn định" (không hash byte vì kết quả render có thể đổi
//! theo phiên bản PDFium): kiểm tra mở được file, đúng số trang, đúng tỉ lệ
//! kích thước, và trang KHÔNG trắng trơn (có pixel tối = đã vẽ nội dung).

use std::path::PathBuf;

/// Thư mục gốc workspace (từ crate dir đi lên 2 cấp: crates/ff-engine -> root).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn ensure_pdfium_path() {
    // Trỏ engine tới pdfium.dll ở gốc workspace (tải bằng scripts/fetch-pdfium.ps1).
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
}

#[test]
fn renders_hello_sample() {
    ensure_pdfium_path();
    let pdfium = ff_engine::bind_pdfium().expect("nạp PDFium (chạy scripts/fetch-pdfium.ps1?)");

    let pdf = workspace_root().join("corpus").join("hello.pdf");
    assert!(pdf.exists(), "thiếu file mẫu: {}", pdf.display());

    // Đúng số trang.
    let pages = ff_engine::page_count(&pdfium, &pdf, None).expect("đếm trang");
    assert_eq!(pages, 1, "hello.pdf phải có 1 trang");

    // Render trang 0.
    let target_width = 800;
    let rendered = ff_engine::render::render_page(&pdfium, &pdf, 0, target_width, None)
        .expect("render trang 0");

    // Đúng chiều rộng mục tiêu.
    assert_eq!(rendered.width, target_width, "chiều rộng render sai");

    // Trang Letter (612x792) -> tỉ lệ cao/rộng ~ 792/612 = 1.294.
    let ratio = rendered.height as f32 / rendered.width as f32;
    assert!(
        (ratio - 792.0 / 612.0).abs() < 0.02,
        "tỉ lệ trang sai: {ratio}"
    );

    // Trang phải có nội dung: đếm pixel "tối" (chữ đen trên nền trắng).
    let rgba = rendered.image.to_rgba8();
    let dark = rgba
        .pixels()
        .filter(|p| p.0[3] > 10 && p.0[0] < 128 && p.0[1] < 128 && p.0[2] < 128)
        .count();
    assert!(
        dark > 200,
        "trang gần như trắng trơn (chỉ {dark} pixel tối) -> render hỏng?"
    );
}
