//! Test bảo mật/làm sạch tài liệu (docsec): Search & Redact (mẫu tiếng Việt +
//! Luhn, cụm vắt dòng, chữ phủ, nguyên trang), Sanitize (từng hạng mục bị xoá
//! thật, file vẫn mở được), Đính kèm (round-trip tên tiếng Việt), Thuộc tính
//! (ghi/đọc Info + XMP + Initial View, đọc lại bằng PDFium), Trợ năng (kiểm
//! tra + sửa tự động + Alt text trên fixture gắn thẻ).

use std::path::{Path, PathBuf};

use ff_engine::*;
use lopdf::{dictionary, Dictionary, Document, Object, Stream, StringFormat};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
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

fn add_text(x: f32, y: f32, s: &str) -> EditOp {
    EditOp::AddText { x, y, text: s.into(), font_size: 12.0, color: [0, 0, 0, 255], font_family: None, bold: false, italic: false }
}

fn all_text(pdf: &pdfium_render::prelude::Pdfium, p: &Path) -> String {
    let n = page_count(pdf, p, None).unwrap();
    (0..n).map(|i| extract_text(pdf, p, i, None).unwrap()).collect::<Vec<_>>().join("\n")
}

// ------------------------------------------------------------ Search & Redact

fn redact_fixture(pdf: &pdfium_render::prelude::Pdfium) -> PathBuf {
    let fx = tmp("ff_docsec_sr_fx.pdf");
    apply_edits(
        pdf,
        &sample(),
        0,
        &[
            add_text(60.0, 500.0, "Liên hệ: nguyen.van.an@congty.vn"),
            add_text(60.0, 480.0, "Điện thoại 0912 345 678, CCCD 001203004567"),
            add_text(60.0, 460.0, "Thẻ 4111 1111 1111 1111 và thẻ sai 4111 1111 1111 1112"),
            add_text(60.0, 440.0, "Người ký: ông Nguyễn Văn"),
            add_text(60.0, 425.0, "An, giám đốc"),
        ],
        &fx,
        None,
    )
    .expect("fixture");
    fx
}

#[test]
fn search_redact_finds_vietnamese_patterns_with_rects() {
    let pdf = pdfium();
    let fx = redact_fixture(&pdf);
    let spec = RedactSearchSpec {
        patterns: vec![RedactPattern::Email, RedactPattern::Phone, RedactPattern::Cccd, RedactPattern::BankCard],
        ..Default::default()
    };
    let hits = search_redact(&pdf, &fx, &spec, None).expect("search");
    let texts: Vec<&str> = hits.iter().map(|h| h.text.as_str()).collect();
    assert!(texts.contains(&"nguyen.van.an@congty.vn"), "{texts:?}");
    assert!(texts.contains(&"0912 345 678"), "{texts:?}");
    assert!(texts.contains(&"001203004567"), "{texts:?}");
    assert!(texts.contains(&"4111 1111 1111 1111"), "{texts:?}");
    assert!(!texts.contains(&"4111 1111 1111 1112"), "số thẻ sai Luhn không được khớp: {texts:?}");
    for h in &hits {
        assert_eq!(h.page_index, 0);
        assert_eq!(h.rects.len(), 1, "{h:?}");
        let r = h.rects[0];
        assert!(r.right > r.left && r.top > r.bottom);
    }
    let email = hits.iter().find(|h| h.source == "email").unwrap();
    assert!(email.rects[0].bottom > 490.0 && email.rects[0].bottom < 510.0, "{:?}", email.rects);

    // Cụm vắt 2 dòng → 2 hộp; không phân biệt hoa thường.
    let spec = RedactSearchSpec { terms: vec!["nguyễn văn an".into()], ..Default::default() };
    let hits = search_redact(&pdf, &fx, &spec, None).expect("search");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].rects.len(), 2, "cụm vắt dòng phải có 2 hộp: {:?}", hits[0].rects);
    // Whole word + match case.
    let spec = RedactSearchSpec { terms: vec!["An".into()], match_case: true, whole_word: true, ..Default::default() };
    assert_eq!(search_redact(&pdf, &fx, &spec, None).unwrap().len(), 1);
    let spec = RedactSearchSpec { terms: vec!["Vă".into()], whole_word: true, ..Default::default() };
    assert!(search_redact(&pdf, &fx, &spec, None).unwrap().is_empty());
    // Regex người dùng.
    let spec = RedactSearchSpec { regex: Some(r"CCCD\s+\d+".into()), ..Default::default() };
    let hits = search_redact(&pdf, &fx, &spec, None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "CCCD 001203004567");
}

