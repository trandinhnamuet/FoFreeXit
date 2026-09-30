//! OCR (Phase 7): nhận dạng chữ trong PDF scan bằng Tesseract (sidecar CLI,
//! như qpdf) rồi thêm **LỚP TEXT ẨN** (render mode Invisible) khớp toạ độ lên
//! CHÍNH trang gốc — file thành searchable/copy được mà không đổi hình ảnh.
//!
//! Luồng: render trang 300 DPI → (tuỳ chọn) tiền xử lý: xám + nhị phân Otsu +
//! chỉnh nghiêng theo profile chiếu → `tesseract ... tsv` (bảng từ + bbox pixel
//! + confidence) → quy đổi pixel→điểm PDF (quay ngược góc nghiêng nếu có) →
//! tạo text object vô hình đúng khung từng từ. Ngôn ngữ mặc định `vie+eng`.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::{DynamicImage, GrayImage, Luma};
use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::text::Rect;
use crate::EngineError;

/// DPI render cho OCR — 300 là chuẩn khuyến nghị của Tesseract.
const OCR_DPI: f32 = 300.0;
/// Ngưỡng confidence (0-100) dưới mức này thì bỏ từ (nhiễu).
const MIN_CONFIDENCE: f32 = 30.0;
/// Trang có ít nhất chừng này ký tự (không kể khoảng trắng) coi là "đã có chữ".
const HAS_TEXT_MIN_CHARS: usize = 12;

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
    Err(EngineError::Ocr(
        "không tìm thấy tesseract. Cài Tesseract OCR (kèm gói ngôn ngữ vie) và/hoặc đặt FOFREEXIT_TESSERACT_PATH".into(),
    ))
}

/// 1 từ OCR được: text + khung theo điểm PDF + confidence 0-100.
#[derive(Clone, Debug)]
pub struct OcrWord {
    pub text: String,
    /// Khung bao thẳng trục (điểm PDF, gốc dưới-trái).
    pub rect: Rect,
    pub confidence: f32,
    /// Góc dòng chữ (độ, ngược chiều kim đồng hồ) — ≠ 0 khi trang scan nghiêng.
    pub angle: f32,
    /// Gốc baseline-trái của từ (điểm PDF) — nơi đặt text object.
    pub origin: (f32, f32),
    /// Chiều cao từ đo vuông góc với dòng (điểm PDF) — dùng làm cỡ chữ.
    pub height: f32,
}

// ---------------------------------------------------------------------------
// Tiền xử lý ảnh scan
// ---------------------------------------------------------------------------

/// Ngưỡng Otsu (tối đa phương sai giữa 2 lớp) của ảnh xám.
pub fn otsu_threshold(img: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 128;
    }
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut w_b, mut sum_b, mut best, mut best_t) = (0f64, 0f64, -1f64, 128u8);
    for (t, &c) in hist.iter().enumerate() {
        w_b += c as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total as f64 - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * c as f64;
        let m_b = sum_b / w_b;
        let m_f = (sum_all - sum_b) / w_f;
        let between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if between > best {
            best = between;
            best_t = t as u8;
        }
    }
    best_t
}

/// Nhị phân hoá: ≤ ngưỡng → đen (0), còn lại trắng (255).
pub fn binarize(img: &GrayImage, threshold: u8) -> GrayImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        p.0[0] = if p.0[0] <= threshold { 0 } else { 255 };
    }
    out
}

