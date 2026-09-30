//! Round-trip cho chú thích mở rộng (tab Comment kiểu Foxit): tạo từng loại
//! Ink/Line/Arrow/Square/Circle/Polygon/PolyLine/Cloud/Stamp → mở lại bằng
//! PDFium (đúng loại, số lượng, khung, màu vẽ ra) + đọc chi tiết bằng
//! `list_annotations_detailed`; sửa/xoá annotation có sẵn; lưu tổng hợp.

use std::path::{Path, PathBuf};

use ff_engine::{
    AnnotKind, AnnotMeta, AnnotRef, AnnotSaveRequest, AnnotSpec, AnnotUpdate, Rect, ShapeKind,
    ShapeSpec, StampSpec,
};
use pdfium_render::prelude::*;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn pdfium() -> Pdfium {
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

fn meta() -> AnnotMeta {
    AnnotMeta { author: Some("Nguyễn Tester".into()), date: Some("D:20260930120000+07'00'".into()) }
}

/// (loại PDFium, khung) của mọi annotation trang `page`.
fn pdfium_annots(pdf: &Pdfium, path: &Path, page: u16) -> Vec<(PdfPageAnnotationType, Rect)> {
    let doc = pdf.load_pdf_from_file(path, None).expect("mở lại");
    let p = doc.pages().get(page).expect("trang");
    p.annotations()
        .iter()
        .map(|a| {
            let b = a.bounds().expect("bounds");
            (
                a.annotation_type(),
                Rect { left: b.left().value, bottom: b.bottom().value, right: b.right().value, top: b.top().value },
            )
        })
        .collect()
}

/// Số pixel gần màu `rgb` trong vùng (toạ độ PDF) khi render trang ở 1px/pt.
fn color_pixels(pdf: &Pdfium, path: &Path, page: u16, area: Rect, rgb: [u8; 3]) -> usize {
    let img = ff_engine::render::render_page(pdf, path, page, 612, None).expect("render").image.to_rgba8();
    let (w, h) = img.dimensions();
    let s = w as f32 / 612.0;
    let x0 = (area.left * s).max(0.0) as u32;
    let x1 = ((area.right * s) as u32).min(w - 1);
    let y0 = ((792.0 - area.top) * s).max(0.0) as u32;
    let y1 = (((792.0 - area.bottom) * s) as u32).min(h - 1);
    let mut n = 0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = img.get_pixel(x, y).0;
            if (0..3).all(|i| (p[i] as i16 - rgb[i] as i16).abs() < 60) {
                n += 1;
            }
        }
    }
    n
}

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn contains(outer: &Rect, inner: &Rect, tol: f32) -> bool {
    outer.left <= inner.left + tol
        && outer.bottom <= inner.bottom + tol
        && outer.right >= inner.right - tol
        && outer.top >= inner.top - tol
}

const RED: [u8; 3] = [230, 20, 20];
const BLUE: [u8; 3] = [20, 60, 230];

fn shape(kind: ShapeKind) -> ShapeSpec {
    let mut s = ShapeSpec::new(kind, 0, RED);
    s.width = 3.0;
    s
}

/// Tạo 1 shape → kiểm PDFium (loại + đúng 1 annotation + khung chứa hình) +
/// có pixel đúng màu trong vùng + chi tiết (tác giả, NM, màu, subtype).
fn roundtrip_one(pdf: &Pdfium, spec: ShapeSpec, expect: PdfPageAnnotationType, subtype: &str, geom: Rect) {
    let out = tmp(&format!("ff_annot_ext_{subtype}_{:?}.pdf", spec.kind));
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[spec.clone()], &meta()).expect("apply shape");

    let list = pdfium_annots(&pdf, &out, 0);
    assert_eq!(list.len(), 1, "trang 0 phải có đúng 1 annotation: {list:?}");
    assert_eq!(list[0].0, expect, "sai loại");
    assert!(contains(&list[0].1, &geom, 0.5), "khung {:?} phải bao hình {:?}", list[0].1, geom);
    assert!(
        list[0].1.right - list[0].1.left < (geom.right - geom.left) + 40.0,
        "khung quá rộng so với hình: {:?}",
        list[0].1
    );

    let px = color_pixels(&pdf, &out, 0, pad(geom, 4.0), spec.color);
    assert!(px > 20, "{subtype}: không thấy nét màu được render ({px} px)");

    let det = ff_engine::list_annotations_detailed(pdf, &out).expect("list detailed");
    assert_eq!(det.len(), 1);
    let d = &det[0];
    assert_eq!(d.subtype, subtype);
    assert_eq!(d.author.as_deref(), Some("Nguyễn Tester"));
    assert!(d.nm.as_deref().unwrap_or("").starts_with("ff-"), "thiếu /NM: {:?}", d.nm);
    assert_eq!(d.modified.as_deref(), Some("D:20260930120000+07'00'"));
    assert_eq!(d.created.as_deref(), Some("D:20260930120000+07'00'"));
    let c = d.color.expect("màu /C");
    assert!((0..3).all(|i| (c[i] as i16 - spec.color[i] as i16).abs() <= 1), "màu {c:?}");
}