#[test]
fn styled_redaction_overlay_and_whole_page() {
    let pdf = pdfium();
    let fx = redact_fixture(&pdf);
    let spec = RedactSearchSpec { patterns: vec![RedactPattern::Email, RedactPattern::Cccd], ..Default::default() };
    let hits = search_redact(&pdf, &fx, &spec, None).unwrap();
    let rects: Vec<Rect> = hits.iter().flat_map(|h| h.rects.iter().copied()).collect();
    let style = RedactStyle {
        fill: [200, 0, 0],
        overlay_text: Some("[ĐÃ XOÁ]".into()),
        text_color: [255, 255, 255],
        font_size: None,
        repeat: false,
        align: RedactAlign::Center,
    };
    let out = tmp("ff_docsec_sr_out.pdf");
    let n = redact_areas_styled(&pdf, &fx, 0, &rects, false, &style, &out, None).expect("redact");
    assert!(n >= 2);
    // PDFium trên Windows trả dấu cách của chữ phủ thành NBSP — chuẩn hoá trước khi so.
    let t = extract_text(&pdf, &out, 0, None).unwrap().replace('\u{a0}', " ");
    assert!(!t.contains("nguyen.van.an"), "email còn: {t}");
    assert!(!t.contains("001203004567"), "CCCD còn: {t}");
    assert!(t.contains("[ĐÃ XOÁ]"), "thiếu chữ phủ: {t}");
    assert!(t.contains("0912 345 678"), "nội dung ngoài vùng phải giữ: {t}");
    // Màu tô đúng (render: pixel giữa vùng CCCD đỏ).
    let img = render::render_page(&pdf, &out, 0, 612, None).unwrap();
    let dyn_img = img.image.to_rgba8();
    let scale = dyn_img.width() as f32 / 612.0;
    let cccd = hits.iter().find(|h| h.source == "cccd").unwrap().rects[0];
    // Góc (tránh chữ phủ ở giữa).
    let px = ((cccd.left + 1.5) * scale) as u32;
    let page_h = dyn_img.height() as f32 / scale;
    let py = ((page_h - cccd.bottom - 1.5) * scale) as u32;
    let p = dyn_img.get_pixel(px, py);
    assert!(p[0] > 150 && p[1] < 60 && p[2] < 60, "pixel {:?}", p);

    // Lặp chữ phủ: nhiều bản sao trong vùng lớn.
    let big = Rect { left: 50.0, bottom: 100.0, right: 400.0, top: 200.0 };
    let rep = RedactStyle { repeat: true, font_size: Some(10.0), overlay_text: Some("MẬT".into()), ..Default::default() };
    let out3 = tmp("ff_docsec_sr_rep.pdf");
    redact_areas_styled(&pdf, &fx, 0, &[big], false, &rep, &out3, None).unwrap();
    let t3 = extract_text(&pdf, &out3, 0, None).unwrap();
    assert!(t3.matches("MẬT").count() > 10, "{t3}");

    // Nguyên trang: mọi chữ gốc biến mất.
    let out2 = tmp("ff_docsec_sr_page.pdf");
    redact_areas_styled(&pdf, &fx, 0, &[], true, &RedactStyle::default(), &out2, None).unwrap();
    let t2 = extract_text(&pdf, &out2, 0, None).unwrap();
    assert!(t2.trim().is_empty(), "trang còn chữ: {t2:?}");
    assert_eq!(page_count(&pdf, &out2, None).unwrap(), page_count(&pdf, &fx, None).unwrap());
}

// ------------------------------------------------------------ fixture lopdf

fn text_str(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(b, StringFormat::Hexadecimal)
    }
}

