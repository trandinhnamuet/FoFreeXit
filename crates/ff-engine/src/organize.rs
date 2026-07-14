//! Tổ chức trang (Phase 3): chèn/xoá/xoay/trích/thay/đảo/merge/split/crop.
//!
//! Mọi thao tác quy về MỘT core: `build_document` dựng tài liệu MỚI bằng cách
//! copy trang theo đúng thứ tự mô tả trong `plan` từ một hoặc nhiều tài liệu
//! nguồn (PDFium `FPDF_ImportPages*` giữ nguyên nội dung + annotation của
//! trang), rồi áp rotation/crop sau khi copy xong. Insert/Delete/Reorder/
//! Replace/Extract/Merge/Split chỉ là cách build `plan` khác nhau ở tầng gọi
//! (Tauri command) — không cần nhiều hàm engine riêng cho từng thao tác.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

use crate::text::Rect;
use crate::EngineError;

/// Nguồn của một trang trong tài liệu đích.
#[derive(Clone, Debug)]
pub enum PageSource {
    /// Copy trang `src_index` từ file `source` (None = tài liệu chính truyền
    /// vào `build_document`, đã mở kèm `password` nếu có).
    Existing { source: Option<PathBuf>, src_index: u16 },
    /// Trang trắng mới, kích thước theo điểm PDF.
    Blank { width_pt: f32, height_pt: f32 },
}

/// Một "khe" trang trong tài liệu đích.
#[derive(Clone, Debug)]
pub struct PagePlanEntry {
    pub page: PageSource,
    /// Cộng dồn vào rotation hiện có của trang nguồn (0/90/180/270, có thể âm).
    pub rotation_delta: i32,
    /// Nếu có: đặt làm CropBox mới (điểm PDF, gốc toạ độ dưới-trái trang).
    pub crop: Option<Rect>,
}

impl PagePlanEntry {
    pub fn existing(src_index: u16) -> Self {
        PagePlanEntry { page: PageSource::Existing { source: None, src_index }, rotation_delta: 0, crop: None }
    }

    pub fn from_file(source: PathBuf, src_index: u16) -> Self {
        PagePlanEntry { page: PageSource::Existing { source: Some(source), src_index }, rotation_delta: 0, crop: None }
    }

    pub fn blank(width_pt: f32, height_pt: f32) -> Self {
        PagePlanEntry { page: PageSource::Blank { width_pt, height_pt }, rotation_delta: 0, crop: None }
    }
}

fn rotation_to_enum(deg: i32) -> PdfPageRenderRotation {
    match ((deg % 360) + 360) % 360 {
        90 => PdfPageRenderRotation::Degrees90,
        180 => PdfPageRenderRotation::Degrees180,
        270 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    }
}

fn rotation_to_degrees(r: PdfPageRenderRotation) -> i32 {
    match r {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 90,
        PdfPageRenderRotation::Degrees180 => 180,
        PdfPageRenderRotation::Degrees270 => 270,
    }
}

/// Dựng tài liệu mới theo `plan`, ghi ra `output`. `main_input` là tài liệu
/// "đang mở" dùng cho các khe có `source: None`; `password` chỉ áp cho
/// `main_input` (file nguồn khác giả định không mã hoá — đủ cho Phase 3).
pub fn build_document(
    pdfium: &Pdfium,
    main_input: &Path,
    plan: &[PagePlanEntry],
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    if plan.is_empty() {
        return Err(EngineError::Pdfium(
            "plan rỗng — tài liệu phải có ít nhất 1 trang".into(),
        ));
    }

    // Mở mỗi file nguồn đúng 1 lần, dùng lại cho mọi khe trỏ tới nó.
    let mut cache: HashMap<PathBuf, PdfDocument> = HashMap::new();
    for entry in plan {
        if let PageSource::Existing { source, .. } = &entry.page {
            let path = source.clone().unwrap_or_else(|| main_input.to_path_buf());
            if !cache.contains_key(&path) {
                let pw = if path == main_input { password } else { None };
                let doc = pdfium
                    .load_pdf_from_file(&path, pw)
                    .map_err(|e| EngineError::Pdfium(format!("mở {}: {e}", path.display())))?;
                cache.insert(path, doc);
            }
        }
    }

    let mut dest = pdfium
        .create_new_pdf()
        .map_err(|e| EngineError::Pdfium(format!("tạo tài liệu mới: {e}")))?;

    for entry in plan {
        let dest_index = dest.pages().len();
        match &entry.page {
            PageSource::Existing { source, src_index } => {
                let path = source.clone().unwrap_or_else(|| main_input.to_path_buf());
                let src_doc = cache.get(&path).expect("đã mở ở vòng trên");
                dest.pages_mut()
                    .copy_page_from_document(src_doc, *src_index, dest_index)
                    .map_err(|e| EngineError::Pdfium(format!("copy trang {src_index}: {e}")))?;
            }
            PageSource::Blank { width_pt, height_pt } => {
                dest.pages_mut()
                    .create_page_at_index(
                        PdfPagePaperSize::Custom(PdfPoints::new(*width_pt), PdfPoints::new(*height_pt)),
                        dest_index,
                    )
                    .map_err(|e| EngineError::Pdfium(format!("tạo trang trắng: {e}")))?;
            }
        }
    }

    // Áp rotation/crop SAU khi đã copy hết toàn bộ (đảm bảo đủ trang & index ổn định).
    for (i, entry) in plan.iter().enumerate() {
        if entry.rotation_delta == 0 && entry.crop.is_none() {
            continue;
        }
        let mut page = dest
            .pages()
            .get(i as u16)
            .map_err(|e| EngineError::Pdfium(format!("lấy trang đích {i}: {e}")))?;
        if entry.rotation_delta != 0 {
            let cur = rotation_to_degrees(page.rotation().unwrap_or(PdfPageRenderRotation::None));
            page.set_rotation(rotation_to_enum(cur + entry.rotation_delta));
        }
        if let Some(c) = &entry.crop {
            page.boundaries_mut()
                .set_crop(PdfRect::new(
                    PdfPoints::new(c.bottom),
                    PdfPoints::new(c.left),
                    PdfPoints::new(c.top),
                    PdfPoints::new(c.right),
                ))
                .map_err(|e| EngineError::Pdfium(format!("crop trang {i}: {e}")))?;
        }
    }

    dest.save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(())
}

