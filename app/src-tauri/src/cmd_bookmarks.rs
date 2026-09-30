//! Command cho Bookmark (sửa cây outline), Liên kết (liệt kê/tạo/xoá, mở URL)
//! và các kiểu Tách tệp mới (dải trang / bookmark / dung lượng).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pdfium;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BookmarkDto {
    title: String,
    page_index: Option<u32>,
    uri: Option<String>,
    left: Option<f32>,
    top: Option<f32>,
    zoom: Option<f32>,
    fit: bool,
    bold: bool,
    italic: bool,
    color: Option<[u8; 3]>,
    open: bool,
    children: Vec<BookmarkDto>,
}

impl From<ff_engine::Bookmark> for BookmarkDto {
    fn from(b: ff_engine::Bookmark) -> Self {
        BookmarkDto {
            title: b.title,
            page_index: b.page_index,
            uri: b.uri,
            left: b.left,
            top: b.top,
            zoom: b.zoom,
            fit: b.fit,
            bold: b.bold,
            italic: b.italic,
            color: b.color,
            open: b.open,
            children: b.children.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<BookmarkDto> for ff_engine::Bookmark {
    fn from(b: BookmarkDto) -> Self {
        ff_engine::Bookmark {
            title: b.title,
            page_index: b.page_index,
            uri: b.uri,
            left: b.left,
            top: b.top,
            zoom: b.zoom,
            fit: b.fit,
            bold: b.bold,
            italic: b.italic,
            color: b.color,
            open: b.open,
            children: b.children.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RectDto {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkDto {
    id: String,
    page_index: u32,
    rect: RectDto,
    dest_page: Option<u32>,
    dest_top: Option<f32>,
    uri: Option<String>,
    other: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLinkDto {
    page_index: u32,
    rect: RectDto,
    dest_page: Option<u32>,
    dest_top: Option<f32>,
    uri: Option<String>,
    border: Option<[u8; 3]>,
}

/// Đọc toàn bộ cây bookmark (lopdf) — đủ cấp, kiểu chữ, màu, trạng thái mở.
#[tauri::command(async)]
pub fn bm_get_tree(path: String) -> Result<Vec<BookmarkDto>, String> {
    let t = ff_engine::get_outline(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(t.into_iter().map(Into::into).collect())
}

/// Tự tạo cây bookmark từ tiêu đề (cỡ chữ lớn) — chỉ trả cây, chưa ghi.
#[tauri::command(async)]
pub fn bm_auto_generate(path: String, max_levels: u32) -> Result<Vec<BookmarkDto>, String> {
    let pdfium = pdfium()?;
    let t = ff_engine::auto_bookmarks_from_headings(&pdfium, Path::new(&path), max_levels, None)
        .map_err(|e| e.to_string())?;
    Ok(t.into_iter().map(Into::into).collect())
}

/// Lưu một lần: cây bookmark mới (None = giữ nguyên) + xoá/tạo liên kết.
#[tauri::command(async)]
pub fn bm_save(
    input: String,
    output: String,
    tree: Option<Vec<BookmarkDto>>,
    remove_links: Vec<String>,
    add_links: Vec<NewLinkDto>,
) -> Result<(), String> {
    let tree: Option<Vec<ff_engine::Bookmark>> = tree.map(|t| t.into_iter().map(Into::into).collect());
    let add: Vec<ff_engine::NewLink> = add_links
        .into_iter()
        .map(|l| ff_engine::NewLink {
            page_index: l.page_index,
            rect: ff_engine::Rect { left: l.rect.left, bottom: l.rect.bottom, right: l.rect.right, top: l.rect.top },
            dest_page: l.dest_page,
            dest_top: l.dest_top,
            uri: l.uri,
            border: l.border,
        })
        .collect();
    // Ghi ra tệp tạm cạnh đích rồi đổi tên: lưu đè chính tệp đang mở cũng an toàn.
    let out = Path::new(&output);
    let tmp = out.with_extension("ffbm.tmp");
    ff_engine::save_outline_and_links(Path::new(&input), &tmp, tree.as_deref(), &remove_links, &add)
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })?;
    std::fs::rename(&tmp, out)
        .or_else(|_| std::fs::copy(&tmp, out).map(|_| ()).and_then(|_| std::fs::remove_file(&tmp)))
        .map_err(|e| format!("ghi {}: {e}", out.display()))
}

/// Mọi liên kết của tài liệu (toạ độ hiển thị, đích đã giải).
#[tauri::command(async)]
pub fn link_list(path: String) -> Result<Vec<LinkDto>, String> {
    let links = ff_engine::list_links(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(links
        .into_iter()
        .map(|l| LinkDto {
            id: l.id,
            page_index: l.page_index,
            rect: RectDto { left: l.rect.left, bottom: l.rect.bottom, right: l.rect.right, top: l.rect.top },
            dest_page: l.dest_page,
            dest_top: l.dest_top,
            uri: l.uri,
            other: l.other,
        })
        .collect())
}

/// Mở URL bằng trình duyệt mặc định. Chỉ cho các scheme web/mail an toàn —
/// không bao giờ chạy tệp/lệnh tuỳ ý từ tài liệu PDF.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if !["http://", "https://", "mailto:", "ftp://"].iter().any(|p| lower.starts_with(p)) {
        return Err(format!("không hỗ trợ mở địa chỉ này: {u}"));
    }
    if u.chars().any(|c| c.is_control()) {
        return Err("địa chỉ chứa ký tự không hợp lệ".into());
    }
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", u]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(u).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(u).spawn();
    r.map(|_| ()).map_err(|e| format!("không mở được trình duyệt: {e}"))
}

fn outs(v: Vec<std::path::PathBuf>) -> Vec<String> {
    v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Tách theo dải trang "1-3, 4-10, 11-" (mỗi dải một tệp).
#[tauri::command(async)]
pub fn split_by_ranges(input: String, spec: String, out_dir: String, base_name: String) -> Result<Vec<String>, String> {
    let pdfium = pdfium()?;
    let p = Path::new(&input);
    let count = ff_engine::page_count(&pdfium, p, None).map_err(|e| e.to_string())?;
    let ranges = ff_engine::parse_page_ranges(&spec, count).map_err(|e| e.to_string())?;
    ff_engine::split_by_ranges(&pdfium, p, &ranges, Path::new(&out_dir), &base_name, None)
        .map(outs)
        .map_err(|e| e.to_string())
}

/// Tách theo bookmark cấp cao nhất (tên tệp = tiêu đề bookmark).
#[tauri::command(async)]
pub fn split_by_bookmarks(input: String, out_dir: String, base_name: String) -> Result<Vec<String>, String> {
    let pdfium = pdfium()?;
    ff_engine::split_by_bookmarks(&pdfium, Path::new(&input), Path::new(&out_dir), &base_name, None)
        .map(outs)
        .map_err(|e| e.to_string())
}

/// Tách theo dung lượng tối đa mỗi tệp (MB, xấp xỉ).
#[tauri::command(async)]
pub fn split_by_size(input: String, max_mb: f64, out_dir: String, base_name: String) -> Result<Vec<String>, String> {
    if !(max_mb > 0.0) {
        return Err("dung lượng phải > 0".into());
    }
    let pdfium = pdfium()?;
    let bytes = (max_mb * 1024.0 * 1024.0) as u64;
    ff_engine::split_by_size(&pdfium, Path::new(&input), bytes, Path::new(&out_dir), &base_name, None)
        .map(outs)
        .map_err(|e| e.to_string())
}
