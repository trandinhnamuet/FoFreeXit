//! Test lớp an toàn QPDF (Phase 3): repair file hỏng trước khi PDFium mở,
//! và mã hoá lại file sau khi PDFium/lopdf chỉnh sửa — đường GHI/MỞ file rủi
//! ro nhất nên test bằng fixture THẬT (corrupt-truncated.pdf bị PDFium từ
//! chối thẳng, encrypted.pdf cần đúng password).

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
    if std::env::var("FOFREEXIT_QPDF_PATH").is_err() {
        std::env::set_var("FOFREEXIT_QPDF_PATH", workspace_root().join("qpdf").join("bin"));
    }
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn find_qpdf_locates_binary() {
    let _ = pdfium(); // đảm bảo FOFREEXIT_QPDF_PATH đã set
    let path = ff_engine::find_qpdf().expect("find_qpdf");
    assert!(path.is_file(), "qpdf.exe phải tồn tại tại {}", path.display());
}

#[test]
fn pdfium_rejects_truncated_file_directly() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("corrupt-truncated.pdf");
    let err = pdf.load_pdf_from_file(&input, None);
    assert!(err.is_err(), "PDFium phải từ chối file bị cắt cụt khi chưa repair");
}

#[test]
fn repair_makes_truncated_file_openable() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("corrupt-truncated.pdf");
    let output = tmp("ff_qpdf_repaired.pdf");

    ff_engine::repair(&input, &output).expect("repair");
    let doc = pdf.load_pdf_from_file(&output, None).expect("mở file đã repair");
    assert_eq!(doc.pages().len(), 3, "phải khôi phục đủ 3 trang gốc");
}

#[test]
fn ensure_openable_repairs_automatically() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("corrupt-truncated.pdf");

    let usable = ff_engine::ensure_openable(&pdf, &input, None).expect("ensure_openable");
    assert_ne!(usable, input, "phải trả về 1 file tạm đã repair, không phải input gốc");
    assert_eq!(
        ff_engine::page_count(&pdf, &usable, None).expect("page_count"),
        3
    );
}

#[test]
fn ensure_openable_passes_through_file_that_already_opens() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let usable = ff_engine::ensure_openable(&pdf, &input, None).expect("ensure_openable");
    assert_eq!(usable, input, "file mở thẳng được thì không cần repair");
}

#[test]
fn encrypted_fixture_requires_correct_password() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("encrypted.pdf");

    assert!(pdf.load_pdf_from_file(&input, None).is_err(), "không password phải lỗi");
    assert!(
        pdf.load_pdf_from_file(&input, Some("sai-password")).is_err(),
        "sai password phải lỗi"
    );
    let doc = pdf
        .load_pdf_from_file(&input, Some("fofreexit"))
        .expect("đúng password phải mở được");
    assert_eq!(doc.pages().len(), 3);
}

#[test]
fn encrypt_with_password_round_trips() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf"); // chưa mã hoá
    let output = tmp("ff_qpdf_encrypted.pdf");

    ff_engine::encrypt_with_password(&input, &output, "userpw", "ownerpw")
        .expect("encrypt_with_password");

    assert!(pdf.load_pdf_from_file(&output, None).is_err(), "phải cần password sau khi mã hoá");
    let doc = pdf
        .load_pdf_from_file(&output, Some("userpw"))
        .expect("đúng password phải mở được");
    assert_eq!(doc.pages().len(), 3, "nội dung phải giữ nguyên sau khi mã hoá");
}