/// Tài liệu 2 trang đủ mọi thứ "ẩn": Info + XMP, comment + popup, link,
/// widget có AP, FileAttachment, EmbeddedFiles, JavaScript (name tree +
/// OpenAction + /AA), lớp OC đang tắt, dấu trang, chữ Tr 3, /Thumb, /PieceInfo.
fn rich_fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let ocg = doc.add_object(dictionary! { "Type" => "OCG", "Name" => text_str("Lớp ẩn") });
    let content = b"BT /F1 24 Tf 72 700 Td (VISIBLETEXT) Tj ET\n/OC /L1 BDC BT /F1 24 Tf 72 600 Td (LAYERSECRET) Tj ET EMC\nBT 3 Tr /F1 12 Tf 72 500 Td (HIDDENOCR) Tj ET\n".to_vec();
    let c1 = doc.add_object(Stream::new(Dictionary::new(), content));
    let ap = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } } },
        b"BT /F1 12 Tf 2 5 Td (FIELDVALUE) Tj ET".to_vec(),
    ));
    let js_action = doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("app.alert('SECRETJS')") });
    let widget = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => Object::string_literal("ho_ten"),
        "V" => Object::string_literal("FIELDVALUE"), "Rect" => vec![72.into(), 300.into(), 272.into(), 320.into()],
        "AP" => dictionary! { "N" => ap }, "AA" => dictionary! { "K" => js_action }, "F" => 4,
    });
    let popup = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Popup", "Rect" => vec![300.into(), 600.into(), 400.into(), 700.into()] });
    let note = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => vec![300.into(), 700.into(), 320.into(), 720.into()],
        "Contents" => text_str("COMMENTSECRET bình luận"), "Popup" => popup,
    });
    let link = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => vec![72.into(), 690.into(), 250.into(), 730.into()],
        "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") },
    });
    let att_data = doc.add_object(Stream::new(dictionary! { "Type" => "EmbeddedFile" }, b"ATTACHSECRET".to_vec()));
    let fa = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "FileAttachment", "Rect" => vec![500.into(), 700.into(), 520.into(), 720.into()],
        "FS" => dictionary! { "Type" => "Filespec", "F" => Object::string_literal("ghim.txt"), "EF" => dictionary! { "F" => att_data } },
    });
    let thumb = doc.add_object(Stream::new(dictionary! { "Width" => 1, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 }, vec![0]));
    let p1 = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c1,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font }, "Properties" => dictionary! { "L1" => ocg } },
        "Annots" => vec![note.into(), popup.into(), link.into(), widget.into(), fa.into()],
        "AA" => dictionary! { "O" => js_action }, "Thumb" => thumb,
        "PieceInfo" => dictionary! { "Illustrator" => dictionary! { "Private" => Object::string_literal("PRIVATEDATA") } },
    });
    let c2 = doc.add_object(Stream::new(Dictionary::new(), b"BT /F1 18 Tf 72 700 Td (PAGE TWO) Tj ET".to_vec()));
    let p2 = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c2, "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![p1.into(), p2.into()], "Count" => 2 }));
    let emb = doc.add_object(Stream::new(dictionary! { "Type" => "EmbeddedFile" }, b"EMBEDSECRET".to_vec()));
    let fs = doc.add_object(dictionary! { "Type" => "Filespec", "F" => Object::string_literal("a.txt"), "UF" => text_str("tệp.txt"), "EF" => dictionary! { "F" => emb } });
    let js_doc = doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("app.alert('DOCJS')") });
    let o2 = doc.new_object_id();
    let o1 = doc.add_object(dictionary! { "Title" => text_str("Chương 1"), "Parent" => o2, "Dest" => vec![p1.into(), "Fit".into()] });
    doc.objects.insert(o2, Object::Dictionary(dictionary! { "Type" => "Outlines", "First" => o1, "Last" => o1, "Count" => 1 }));
    let acro = doc.add_object(dictionary! { "Fields" => vec![widget.into()], "DA" => Object::string_literal("/Helv 0 Tf 0 g") });
    let xmp = doc.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/" xmlns:dc="http://purl.org/dc/elements/1.1/" pdf:Keywords="XMPOLDKEY"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">XMPOLDTITLE</rdf:li></rdf:Alt></dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#.to_vec(),
    ).with_compression(false));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "Outlines" => o2, "AcroForm" => acro, "Metadata" => xmp,
        "OpenAction" => js_doc,
        "Names" => dictionary! {
            "EmbeddedFiles" => dictionary! { "Names" => vec![Object::string_literal("a.txt"), fs.into()] },
            "JavaScript" => dictionary! { "Names" => vec![Object::string_literal("init"), js_doc.into()] },
        },
        "OCProperties" => dictionary! { "OCGs" => vec![ocg.into()], "D" => dictionary! { "OFF" => vec![ocg.into()], "Order" => vec![ocg.into()] } },
        "PieceInfo" => dictionary! { "App" => dictionary! { "Private" => Object::string_literal("PRIVATEDATA2") } },
    });
    let info = doc.add_object(dictionary! { "Author" => text_str("Nguyễn Văn A"), "Title" => Object::string_literal("OLDTITLE"), "Producer" => Object::string_literal("FixtureMaker") });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    doc.save(path).expect("save fixture");
}

