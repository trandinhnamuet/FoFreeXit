//! Test "Tạo PDF" (ảnh / trắng / văn bản / gộp tệp / web) + "Xuất" (Excel,
//! PowerPoint, JPEG, TIFF nhiều trang, HTML, RTF) + OCR mở rộng (phạm vi trang,
//! bỏ trang có chữ, tiền xử lý chỉnh nghiêng, OCR thẳng ảnh).

use std::path::{Path, PathBuf};

use ff_engine::{ImagePdfOptions, Orientation, PageSizeMode};
use image::{GrayImage, Luma, Rgb, RgbImage};

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

/// Giải nén bằng `unzip` hệ thống (chắc chắn zip CHUẨN), trả thư mục.
fn unzip(file: &Path, dir_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(dir_name);
    let _ = std::fs::remove_dir_all(&dir);
    let ok = std::process::Command::new("unzip")
        .args(["-o", "-q", &file.to_string_lossy(), "-d", &dir.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "unzip phải giải được {file:?}");
    dir
}

fn read(dir: &Path, part: &str) -> String {
    std::fs::read_to_string(dir.join(part)).unwrap_or_else(|_| panic!("thiếu phần {part}"))
}

/// Ảnh thử: nền màu + khối đậm (không đơn sắc để không bị đóng gói 1-bit).
fn test_rgb(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 120]))
}

fn image_kinds(pdf: &pdfium_render::prelude::Pdfium, path: &Path, page: u16) -> usize {
    ff_engine::list_objects(pdf, path, page, None)
        .expect("list objects")
        .iter()
        .filter(|o| o.kind == ff_engine::ObjectKind::Image)
        .count()
}

// ---------------------------------------------------------------------------
// Tạo PDF
// ---------------------------------------------------------------------------

#[test]
fn images_to_pdf_fit_and_jpeg_passthrough() {
    let pdf = pdfium();
    let png = tmp("ff_create_a.png");
    let jpg = tmp("ff_create_b.jpg");
    let bmp = tmp("ff_create_c.bmp");
    test_rgb(200, 100).save(&png).unwrap();
    test_rgb(320, 240).save(&jpg).unwrap();
    test_rgb(64, 128).save(&bmp).unwrap();
    let out = tmp("ff_create_images.pdf");
    let n = ff_engine::images_to_pdf(&[png.clone(), jpg.clone(), bmp.clone()], &ImagePdfOptions::default(), &out)
        .expect("ảnh → PDF");
    assert_eq!(n, 3);
    let dims = ff_engine::page_dims(&pdf, &out, None).expect("dims");
    assert_eq!(dims.len(), 3, "3 ảnh → 3 trang");
    // Không khai DPI → 96 DPI: 200 px = 150 pt.
    assert!((dims[0].width_pt - 150.0).abs() < 0.6 && (dims[0].height_pt - 75.0).abs() < 0.6, "{:?}", dims[0]);
    assert!(dims[2].height_pt > dims[2].width_pt, "BMP dọc");
    for p in 0..3 {
        assert_eq!(image_kinds(&pdf, &out, p), 1, "trang {p} phải có 1 ảnh");
    }
    // JPEG nhúng NGUYÊN VẸN (DCTDecode, bytes y hệt tệp gốc).
    let doc = lopdf::Document::load(&out).expect("lopdf");
    let original = std::fs::read(&jpg).unwrap();
    let found = doc.objects.values().any(|o| match o {
        lopdf::Object::Stream(s) => {
            s.dict.get(b"Filter").ok().and_then(|f| f.as_name().ok()) == Some(&b"DCTDecode"[..]) && s.content == original
        }
        _ => false,
    });
    assert!(found, "JPEG phải được nhúng passthrough");
}

