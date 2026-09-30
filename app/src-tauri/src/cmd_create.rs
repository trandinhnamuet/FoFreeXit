//! Lệnh Tauri cho "Tạo PDF" (ảnh / trắng / văn bản / web / Office / máy quét /
//! clipboard / gộp tệp), "Xuất" (Word/Excel/PowerPoint/ảnh/TIFF/HTML/RTF/TXT
//! theo phạm vi trang) và OCR mở rộng (bỏ trang có chữ, tiền xử lý, OCR ảnh).
//! Việc nặng chạy `async` (luồng riêng) để UI không đứng.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

use crate::pdfium;

fn stamp() -> String {
    format!(
        "{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn page_size(size: &str, width_mm: Option<f32>, height_mm: Option<f32>) -> Result<(f32, f32), String> {
    if size == "custom" {
        let mm = 72.0 / 25.4;
        let (w, h) = (width_mm.unwrap_or(210.0), height_mm.unwrap_or(297.0));
        if !(20.0..=5000.0).contains(&w) || !(20.0..=5000.0).contains(&h) {
            return Err("kích thước trang tuỳ chỉnh phải trong khoảng 20–5000 mm".into());
        }
        return Ok((w * mm, h * mm));
    }
    ff_engine::standard_page_size(size).ok_or_else(|| format!("khổ giấy không hỗ trợ: {size}"))
}

fn orientation(s: &str) -> ff_engine::Orientation {
    match s {
        "portrait" => ff_engine::Orientation::Portrait,
        "landscape" => ff_engine::Orientation::Landscape,
        _ => ff_engine::Orientation::Auto,
    }
}

/// Thiết lập trang khi tạo PDF từ ảnh.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PageSetupDto {
    /// "fit" | "a4" | "a3" | "a5" | "letter" | "legal" | "custom".
    pub size: String,
    #[serde(default)]
    pub orientation: String,
    #[serde(default)]
    pub margin_mm: f32,
    #[serde(default)]
    pub width_mm: Option<f32>,
    #[serde(default)]
    pub height_mm: Option<f32>,
    #[serde(default = "yes")]
    pub jpeg_passthrough: bool,
    #[serde(default)]
    pub enlarge: bool,
}

fn yes() -> bool {
    true
}

fn image_opts(d: &PageSetupDto) -> Result<ff_engine::ImagePdfOptions, String> {
    let page = if d.size == "fit" {
        ff_engine::PageSizeMode::FitImage
    } else {
        let (w, h) = page_size(&d.size, d.width_mm, d.height_mm)?;
        ff_engine::PageSizeMode::Fixed { width_pt: w, height_pt: h }
    };
    Ok(ff_engine::ImagePdfOptions {
        page,
        orientation: orientation(&d.orientation),
        margin_pt: d.margin_mm.clamp(0.0, 100.0) * 72.0 / 25.4,
        jpeg_passthrough: d.jpeg_passthrough,
        enlarge_small: d.enlarge,
    })
}

/// OCR đơn giản kèm theo khi tạo PDF từ ảnh/máy quét.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OcrAfterDto {
    pub lang: String,
    #[serde(default = "yes")]
    pub preprocess: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResultDto {
    pub pages: u32,
    /// Số từ OCR (nếu có chạy OCR).
    pub words: Option<usize>,
}

fn paths(v: &[String]) -> Vec<PathBuf> {
    v.iter().map(PathBuf::from).collect()
}