fn raw_contains(p: &Path, needle: &str) -> bool {
    // Giải nén mọi stream để tìm cả trong stream nén.
    let doc = Document::load(p).unwrap();
    if std::fs::read(p).unwrap().windows(needle.len()).any(|w| w == needle.as_bytes()) {
        return true;
    }
    doc.objects.values().any(|o| match o {
        Object::Stream(s) => s.decompressed_content().unwrap_or_else(|_| s.content.clone()).windows(needle.len()).any(|w| w == needle.as_bytes()),
        _ => false,
    })
}

#[test]
fn sanitize_examines_and_removes_every_category() {
    let pdf = pdfium();
    let fx = tmp("ff_docsec_rich.pdf");
    rich_fixture(&fx);
    let r = examine_document(&fx).expect("examine");
    assert!(r.metadata >= 2, "{r:?}");
    assert_eq!(r.comments, 2, "comment + FileAttachment: {r:?}");
    assert_eq!(r.links, 1, "{r:?}");
    assert_eq!(r.form_fields, 1, "{r:?}");
    assert!(r.attachments >= 2, "{r:?}");
    assert!(r.javascript >= 3, "{r:?}");
    assert_eq!(r.hidden_layers, 1, "{r:?}");
    assert_eq!(r.bookmarks, 1, "{r:?}");
    assert_eq!(r.hidden_text, 1, "{r:?}");
    assert_eq!(r.thumbnails, 1, "{r:?}");
    assert_eq!(r.private_data, 2, "{r:?}");

    // Làm sạch toàn bộ (form làm phẳng, kèm chữ ẩn).
    let out = tmp("ff_docsec_rich_clean.pdf");
    let opts = SanitizeOptions { hidden_text: true, ..SanitizeOptions::all() };
    sanitize_document(&fx, &out, &opts).expect("sanitize");
    assert_eq!(page_count(&pdf, &out, None).unwrap(), 2, "file phải mở được");
    let again = examine_document(&out).unwrap();
    assert_eq!(
        (again.metadata, again.comments, again.links, again.form_fields, again.attachments, again.javascript),
        (0, 0, 0, 0, 0, 0),
        "{again:?}"
    );
    assert_eq!((again.hidden_layers, again.bookmarks, again.hidden_text, again.thumbnails, again.private_data), (0, 0, 0, 0, 0), "{again:?}");
    for secret in ["SECRETJS", "DOCJS", "COMMENTSECRET", "ATTACHSECRET", "EMBEDSECRET", "LAYERSECRET", "HIDDENOCR", "PRIVATEDATA", "XMPOLDTITLE", "FixtureMaker"] {
        assert!(!raw_contains(&out, secret), "{secret} vẫn còn trong file");
    }
    // Nội dung hiển thị giữ nguyên; field được "in" vào trang.
    let t = all_text(&pdf, &out);
    assert!(t.contains("VISIBLETEXT") && t.contains("PAGE TWO"), "{t}");
    assert!(t.contains("FIELDVALUE"), "field phải được làm phẳng vào trang: {t}");
    assert!(sanitize::annotation_subtypes(&out).unwrap().is_empty());

    // Chỉ một hạng mục: giữ phần còn lại.
    let out2 = tmp("ff_docsec_rich_js.pdf");
    let only_js = SanitizeOptions { javascript: true, ..Default::default() };
    sanitize_document(&fx, &out2, &only_js).unwrap();
    let r2 = examine_document(&out2).unwrap();
    assert_eq!(r2.javascript, 0, "{r2:?}");
    assert_eq!((r2.comments, r2.links, r2.form_fields, r2.bookmarks), (2, 1, 1, 1), "{r2:?}");
    assert!(!raw_contains(&out2, "SECRETJS") && !raw_contains(&out2, "DOCJS"));
    // Form: xoá hẳn (không làm phẳng).
    let out3 = tmp("ff_docsec_rich_forms.pdf");
    sanitize_document(&fx, &out3, &SanitizeOptions { forms: FormAction::Remove, ..Default::default() }).unwrap();
    assert!(!all_text(&pdf, &out3).contains("FIELDVALUE"));
    assert_eq!(examine_document(&out3).unwrap().form_fields, 0);
    // Lớp ẩn: nội dung lớp tắt biến mất, chữ thường còn.
    let out4 = tmp("ff_docsec_rich_oc.pdf");
    sanitize_document(&fx, &out4, &SanitizeOptions { hidden_layers: true, ..Default::default() }).unwrap();
    assert!(!raw_contains(&out4, "LAYERSECRET"));
    assert!(all_text(&pdf, &out4).contains("VISIBLETEXT"));
    assert!(raw_contains(&out4, "HIDDENOCR"), "chữ ẩn không chọn thì phải giữ");
}

