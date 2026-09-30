//! Round-trip test: sửa bookmark (outline) — cây lồng nhau, tiêu đề tiếng Việt,
//! thứ tự, trang đích, kiểu chữ/màu, /Count; liên kết (tạo/liệt kê/xoá, trang
//! xoay); tách tệp theo dải trang / bookmark / dung lượng; tự tạo bookmark từ
//! tiêu đề. Ghi → mở lại bằng lopdf VÀ PDFium → so khớp.

use std::path::PathBuf;

use ff_engine::{Bookmark, NewLink, Rect};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
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

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Dựng PDF `n` trang A4-ish (612x792). Mỗi trang: tiêu đề 24pt "Chapter k",
/// tiêu đề phụ 16pt "Section k.1", vài dòng thân bài 11pt. `rotate[i]` = /Rotate.
fn build_pdf(path: &PathBuf, n: usize, rotate: &[i64]) -> Vec<lopdf::ObjectId> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut kids = Vec::new();
    let mut ids = Vec::new();
    for i in 0..n {
        let mut body = format!(
            "BT /F1 24 Tf 72 720 Td (Chapter {k}) Tj ET\nBT /F1 16 Tf 72 680 Td (Section {k}.1) Tj ET\n",
            k = i + 1
        );
        for l in 0..8 {
            body.push_str(&format!(
                "BT /F1 11 Tf 72 {} Td (Body text line {l} of page {k} lorem ipsum dolor sit amet) Tj ET\n",
                640 - l * 16,
                k = i + 1
            ));
        }
        let content = doc.add_object(Stream::new(dictionary! {}, body.into_bytes()));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        };
        if let Some(r) = rotate.get(i) {
            if *r != 0 {
                page.set("Rotate", Object::Integer(*r));
            }
        }
        let pid = doc.add_object(page);
        kids.push(Object::Reference(pid));
        ids.push(pid);
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
    ids
}

fn sample_tree() -> Vec<Bookmark> {
    vec![
        Bookmark {
            title: "Chương 1: Giới thiệu".into(),
            page_index: Some(0),
            bold: true,
            color: Some([255, 0, 0]),
            open: true,
            children: vec![
                Bookmark { title: "Mục 1.1 Tổng quan".into(), page_index: Some(1), top: Some(500.0), ..Default::default() },
                Bookmark { title: "Mục 1.2 (Fit)".into(), page_index: Some(2), fit: true, ..Default::default() },
            ],
            ..Default::default()
        },
        Bookmark {
            title: "Chương 2 — Kết luận".into(),
            page_index: Some(3),
            open: false,
            children: vec![Bookmark {
                title: "Phụ lục ư ơ đ Ạ".into(),
                page_index: Some(4),
                italic: true,
                left: Some(10.0),
                top: Some(700.0),
                zoom: Some(1.5),
                ..Default::default()
            }],
            ..Default::default()
        },
        Bookmark { title: "Plain ASCII (with parens)".into(), page_index: Some(4), ..Default::default() },
    ]
}