/// Ảnh (nhiều tệp, đúng thứ tự) → PDF; tuỳ chọn OCR ngay sau đó.
#[tauri::command(async)]
pub fn create_from_images(
    images: Vec<String>,
    setup: PageSetupDto,
    output: String,
    ocr: Option<OcrAfterDto>,
) -> Result<CreateResultDto, String> {
    let opts = image_opts(&setup)?;
    let out = Path::new(&output);
    match ocr {
        Some(o) => {
            let pdfium = pdfium()?;
            let ocr_opts = ff_engine::OcrOptions { lang: o.lang, preprocess: o.preprocess, ..Default::default() };
            let r = ff_engine::ocr_images_to_pdf(&pdfium, &paths(&images), &opts, &ocr_opts, out)
                .map_err(|e| e.to_string())?;
            let pages = ff_engine::page_count(&pdfium, out, None).map_err(|e| e.to_string())? as u32;
            Ok(CreateResultDto { pages, words: Some(r.words) })
        }
        None => {
            let pages = ff_engine::images_to_pdf(&paths(&images), &opts, out).map_err(|e| e.to_string())?;
            Ok(CreateResultDto { pages, words: None })
        }
    }
}

/// PDF trắng.
#[tauri::command(async)]
pub fn create_blank(
    size: String,
    orientation: String,
    width_mm: Option<f32>,
    height_mm: Option<f32>,
    count: u32,
    output: String,
) -> Result<u32, String> {
    let (w, h) = page_size(&size, width_mm, height_mm)?;
    let (short, long) = (w.min(h), w.max(h));
    let (w, h) = match orientation.as_str() {
        "landscape" => (long, short),
        "portrait" => (short, long),
        _ => (w, h), // tuỳ chỉnh: giữ đúng như người dùng nhập
    };
    ff_engine::blank_pdf(w, h, count, Path::new(&output)).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSetupDto {
    pub size: String,
    #[serde(default)]
    pub orientation: String,
    #[serde(default = "default_text_margin")]
    pub margin_mm: f32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
}

fn default_text_margin() -> f32 {
    20.0
}
fn default_font_size() -> f32 {
    11.0
}

/// Tệp văn bản (.txt UTF-8…) → PDF.
#[tauri::command(async)]
pub fn create_from_text(input: String, setup: TextSetupDto, output: String) -> Result<u32, String> {
    let (w, h) = page_size(&setup.size, None, None)?;
    let (short, long) = (w.min(h), w.max(h));
    let (w, h) = if setup.orientation == "landscape" { (long, short) } else { (short, long) };
    let opts = ff_engine::TextPdfOptions {
        width_pt: w,
        height_pt: h,
        margin_pt: setup.margin_mm.clamp(0.0, 80.0) * 72.0 / 25.4,
        font_size: setup.font_size,
        ..Default::default()
    };
    ff_engine::text_to_pdf(Path::new(&input), &opts, Path::new(&output)).map_err(|e| e.to_string())
}

/// Trang web (URL) / tệp HTML → PDF qua Edge/Chrome headless.
#[tauri::command(async)]
pub fn create_from_web(source: String, output: String, wait_ms: Option<u32>) -> Result<u32, String> {
    let out = Path::new(&output);
    ff_engine::html_to_pdf(&source, out, wait_ms.unwrap_or(3000)).map_err(|e| e.to_string())?;
    let pdfium = pdfium()?;
    ff_engine::page_count(&pdfium, out, None).map(|n| n as u32).map_err(|e| e.to_string())
}

/// Tệp Office → PDF (LibreOffice), ghi đúng `output` (qua thư mục tạm để không
/// đè tệp trùng tên cạnh tệp nguồn).
#[tauri::command(async)]
pub fn create_from_office(input: String, output: String) -> Result<u32, String> {
    let dir = std::env::temp_dir().join(format!("ff_office_{}", stamp()));
    let produced = ff_engine::office_to_pdf(Path::new(&input), &dir).map_err(|e| e.to_string())?;
    let r = std::fs::copy(&produced, &output).map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    r?;
    let pdfium = pdfium()?;
    ff_engine::page_count(&pdfium, Path::new(&output), None).map(|n| n as u32).map_err(|e| e.to_string())
}

/// Gộp nhiều tệp (PDF + ảnh + Office + văn bản + HTML) thành 1 PDF.
#[tauri::command(async)]
pub fn create_combine(files: Vec<String>, setup: PageSetupDto, output: String) -> Result<u32, String> {
    let pdfium = pdfium()?;
    let opts = ff_engine::CombineOptions { image: image_opts(&setup)?, ..Default::default() };
    ff_engine::combine_files(&pdfium, &paths(&files), &opts, Path::new(&output)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfoDto {
    pub path: String,
    /// "pdf" | "image" | "text" | "office" | "html" | "" (không hỗ trợ).
    pub kind: String,
    /// Số trang (PDF, TIFF nhiều trang); None = chưa biết (Office/HTML/văn bản).
    pub pages: Option<u32>,
}

/// Thông tin từng tệp cho danh sách trong hộp thoại (loại + số trang).
#[tauri::command(async)]
pub fn create_source_info(files: Vec<String>) -> Vec<SourceInfoDto> {
    let pdfium = pdfium().ok();
    files
        .into_iter()
        .map(|f| {
            let p = PathBuf::from(&f);
            let kind = ff_engine::source_kind(&p).to_string();
            let pages = match kind.as_str() {
                "pdf" => pdfium.as_ref().and_then(|pd| ff_engine::page_count(pd, &p, None).ok()).map(|n| n as u32),
                "image" => ff_engine::image_page_count(&p).ok().map(|n| n as u32),
                _ => None,
            };
            SourceInfoDto { path: f, kind, pages }
        })
        .collect()
}

/// Công cụ ngoài sẵn có (UI bật/tắt mục + giải thích).
#[tauri::command]
pub fn create_tools_status() -> serde_json::Value {
    serde_json::json!({
        "browser": ff_engine::find_browser().is_ok(),
        "soffice": ff_engine::find_soffice().is_ok(),
        "tesseract": ff_engine::find_tesseract().is_ok(),
        "scanner": cfg!(windows),
        "clipboard": cfg!(windows),
    })
}

// ---------------------------------------------------------------------------
// Máy quét (WIA) + clipboard — Windows, qua PowerShell COM (không thêm crate)
// ---------------------------------------------------------------------------

/// Chạy script PowerShell (STA — COM WIA / WinForms clipboard cần STA) với
/// tham số `-Out <đường dẫn>`, trả dòng kết quả cuối (FF_OK / FF_ERR:...).
#[cfg(windows)]
fn run_ps(script: &str, out: &Path) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let ps1 = std::env::temp_dir().join(format!("ff_ps_{}.ps1", stamp()));
    // BOM UTF-8 để PowerShell 5.1 đọc đúng.
    let mut body = vec![0xEF, 0xBB, 0xBF];
    body.extend_from_slice(script.as_bytes());
    std::fs::write(&ps1, body).map_err(|e| e.to_string())?;
    let res = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&ps1)
        .arg("-Out")
        .arg(out)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW (hộp thoại WIA vẫn hiện)
        .output();
    let _ = std::fs::remove_file(&ps1);
    let o = res.map_err(|e| format!("không chạy được PowerShell: {e}"))?;
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    Ok(text.lines().rev().find(|l| l.starts_with("FF_")).unwrap_or("").trim().to_string())
}

#[cfg(windows)]
const SCAN_PS: &str = r#"param([string]$Out)
$ErrorActionPreference = 'Stop'
try { $dlg = New-Object -ComObject WIA.CommonDialog } catch { Write-Output 'FF_ERR:NOWIA'; exit 2 }
try {
  # 1 = máy quét; 1 = ảnh màu; định dạng JPEG; chọn thiết bị nếu nhiều; hiện UI của driver.
  $img = $dlg.ShowAcquireImage(1, 1, 0, '{B96B3CAE-0728-11D3-9D7B-0000F81EF32E}', $false, $true, $false)
} catch {
  $code = '{0:X8}' -f $_.Exception.HResult
  if ($code -eq '80210015' -or $code -eq '80210005') { Write-Output 'FF_ERR:NODEVICE' }
  elseif ($code -eq '80210064') { Write-Output 'FF_ERR:CANCEL' }
  else { Write-Output ('FF_ERR:' + $_.Exception.Message) }
  exit 3
}
if ($img -eq $null) { Write-Output 'FF_ERR:CANCEL'; exit 4 }
if (Test-Path -LiteralPath $Out) { Remove-Item -LiteralPath $Out -Force }
$img.SaveFile($Out)
Write-Output 'FF_OK'
"#;

#[cfg(windows)]
const CLIP_PS: &str = r#"param([string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$img = [System.Windows.Forms.Clipboard]::GetImage()
if ($img -eq $null) {
  $files = [System.Windows.Forms.Clipboard]::GetFileDropList()
  foreach ($f in $files) {
    if ($f -match '\.(png|jpe?g|bmp|gif|tiff?|webp)$') { Copy-Item -LiteralPath $f -Destination $Out -Force; Write-Output 'FF_OK'; exit 0 }
  }
  Write-Output 'FF_ERR:NOIMAGE'; exit 2
}
$img.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output 'FF_OK'
"#;

/// Quét 1 trang từ máy quét (WIA, hộp thoại của driver). Trả đường dẫn ảnh tạm;
/// None = người dùng huỷ. Lỗi "NO_SCANNER" khi không có máy quét.
#[tauri::command(async)]
pub fn create_scan_page() -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        let out = std::env::temp_dir().join(format!("ff_scan_{}.jpg", stamp()));
        let r = run_ps(SCAN_PS, &out)?;
        return match r.as_str() {
            "FF_OK" if out.is_file() => Ok(Some(out.to_string_lossy().into_owned())),
            "FF_ERR:CANCEL" => Ok(None),
            "FF_ERR:NODEVICE" | "FF_ERR:NOWIA" => Err("NO_SCANNER".into()),
            other => Err(other.trim_start_matches("FF_ERR:").to_string()),
        };
    }
    #[cfg(not(windows))]
    Err("NO_SCANNER".into())
}

