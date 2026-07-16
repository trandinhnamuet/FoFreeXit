//! Test Phase 7 (OCR & Convert). OCR cần binary `tesseract` + gói `vie`/`eng`
//! (như qpdf); LibreOffice test riêng, tự bỏ qua nếu máy không có soffice.

use std::path::PathBuf;

use ff_engine::EditOp;

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

fn sample() -> PathBuf {
    workspace_root().join("corpus").join("sample-multipage.pdf")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Dựng PDF-SCAN giả lập: render 1 trang có chữ to rõ thành PNG rồi nhét PNG
/// đó vào 1 trang mới → PDF chỉ có ẢNH, extract_text = rỗng (như scan thật).
fn make_scanned_fixture(pdf: &pdfium_render::prelude::Pdfium, text: &str, out: &std::path::Path) {
    let with_text = tmp("ff_ocr_srctext.pdf");
    ff_engine::apply_edits(
        pdf,
        &sample(),
        0,
        &[EditOp::AddText {
            x: 60.0,
            y: 520.0,
            text: text.into(),
            font_size: 32.0,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        }],
        &with_text,
        None,
    )
    .expect("dựng trang chữ");
    let png = tmp("ff_ocr_scan.png");
    ff_engine::render_page_png(pdf, &with_text, 0, &png, 1700, None).expect("render scan");

    // Trang mới chỉ chứa ảnh (dùng trang 2 của sample làm nền trắng? — đơn giản:
    // đè ảnh phủ toàn trang 0 của sample; nội dung text gốc bị ảnh che nhưng
    // extract vẫn thấy → thay bằng cách XOÁ text gốc trước rồi đặt ảnh).
    let dims = ff_engine::page_dims(pdf, &sample(), None).expect("dims");
    let (w, h) = (dims[0].width_pt, dims[0].height_pt);
    // Xoá mọi object trang 0 rồi thêm ảnh full trang.
    let objs = ff_engine::list_objects(pdf, &sample(), 0, None).expect("list");
    let mut ops: Vec<EditOp> = objs.iter().map(|o| EditOp::Delete { index: o.index }).collect();
    ops.push(EditOp::AddImage {
        x: 0.0,
        y: 0.0,
        width_pt: w,
        height_pt: h,
        image_path: png.to_string_lossy().into_owned(),
    });
    ff_engine::apply_edits(pdf, &sample(), 0, &ops, out, None).expect("dựng pdf scan");

    // Xác nhận đúng chất scan: không extract được chữ.
    let t = ff_engine::extract_text(pdf, out, 0, None).expect("extract fixture");
    assert!(
        !t.to_lowercase().contains("hello") && !t.contains("OCRTARGET"),
        "fixture scan không được còn text: {t:?}"
    );
}

/// OCR tiếng Anh: PDF scan → lớp text ẩn → tìm/copy được chữ, hình ảnh giữ nguyên.
#[test]
fn ocr_makes_scanned_pdf_searchable_english() {
    let pdf = pdfium();
    let scan = tmp("ff_ocr_scan_en.pdf");
    let out = tmp("ff_ocr_out_en.pdf");
    make_scanned_fixture(&pdf, "OCRTARGET WORKS FINE", &scan);

    let n = ff_engine::ocr_add_text_layer(&pdf, &scan, &[], "eng", &out, None).expect("ocr");
    assert!(n >= 3, "phải nhận dạng được ≥3 từ, được {n}");

    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract out");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("OCRTARGET"), "từ OCR phải tìm được: {norm:?}");
    // Vị trí đúng: từ nằm quanh y≈520-552 (cỡ 32pt tại y=520).
    let hits = ff_engine::search(&pdf, &out, "OCRTARGET", false, None).expect("search");
    assert!(!hits.is_empty(), "search phải thấy từ OCR");
    let r = hits[0].rect.as_ref().expect("hit phải có rect");
    assert!(r.bottom > 480.0 && r.top < 590.0, "toạ độ lớp ẩn phải khớp vùng chữ: {r:?}");
}

/// OCR tiếng Việt: nhận dạng đúng dấu.
#[test]
fn ocr_vietnamese_diacritics() {
    let pdf = pdfium();
    let scan = tmp("ff_ocr_scan_vi.pdf");
    let out = tmp("ff_ocr_out_vi.pdf");
    make_scanned_fixture(&pdf, "Việt Nam đất nước", &scan);

    ff_engine::ocr_add_text_layer(&pdf, &scan, &[], "vie", &out, None).expect("ocr vie");
    let text = ff_engine::extract_text(&pdf, &out, 0, None).expect("extract");
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("Việt Nam"), "tiếng Việt đúng dấu: {norm:?}");
}