#[test]
fn outline_roundtrip_nested_vietnamese_order_dest_style() {
    let input = tmp("ff_bm_in.pdf");
    let output = tmp("ff_bm_out.pdf");
    build_pdf(&input, 5, &[]);
    let tree = sample_tree();
    ff_engine::set_outline(&input, &output, &tree).expect("set_outline");

    let back = ff_engine::get_outline(&output).expect("get_outline");
    assert_eq!(back, tree, "đọc lại phải y hệt cây đã ghi");

    // /Count đúng spec: gốc = 3 nút cấp 1 + 2 con hiển thị của nút đang mở.
    let doc = Document::load(&output).unwrap();
    let root = doc.catalog().unwrap().get(b"Outlines").unwrap().as_reference().unwrap();
    let rd = doc.get_dictionary(root).unwrap();
    assert_eq!(rd.get(b"Count").unwrap().as_i64().unwrap(), 5);
    let first = rd.get(b"First").unwrap().as_reference().unwrap();
    let fd = doc.get_dictionary(first).unwrap();
    assert_eq!(fd.get(b"Count").unwrap().as_i64().unwrap(), 2);
    assert_eq!(fd.get(b"Parent").unwrap().as_reference().unwrap(), root);
    let second = fd.get(b"Next").unwrap().as_reference().unwrap();
    let sd = doc.get_dictionary(second).unwrap();
    assert_eq!(sd.get(b"Count").unwrap().as_i64().unwrap(), -1, "nút đóng: Count âm");
    assert_eq!(sd.get(b"Prev").unwrap().as_reference().unwrap(), first);
    // Tiêu đề tiếng Việt ghi UTF-16BE có BOM.
    let raw = fd.get(b"Title").unwrap().as_str().unwrap();
    assert!(raw.starts_with(b"\xFE\xFF"));

    // PDFium (bộ đọc độc lập) thấy đúng tiêu đề + trang đích.
    let pdf = pdfium();
    let d = pdf.load_pdf_from_file(&output, None).unwrap();
    let bms = d.bookmarks();
    let first = bms.root().expect("root bookmark");
    assert_eq!(first.title().unwrap(), "Chương 1: Giới thiệu");
    assert_eq!(first.destination().unwrap().page_index().unwrap(), 0);
    let child = first.first_child().expect("con đầu");
    assert_eq!(child.title().unwrap(), "Mục 1.1 Tổng quan");
    assert_eq!(child.destination().unwrap().page_index().unwrap(), 1);
    let sib = first.next_sibling().expect("anh em");
    assert_eq!(sib.title().unwrap(), "Chương 2 — Kết luận");
    assert_eq!(sib.destination().unwrap().page_index().unwrap(), 3);
}

#[test]
fn outline_replace_and_clear() {
    let input = tmp("ff_bm_rep_in.pdf");
    let a = tmp("ff_bm_rep_a.pdf");
    let b = tmp("ff_bm_rep_b.pdf");
    let c = tmp("ff_bm_rep_c.pdf");
    build_pdf(&input, 3, &[]);
    ff_engine::set_outline(&input, &a, &sample_tree()[..1]).unwrap();
    let objs_a = Document::load(&a).unwrap().objects.len();
    // Thay cây mới: cây cũ bị gỡ, không để lại object mồ côi.
    let t2 = vec![Bookmark::to_page("Only", 2)];
    ff_engine::set_outline(&a, &b, &t2).unwrap();
    assert_eq!(ff_engine::get_outline(&b).unwrap(), t2);
    let objs_b = Document::load(&b).unwrap().objects.len();
    assert_eq!(objs_b, objs_a - 3 + 1, "3 nút cũ bị xoá, 1 nút mới");
    // Cây rỗng = bỏ outline.
    ff_engine::set_outline(&b, &c, &[]).unwrap();
    assert!(ff_engine::get_outline(&c).unwrap().is_empty());
    assert!(Document::load(&c).unwrap().catalog().unwrap().get(b"Outlines").is_err());
    // Trang không tồn tại → lỗi rõ ràng, không ghi.
    let d = tmp("ff_bm_rep_d.pdf");
    assert!(ff_engine::set_outline(&input, &d, &[Bookmark::to_page("x", 9)]).is_err());
}

