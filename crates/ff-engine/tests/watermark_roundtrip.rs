//! Round-trip test cho Page Marks (hình mờ / nền / đầu-chân trang / Bates):
//! thêm → mở lại bằng PDFium (trích chữ, render điểm ảnh) + lopdf (cấu trúc) →
//! gỡ → dấu biến mất, nội dung gốc còn nguyên.

use std::path::{Path, PathBuf};

use ff_engine::{
    Anchor, BatesFormat, HeaderFooterSpec, MarkKind, PageMarkJob, PageSubset, StampSource, WatermarkSpec,
};
use lopdf::{dictionary, Document, Object, Stream};

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

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn sample() -> PathBuf {
    workspace_root().join("corpus").join("sample-multipage.pdf") // 3 trang Letter
}

fn text(pdf: &pdfium_render::prelude::Pdfium, p: &Path, i: u16) -> String {
    ff_engine::extract_text(pdf, p, i, None).expect("extract_text")
}

/// PDF tự dựng: mỗi trang 1 dòng Helvetica ở (72, 700); `rotate` = /Rotate.
/// `extra` được nối vào content của trang đầu (để giả lập dấu của Acrobat).
fn make_pdf(path: &Path, texts: &[&str], rotate: i64, extra: &str, page_extra: Option<lopdf::Dictionary>) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut kids = vec![];
    for (i, t) in texts.iter().enumerate() {
        let mut content = format!("BT /F1 18 Tf 72 700 Td ({t}) Tj ET\n");
        if i == 0 {
            content += extra;
        }
        let cid = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => cid,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        };
        if rotate != 0 {
            page.set("Rotate", rotate);
        }
        if i == 0 {
            if let Some(extra) = &page_extra {
                for (k, v) in extra.iter() {
                    if k.as_slice() == b"Resources" {
                        if let (Ok(Object::Dictionary(r)), Object::Dictionary(add)) = (page.get_mut(b"Resources"), v) {
                            for (rk, rv) in add.iter() {
                                r.set(rk.clone(), rv.clone());
                            }
                        }
                    } else {
                        page.set(k.clone(), v.clone());
                    }
                }
            }
        }
        kids.push(doc.add_object(page).into());
    }
    let n = kids.len() as i64;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).expect("save fixture");
}

fn wm_text(t: &str) -> WatermarkSpec {
    WatermarkSpec::text(t)
}

fn pixel(pdf: &pdfium_render::prelude::Pdfium, p: &Path, page: u16, fx: f32, fy: f32) -> [u8; 3] {
    let img = ff_engine::render::render_page(pdf, p, page, 400, None).expect("render");
    let rgb = img.image.to_rgb8();
    let x = ((rgb.width() as f32 - 1.0) * fx) as u32;
    let y = ((rgb.height() as f32 - 1.0) * fy) as u32;
    rgb.get_pixel(x, y).0
}

// ---------------------------------------------------------------- hình mờ chữ

#[test]
fn text_watermark_all_pages_then_remove_restores_text() {
    let pdf = pdfium();
    let input = sample();
    let out = tmp("ffpm_wm_all.pdf");
    ff_engine::add_watermark(&pdf, &input, &wm_text("CONFIDENTIAL"), &out, None).expect("add_watermark");
    for i in 0..3u16 {
        assert!(text(&pdf, &out, i).contains("CONFIDENTIAL"), "trang {i} phải có hình mờ");
    }
    let counts = ff_engine::scan_page_marks(&pdf, &out, None).unwrap();
    assert_eq!(counts.watermark, 3);
    assert_eq!(counts.header_footer, 0);

    let cleaned = tmp("ffpm_wm_all_removed.pdf");
    let n = ff_engine::remove_page_marks(&pdf, &out, &[MarkKind::Watermark], &cleaned, None).unwrap();
    assert_eq!(n, 3);
    for i in 0..3u16 {
        let after = text(&pdf, &cleaned, i);
        assert!(!after.contains("CONFIDENTIAL"), "trang {i}: hình mờ phải bị gỡ: {after:?}");
        assert_eq!(after.trim(), text(&pdf, &input, i).trim(), "nội dung gốc giữ nguyên");
    }
    assert_eq!(ff_engine::scan_page_marks(&pdf, &cleaned, None).unwrap().watermark, 0);
}

