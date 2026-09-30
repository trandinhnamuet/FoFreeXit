//! Chữ ký số nâng cao (PFX, chữ ký hiển thị, ký nhiều lần, ký vào ô có sẵn),
//! Điền & Ký (đóng dấu vào nội dung) và Batch / Action Wizard. Cần `qpdf`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use ff_engine::{BatchStep, FillItem, FillMarkKind as MarkKind, SigAppearance, SignRequest};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
}

fn ensure_env() {
    if std::env::var("FOFREEXIT_QPDF_PATH").is_err() {
        let ws = workspace_root().join("qpdf").join("bin");
        std::env::set_var("FOFREEXIT_QPDF_PATH", if ws.exists() { ws } else { PathBuf::from("/usr/bin") });
    }
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    ensure_env();
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

fn corpus(name: &str) -> PathBuf {
    workspace_root().join("corpus").join(name)
}

/// Bản chuẩn hoá qua qpdf (xref đúng) — tệp corpus viết tay có xref lệch nên
/// bộ ký sẽ tự chuẩn hoá; dùng bản chuẩn để kiểm được tính "incremental".
fn norm(name: &str) -> PathBuf {
    ensure_env();
    let p = tmp(&format!("norm_{name}"));
    ff_engine::repair(&corpus(name), &p).expect("qpdf repair");
    p
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("signx_{name}"));
    let _ = std::fs::remove_file(&p);
    p
}

/// PFX dựng NGAY TRONG TEST: khoá RSA + cert tự ký (rcgen) → PKCS#12 (p12-keystore).
fn test_pfx(cn: &str, password: &str) -> PathBuf {
    use rsa::pkcs8::EncodePrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    let key_der = key.to_pkcs8_der().unwrap().as_bytes().to_vec();
    let key_pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap().to_string();
    let kp = rcgen::KeyPair::from_pkcs8_pem_and_sign_algo(&key_pem, &rcgen::PKCS_RSA_SHA256).unwrap();
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, cn);
    dn.push(rcgen::DnType::OrganizationName, "Test Org");
    params.distinguished_name = dn;
    let cert = params.self_signed(&kp).unwrap();
    let c = p12_keystore::Certificate::from_der(cert.der()).unwrap();
    let chain = p12_keystore::PrivateKeyChain::new(key_der, vec![1u8, 2, 3, 4], vec![c]);
    let mut ks = p12_keystore::KeyStore::new();
    ks.add_entry("test", p12_keystore::KeyStoreEntry::PrivateKeyChain(chain));
    let bytes = ks.writer(password).write().unwrap();
    let p = tmp(&format!("{cn}.pfx").replace(' ', "_"));
    std::fs::write(&p, bytes).unwrap();
    p
}

/// PNG chữ ký nhỏ (nền trong suốt, nét xanh).
fn sig_png() -> PathBuf {
    let mut img = image::RgbaImage::new(120, 40);
    for x in 5..115 {
        let y = (20.0 + 12.0 * ((x as f32) / 12.0).sin()) as u32;
        for dy in 0..3 {
            img.put_pixel(x, (y + dy).min(39), image::Rgba([10, 40, 160, 255]));
        }
    }
    let p = tmp("sig.png");
    img.save(&p).unwrap();
    p
}

fn visible_req(page: u16) -> SignRequest {
    SignRequest {
        reason: "Tôi phê duyệt tài liệu".into(),
        location: "Hà Nội".into(),
        contact_info: "a@example.com".into(),
        signer_name: String::new(),
        tz_offset_min: 420,
        field_name: None,
        appearance: Some(SigAppearance {
            page,
            rect: [300.0, 80.0, 540.0, 150.0],
            image_path: Some(sig_png().to_string_lossy().into_owned()),
            big_text: String::new(),
            lines: vec![
                "Ký số bởi: Nguyễn Văn Ký".into(),
                "Ngày: 2026.09.30 10:15:00 +07'00'".into(),
                "Lý do: Tôi phê duyệt tài liệu".into(),
                "Nơi ký: Hà Nội".into(),
            ],
        }),
    }
}

