//! Lớp an toàn bao ngoài PDFium, dùng QPDF qua CLI (`qpdf.exe` prebuilt, xem
//! `scripts/fetch-qpdf.ps1`) — đúng vai trò đã chốt ở `docs/02-tech-stack.md`
//! ("QPDF cho cấu trúc/mã hoá/repair"):
//!
//! 1. `repair()` — sửa file hỏng (xref/trailer sai) TRƯỚC khi PDFium mở, để
//!    không bị `FormatError` trên file PDFium tự đọc thô không nổi.
//! 2. `encrypt_with_password()` — áp lại mật khẩu/mã hoá lên file đã được
//!    PDFium/lopdf chỉnh sửa (ghi ra bản KHÔNG mã hoá), để thao tác trang trên
//!    file có mật khẩu không làm hỏng/mất mã hoá gốc.
//!
//! QPDF thoát mã 0 = không vấn đề, 3 = thành công nhưng có cảnh báo (file vẫn
//! ghi ra được — vd khi repair xref hỏng), 2 = lỗi thật. Coi 0 và 3 là OK.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::EngineError;

/// Tìm `qpdf.exe`. Thứ tự: biến môi trường `FOFREEXIT_QPDF_PATH` (đường dẫn
/// tới file exe hoặc thư mục chứa nó) → `<cwd>/qpdf/bin/qpdf.exe` → cạnh file
/// thực thi hiện tại → `qpdf` trong PATH hệ thống.
pub fn find_qpdf() -> Result<PathBuf, EngineError> {
    let exe_name = if cfg!(windows) { "qpdf.exe" } else { "qpdf" };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("FOFREEXIT_QPDF_PATH") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            candidates.push(p.join(exe_name));
        } else {
            candidates.push(p);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("qpdf").join("bin").join(exe_name));
        candidates.push(cwd.join(exe_name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(exe_name));
        }
    }

    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }

    // Cuối cùng thử PATH hệ thống.
    if Command::new(exe_name).arg("--version").output().is_ok() {
        return Ok(PathBuf::from(exe_name));
    }

    Err(EngineError::Pdfium(format!(
        "không tìm thấy qpdf. Đặt biến môi trường FOFREEXIT_QPDF_PATH, hoặc chạy scripts/fetch-qpdf.ps1. Đã thử: {}",
        candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")
    )))
}

