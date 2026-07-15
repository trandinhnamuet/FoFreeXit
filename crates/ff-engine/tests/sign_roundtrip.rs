//! Test chữ ký số (Phase 5 iteration 2): tạo Digital ID → ký → xác thực hợp
//! lệ; và phát hiện GIẢ MẠO (sửa 1 byte nội dung sau khi ký → chữ ký hỏng).
//! Cần binary `qpdf`.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn ensure_qpdf() {
    if std::env::var("FOFREEXIT_QPDF_PATH").is_err() {
        std::env::set_var("FOFREEXIT_QPDF_PATH", "/usr/bin");
    }
}

fn sample() -> PathBuf {
    workspace_root().join("corpus").join("sample-multipage.pdf")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn sign_then_verify_is_valid() {
    ensure_qpdf();
    let id = tmp("ff_sign_id.pem");
    let out = tmp("ff_signed.pdf");
    ff_engine::generate_self_signed_id("Nguyen Van Ky", &id).expect("tạo ID");
    ff_engine::sign_pdf(&sample(), &id, "Tôi đồng ý nội dung", "Nguyen Van Ky", &out).expect("ký");

    let checks = ff_engine::verify_signatures(&out).expect("verify");
    assert_eq!(checks.len(), 1, "phải có đúng 1 chữ ký");
    let c = &checks[0];
    assert!(c.crypto_valid, "chữ ký RSA phải hợp lệ");
    assert!(c.digest_matches, "messageDigest phải khớp digest file");
    assert!(c.covers_document, "chữ ký phải phủ toàn bộ file");
    assert!(c.is_valid(), "tổng thể phải VALID");
    assert!(c.signer.contains("Nguyen Van Ky"), "tên người ký: {:?}", c.signer);
}

#[test]
fn tampering_after_signing_is_detected() {
    ensure_qpdf();
    let id = tmp("ff_sign_id2.pem");
    let signed = tmp("ff_signed2.pdf");
    let tampered = tmp("ff_tampered2.pdf");
    ff_engine::generate_self_signed_id("Kiem Thu", &id).expect("tạo ID");
    ff_engine::sign_pdf(&sample(), &id, "ký", "Kiem Thu", &signed).expect("ký");

    // Sửa 1 byte trong VÙNG ĐƯỢC KÝ (đầu file — header/nội dung), không đụng
    // vùng Contents. Chữ ký phải phát hiện.
    let mut bytes = std::fs::read(&signed).expect("đọc signed");
    // Tìm 1 byte chữ cái gần đầu (sau "%PDF-") để lật.
    let pos = 8;
    bytes[pos] ^= 0x01;
    std::fs::write(&tampered, &bytes).expect("ghi tampered");

    let checks = ff_engine::verify_signatures(&tampered).expect("verify tampered");
    assert_eq!(checks.len(), 1);
    assert!(
        !checks[0].is_valid(),
        "sửa nội dung sau khi ký phải làm chữ ký KHÔNG hợp lệ (digest_matches={}, crypto={}, covers={})",
        checks[0].digest_matches,
        checks[0].crypto_valid,
        checks[0].covers_document
    );
}

#[test]
fn verify_reports_none_for_unsigned() {
    ensure_qpdf();
    let checks = ff_engine::verify_signatures(&sample()).expect("verify unsigned");
    assert!(checks.is_empty(), "file chưa ký không có chữ ký nào");
}