#[test]
fn outline_reads_named_dests_and_goto_actions() {
    let input = tmp("ff_bm_named.pdf");
    let ids = build_pdf(&input, 3, &[]);
    let mut doc = Document::load(&input).unwrap();
    // Name tree /Names/Dests + /Dests kiểu cũ + /A GoTo + URI.
    let dests_tree = doc.add_object(dictionary! {
        "Names" => vec![
            Object::String(b"chap2".to_vec(), StringFormat::Literal),
            Object::Array(vec![Object::Reference(ids[1]), "FitH".into(), 600.into()]),
        ],
    });
    let names = doc.add_object(dictionary! { "Dests" => dests_tree });
    let old_dests = doc.add_object(dictionary! {
        "old3" => dictionary! { "D" => vec![Object::Reference(ids[2]), "Fit".into()] },
    });
    let root = doc.new_object_id();
    let o1 = doc.new_object_id();
    let o2 = doc.new_object_id();
    let o3 = doc.new_object_id();
    doc.objects.insert(o1, Object::Dictionary(dictionary! {
        "Title" => Object::String(b"Named".to_vec(), StringFormat::Literal),
        "Parent" => root, "Next" => o2,
        "Dest" => Object::String(b"chap2".to_vec(), StringFormat::Literal),
    }));
    doc.objects.insert(o2, Object::Dictionary(dictionary! {
        "Title" => Object::String(b"Action".to_vec(), StringFormat::Literal),
        "Parent" => root, "Prev" => o1, "Next" => o3,
        "A" => dictionary! { "S" => "GoTo", "D" => Object::Name(b"old3".to_vec()) },
    }));
    doc.objects.insert(o3, Object::Dictionary(dictionary! {
        "Title" => Object::String(b"Web".to_vec(), StringFormat::Literal),
        "Parent" => root, "Prev" => o2,
        "A" => dictionary! { "S" => "URI", "URI" => Object::String(b"https://example.com/".to_vec(), StringFormat::Literal) },
    }));
    doc.objects.insert(root, Object::Dictionary(dictionary! {
        "Type" => "Outlines", "First" => o1, "Last" => o3, "Count" => 3,
    }));
    let cat = doc.catalog_mut().unwrap();
    cat.set("Outlines", root);
    cat.set("Names", names);
    cat.set("Dests", old_dests);
    doc.save(&input).unwrap();

    let t = ff_engine::get_outline(&input).unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(t[0].page_index, Some(1));
    assert_eq!(t[0].top, Some(600.0));
    assert_eq!(t[1].page_index, Some(2));
    assert!(t[1].fit);
    assert_eq!(t[2].uri.as_deref(), Some("https://example.com/"));
    assert_eq!(t[2].page_index, None);

    // Ghi lại nguyên cây (đích tên → đích tường minh) vẫn giữ trang + URI.
    let out = tmp("ff_bm_named_out.pdf");
    ff_engine::set_outline(&input, &out, &t).unwrap();
    assert_eq!(ff_engine::get_outline(&out).unwrap(), t);
}

#[test]
fn links_create_list_delete_roundtrip() {
    let input = tmp("ff_lnk_in.pdf");
    let a = tmp("ff_lnk_a.pdf");
    let b = tmp("ff_lnk_b.pdf");
    // Trang 1 xoay 90 để thử chuyển toạ độ hiển thị ↔ user space.
    build_pdf(&input, 3, &[0, 90, 0]);
    let r0 = Rect { left: 72.0, bottom: 700.0, right: 250.0, top: 740.0 };
    let r1 = Rect { left: 100.0, bottom: 50.0, right: 300.0, top: 120.0 };
    let add = vec![
        NewLink { page_index: 0, rect: r0, dest_page: Some(2), dest_top: Some(400.0), uri: None, border: None },
        NewLink { page_index: 1, rect: r1, dest_page: None, dest_top: None, uri: Some("https://foxit.example/vi?q=1".into()), border: Some([0, 0, 255]) },
    ];
    ff_engine::edit_links(&input, &a, &[], &add).expect("tạo link");

    let links = ff_engine::list_links(&a).expect("list");
    assert_eq!(links.len(), 2);
    let close = |x: &Rect, y: &Rect| {
        (x.left - y.left).abs() < 0.01 && (x.right - y.right).abs() < 0.01
            && (x.top - y.top).abs() < 0.01 && (x.bottom - y.bottom).abs() < 0.01
    };
    assert_eq!(links[0].page_index, 0);
    assert!(close(&links[0].rect, &r0), "{:?}", links[0].rect);
    assert_eq!(links[0].dest_page, Some(2));
    assert_eq!(links[0].dest_top, Some(400.0));
    assert_eq!(links[1].page_index, 1);
    assert!(close(&links[1].rect, &r1), "trang xoay: {:?}", links[1].rect);
    assert_eq!(links[1].uri.as_deref(), Some("https://foxit.example/vi?q=1"));

    // Trang xoay 90: /Rect user space khác hẳn toạ độ hiển thị (x1 - top ...).
    let doc = Document::load(&a).unwrap();
    let pid = doc.get_pages()[&2];
    let annots = doc.get_page_annotations(pid).unwrap();
    let rect: Vec<f32> = annots[0].get(b"Rect").unwrap().as_array().unwrap().iter().map(|o| o.as_float().unwrap()).collect();
    assert_eq!(rect, vec![612.0 - 120.0, 100.0, 612.0 - 50.0, 300.0]);

    // PDFium thấy link annotation + đích.
    let pdf = pdfium();
    let d = pdf.load_pdf_from_file(&a, None).unwrap();
    let p0 = d.pages().get(0).unwrap();
    let n_links = p0.links().iter().count();
    assert_eq!(n_links, 1);
    let l = p0.links().iter().next().unwrap();
    assert_eq!(l.destination().unwrap().page_index().unwrap(), 2);

    // Xoá link URI theo id → còn 1.
    ff_engine::edit_links(&a, &b, &[links[1].id.clone()], &[]).expect("xoá link");
    let left = ff_engine::list_links(&b).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].dest_page, Some(2));
    let doc = Document::load(&b).unwrap();
    assert!(doc.get_dictionary(pid).unwrap().get(b"Annots").is_err(), "trang không còn annotation");
}