/// `qpdf.exe` hiểu nhầm tiền tố verbatim `\\?\...` (sinh ra bởi
/// `Path::canonicalize()` trên Windows) thành đường dẫn UNC, làm sai lệch
/// path truyền qua command-line. Bỏ tiền tố này trước khi đưa vào argv —
/// an toàn vì các path engine dùng đều ngắn, không cần hỗ trợ path dài.
fn path_arg(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn run_qpdf(args: &[&str]) -> Result<(), EngineError> {
    let qpdf = find_qpdf()?;
    let output = Command::new(&qpdf)
        .args(args)
        .output()
        .map_err(|e| EngineError::Pdfium(format!("chạy qpdf thất bại: {e}")))?;

    // 0 = OK, 3 = OK nhưng có cảnh báo (vd đã tự sửa xref hỏng). Khác đi = lỗi thật.
    match output.status.code() {
        Some(0) | Some(3) => Ok(()),
        _ => Err(EngineError::Pdfium(format!(
            "qpdf lỗi (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ))),
    }
}

/// Sửa file PDF hỏng (xref/trailer sai, thiếu startxref...) bằng cách đọc rồi
/// ghi lại qua QPDF — QPDF tự dựng lại cross-reference table khi cần. Ghi kết
/// quả ra `output`; không sửa `input`.
pub fn repair(input: &Path, output: &Path) -> Result<(), EngineError> {
    let i = path_arg(input);
    let o = path_arg(output);
    run_qpdf(&[&i, &o])
}

/// Quyền hạn áp khi mã hoá (Phase 5) — map thẳng sang cờ AES-256 của qpdf.
/// Mặc định: cho phép tất cả (chỉ chặn bằng mật khẩu mở file).
#[derive(Clone, Copy, Debug)]
pub struct Permissions {
    /// In tài liệu (`--print=full|none`).
    pub allow_print: bool,
    /// Sửa nội dung (`--modify=all|none`).
    pub allow_modify: bool,
    /// Sao chép/trích text & ảnh (`--extract=y|n`).
    pub allow_extract: bool,
    /// Thêm/sửa chú thích + điền form (`--annotate=y|n`).
    pub allow_annotate: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { allow_print: true, allow_modify: true, allow_extract: true, allow_annotate: true }
    }
}

/// Mã hoá `input` (đang KHÔNG mã hoá) thành `output`, AES-256, permissions
/// đầy đủ. Giữ cho tương thích — bản đầy đủ: [`encrypt_with_password_perms`].
pub fn encrypt_with_password(
    input: &Path,
    output: &Path,
    user_password: &str,
    owner_password: &str,
) -> Result<(), EngineError> {
    encrypt_with_password_perms(input, output, user_password, owner_password, Permissions::default())
}

/// Mã hoá AES-256 với user password (mở file), owner password (đổi quyền) và
/// bộ quyền hạn chỉ định. Owner password rỗng → qpdf dùng user password làm
/// owner (tránh file "khoá quyền nhưng ai cũng mở được" thiếu chủ đích).
pub fn encrypt_with_password_perms(
    input: &Path,
    output: &Path,
    user_password: &str,
    owner_password: &str,
    perms: Permissions,
) -> Result<(), EngineError> {
    let i = path_arg(input);
    let o = path_arg(output);
    let owner = if owner_password.is_empty() { user_password } else { owner_password };
    let print = if perms.allow_print { "--print=full" } else { "--print=none" };
    let modify = if perms.allow_modify { "--modify=all" } else { "--modify=none" };
    let extract = if perms.allow_extract { "--extract=y" } else { "--extract=n" };
    let annotate = if perms.allow_annotate { "--annotate=y" } else { "--annotate=n" };
    run_qpdf(&[
        "--encrypt",
        user_password,
        owner,
        "256",
        print,
        modify,
        extract,
        annotate,
        "--",
        &i,
        &o,
    ])
}

/// GỠ mật khẩu/mã hoá: giải mã `input` (mở bằng `password`) và ghi bản KHÔNG
/// mã hoá ra `output`. Cần đúng password (user hoặc owner).
pub fn decrypt_remove_password(
    input: &Path,
    password: &str,
    output: &Path,
) -> Result<(), EngineError> {
    let i = path_arg(input);
    let o = path_arg(output);
    let pw = format!("--password={password}");
    run_qpdf(&[&pw, "--decrypt", &i, &o])
}

/// Mở `input` bằng PDFium an toàn cho cả file hỏng và file mã hoá:
/// 1. Nếu mở thẳng được (đúng password nếu có) → dùng luôn.
/// 2. Nếu PDFium báo lỗi format (không phải do password) → thử `repair()` rồi mở lại.
/// Trả về đường dẫn THỰC SỰ nên dùng để mở tiếp (có thể là `input` hoặc 1 file tạm đã repair).
pub fn ensure_openable(
    pdfium: &pdfium_render::prelude::Pdfium,
    input: &Path,
    password: Option<&str>,
) -> Result<PathBuf, EngineError> {
    if pdfium.load_pdf_from_file(input, password).is_ok() {
        return Ok(input.to_path_buf());
    }
    let repaired = std::env::temp_dir().join(format!(
        "ff_repaired_{}.pdf",
        input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    repair(input, &repaired)?;
    pdfium
        .load_pdf_from_file(&repaired, password)
        .map_err(|e| EngineError::Pdfium(format!("vẫn không mở được sau khi repair: {e}")))?;
    Ok(repaired)
}