#[test]
fn images_to_pdf_fixed_a4_auto_orientation_and_margin() {
    let pdf = pdfium();
    let wide = tmp("ff_create_wide.png");
    let tall = tmp("ff_create_tall.png");
    test_rgb(3000, 1500).save(&wide).unwrap();
    test_rgb(1000, 2000).save(&tall).unwrap();
    let out = tmp("ff_create_a4.pdf");
    let (w, h) = ff_engine::standard_page_size("a4").unwrap();
    let opts = ImagePdfOptions {
        page: PageSizeMode::Fixed { width_pt: w, height_pt: h },
        orientation: Orientation::Auto,
        margin_pt: 36.0,
        ..Default::default()
    };
    ff_engine::images_to_pdf(&[wide, tall], &opts, &out).expect("A4");
    let dims = ff_engine::page_dims(&pdf, &out, None).unwrap();
    assert!((dims[0].width_pt - 841.89).abs() < 0.5 && (dims[0].height_pt - 595.28).abs() < 0.5, "ảnh ngang → A4 ngang");
    assert!((dims[1].width_pt - 595.28).abs() < 0.5, "ảnh dọc → A4 dọc");
    // Ảnh nằm trong lề 36pt.
    let objs = ff_engine::list_objects(&pdf, &out, 0, None).unwrap();
    let img = objs.iter().find(|o| o.kind == ff_engine::ObjectKind::Image).expect("ảnh");
    assert!(img.rect.left >= 35.5 && img.rect.right <= 841.89 - 35.5, "{:?}", img.rect);

    // Ép dọc: ảnh ngang vẫn trang dọc.
    let out2 = tmp("ff_create_a4_portrait.pdf");
    let opts2 = ImagePdfOptions { orientation: Orientation::Portrait, ..opts };
    ff_engine::images_to_pdf(&[tmp_png_wide()], &opts2, &out2).unwrap();
    let d2 = ff_engine::page_dims(&pdf, &out2, None).unwrap();
    assert!(d2[0].height_pt > d2[0].width_pt);
}

fn tmp_png_wide() -> PathBuf {
    let p = tmp("ff_create_wide2.png");
    test_rgb(400, 200).save(&p).unwrap();
    p
}

/// TIFF nhiều trang (tự ghi bằng crate tiff) → đếm đúng trang + mỗi trang 1 ảnh.
#[test]
fn multipage_tiff_to_pdf() {
    let pdf = pdfium();
    let path = tmp("ff_create_multi.tif");
    {
        use tiff::encoder::{colortype, TiffEncoder};
        let f = std::fs::File::create(&path).unwrap();
        let mut enc = TiffEncoder::new(f).unwrap();
        for k in 0..3u32 {
            let img = test_rgb(100 + k * 20, 80);
            enc.write_image::<colortype::RGB8>(img.width(), img.height(), img.as_raw()).unwrap();
        }
        // Trang 4: xám 1 kênh.
        let g = GrayImage::from_fn(50, 50, |x, _| Luma([(x * 5) as u8]));
        enc.write_image::<colortype::Gray8>(50, 50, g.as_raw()).unwrap();
    }
    assert_eq!(ff_engine::image_page_count(&path).unwrap(), 4);
    let out = tmp("ff_create_tiff.pdf");
    let n = ff_engine::images_to_pdf(&[path], &ImagePdfOptions::default(), &out).expect("tiff → pdf");
    assert_eq!(n, 4);
    let dims = ff_engine::page_dims(&pdf, &out, None).unwrap();
    assert_eq!(dims.len(), 4);
    assert!((dims[1].width_pt - 90.0).abs() < 0.6, "trang 2 rộng 120px = 90pt: {:?}", dims[1]);
    for p in 0..4 {
        assert_eq!(image_kinds(&pdf, &out, p), 1);
    }
}

#[test]
fn blank_pdf_pages_and_size() {
    let pdf = pdfium();
    let out = tmp("ff_create_blank.pdf");
    let (w, h) = ff_engine::standard_page_size("letter").unwrap();
    ff_engine::blank_pdf(h, w, 3, &out).expect("blank"); // ngang
    let dims = ff_engine::page_dims(&pdf, &out, None).unwrap();
    assert_eq!(dims.len(), 3);
    assert!((dims[0].width_pt - 792.0).abs() < 0.1 && (dims[0].height_pt - 612.0).abs() < 0.1);
    assert!(ff_engine::blank_pdf(w, h, 0, &out).is_err(), "0 trang phải lỗi");
}

