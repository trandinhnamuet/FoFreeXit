//! Lệnh Tauri cho chú thích mở rộng (tab Comment kiểu Foxit): vẽ tự do,
//! con dấu, đọc/sửa/xoá chú thích có sẵn, tên tác giả.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

use ff_engine::{
    AnnotDetail, AnnotKind, AnnotMeta, AnnotRef, AnnotSaveRequest, AnnotSpec, AnnotUpdate, Rect,
    ReplySpec, ShapeKind, ShapeSpec, StampSpec,
};

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RectDto {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

impl From<RectDto> for Rect {
    fn from(r: RectDto) -> Self {
        Rect { left: r.left, bottom: r.bottom, right: r.right, top: r.top }
    }
}
impl From<Rect> for RectDto {
    fn from(r: Rect) -> Self {
        RectDto { left: r.left, bottom: r.bottom, right: r.right, top: r.top }
    }
}

/// Chi tiết 1 annotation có sẵn (danh sách Comments).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotDetailDto {
    page_index: u16,
    annot_index: usize,
    subtype: String,
    rect: RectDto,
    color: Option<[u8; 3]>,
    fill: Option<[u8; 3]>,
    width: Option<f32>,
    opacity: f32,
    contents: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    modified: Option<String>,
    created: Option<String>,
    nm: Option<String>,
    quads: Vec<RectDto>,
    vertices: Vec<[f32; 2]>,
    ink: Vec<Vec<[f32; 2]>>,
    line: Option<[f32; 4]>,
    line_endings: Option<[String; 2]>,
    font_size: Option<f32>,
    icon: Option<String>,
    in_reply_to: Option<usize>,
    hidden: bool,
    cloudy: bool,
}

impl From<AnnotDetail> for AnnotDetailDto {
    fn from(d: AnnotDetail) -> Self {
        AnnotDetailDto {
            page_index: d.page_index,
            annot_index: d.annot_index,
            subtype: d.subtype,
            rect: d.rect.into(),
            color: d.color,
            fill: d.fill,
            width: d.width,
            opacity: d.opacity,
            contents: d.contents,
            author: d.author,
            subject: d.subject,
            modified: d.modified,
            created: d.created,
            nm: d.nm,
            quads: d.quads.into_iter().map(Into::into).collect(),
            vertices: d.vertices,
            ink: d.ink,
            line: d.line,
            line_endings: d.line_endings,
            font_size: d.font_size,
            icon: d.icon,
            in_reply_to: d.in_reply_to,
            hidden: d.hidden,
            cloudy: d.cloudy,
        }
    }
}