fn pad(r: Rect, p: f32) -> Rect {
    Rect { left: r.left - p, bottom: r.bottom - p, right: r.right + p, top: r.top + p }
}

#[test]
fn ink_roundtrip() {
    let pdf = &pdfium();
    let mut s = shape(ShapeKind::Ink);
    s.strokes = vec![
        (0..20).map(|i| [100.0 + i as f32 * 5.0, 300.0 + (i as f32 * 0.5).sin() * 20.0]).collect(),
        vec![[120.0, 250.0], [180.0, 260.0], [220.0, 240.0]],
    ];
    roundtrip_one(pdf,s, PdfPageAnnotationType::Ink, "Ink", Rect { left: 100.0, bottom: 240.0, right: 220.0, top: 320.0 });
    // InkList giữ đủ 2 nét.
    let out = tmp("ff_annot_ext_ink_strokes.pdf");
    let mut s = shape(ShapeKind::Ink);
    s.strokes = vec![vec![[100.0, 300.0], [200.0, 300.0]], vec![[100.0, 280.0], [200.0, 260.0]]];
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[s], &meta()).unwrap();
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert_eq!(d[0].ink.len(), 2, "phải còn 2 nét: {:?}", d[0].ink);
    assert!(approx(d[0].width.unwrap(), 3.0, 0.01));
}