#[test]
fn text_to_pdf_vietnamese_roundtrip_and_wrap() {
    let pdf = pdfium();
    let src = tmp("ff_create_vi.txt");
    let mut body = String::from("\u{feff}Xin chào Việt Nam — đường phố Hà Nội\r\n");
    body.push_str(&"Tiếng Việt có dấu đầy đủ: ắ ằ ẳ ẵ ặ ơ ư đ. ".repeat(20));
    body.push('\n');
    for i in 0..80 {
        body.push_str(&format!("Dòng số {i}\n"));
    }
    std::fs::write(&src, body).unwrap();
    let out = tmp("ff_create_vi.pdf");
    let (w, h) = ff_engine::standard_page_size("a4").unwrap();
    let opts = ff_engine::TextPdfOptions { width_pt: w, height_pt: h, ..Default::default() };
    let n = ff_engine::text_to_pdf(&src, &opts, &out).expect("txt → pdf");
    assert!(n >= 2, "80+ dòng phải sang trang, được {n}");
    let t = ff_engine::extract_text(&pdf, &out, 0, None).unwrap();
    let norm = t.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(norm.contains("Xin chào Việt Nam"), "trích lại tiếng Việt: {norm:?}");
    assert!(norm.contains("đường phố Hà Nội"), "{norm:?}");
    // Đoạn dài bị ngắt dòng: không dòng nào vượt khổ.
    let boxes = ff_engine::page_char_boxes(&pdf, &out, 0, None).unwrap();
    let max_right = boxes.iter().map(|b| b.right).fold(0.0f32, f32::max);
    assert!(max_right <= w - 50.0, "chữ vượt lề phải: {max_right}");
    let last = ff_engine::extract_text(&pdf, &out, (n - 1) as u16, None).unwrap();
    assert!(last.contains("Dòng số 79"), "dòng cuối ở trang cuối: {last:?}");
}

#[test]
fn combine_mixed_files() {
    let pdf = pdfium();
    let png = tmp("ff_combine_img.png");
    test_rgb(120, 90).save(&png).unwrap();
    let txt = tmp("ff_combine_note.txt");
    std::fs::write(&txt, "Ghi chú gộp tệp").unwrap();
    let out = tmp("ff_combine_out.pdf");
    let n = ff_engine::combine_files(&pdf, &[sample(), png, txt], &ff_engine::CombineOptions::default(), &out)
        .expect("gộp");
    assert_eq!(n, 5, "3 trang PDF + 1 ảnh + 1 văn bản");
    let t = ff_engine::extract_text(&pdf, &out, 4, None).unwrap();
    assert!(t.contains("Ghi chú gộp tệp"), "{t:?}");
    assert_eq!(image_kinds(&pdf, &out, 3), 1);
    // Tệp lạ → lỗi rõ.
    let bad = tmp("ff_combine_bad.xyz");
    std::fs::write(&bad, "x").unwrap();
    assert!(ff_engine::combine_files(&pdf, &[bad], &Default::default(), &tmp("ff_combine_bad.pdf")).is_err());
}

#[test]
fn web_source_normalization_and_browser_optional() {
    use ff_engine::create::normalize_web_source;
    assert_eq!(normalize_web_source("example.com").unwrap(), "https://example.com");
    assert_eq!(normalize_web_source("http://a.b/x").unwrap(), "http://a.b/x");
    assert!(normalize_web_source("javascript:alert(1)").is_err());
    assert!(normalize_web_source("").is_err());
    let html = tmp("ff web page.html");
    std::fs::write(&html, "<html><body><h1>Trang web thử</h1><p>Xin chào</p></body></html>").unwrap();
    let url = normalize_web_source(&html.to_string_lossy()).unwrap();
    assert!(url.starts_with("file:///") && url.contains("ff%20web%20page.html"), "{url}");

    if ff_engine::find_browser().is_err() {
        eprintln!("BỎ QUA html_to_pdf: máy không có Edge/Chrome");
        return;
    }
    let pdf = pdfium();
    let out = tmp("ff_web_out.pdf");
    ff_engine::html_to_pdf(&html.to_string_lossy(), &out, 500).expect("html → pdf");
    let t = ff_engine::extract_text(&pdf, &out, 0, None).unwrap();
    assert!(t.contains("Xin chào"), "{t:?}");
}