#[test]
fn watermark_page_range_and_even_subset() {
    let pdf = pdfium();
    let out = tmp("ffpm_wm_even.pdf");
    let mut spec = wm_text("EVENONLY");
    spec.subset = PageSubset::Even; // trang 2 (index 1)
    ff_engine::add_watermark(&pdf, &sample(), &spec, &out, None).unwrap();
    assert!(!text(&pdf, &out, 0).contains("EVENONLY"));
    assert!(text(&pdf, &out, 1).contains("EVENONLY"));
    assert!(!text(&pdf, &out, 2).contains("EVENONLY"));

    let out2 = tmp("ffpm_wm_range.pdf");
    let mut spec = wm_text("RANGE");
    spec.pages = vec![1, 2];
    spec.subset = PageSubset::Odd; // trong 2–3 chỉ trang 3
    ff_engine::add_watermark(&pdf, &sample(), &spec, &out2, None).unwrap();
    assert!(!text(&pdf, &out2, 1).contains("RANGE"));
    assert!(text(&pdf, &out2, 2).contains("RANGE"));
}

#[test]
fn watermark_vietnamese_multiline_round_trips() {
    let pdf = pdfium();
    let out = tmp("ffpm_wm_viet.pdf");
    ff_engine::add_watermark(&pdf, &sample(), &wm_text("Bản nháp\nKhông phát hành"), &out, None).unwrap();
    let t = text(&pdf, &out, 0);
    assert!(t.contains("Bản nháp"), "{t:?}");
    assert!(t.contains("Không phát hành"), "{t:?}");
}

#[test]
fn replace_existing_watermark_updates_instead_of_stacking() {
    let pdf = pdfium();
    let a = tmp("ffpm_wm_a.pdf");
    let b = tmp("ffpm_wm_b.pdf");
    ff_engine::add_watermark(&pdf, &sample(), &wm_text("FIRSTMARK"), &a, None).unwrap();
    let mut spec = wm_text("SECONDMARK");
    spec.replace_existing = true;
    ff_engine::add_watermark(&pdf, &a, &spec, &b, None).unwrap();
    let t = text(&pdf, &b, 0);
    assert!(t.contains("SECONDMARK") && !t.contains("FIRSTMARK"), "{t:?}");
}

#[test]
fn watermark_behind_or_on_top_is_marked_artifact_in_content_order() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_order.pdf");
    make_pdf(&input, &["BODYTEXT"], 0, "", None);
    for behind in [true, false] {
        let out = tmp(if behind { "ffpm_wm_behind.pdf" } else { "ffpm_wm_top.pdf" });
        let mut spec = wm_text("ORDER");
        spec.behind = behind;
        ff_engine::add_watermark(&pdf, &input, &spec, &out, None).unwrap();
        let doc = Document::load(&out).unwrap();
        let pid = *doc.get_pages().values().next().unwrap();
        let content = String::from_utf8_lossy(&doc.get_page_content(pid).unwrap()).into_owned();
        let art = content.find("/Artifact <</Subtype /Watermark /Type /Pagination>> BDC").expect("có khối Artifact");
        let body = content.find("(BODYTEXT)").unwrap();
        assert_eq!(art < body, behind, "behind={behind}: thứ tự vẽ sai\n{content}");
    }
}

// ---------------------------------------------------------------- hình mờ ảnh / trang PDF

fn red_png(path: &Path) {
    let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
    img.save(path).expect("save png");
}