#[test]
fn line_and_arrow_roundtrip() {
    let pdf = &pdfium();
    let mut s = shape(ShapeKind::Line);
    s.points = vec![[100.0, 300.0], [300.0, 350.0]];
    roundtrip_one(pdf,s, PdfPageAnnotationType::Line, "Line", Rect { left: 100.0, bottom: 300.0, right: 300.0, top: 350.0 });

    let mut a = shape(ShapeKind::Arrow);
    a.color = BLUE;
    a.points = vec![[100.0, 300.0], [300.0, 300.0]];
    roundtrip_one(pdf,a.clone(), PdfPageAnnotationType::Line, "Line", Rect { left: 100.0, bottom: 299.0, right: 300.0, top: 301.0 });
    let out = tmp("ff_annot_ext_arrow_le.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[a], &meta()).unwrap();
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert_eq!(d[0].line_endings.as_ref().unwrap()[1], "OpenArrow");
    assert_eq!(d[0].line, Some([100.0, 300.0, 300.0, 300.0]));
    // Đầu mũi tên vẽ ra: có pixel xanh phía trên/dưới thân (cách thân ≥3pt) gần mũi.
    let wings = color_pixels(&pdf, &out, 0, Rect { left: 285.0, bottom: 303.0, right: 299.0, top: 312.0 }, BLUE);
    assert!(wings > 3, "không thấy cánh mũi tên ({wings} px)");
}

#[test]
fn square_circle_with_fill_and_opacity() {
    let pdf = &pdfium();
    let mut s = shape(ShapeKind::Square);
    s.rect = Rect { left: 100.0, bottom: 250.0, right: 250.0, top: 350.0 };
    s.fill = Some(BLUE);
    roundtrip_one(pdf,s.clone(), PdfPageAnnotationType::Square, "Square", s.rect);
    // Tô bên trong = xanh.
    let out = tmp("ff_annot_ext_square_fill.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[s], &meta()).unwrap();
    let inside = color_pixels(&pdf, &out, 0, Rect { left: 140.0, bottom: 280.0, right: 200.0, top: 320.0 }, BLUE);
    assert!(inside > 1500, "phần tô xanh thiếu ({inside} px)");
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert_eq!(d[0].fill, Some(BLUE));

    let mut c = shape(ShapeKind::Circle);
    c.rect = Rect { left: 300.0, bottom: 250.0, right: 450.0, top: 350.0 };
    c.opacity = 0.5;
    c.color = [0, 0, 0];
    let out = tmp("ff_annot_ext_circle.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[c.clone()], &meta()).unwrap();
    let list = pdfium_annots(&pdf, &out, 0);
    assert_eq!(list[0].0, PdfPageAnnotationType::Circle);
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert!(approx(d[0].opacity, 0.5, 0.01), "opacity {}", d[0].opacity);
    // Độ mờ 50% → nét đen hiện ra xám (~128), không phải đen tuyền.
    let gray = color_pixels(&pdf, &out, 0, pad(c.rect, 2.0), [128, 128, 128]);
    let black = color_pixels(&pdf, &out, 0, pad(c.rect, 2.0), [0, 0, 0]);
    assert!(gray > 20, "nét 50% phải ra xám ({gray} px, đen {black})");
    // Tâm oval không tô.
    let center = color_pixels(&pdf, &out, 0, Rect { left: 360.0, bottom: 290.0, right: 390.0, top: 310.0 }, [128, 128, 128]);
    assert_eq!(center, 0, "oval không tô không được có pixel ở tâm");
}

#[test]
fn polygon_polyline_cloud_roundtrip() {
    let pdf = &pdfium();
    let pts = vec![[100.0, 250.0], [250.0, 250.0], [200.0, 350.0], [120.0, 330.0]];
    let geom = Rect { left: 100.0, bottom: 250.0, right: 250.0, top: 350.0 };
    let mut p = shape(ShapeKind::Polygon);
    p.points = pts.clone();
    roundtrip_one(pdf,p, PdfPageAnnotationType::Polygon, "Polygon", geom);
    let mut l = shape(ShapeKind::PolyLine);
    l.points = pts.clone();
    roundtrip_one(pdf,l, PdfPageAnnotationType::Polyline, "PolyLine", geom);
    let mut c = shape(ShapeKind::Cloud);
    c.points = pts.clone();
    roundtrip_one(pdf,c.clone(), PdfPageAnnotationType::Polygon, "Polygon", geom);
    let out = tmp("ff_annot_ext_cloud.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[c], &meta()).unwrap();
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert!(d[0].cloudy, "Cloud phải có /BE /S /C");
    assert_eq!(d[0].vertices.len(), 4);
}

#[test]
fn text_stamp_roundtrip() {
    let pdf = &pdfium();
    let mut s = ShapeSpec::new(ShapeKind::Stamp, 0, [200, 30, 30]);
    s.rect = Rect { left: 100.0, bottom: 250.0, right: 300.0, top: 310.0 };
    s.stamp = Some(StampSpec {
        name: "Approved".into(),
        label: "ĐÃ DUYỆT".into(),
        sub: Some("30/09/2026 12:00 · Tester".into()),
        image: None,
    });
    roundtrip_one(pdf,s.clone(), PdfPageAnnotationType::Stamp, "Stamp", Rect { left: 101.0, bottom: 251.0, right: 299.0, top: 309.0 });
    let out = tmp("ff_annot_ext_stamp_name.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[s], &meta()).unwrap();
    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    assert_eq!(d[0].icon.as_deref(), Some("Approved"));
    assert!(d[0].subject.as_deref().unwrap_or("").starts_with("ĐÃ DUYỆT"));
}

#[test]
fn image_stamp_roundtrip() {
    // Ảnh PNG 40x20 xanh dương có kênh alpha.
    let png = tmp("ff_annot_ext_stamp.png");
    let mut img = image::RgbaImage::new(40, 20);
    for (x, _, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([20, 60, 230, if x < 4 { 0 } else { 255 }]);
    }
    img.save(&png).unwrap();
    let mut s = ShapeSpec::new(ShapeKind::Stamp, 0, [0, 0, 0]);
    s.rect = Rect { left: 100.0, bottom: 250.0, right: 260.0, top: 330.0 };
    s.stamp = Some(StampSpec { name: "FFImage".into(), image: Some(png), ..Default::default() });
    let pdf = &pdfium();
    let out = tmp("ff_annot_ext_stamp_img.pdf");
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[s], &meta()).unwrap();
    let list = pdfium_annots(&pdf, &out, 0);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].0, PdfPageAnnotationType::Stamp);
    let blue = color_pixels(&pdf, &out, 0, Rect { left: 130.0, bottom: 260.0, right: 250.0, top: 320.0 }, BLUE);
    assert!(blue > 5000, "ảnh con dấu không hiện ({blue} px)");
}

/// Tạo 3 annotation → sửa (dời + đổi màu + nội dung + tác giả) & xoá → kiểm lại.
#[test]
fn update_and_delete_existing() {
    let pdf = &pdfium();
    let base = tmp("ff_annot_ext_upd_base.pdf");
    let mut sq = shape(ShapeKind::Square);
    sq.rect = Rect { left: 100.0, bottom: 250.0, right: 200.0, top: 300.0 };
    let mut ink = shape(ShapeKind::Ink);
    ink.strokes = vec![vec![[300.0, 250.0], [400.0, 300.0]]];
    let mut ln = shape(ShapeKind::Arrow);
    ln.points = vec![[100.0, 400.0], [200.0, 400.0]];
    ff_engine::apply_shape_annotations(pdf, &sample(), &base, &[sq, ink, ln], &meta()).unwrap();
    let before = ff_engine::list_annotations_detailed(pdf, &base).unwrap();
    assert_eq!(before.len(), 3);

    // Sửa Square: dời sang (100,150)-(250,200) (rộng hơn), đổi xanh, thêm nội dung.
    let mut u = AnnotUpdate::new(AnnotRef { page_index: 0, annot_index: 0, nm: before[0].nm.clone() });
    u.rect = Some(Rect { left: 100.0, bottom: 150.0, right: 250.0, top: 200.0 });
    u.color = Some(BLUE);
    u.contents = Some("Đã sửa".into());
    u.author = Some("Người sửa".into());
    // Ink: dời lên 100pt (hình học InkList phải đi theo).
    let r1 = before[1].rect;
    let mut u2 = AnnotUpdate::new(AnnotRef { page_index: 0, annot_index: 1, nm: None });
    u2.rect = Some(Rect { left: r1.left, bottom: r1.bottom + 100.0, right: r1.right, top: r1.top + 100.0 });
    u2.width = Some(6.0);
    let upd = tmp("ff_annot_ext_upd.pdf");
    ff_engine::update_annotations(pdf, &base, &upd, &[u, u2], &AnnotMeta { author: None, date: Some("D:20261001000000Z".into()) }).unwrap();

    let after = ff_engine::list_annotations_detailed(pdf, &upd).unwrap();
    assert_eq!(after.len(), 3);
    let s = &after[0];
    assert_eq!(s.color, Some(BLUE));
    assert_eq!(s.contents.as_deref(), Some("Đã sửa"));
    assert_eq!(s.author.as_deref(), Some("Người sửa"));
    assert_eq!(s.modified.as_deref(), Some("D:20261001000000Z"));
    assert!(approx(s.rect.left, 100.0, 0.5) && approx(s.rect.top, 200.0, 0.5) && approx(s.rect.right, 250.0, 0.5));
    // AP dựng lại: pixel xanh ở chỗ mới, không còn đỏ ở chỗ cũ.
    assert!(color_pixels(&pdf, &upd, 0, Rect { left: 98.0, bottom: 148.0, right: 252.0, top: 202.0 }, BLUE) > 100);
    assert_eq!(color_pixels(&pdf, &upd, 0, Rect { left: 98.0, bottom: 248.0, right: 202.0, top: 302.0 }, RED), 0);
    // Ink đã dời theo.
    let ink_after = &after[1];
    assert!(approx(ink_after.ink[0][0][1], 350.0, 0.5), "InkList phải dời lên: {:?}", ink_after.ink);
    assert!(approx(ink_after.width.unwrap(), 6.0, 0.01));
    assert!(color_pixels(&pdf, &upd, 0, Rect { left: 295.0, bottom: 345.0, right: 405.0, top: 405.0 }, RED) > 50);

    // Xoá mũi tên (theo NM, cố tình sai index để kiểm tra tìm theo /NM).
    let del = tmp("ff_annot_ext_del.pdf");
    ff_engine::delete_annotations(pdf, &upd, &del, &[AnnotRef { page_index: 0, annot_index: 0, nm: after[2].nm.clone() }]).unwrap();
    let left = ff_engine::list_annotations_detailed(pdf, &del).unwrap();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|a| a.subtype != "Line"), "mũi tên phải bị xoá: {left:?}");
    assert_eq!(pdfium_annots(&pdf, &del, 0).len(), 2);
    // NM sai → báo lỗi rõ, không ghi file.
    let bad = tmp("ff_annot_ext_bad.pdf");
    let err = ff_engine::delete_annotations(pdf, &del, &bad, &[AnnotRef { page_index: 0, annot_index: 0, nm: Some("khong-co".into()) }]);
    assert!(err.is_err());
    assert!(!bad.exists());
}