/// Ước lượng góc nghiêng của dòng chữ (độ; dương = dòng đi LÊN về bên phải)
/// bằng profile chiếu: với mỗi góc thử, chiếu điểm đen theo phương dòng và
/// chọn góc cho histogram "nhọn" nhất (tổng bình phương lớn nhất). Ảnh vào là
/// ảnh nhị phân (đen = 0). Tìm thô ±10° bước 0.5° rồi tinh ±0.5° bước 0.05°.
pub fn estimate_skew_deg(bin: &GrayImage) -> f32 {
    // Thu nhỏ để nhanh (giữ tỉ lệ), lấy toạ độ điểm đen.
    let (w, h) = bin.dimensions();
    let step = ((w.max(h) as f32 / 1200.0).ceil() as u32).max(1);
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            if bin.get_pixel(x, y).0[0] < 128 {
                pts.push(((x / step) as f32, (y / step) as f32));
            }
        }
    }
    let total_px = ((w / step) * (h / step)).max(1) as usize;
    // Quá ít điểm đen (trang trắng) hoặc quá nhiều (ảnh tối/ảnh chụp) → bỏ qua.
    if pts.len() < 50 || pts.len() > total_px * 6 / 10 {
        return 0.0;
    }
    let hh = (h / step) as f32;
    let ww = (w / step) as f32;
    let score = |deg: f32| -> f64 {
        let t = deg.to_radians().tan();
        let off = ww * t.abs() + 2.0;
        let n = (hh + 2.0 * off).ceil() as usize + 2;
        let mut bins = vec![0u32; n];
        for &(x, y) in &pts {
            // Trên 1 dòng nghiêng θ (y hướng xuống): y + x·tanθ = hằng.
            let k = (y + x * t + off).round();
            if k >= 0.0 && (k as usize) < n {
                bins[k as usize] += 1;
            }
        }
        bins.iter().map(|&c| (c as f64) * (c as f64)).sum()
    };
    let search = |from: f32, to: f32, step: f32| -> f32 {
        let mut best = (from, f64::MIN);
        let mut a = from;
        while a <= to + 1e-4 {
            let s = score(a);
            if s > best.1 {
                best = (a, s);
            }
            a += step;
        }
        best.0
    };
    let coarse = search(-10.0, 10.0, 0.5);
    let fine = search(coarse - 0.5, coarse + 0.5, 0.05);
    // So với góc 0: chênh không đáng kể thì giữ nguyên (tránh xoay oan).
    if (score(fine) - score(0.0)) / score(0.0).max(1.0) < 0.02 {
        return 0.0;
    }
    (fine * 100.0).round() / 100.0
}

/// Xoay ảnh xám quanh tâm để dòng nghiêng `deg` (quy ước như
/// `estimate_skew_deg`) thành nằm ngang. Giữ nguyên khung, nền trắng.
pub fn rotate_gray(img: &GrayImage, deg: f32) -> GrayImage {
    let (w, h) = img.dimensions();
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut out = GrayImage::from_pixel(w, h, Luma([255]));
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            // Nguồn = Mᵀ·(đích): M = [c −s; s c] (y hướng xuống).
            let sx = c * dx + s * dy + cx;
            let sy = -s * dx + c * dy + cy;
            if sx >= 0.0 && sy >= 0.0 && (sx as u32) < w && (sy as u32) < h {
                out.put_pixel(x, y, *img.get_pixel(sx as u32, sy as u32));
            }
        }
    }
    out
}

/// Điểm trên ảnh đã chỉnh nghiêng → điểm tương ứng trên ảnh gốc.
fn unrotate_point(x: f32, y: f32, deg: f32, w: f32, h: f32) -> (f32, f32) {
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (dx, dy) = (x - cx, y - cy);
    (c * dx + s * dy + cx, -s * dx + c * dy + cy)
}

/// Tiền xử lý cho OCR: xám → Otsu → chỉnh nghiêng. Trả (ảnh, góc đã chỉnh).
pub fn preprocess_for_ocr(img: &DynamicImage) -> (GrayImage, f32) {
    let gray = img.to_luma8();
    let t = otsu_threshold(&gray);
    let bin = binarize(&gray, t);
    let angle = estimate_skew_deg(&bin);
    if angle.abs() < 0.1 {
        (bin, 0.0)
    } else {
        (rotate_gray(&bin, angle), angle)
    }
}

// ---------------------------------------------------------------------------
// OCR
// ---------------------------------------------------------------------------

