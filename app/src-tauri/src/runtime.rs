//! Bản phát hành 1 file: build.rs nhúng pdfium.dll + qpdf + fonts/ vào exe
//! (định dạng ở runtime_pack.rs). Khởi động thì giải nén những file còn
//! thiếu ra `%LOCALAPPDATA%\FoFreeXit\runtime\<id>\` — KHÔNG ghi gì cạnh exe,
//! nên copy riêng FoFreeXit.exe đi đâu cũng chạy — rồi main() trỏ engine tới
//! đó qua các biến môi trường FOFREEXIT_*.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::runtime_pack::{self, Entry};

static PACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.pack"));
/// Băm nội dung gói; rỗng = build dev không nhúng gì.
const PACK_ID: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime.id"));

/// Thư mục runtime đã đủ file; `None` nếu exe không nhúng gói (build dev)
/// hoặc giải nén lỗi — khi đó app dò file cạnh exe/gốc workspace như cũ.
pub fn ensure_extracted() -> Option<PathBuf> {
    if PACK_ID.is_empty() {
        return None;
    }
    let entries = runtime_pack::read_index(PACK)?;
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("FoFreeXit")
        .join("runtime");
    let dir = base.join(PACK_ID);
    for e in &entries {
        let rel = Path::new(&e.name);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return None;
        }
        let dest = dir.join(rel);
        // Đã giải nén từ lần chạy trước → bỏ qua (chỉ so cỡ, rẻ).
        if has_size(&dest, e.size) {
            continue;
        }
        if let Err(err) = write_entry(&dest, e) {
            eprintln!("giải nén runtime {}: {err}", dest.display());
            return None;
        }
    }
    let keep = dir.clone();
    std::thread::spawn(move || remove_old_versions(&base, &keep));
    Some(dir)
}

fn has_size(p: &Path, size: u64) -> bool {
    fs::metadata(p).map(|m| m.is_file() && m.len() == size).unwrap_or(false)
}

/// Ghi ra file tạm rồi đổi tên — 2 cửa sổ app mở cùng lúc không đọc phải
/// file ghi dở.
fn write_entry(dest: &Path, e: &Entry) -> io::Result<()> {
    let data = runtime_pack::unpack(PACK, e)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, &data)?;
    match fs::rename(&tmp, dest) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            // Instance khác vừa ghi xong (DLL đang nạp → không thay được).
            if has_size(dest, e.size) { Ok(()) } else { Err(err) }
        }
    }
}

/// Dọn thư mục của các phiên bản cũ. Bỏ qua bản đang chạy (pdfium.dll còn
/// bị nạp → xoá không được) để không xoá mất font của nó.
fn remove_old_versions(base: &Path, keep: &Path) {
    let Ok(rd) = fs::read_dir(base) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p == keep || !p.is_dir() {
            continue;
        }
        let dll = p.join("pdfium.dll");
        if dll.exists() && fs::remove_file(&dll).is_err() {
            continue;
        }
        let _ = fs::remove_dir_all(&p);
    }
}