// ------------------------------------------------------------ Attachments

#[test]
fn attachments_roundtrip_vietnamese_names() {
    let pdf = pdfium();
    let src = tmp("ff_docsec_att_src.txt");
    std::fs::write(&src, "xin chào — tệp đính kèm").unwrap();
    let out = tmp("ff_docsec_att.pdf");
    let list = apply_attachment_ops(
        &sample(),
        &[
            AttachmentOp::Add { path: src.clone(), name: Some("Báo cáo tài chính.txt".into()), description: "Mô tả tiếng Việt".into() },
            AttachmentOp::Add { path: src.clone(), name: Some("Báo cáo tài chính.txt".into()), description: String::new() },
        ],
        &out,
    )
    .expect("add");
    assert_eq!(page_count(&pdf, &out, None).unwrap(), page_count(&pdf, &sample(), None).unwrap());
    let list2 = list_attachments(&out).unwrap();
    assert_eq!(list, list2);
    assert_eq!(list2.len(), 2, "{list2:?}");
    let a = list2.iter().find(|a| a.file_name == "Báo cáo tài chính.txt").expect("tên tiếng Việt");
    assert_eq!(a.description, "Mô tả tiếng Việt");
    assert_eq!(a.size, Some(std::fs::metadata(&src).unwrap().len()));
    assert_eq!(a.mime.as_deref(), Some("text/plain"));
    assert!(a.mod_date.is_some());
    assert!(list2.iter().any(|a| a.file_name == "Báo cáo tài chính (2).txt"), "trùng tên → hậu tố: {list2:?}");
    // Trích xuất đúng bytes.
    let ex = tmp("ff_docsec_att_ex.txt");
    extract_attachment(&out, &a.key, &ex).unwrap();
    assert_eq!(std::fs::read(&ex).unwrap(), std::fs::read(&src).unwrap());
    // Sửa mô tả + xoá.
    let out2 = tmp("ff_docsec_att2.pdf");
    let l3 = apply_attachment_ops(
        &out,
        &[
            AttachmentOp::SetDescription { key: a.key.clone(), description: "Đã sửa".into() },
            AttachmentOp::Delete { key: "Báo cáo tài chính (2).txt".into() },
        ],
        &out2,
    )
    .unwrap();
    assert_eq!(l3.len(), 1);
    assert_eq!(l3[0].description, "Đã sửa");
    // PDFium thấy đúng đính kèm.
    let d = pdf.load_pdf_from_file(&out2, None).unwrap();
    assert_eq!(d.attachments().len(), 1);
    // Xoá hết → name tree biến mất.
    let out3 = tmp("ff_docsec_att3.pdf");
    assert!(apply_attachment_ops(&out2, &[AttachmentOp::Delete { key: a.key.clone() }], &out3).unwrap().is_empty());
    assert!(!raw_contains(&out3, "EmbeddedFiles"));
    // FileAttachment annot liệt kê chỉ-đọc.
    let fx = tmp("ff_docsec_rich_att.pdf");
    rich_fixture(&fx);
    let l = list_attachments(&fx).unwrap();
    assert!(l.iter().any(|x| x.file_name == "tệp.txt" && x.page_index.is_none()), "{l:?}");
    let ann = l.iter().find(|x| x.page_index == Some(0)).expect("annot");
    assert_eq!(ann.file_name, "ghim.txt");
    let ex2 = tmp("ff_docsec_att_ex2.txt");
    extract_attachment(&fx, &ann.key, &ex2).unwrap();
    assert_eq!(std::fs::read(&ex2).unwrap(), b"ATTACHSECRET");
    assert!(is_risky_file_name("setup.EXE") && !is_risky_file_name("bao-cao.pdf"));
}

