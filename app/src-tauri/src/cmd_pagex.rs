//! Lệnh Tauri cho Page Marks (tab Trang): hình mờ, nền, đầu & chân trang,
//! đánh số Bates, gỡ dấu. Thao tác nặng chạy trên thread nền
//! (`spawn_blocking`) để UI không đơ với tài liệu nghìn trang / nhiều tệp.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

fn one() -> f32 {
    1.0
}
fn d36() -> f32 {
    36.0
}
fn d10() -> f32 {
    10.0
}
fn black4() -> [u8; 4] {
    [0, 0, 0, 255]
}
fn m36() -> f32 {
    36.0
}
fn m72() -> f32 {
    72.0
}
fn start1() -> u32 {
    1
}

fn parse_anchor(s: &str) -> Result<ff_engine::Anchor, String> {
    use ff_engine::Anchor::*;
    Ok(match s {
        "top-left" => TopLeft,
        "top-center" => TopCenter,
        "top-right" => TopRight,
        "middle-left" => MiddleLeft,
        "center" | "" => Center,
        "middle-right" => MiddleRight,
        "bottom-left" => BottomLeft,
        "bottom-center" => BottomCenter,
        "bottom-right" => BottomRight,
        other => return Err(format!("vị trí neo không hợp lệ: {other}")),
    })
}

fn parse_font(s: &str) -> ff_engine::FontChoice {
    use ff_engine::FontChoice::*;
    match s {
        "serif" => Serif,
        "mono" => Mono,
        "helvetica" => Helvetica,
        "times" => Times,
        "courier" => Courier,
        _ => Sans,
    }
}

fn parse_subset(s: &str) -> ff_engine::PageSubset {
    match s {
        "even" => ff_engine::PageSubset::Even,
        "odd" => ff_engine::PageSubset::Odd,
        _ => ff_engine::PageSubset::All,
    }
}

fn parse_kind(s: &str) -> Result<Vec<ff_engine::MarkKind>, String> {
    use ff_engine::MarkKind::*;
    Ok(match s {
        "watermark" => vec![Watermark],
        "background" => vec![Background],
        "headerFooter" => vec![Header, Footer],
        "bates" => vec![Bates],
        other => return Err(format!("loại dấu trang không hợp lệ: {other}")),
    })
}

/// Hình mờ / nền (dạng phẳng cho frontend).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StampDto {
    /// "text" | "file" (ảnh hoặc PDF theo đuôi tệp) | "color"
    source: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    font: String,
    #[serde(default = "d36")]
    font_size: f32,
    #[serde(default)]
    color: [u8; 3],
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    file: String,
    #[serde(default)]
    file_page: u16,
    #[serde(default)]
    rotation_deg: f32,
    #[serde(default = "one")]
    opacity: f32,
    #[serde(default = "one")]
    scale: f32,
    relative_scale: Option<f32>,
    #[serde(default)]
    anchor: String,
    #[serde(default)]
    offset_x: f32,
    #[serde(default)]
    offset_y: f32,
    #[serde(default)]
    behind: bool,
    #[serde(default)]
    replace_existing: bool,
    #[serde(default)]
    pages: Vec<u16>,
    #[serde(default)]
    subset: String,
}

fn stamp_spec(d: StampDto) -> Result<ff_engine::WatermarkSpec, String> {
    let source = match d.source.as_str() {
        "text" => ff_engine::StampSource::Text {
            text: d.text,
            style: ff_engine::TextStyle {
                font: parse_font(&d.font),
                font_size: d.font_size.max(1.0),
                color: d.color,
                bold: d.bold,
                italic: d.italic,
            },
        },
        "file" => {
            if d.file.is_empty() {
                return Err("chưa chọn tệp nguồn".into());
            }
            let path = PathBuf::from(&d.file);
            let is_pdf = path.extension().map_or(false, |e| e.eq_ignore_ascii_case("pdf"));
            if is_pdf {
                ff_engine::StampSource::Page { path, page: d.file_page }
            } else {
                ff_engine::StampSource::Image { path }
            }
        }
        "color" => ff_engine::StampSource::Color { color: d.color },
        other => return Err(format!("nguồn không hợp lệ: {other}")),
    };
    Ok(ff_engine::WatermarkSpec {
        source,
        rotation_deg: d.rotation_deg,
        opacity: d.opacity.clamp(0.0, 1.0),
        scale: d.scale.max(0.01),
        relative_scale: d.relative_scale.filter(|v| *v > 0.0),
        anchor: parse_anchor(&d.anchor)?,
        offset_x: d.offset_x,
        offset_y: d.offset_y,
        behind: d.behind,
        replace_existing: d.replace_existing,
        pages: d.pages,
        subset: parse_subset(&d.subset),
    })
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatesDto {
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    #[serde(default = "start1u64")]
    start: u64,
    #[serde(default = "six")]
    digits: u8,
}
fn start1u64() -> u64 {
    1
}
fn six() -> u8 {
    6
}

/// Đầu/chân trang (và Bates — cùng khung 6 ô như Foxit).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HfDto {
    #[serde(default)]
    top_left: String,
    #[serde(default)]
    top_center: String,
    #[serde(default)]
    top_right: String,
    #[serde(default)]
    bottom_left: String,
    #[serde(default)]
    bottom_center: String,
    #[serde(default)]
    bottom_right: String,
    #[serde(default)]
    font: String,
    #[serde(default = "d10")]
    font_size: f32,
    #[serde(default = "black4")]
    color: [u8; 4],
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default = "m36")]
    margin_top: f32,
    #[serde(default = "m36")]
    margin_bottom: f32,
    #[serde(default = "m72")]
    margin_left: f32,
    #[serde(default = "m72")]
    margin_right: f32,
    #[serde(default)]
    date: String,
    #[serde(default = "start1")]
    start_number: u32,
    bates: Option<BatesDto>,
    #[serde(default)]
    replace_existing: bool,
    #[serde(default)]
    pages: Vec<u16>,
    #[serde(default)]
    subset: String,
}