/// Lưu tổng hợp 1 lần: sửa + xoá annotation có sẵn + tạo markup/text + shape;
/// annotation tạo qua PDFium cũng được gắn tác giả/NM.
#[test]
fn save_annotations_combined() {
    let pdf = &pdfium();
    let base = tmp("ff_annot_ext_comb_base.pdf");
    let mut sq = shape(ShapeKind::Square);
    sq.rect = Rect { left: 100.0, bottom: 250.0, right: 200.0, top: 300.0 };
    let mut ci = shape(ShapeKind::Circle);
    ci.rect = Rect { left: 300.0, bottom: 250.0, right: 400.0, top: 300.0 };
    ff_engine::apply_shape_annotations(pdf, &sample(), &base, &[sq, ci], &AnnotMeta::default()).unwrap();

    let mut u = AnnotUpdate::new(AnnotRef { page_index: 0, annot_index: 0, nm: None });
    u.contents = Some("Ghi chú mới".into());
    let mut ink = shape(ShapeKind::Ink);
    ink.page_index = 1;
    ink.strokes = vec![vec![[100.0, 100.0], [150.0, 150.0]]];
    let req = AnnotSaveRequest {
        specs: vec![
            AnnotSpec::markup(AnnotKind::Highlight, 0, Rect { left: 155.0, bottom: 690.0, right: 213.0, top: 706.0 }, [255, 255, 0, 255]),
            AnnotSpec {
                kind: AnnotKind::Note,
                page_index: 1,
                rect: Rect { left: 50.0, bottom: 700.0, right: 68.0, top: 718.0 },
                quads: vec![],
                color: [255, 200, 0, 255],
                contents: Some("Ghi chú".into()),
                font_size: 12.0,
                bold: false,
                italic: false,
                underline: false,
            },
        ],
        shapes: vec![ink],
        updates: vec![u],
        deletes: vec![AnnotRef { page_index: 0, annot_index: 1, nm: None }],
        meta: meta(),
    };
    let out = tmp("ff_annot_ext_comb.pdf");
    ff_engine::save_annotations(&pdf, &base, &out, &req).expect("save_annotations");
    assert!(!out.with_extension("ffannot.tmp.pdf").exists(), "file tạm phải được dọn");

    let d = ff_engine::list_annotations_detailed(pdf, &out).unwrap();
    let p0: Vec<_> = d.iter().filter(|a| a.page_index == 0).collect();
    let p1: Vec<_> = d.iter().filter(|a| a.page_index == 1).collect();
    let k0: Vec<&str> = p0.iter().map(|a| a.subtype.as_str()).collect();
    assert_eq!(k0, vec!["Square", "Highlight"], "trang 0: Circle bị xoá, thêm Highlight");
    assert_eq!(p0[0].contents.as_deref(), Some("Ghi chú mới"));
    assert!(p1.iter().any(|a| a.subtype == "Text" && a.contents.as_deref() == Some("Ghi chú")));
    assert!(p1.iter().any(|a| a.subtype == "Ink"));
    // Mọi annotation MỚI đều có tác giả + NM.
    for a in d.iter().filter(|a| a.subtype != "Square") {
        assert_eq!(a.author.as_deref(), Some("Nguyễn Tester"), "{} thiếu /T", a.subtype);
        assert!(a.nm.is_some(), "{} thiếu /NM", a.subtype);
    }
    assert_eq!(pdfium_annots(&pdf, &out, 0).len(), 2);
}

/// Render ẩn annotation đang sửa (UI vẽ preview thay) — pixel của nó biến mất.
#[test]
fn render_hiding_hides_annotation() {
    let pdf = &pdfium();
    let out = tmp("ff_annot_ext_hide.pdf");
    let mut sq = shape(ShapeKind::Square);
    sq.rect = Rect { left: 100.0, bottom: 250.0, right: 200.0, top: 300.0 };
    sq.fill = Some(RED);
    ff_engine::apply_shape_annotations(pdf, &sample(), &out, &[sq], &meta()).unwrap();
    let count = |img: image::RgbaImage| img.pixels().filter(|p| p.0[0] > 200 && p.0[1] < 80 && p.0[2] < 80).count();
    let shown = ff_engine::render::render_page(&pdf, &out, 0, 612, None).unwrap().image.to_rgba8();
    let hidden = ff_engine::render_page_hiding(&pdf, &out, 0, 612, &[0]).unwrap().to_rgba8();
    assert!(count(shown) > 3000);
    assert_eq!(count(hidden), 0);
}