// ------------------------------------------------------------ Properties

#[test]
fn properties_write_read_info_xmp_and_initial_view() {
    let pdf = pdfium();
    let fx = tmp("ff_docsec_props_fx.pdf");
    rich_fixture(&fx);
    let p = read_properties(Some(&pdf), &fx, None).unwrap();
    assert_eq!(p.title, "OLDTITLE");
    assert_eq!(p.author, "Nguyễn Văn A");
    assert_eq!(p.keywords, "XMPOLDKEY", "từ khoá lấy từ XMP khi Info thiếu");
    assert_eq!(p.producer, "FixtureMaker");
    assert_eq!(p.page_count, 2);
    assert!((p.page_width - 612.0).abs() < 0.1);
    assert!(p.has_xmp && !p.tagged && p.security.is_none());
    let helv = p.fonts.iter().find(|f| f.name == "Helvetica").expect("font");
    assert!(!helv.embedded && !helv.subset && helv.font_type == "Type 1");

    let out = tmp("ff_docsec_props_out.pdf");
    let view = InitialView {
        page_layout: "TwoColumnLeft".into(),
        navigation: "UseOutlines".into(),
        open_page: 1,
        magnification: "fitWidth".into(),
        display_doc_title: true,
        center_window: true,
        ..Default::default()
    };
    write_properties(
        &fx,
        &out,
        &DocPropsUpdate {
            title: Some("Hợp đồng thử nghiệm".into()),
            author: Some("Trần Thị B; Lê C".into()),
            subject: Some("Chủ đề".into()),
            keywords: Some("hợp đồng, thử".into()),
            lang: Some("vi-VN".into()),
            view: Some(view.clone()),
        },
    )
    .unwrap();
    let q = read_properties(Some(&pdf), &out, None).unwrap();
    assert_eq!(q.title, "Hợp đồng thử nghiệm");
    assert_eq!(q.author, "Trần Thị B; Lê C");
    assert_eq!(q.subject, "Chủ đề");
    assert_eq!(q.keywords, "hợp đồng, thử");
    assert_eq!(q.lang, "vi-VN");
    assert!(q.mod_date.is_some());
    assert_eq!(q.view, view);
    let x = docprops::read_xmp_fields(&out).unwrap().expect("xmp");
    assert_eq!(x, ("Hợp đồng thử nghiệm".into(), "Trần Thị B; Lê C".into(), "Chủ đề".into(), "hợp đồng, thử".into()));
    assert!(!raw_contains(&out, "XMPOLDTITLE") && !raw_contains(&out, "XMPOLDKEY"));
    // PDFium đọc lại cùng giá trị.
    use pdfium_render::prelude::*;
    let d = pdf.load_pdf_from_file(&out, None).unwrap();
    let title = d.metadata().get(PdfDocumentMetadataTagType::Title).map(|t| t.value().to_string());
    assert_eq!(title.as_deref(), Some("Hợp đồng thử nghiệm"));
    // Về mặc định: bỏ OpenAction (đích), PageLayout/PageMode.
    let out2 = tmp("ff_docsec_props_out2.pdf");
    write_properties(&out, &out2, &DocPropsUpdate { view: Some(InitialView { magnification: "default".into(), ..Default::default() }), ..Default::default() }).unwrap();
    let r = read_properties(None, &out2, None).unwrap();
    assert_eq!(r.view, InitialView { magnification: "default".into(), ..Default::default() });
    // Phóng to theo %, toàn màn hình giữ bảng điều hướng.
    let out3 = tmp("ff_docsec_props_out3.pdf");
    let v3 = InitialView { magnification: "150".into(), full_screen: true, navigation: "UseThumbs".into(), ..Default::default() };
    write_properties(&out2, &out3, &DocPropsUpdate { view: Some(v3.clone()), ..Default::default() }).unwrap();
    assert_eq!(read_properties(None, &out3, None).unwrap().view, v3);
    // Tệp mã hoá: thông tin bảo mật.
    let enc = workspace_root().join("corpus").join("encrypted.pdf");
    if enc.exists() {
        if let Ok(pe) = read_properties(Some(&pdf), &enc, Some(&enc)) {
            assert!(pe.security.is_some());
        }
    }
}