#[test]
fn pfx_visible_signature_verifies_and_has_appearance() {
    let pdfium = pdfium();
    let pfx = test_pfx("Nguyen Van Ky", "matkhau123");
    // Sai mật khẩu → lỗi rõ ràng.
    assert!(ff_engine::load_identity_file(&pfx, Some("sai")).is_err(), "sai mật khẩu PFX phải lỗi");
    let id = ff_engine::load_identity_file(&pfx, Some("matkhau123")).expect("nạp PFX");
    let info = ff_engine::identity_info(&pfx, Some("matkhau123")).expect("info");
    assert_eq!(info.common_name, "Nguyen Van Ky");
    assert!(info.self_signed);

    let out = tmp("visible.pdf");
    let src = norm("sample-multipage.pdf");
    ff_engine::sign_pdf_ex(Some(&pdfium), &src, &id, &visible_req(1), &out).expect("ký");

    // Bản gốc giữ nguyên từng byte ở đầu (incremental update).
    let orig = std::fs::read(&src).unwrap();
    let signed = std::fs::read(&out).unwrap();
    assert!(signed.starts_with(&orig), "ký phải là incremental update (giữ nguyên bản gốc)");

    let checks = ff_engine::verify_signatures(&out).expect("verify");
    assert_eq!(checks.len(), 1);
    let c = &checks[0];
    assert!(c.is_valid(), "chữ ký hiển thị phải hợp lệ: {c:?}");
    assert_eq!(c.signer, "Nguyen Van Ky");
    assert_eq!(c.reason, "Tôi phê duyệt tài liệu");
    assert_eq!(c.location, "Hà Nội");
    assert!(c.sign_time.starts_with("20"), "thời gian ký: {}", c.sign_time);
    assert!(c.cert.self_signed && c.cert.subject.contains("Nguyen Van Ky"), "{:?}", c.cert);
    assert!(c.cert_valid_at_signing);
    assert_eq!(c.cert.key_bits, 2048);

    // Widget trên trang 2 có /AP /N (Form XObject có nội dung + ảnh).
    let doc = lopdf::Document::load(&out).expect("lopdf đọc bản ký");
    let fields = ff_engine::list_signature_fields(&out).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].page_index, Some(1));
    assert!(fields[0].signed);
    let r = fields[0].rect.unwrap();
    assert!((r[0] - 300.0).abs() < 0.5 && (r[3] - 150.0).abs() < 0.5, "{r:?}");
    let pages = doc.get_pages();
    let page = doc.get_dictionary(pages[&2]).unwrap();
    let annots = page.get(b"Annots").unwrap().as_array().unwrap();
    let w = doc.get_dictionary(annots.last().unwrap().as_reference().unwrap()).unwrap();
    let ap = w.get(b"AP").unwrap().as_dict().unwrap();
    let n = doc.get_object(ap.get(b"N").unwrap().as_reference().unwrap()).unwrap().as_stream().unwrap();
    assert!(!n.content.is_empty(), "appearance stream rỗng");
    let res = n.dict.get(b"Resources").unwrap();
    let res = match res { lopdf::Object::Reference(r) => doc.get_dictionary(*r).unwrap(), o => o.as_dict().unwrap() };
    assert!(res.has(b"XObject"), "appearance phải chứa ảnh chữ ký");
    assert!(res.has(b"Font"), "appearance phải chứa chữ");

    // PDFium mở được + trang 2 render ra có mực trong vùng chữ ký.
    let img = ff_engine::render::render_page(&pdfium, &out, 1, 612, None).unwrap().image.to_rgba8();
    let (x0, x1) = (300u32, 540u32);
    let h = img.height() as f32;
    let scale = img.width() as f32 / 612.0;
    let (y0, y1) = ((h - 150.0 * scale) as u32, (h - 80.0 * scale) as u32);
    let dark = (y0..y1)
        .flat_map(|y| ((x0 as f32 * scale) as u32..(x1 as f32 * scale) as u32).map(move |x| (x, y)))
        .filter(|&(x, y)| img.get_pixel(x, y).0[0] < 128)
        .count();
    assert!(dark > 200, "vùng chữ ký phải có nội dung hiển thị (dark={dark})");
}

