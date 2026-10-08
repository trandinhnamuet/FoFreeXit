#[allow(dead_code)]
#[path = "src/runtime_pack.rs"]
mod runtime_pack;

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn main() {
    embed_runtime();
    tauri_build::build()
}

/// Bản 1 file: CI đặt FOFREEXIT_EMBED_RUNTIME=1 → gói pdfium.dll, qpdf/bin
/// (exe + dll) và fonts/ ở gốc workspace vào exe (app giải nén lúc chạy, xem
/// src/runtime.rs). Build dev không đặt → gói rỗng, app dò file như cũ.
fn embed_runtime() {
    println!("cargo:rerun-if-env-changed=FOFREEXIT_EMBED_RUNTIME");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if std::env::var("FOFREEXIT_EMBED_RUNTIME").as_deref() == Ok("1") {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
        add(&mut files, &root.join("pdfium.dll"), "pdfium.dll".into());
        for p in dir_files(&root.join("qpdf/bin")) {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if ext == "exe" || ext == "dll" {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                add(&mut files, &p, name);
            }
        }
        for p in dir_files(&root.join("fonts")) {
            let name = format!("fonts/{}", p.file_name().unwrap().to_string_lossy());
            add(&mut files, &p, name);
        }
        for need in ["pdfium.dll", "qpdf.exe", "fonts/NotoSans-Regular.ttf"] {
            assert!(
                files.iter().any(|(n, _)| n == need),
                "FOFREEXIT_EMBED_RUNTIME=1 nhưng thiếu {need} — chạy scripts/fetch-*.ps1 trước"
            );
        }
    }
    let pack = runtime_pack::write_pack(&files).expect("đóng gói runtime");
    let id = if files.is_empty() {
        String::new()
    } else {
        Sha256::digest(&pack)[..8].iter().map(|b| format!("{b:02x}")).collect()
    };
    fs::write(out.join("runtime.pack"), &pack).unwrap();
    fs::write(out.join("runtime.id"), id).unwrap();
}

fn add(files: &mut Vec<(String, Vec<u8>)>, path: &Path, name: String) {
    println!("cargo:rerun-if-changed={}", path.display());
    if let Ok(data) = fs::read(path) {
        files.push((name, data));
    }
}

/// File (không đệ quy) trong thư mục, sắp theo tên để gói ổn định giữa các lần build.
fn dir_files(dir: &Path) -> Vec<PathBuf> {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    v.sort();
    v
}