// ---------------------------------------------------------------------------
// Xuất
// ---------------------------------------------------------------------------

/// PDF có bảng 3 cột (Helvetica, vị trí cố định) dựng bằng lopdf.
fn make_table_pdf(path: &Path) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let rows = [
        ("Name", "Qty", "Price"),
        ("Apple", "12", "3.50"),
        ("Banana", "7", "1,234.75"),
        ("Cherry", "0123", "(5)"),
    ];
    let mut content = String::from("BT /F1 12 Tf\n");
    content.push_str("1 0 0 1 72 760 Tm (Report title spanning) Tj\n");
    for (i, (a, b, c)) in rows.iter().enumerate() {
        let y = 700 - i as i32 * 20;
        content.push_str(&format!("1 0 0 1 72 {y} Tm ({a}) Tj 1 0 0 1 250 {y} Tm ({b}) Tj 1 0 0 1 400 {y} Tm ({c}) Tj\n"));
    }
    content.push_str("ET");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

#[test]
fn number_cell_parsing() {
    use ff_engine::export::parse_number_cell as p;
    assert_eq!(p("12"), Some(12.0));
    assert_eq!(p("3.50"), Some(3.5));
    assert_eq!(p("1,234.75"), Some(1234.75));
    assert_eq!(p("1.234,75"), Some(1234.75));
    assert_eq!(p("1.234.567"), Some(1234567.0));
    assert_eq!(p("2,5"), Some(2.5));
    assert_eq!(p("(5)"), Some(-5.0));
    assert_eq!(p("-7"), Some(-7.0));
    assert_eq!(p("12%"), Some(0.12));
    assert_eq!(p("0123"), None, "mã có số 0 đầu giữ chuỗi");
    assert_eq!(p("abc"), None);
    assert_eq!(p("1.2.3"), None);
}