/// Markup/Text box/Note (cùng dạng DTO với lệnh `apply_annotations` cũ).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkupDto {
    kind: String,
    page_index: u16,
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
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
fn default_one() -> f32 {
    1.0
}
fn default_width() -> f32 {
    2.0
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampDto {
    #[serde(default)]
    name: String,
    #[serde(default)]
    label: String,
    sub: Option<String>,
    image: Option<String>,
}

/// Chú thích vẽ tự do / con dấu.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeDto {
    kind: String,
    page_index: u16,
    #[serde(default)]
    left: f32,
    #[serde(default)]
    bottom: f32,
    #[serde(default)]
    right: f32,
    #[serde(default)]
    top: f32,
    #[serde(default)]
    points: Vec<[f32; 2]>,
    #[serde(default)]
    strokes: Vec<Vec<[f32; 2]>>,
    color: [u8; 3],
    fill: Option<[u8; 3]>,
    #[serde(default = "default_width")]
    width: f32,
    #[serde(default = "default_one")]
    opacity: f32,
    contents: Option<String>,
    stamp: Option<StampDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefDto {
    page_index: u16,
    annot_index: usize,
    nm: Option<String>,
}

impl From<RefDto> for AnnotRef {
    fn from(r: RefDto) -> Self {
        AnnotRef { page_index: r.page_index, annot_index: r.annot_index, nm: r.nm }
    }
}

/// Trả lời 1 chú thích có sẵn.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyDto {
    page_index: u16,
    annot_index: usize,
    nm: Option<String>,
    contents: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDto {
    page_index: u16,
    annot_index: usize,
    nm: Option<String>,
    rect: Option<RectDto>,
    color: Option<[u8; 3]>,
    /// true = đổi màu tô theo `fill` (null = bỏ tô).
    #[serde(default)]
    set_fill: bool,
    fill: Option<[u8; 3]>,
    width: Option<f32>,
    opacity: Option<f32>,
    contents: Option<String>,
    author: Option<String>,
}

fn map_markup(s: MarkupDto) -> Result<AnnotSpec, String> {
    let kind = match s.kind.as_str() {
        "highlight" => AnnotKind::Highlight,
        "underline" => AnnotKind::Underline,
        "strikeout" => AnnotKind::Strikeout,
        "square" => AnnotKind::Square,
        "freetext" => AnnotKind::FreeText,
        "note" => AnnotKind::Note,
        other => return Err(format!("loại annotation không hỗ trợ: {other}")),
    };
    Ok(AnnotSpec {
        kind,
        page_index: s.page_index,
        rect: Rect { left: s.left, bottom: s.bottom, right: s.right, top: s.top },
        quads: s.quads.into_iter().map(Into::into).collect(),
        color: s.color,
        contents: s.contents,
        font_size: s.font_size,
        bold: s.bold,
        italic: s.italic,
        underline: s.underline,
    })
}

fn map_shape(s: ShapeDto) -> Result<ShapeSpec, String> {
    let kind = match s.kind.as_str() {
        "ink" => ShapeKind::Ink,
        "line" => ShapeKind::Line,
        "arrow" => ShapeKind::Arrow,
        "square" | "rect" => ShapeKind::Square,
        "circle" | "oval" => ShapeKind::Circle,
        "polygon" => ShapeKind::Polygon,
        "polyline" => ShapeKind::PolyLine,
        "cloud" => ShapeKind::Cloud,
        "stamp" => ShapeKind::Stamp,
        other => return Err(format!("loại hình không hỗ trợ: {other}")),
    };
    Ok(ShapeSpec {
        kind,
        page_index: s.page_index,
        rect: Rect { left: s.left, bottom: s.bottom, right: s.right, top: s.top },
        points: s.points,
        strokes: s.strokes,
        color: s.color,
        fill: s.fill,
        width: s.width,
        opacity: s.opacity,
        contents: s.contents,
        stamp: s.stamp.map(|st| StampSpec {
            name: st.name,
            label: st.label,
            sub: st.sub,
            image: st.image.map(PathBuf::from),
        }),
    })
}

fn map_update(u: UpdateDto) -> AnnotUpdate {
    let mut out = AnnotUpdate::new(AnnotRef { page_index: u.page_index, annot_index: u.annot_index, nm: u.nm });
    out.rect = u.rect.map(Into::into);
    out.color = u.color;
    out.fill = if u.set_fill { Some(u.fill) } else { None };
    out.width = u.width;
    out.opacity = u.opacity;
    out.contents = u.contents;
    out.author = u.author;
    out
}

/// Danh sách chú thích có sẵn trong file (kèm chi tiết để hiển thị/sửa).
#[tauri::command]
pub fn annot_list_detailed(path: String) -> Result<Vec<AnnotDetailDto>, String> {
    let pdfium = crate::pdfium()?;
    ff_engine::list_annotations_detailed(&pdfium, Path::new(&path))
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

/// Lưu mọi thay đổi chú thích của phiên sang `output` trong 1 lần.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn annot_save(
    input: String,
    output: String,
    specs: Vec<MarkupDto>,
    shapes: Vec<ShapeDto>,
    updates: Vec<UpdateDto>,
    deletes: Vec<RefDto>,
    replies: Option<Vec<ReplyDto>>,
    author: Option<String>,
    date: Option<String>,
) -> Result<(), String> {
    // Lưu đè chính tệp đang mở: ghi ra tệp tạm cạnh đó rồi thay thế — không
    // bao giờ ghi thẳng lên tệp đang đọc.
    let same = Path::new(&input) == Path::new(&output);
    let target = if same { format!("{output}.ffsave.tmp.pdf") } else { output.clone() };
    let pdfium = crate::pdfium()?;
    let req = AnnotSaveRequest {
        specs: specs.into_iter().map(map_markup).collect::<Result<_, _>>()?,
        shapes: shapes.into_iter().map(map_shape).collect::<Result<_, _>>()?,
        updates: updates.into_iter().map(map_update).collect(),
        deletes: deletes.into_iter().map(Into::into).collect(),
        replies: replies
            .unwrap_or_default()
            .into_iter()
            .map(|r| ReplySpec {
                target: AnnotRef { page_index: r.page_index, annot_index: r.annot_index, nm: r.nm },
                contents: r.contents,
            })
            .collect(),
        meta: AnnotMeta { author, date },
    };
    ff_engine::save_annotations(&pdfium, Path::new(&input), Path::new(&target), &req).map_err(|e| {
        if same {
            let _ = std::fs::remove_file(&target);
        }
        e.to_string()
    })?;
    drop(pdfium);
    if same {
        std::fs::rename(&target, &output).map_err(|e| {
            let _ = std::fs::remove_file(&target);
            format!("không thay được tệp {output}: {e}")
        })?;
    }
    Ok(())
}

/// Render trang nhưng ẩn các annotation đang sửa/xoá trên UI (chỉ số /Annots).
#[tauri::command]
pub fn annot_render_page(path: String, page: u16, width: u32, hide: Vec<usize>) -> Result<String, String> {
    let pdfium = crate::pdfium()?;
    let img = ff_engine::render_page_hiding(&pdfium, Path::new(&path), page, width, &hide)
        .map_err(|e| e.to_string())?;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Tên tác giả mặc định = tên người dùng hệ điều hành.
#[tauri::command]
pub fn annot_default_author() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// Chọn ảnh PNG/JPG cho con dấu tuỳ chỉnh.
#[tauri::command]
pub fn annot_pick_stamp_image(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("PNG/JPG", &["png", "jpg", "jpeg"])
        .blocking_pick_file()
        .map(|fp| fp.to_string())
}

/// Ảnh (thu nhỏ) dạng data URL để xem trước con dấu ảnh + kích thước gốc.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreview {
    data_url: String,
    width: u32,
    height: u32,
}

#[tauri::command]
pub fn annot_image_preview(path: String) -> Result<ImagePreview, String> {
    let img = image::open(&path).map_err(|e| format!("không đọc được ảnh: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let thumb = if w > 480 || h > 480 { img.thumbnail(480, 480) } else { img };
    let mut buf = Cursor::new(Vec::new());
    thumb.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());
    Ok(ImagePreview { data_url: format!("data:image/png;base64,{b64}"), width: w, height: h })
}
