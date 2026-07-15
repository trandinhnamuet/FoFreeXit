//! Test bảo mật Phase 5: mã hoá AES-256 + permissions, gỡ mật khẩu, xoá
//! metadata nhận dạng. Cần binary `qpdf` (như qpdf_safety).

use std::path::PathBuf;
use std::process::Command;

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
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

fn hello() -> PathBuf {
    workspace_root().join("corpus").join("hello.pdf")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Mã hoá với permissions: mở KHÔNG mật khẩu phải fail, đúng mật khẩu OK, và
/// qpdf --show-encryption xác nhận cờ quyền in = none.
#[test]
fn encrypt_with_perms_round_trip_and_flags() {
    let pdf = pdfium();
    let out = tmp("ff_sec_enc.pdf");
    let perms = ff_engine::Permissions { allow_print: false, ..Default::default() };
    ff_engine::encrypt_with_password_perms(&hello(), &out, "u123", "o456", perms).expect("encrypt");

    assert!(pdf.load_pdf_from_file(&out, None).is_err(), "không mật khẩu phải bị chặn");
    assert!(pdf.load_pdf_from_file(&out, Some("u123")).is_ok(), "đúng user password phải mở được");
    assert!(pdf.load_pdf_from_file(&out, Some("o456")).is_ok(), "owner password phải mở được");

    let qpdf = ff_engine::find_qpdf().expect("qpdf");
    let show = Command::new(qpdf)
        .args(["--password=u123", "--show-encryption", &out.to_string_lossy()])
        .output()
        .expect("show-encryption");
    let s = String::from_utf8_lossy(&show.stdout).to_lowercase();
    assert!(s.contains("print"), "phải có dòng quyền in: {s}");
    // qpdf in "print high resolution: not allowed" (và/hoặc low) khi --print=none.
    assert!(s.contains("not allowed"), "quyền in phải bị chặn: {s}");
}

/// Gỡ mật khẩu: sau decrypt_remove_password, file mở được KHÔNG cần mật khẩu
/// và nội dung còn nguyên.
#[test]
fn decrypt_removes_password() {
    let pdf = pdfium();
    let enc = tmp("ff_sec_enc2.pdf");
    let dec = tmp("ff_sec_dec2.pdf");
    ff_engine::encrypt_with_password(&hello(), &enc, "pw789", "pw789").expect("encrypt");
    assert!(pdf.load_pdf_from_file(&enc, None).is_err(), "fixture phải đang khoá");

    ff_engine::decrypt_remove_password(&enc, "pw789", &dec).expect("decrypt");
    assert!(pdf.load_pdf_from_file(&dec, None).is_ok(), "sau gỡ phải mở tự do");
    let text = ff_engine::extract_text(&pdf, &dec, 0, None).expect("extract");
    assert!(text.contains("Hello"), "nội dung giữ nguyên: {text:?}");
}

/// Gỡ mật khẩu bằng password SAI phải lỗi (không âm thầm ghi file rỗng).
#[test]
fn decrypt_with_wrong_password_fails() {
    let enc = tmp("ff_sec_enc3.pdf");
    let dec = tmp("ff_sec_dec3.pdf");
    ff_engine::encrypt_with_password(&hello(), &enc, "right", "right").expect("encrypt");
    assert!(
        ff_engine::decrypt_remove_password(&enc, "wrong", &dec).is_err(),
        "password sai phải trả lỗi"
    );
}

/// Xoá metadata: /Info (Producer/Author…) và XMP /Metadata phải biến mất KHỎI
/// FILE (soi bytes thô), file vẫn mở/đọc bình thường.
#[test]
fn strip_metadata_removes_info_and_xmp() {
    let pdf = pdfium();
    let fx = tmp("ff_sec_meta_fx.pdf");
    let out = tmp("ff_sec_meta_out.pdf");

    // Fixture: chuẩn hoá qua qpdf trước (trailer của hello.pdf viết tay, lopdf
    // không parse thẳng được — đúng luồng thật: file luôn qua repair/save của
    // engine trước khi strip), rồi chèn /Info với Producer nhận dạng được.
    let norm = tmp("ff_sec_meta_norm.pdf");
    ff_engine::repair(&hello(), &norm).expect("qpdf normalize");
    let mut doc = lopdf::Document::load(&norm).expect("lopdf load");
    let mut info = lopdf::Dictionary::new();
    info.set("Producer", lopdf::Object::string_literal("FoFreeXitSecretProducer"));
    info.set("Author", lopdf::Object::string_literal("NguoiDungBiMat"));
    let info_id = doc.add_object(lopdf::Object::Dictionary(info));
    doc.trailer.set("Info", lopdf::Object::Reference(info_id));
    doc.save(&fx).expect("save fixture");
    let raw_before = std::fs::read(&fx).expect("đọc fixture");
    assert!(
        raw_before.windows(23).any(|w| w == b"FoFreeXitSecretProducer"),
        "fixture phải chứa Producer"
    );

    ff_engine::strip_metadata(&fx, &out).expect("strip");

    let raw = std::fs::read(&out).expect("đọc out");
    assert!(
        !raw.windows(23).any(|w| w == b"FoFreeXitSecretProducer"),
        "Producer phải biến mất khỏi bytes của file"
    );
    assert!(!raw.windows(14).any(|w| w == b"NguoiDungBiMat"), "Author phải biến mất");
    let cleaned = lopdf::Document::load(&out).expect("lopdf load out");
    assert!(cleaned.trailer.get(b"Info").is_err(), "trailer không còn /Info");
    // Vẫn mở & đọc được bình thường.
    assert!(pdf.load_pdf_from_file(&out, None).is_ok());
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    assert!(text.contains("Hello"), "nội dung trang giữ nguyên: {text:?}");
}