// ------------------------------------------------------------ Accessibility

fn status(checks: &[A11yCheck], id: &str) -> CheckStatus {
    checks.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("thiếu mục {id}")).status
}

/// Tài liệu gắn thẻ: Figure (ảnh, thiếu Alt, MCID 0), Table không có TH,
/// H1 → H3 (nhảy cấp), LI nằm ngoài L; có Link + widget không tooltip.
fn tagged_fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let img = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 },
        vec![255, 0, 0],
    ));
    let content = b"/H1 <</MCID 1>> BDC BT /F1 20 Tf 72 740 Td (Gioi thieu) Tj ET EMC\n/Figure <</MCID 0>> BDC q 100 0 0 50 72 400 cm /Im1 Do Q EMC\n/H3 <</MCID 2>> BDC BT /F1 14 Tf 72 360 Td (Chi tiet) Tj ET EMC\n/Artifact BMC BT /F1 8 Tf 72 30 Td (footer) Tj ET EMC\n".to_vec();
    let c = doc.add_object(Stream::new(Dictionary::new(), content));
    let link = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => vec![72.into(), 700.into(), 200.into(), 720.into()],
        "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://vi.wikipedia.org") }, "StructParent" => 1,
    });
    let widget = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => Object::string_literal("ho_ten_nguoi_ky"),
        "Rect" => vec![72.into(), 200.into(), 272.into(), 220.into()], "P" => page_id,
    });
    doc.objects.insert(page_id, Object::Dictionary(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Contents" => c,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font }, "XObject" => dictionary! { "Im1" => img } },
        "Annots" => vec![link.into(), widget.into()], "StructParents" => 0,
    }));
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
    let root_se = doc.new_object_id();
    let docel = doc.new_object_id();
    let fig = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "Figure", "P" => docel, "Pg" => page_id, "K" => 0 });
    let h1 = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "H1", "P" => docel, "Pg" => page_id, "K" => 1 });
    let h3 = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "Heading3", "P" => docel, "Pg" => page_id, "K" => 2 });
    let td = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "TD", "Pg" => page_id });
    let tr = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "TR", "K" => vec![td.into()] });
    let table = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "Table", "P" => docel, "K" => vec![tr.into()] });
    let li = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "LI", "P" => docel });
    doc.objects.insert(docel, Object::Dictionary(dictionary! {
        "Type" => "StructElem", "S" => "Document", "P" => root_se,
        "K" => vec![h1.into(), fig.into(), h3.into(), table.into(), li.into()],
    }));
    doc.objects.insert(root_se, Object::Dictionary(dictionary! {
        "Type" => "StructTreeRoot", "K" => docel, "RoleMap" => dictionary! { "Heading3" => "H3" },
    }));
    let acro = doc.add_object(dictionary! { "Fields" => vec![widget.into()] });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "StructTreeRoot" => root_se, "MarkInfo" => dictionary! { "Marked" => true }, "AcroForm" => acro,
    });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