/// Ảnh trong clipboard → tệp PNG tạm. Lỗi "NO_CLIPBOARD_IMAGE" nếu không có ảnh.
#[tauri::command(async)]
pub fn create_clipboard_image() -> Result<String, String> {
    #[cfg(windows)]
    {
        let out = std::env::temp_dir().join(format!("ff_scan_clip_{}.png", stamp()));
        let r = run_ps(CLIP_PS, &out)?;
        return match r.as_str() {
            "FF_OK" if out.is_file() => Ok(out.to_string_lossy().into_owned()),
            "FF_ERR:NOIMAGE" => Err("NO_CLIPBOARD_IMAGE".into()),
            other => Err(other.trim_start_matches("FF_ERR:").to_string()),
        };
    }
    #[cfg(not(windows))]
    Err("NO_CLIPBOARD_IMAGE".into())
}

/// Dọn ảnh quét/clipboard tạm — chỉ xoá tệp `ff_scan_*` trong thư mục temp.
#[tauri::command]
pub fn create_cleanup(paths: Vec<String>) {
    let tmp = std::env::temp_dir();
    for p in paths {
        let path = PathBuf::from(&p);
        let ok_name = path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("ff_scan_")).unwrap_or(false);
        if ok_name && path.parent().map(|d| d == tmp).unwrap_or(false) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// Hộp thoại chọn tệp
// ---------------------------------------------------------------------------

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "bmp", "gif", "tif", "tiff", "webp"];

