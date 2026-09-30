//! So sánh tài liệu (Compare Files): dựng 2 PDF bằng lopdf có nội dung biết
//! trước (chèn 1 từ, xoá 1 câu, thay 1 từ, chèn 1 trang) → kiểm bản ghi thay
//! đổi; diff hình ảnh bắt được khối chữ nhật bị dời; báo cáo xuất đúng số chú
//! thích + trang bìa có liên kết.

use std::path::{Path, PathBuf};

use ff_engine::{ChangeKind, CompareMode, CompareOptions};

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
    let p = std::env::temp_dir().join(format!("ff_compare_{name}"));
    let _ = std::fs::remove_file(&p);
    p
}

/// Mỗi trang: danh sách (y, dòng chữ) Helvetica 12pt ở x=72 + lệnh vẽ thêm.
struct PageSpec<'a> {
    lines: Vec<(f32, &'a str)>,
    extra: &'a str,
}

fn page<'a>(lines: &[(f32, &'a str)]) -> PageSpec<'a> {
    PageSpec { lines: lines.to_vec(), extra: "" }
}

fn build_pdf(path: &Path, pages: &[PageSpec]) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut kids: Vec<Object> = Vec::new();
    for p in pages {
        let mut content = String::new();
        content.push_str(p.extra);
        content.push('\n');
        for (y, t) in &p.lines {
            content.push_str(&format!("BT /F1 12 Tf 72 {y} Td ({t}) Tj ET\n"));
        }
        let cid = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let pid = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            "Contents" => cid,
        });
        kids.push(pid.into());
    }
    let n = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("lưu fixture");
}

fn text_pair() -> (PathBuf, PathBuf) {
    let old = tmp("old.pdf");
    let new = tmp("new.pdf");
    build_pdf(
        &old,
        &[
            page(&[
                (700.0, "The quick brown fox jumps over the lazy dog."),
                (680.0, "This sentence will be deleted soon."),
                (660.0, "Alpha beta gamma delta epsilon."),
            ]),
            page(&[(700.0, "Second page content stays the same here.")]),
        ],
    );
    build_pdf(
        &new,
        &[
            page(&[
                (700.0, "The quick brown fox swiftly jumps over the lazy dog."),
                (680.0, "Alpha beta omega delta epsilon."),
            ]),
            page(&[(700.0, "Brand new inserted page with unique words xylophone.")]),
            page(&[(700.0, "Second page content stays the same here.")]),
        ],
    );
    (old, new)
}

fn opts(mode: CompareMode) -> CompareOptions {
    CompareOptions { mode, ..Default::default() }
}

#[test]
fn text_compare_detects_insert_delete_replace_and_page() {
    let pdf = pdfium();
    let (old, new) = text_pair();
    let mut stages = Vec::new();
    let r = ff_engine::compare_documents(&pdf, &old, &new, &opts(CompareMode::Text), &mut |p| {
        stages.push(p.stage);
        true
    })
    .expect("compare");
    assert!(stages.contains(&"text") && stages.contains(&"diff"), "thiếu tiến độ: {stages:?}");

    // Căn trang: cũ 0↔mới 0, mới 1 chèn, cũ 1↔mới 2.
    let map: Vec<(Option<u16>, Option<u16>)> = r.page_map.iter().map(|p| (p.old, p.new)).collect();
    assert_eq!(map, vec![(Some(0), Some(0)), (None, Some(1)), (Some(1), Some(2))]);

    let s = &r.summary;
    assert_eq!(
        (s.inserted, s.deleted, s.replaced, s.page_inserted, s.page_deleted, s.graphics),
        (1, 1, 1, 1, 0, 0),
        "changes: {:#?}",
        r.changes
    );

    let ins = r.changes.iter().find(|c| c.kind == ChangeKind::Inserted).unwrap();
    assert_eq!(ins.new_text, "swiftly");
    assert_eq!((ins.old_page, ins.new_page), (Some(0), Some(0)));
    let rr = ins.new_rects[0];
    assert!(rr.bottom < 703.0 && rr.top > 703.0 && rr.left > 150.0, "hộp 'swiftly' sai: {rr:?}");
    assert_eq!(ins.old_rects.len(), 1, "phải có dấu mũ bên cũ");

    let del = r.changes.iter().find(|c| c.kind == ChangeKind::Deleted).unwrap();
    assert_eq!(del.old_text, "This sentence will be deleted soon.");
    assert_eq!(del.old_page, Some(0));
    assert_eq!(del.old_rects.len(), 1, "câu xoá nằm trên 1 dòng → 1 hộp");
    let dr = del.old_rects[0];
    assert!(dr.bottom < 683.0 && dr.top > 683.0, "hộp câu xoá sai dòng: {dr:?}");
    assert_eq!(del.new_page, Some(0));

    let rep = r.changes.iter().find(|c| c.kind == ChangeKind::Replaced).unwrap();
    assert_eq!((rep.old_text.as_str(), rep.new_text.as_str()), ("gamma", "omega"));
    assert!(rep.new_rects[0].bottom < 683.0 && rep.new_rects[0].top > 683.0);

    let pi = r.changes.iter().find(|c| c.kind == ChangeKind::PageInserted).unwrap();
    assert_eq!(pi.new_page, Some(1));
    assert!(pi.new_text.contains("xylophone"));

    // Thứ tự: theo hàng trang rồi từ trên xuống; id liên tục.
    let kinds: Vec<ChangeKind> = r.changes.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![ChangeKind::Inserted, ChangeKind::Deleted, ChangeKind::Replaced, ChangeKind::PageInserted]
    );
    assert!(r.changes.iter().enumerate().all(|(i, c)| c.id == i));
}