fn hf_spec(d: HfDto) -> ff_engine::HeaderFooterSpec {
    ff_engine::HeaderFooterSpec {
        top_left: d.top_left,
        top_center: d.top_center,
        top_right: d.top_right,
        bottom_left: d.bottom_left,
        bottom_center: d.bottom_center,
        bottom_right: d.bottom_right,
        font: parse_font(&d.font),
        font_size: d.font_size.max(1.0),
        color: d.color,
        bold: d.bold,
        italic: d.italic,
        margin_top: d.margin_top,
        margin_bottom: d.margin_bottom,
        margin_left: d.margin_left,
        margin_right: d.margin_right,
        date: d.date,
        start_number: d.start_number,
        bates: d.bates.map(|b| ff_engine::BatesFormat {
            prefix: b.prefix,
            suffix: b.suffix,
            start: b.start,
            digits: b.digits.min(15),
        }),
        replace_existing: d.replace_existing,
        pages: d.pages,
        subset: parse_subset(&d.subset),
    }
}

/// Chạy việc nặng trên thread nền.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn temp_preview(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("ff_pagex_{tag}_{}_{}.pdf", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// Dựng bản xem trước 1 trang ra tệp tạm rồi render thành data URL PNG.
fn render_preview(
    input: &str,
    width: u32,
    tag: &str,
    job: impl FnOnce(&pdfium_render::prelude::Pdfium, &Path, &Path) -> Result<(), ff_engine::EngineError>,
) -> Result<String, String> {
    let pdfium = crate::pdfium()?;
    let tmp = temp_preview(tag);
    let r = job(&pdfium, Path::new(input), &tmp).map_err(|e| e.to_string());
    let out = r.and_then(|_| crate::render_temp_page(&pdfium, &tmp, 0, width));
    let _ = std::fs::remove_file(&tmp);
    out
}

/// Thêm hình mờ (`kind` = "watermark") hoặc nền ("background").
#[tauri::command]
pub async fn pagex_stamp_add(
    input: String,
    kind: String,
    spec: StampDto,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let ws = stamp_spec(spec)?;
        let (i, o) = (Path::new(&input), Path::new(&output));
        let r = if kind == "background" {
            ff_engine::add_background(&pdfium, i, &ws, o, password.as_deref())
        } else {
            ff_engine::add_watermark(&pdfium, i, &ws, o, password.as_deref())
        };
        r.map_err(|e| e.to_string())
    })
    .await
}

/// Xem trước hình mờ / nền trên trang `page` (không sửa `input`), trả data URL PNG.
#[tauri::command]
pub async fn pagex_stamp_preview(
    input: String,
    kind: String,
    page: u16,
    spec: StampDto,
    width: u32,
    password: Option<String>,
) -> Result<String, String> {
    blocking(move || {
        let ws = stamp_spec(spec)?;
        let pw = password;
        render_preview(&input, width, "stamp", move |pdfium, i, tmp| {
            let job = if kind == "background" {
                ff_engine::PageMarkJob::Background(&ws)
            } else {
                ff_engine::PageMarkJob::Watermark(&ws)
            };
            ff_engine::preview_page_mark(pdfium, i, page, job, tmp, pw.as_deref())
        })
    })
    .await
}