#[test]
fn tampering_visible_signature_is_invalid() {
    let pdfium = pdfium();
    let pfx = test_pfx("Kiem Thu", "pw");
    let id = ff_engine::load_identity_file(&pfx, Some("pw")).unwrap();
    let out = tmp("tamper_src.pdf");
    ff_engine::sign_pdf_ex(Some(&pdfium), &corpus("sample-multipage.pdf"), &id, &visible_req(0), &out).unwrap();
    let mut b = std::fs::read(&out).unwrap();
    // Lật 1 byte trong nội dung trang (vùng được ký, không phải Contents).
    let pos = b.windows(2).position(|w| w == b"BT").unwrap_or(40);
    b[pos + 3] ^= 0x01;
    let bad = tmp("tampered.pdf");
    std::fs::write(&bad, &b).unwrap();
    let c = ff_engine::verify_signatures(&bad).unwrap();
    assert_eq!(c.len(), 1);
    assert!(!c[0].is_valid() && !c[0].digest_matches, "sửa sau khi ký phải bị phát hiện");

    // Nối thêm dữ liệu sau khi ký → phần ký vẫn nguyên nhưng không phủ toàn bộ.
    let mut b2 = std::fs::read(&out).unwrap();
    b2.extend_from_slice(b"\n% appended\n");
    let app = tmp("appended.pdf");
    std::fs::write(&app, &b2).unwrap();
    let c2 = ff_engine::verify_signatures(&app).unwrap();
    assert!(c2[0].intact() && !c2[0].covers_document && !c2[0].later_changes_are_signatures);
}

#[test]
fn second_signature_keeps_first_valid() {
    let pdfium = pdfium();
    let a = ff_engine::load_identity_file(&test_pfx("Nguoi A", "a"), Some("a")).unwrap();
    let b = ff_engine::load_identity_file(&test_pfx("Nguoi B", "b"), Some("b")).unwrap();
    let s1 = tmp("multi1.pdf");
    let s2 = tmp("multi2.pdf");
    ff_engine::sign_pdf_ex(Some(&pdfium), &corpus("hello.pdf"), &a, &visible_req(0), &s1).unwrap();
    let mut req = visible_req(0);
    req.appearance.as_mut().unwrap().rect = [60.0, 80.0, 280.0, 150.0];
    ff_engine::sign_pdf_ex(Some(&pdfium), &s1, &b, &req, &s2).unwrap();
    let c = ff_engine::verify_signatures(&s2).unwrap();
    assert_eq!(c.len(), 2);
    assert!(c[0].intact() && !c[0].covers_document && c[0].later_changes_are_signatures, "{:?}", c[0]);
    assert!(c[1].is_valid(), "{:?}", c[1]);
    assert_eq!(c[0].signer, "Nguoi A");
    assert_eq!(c[1].signer, "Nguoi B");
    assert_eq!(ff_engine::list_signature_fields(&s2).unwrap().len(), 2);
    // PDFium đọc được chuỗi xref incremental.
    assert_eq!(ff_engine::page_count(&pdfium, &s2, None).unwrap(), 1);
}