/// PDF → PNG mỗi trang.
#[test]
fn export_images_per_page() {
    let pdf = pdfium();
    let dir = std::env::temp_dir().join("ff_conv_imgs");
    let _ = std::fs::remove_dir_all(&dir);
    let files = ff_engine::export_images(&pdf, &sample(), &dir, 150.0, None).expect("export png");
    assert_eq!(files.len(), 3, "3 trang → 3 PNG");
    for f in &files {
        assert!(f.is_file(), "thiếu {f:?}");
        let img = image::open(f).expect("mở png");
        assert!(img.width() > 500, "150dpi phải ra ảnh đủ lớn");
    }
}

/// PDF → TXT chứa nội dung các trang.
#[test]
fn export_text_all_pages() {
    let pdf = pdfium();
    let out = tmp("ff_conv_out.txt");
    ff_engine::export_text(&pdf, &sample(), &out, None).expect("export txt");
    let t = std::fs::read_to_string(&out).expect("đọc txt");
    assert!(t.contains("Page one"), "trang 1: {t:?}");
    assert!(t.contains("Page two") || t.contains("two"), "trang 2 phải có mặt");
}

/// PDF → DOCX (tự viết): file zip hợp lệ, document.xml chứa text + ngắt trang.
#[test]
fn export_docx_basic_layout() {
    let pdf = pdfium();
    let out = tmp("ff_conv_out.docx");
    ff_engine::export_docx(&pdf, &sample(), &out, None).expect("export docx");
    let bytes = std::fs::read(&out).expect("đọc docx");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "docx phải là zip");
    // Giải nén bằng unzip hệ thống để chắc là zip CHUẨN.
    let dir = std::env::temp_dir().join("ff_docx_extract");
    let _ = std::fs::remove_dir_all(&dir);
    let ok = std::process::Command::new("unzip")
        .args(["-o", "-q", &out.to_string_lossy(), "-d", &dir.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "unzip phải giải được docx");
    let doc = std::fs::read_to_string(dir.join("word/document.xml")).expect("document.xml");
    assert!(doc.contains("Page one"), "text trang 1 trong docx");
    assert!(doc.contains(r#"<w:br w:type="page"/>"#), "có ngắt trang giữa các trang");
}

/// Office→PDF và PDF→DOCX qua LibreOffice — tự bỏ qua nếu máy không có soffice.
#[test]
fn libreoffice_round_trip_when_available() {
    if ff_engine::find_soffice().is_err() {
        eprintln!("BỎ QUA: máy không có LibreOffice (soffice)");
        return;
    }
    let pdf = pdfium();
    let dir = std::env::temp_dir().join("ff_soffice_out");
    let _ = std::fs::remove_dir_all(&dir);

    // PDF → DOCX (LibreOffice).
    let docx = ff_engine::pdf_to_docx_via_soffice(&sample(), &dir).expect("pdf→docx");
    assert!(docx.is_file());

    // DOCX (bản tự viết từ sample) → PDF: kiểm chiều Office→PDF.
    let our_docx = tmp("ff_office_src.docx");
    ff_engine::export_docx(&pdf, &sample(), &our_docx, None).expect("docx nguồn");
    let out_pdf = ff_engine::office_to_pdf(&our_docx, &dir).expect("office→pdf");
    assert!(out_pdf.is_file());
    // PDF kết quả mở được và còn nội dung.
    assert!(pdf.load_pdf_from_file(&out_pdf, None).is_ok(), "PDF từ Office phải mở được");
    let text = ff_engine::extract_text(&pdf, &out_pdf, 0, None).expect("extract");
    assert!(text.contains("Page one"), "nội dung giữ qua Office→PDF: {text:?}");
}
