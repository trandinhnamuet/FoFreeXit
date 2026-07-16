//! OCR (Phase 7): nhận dạng chữ trong PDF scan bằng Tesseract (sidecar CLI,
//! như qpdf) rồi thêm **LỚP TEXT ẨN** (render mode Invisible) khớp toạ độ lên
//! CHÍNH trang gốc — file thành searchable/copy được mà không đổi hình ảnh.
//!
//! Luồng: render trang 300 DPI → `tesseract ... tsv` (bảng từ + bbox pixel +
//! confidence) → quy đổi pixel→điểm PDF → tạo text object vô hình đúng khung
//! từng từ. Ngôn ngữ mặc định `vie+eng`.

use std::path::{Path, PathBuf};
use std::process::Command;

use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::text::Rect;
use crate::EngineError;

/// DPI render cho OCR — 300 là chuẩn khuyến nghị của Tesseract.
const OCR_DPI: f32 = 300.0;
/// Ngưỡng confidence (0-100) dưới mức này thì bỏ từ (nhiễu).
const MIN_CONFIDENCE: f32 = 30.0;

/// Tìm binary `tesseract`: env `FOFREEXIT_TESSERACT_PATH` (file hoặc thư mục)
/// → PATH hệ thống.
pub fn find_tesseract() -> Result<PathBuf, EngineError> {
    let exe = if cfg!(windows) { "tesseract.exe" } else { "tesseract" };
    if let Ok(p) = std::env::var("FOFREEXIT_TESSERACT_PATH") {
        let p = PathBuf::from(p);
        let candidate = if p.is_dir() { p.join(exe) } else { p };
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if Command::new(exe).arg("--version").output().is_ok() {
        return Ok(PathBuf::from(exe));
    }
    Err(EngineError::Pdfium(
        "không tìm thấy tesseract. Cài Tesseract OCR (kèm gói ngôn ngữ vie) và/hoặc đặt FOFREEXIT_TESSERACT_PATH".into(),
    ))
}

/// 1 từ OCR được: text + khung theo điểm PDF + confidence 0-100.
#[derive(Clone, Debug)]
pub struct OcrWord {
    pub text: String,
    pub rect: Rect,
    pub confidence: f32,
}

/// OCR 1 trang → danh sách từ (toạ độ điểm PDF, gốc dưới-trái).
pub fn ocr_page_words(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    lang: &str,
    password: Option<&str>,
) -> Result<Vec<OcrWord>, EngineError> {
    let tess = find_tesseract()?;
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let dim = dims
        .iter()
        .find(|d| d.index == page_index)
        .ok_or_else(|| EngineError::Pdfium(format!("không có trang {page_index}")))?;
    let width_px = (dim.width_pt / 72.0 * OCR_DPI).round().max(64.0) as u32;

    let stamp = format!("{}_{}", std::process::id(), page_index);
    let png = std::env::temp_dir().join(format!("ff_ocr_{stamp}.png"));
    let out_base = std::env::temp_dir().join(format!("ff_ocr_{stamp}"));
    let rendered = crate::render::render_page_png(pdfium, input, page_index, &png, width_px, password)?;

    // tesseract <ảnh> <out_base> -l <lang> tsv  → out_base.tsv
    let output = Command::new(&tess)
        .arg(&png)
        .arg(&out_base)
        .args(["-l", lang, "--psm", "3", "tsv"])
        .output()
        .map_err(|e| EngineError::Pdfium(format!("chạy tesseract: {e}")))?;
    let _ = std::fs::remove_file(&png);
    if !output.status.success() {
        return Err(EngineError::Pdfium(format!(
            "tesseract lỗi (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let tsv_path = out_base.with_extension("tsv");
    let tsv = std::fs::read_to_string(&tsv_path)?;
    let _ = std::fs::remove_file(&tsv_path);

    // px → pt theo bề rộng render thật (render có thể làm tròn).
    let scale = dim.width_pt / rendered.width.max(1) as f32;
    let page_h = dim.height_pt;

    let mut words = Vec::new();
    for line in tsv.lines().skip(1) {
        // level page block par line word left top width height conf text
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let conf: f32 = cols[10].parse().unwrap_or(-1.0);
        let text = cols[11].trim();
        if conf < MIN_CONFIDENCE || text.is_empty() {
            continue;
        }
        let (l, t, w, h): (f32, f32, f32, f32) = match (
            cols[6].parse(),
            cols[7].parse(),
            cols[8].parse(),
            cols[9].parse(),
        ) {
            (Ok(l), Ok(t), Ok(w), Ok(h)) => (l, t, w, h),
            _ => continue,
        };
        words.push(OcrWord {
            text: text.to_string(),
            rect: Rect {
                left: l * scale,
                top: page_h - t * scale,
                right: (l + w) * scale,
                bottom: page_h - (t + h) * scale,
            },
            confidence: conf,
        });
    }
    Ok(words)
}

/// OCR các trang `pages` (rỗng = mọi trang) và thêm lớp text ẨN khớp toạ độ
/// lên trang gốc, ghi ra `output`. Trả tổng số từ đã nhận dạng.
pub fn ocr_add_text_layer(
    pdfium: &Pdfium,
    input: &Path,
    pages: &[u16],
    lang: &str,
    output: &Path,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    let err = |e: PdfiumError| EngineError::Pdfium(format!("ocr layer: {e}"));

    // (1) OCR trước (mỗi trang render riêng) — chưa mở document ghi.
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let targets: Vec<u16> = if pages.is_empty() {
        dims.iter().map(|d| d.index).collect()
    } else {
        pages.to_vec()
    };
    let mut per_page: Vec<(u16, Vec<OcrWord>)> = Vec::new();
    for &p in &targets {
        let words = ocr_page_words(pdfium, input, p, lang, password)?;
        if !words.is_empty() {
            per_page.push((p, words));
        }
    }

    // (2) Mở document, nạp font Unicode (đủ tiếng Việt) 1 lần.
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let bytes = find_font_bytes(false, false)
        .ok_or_else(|| EngineError::Pdfium("không tìm được font hệ thống cho lớp OCR".into()))?;
    let token = document
        .fonts_mut()
        .load_true_type_from_bytes(&bytes, true)
        .map_err(|e| EngineError::Pdfium(format!("nạp font OCR: {e}")))?;

    let mut total = 0usize;
    for (page_index, words) in &per_page {
        let mut page = document
            .pages()
            .get(*page_index)
            .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        for w in words {
            let size = (w.rect.top - w.rect.bottom).clamp(4.0, 96.0);
            let mut obj = page
                .objects_mut()
                .create_text_object(
                    PdfPoints::new(w.rect.left),
                    PdfPoints::new(w.rect.bottom),
                    w.text.clone(),
                    token,
                    PdfPoints::new(size),
                )
                .map_err(err)?;
            if let Some(t) = obj.as_text_object_mut() {
                t.set_render_mode(PdfPageTextRenderMode::Invisible).map_err(err)?;
            }
            total += 1;
        }
        page.regenerate_content().map_err(err)?;
    }

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file OCR: {e}")))?;
    Ok(total)
}