#[test]
fn image_watermark_is_image_object_visible_then_removed() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_img.pdf");
    make_pdf(&input, &["IMG PAGE"], 0, "", None);
    let png = std::env::temp_dir().join("ffpm_red.png");
    red_png(&png);
    let out = tmp("ffpm_wm_img.pdf");
    let mut spec = wm_text("");
    spec.source = StampSource::Image { path: png.clone() };
    spec.rotation_deg = 0.0;
    spec.opacity = 1.0;
    spec.relative_scale = Some(0.5);
    ff_engine::add_watermark(&pdf, &input, &spec, &out, None).unwrap();

    // Cấu trúc: XObject FFpm* là Form chứa 1 Image XObject.
    let doc = Document::load(&out).unwrap();
    let pid = *doc.get_pages().values().next().unwrap();
    let page = doc.get_dictionary(pid).unwrap();
    let res = page.get(b"Resources").unwrap().as_dict().unwrap();
    let xo = res.get(b"XObject").unwrap().as_dict().unwrap();
    let (_, form_ref) = xo.iter().find(|(k, _)| k.starts_with(b"FFpm")).expect("có XObject FFpm");
    let form = doc.get_object(form_ref.as_reference().unwrap()).unwrap().as_stream().unwrap();
    let fres = form.dict.get(b"Resources").unwrap().as_dict().unwrap();
    let fxo = fres.get(b"XObject").unwrap().as_dict().unwrap();
    let img = doc.get_object(fxo.get(b"Im0").unwrap().as_reference().unwrap()).unwrap().as_stream().unwrap();
    assert_eq!(img.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Image");

    // Hiển thị: tâm trang thành đỏ (50% bề rộng trang).
    let c = pixel(&pdf, &out, 0, 0.5, 0.5);
    assert!(c[0] > 200 && c[1] < 60 && c[2] < 60, "tâm trang phải đỏ: {c:?}");

    let cleaned = tmp("ffpm_wm_img_removed.pdf");
    ff_engine::remove_page_marks(&pdf, &out, &[MarkKind::Watermark], &cleaned, None).unwrap();
    let c = pixel(&pdf, &cleaned, 0, 0.5, 0.5);
    assert!(c.iter().all(|v| *v > 240), "gỡ xong tâm trang trắng lại: {c:?}");
    assert!(text(&pdf, &cleaned, 0).contains("IMG PAGE"));
}

#[test]
fn pdf_page_as_watermark_source() {
    let pdf = pdfium();
    let out = tmp("ffpm_wm_pdfpage.pdf");
    let mut spec = wm_text("");
    spec.source = StampSource::Page { path: workspace_root().join("corpus").join("hello.pdf"), page: 0 };
    spec.relative_scale = Some(0.8);
    ff_engine::add_watermark(&pdf, &sample(), &spec, &out, None).unwrap();
    let t = text(&pdf, &out, 1);
    assert!(t.contains("Hello FoFreeXit"), "trang PDF nguồn phải được đóng dấu: {t:?}");
}

// ---------------------------------------------------------------- nền

#[test]
fn background_color_is_behind_text_and_removable() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_bg.pdf");
    make_pdf(&input, &["ON BLUE"], 0, "", None);
    let out = tmp("ffpm_bg.pdf");
    let mut spec = wm_text("");
    spec.source = StampSource::Color { color: [0, 0, 255] };
    spec.opacity = 1.0;
    ff_engine::add_background(&pdf, &input, &spec, &out, None).unwrap();

    let corner = pixel(&pdf, &out, 0, 0.02, 0.98);
    assert!(corner[2] > 200 && corner[0] < 50, "nền xanh phải phủ góc trang: {corner:?}");
    assert!(text(&pdf, &out, 0).contains("ON BLUE"), "chữ vẫn còn trên nền");
    // Chữ đen vẫn vẽ ĐÈ lên nền: có điểm ảnh tối trong ảnh render.
    let img = ff_engine::render::render_page(&pdf, &out, 0, 600, None).unwrap().image.to_rgb8();
    assert!(img.pixels().any(|p| p.0[0] < 40 && p.0[1] < 40 && p.0[2] < 40), "chữ phải nằm trên nền");
    assert_eq!(ff_engine::scan_page_marks(&pdf, &out, None).unwrap().background, 1);

    let cleaned = tmp("ffpm_bg_removed.pdf");
    ff_engine::remove_page_marks(&pdf, &out, &[MarkKind::Background], &cleaned, None).unwrap();
    let corner = pixel(&pdf, &cleaned, 0, 0.02, 0.98);
    assert!(corner.iter().all(|v| *v > 240), "gỡ nền → trắng: {corner:?}");
}