#[test]
fn page_deleted_and_ranges() {
    let pdf = pdfium();
    let (old, new) = text_pair();
    // Đảo vai: mới→cũ ⇒ trang bị xoá.
    let r = ff_engine::compare_documents(&pdf, &new, &old, &opts(CompareMode::Text), &mut |_| true).unwrap();
    assert_eq!(r.summary.page_deleted, 1);
    assert_eq!(r.summary.inserted, 1, "câu bị xoá giờ là chèn");
    let pd = r.changes.iter().find(|c| c.kind == ChangeKind::PageDeleted).unwrap();
    assert_eq!(pd.old_page, Some(1));
    assert!(pd.new_page.is_some(), "trang xoá phải có trang lân cận bên mới");

    // Chỉ so trang cuối của mỗi bên → giống hệt.
    let o = CompareOptions { old_pages: Some(vec![1]), new_pages: Some(vec![2]), ..opts(CompareMode::Text) };
    let r = ff_engine::compare_documents(&pdf, &old, &new, &o, &mut |_| true).unwrap();
    assert_eq!(r.changes.len(), 0, "{:#?}", r.changes);
    assert_eq!(r.page_map.len(), 1);
}

#[test]
fn ignore_case_and_punctuation() {
    let pdf = pdfium();
    let a = tmp("case_a.pdf");
    let b = tmp("case_b.pdf");
    build_pdf(&a, &[page(&[(700.0, "Hello World, again!")])]);
    build_pdf(&b, &[page(&[(700.0, "hello world again")])]);
    let strict = ff_engine::compare_documents(&pdf, &a, &b, &opts(CompareMode::Text), &mut |_| true).unwrap();
    assert!(strict.summary.total() > 0);
    let o = CompareOptions { ignore_case: true, ignore_punctuation: true, ..opts(CompareMode::Text) };
    let loose = ff_engine::compare_documents(&pdf, &a, &b, &o, &mut |_| true).unwrap();
    assert_eq!(loose.summary.total(), 0, "{:#?}", loose.changes);
}

#[test]
fn cancel_stops_compare() {
    let pdf = pdfium();
    let (old, new) = text_pair();
    let err = ff_engine::compare_documents(&pdf, &old, &new, &opts(CompareMode::Both), &mut |_| false)
        .err()
        .expect("phải bị huỷ");
    assert!(err.to_string().contains(ff_engine::COMPARE_CANCELLED));
}

#[test]
fn visual_compare_detects_moved_rectangle() {
    let pdf = pdfium();
    let a = tmp("vis_a.pdf");
    let b = tmp("vis_b.pdf");
    let text = [(700.0, "Same caption text on both pages.")];
    build_pdf(&a, &[PageSpec { lines: text.to_vec(), extra: "0.1 0.3 0.8 rg 100 400 80 60 re f" }]);
    build_pdf(&b, &[PageSpec { lines: text.to_vec(), extra: "0.1 0.3 0.8 rg 300 400 80 60 re f" }]);

    let r = ff_engine::compare_documents(&pdf, &a, &b, &opts(CompareMode::Both), &mut |_| true).unwrap();
    assert_eq!(r.summary.inserted + r.summary.deleted + r.summary.replaced, 0, "chữ không đổi");
    let g: Vec<_> = r.changes.iter().filter(|c| c.kind == ChangeKind::Graphics).collect();
    assert!(!g.is_empty(), "không phát hiện đồ hoạ đổi");
    let covers = |x: f32, y: f32| {
        g.iter().any(|c| {
            let r = c.new_rects[0];
            r.left <= x && x <= r.right && r.bottom <= y && y <= r.top
        })
    };
    assert!(covers(140.0, 430.0), "thiếu vùng vị trí cũ: {g:#?}");
    assert!(covers(340.0, 430.0), "thiếu vùng vị trí mới: {g:#?}");
    for c in &g {
        let r = c.new_rects[0];
        assert!(r.bottom > 380.0 && r.top < 480.0, "vùng lan quá rộng: {r:?}");
    }

    // Cùng một tệp → không có khác biệt nào (kể cả chế độ chỉ hình ảnh).
    let same = ff_engine::compare_documents(&pdf, &a, &a, &opts(CompareMode::Visual), &mut |_| true).unwrap();
    assert_eq!(same.changes.len(), 0);
}