#[test]
fn accessibility_checks_fixes_and_alt_text() {
    let pdf = pdfium();
    // Tài liệu không gắn thẻ.
    let c = check_accessibility(&pdf, &sample(), false).unwrap();
    assert_eq!(status(&c, "document.tagged"), CheckStatus::Failed);
    assert_eq!(status(&c, "document.readingOrder"), CheckStatus::Manual);
    let quick = check_accessibility(&pdf, &sample(), true).unwrap();
    assert!(quick.iter().all(|x| x.category == "document"));

    let fx = tmp("ff_docsec_a11y_fx.pdf");
    tagged_fixture(&fx);
    let c = check_accessibility(&pdf, &fx, false).unwrap();
    assert_eq!(status(&c, "document.tagged"), CheckStatus::Passed);
    for id in ["document.title", "document.language", "document.displayDocTitle", "page.tabOrder", "forms.descriptions", "page.linkContents"] {
        assert_eq!(status(&c, id), CheckStatus::Failed, "{id}");
        assert!(c.iter().find(|x| x.id == id).unwrap().fixable, "{id} phải sửa tự động được");
    }
    assert_eq!(status(&c, "altText.figures"), CheckStatus::Failed);
    assert_eq!(status(&c, "tables.headers"), CheckStatus::Failed);
    assert_eq!(status(&c, "tables.cells"), CheckStatus::Passed);
    assert_eq!(status(&c, "lists.items"), CheckStatus::Failed);
    assert_eq!(status(&c, "headings.nesting"), CheckStatus::Failed, "H1 → H3 (qua RoleMap) phải bị bắt");
    assert_eq!(status(&c, "page.fontsEmbedded"), CheckStatus::Failed);
    assert_eq!(status(&c, "page.taggedContent"), CheckStatus::Passed);
    assert_eq!(status(&c, "forms.tagged"), CheckStatus::Failed);
    assert_eq!(status(&c, "document.imageOnly"), CheckStatus::Passed);

    // Sửa tự động.
    let out = tmp("ff_docsec_a11y_fixed.pdf");
    let n = fix_accessibility(
        &fx,
        &out,
        &A11yFixes {
            title: Some("Báo cáo trợ năng".into()),
            lang: Some("vi-VN".into()),
            display_doc_title: true,
            tab_order: true,
            field_tooltips: true,
            link_contents: Some("Đi tới trang {n}".into()),
            bookmarks_from_headings: true,
        },
    )
    .unwrap();
    assert!(n >= 6, "{n}");
    let c2 = check_accessibility(&pdf, &out, false).unwrap();
    for id in ["document.title", "document.language", "document.displayDocTitle", "page.tabOrder", "forms.descriptions", "page.linkContents"] {
        assert_eq!(status(&c2, id), CheckStatus::Passed, "{id} sau khi sửa");
    }
    let p = read_properties(None, &out, None).unwrap();
    assert_eq!(p.title, "Báo cáo trợ năng");
    let o = outline(&pdf, &out, None).unwrap();
    assert!(o.iter().any(|b| b.title == "Gioi thieu"), "{o:?}");

    // Văn bản thay thế.
    let figs = list_figures(&out).unwrap();
    assert_eq!(figs.len(), 1);
    assert_eq!(figs[0].page_index, Some(0));
    let r = figs[0].rect.expect("rect hình");
    assert!((r[0] - 72.0).abs() < 0.5 && (r[1] - 400.0).abs() < 0.5 && (r[2] - 172.0).abs() < 0.5 && (r[3] - 450.0).abs() < 0.5, "{r:?}");
    let out2 = tmp("ff_docsec_a11y_alt.pdf");
    assert_eq!(set_alt_texts(&out, &out2, &[(figs[0].id.clone(), "Biểu đồ doanh thu quý 3".into())]).unwrap(), 1);
    assert_eq!(list_figures(&out2).unwrap()[0].alt, "Biểu đồ doanh thu quý 3");
    let c3 = check_accessibility(&pdf, &out2, false).unwrap();
    assert_eq!(status(&c3, "altText.figures"), CheckStatus::Passed);
    assert_eq!(page_count(&pdf, &out2, None).unwrap(), 1);
}