// ---------------------------------------------------------------- đầu / chân trang

#[test]
fn header_footer_page_formats_start_number_and_date() {
    let pdf = pdfium();
    let out = tmp("ffpm_hf_num.pdf");
    let spec = HeaderFooterSpec {
        bottom_center: "Trang {page}/{total}".to_string(),
        bottom_right: "Page {page} of {total}".to_string(),
        top_right: "{date}".to_string(),
        date: "17/06/2026".to_string(),
        start_number: 5,
        ..Default::default()
    };
    ff_engine::add_header_footer(&pdf, &sample(), &spec, &out, None).unwrap();
    for i in 0..3u16 {
        let t = text(&pdf, &out, i);
        assert!(t.contains(&format!("Trang {}/7", i + 5)), "trang {i}: {t:?}");
        assert!(t.contains(&format!("Page {} of 7", i + 5)), "trang {i}: {t:?}");
        assert!(t.contains("17/06/2026"), "trang {i}: {t:?}");
    }
}

#[test]
fn header_footer_respects_four_margins() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_margin.pdf");
    make_pdf(&input, &["x"], 0, "", None);
    let out = tmp("ffpm_hf_margin.pdf");
    let spec = HeaderFooterSpec {
        top_left: "LT".to_string(),
        bottom_right: "RB".to_string(),
        font_size: 12.0,
        margin_top: 50.0,
        margin_left: 90.0,
        margin_bottom: 40.0,
        margin_right: 100.0,
        ..Default::default()
    };
    ff_engine::add_header_footer(&pdf, &input, &spec, &out, None).unwrap();
    let boxes = ff_engine::page_char_boxes(&pdf, &out, 0, None).unwrap();
    let l = boxes.iter().find(|b| b.ch == "L").expect("có chữ L");
    assert!((l.left - 90.0).abs() < 4.0, "lề trái: {}", l.left);
    assert!(l.top <= 792.0 - 50.0 + 1.0 && l.top > 792.0 - 50.0 - 18.0, "lề trên: {}", l.top);
    let b = boxes.iter().find(|b| b.ch == "B").expect("có chữ B");
    assert!((b.right - (612.0 - 100.0)).abs() < 4.0, "lề phải: {}", b.right);
    assert!(b.bottom >= 40.0 - 1.0 && b.bottom < 40.0 + 12.0, "lề dưới: {}", b.bottom);
}

#[test]
fn header_footer_on_rotated_page_follows_displayed_bottom() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_rot.pdf");
    make_pdf(&input, &["rotated"], 90, "", None);
    let out = tmp("ffpm_hf_rot.pdf");
    let spec = HeaderFooterSpec { bottom_center: "ROTFOOT".to_string(), ..Default::default() };
    ff_engine::add_header_footer(&pdf, &input, &spec, &out, None).unwrap();
    assert!(text(&pdf, &out, 0).contains("ROTFOOT"));
    // /Rotate 90: mép dưới khi nhìn = mép PHẢI user space (x ≈ 612).
    let boxes = ff_engine::page_char_boxes(&pdf, &out, 0, None).unwrap();
    let r = boxes.iter().find(|b| b.ch == "R").expect("có chữ R");
    assert!(r.left > 612.0 - 36.0 - 20.0, "chân trang phải nằm sát mép phải user space: {}", r.left);
}