#[test]
fn sign_into_existing_empty_field() {
    let pdfium = pdfium();
    let with_field = tmp("with_field.pdf");
    // Ô chữ ký trống (/FT /Sig, chưa /V) dựng bằng lopdf — như file do Word/Foxit tạo.
    {
        use lopdf::{Dictionary, Object};
        let mut doc = lopdf::Document::load(norm("sample-multipage.pdf")).unwrap();
        let page_id = doc.get_pages()[&1];
        let mut w = Dictionary::new();
        w.set("Type", Object::Name(b"Annot".to_vec()));
        w.set("Subtype", Object::Name(b"Widget".to_vec()));
        w.set("FT", Object::Name(b"Sig".to_vec()));
        w.set("T", Object::string_literal("ChuKyGiamDoc"));
        w.set("Rect", Object::Array(vec![100.into(), 100.into(), 300.into(), 160.into()]));
        w.set("P", Object::Reference(page_id));
        w.set("F", Object::Integer(4));
        let wid = doc.add_object(Object::Dictionary(w));
        doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap().set("Annots", Object::Array(vec![Object::Reference(wid)]));
        let acro = doc.add_object(Object::Dictionary(Dictionary::from_iter(vec![("Fields", Object::Array(vec![Object::Reference(wid)]))])));
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_object_mut(root).unwrap().as_dict_mut().unwrap().set("AcroForm", Object::Reference(acro));
        doc.save(&with_field).unwrap();
    }
    let fields = ff_engine::list_signature_fields(&with_field).unwrap();
    let f = fields.iter().find(|f| f.name == "ChuKyGiamDoc");
    assert!(f.is_some(), "không thấy ô chữ ký trống: {fields:?}");
    assert!(!f.unwrap().signed);
    let id = ff_engine::load_identity_file(&test_pfx("Giam Doc", "gd"), Some("gd")).unwrap();
    let mut req = visible_req(0);
    req.field_name = Some("ChuKyGiamDoc".into());
    let out = tmp("field_signed.pdf");
    ff_engine::sign_pdf_ex(Some(&pdfium), &with_field, &id, &req, &out).unwrap();
    let c = ff_engine::verify_signatures(&out).unwrap();
    assert_eq!(c.len(), 1);
    assert!(c[0].is_valid());
    let fields = ff_engine::list_signature_fields(&out).unwrap();
    assert_eq!(fields.len(), 1, "không được tạo ô mới: {fields:?}");
    assert!(fields[0].signed);
    // Ký lại ô đã ký → lỗi.
    assert!(ff_engine::sign_pdf_ex(Some(&pdfium), &out, &id, &req, &tmp("again.pdf")).is_err());
}

#[test]
fn self_signed_pfx_generation_roundtrip() {
    ensure_env();
    let p = tmp("gen.pfx");
    ff_engine::generate_self_signed_pfx("Trần Thị Bình", "Công ty ABC", "binh@abc.vn", "123456", &p).unwrap();
    assert!(ff_engine::load_identity_file(&p, Some("000000")).is_err());
    let info = ff_engine::identity_info(&p, Some("123456")).unwrap();
    assert_eq!(info.common_name, "Trần Thị Bình");
    assert!(!info.expired);
    let id = ff_engine::load_identity_file(&p, Some("123456")).unwrap();
    let out = tmp("gen_signed.pdf");
    let req = SignRequest { reason: "OK".into(), ..Default::default() };
    ff_engine::sign_pdf_ex(None, &corpus("hello.pdf"), &id, &req, &out).unwrap();
    let c = ff_engine::verify_signatures(&out).unwrap();
    assert!(c[0].is_valid());
    assert_eq!(c[0].signer, "Trần Thị Bình");
}