/// Thêm đầu & chân trang.
#[tauri::command]
pub async fn pagex_hf_add(input: String, spec: HfDto, output: String, password: Option<String>) -> Result<(), String> {
    blocking(move || {
        let pdfium = crate::pdfium()?;
        ff_engine::add_header_footer(&pdfium, Path::new(&input), &hf_spec(spec), Path::new(&output), password.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Xem trước đầu & chân trang / Bates trên trang `page`.
#[tauri::command]
pub async fn pagex_hf_preview(
    input: String,
    page: u16,
    spec: HfDto,
    width: u32,
    password: Option<String>,
) -> Result<String, String> {
    blocking(move || {
        let hf = hf_spec(spec);
        let pw = password;
        render_preview(&input, width, "hf", move |pdfium, i, tmp| {
            ff_engine::preview_page_mark(pdfium, i, page, ff_engine::PageMarkJob::HeaderFooter(&hf), tmp, pw.as_deref())
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatesJobDto {
    /// Tệp đọc vào (có thể là bản tạm đã dựng theo trạng thái tổ chức trang).
    input: String,
    /// Tên gốc hiển thị (đặt tên tệp đầu ra = tên gốc + hậu tố).
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatesRangeDto {
    output: String,
    first: String,
    last: String,
    pages: u32,
}

/// Đánh số Bates cho danh sách tệp theo thứ tự (số nối tiếp qua các tệp).
/// Một tệp + `output` → ghi đúng `output`; nhiều tệp → ghi vào `out_dir`,
/// tên = tên gốc + `name_suffix` + ".pdf" (không bao giờ ghi đè tệp nguồn).
#[tauri::command]
pub async fn pagex_bates_add(
    jobs: Vec<BatesJobDto>,
    spec: HfDto,
    output: Option<String>,
    out_dir: Option<String>,
    name_suffix: String,
) -> Result<Vec<BatesRangeDto>, String> {
    blocking(move || {
        if jobs.is_empty() {
            return Err("danh sách tệp trống".into());
        }
        let mut pairs = Vec::new();
        for (k, j) in jobs.iter().enumerate() {
            let out = match (&output, &out_dir) {
                (Some(o), _) if jobs.len() == 1 => PathBuf::from(o),
                (_, Some(dir)) => {
                    let stem = Path::new(&j.name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| format!("file{}", k + 1));
                    Path::new(dir).join(format!("{stem}{name_suffix}.pdf"))
                }
                _ => return Err("chưa chọn nơi lưu".into()),
            };
            let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
                (Ok(x), Ok(y)) => x == y,
                _ => false,
            };
            if jobs.iter().any(|x| same(Path::new(&x.input), &out) || same(Path::new(&x.name), &out)) {
                return Err(format!("tệp đầu ra trùng tệp nguồn: {}", out.display()));
            }
            pairs.push((PathBuf::from(&j.input), out));
        }
        let pdfium = crate::pdfium()?;
        let ranges = ff_engine::add_bates(&pdfium, &pairs, &hf_spec(spec)).map_err(|e| e.to_string())?;
        Ok(ranges
            .into_iter()
            .map(|r| BatesRangeDto {
                output: r.output.to_string_lossy().into_owned(),
                first: r.first,
                last: r.last,
                pages: r.pages,
            })
            .collect())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkCountsDto {
    watermark: u32,
    header_footer: u32,
    background: u32,
    bates: u32,
}

/// Đếm số trang có từng loại dấu (để báo "không có…" trước khi gỡ).
#[tauri::command]
pub async fn pagex_marks_scan(input: String, password: Option<String>) -> Result<MarkCountsDto, String> {
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let c = ff_engine::scan_page_marks(&pdfium, Path::new(&input), password.as_deref()).map_err(|e| e.to_string())?;
        Ok(MarkCountsDto { watermark: c.watermark, header_footer: c.header_footer, background: c.background, bates: c.bates })
    })
    .await
}

/// Gỡ dấu trang loại `kind` ("watermark" | "background" | "headerFooter" | "bates").
#[tauri::command]
pub async fn pagex_marks_remove(
    input: String,
    kind: String,
    output: String,
    password: Option<String>,
) -> Result<usize, String> {
    blocking(move || {
        let kinds = parse_kind(&kind)?;
        let pdfium = crate::pdfium()?;
        ff_engine::remove_page_marks(&pdfium, Path::new(&input), &kinds, Path::new(&output), password.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Chọn nhiều tệp PDF (danh sách Bates).
#[tauri::command]
pub fn pagex_pick_pdfs(app: tauri::AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .blocking_pick_files()
        .map(|v| v.into_iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Chọn tệp nguồn cho hình mờ / nền: ảnh hoặc PDF.
#[tauri::command]
pub fn pagex_pick_stamp_file(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PDF / Image", &["pdf", "png", "jpg", "jpeg", "bmp", "gif", "webp"])
        .blocking_pick_file()
        .map(|fp| fp.to_string())
}