#[test]
fn remove_header_footer_keeps_watermark() {
    let pdf = pdfium();
    let wm = tmp("ffpm_mix_wm.pdf");
    let both = tmp("ffpm_mix_both.pdf");
    ff_engine::add_watermark(&pdf, &sample(), &wm_text("KEEPWM"), &wm, None).unwrap();
    let spec = HeaderFooterSpec { top_center: "DROPHDR".to_string(), bottom_left: "DROPFTR".to_string(), ..Default::default() };
    ff_engine::add_header_footer(&pdf, &wm, &spec, &both, None).unwrap();
    let c = ff_engine::scan_page_marks(&pdf, &both, None).unwrap();
    assert_eq!((c.watermark, c.header_footer), (3, 3));

    let cleaned = tmp("ffpm_mix_clean.pdf");
    let n = ff_engine::remove_page_marks(&pdf, &both, &[MarkKind::Header, MarkKind::Footer], &cleaned, None).unwrap();
    assert_eq!(n, 6, "3 trang × (đầu + chân)");
    let t = text(&pdf, &cleaned, 0);
    assert!(t.contains("KEEPWM") && !t.contains("DROPHDR") && !t.contains("DROPFTR"), "{t:?}");
}

// ---------------------------------------------------------------- Bates

#[test]
fn bates_numbering_continues_across_two_files_and_removes() {
    let pdf = pdfium();
    let a = tmp("ffpm_bates_a_in.pdf");
    let b = tmp("ffpm_bates_b_in.pdf");
    make_pdf(&a, &["A1", "A2", "A3"], 0, "", None);
    make_pdf(&b, &["B1", "B2"], 0, "", None);
    let (oa, ob) = (tmp("ffpm_bates_a.pdf"), tmp("ffpm_bates_b.pdf"));
    let spec = HeaderFooterSpec {
        bottom_right: "{bates}".to_string(),
        bates: Some(BatesFormat { prefix: "ABC".into(), suffix: "-X".into(), start: 7, digits: 6 }),
        ..Default::default()
    };
    let ranges = ff_engine::add_bates(&pdf, &[(a.clone(), oa.clone()), (b.clone(), ob.clone())], &spec).unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!((ranges[0].first.as_str(), ranges[0].last.as_str()), ("ABC000007-X", "ABC000009-X"));
    assert_eq!((ranges[1].first.as_str(), ranges[1].last.as_str()), ("ABC000010-X", "ABC000011-X"));
    assert!(text(&pdf, &oa, 2).contains("ABC000009-X"));
    assert!(text(&pdf, &ob, 0).contains("ABC000010-X"));
    assert!(text(&pdf, &ob, 1).contains("ABC000011-X"));
    assert_eq!(ff_engine::scan_page_marks(&pdf, &ob, None).unwrap().bates, 2);

    let cleaned = tmp("ffpm_bates_b_clean.pdf");
    ff_engine::remove_page_marks(&pdf, &ob, &[MarkKind::Bates], &cleaned, None).unwrap();
    let t = text(&pdf, &cleaned, 1);
    assert!(!t.contains("ABC0000") && t.contains("B2"), "{t:?}");
}

// ---------------------------------------------------------------- dấu do Acrobat/Foxit thêm

