//! Lệnh Tauri cho So sánh tài liệu (Compare Files). Diff chạy ở luồng nền
//! (spawn_blocking) và báo tiến độ qua sự kiện "compare-progress"; kết quả
//! gần nhất được giữ lại để xuất báo cáo mà không phải gửi ngược từ UI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

static CANCEL: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<ff_engine::CompareResult>> = Mutex::new(None);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareOptionsDto {
    /// "text" | "visual" | "both"
    mode: String,
    #[serde(default)]
    old_pages: Option<Vec<u16>>,
    #[serde(default)]
    new_pages: Option<Vec<u16>>,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default)]
    ignore_whitespace: bool,
    #[serde(default)]
    ignore_punctuation: bool,
    #[serde(default)]
    visual_dpi: Option<f32>,
    #[serde(default)]
    old_password: Option<String>,
    #[serde(default)]
    new_password: Option<String>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RectDto {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDto {
    id: usize,
    kind: &'static str,
    old_page: Option<u16>,
    new_page: Option<u16>,
    old_rects: Vec<RectDto>,
    new_rects: Vec<RectDto>,
    old_text: String,
    new_text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSizeDto {
    width_pt: f32,
    height_pt: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePairDto {
    old: Option<u16>,
    new: Option<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryDto {
    replaced: usize,
    inserted: usize,
    deleted: usize,
    page_inserted: usize,
    page_deleted: usize,
    graphics: usize,
    total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareResultDto {
    old_path: String,
    new_path: String,
    old_pages: Vec<PageSizeDto>,
    new_pages: Vec<PageSizeDto>,
    page_map: Vec<PagePairDto>,
    changes: Vec<ChangeDto>,
    summary: SummaryDto,
    elapsed_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressDto {
    stage: &'static str,
    done: usize,
    total: usize,
}

fn rects(v: &[ff_engine::Rect]) -> Vec<RectDto> {
    v.iter().map(|r| RectDto { left: r.left, bottom: r.bottom, right: r.right, top: r.top }).collect()
}

fn to_dto(r: &ff_engine::CompareResult, elapsed_ms: u64) -> CompareResultDto {
    let sizes = |v: &[(f32, f32)]| v.iter().map(|&(w, h)| PageSizeDto { width_pt: w, height_pt: h }).collect();
    let s = &r.summary;
    CompareResultDto {
        old_path: r.old_path.to_string_lossy().into_owned(),
        new_path: r.new_path.to_string_lossy().into_owned(),
        old_pages: sizes(&r.old_sizes),
        new_pages: sizes(&r.new_sizes),
        page_map: r.page_map.iter().map(|p| PagePairDto { old: p.old, new: p.new }).collect(),
        changes: r
            .changes
            .iter()
            .map(|c| ChangeDto {
                id: c.id,
                kind: c.kind.as_str(),
                old_page: c.old_page,
                new_page: c.new_page,
                old_rects: rects(&c.old_rects),
                new_rects: rects(&c.new_rects),
                old_text: c.old_text.clone(),
                new_text: c.new_text.clone(),
            })
            .collect(),
        summary: SummaryDto {
            replaced: s.replaced,
            inserted: s.inserted,
            deleted: s.deleted,
            page_inserted: s.page_inserted,
            page_deleted: s.page_deleted,
            graphics: s.graphics,
            total: s.total(),
        },
        elapsed_ms,
    }
}

/// So sánh 2 tệp PDF ở luồng nền. Lỗi "compare-cancelled" = người dùng huỷ.
#[tauri::command]
pub async fn compare_run(
    app: tauri::AppHandle,
    old_path: String,
    new_path: String,
    options: CompareOptionsDto,
) -> Result<CompareResultDto, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let job = move || -> Result<CompareResultDto, String> {
        let pdfium = crate::pdfium()?;
        let mode = match options.mode.as_str() {
            "text" => ff_engine::CompareMode::Text,
            "visual" => ff_engine::CompareMode::Visual,
            _ => ff_engine::CompareMode::Both,
        };
        let o = ff_engine::CompareOptions {
            mode,
            old_pages: options.old_pages,
            new_pages: options.new_pages,
            ignore_case: options.ignore_case,
            ignore_whitespace: options.ignore_whitespace,
            ignore_punctuation: options.ignore_punctuation,
            visual_dpi: options.visual_dpi.unwrap_or(72.0),
            old_password: options.old_password,
            new_password: options.new_password,
            ..Default::default()
        };
        let t0 = Instant::now();
        let mut last_emit: Option<Instant> = None;
        let result = ff_engine::compare_documents(
            &pdfium,
            &PathBuf::from(&old_path),
            &PathBuf::from(&new_path),
            &o,
            &mut |p| {
                // Giới hạn ~10 sự kiện/giây — không làm nghẽn WebView.
                if last_emit.map_or(true, |t| t.elapsed() >= Duration::from_millis(100)) || p.done == p.total {
                    last_emit = Some(Instant::now());
                    let _ = app.emit("compare-progress", ProgressDto { stage: p.stage, done: p.done, total: p.total });
                }
                !CANCEL.load(Ordering::SeqCst)
            },
        )
        .map_err(|e| e.to_string())?;
        let dto = to_dto(&result, t0.elapsed().as_millis() as u64);
        *LAST.lock().map_err(|e| e.to_string())? = Some(result);
        Ok(dto)
    };
    tauri::async_runtime::spawn_blocking(job).await.map_err(|e| e.to_string())?
}

/// Huỷ phép so sánh đang chạy.
#[tauri::command]
pub fn compare_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStatsDto {
    cover_pages: usize,
    annotations: usize,
    links: usize,
}

/// Xuất báo cáo so sánh (bản sao tệp MỚI có chú thích + trang bìa tóm tắt)
/// từ kết quả so sánh gần nhất. `labels` = nhãn đã dịch theo ngôn ngữ UI.
#[tauri::command]
pub async fn compare_export_report(
    output: String,
    old_name: String,
    new_name: String,
    labels: HashMap<String, String>,
) -> Result<ReportStatsDto, String> {
    let job = move || -> Result<ReportStatsDto, String> {
        let guard = LAST.lock().map_err(|e| e.to_string())?;
        let result = guard.as_ref().ok_or_else(|| "no compare result".to_string())?;
        let out = PathBuf::from(&output);
        if out == result.new_path || out == result.old_path {
            return Err("output must differ from the compared files".into());
        }
        let d = ff_engine::ReportLabels::default();
        let g = |k: &str, def: String| labels.get(k).filter(|s| !s.is_empty()).cloned().unwrap_or(def);
        let l = ff_engine::ReportLabels {
            title: g("title", d.title),
            old_file: g("oldFile", d.old_file),
            new_file: g("newFile", d.new_file),
            date_line: g("dateLine", d.date_line),
            summary: g("summary", d.summary),
            total: g("total", d.total),
            details: g("details", d.details),
            col_no: g("colNo", d.col_no),
            col_type: g("colType", d.col_type),
            col_old_page: g("colOldPage", d.col_old_page),
            col_new_page: g("colNewPage", d.col_new_page),
            col_content: g("colContent", d.col_content),
            kind_inserted: g("inserted", d.kind_inserted),
            kind_deleted: g("deleted", d.kind_deleted),
            kind_replaced: g("replaced", d.kind_replaced),
            kind_page_inserted: g("pageInserted", d.kind_page_inserted),
            kind_page_deleted: g("pageDeleted", d.kind_page_deleted),
            kind_graphics: g("graphics", d.kind_graphics),
            more: g("more", d.more),
            no_diff: g("noDiff", d.no_diff),
            author: g("author", d.author),
        };
        let pdfium = crate::pdfium()?;
        let info = ff_engine::ReportInfo { old_name, new_name, labels: l };
        let s = ff_engine::export_compare_report(&pdfium, result, &out, &info).map_err(|e| e.to_string())?;
        Ok(ReportStatsDto { cover_pages: s.cover_pages, annotations: s.annotations, links: s.links })
    };
    tauri::async_runtime::spawn_blocking(job).await.map_err(|e| e.to_string())?
}