#[test]
fn save_outline_and_links_in_one_pass() {
    let input = tmp("ff_bml_in.pdf");
    let out = tmp("ff_bml_out.pdf");
    build_pdf(&input, 5, &[]);
    let tree = sample_tree();
    let add = vec![NewLink {
        page_index: 4, rect: Rect { left: 10.0, bottom: 10.0, right: 60.0, top: 30.0 },
        dest_page: Some(0), dest_top: None, uri: None, border: None,
    }];
    ff_engine::save_outline_and_links(&input, &out, Some(&tree), &[], &add).unwrap();
    assert_eq!(ff_engine::get_outline(&out).unwrap(), tree);
    let l = ff_engine::list_links(&out).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].page_index, l[0].dest_page), (4, Some(0)));
}

#[test]
fn parse_ranges_variants() {
    assert_eq!(ff_engine::parse_page_ranges("1-3, 4-10, 11-", 12).unwrap(), vec![(0, 2), (3, 9), (10, 11)]);
    assert_eq!(ff_engine::parse_page_ranges("-2; 5", 6).unwrap(), vec![(0, 1), (4, 4)]);
    assert_eq!(ff_engine::parse_page_ranges("3-99", 5).unwrap(), vec![(2, 4)]);
    assert!(ff_engine::parse_page_ranges("4-2", 5).is_err());
    assert!(ff_engine::parse_page_ranges("abc", 5).is_err());
    assert!(ff_engine::parse_page_ranges("7", 5).is_err());
    assert!(ff_engine::parse_page_ranges(" , ", 5).is_err());
}

#[test]
fn split_by_ranges_writes_expected_pages() {
    let pdf = pdfium();
    let input = tmp("ff_split_rng.pdf");
    build_pdf(&input, 6, &[]);
    let dir = tmp_dir("ff_split_rng_out");
    let ranges = ff_engine::parse_page_ranges("1-2, 3, 4-", 6).unwrap();
    let outs = ff_engine::split_by_ranges(&pdf, &input, &ranges, &dir, "doc", None).unwrap();
    assert_eq!(outs.len(), 3);
    assert!(outs[0].ends_with("doc_part1.pdf"));
    let counts: Vec<u16> = outs.iter().map(|o| ff_engine::page_count(&pdf, o, None).unwrap()).collect();
    assert_eq!(counts, vec![2, 1, 3]);
    let t = ff_engine::extract_text(&pdf, &outs[2], 0, None).unwrap();
    assert!(t.contains("Chapter 4"), "phần 3 bắt đầu từ trang 4: {t:?}");
}

