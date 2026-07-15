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

/// Hộp thoại chọn thư mục (dùng cho Tách file — chọn nơi lưu các phần).
#[tauri::command]
fn pick_dir(app: tauri::AppHandle) -> Option<String> {
    app.dialog().file().blocking_pick_folder().map(|fp| fp.to_string())
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
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

// ==================== Phase 3: Tổ chức trang ====================

/// Nếu PDFium không mở thẳng được `path` (file hỏng xref/trailer), dùng QPDF
/// repair rồi trả về đường dẫn file tạm đã sửa — frontend mở lại bằng path đó.
/// Trả về chính `path` nếu mở thẳng được (không cần repair).
#[tauri::command]
fn ensure_openable(path: String, password: Option<String>) -> Result<String, String> {
    let pdfium = pdfium()?;
    let usable = ff_engine::ensure_openable(&pdfium, std::path::Path::new(&path), password.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(usable.to_string_lossy().into_owned())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagePlanEntryDto {
    /// "existing" | "blank"
    kind: String,
    source: Option<String>,
    src_index: Option<u16>,
    width_pt: Option<f32>,
    height_pt: Option<f32>,
    #[serde(default)]
    rotation_delta: i32,
    crop: Option<RectDto>,
}

fn plan_entry_from_dto(d: PagePlanEntryDto) -> Result<ff_engine::PagePlanEntry, String> {
    let page = match d.kind.as_str() {
        "existing" => ff_engine::PageSource::Existing {
            source: d.source.map(PathBuf::from),
            src_index: d.src_index.ok_or("thiếu srcIndex cho khe 'existing'")?,
        },
        "blank" => ff_engine::PageSource::Blank {
            width_pt: d.width_pt.ok_or("thiếu widthPt cho khe 'blank'")?,
            height_pt: d.height_pt.ok_or("thiếu heightPt cho khe 'blank'")?,
        },
        other => return Err(format!("loại khe trang không hỗ trợ: {other}")),
    };
    Ok(ff_engine::PagePlanEntry {
        page,
        rotation_delta: d.rotation_delta,
        crop: d.crop.map(|c| ff_engine::Rect { left: c.left, bottom: c.bottom, right: c.right, top: c.top }),
    })
}

fn plan_entry_to_dto(e: &ff_engine::PagePlanEntry) -> PagePlanEntryDto {
    let (kind, source, src_index, width_pt, height_pt) = match &e.page {
        ff_engine::PageSource::Existing { source, src_index } => {
            ("existing".to_string(), source.as_ref().map(|p| p.to_string_lossy().into_owned()), Some(*src_index), None, None)
        }
        ff_engine::PageSource::Blank { width_pt, height_pt } => {
            ("blank".to_string(), None, None, Some(*width_pt), Some(*height_pt))
        }
    };
    PagePlanEntryDto {
        kind,
        source,
        src_index,
        width_pt,
        height_pt,
        rotation_delta: e.rotation_delta,
        crop: e.crop.map(|c| RectDto { left: c.left, bottom: c.bottom, right: c.right, top: c.top }),
    }
}

/// Plan "giữ nguyên" — điểm khởi đầu cho UI Tổ chức trang khi mở 1 file.
#[tauri::command]
fn organize_identity_plan(path: String, password: Option<String>) -> Result<Vec<PagePlanEntryDto>, String> {
    let pdfium = pdfium()?;
    let plan = ff_engine::identity_plan(&pdfium, std::path::Path::new(&path), password.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(plan.iter().map(plan_entry_to_dto).collect())
}

/// Áp dụng 1 plan trang đầy đủ (chèn/xoá/xoay/đảo/thay/crop) — dựng tài liệu
/// mới ghi ra `output`. `mainInput` dùng cho các khe `source: null`.
#[tauri::command]
fn organize_apply(
    main_input: String,
    plan: Vec<PagePlanEntryDto>,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    let pdfium = pdfium()?;
    let mapped: Vec<ff_engine::PagePlanEntry> =
        plan.into_iter().map(plan_entry_from_dto).collect::<Result<_, String>>()?;
    ff_engine::build_document(
        &pdfium,
        std::path::Path::new(&main_input),
        &mapped,
        std::path::Path::new(&output),
        password.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Trích các trang `pages` (0-based, giữ thứ tự truyền vào) ra file mới.
#[tauri::command]
fn organize_extract(
    input: String,
    pages: Vec<u16>,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    let pdfium = pdfium()?;
    ff_engine::extract_pages(
        &pdfium,
        std::path::Path::new(&input),
        &pages,
        std::path::Path::new(&output),
        password.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Trộn nhiều file PDF thành 1, theo đúng thứ tự `files`.
#[tauri::command]
fn organize_merge(files: Vec<String>, output: String) -> Result<(), String> {
    let pdfium = pdfium()?;
    let paths: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    ff_engine::merge_files(&pdfium, &paths, std::path::Path::new(&output)).map_err(|e| e.to_string())
}

/// Tách `input` thành nhiều file, mỗi file tối đa `pages_per_file` trang.
/// Trả về danh sách đường dẫn đã ghi.
#[tauri::command]
fn organize_split(
    input: String,
    pages_per_file: u16,
    out_dir: String,
    base_name: String,
    password: Option<String>,
) -> Result<Vec<String>, String> {
    let pdfium = pdfium()?;
    let outputs = ff_engine::split_by_page_count(
        &pdfium,
        std::path::Path::new(&input),
        pages_per_file,
        std::path::Path::new(&out_dir),
        &base_name,
        password.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    Ok(outputs.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

/// Dựng tạm tài liệu theo `plan` HIỆN TẠI (chưa lưu) ra 1 file tạm — dùng làm
/// "input" cho Watermark/Header-Footer khi người dùng đang có thay đổi tổ
/// chức trang (chèn/xoá/đảo/xoay/crop) chưa lưu, để 2 thao tác đó thấy đúng
/// trạng thái đang xem trên lưới, không phải file gốc còn trên đĩa.
#[tauri::command]
fn organize_materialize(
    main_input: String,
    plan: Vec<PagePlanEntryDto>,
    password: Option<String>,
) -> Result<String, String> {
    let pdfium = pdfium()?;
    let mapped: Vec<ff_engine::PagePlanEntry> =
        plan.into_iter().map(plan_entry_from_dto).collect::<Result<_, String>>()?;
    let tmp = std::env::temp_dir().join(format!("ff_materialized_{}.pdf", std::process::id()));
    ff_engine::build_document(
        &pdfium,
        std::path::Path::new(&main_input),
        &mapped,
        &tmp,
        password.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    Ok(tmp.to_string_lossy().into_owned())
}

fn parse_anchor(s: &str) -> Result<ff_engine::Anchor, String> {
    use ff_engine::Anchor::*;
    Ok(match s {
        "top-left" => TopLeft,
        "top-center" => TopCenter,
        "top-right" => TopRight,
        "middle-left" => MiddleLeft,
        "center" => Center,
        "middle-right" => MiddleRight,
        "bottom-left" => BottomLeft,
        "bottom-center" => BottomCenter,
        "bottom-right" => BottomRight,
        other => return Err(format!("vị trí neo không hợp lệ: {other}")),
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatermarkDto {
    text: String,
    font_size: f32,
    color: [u8; 4],
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    rotation_deg: f32,
    anchor: String,
    #[serde(default)]
    pages: Vec<u16>,
}

/// Thêm watermark văn bản vào `input`, ghi ra `output`.
#[tauri::command]
fn watermark_add(
    input: String,
    spec: WatermarkDto,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    let pdfium = pdfium()?;
    let anchor = parse_anchor(&spec.anchor)?;
    let ws = ff_engine::WatermarkSpec {
        text: spec.text,
        font_size: spec.font_size,
        color: spec.color,
        bold: spec.bold,
        italic: spec.italic,
        rotation_deg: spec.rotation_deg,
        anchor,
        pages: spec.pages,
    };
    ff_engine::add_watermark(&pdfium, std::path::Path::new(&input), &ws, std::path::Path::new(&output), password.as_deref())
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HeaderFooterDto {
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
    font_size: f32,
    color: [u8; 4],
    margin_pt: f32,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    date: String,
    #[serde(default)]
    pages: Vec<u16>,
}

/// Thêm header/footer (gồm đánh số trang qua token {page}/{total}) vào `input`.
#[tauri::command]
fn header_footer_add(
    input: String,
    spec: HeaderFooterDto,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    let pdfium = pdfium()?;
    let hf = ff_engine::HeaderFooterSpec {
        top_left: spec.top_left,
        top_center: spec.top_center,
        top_right: spec.top_right,
        bottom_left: spec.bottom_left,
        bottom_center: spec.bottom_center,
        bottom_right: spec.bottom_right,
        font_size: spec.font_size,
        color: spec.color,
        margin_pt: spec.margin_pt,
        bold: spec.bold,
        italic: spec.italic,
        date: spec.date,
        pages: spec.pages,
    };
    ff_engine::add_header_footer(&pdfium, std::path::Path::new(&input), &hf, std::path::Path::new(&output), password.as_deref())
        .map_err(|e| e.to_string())
}

fn render_temp_page(pdfium: &pdfium_render::prelude::Pdfium, path: &std::path::Path, page: u16, width: u32) -> Result<String, String> {
    let rendered = ff_engine::render::render_page(pdfium, path, page, width, None).map_err(|e| e.to_string())?;
    let mut buf = Cursor::new(Vec::new());
    rendered.image.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Xem trước watermark trên 1 trang: áp vào file tạm rồi render, không sửa `input`.
#[tauri::command]
fn preview_watermark(input: String, page: u16, spec: WatermarkDto, width: u32) -> Result<String, String> {
    let pdfium = pdfium()?;
    let anchor = parse_anchor(&spec.anchor)?;
    let ws = ff_engine::WatermarkSpec {
        text: spec.text, font_size: spec.font_size, color: spec.color,
        bold: spec.bold, italic: spec.italic, rotation_deg: spec.rotation_deg,
        anchor, pages: vec![page],
    };
    let tmp = std::env::temp_dir().join(format!("ff_preview_wm_{}.pdf", std::process::id()));
    ff_engine::add_watermark(&pdfium, std::path::Path::new(&input), &ws, &tmp, None).map_err(|e| e.to_string())?;
    let result = render_temp_page(&pdfium, &tmp, page, width);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Xem trước header/footer trên 1 trang: áp vào file tạm rồi render, không sửa `input`.
#[tauri::command]
fn preview_header_footer(input: String, page: u16, spec: HeaderFooterDto, width: u32) -> Result<String, String> {
    let pdfium = pdfium()?;
    let hf = ff_engine::HeaderFooterSpec {
        top_left: spec.top_left, top_center: spec.top_center, top_right: spec.top_right,
        bottom_left: spec.bottom_left, bottom_center: spec.bottom_center, bottom_right: spec.bottom_right,
        font_size: spec.font_size, color: spec.color, margin_pt: spec.margin_pt,
        bold: spec.bold, italic: spec.italic, date: spec.date, pages: vec![page],
    };
    let tmp = std::env::temp_dir().join(format!("ff_preview_hf_{}.pdf", std::process::id()));
    ff_engine::add_header_footer(&pdfium, std::path::Path::new(&input), &hf, &tmp, None).map_err(|e| e.to_string())?;
    let result = render_temp_page(&pdfium, &tmp, page, width);
    let _ = std::fs::remove_file(&tmp);
    result
}

// ---------- Phase 4: Sửa nội dung (Edit) ----------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectInfoDto {
    index: u16,
    kind: String,
    rect: RectDto,
    text: Option<String>,
    font_name: Option<String>,
    font_family: Option<String>,
    font_bold: Option<bool>,
    font_italic: Option<bool>,
    font_embedded: Option<bool>,
    font_size: Option<f32>,
    color: Option<[u8; 4]>,
}

/// Liệt kê page object của 1 trang để UI vẽ overlay chỉnh sửa.
#[tauri::command]
fn edit_list_objects(path: String, page: u16, password: Option<String>) -> Result<Vec<ObjectInfoDto>, String> {
    let pdfium = pdfium()?;
    let objs = ff_engine::list_objects(&pdfium, std::path::Path::new(&path), page, password.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(objs
        .into_iter()
        .map(|o| ObjectInfoDto {
            index: o.index,
            kind: o.kind.as_str().to_string(),
            rect: RectDto { left: o.rect.left, bottom: o.rect.bottom, right: o.rect.right, top: o.rect.top },
            text: o.text,
            font_name: o.font_name,
            font_family: o.font_family,
            font_bold: o.font_bold,
            font_italic: o.font_italic,
            font_embedded: o.font_embedded,
            font_size: o.font_size,
            color: o.color,
        })
        .collect())
}

/// 1 thao tác sửa nội dung (tagged theo field `op`). Field thừa cho mỗi loại để None/mặc định.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditOpDto {
    /// "transform" | "setText" | "delete" | "replaceImage" | "addText" | "addImage"
    op: String,
    #[serde(default)]
    index: u16,
    #[serde(default)]
    dx: f32,
    #[serde(default)]
    dy: f32,
    #[serde(default = "one")]
    sx: f32,
    #[serde(default = "one")]
    sy: f32,
    #[serde(default)]
    text: String,
    font_size: Option<f32>,
    color: Option<[u8; 4]>,
    /// setText/addText: None = giữ font gốc; Some = đổi sang family này.
    font_family: Option<String>,
    /// setText: None = giữ kiểu gốc. addText: None ≙ false.
    bold: Option<bool>,
    italic: Option<bool>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    width_pt: f32,
    #[serde(default)]
    height_pt: f32,
    #[serde(default)]
    image_path: String,
    /// reflowText: index các run của khối đoạn văn.
    #[serde(default)]
    indices: Vec<u16>,
}

fn one() -> f32 {
    1.0
}

fn edit_op_from_dto(d: EditOpDto) -> Result<ff_engine::EditOp, String> {
    Ok(match d.op.as_str() {
        "transform" => ff_engine::EditOp::Transform { index: d.index, dx: d.dx, dy: d.dy, sx: d.sx, sy: d.sy },
        "setText" => ff_engine::EditOp::SetText {
            index: d.index,
            text: d.text,
            font_size: d.font_size,
            color: d.color,
            font_family: d.font_family,
            bold: d.bold,
            italic: d.italic,
        },
        "delete" => ff_engine::EditOp::Delete { index: d.index },
        "replaceImage" => ff_engine::EditOp::ReplaceImage { index: d.index, image_path: d.image_path },
        "addText" => ff_engine::EditOp::AddText {
            x: d.x,
            y: d.y,
            text: d.text,
            font_size: d.font_size.unwrap_or(14.0),
            color: d.color.unwrap_or([0, 0, 0, 255]),
            font_family: d.font_family,
            bold: d.bold.unwrap_or(false),
            italic: d.italic.unwrap_or(false),
        },
        "reflowText" => ff_engine::EditOp::ReflowText { indices: d.indices, text: d.text },
        "addImage" => ff_engine::EditOp::AddImage {
            x: d.x,
            y: d.y,
            width_pt: d.width_pt,
            height_pt: d.height_pt,
            image_path: d.image_path,
        },
        other => return Err(format!("op sửa nội dung không hỗ trợ: {other}")),
    })
}

/// Áp các thao tác sửa nội dung lên trang `page`, ghi ra `output`.
#[tauri::command]
fn edit_apply(
    input: String,
    page: u16,
    ops: Vec<EditOpDto>,
    output: String,
    password: Option<String>,
) -> Result<(), String> {
    let pdfium = pdfium()?;
    let mapped: Vec<ff_engine::EditOp> = ops.into_iter().map(edit_op_from_dto).collect::<Result<_, String>>()?;
    ff_engine::apply_edits(
        &pdfium,
        std::path::Path::new(&input),
        page,
        &mapped,
        std::path::Path::new(&output),
        password.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Áp `ops` rồi ghi ra 1 file PDF tạm MỚI (duy nhất), trả về đường dẫn — dùng
/// cho mô hình "materialize tức thì" ở UI: mỗi thao tác sửa tạo 1 bản làm việc
/// mới để đọc lại object + render WYSIWYG, và để undo quay về bản trước.
#[tauri::command]
fn edit_apply_to_temp(input: String, page: u16, ops: Vec<EditOpDto>, password: Option<String>) -> Result<String, String> {
    let pdfium = pdfium()?;
    let mapped: Vec<ff_engine::EditOp> = ops.into_iter().map(edit_op_from_dto).collect::<Result<_, String>>()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let out = std::env::temp_dir().join(format!("ff_edit_{}_{}.pdf", std::process::id(), nanos));
    ff_engine::apply_edits(
        &pdfium,
        std::path::Path::new(&input),
        page,
        &mapped,
        &out,
        password.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().into_owned())
}

/// Xem trước WYSIWYG: áp `ops` vào file tạm rồi render trang `page` ra PNG.
#[tauri::command]
fn edit_preview(input: String, page: u16, ops: Vec<EditOpDto>, width: u32, password: Option<String>) -> Result<String, String> {
    let pdfium = pdfium()?;
    let mapped: Vec<ff_engine::EditOp> = ops.into_iter().map(edit_op_from_dto).collect::<Result<_, String>>()?;
    let tmp = std::env::temp_dir().join(format!("ff_preview_edit_{}.pdf", std::process::id()));
    ff_engine::apply_edits(&pdfium, std::path::Path::new(&input), page, &mapped, &tmp, password.as_deref())
        .map_err(|e| e.to_string())?;
    let result = render_temp_page(&pdfium, &tmp, page, width);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Dọn các file làm việc tạm của chế độ sửa (undo stack). Chỉ xoá file có tên
/// `ff_edit_*` nằm đúng trong thư mục temp — không bao giờ đụng file người dùng.
#[tauri::command]
fn edit_cleanup(paths: Vec<String>) {
    let tmp = std::env::temp_dir();
    for p in paths {
        let path = std::path::PathBuf::from(&p);
        let name_ok = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("ff_edit_") && n.ends_with(".pdf"))
            .unwrap_or(false);
        let dir_ok = path.parent().map(|d| d == tmp).unwrap_or(false);
        if name_ok && dir_ok {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Hộp thoại chọn file ảnh (cho Thêm ảnh / Thay ảnh).
#[tauri::command]
fn pick_image(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Ảnh", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
        .blocking_pick_file()
        .map(|fp| fp.to_string())
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
            pick_dir,
            apply_annotations,
            default_pdf,
            initial_file,
            ensure_openable,
            organize_identity_plan,
            organize_apply,
            organize_extract,
            organize_merge,
            organize_split,
            organize_materialize,
            watermark_add,
            header_footer_add,
            preview_watermark,
            preview_header_footer,
            edit_list_objects,
            edit_apply,
            edit_apply_to_temp,
            edit_preview,
            edit_cleanup,
            pick_image
        ])
        .run(tauri::generate_context!())
        .expect("lỗi khi chạy ứng dụng Tauri");
}