#[test]
fn report_has_expected_annotations_and_cover() {
    let pdf = pdfium();
    let (old, new) = text_pair();
    let r = ff_engine::compare_documents(&pdf, &old, &new, &opts(CompareMode::Text), &mut |_| true).unwrap();
    let out = tmp("report.pdf");
    let info = ff_engine::ReportInfo {
        old_name: "old.pdf".into(),
        new_name: "new.pdf".into(),
        labels: ff_engine::ReportLabels { date_line: "Date: 2026-09-30".into(), ..Default::default() },
    };
    let stats = ff_engine::export_compare_report(&pdf, &r, &out, &info).expect("export");
    assert_eq!(stats.cover_pages, 1);
    assert_eq!(stats.annotations, 4);
    assert_eq!(stats.links, 4);

    // Mở lại: 1 bìa + 3 trang của tài liệu mới; tệp mới không bị sửa.
    assert_eq!(ff_engine::page_count(&pdf, &out, None).unwrap(), 4);
    assert_eq!(ff_engine::page_count(&pdf, &new, None).unwrap(), 3);
    assert_eq!(ff_engine::list_annotations(&pdf, &new).unwrap().len(), 0);

    let annots = ff_engine::list_annotations(&pdf, &out).unwrap();
    let links = annots.iter().filter(|a| a.page_index == 0 && a.kind == "Link").count();
    assert_eq!(links, 4);
    let marks: Vec<_> = annots.iter().filter(|a| a.page_index >= 1).collect();
    assert_eq!(marks.len(), 4, "{marks:#?}");
    let hl: Vec<_> = marks.iter().filter(|a| a.kind == "Highlight").collect();
    assert_eq!(hl.len(), 2);
    assert!(hl.iter().any(|a| a.contents.as_deref().unwrap_or("").contains("swiftly")), "{hl:#?}");
    assert!(hl.iter().any(|a| a.contents.as_deref().unwrap_or("").contains("gamma")));
    let note = marks.iter().find(|a| a.kind == "Text").expect("ghi chú chữ bị xoá");
    assert!(note.contents.as_deref().unwrap_or("").contains("deleted soon"));
    assert_eq!(note.page_index, 1);
    let sq = marks.iter().find(|a| a.kind == "Square").expect("khung trang chèn");
    assert_eq!(sq.page_index, 2);

    // Trang bìa có chữ (render không trắng).
    let img = ff_engine::render::render_page(&pdf, &out, 0, 300, None).unwrap().image.to_rgba8();
    let dark = img.pixels().filter(|p| p.0[0] < 128 && p.0[1] < 128 && p.0[2] < 128).count();
    assert!(dark > 200, "trang bìa trống? dark={dark}");
}

#[test]
fn large_document_is_fast_and_accurate() {
    let pdf = pdfium();
    let a = tmp("big_a.pdf");
    let b = tmp("big_b.pdf");
    let line = |p: usize, l: usize| format!("Page {p} line {l} lorem ipsum dolor sit amet consectetur adipiscing elit");
    let lines_a: Vec<Vec<String>> = (0..120).map(|p| (0..30).map(|l| line(p, l)).collect()).collect();
    let mut lines_b = lines_a.clone();
    lines_b[60][10] = lines_b[60][10].replace("dolor", "DOLOR");
    lines_b.insert(30, vec!["Freshly inserted page".to_string()]);
    let mk = |v: &Vec<Vec<String>>| -> Vec<Vec<(f32, String)>> {
        v.iter().map(|ls| ls.iter().enumerate().map(|(i, s)| (740.0 - i as f32 * 22.0, s.clone())).collect()).collect()
    };
    let (pa, pb) = (mk(&lines_a), mk(&lines_b));
    fn as_pages(v: &[Vec<(f32, String)>]) -> Vec<PageSpec<'_>> {
        v.iter().map(|ls| PageSpec { lines: ls.iter().map(|(y, s)| (*y, s.as_str())).collect(), extra: "" }).collect()
    }
    build_pdf(&a, &as_pages(&pa));
    build_pdf(&b, &as_pages(&pb));

    let t0 = std::time::Instant::now();
    let r = ff_engine::compare_documents(&pdf, &a, &b, &opts(CompareMode::Text), &mut |_| true).unwrap();
    let el = t0.elapsed();
    eprintln!("so sánh 120 vs 121 trang: {el:?}");
    assert!(el.as_secs_f32() < 20.0, "quá chậm: {el:?}");
    assert_eq!(r.summary.page_inserted, 1);
    assert_eq!(r.summary.replaced, 1, "{:#?}", r.changes);
    assert_eq!(r.summary.total(), 2);
    let rep = r.changes.iter().find(|c| c.kind == ChangeKind::Replaced).unwrap();
    assert_eq!((rep.old_page, rep.new_page), (Some(60), Some(61)));
}
