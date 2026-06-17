// Ẩn cửa sổ console trên Windows ở bản release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Cursor;
use std::path::PathBuf;

use base64::Engine as _;
use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

/// Thư mục gốc workspace (app/src-tauri -> ../../).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Đảm bảo engine tìm thấy pdfium.dll (dev: ở gốc workspace).
fn ensure_pdfium_env() {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
}

fn pdfium() -> Result<pdfium_render::prelude::Pdfium, String> {
    ensure_pdfium_env();
    ff_engine::bind_pdfium().map_err(|e| e.to_string())
}

// ---- Kiểu dữ liệu trả về frontend ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageMeta {
    index: u16,
    width_pt: f32,
    height_pt: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutlineMeta {
    title: String,
    page_index: Option<u16>,
    level: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocMeta {
    page_count: u16,
    pages: Vec<PageMeta>,
    outline: Vec<OutlineMeta>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RectMeta {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHitMeta {
    page_index: u16,
    char_start: usize,
    char_len: usize,
    rect: Option<RectMeta>,
}

// ---- Commands ----

/// Mở tài liệu: trả số trang, kích thước từng trang, và outline.
#[tauri::command]
fn open_document(path: String) -> Result<DocMeta, String> {
    let pdfium = pdfium()?;
    let p = PathBuf::from(&path);

    let dims = ff_engine::page_dims(&pdfium, &p, None).map_err(|e| e.to_string())?;
    let outline = ff_engine::outline(&pdfium, &p, None).map_err(|e| e.to_string())?;

    Ok(DocMeta {
        page_count: dims.len() as u16,
        pages: dims
            .into_iter()
            .map(|d| PageMeta {
                index: d.index,
                width_pt: d.width_pt,
                height_pt: d.height_pt,
            })
            .collect(),
        outline: outline
            .into_iter()
            .map(|o| OutlineMeta {
                title: o.title,
                page_index: o.page_index,
                level: o.level,
            })
            .collect(),
    })
}

/// Render một trang ra data URL PNG với chiều rộng `width` px.
#[tauri::command]
fn render_page(path: String, page: u16, width: u32) -> Result<String, String> {
    let pdfium = pdfium()?;
    let p = PathBuf::from(&path);
    let rendered =
        ff_engine::render::render_page(&pdfium, &p, page, width, None).map_err(|e| e.to_string())?;

    let mut buf = Cursor::new(Vec::new());
    rendered
        .image
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Tìm chuỗi trong tài liệu.
#[tauri::command]
fn search_document(
    path: String,
    query: String,
    case_sensitive: bool,
) -> Result<Vec<SearchHitMeta>, String> {
    let pdfium = pdfium()?;
    let p = PathBuf::from(&path);
    let hits =
        ff_engine::search(&pdfium, &p, &query, case_sensitive, None).map_err(|e| e.to_string())?;
    Ok(hits
        .into_iter()
        .map(|h| SearchHitMeta {
            page_index: h.page_index,
            char_start: h.char_start,
            char_len: h.char_len,
            rect: h.rect.map(|r| RectMeta {
                left: r.left,
                bottom: r.bottom,
                right: r.right,
                top: r.top,
            }),
        })
        .collect())
}

/// Lấy text của một trang (cho chức năng copy).
#[tauri::command]
fn page_text(path: String, page: u16) -> Result<String, String> {
    let pdfium = pdfium()?;
    let p = PathBuf::from(&path);
    ff_engine::extract_text(&pdfium, &p, page, None).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CharBoxMeta {
    ch: String,
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

/// Hộp bao từng ký tự của một trang (dựng text-layer để chọn & copy).
#[tauri::command]
fn page_text_layer(path: String, page: u16) -> Result<Vec<CharBoxMeta>, String> {
    let pdfium = pdfium()?;
    let p = PathBuf::from(&path);
    let boxes = ff_engine::page_char_boxes(&pdfium, &p, page, None).map_err(|e| e.to_string())?;
    Ok(boxes
        .into_iter()
        .map(|b| CharBoxMeta {
            ch: b.ch,
            left: b.left,
            bottom: b.bottom,
            right: b.right,
            top: b.top,
        })
        .collect())
}

/// Mở hộp thoại chọn file PDF; trả về đường dẫn đã chọn (hoặc None nếu huỷ).
#[tauri::command]
fn pick_pdf(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .blocking_pick_file()
        .map(|fp| fp.to_string())
}

/// Hộp thoại lưu file PDF; trả về đường dẫn (hoặc None nếu huỷ).
#[tauri::command]
fn pick_save_pdf(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_file_name("annotated.pdf")
        .blocking_save_file()
        .map(|fp| fp.to_string())
}

/// Hộp chữ nhật đơn giản (1 quad) dùng cho `AnnotSpecDto.quads`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RectDto {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

/// DTO nhận từ frontend cho một annotation.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnnotSpecDto {
    kind: String,
    page_index: u16,
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
    /// Highlight/Underline/Strikeout theo text nhiều dòng: 1 quad/dòng.
    /// Rỗng/thiếu → engine suy ra 1 quad duy nhất từ left/bottom/right/top.
    #[serde(default)]
    quads: Vec<RectDto>,
    color: [u8; 4],
    contents: Option<String>,
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    underline: bool,
}

fn default_font_size() -> f32 {
    14.0
}

/// Tạo annotation theo specs rồi lưu sang `output`.
#[tauri::command]
fn apply_annotations(
    input: String,
    output: String,
    specs: Vec<AnnotSpecDto>,
) -> Result<(), String> {
    use ff_engine::{AnnotKind, AnnotSpec, Rect};
    let pdfium = pdfium()?;
    let mut mapped = Vec::with_capacity(specs.len());
    for s in specs {
        let kind = match s.kind.as_str() {
            "highlight" => AnnotKind::Highlight,
            "underline" => AnnotKind::Underline,
            "strikeout" => AnnotKind::Strikeout,
            "square" => AnnotKind::Square,
            "freetext" => AnnotKind::FreeText,
            "note" => AnnotKind::Note,
            other => return Err(format!("loại annotation không hỗ trợ: {other}")),
        };
        mapped.push(AnnotSpec {
            kind,
            page_index: s.page_index,
            rect: Rect {
                left: s.left,
                bottom: s.bottom,
                right: s.right,
                top: s.top,
            },
            quads: s
                .quads
                .into_iter()
                .map(|q| Rect { left: q.left, bottom: q.bottom, right: q.right, top: q.top })
                .collect(),
            color: s.color,
            contents: s.contents,
            font_size: s.font_size,
            bold: s.bold,
            italic: s.italic,
            underline: s.underline,
        });
    }
    ff_engine::apply_annotations(
        &pdfium,
        std::path::Path::new(&input),
        std::path::Path::new(&output),
        &mapped,
    )
    .map_err(|e| e.to_string())
}

/// Đường dẫn file PDF mẫu để mở khi khởi động.
#[tauri::command]
fn default_pdf() -> String {
    workspace_root()
        .join("corpus")
        .join("sample-multipage.pdf")
        .to_string_lossy()
        .into_owned()
}

/// File PDF được truyền qua command-line (Explorer gọi `fofreexit-app.exe "C:\...\a.pdf"`
/// khi double-click hoặc "Open with FoFreeXit"). `None` nếu mở app trực tiếp (không kèm file).
#[tauri::command]
fn initial_file() -> Option<String> {
    let arg = std::env::args().nth(1)?;
    let path = std::path::PathBuf::from(&arg);
    let is_pdf = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false);
    if is_pdf && path.exists() {
        Some(arg)
    } else {
        None
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_document,
            render_page,
            search_document,
            page_text,
            page_text_layer,
            pick_pdf,
            pick_save_pdf,
            apply_annotations,
            default_pdf,
            initial_file
        ])
        .run(tauri::generate_context!())
        .expect("lỗi khi chạy ứng dụng Tauri");
}