#[test]
fn export_xlsx_table_structure_and_package() {
    let pdf = pdfium();
    let src = tmp("ff_table.pdf");
    make_table_pdf(&src);
    let cells = ff_engine::export::page_table(&pdf, &src, 0, None).unwrap();
    let at = |r: usize, c: usize| cells.iter().find(|x| x.row == r && x.col == c).map(|x| x.text.clone());
    assert_eq!(at(1, 0).as_deref(), Some("Name"), "{cells:?}");
    assert_eq!(at(1, 2).as_deref(), Some("Price"));
    assert_eq!(at(2, 1).as_deref(), Some("12"));
    assert_eq!(at(3, 2).as_deref(), Some("1,234.75"));
    assert_eq!(at(0, 0).as_deref(), Some("Report title spanning"), "dòng tiêu đề vào cột A");

    let out = tmp("ff_table.xlsx");
    ff_engine::export_xlsx(&pdf, &src, &out, &[], "Trang", None).expect("xlsx");
    let dir = unzip(&out, "ff_xlsx_extract");
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/styles.xml",
        "xl/sharedStrings.xml",
        "xl/worksheets/sheet1.xml",
    ] {
        assert!(dir.join(part).is_file(), "thiếu {part}");
    }
    let ct = read(&dir, "[Content_Types].xml");
    assert!(ct.contains("/xl/worksheets/sheet1.xml") && ct.contains("sharedStrings+xml"));
    let wb = read(&dir, "xl/workbook.xml");
    assert!(wb.contains(r#"name="Trang 1""#), "{wb}");
    let sst = read(&dir, "xl/sharedStrings.xml");
    for s in ["Name", "Apple", "Banana", "0123"] {
        assert!(sst.contains(&format!(">{s}<")), "chuỗi {s} trong sharedStrings");
    }
    let sheet = read(&dir, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<c r="B3"><v>12</v></c>"#), "Qty là ô SỐ: {sheet}");
    assert!(sheet.contains(r#"<c r="C3"><v>3.5</v></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="C4"><v>1234.75</v></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="C5"><v>-5</v></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="B5" t="s">"#), "0123 là chuỗi");

    // Nhiều trang → nhiều sheet; phạm vi trang.
    let out2 = tmp("ff_sample.xlsx");
    ff_engine::export_xlsx(&pdf, &sample(), &out2, &[0, 2], "Page", None).unwrap();
    let d2 = unzip(&out2, "ff_xlsx_extract2");
    assert!(d2.join("xl/worksheets/sheet2.xml").is_file() && !d2.join("xl/worksheets/sheet3.xml").exists());
    let wb2 = read(&d2, "xl/workbook.xml");
    assert!(wb2.contains(r#"name="Page 1""#) && wb2.contains(r#"name="Page 3""#), "{wb2}");
}

#[test]
fn export_pptx_package_and_text() {
    let pdf = pdfium();
    let out = tmp("ff_sample.pptx");
    ff_engine::export_pptx(&pdf, &sample(), &out, &[], &ff_engine::PptxOptions::default(), None).expect("pptx");
    let dir = unzip(&out, "ff_pptx_extract");
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slideMasters/slideMaster1.xml",
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        "ppt/slideLayouts/slideLayout1.xml",
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        "ppt/theme/theme1.xml",
        "ppt/presProps.xml",
        "ppt/viewProps.xml",
        "ppt/tableStyles.xml",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide3.xml",
        "ppt/slides/_rels/slide1.xml.rels",
        "ppt/media/image1.jpeg",
    ] {
        assert!(dir.join(part).is_file(), "thiếu {part}");
    }
    let pres = read(&dir, "ppt/presentation.xml");
    assert_eq!(pres.matches("<p:sldId ").count(), 3, "3 slide");
    assert!(pres.contains("<p:sldMasterId ") && pres.contains("<p:sldSz "));
    let rels = read(&dir, "ppt/_rels/presentation.xml.rels");
    assert!(rels.contains("slideMasters/slideMaster1.xml") && rels.contains("theme/theme1.xml"));
    let slide = read(&dir, "ppt/slides/slide1.xml");
    assert!(slide.contains("<p:pic>") && slide.contains(r#"r:embed="rId2""#));
    assert!(slide.contains("Page one"), "hộp chữ sửa được: {slide}");
    // Mọi XML con phải well-formed tối thiểu (mở/đóng thẻ gốc).
    assert!(slide.trim_end().ends_with("</p:sld>"));
    assert!(image::open(dir.join("ppt/media/image1.jpeg")).is_ok());

    // Chế độ chỉ ảnh + phạm vi trang.
    let out2 = tmp("ff_sample_pic.pptx");
    let opts = ff_engine::PptxOptions { editable_text: false, dpi: 96.0 };
    ff_engine::export_pptx(&pdf, &sample(), &out2, &[1], &opts, None).unwrap();
    let d2 = unzip(&out2, "ff_pptx_extract2");
    let s2 = read(&d2, "ppt/slides/slide1.xml");
    assert!(!s2.contains("<a:t>"), "chế độ ảnh không có hộp chữ");
    assert!(!d2.join("ppt/slides/slide2.xml").exists());
}

#[test]
fn export_jpeg_and_multipage_tiff() {
    let pdf = pdfium();
    let dir = std::env::temp_dir().join("ff_export_jpg");
    let _ = std::fs::remove_dir_all(&dir);
    let files = ff_engine::export_page_images(
        &pdf,
        &sample(),
        &dir,
        &[1],
        100.0,
        ff_engine::RasterFormat::Jpeg { quality: 60 },
        None,
    )
    .expect("jpeg");
    assert_eq!(files.len(), 1);
    assert!(files[0].to_string_lossy().ends_with("-p2.jpg"), "{:?}", files[0]);
    let img = image::open(&files[0]).expect("mở jpg");
    assert!(img.width() > 700 && img.width() < 900, "100 DPI ≈ 827px: {}", img.width());

    let tif = tmp("ff_export.tif");
    let n = ff_engine::export_tiff(&pdf, &sample(), &tif, &[], 72.0, None).expect("tiff");
    assert_eq!(n, 3);
    assert_eq!(ff_engine::image_page_count(&tif).unwrap(), 3, "TIFF nhiều trang");
    // Vòng lại: TIFF → PDF ra đúng 3 trang.
    let back = tmp("ff_export_tif_back.pdf");
    assert_eq!(ff_engine::images_to_pdf(&[tif], &ImagePdfOptions::default(), &back).unwrap(), 3);
}

#[test]
fn export_html_rtf_text_ranges() {
    let pdf = pdfium();
    let h1 = tmp("ff_export_pos.html");
    ff_engine::export_html(&pdf, &sample(), &h1, &[], ff_engine::HtmlMode::Positioned, 96.0, None).expect("html");
    let s = std::fs::read_to_string(&h1).unwrap();
    assert!(s.starts_with("<!DOCTYPE html>") && s.contains("Page one") && s.contains("data:image/jpeg;base64,"));
    assert_eq!(s.matches(r#"class="page""#).count(), 3);

    let h2 = tmp("ff_export_flow.html");
    ff_engine::export_html(&pdf, &sample(), &h2, &[0], ff_engine::HtmlMode::Flowing, 96.0, None).expect("html flow");
    let s2 = std::fs::read_to_string(&h2).unwrap();
    assert!(s2.contains("Page one") && s2.matches(r#"class="flow""#).count() == 1);

    let rtf = tmp("ff_export.rtf");
    ff_engine::export_rtf(&pdf, &sample(), &rtf, &[], None).expect("rtf");
    let r = std::fs::read_to_string(&rtf).unwrap();
    assert!(r.starts_with("{\\rtf1") && r.contains("Page one") && r.contains("\\page"));

    let txt = tmp("ff_export_range.txt");
    ff_engine::export_text_range(&pdf, &sample(), &txt, &[1], None).unwrap();
    let t = std::fs::read_to_string(&txt).unwrap();
    assert!(!t.contains("Page one"), "chỉ trang 2: {t:?}");

    let docx = tmp("ff_export_range.docx");
    ff_engine::export_docx_pages(&pdf, &sample(), &docx, &[0], None).unwrap();
    let d = unzip(&docx, "ff_docx_range");
    let doc = read(&d, "word/document.xml");
    assert!(doc.contains("Page one") && !doc.contains(r#"w:type="page""#), "1 trang → không ngắt trang");
}

// ---------------------------------------------------------------------------
// OCR: tiền xử lý + tuỳ chọn
// ---------------------------------------------------------------------------

/// Ảnh "trang chữ" tổng hợp: các dòng gồm nhiều "từ" (khối đen) đi LÊN về bên
/// phải góc `deg` (y hướng xuống).
fn synthetic_skewed(deg: f32) -> GrayImage {
    let (w, h) = (1400u32, 1000u32);
    let mut img = GrayImage::from_pixel(w, h, Luma([255]));
    let t = deg.to_radians().tan();
    for line in 0..14 {
        let y0 = 150.0 + line as f32 * 50.0;
        for x in 150..1250u32 {
            // Khoảng trắng giữa các "từ".
            if (x / 40) % 5 == 4 {
                continue;
            }
            let yc = y0 - x as f32 * t;
            for dy in 0..14 {
                let y = (yc + dy as f32) as i32;
                if y >= 0 && (y as u32) < h {
                    img.put_pixel(x, y as u32, Luma([0]));
                }
            }
        }
    }
    img
}

#[test]
fn deskew_estimates_and_corrects_rotation() {
    use ff_engine::ocr::{estimate_skew_deg, rotate_gray};
    for deg in [3.0f32, -2.0, 0.0] {
        let img = synthetic_skewed(deg);
        let est = estimate_skew_deg(&img);
        assert!((est - deg).abs() <= 0.3, "góc thật {deg}, ước lượng {est}");
        let fixed = rotate_gray(&img, est);
        let after = estimate_skew_deg(&fixed);
        assert!(after.abs() <= 0.3, "sau chỉnh nghiêng phải ≈0, được {after} (từ {deg})");
    }
    // Otsu tách được nền/chữ.
    let g = GrayImage::from_fn(100, 100, |x, _| Luma([if x < 30 { 40 } else { 220 }]));
    let th = ff_engine::ocr::otsu_threshold(&g);
    assert!((40..220).contains(&th), "ngưỡng {th}");
}

#[test]
fn ocr_skips_pages_with_text() {
    if ff_engine::find_tesseract().is_err() {
        eprintln!("BỎ QUA: không có tesseract");
        return;
    }
    let pdf = pdfium();
    let out = tmp("ff_ocr_skip.pdf");
    let opts = ff_engine::OcrOptions {
        lang: "eng".into(),
        pages: vec![0, 1],
        skip_text_pages: true,
        preprocess: true,
    };
    let r = ff_engine::ocr_document(&pdf, &sample(), &opts, &out, None).expect("ocr");
    assert_eq!(r.pages_skipped, 2, "trang đã có chữ phải bỏ qua: {r:?}");
    assert_eq!(r.pages_ocred, 0);
    assert_eq!(ff_engine::page_count(&pdf, &out, None).unwrap(), 3, "giữ nguyên tài liệu");
}

/// OCR thẳng ẢNH scan NGHIÊNG: ảnh → PDF → tiền xử lý chỉnh nghiêng → chữ tìm được.
#[test]
fn ocr_image_file_with_deskew() {
    if ff_engine::find_tesseract().is_err() {
        eprintln!("BỎ QUA: không có tesseract");
        return;
    }
    let pdf = pdfium();
    // Trang chữ to rõ → ảnh → xoay nghiêng 2.5° (giả scan lệch).
    let with_text = tmp("ff_ocrimg_src.pdf");
    ff_engine::apply_edits(
        &pdf,
        &sample(),
        0,
        &[ff_engine::EditOp::AddText {
            x: 60.0,
            y: 420.0,
            text: "SKEWTARGET READABLE".into(),
            font_size: 30.0,
            color: [0, 0, 0, 255],
            font_family: None,
            bold: false,
            italic: false,
        }],
        &with_text,
        None,
    )
    .unwrap();
    let png = tmp("ff_ocrimg_flat.png");
    let rendered = ff_engine::render_page_png(&pdf, &with_text, 0, &png, 1700, None).unwrap();
    let gray = rendered.image.to_luma8();
    let skewed = ff_engine::ocr::rotate_gray(&gray, -2.5);
    let scan = tmp("ff_ocrimg_scan.png");
    skewed.save(&scan).unwrap();

    let out = tmp("ff_ocrimg_out.pdf");
    let opts = ff_engine::OcrOptions { lang: "eng".into(), preprocess: true, ..Default::default() };
    let r = ff_engine::ocr_images_to_pdf(&pdf, &[scan], &ImagePdfOptions::default(), &opts, &out).expect("ocr ảnh");
    assert!(r.words >= 2, "{r:?}");
    let text = ff_engine::extract_text(&pdf, &out, 0, None).unwrap();
    assert!(text.contains("SKEWTARGET"), "chữ từ ảnh scan nghiêng: {text:?}");
    let hits = ff_engine::search(&pdf, &out, "SKEWTARGET", false, None).unwrap();
    assert!(!hits.is_empty());
}