/// Chọn NHIỀU ảnh.
#[tauri::command]
pub fn pick_images(app: tauri::AppHandle, title: String) -> Vec<String> {
    app.dialog()
        .file()
        .set_title(&title)
        .add_filter(&title, IMAGE_EXTS)
        .blocking_pick_files()
        .map(|v| v.into_iter().map(|f| f.to_string()).collect())
        .unwrap_or_default()
}

/// Chọn tệp văn bản.
#[tauri::command]
pub fn pick_text_file(app: tauri::AppHandle, title: String) -> Option<String> {
    app.dialog()
        .file()
        .set_title(&title)
        .add_filter(&title, &["txt", "text", "log", "md", "csv", "ini", "json", "xml"])
        .blocking_pick_file()
        .map(|f| f.to_string())
}

/// Chọn tệp HTML cục bộ.
#[tauri::command]
pub fn pick_html_file(app: tauri::AppHandle, title: String) -> Option<String> {
    app.dialog()
        .file()
        .set_title(&title)
        .add_filter("HTML", &["html", "htm", "xhtml", "mht", "mhtml"])
        .blocking_pick_file()
        .map(|f| f.to_string())
}

/// Chọn NHIỀU tệp đủ loại cho "Gộp tệp".
#[tauri::command]
pub fn pick_combine_files(app: tauri::AppHandle, title: String) -> Vec<String> {
    let mut all: Vec<&str> = vec!["pdf"];
    all.extend_from_slice(IMAGE_EXTS);
    all.extend_from_slice(&[
        "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "txt", "md", "csv", "htm", "html",
    ]);
    app.dialog()
        .file()
        .set_title(&title)
        .add_filter(&title, &all)
        .blocking_pick_files()
        .map(|v| v.into_iter().map(|f| f.to_string()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Xuất (một lệnh chung cho hộp thoại xuất dùng chung)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptsDto {
    /// Trang 0-based; rỗng = mọi trang.
    #[serde(default)]
    pub pages: Vec<u16>,
    pub dpi: Option<f32>,
    pub quality: Option<u8>,
    /// Ưu tiên LibreOffice (Word/PowerPoint) khi máy có.
    #[serde(default)]
    pub use_office: bool,
    /// PowerPoint: "editable" | "picture".
    pub ppt_mode: Option<String>,
    /// HTML: "positioned" | "flowing".
    pub html_mode: Option<String>,
    /// Excel: tiền tố tên sheet (đã dịch theo ngôn ngữ UI).
    pub sheet_prefix: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    pub files: Vec<String>,
    /// "libreoffice" | "basic".
    pub engine: String,
}

/// Chạy bộ chuyển của LibreOffice trên phạm vi trang, ghi đúng `target`.
fn via_office(
    pdfium: &pdfium_render::prelude::Pdfium,
    input: &Path,
    pages: &[u16],
    target: &Path,
    conv: fn(&Path, &Path) -> Result<PathBuf, ff_engine::EngineError>,
) -> Result<(), String> {
    let subset = ff_engine::subset_to_temp(pdfium, input, pages, None).map_err(|e| e.to_string())?;
    let src = subset.clone().unwrap_or_else(|| input.to_path_buf());
    let dir = std::env::temp_dir().join(format!("ff_export_{}", stamp()));
    let r = conv(&src, &dir).map_err(|e| e.to_string()).and_then(|produced| {
        std::fs::copy(&produced, target).map(|_| ()).map_err(|e| e.to_string())
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(s) = subset {
        if let Some(d) = s.parent() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    r
}

/// Xuất PDF sang `format` ("docx"|"xlsx"|"pptx"|"png"|"jpeg"|"tiff"|"html"|"txt"|"rtf").
/// `target` = tệp đích, riêng png/jpeg là THƯ MỤC.
#[tauri::command(async)]
pub fn export_run(input: String, format: String, opts: ExportOptsDto, target: String) -> Result<ExportResultDto, String> {
    let pdfium = pdfium()?;
    let inp = Path::new(&input);
    let out = Path::new(&target);
    let pages = &opts.pages;
    let dpi = opts.dpi.unwrap_or(150.0);
    let single = |engine: &str| ExportResultDto { files: vec![target.clone()], engine: engine.into() };
    let e2s = |e: ff_engine::EngineError| e.to_string();
    match format.as_str() {
        "docx" => {
            if opts.use_office && ff_engine::find_soffice().is_ok()
                && via_office(&pdfium, inp, pages, out, ff_engine::pdf_to_docx_via_soffice).is_ok()
            {
                return Ok(single("libreoffice"));
            }
            ff_engine::export_docx_pages(&pdfium, inp, out, pages, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "pptx" => {
            if opts.use_office && ff_engine::find_soffice().is_ok()
                && via_office(&pdfium, inp, pages, out, ff_engine::pdf_to_pptx_via_soffice).is_ok()
            {
                return Ok(single("libreoffice"));
            }
            let o = ff_engine::PptxOptions {
                editable_text: opts.ppt_mode.as_deref() != Some("picture"),
                dpi: dpi.clamp(72.0, 300.0),
            };
            ff_engine::export_pptx(&pdfium, inp, out, pages, &o, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "xlsx" => {
            let prefix = opts.sheet_prefix.clone().unwrap_or_else(|| "Page".into());
            ff_engine::export_xlsx(&pdfium, inp, out, pages, &prefix, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "png" | "jpeg" => {
            let fmt = if format == "png" {
                ff_engine::RasterFormat::Png
            } else {
                ff_engine::RasterFormat::Jpeg { quality: opts.quality.unwrap_or(85) }
            };
            let files = ff_engine::export_page_images(&pdfium, inp, out, pages, dpi, fmt, None).map_err(e2s)?;
            Ok(ExportResultDto {
                files: files.into_iter().map(|p| p.to_string_lossy().into_owned()).collect(),
                engine: "basic".into(),
            })
        }
        "tiff" => {
            ff_engine::export_tiff(&pdfium, inp, out, pages, dpi, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "html" => {
            let mode = if opts.html_mode.as_deref() == Some("flowing") {
                ff_engine::HtmlMode::Flowing
            } else {
                ff_engine::HtmlMode::Positioned
            };
            ff_engine::export_html(&pdfium, inp, out, pages, mode, dpi.clamp(72.0, 200.0), None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "txt" => {
            ff_engine::export_text_range(&pdfium, inp, out, pages, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        "rtf" => {
            ff_engine::export_rtf(&pdfium, inp, out, pages, None).map_err(e2s)?;
            Ok(single("basic"))
        }
        other => Err(format!("định dạng xuất không hỗ trợ: {other}")),
    }
}

// ---------------------------------------------------------------------------
// OCR mở rộng
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrOptsDto {
    pub lang: String,
    #[serde(default)]
    pub pages: Vec<u16>,
    #[serde(default)]
    pub skip_text: bool,
    #[serde(default)]
    pub preprocess: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReportDto {
    pub words: usize,
    pub pages_ocred: usize,
    pub pages_skipped: usize,
}

impl From<ff_engine::OcrReport> for OcrReportDto {
    fn from(r: ff_engine::OcrReport) -> Self {
        OcrReportDto { words: r.words, pages_ocred: r.pages_ocred, pages_skipped: r.pages_skipped }
    }
}

/// OCR tài liệu với phạm vi trang / bỏ trang có chữ / tiền xử lý.
#[tauri::command(async)]
pub fn ocr_run_ex(input: String, opts: OcrOptsDto, output: String) -> Result<OcrReportDto, String> {
    let pdfium = pdfium()?;
    let o = ff_engine::OcrOptions {
        lang: opts.lang,
        pages: opts.pages,
        skip_text_pages: opts.skip_text,
        preprocess: opts.preprocess,
    };
    ff_engine::ocr_document(&pdfium, Path::new(&input), &o, Path::new(&output), None)
        .map(Into::into)
        .map_err(|e| e.to_string())
}

/// OCR thẳng tệp ảnh → PDF có lớp chữ ẩn.
#[tauri::command(async)]
pub fn ocr_images(images: Vec<String>, lang: String, preprocess: bool, output: String) -> Result<OcrReportDto, String> {
    let pdfium = pdfium()?;
    let o = ff_engine::OcrOptions { lang, preprocess, ..Default::default() };
    ff_engine::ocr_images_to_pdf(&pdfium, &paths(&images), &Default::default(), &o, Path::new(&output))
        .map(Into::into)
        .map_err(|e| e.to_string())
}