#[test]
fn fill_and_sign_stamps_into_content() {
    let pdfium = pdfium();
    let out = tmp("filled.pdf");
    let items = vec![
        FillItem::Text {
            page: 0,
            x: 72.0,
            y: 700.0,
            text: "Nguyễn Văn Điền\nDòng hai".into(),
            font_size: 14.0,
            color: [0, 0, 0, 255],
            font_family: None,
            line_height: None,
        },
        FillItem::Image {
            page: 1,
            x: 72.0,
            y: 100.0,
            width: 150.0,
            height: 50.0,
            image_path: sig_png().to_string_lossy().into_owned(),
        },
        FillItem::Mark { page: 0, mark: MarkKind::Check, x: 72.0, y: 600.0, width: 16.0, height: 16.0, color: [0, 0, 0, 255], stroke: None },
        FillItem::Mark { page: 0, mark: MarkKind::Cross, x: 100.0, y: 600.0, width: 16.0, height: 16.0, color: [0, 0, 0, 255], stroke: None },
        FillItem::Mark { page: 0, mark: MarkKind::Dot, x: 130.0, y: 600.0, width: 8.0, height: 8.0, color: [0, 0, 0, 255], stroke: None },
        FillItem::Mark { page: 0, mark: MarkKind::Line, x: 72.0, y: 560.0, width: 200.0, height: 4.0, color: [0, 0, 0, 255], stroke: Some(1.0) },
    ];
    let before0 = ff_engine::list_objects(&pdfium, &corpus("sample-multipage.pdf"), 0, None).unwrap().len();
    let before1 = ff_engine::list_objects(&pdfium, &corpus("sample-multipage.pdf"), 1, None).unwrap().len();
    let n = ff_engine::apply_fill_sign(&pdfium, &corpus("sample-multipage.pdf"), &items, &out, None).unwrap();
    assert_eq!(n, items.len());
    let text = ff_engine::extract_text(&pdfium, &out, 0, None).unwrap();
    assert!(text.contains("Nguyễn Văn Điền") && text.contains("Dòng hai"), "text: {text}");
    let after0 = ff_engine::list_objects(&pdfium, &out, 0, None).unwrap();
    assert!(after0.len() >= before0 + 2 + 4, "trang 1 phải thêm 2 dòng chữ + 4 dấu: {} → {}", before0, after0.len());
    let after1 = ff_engine::list_objects(&pdfium, &out, 1, None).unwrap();
    assert_eq!(after1.len(), before1 + 1, "trang 2 phải thêm ảnh chữ ký");
    assert!(after1.iter().any(|o| matches!(o.kind, ff_engine::ObjectKind::Image)));
    // Không còn annotation nào (đã flatten vào nội dung).
    assert_eq!(ff_engine::count_annotations(&pdfium, &out, 0).unwrap_or(0), 0);
}

#[test]
fn batch_watermark_optimize_encrypt_two_files() {
    let pdfium = pdfium();
    let out_dir = std::env::temp_dir().join("signx_batch_out");
    let _ = std::fs::remove_dir_all(&out_dir);
    let files = vec![corpus("sample-multipage.pdf"), corpus("hello.pdf")];
    let steps: Vec<BatchStep> = serde_json_like();
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let res = ff_engine::run_batch(&pdfium, &files, &steps, &out_dir, "_batch", &cancel, &mut |e| events.push(e)).unwrap();
    assert_eq!(res.len(), 2);
    for r in &res {
        assert!(r.error.is_none(), "lỗi: {:?}", r.error);
        let o = PathBuf::from(&r.outputs[0]);
        assert!(o.exists() && o.file_name().unwrap().to_string_lossy().ends_with("_batch.pdf"), "{o:?}");
        // Đã mã hoá: không mật khẩu → không mở được; đúng mật khẩu → có chữ watermark.
        assert!(pdfium.load_pdf_from_file(&o, None).is_err(), "phải bị mã hoá");
        let t = ff_engine::extract_text(&pdfium, &o, 0, Some("user-pw")).unwrap();
        assert!(t.contains("CONFIDENTIAL"), "thiếu watermark: {t}");
    }
    assert!(events.iter().any(|e| matches!(e, ff_engine::BatchEvent::Finished { ok: 2, failed: 0, .. })));
    // Bước kết thúc sai chỗ → từ chối.
    let bad = vec![steps[2].clone(), steps[0].clone()];
    assert!(ff_engine::validate_steps(&bad).is_err());
    // Chuyển Text làm bước cuối.
    let conv = vec![BatchStep::RemoveMetadata, BatchStep::Convert { format: ff_engine::ConvertFormat::Text, dpi: 150.0 }];
    let r2 = ff_engine::run_batch(&pdfium, &files[..1], &conv, &out_dir, "", &cancel, &mut |_| {}).unwrap();
    assert!(r2[0].error.is_none(), "chuyển text lỗi: {:?}", r2[0].error);
    assert!(r2[0].outputs[0].ends_with(".txt") && PathBuf::from(&r2[0].outputs[0]).exists());
}

fn serde_json_like() -> Vec<BatchStep> {
    vec![
        BatchStep::Watermark { text: "CONFIDENTIAL".into(), font_size: 40.0, opacity: 0.3, rotation: 45.0, color: [200, 0, 0] },
        BatchStep::Optimize,
        BatchStep::Encrypt { user_password: "user-pw".into(), owner_password: "owner-pw".into() },
    ]
}
