//! Render PDF qua PDFium (tải động pdfium.dll qua pdfium-render).

use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

use crate::EngineError;

/// Ảnh một trang đã render (RGBA8).
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub image: image::DynamicImage,
}

/// Khởi tạo PDFium: tìm và nạp thư viện động.
///
/// Thứ tự tìm: biến môi trường `FOFREEXIT_PDFIUM_PATH` (thư mục chứa lib) →
/// thư mục làm việc hiện tại → thư mục chứa file thực thi → thư viện hệ thống.
pub fn bind_pdfium() -> Result<Pdfium, EngineError> {
    let mut tried: Vec<String> = Vec::new();

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("FOFREEXIT_PDFIUM_PATH") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.clone());
        candidates.push(cwd.join("pdfium"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
        }
    }

    for dir in candidates {
        let lib = Pdfium::pdfium_platform_library_name_at_path(&dir);
        match Pdfium::bind_to_library(&lib) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(e) => tried.push(format!("{} ({e})", lib.display())),
        }
    }

    // Cuối cùng thử thư viện hệ thống.
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(e) => {
            tried.push(format!("system library ({e})"));
            Err(EngineError::PdfiumNotFound(tried.join("; ")))
        }
    }
}

/// Đếm số trang của tài liệu.
pub fn page_count(pdfium: &Pdfium, input: &Path, password: Option<&str>) -> Result<u16, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    Ok(document.pages().len())
}

/// Render một trang (0-based) ra ảnh với chiều rộng mục tiêu `target_width` px.
pub fn render_page(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    target_width: u32,
    password: Option<&str>,
) -> Result<PageImage, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;

    let config = PdfRenderConfig::new()
        .set_target_width(target_width as i32)
        .set_maximum_height((target_width as i32) * 4);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| EngineError::Pdfium(format!("render trang {page_index} thất bại: {e}")))?;

    let image = bitmap.as_image();
    Ok(PageImage {
        width: image.width(),
        height: image.height(),
        image,
    })
}

/// Tỉ lệ pixel KHÁC NHAU (0.0–1.0) giữa cùng 1 trang của 2 file, render cùng
/// bề rộng. Dùng làm CỔNG AN TOÀN cho các phép biến đổi phải giữ nguyên hiển
/// thị (mở gói Form XObject…): lệch quá ngưỡng → từ chối biến đổi thay vì âm
/// thầm làm hỏng file. Sai khác ≤12/255 mỗi kênh bỏ qua (nhiễu anti-alias).
pub fn page_render_mismatch(
    pdfium: &Pdfium,
    a: &Path,
    b: &Path,
    page_index: u16,
    target_width: u32,
) -> Result<f32, EngineError> {
    let ia = render_page(pdfium, a, page_index, target_width, None)?.image.to_rgba8();
    let ib = render_page(pdfium, b, page_index, target_width, None)?.image.to_rgba8();
    if ia.dimensions() != ib.dimensions() {
        return Ok(1.0);
    }
    let (w, h) = ia.dimensions();
    let total = (w as u64 * h as u64).max(1);
    let mut diff = 0u64;
    for (pa, pb) in ia.pixels().zip(ib.pixels()) {
        let d = pa
            .0
            .iter()
            .zip(pb.0.iter())
            .map(|(x, y)| (*x as i16 - *y as i16).unsigned_abs())
            .max()
            .unwrap_or(0);
        if d > 12 {
            diff += 1;
        }
    }
    Ok(diff as f32 / total as f32)
}

/// Render một trang và ghi ra file PNG.
pub fn render_page_png(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    output: &Path,
    target_width: u32,
    password: Option<&str>,
) -> Result<PageImage, EngineError> {
    let rendered = render_page(pdfium, input, page_index, target_width, password)?;
    rendered
        .image
        .save(output)
        .map_err(|e| EngineError::Pdfium(format!("ghi PNG thất bại: {e}")))?;
    Ok(rendered)
}