#[test]
fn acrobat_style_marks_inline_image_and_watermark_annots_are_handled() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_acro.pdf");
    let extra = "/Artifact <</Subtype /Watermark /Type /Pagination>> BDC\nBT /F1 30 Tf 100 400 Td (ACROWM) Tj ET\nEMC\n\
                 /Artifact /MC0 BDC\nBT /F1 10 Tf 72 760 Td (ACROHDR) Tj ET\nEMC\n\
                 q 20 0 0 20 300 300 cm BI /W 2 /H 1 /BPC 8 /CS /G ID \u{0}E EI Q\n";
    let mut props = lopdf::Dictionary::new();
    props.set("MC0", dictionary! { "Subtype" => "Header", "Type" => "Pagination" });
    let wm_annot = dictionary! {
        "Type" => "Annot", "Subtype" => "Watermark",
        "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
    };
    let page_extra = dictionary! {
        "Resources" => dictionary! { "Properties" => props },
        "Annots" => vec![Object::Dictionary(wm_annot)],
    };
    make_pdf(&input, &["KEEPBODY"], 0, extra, Some(page_extra));
    let c = ff_engine::scan_page_marks(&pdf, &input, None).unwrap();
    assert_eq!((c.watermark, c.header_footer), (1, 1));

    let no_wm = tmp("ffpm_acro_nowm.pdf");
    let n = ff_engine::remove_page_marks(&pdf, &input, &[MarkKind::Watermark], &no_wm, None).unwrap();
    assert_eq!(n, 2, "1 khối Artifact + 1 annotation /Watermark");
    let t = text(&pdf, &no_wm, 0);
    assert!(!t.contains("ACROWM") && t.contains("ACROHDR") && t.contains("KEEPBODY"), "{t:?}");
    let doc = Document::load(&no_wm).unwrap();
    let pid = *doc.get_pages().values().next().unwrap();
    let content = doc.get_page_content(pid).unwrap();
    assert!(content.windows(3).any(|w| w == b" BI"), "inline image giữ nguyên byte");
    assert!(content.windows(4).any(|w| w == b"\0E E"), "dữ liệu inline image nguyên vẹn");

    let no_hdr = tmp("ffpm_acro_nohdr.pdf");
    ff_engine::remove_page_marks(&pdf, &no_wm, &[MarkKind::Header, MarkKind::Footer], &no_hdr, None).unwrap();
    let t = text(&pdf, &no_hdr, 0);
    assert!(!t.contains("ACROHDR") && t.contains("KEEPBODY"), "{t:?}");
}

// ---------------------------------------------------------------- xem trước / mã hoá / tổ chức trang

#[test]
fn preview_uses_real_page_number_and_total() {
    let pdf = pdfium();
    let out = tmp("ffpm_preview.pdf");
    let spec = HeaderFooterSpec { bottom_center: "{page}/{total}".to_string(), ..Default::default() };
    ff_engine::preview_page_mark(&pdf, &sample(), 2, PageMarkJob::HeaderFooter(&spec), &out, None).unwrap();
    assert_eq!(ff_engine::page_count(&pdf, &out, None).unwrap(), 1);
    assert!(text(&pdf, &out, 0).contains("3/3"));

    let mut wm = wm_text("PREVIEWWM");
    wm.pages = vec![0]; // xem trước bỏ qua phạm vi — luôn hiện dấu
    let out2 = tmp("ffpm_preview_wm.pdf");
    ff_engine::preview_page_mark(&pdf, &sample(), 1, PageMarkJob::Watermark(&wm), &out2, None).unwrap();
    assert!(text(&pdf, &out2, 0).contains("PREVIEWWM"));
}

#[test]
fn encrypted_input_stays_encrypted_with_watermark() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("encrypted.pdf");
    let out = tmp("ffpm_wm_encrypted.pdf");
    ff_engine::add_watermark(&pdf, &input, &wm_text("SECRETWM"), &out, Some("fofreexit")).unwrap();
    assert!(pdf.load_pdf_from_file(&out, None).is_err(), "đầu ra phải còn mật khẩu");
    let t = ff_engine::extract_text(&pdf, &out, 0, Some("fofreexit")).unwrap();
    assert!(t.contains("SECRETWM"), "{t:?}");
}

#[test]
fn organize_duplicate_and_reverse_plans() {
    let pdf = pdfium();
    let input = tmp("ffpm_fix_org.pdf");
    make_pdf(&input, &["P1", "P2", "P3"], 0, "", None);
    // Nhân bản trang 2 (chèn ngay sau) rồi đảo thứ tự toàn bộ: P3 P2 P2 P1.
    let mut plan = ff_engine::identity_plan(&pdf, &input, None).unwrap();
    plan.insert(2, ff_engine::PagePlanEntry::existing(1));
    plan.reverse();
    let out = tmp("ffpm_org.pdf");
    ff_engine::build_document(&pdf, &input, &plan, &out, None).unwrap();
    let got: Vec<String> = (0..4).map(|i| text(&pdf, &out, i).trim().to_string()).collect();
    assert_eq!(got, vec!["P3", "P2", "P2", "P1"]);
    let _ = Anchor::Center;
}