#[test]
fn split_by_bookmarks_names_files_and_keeps_children() {
    let pdf = pdfium();
    let input = tmp("ff_split_bm_in.pdf");
    let with_bm = tmp("ff_split_bm.pdf");
    build_pdf(&input, 6, &[]);
    let tree = vec![
        Bookmark {
            title: "Phần 1: Mở đầu".into(),
            page_index: Some(1),
            children: vec![Bookmark::to_page("Con 1.1", 2)],
            open: true,
            ..Default::default()
        },
        Bookmark { title: "A/B: Kết?".into(), page_index: Some(3), ..Default::default() },
        Bookmark { title: "Không trang".into(), page_index: None, ..Default::default() },
        Bookmark { title: "A/B: Kết?".into(), page_index: Some(5), ..Default::default() },
    ];
    ff_engine::set_outline(&input, &with_bm, &tree).unwrap();
    let dir = tmp_dir("ff_split_bm_out");
    let outs = ff_engine::split_by_bookmarks(&pdf, &with_bm, &dir, "doc", None).unwrap();
    let names: Vec<String> = outs.iter().map(|o| o.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["Phần 1_ Mở đầu.pdf", "A_B_ Kết_.pdf", "A_B_ Kết_ (2).pdf"]);
    let counts: Vec<u16> = outs.iter().map(|o| ff_engine::page_count(&pdf, o, None).unwrap()).collect();
    // Trang 0 (trước bookmark đầu) gộp vào tệp đầu: 0..=2, 3..=4, 5.
    assert_eq!(counts, vec![3, 2, 1]);
    let t0 = ff_engine::get_outline(&outs[0]).unwrap();
    assert_eq!(t0.len(), 1);
    assert_eq!(t0[0].title, "Phần 1: Mở đầu");
    assert_eq!(t0[0].page_index, Some(1));
    assert_eq!(t0[0].children[0].page_index, Some(2));
    let t1 = ff_engine::get_outline(&outs[1]).unwrap();
    assert_eq!(t1[0].page_index, Some(0));
    // Không có bookmark → lỗi rõ.
    assert!(ff_engine::split_by_bookmarks(&pdf, &input, &dir, "doc", None).is_err());
}

#[test]
fn split_by_size_respects_limit_approximately() {
    let pdf = pdfium();
    let input = tmp("ff_split_size.pdf");
    build_pdf(&input, 10, &[]);
    // Mỗi trang ~ 700-900 byte nội dung; ngưỡng 4 KB → nhiều tệp, mỗi tệp ≥ 1 trang.
    let ranges = ff_engine::plan_split_by_size(&input, 4096).unwrap();
    assert!(ranges.len() >= 2, "{ranges:?}");
    assert_eq!(ranges.first().unwrap().0, 0);
    assert_eq!(ranges.last().unwrap().1, 9);
    for w in ranges.windows(2) {
        assert_eq!(w[0].1 + 1, w[1].0, "liên tục, không chồng: {ranges:?}");
    }
    // Ngưỡng rất lớn → 1 tệp.
    assert_eq!(ff_engine::plan_split_by_size(&input, 50_000_000).unwrap(), vec![(0, 9)]);
    let dir = tmp_dir("ff_split_size_out");
    let outs = ff_engine::split_by_size(&pdf, &input, 4096, &dir, "doc", None).unwrap();
    assert_eq!(outs.len(), ranges.len());
    let total: u16 = outs.iter().map(|o| ff_engine::page_count(&pdf, o, None).unwrap()).sum();
    assert_eq!(total, 10);
    for o in &outs {
        let sz = std::fs::metadata(o).unwrap().len();
        assert!(sz < 4096 * 2, "{} = {sz} byte", o.display());
    }
}

#[test]
fn auto_bookmarks_from_font_sizes() {
    let pdf = pdfium();
    let input = tmp("ff_bm_auto.pdf");
    build_pdf(&input, 3, &[]);
    let t = ff_engine::auto_bookmarks_from_headings(&pdf, &input, 3, None).unwrap();
    let titles: Vec<&str> = t.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, vec!["Chapter 1", "Chapter 2", "Chapter 3"]);
    for (i, b) in t.iter().enumerate() {
        assert_eq!(b.page_index, Some(i as u32));
        assert_eq!(b.children.len(), 1, "Section là cấp 2");
        assert_eq!(b.children[0].title, format!("Section {}.1", i + 1));
        assert!(b.top.unwrap() > 700.0);
    }
    // 1 cấp: Section dồn lên cùng cấp.
    let flat = ff_engine::auto_bookmarks_from_headings(&pdf, &input, 1, None).unwrap();
    assert_eq!(flat.len(), 6);
    // Ghi được luôn.
    let out = tmp("ff_bm_auto_out.pdf");
    ff_engine::set_outline(&input, &out, &t).unwrap();
    assert_eq!(ff_engine::get_outline(&out).unwrap().len(), 3);
}