/// OCR 1 trang → danh sách từ (toạ độ điểm PDF, gốc dưới-trái).
pub fn ocr_page_words(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    lang: &str,
    password: Option<&str>,
) -> Result<Vec<OcrWord>, EngineError> {
    ocr_page_words_ex(pdfium, input, page_index, lang, false, password)
}

/// Như `ocr_page_words`, có tuỳ chọn tiền xử lý (nhị phân + chỉnh nghiêng).
pub fn ocr_page_words_ex(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    lang: &str,
    preprocess: bool,
    password: Option<&str>,
) -> Result<Vec<OcrWord>, EngineError> {
    let tess = find_tesseract()?;
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let dim = dims
        .iter()
        .find(|d| d.index == page_index)
        .ok_or_else(|| EngineError::Pdfium(format!("không có trang {page_index}")))?;
    let width_px = (dim.width_pt / 72.0 * OCR_DPI).round().max(64.0) as u32;

    let stamp = format!("{}_{}_{}", std::process::id(), page_index, crate::create::nanos());
    let png = std::env::temp_dir().join(format!("ff_ocr_{stamp}.png"));
    let out_base = std::env::temp_dir().join(format!("ff_ocr_{stamp}"));
    let rendered = crate::render::render_page(pdfium, input, page_index, width_px, password)?;
    let (img_w, img_h) = (rendered.width as f32, rendered.height as f32);
    let mut angle = 0.0f32;
    if preprocess {
        let (pre, a) = preprocess_for_ocr(&rendered.image);
        angle = a;
        pre.save(&png).map_err(|e| EngineError::Pdfium(format!("ghi ảnh OCR: {e}")))?;
    } else {
        rendered
            .image
            .save(&png)
            .map_err(|e| EngineError::Pdfium(format!("ghi PNG thất bại: {e}")))?;
    }

    // tesseract <ảnh> <out_base> -l <lang> tsv  → out_base.tsv
    let mut cmd = Command::new(&tess);
    cmd.arg(&png).arg(&out_base).args(["-l", lang, "--psm", "3", "tsv"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().map_err(|e| EngineError::Ocr(format!("chạy tesseract: {e}")))?;
    let _ = std::fs::remove_file(&png);
    if !output.status.success() {
        return Err(EngineError::Ocr(format!(
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
    let to_pt = |x: f32, y: f32| -> (f32, f32) {
        let (sx, sy) = if angle != 0.0 { unrotate_point(x, y, angle, img_w, img_h) } else { (x, y) };
        (sx * scale, page_h - sy * scale)
    };

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
        let corners = [to_pt(l, t), to_pt(l + w, t), to_pt(l, t + h), to_pt(l + w, t + h)];
        let rect = Rect {
            left: corners.iter().map(|c| c.0).fold(f32::MAX, f32::min),
            right: corners.iter().map(|c| c.0).fold(f32::MIN, f32::max),
            bottom: corners.iter().map(|c| c.1).fold(f32::MAX, f32::min),
            top: corners.iter().map(|c| c.1).fold(f32::MIN, f32::max),
        };
        words.push(OcrWord {
            text: text.to_string(),
            rect,
            confidence: conf,
            angle,
            origin: corners[2],
            height: h * scale,
        });
    }
    Ok(words)
}

/// Tuỳ chọn OCR tài liệu.
#[derive(Clone, Debug)]
pub struct OcrOptions {
    pub lang: String,
    /// Trang cần OCR (0-based); rỗng = mọi trang.
    pub pages: Vec<u16>,
    /// Bỏ qua trang đã có chữ (PDF gốc số hoá / đã OCR).
    pub skip_text_pages: bool,
    /// Nhị phân Otsu + chỉnh nghiêng trước khi nhận dạng.
    pub preprocess: bool,
}

impl Default for OcrOptions {
    fn default() -> Self {
        OcrOptions { lang: "vie+eng".into(), pages: Vec::new(), skip_text_pages: false, preprocess: false }
    }
}

/// Kết quả OCR.
#[derive(Clone, Debug, Default)]
pub struct OcrReport {
    pub words: usize,
    pub pages_ocred: usize,
    pub pages_skipped: usize,
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
    let opts = OcrOptions { lang: lang.into(), pages: pages.to_vec(), ..Default::default() };
    Ok(ocr_document(pdfium, input, &opts, output, password)?.words)
}

/// OCR đầy đủ tuỳ chọn (phạm vi trang, bỏ trang có chữ, tiền xử lý).
pub fn ocr_document(
    pdfium: &Pdfium,
    input: &Path,
    opts: &OcrOptions,
    output: &Path,
    password: Option<&str>,
) -> Result<OcrReport, EngineError> {
    let err = |e: PdfiumError| EngineError::Pdfium(format!("ocr layer: {e}"));

    // (1) OCR trước (mỗi trang render riêng) — chưa mở document ghi.
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let targets: Vec<u16> = crate::export::resolve_pages(dims.len() as u16, &opts.pages);
    let mut report = OcrReport::default();
    let mut per_page: Vec<(u16, Vec<OcrWord>)> = Vec::new();
    for &p in &targets {
        if opts.skip_text_pages {
            let t = crate::text::extract_text(pdfium, input, p, password)?;
            if t.chars().filter(|c| !c.is_whitespace()).count() >= HAS_TEXT_MIN_CHARS {
                report.pages_skipped += 1;
                continue;
            }
        }
        let words = ocr_page_words_ex(pdfium, input, p, &opts.lang, opts.preprocess, password)?;
        report.pages_ocred += 1;
        if !words.is_empty() {
            per_page.push((p, words));
        }
    }

    // (2) Mở document, nạp font Unicode (đủ tiếng Việt) 1 lần.
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let bytes = find_font_bytes(false, false)
        .ok_or_else(|| EngineError::Ocr("không tìm được font hệ thống cho lớp OCR".into()))?;
    let token = document
        .fonts_mut()
        .load_true_type_from_bytes(&bytes, true)
        .map_err(|e| EngineError::Pdfium(format!("nạp font OCR: {e}")))?;

    for (page_index, words) in &per_page {
        let mut page = document
            .pages()
            .get(*page_index)
            .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        for w in words {
            let rotated = w.angle.abs() >= 0.05;
            let size = w.height.clamp(4.0, 96.0);
            let (x, y) = if rotated { (0.0, 0.0) } else { (w.rect.left, w.rect.bottom) };
            let mut obj = page
                .objects_mut()
                .create_text_object(PdfPoints::new(x), PdfPoints::new(y), w.text.clone(), token, PdfPoints::new(size))
                .map_err(err)?;
            if rotated {
                // Quay theo dòng scan rồi dời tới gốc baseline của từ.
                let (c, s) = (w.angle.to_radians().cos(), w.angle.to_radians().sin());
                obj.apply_matrix(PdfMatrix::new(c, s, -s, c, w.origin.0, w.origin.1)).map_err(err)?;
            }
            if let Some(t) = obj.as_text_object_mut() {
                t.set_render_mode(PdfPageTextRenderMode::Invisible).map_err(err)?;
            }
            report.words += 1;
        }
        page.regenerate_content().map_err(err)?;
    }

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file OCR: {}", crate::pdfium_msg(&e))))?;
    Ok(report)
}

/// OCR thẳng tệp ảnh (JPG/PNG/TIFF nhiều trang…): ảnh → PDF → lớp chữ ẩn.
pub fn ocr_images_to_pdf(
    pdfium: &Pdfium,
    images: &[PathBuf],
    image_opts: &crate::create::ImagePdfOptions,
    opts: &OcrOptions,
    output: &Path,
) -> Result<OcrReport, EngineError> {
    let tmp = std::env::temp_dir().join(format!("ff_ocrimg_{}_{}.pdf", std::process::id(), crate::create::nanos()));
    crate::create::images_to_pdf(images, image_opts, &tmp)?;
    let r = ocr_document(pdfium, &tmp, opts, output, None);
    let _ = std::fs::remove_file(&tmp);
    r
}