/// Plan "identity": giữ nguyên toàn bộ trang của `input` theo thứ tự hiện tại
/// (dùng làm điểm khởi đầu cho UI Organize, hoặc test).
pub fn identity_plan(pdfium: &Pdfium, input: &Path, password: Option<&str>) -> Result<Vec<PagePlanEntry>, EngineError> {
    let count = crate::render::page_count(pdfium, input, password)?;
    Ok((0..count).map(PagePlanEntry::existing).collect())
}

/// Xoá các trang có index trong `remove` (0-based) khỏi `input`, ghi ra `output`.
pub fn delete_pages(
    pdfium: &Pdfium,
    input: &Path,
    remove: &[u16],
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let plan: Vec<PagePlanEntry> = identity_plan(pdfium, input, password)?
        .into_iter()
        .filter(|e| match &e.page {
            PageSource::Existing { src_index, .. } => !remove.contains(src_index),
            _ => true,
        })
        .collect();
    if plan.is_empty() {
        return Err(EngineError::Pdfium("không thể xoá hết toàn bộ trang".into()));
    }
    build_document(pdfium, input, &plan, output, password)
}

/// Xoay các trang trong `pages` (0-based, rỗng = tất cả) thêm `delta_deg` độ.
pub fn rotate_pages(
    pdfium: &Pdfium,
    input: &Path,
    pages: &[u16],
    delta_deg: i32,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut plan = identity_plan(pdfium, input, password)?;
    for (i, entry) in plan.iter_mut().enumerate() {
        if pages.is_empty() || pages.contains(&(i as u16)) {
            entry.rotation_delta += delta_deg;
        }
    }
    build_document(pdfium, input, &plan, output, password)
}

/// Trích các trang `pages` (0-based, thứ tự giữ nguyên như truyền vào) của
/// `input` ra một file PDF mới `output` (không sửa `input`).
pub fn extract_pages(
    pdfium: &Pdfium,
    input: &Path,
    pages: &[u16],
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    if pages.is_empty() {
        return Err(EngineError::Pdfium("phải chọn ít nhất 1 trang để trích".into()));
    }
    let plan: Vec<PagePlanEntry> = pages.iter().copied().map(PagePlanEntry::existing).collect();
    build_document(pdfium, input, &plan, output, password)
}

/// Trộn nhiều file PDF thành một, theo đúng thứ tự `files` truyền vào (mỗi
/// file lấy toàn bộ trang theo thứ tự gốc). File đầu tiên không cần tồn tại
/// làm "main_input" — chỉ dùng làm tài liệu mở PDFium gốc, không lấy trang từ nó
/// trừ khi nó cũng nằm trong `files`.
pub fn merge_files(pdfium: &Pdfium, files: &[PathBuf], output: &Path) -> Result<(), EngineError> {
    if files.is_empty() {
        return Err(EngineError::Pdfium("phải có ít nhất 1 file để trộn".into()));
    }
    let mut plan = Vec::new();
    for f in files {
        let count = crate::render::page_count(pdfium, f, None)?;
        for i in 0..count {
            plan.push(PagePlanEntry::from_file(f.clone(), i));
        }
    }
    // main_input chỉ cần là 1 đường dẫn hợp lệ (mọi khe đều có `source` riêng).
    build_document(pdfium, &files[0], &plan, output, None)
}

/// Tách `input` thành nhiều file, mỗi file tối đa `pages_per_file` trang liên
/// tiếp. Trả về danh sách đường dẫn đã ghi.
pub fn split_by_page_count(
    pdfium: &Pdfium,
    input: &Path,
    pages_per_file: u16,
    out_dir: &Path,
    base_name: &str,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    if pages_per_file == 0 {
        return Err(EngineError::Pdfium("pages_per_file phải > 0".into()));
    }
    // Mở file nguồn ĐÚNG 1 LẦN, dùng lại cho mọi phần (tránh mở/parse lại N lần
    // như khi gọi build_document mỗi phần — quan trọng với file nhiều trang).
    let src = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(format!("mở {}: {e}", input.display())))?;
    let count = src.pages().len();
    let mut outputs = Vec::new();
    let mut start = 0u16;
    let mut part = 1u32;
    while start < count {
        let end = (start + pages_per_file).min(count);
        let mut dest = pdfium
            .create_new_pdf()
            .map_err(|e| EngineError::Pdfium(format!("tạo tài liệu mới: {e}")))?;
        for (k, i) in (start..end).enumerate() {
            dest.pages_mut()
                .copy_page_from_document(&src, i, k as u16)
                .map_err(|e| EngineError::Pdfium(format!("copy trang {i}: {e}")))?;
        }
        let out = out_dir.join(format!("{base_name}_part{part}.pdf"));
        dest.save_to_file(&out)
            .map_err(|e| EngineError::Pdfium(format!("lưu {}: {e}", out.display())))?;
        outputs.push(out);
        start = end;
        part += 1;
    }
    Ok(outputs)
}
