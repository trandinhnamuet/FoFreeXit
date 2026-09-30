//! Lệnh Tauri cho Form mức Foxit (điền trực tiếp trên trang, tạo/sửa field,
//! nhận diện field, reset, nhập/xuất FDF/XFDF/XML/CSV/TXT).

use std::path::{Path, PathBuf};

use ff_engine::{ButtonAction, FieldEdit, FieldKind, FieldSpec, FillValue};
use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

/// lopdf cần trailer chuẩn — file lạ thì chuẩn hoá qua qpdf ra file tạm.
fn for_lopdf(input: &str) -> Result<(PathBuf, Option<PathBuf>), String> {
    let p = PathBuf::from(input);
    if lopdf::Document::load(&p).is_ok() {
        return Ok((p, None));
    }
    let norm = std::env::temp_dir().join(format!(
        "ff_formx_{}_{}.pdf",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    ff_engine::repair(&p, &norm).map_err(|e| e.to_string())?;
    Ok((norm.clone(), Some(norm)))
}

fn with_lopdf<T>(input: &str, f: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
    let (src, tmp) = for_lopdf(input)?;
    let r = f(&src);
    if let Some(t) = tmp {
        let _ = std::fs::remove_file(t);
    }
    r
}

/// Chạy việc nặng ngoài luồng chính (lệnh sync của Tauri chạy trên main thread → đơ UI).
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn kind_str(w: &ff_engine::WidgetInfo) -> String {
    if w.kind == FieldKind::Text && !w.date_format.is_empty() {
        "date".into()
    } else {
        w.kind.as_str().into()
    }
}

fn kind_of(s: &str) -> FieldKind {
    match s {
        "checkbox" => FieldKind::Checkbox,
        "radio" => FieldKind::Radio,
        "combo" => FieldKind::Combo,
        "list" => FieldKind::List,
        "button" => FieldKind::Button,
        "signature" => FieldKind::Signature,
        _ => FieldKind::Text,
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ActionDto {
    kind: String,
    target: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDto {
    id: String,
    field_id: String,
    name: String,
    kind: String,
    page_index: u16,
    rect: [f32; 4],
    value: String,
    values: Vec<String>,
    default_value: String,
    export_value: String,
    checked: bool,
    options: Vec<String>,
    option_exports: Vec<String>,
    read_only: bool,
    required: bool,
    multiline: bool,
    password: bool,
    comb: bool,
    do_not_scroll: bool,
    multi_select: bool,
    editable: bool,
    max_len: u32,
    align: u8,
    font_size: f32,
    text_color: [f32; 3],
    border_color: Option<[f32; 3]>,
    fill_color: Option<[f32; 3]>,
    border_width: f32,
    border_style: String,
    tooltip: String,
    date_format: String,
    caption: String,
    check_style: String,
    action: ActionDto,
    hidden: bool,
    tab_order: u32,
}

/// Mọi widget form của tài liệu (theo thứ tự Tab) — UI dựng ô điền trên trang.
#[tauri::command]
pub async fn formx_widgets(path: String) -> Result<Vec<WidgetDto>, String> {
    // Ngoài luồng UI: lopdf nạp cả tài liệu (file lớn mất vài trăm ms).
    let list = blocking(move || with_lopdf(&path, |p| ff_engine::list_widgets(p).map_err(|e| e.to_string()))).await?;
    Ok(list
        .into_iter()
        .map(|w| WidgetDto {
            kind: kind_str(&w),
            id: w.id,
            field_id: w.field_id,
            name: w.name,
            page_index: w.page_index,
            rect: w.rect,
            value: w.value,
            values: w.values,
            default_value: w.default_value,
            export_value: w.export_value,
            checked: w.checked,
            options: w.options,
            option_exports: w.option_exports,
            read_only: w.read_only,
            required: w.required,
            multiline: w.multiline,
            password: w.password,
            comb: w.comb,
            do_not_scroll: w.do_not_scroll,
            multi_select: w.multi_select,
            editable: w.editable,
            max_len: w.max_len,
            align: w.align,
            font_size: w.font_size,
            text_color: w.text_color,
            border_color: w.border_color,
            fill_color: w.fill_color,
            border_width: w.border_width,
            border_style: w.border_style,
            tooltip: w.tooltip,
            date_format: w.date_format,
            caption: w.caption,
            check_style: w.check_style,
            action: ActionDto { kind: w.action.kind, target: w.action.target },
            hidden: w.hidden,
            tab_order: w.tab_order,
        })
        .collect())
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct SpecDto {
    name: String,
    kind: String,
    page_index: u16,
    rect: [f32; 4],
    tooltip: String,
    required: bool,
    read_only: bool,
    value: String,
    values: Vec<String>,
    font_size: f32,
    align: u8,
    text_color: Option<[f32; 3]>,
    border_color: Option<[f32; 3]>,
    fill_color: Option<[f32; 3]>,
    border_width: Option<f32>,
    border_style: String,
    options: Vec<String>,
    option_exports: Vec<String>,
    export_value: String,
    checked: bool,
    check_style: String,
    multiline: bool,
    max_len: u32,
    comb: bool,
    password: bool,
    do_not_scroll: bool,
    multi_select: bool,
    editable: bool,
    caption: String,
    action: ActionDto,
    date_format: String,
}

fn to_spec(d: SpecDto) -> FieldSpec {
    let date = d.kind == "date";
    FieldSpec {
        name: d.name,
        kind: kind_of(&d.kind),
        page_index: d.page_index,
        rect: d.rect,
        tooltip: d.tooltip,
        required: d.required,
        read_only: d.read_only,
        value: d.value,
        values: d.values,
        font_size: d.font_size.max(0.0),
        align: d.align.min(2),
        text_color: d.text_color.unwrap_or([0.0, 0.0, 0.0]),
        border_color: d.border_color,
        fill_color: d.fill_color,
        border_width: d.border_width.unwrap_or(1.0),
        border_style: if d.border_style.is_empty() { "solid".into() } else { d.border_style },
        options: d.options,
        option_exports: d.option_exports,
        export_value: d.export_value,
        checked: d.checked,
        check_style: d.check_style,
        multiline: d.multiline,
        max_len: d.max_len,
        comb: d.comb,
        password: d.password,
        do_not_scroll: d.do_not_scroll,
        multi_select: d.multi_select,
        editable: d.editable,
        caption: d.caption,
        action: ButtonAction { kind: d.action.kind, target: d.action.target },
        date_format: if date && d.date_format.trim().is_empty() { "dd/mm/yyyy".into() } else { d.date_format },
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EditDto {
    widget_id: String,
    delete: bool,
    rect: Option<[f32; 4]>,
    props: Option<SpecDto>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FillDto {
    name: String,
    value: String,
    values: Vec<String>,
}

fn to_fills(v: Vec<FillDto>) -> Vec<FillValue> {
    v.into_iter().map(|f| FillValue { name: f.name, value: f.value, values: f.values }).collect()
}

/// Áp một lượt thay đổi form: tạo field mới, sửa/di chuyển/xoá field, điền giá trị.
#[tauri::command]
pub async fn formx_apply(
    input: String,
    creates: Vec<SpecDto>,
    edits: Vec<EditDto>,
    fills: Vec<FillDto>,
    output: String,
) -> Result<usize, String> {
    let creates: Vec<FieldSpec> = creates.into_iter().map(to_spec).collect();
    let edits: Vec<FieldEdit> = edits
        .into_iter()
        .map(|e| FieldEdit { widget_id: e.widget_id, delete: e.delete, rect: e.rect, props: e.props.map(to_spec) })
        .collect();
    let fills = to_fills(fills);
    blocking(move || {
        with_lopdf(&input, |src| {
            ff_engine::apply_form_changes(src, &creates, &edits, &fills, Path::new(&output)).map_err(|e| e.to_string())
        })
    })
    .await
}

/// Reset form về giá trị mặc định (`clear` = xoá trắng). `names` rỗng = mọi field.
#[tauri::command]
pub async fn formx_reset(input: String, output: String, names: Vec<String>, clear: bool) -> Result<usize, String> {
    blocking(move || {
        with_lopdf(&input, |src| ff_engine::reset_form(src, Path::new(&output), &names, clear).map_err(|e| e.to_string()))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDto {
    page_index: u16,
    kind: String,
    rect: [f32; 4],
    name: String,
    tooltip: String,
}

/// Nhận diện field tự động (đề xuất để người dùng duyệt).
#[tauri::command]
pub async fn formx_recognize(path: String, pages: Option<Vec<u16>>) -> Result<Vec<ProposalDto>, String> {
    // Chạy ngoài luồng UI: tài liệu nhiều trang mất vài giây.
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let props = ff_engine::recognize_fields(&pdfium, Path::new(&path), &pages.unwrap_or_default())
            .map_err(|e| e.to_string())?;
        Ok(props
            .into_iter()
            .map(|p| ProposalDto { page_index: p.page_index, kind: p.kind, rect: p.rect, name: p.name, tooltip: p.tooltip })
            .collect())
    })
    .await
}

/// Xuất dữ liệu form theo đuôi file (.fdf/.xfdf/.xml/.csv/.txt).
#[tauri::command]
pub async fn formx_export(input: String, output: String) -> Result<(), String> {
    blocking(move || with_lopdf(&input, |src| ff_engine::export_form_data(src, Path::new(&output)).map_err(|e| e.to_string())))
        .await
}

/// Nhập dữ liệu form (FDF/XFDF/XML/CSV/TXT) → ghi PDF mới.
#[tauri::command]
pub async fn formx_import(input: String, data: String, output: String) -> Result<usize, String> {
    blocking(move || {
        with_lopdf(&input, |src| {
            ff_engine::import_form_data(src, Path::new(&data), Path::new(&output)).map_err(|e| e.to_string())
        })
    })
    .await
}

/// Đọc file dữ liệu form → danh sách giá trị (để nạp vào ô đang điền, chưa ghi file).
#[tauri::command]
pub fn formx_read_data(data: String) -> Result<Vec<(String, String, Vec<String>)>, String> {
    let v = ff_engine::read_form_data(Path::new(&data)).map_err(|e| e.to_string())?;
    Ok(v.into_iter().map(|f| (f.name, f.value, f.values)).collect())
}

/// Hộp thoại lưu file dữ liệu theo định dạng (`kind` = fdf|xfdf|xml|csv|txt).
#[tauri::command]
pub fn formx_pick_export(app: tauri::AppHandle, kind: String, base: Option<String>) -> Option<String> {
    let ext = match kind.as_str() {
        "xfdf" | "xml" | "csv" | "txt" => kind.clone(),
        _ => "fdf".into(),
    };
    let name = format!("{}.{}", base.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "form-data".into()), ext);
    app.dialog()
        .file()
        .add_filter(ext.to_uppercase(), &[ext.as_str()])
        .set_file_name(&name)
        .blocking_save_file()
        .map(|fp| {
            let s = fp.to_string();
            if s.to_lowercase().ends_with(&format!(".{ext}")) { s } else { format!("{s}.{ext}") }
        })
}

/// Hộp thoại chọn file dữ liệu form để nhập.
#[tauri::command]
pub fn formx_pick_import(app: tauri::AppHandle, kind: Option<String>) -> Option<String> {
    let exts: Vec<&str> = match kind.as_deref() {
        Some("fdf") => vec!["fdf"],
        Some("xfdf") => vec!["xfdf", "xml"],
        Some("csv") => vec!["csv"],
        Some("txt") => vec!["txt"],
        _ => vec!["fdf", "xfdf", "xml", "csv", "txt"],
    };
    app.dialog()
        .file()
        .add_filter("FDF / XFDF / XML / CSV / TXT", &exts)
        .blocking_pick_file()
        .map(|fp| fp.to_string())
}

/// Mở URL của nút bấm (action URI) bằng trình duyệt mặc định — chỉ http(s)/mailto.
#[tauri::command]
pub fn formx_open_url(url: String) -> Result<(), String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")) {
        return Err(format!("URL không được hỗ trợ: {u}"));
    }
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", u]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(u).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(u).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}
