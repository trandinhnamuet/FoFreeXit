//! Round-trip test cho "Edit Object" chuẩn Foxit (editobj.rs): xoay/lật/cắt/
//! độ mờ/trích ảnh, vẽ hình (Add Shapes), đổi viền-nền path, nhân bản, sắp lớp
//! (z-order), di chuyển nhiều object, giữ căn phải khi reflow. Mọi test: ghi
//! file → MỞ LẠI bằng PDFium (list_objects / render điểm ảnh) → so khớp.
//!
//! Fixture tự dựng bằng lopdf: trang 612×792, ảnh 4×2 px (nửa trái ĐỎ, nửa
//! phải XANH DƯƠNG) vẽ ở khung [100,500]–[300,600].

use std::path::{Path, PathBuf};

use ff_engine::{ArrangeMode, EditOp, ObjectInfo, ObjectKind, PathStyle, EditShapeKind as ShapeKind, EditShapeSpec as ShapeSpec};

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

/// PDF 1 trang: các content stream `streams` (nhiều phần tử = /Contents mảng),
/// tài nguyên ảnh /Im1 (4×2 RGB) + font /F1 Helvetica.
fn build_pdf(path: &Path, streams: &[&[u8]]) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    // 4×2 px: cột 0-1 đỏ, cột 2-3 xanh dương (cả 2 hàng).
    let mut px: Vec<u8> = Vec::new();
    for _row in 0..2 {
        for col in 0..4 {
            px.extend_from_slice(if col < 2 { &[255, 0, 0] } else { &[0, 0, 255] });
        }
    }
    let img_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 4, "Height" => 2,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        px,
    ));
    let content_ids: Vec<Object> = streams
        .iter()
        .map(|s| doc.add_object(Stream::new(dictionary! {}, s.to_vec())).into())
        .collect();
    let contents: Object = if content_ids.len() == 1 {
        content_ids[0].clone()
    } else {
        Object::Array(content_ids)
    };
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Im1" => img_id },
            "Font" => dictionary! { "F1" => font_id },
        },
        "Contents" => contents,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("lưu fixture");
}

const IMG_CONTENT: &[u8] = b"q 200 0 0 100 100 500 cm /Im1 Do Q BT /F1 20 Tf 100 300 Td (Hello object) Tj ET";

fn image_fixture(name: &str) -> PathBuf {
    let p = tmp(name);
    build_pdf(&p, &[IMG_CONTENT]);
    p
}

fn list(pdf: &pdfium_render::prelude::Pdfium, p: &Path) -> Vec<ObjectInfo> {
    ff_engine::list_objects(pdf, p, 0, None).expect("list_objects")
}

fn first_of(objs: &[ObjectInfo], kind: ObjectKind) -> ObjectInfo {
    objs.iter().find(|o| o.kind == kind).cloned().unwrap_or_else(|| panic!("thiếu object {kind:?}: {objs:?}"))
}

/// Render trang 0 ở 1px/pt (612×792) → RGB.
fn render(pdf: &pdfium_render::prelude::Pdfium, p: &Path) -> image::RgbImage {
    ff_engine::render::render_page(pdf, p, 0, 612, None).expect("render").image.to_rgb8()
}

/// Điểm ảnh tại toạ độ PDF (x,y) (y lên).
fn px_at(img: &image::RgbImage, x: f32, y: f32) -> [u8; 3] {
    let h = img.height() as f32;
    img.get_pixel(x as u32, (h - y) as u32).0
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ---------------- Ảnh: xoay / lật / cắt / độ mờ / trích ----------------

#[test]
fn rotate_image_90_swaps_bounds_around_center() {
    let pdf = pdfium();
    let input = image_fixture("ffx_rot_in.pdf");
    let out = tmp("ffx_rot_out.pdf");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Rotate { index: img.index, degrees: 90.0, center: None }], &out, None)
        .expect("rotate");
    let after = first_of(&list(&pdf, &out), ObjectKind::Image);
    let r = after.rect;
    // Khung 200×100 tâm (200,550) → xoay 90° thành 100×200 cùng tâm.
    assert!(near(r.left, 150.0, 1.0) && near(r.right, 250.0, 1.0), "{r:?}");
    assert!(near(r.bottom, 450.0, 1.0) && near(r.top, 650.0, 1.0), "{r:?}");
    assert!(near(after.rotation.unwrap_or(0.0), 90.0, 0.5), "góc xoay phải ~90: {:?}", after.rotation);
    // Xoay trái 90°: nửa TRÁI (đỏ) của ảnh quay xuống DƯỚI.
    let im = render(&pdf, &out);
    let bottom = px_at(&im, 200.0, 480.0);
    let top = px_at(&im, 200.0, 620.0);
    assert!(bottom[0] > 200 && bottom[2] < 60, "dưới phải đỏ: {bottom:?}");
    assert!(top[2] > 200 && top[0] < 60, "trên phải xanh: {top:?}");
}

/// Dòng chữ nhiều run: UI truyền TÂM CHUNG của dòng → xoay 180° cả dòng như
/// 1 khối (2 run đổi chỗ trái↔phải), không phải mỗi run tự quay tại chỗ.
#[test]
fn rotate_runs_around_shared_center_moves_as_block() {
    let pdf = pdfium();
    let input = tmp("ffx_rotc_in.pdf");
    build_pdf(&input, &[b"BT /F1 20 Tf 100 400 Td (AAA) Tj ET BT /F1 20 Tf 300 400 Td (BBB) Tj ET"]);
    let out = tmp("ffx_rotc_out.pdf");
    let objs = list(&pdf, &input);
    let texts: Vec<&ObjectInfo> = objs.iter().filter(|o| o.kind == ObjectKind::Text).collect();
    let r = |o: &ObjectInfo| o.rect;
    let (l, rr) = (r(texts[0]).left.min(r(texts[1]).left), r(texts[0]).right.max(r(texts[1]).right));
    let (b, t) = (r(texts[0]).bottom.min(r(texts[1]).bottom), r(texts[0]).top.max(r(texts[1]).top));
    let c = Some(((l + rr) / 2.0, (b + t) / 2.0));
    let ops: Vec<EditOp> = texts.iter().map(|o| EditOp::Rotate { index: o.index, degrees: 180.0, center: c }).collect();
    ff_engine::apply_edits(&pdf, &input, 0, &ops, &out, None).expect("rotate block");
    let after = list(&pdf, &out);
    let a = after.iter().find(|o| o.text.as_deref().map_or(false, |s| s.contains("AAA"))).unwrap();
    let bb = after.iter().find(|o| o.text.as_deref().map_or(false, |s| s.contains("BBB"))).unwrap();
    assert!(a.rect.left > bb.rect.left + 100.0, "AAA phải sang PHẢI của BBB: {:?} {:?}", a.rect, bb.rect);
    // Khung bao cả khối giữ nguyên chỗ (quay quanh tâm khối).
    assert!(near(a.rect.right, rr, 4.0) && near(bb.rect.left, l, 4.0), "{:?} {:?} l={l} r={rr}", a.rect, bb.rect);
    assert!(near(a.rotation.unwrap_or(0.0).abs(), 180.0, 0.5), "{:?}", a.rotation);
}

#[test]
fn flip_horizontal_mirrors_pixels_keeps_bounds() {
    let pdf = pdfium();
    let input = image_fixture("ffx_flip_in.pdf");
    let out = tmp("ffx_flip_out.pdf");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Flip { index: img.index, horizontal: true, center: None }], &out, None)
        .expect("flip");
    let r = first_of(&list(&pdf, &out), ObjectKind::Image).rect;
    assert!(near(r.left, 100.0, 1.0) && near(r.right, 300.0, 1.0) && near(r.bottom, 500.0, 1.0) && near(r.top, 600.0, 1.0), "{r:?}");
    let im = render(&pdf, &out);
    let left = px_at(&im, 130.0, 550.0);
    let right = px_at(&im, 270.0, 550.0);
    assert!(left[2] > 200 && left[0] < 60, "sau lật, trái phải xanh: {left:?}");
    assert!(right[0] > 200 && right[2] < 60, "sau lật, phải phải đỏ: {right:?}");

    // Lật dọc 2 lần = như cũ (ma trận đúng tâm).
    let out2 = tmp("ffx_flipv_out.pdf");
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[EditOp::Flip { index: img.index, horizontal: false, center: None }, EditOp::Flip { index: img.index, horizontal: false, center: None }],
        &out2,
        None,
    )
    .expect("flip v x2");
    let r2 = first_of(&list(&pdf, &out2), ObjectKind::Image).rect;
    assert!(near(r2.bottom, 500.0, 1.0) && near(r2.top, 600.0, 1.0), "{r2:?}");
}

#[test]
fn crop_image_keeps_only_region_pixels() {
    let pdf = pdfium();
    let input = image_fixture("ffx_crop_in.pdf");
    let out = tmp("ffx_crop_out.pdf");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    assert_eq!(img.image_px, Some((4, 2)));
    let keep = ff_engine::Rect { left: 90.0, bottom: 480.0, right: 200.0, top: 620.0 }; // nửa trái
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::CropImage { index: img.index, keep }], &out, None)
        .expect("crop");
    let after = first_of(&list(&pdf, &out), ObjectKind::Image);
    let r = after.rect;
    assert!(near(r.left, 100.0, 1.0) && near(r.right, 200.0, 1.0), "{r:?}");
    assert!(near(r.bottom, 500.0, 1.0) && near(r.top, 600.0, 1.0), "{r:?}");
    assert_eq!(after.image_px, Some((2, 2)), "điểm ảnh ngoài vùng phải bị bỏ hẳn");
    let im = render(&pdf, &out);
    let kept = px_at(&im, 150.0, 550.0);
    let gone = px_at(&im, 260.0, 550.0);
    assert!(kept[0] > 200 && kept[2] < 60, "phần giữ vẫn đỏ: {kept:?}");
    assert!(gone.iter().all(|&c| c > 240), "phần cắt bỏ phải trắng: {gone:?}");
}

#[test]
fn crop_rotated_image_uses_image_space() {
    let pdf = pdfium();
    let input = image_fixture("ffx_crop_rot_in.pdf");
    let rotated = tmp("ffx_crop_rot_mid.pdf");
    let out = tmp("ffx_crop_rot_out.pdf");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Rotate { index: img.index, degrees: 90.0, center: None }], &rotated, None)
        .expect("rotate");
    // Sau xoay: khung [150,450]-[250,650], nửa đỏ ở DƯỚI. Giữ nửa trên (xanh).
    let keep = ff_engine::Rect { left: 140.0, bottom: 550.0, right: 260.0, top: 660.0 };
    ff_engine::apply_edits(&pdf, &rotated, 0, &[EditOp::CropImage { index: img.index, keep }], &out, None)
        .expect("crop rotated");
    let after = first_of(&list(&pdf, &out), ObjectKind::Image);
    assert_eq!(after.image_px, Some((2, 2)));
    assert!(near(after.rect.bottom, 550.0, 1.0) && near(after.rect.top, 650.0, 1.0), "{:?}", after.rect);
    let im = render(&pdf, &out);
    let p = px_at(&im, 200.0, 600.0);
    assert!(p[2] > 200 && p[0] < 60, "phần giữ phải xanh: {p:?}");
}

#[test]
fn image_opacity_renders_translucent() {
    let pdf = pdfium();
    let input = image_fixture("ffx_opa_in.pdf");
    let out = tmp("ffx_opa_out.pdf");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::SetOpacity { index: img.index, opacity: 0.5 }], &out, None)
        .expect("opacity");
    let im = render(&pdf, &out);
    let p = px_at(&im, 150.0, 550.0);
    // Đỏ 50% trên nền trắng ≈ (255,128,128).
    assert!(p[0] > 230 && p[1] > 90 && p[1] < 170 && p[2] > 90 && p[2] < 170, "phải mờ 50%: {p:?}");
    let r = first_of(&list(&pdf, &out), ObjectKind::Image).rect;
    assert!(near(r.left, 100.0, 1.0) && near(r.top, 600.0, 1.0), "khung giữ nguyên: {r:?}");

    // Đặt lại 50% lần nữa: độ đục là giá trị TUYỆT ĐỐI, không cộng dồn thành 25%.
    let out2 = tmp("ffx_opa_out2.pdf");
    ff_engine::apply_edits(&pdf, &out, 0, &[EditOp::SetOpacity { index: img.index, opacity: 0.5 }], &out2, None)
        .expect("opacity again");
    let p2 = px_at(&render(&pdf, &out2), 150.0, 550.0);
    assert!(p2[1] > 90 && p2[1] < 170, "không được cộng dồn: {p2:?}");
    // Về lại 100%.
    let out3 = tmp("ffx_opa_out3.pdf");
    ff_engine::apply_edits(&pdf, &out2, 0, &[EditOp::SetOpacity { index: img.index, opacity: 1.0 }], &out3, None)
        .expect("opacity 100");
    let p3 = px_at(&render(&pdf, &out3), 150.0, 550.0);
    assert!(p3[0] > 230 && p3[1] < 40, "100% phải đỏ đặc trở lại: {p3:?}");
    assert!(near(first_of(&list(&pdf, &out2), ObjectKind::Image).opacity.unwrap_or(0.0), 0.5, 0.03));
}

#[test]
fn extract_image_writes_png_with_original_pixels() {
    let pdf = pdfium();
    let input = image_fixture("ffx_extract_in.pdf");
    let png = tmp("ffx_extract.png");
    let img = first_of(&list(&pdf, &input), ObjectKind::Image);
    let (w, h) = ff_engine::extract_image(&pdf, &input, 0, img.index, &png, None).expect("extract");
    assert_eq!((w, h), (4, 2));
    let back = image::open(&png).expect("đọc PNG").to_rgb8();
    assert_eq!(back.dimensions(), (4, 2));
    assert_eq!(back.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(back.get_pixel(3, 1).0, [0, 0, 255]);
}

// ---------------- Hình vẽ (Add Shapes) + viền/nền ----------------

fn shape(kind: ShapeKind, x0: f32, y0: f32, x1: f32, y1: f32) -> ShapeSpec {
    ShapeSpec {
        kind,
        x0,
        y0,
        x1,
        y1,
        stroke: Some([220, 0, 0]),
        fill: Some([0, 160, 0]),
        width: 2.0,
        dash: vec![],
        opacity: 1.0,
    }
}

#[test]
fn add_shapes_listed_as_paths_with_bounds_and_colors() {
    let pdf = pdfium();
    let input = image_fixture("ffx_shapes_in.pdf");
    let out = tmp("ffx_shapes_out.pdf");
    let before = list(&pdf, &input).len();
    let mut dashed = shape(ShapeKind::Line, 50.0, 200.0, 250.0, 200.0);
    dashed.dash = vec![6.0, 3.0];
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[
            EditOp::AddShape(shape(ShapeKind::Rect, 50.0, 50.0, 150.0, 100.0)),
            EditOp::AddShape(shape(ShapeKind::RoundRect, 200.0, 50.0, 300.0, 100.0)),
            EditOp::AddShape(shape(ShapeKind::Ellipse, 350.0, 50.0, 450.0, 110.0)),
            EditOp::AddShape(dashed),
            EditOp::AddShape(shape(ShapeKind::Arrow, 300.0, 200.0, 500.0, 200.0)),
        ],
        &out,
        None,
    )
    .expect("add shapes");
    let after = list(&pdf, &out);
    assert_eq!(after.len(), before + 5, "thêm 5 hình = +5 object");
    let paths: Vec<&ObjectInfo> = after.iter().filter(|o| o.kind == ObjectKind::Path).collect();
    assert_eq!(paths.len(), 5, "{after:?}");
    let find = |cx: f32, cy: f32| {
        paths
            .iter()
            .find(|o| o.rect.left <= cx && o.rect.right >= cx && o.rect.bottom <= cy + 1.0 && o.rect.top >= cy - 1.0)
            .unwrap_or_else(|| panic!("không thấy hình tại ({cx},{cy}): {paths:?}"))
    };
    let rect = find(100.0, 75.0);
    assert!(near(rect.rect.left, 50.0, 2.0) && near(rect.rect.right, 150.0, 2.0), "{:?}", rect.rect);
    assert!(near(rect.rect.bottom, 50.0, 2.0) && near(rect.rect.top, 100.0, 2.0), "{:?}", rect.rect);
    assert_eq!(rect.stroke_color.map(|c| [c[0], c[1], c[2]]), Some([220, 0, 0]));
    assert_eq!(rect.fill_color.map(|c| [c[0], c[1], c[2]]), Some([0, 160, 0]));
    assert!(near(rect.stroke_width.unwrap_or(0.0), 2.0, 0.01));
    let ell = find(400.0, 80.0);
    assert!(near(ell.rect.top, 110.0, 2.0) && near(ell.rect.left, 350.0, 2.0), "{:?}", ell.rect);
    let line = find(150.0, 200.0);
    assert!(line.fill_color.is_none(), "đường thẳng không có nền");
    let arrow = find(450.0, 200.0);
    assert_eq!(arrow.fill_color.map(|c| [c[0], c[1], c[2]]), Some([220, 0, 0]), "đầu mũi tên tô màu viền");
    assert!(near(arrow.rect.right, 500.0, 1.0), "mũi nhọn chạm đúng điểm cuối: {:?}", arrow.rect);
    // Render: tâm hình chữ nhật là màu nền xanh lá.
    let im = render(&pdf, &out);
    let c = px_at(&im, 100.0, 75.0);
    assert!(c[1] > 120 && c[0] < 60, "tâm rect phải xanh lá: {c:?}");
}

#[test]
fn path_style_and_opacity_round_trip() {
    let pdf = pdfium();
    let input = image_fixture("ffx_pstyle_in.pdf");
    let mid = tmp("ffx_pstyle_mid.pdf");
    let out = tmp("ffx_pstyle_out.pdf");
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::AddShape(shape(ShapeKind::Rect, 50.0, 50.0, 150.0, 100.0))], &mid, None)
        .expect("add rect");
    let r = first_of(&list(&pdf, &mid), ObjectKind::Path);
    ff_engine::apply_edits(
        &pdf,
        &mid,
        0,
        &[
            EditOp::SetPathStyle {
                index: r.index,
                style: PathStyle { stroke: Some([0, 0, 255]), no_fill: true, width: Some(5.0), ..Default::default() },
            },
            EditOp::SetOpacity { index: r.index, opacity: 0.5 },
        ],
        &out,
        None,
    )
    .expect("style");
    let p = first_of(&list(&pdf, &out), ObjectKind::Path);
    assert_eq!(p.stroke_color.map(|c| [c[0], c[1], c[2]]), Some([0, 0, 255]));
    assert!(p.fill_color.is_none(), "đã bỏ nền: {:?}", p.fill_color);
    assert!(near(p.stroke_width.unwrap_or(0.0), 5.0, 0.01));
    assert!(near(p.opacity.unwrap_or(1.0), 0.5, 0.02), "độ mờ {:?}", p.opacity);
    let im = render(&pdf, &out);
    let center = px_at(&im, 100.0, 75.0);
    assert!(center.iter().all(|&c| c > 240), "không nền → tâm trắng: {center:?}");
    let edge = px_at(&im, 50.0, 75.0);
    assert!(edge[2] > 200 && edge[0] > 90 && edge[0] < 170, "viền xanh mờ 50%: {edge:?}");
}

// ---------------- Nhân bản / di chuyển nhiều ----------------

#[test]
fn duplicate_text_image_path_offsets_copies() {
    let pdf = pdfium();
    let input = image_fixture("ffx_dup_in.pdf");
    let mid = tmp("ffx_dup_mid.pdf");
    let out = tmp("ffx_dup_out.pdf");
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::AddShape(shape(ShapeKind::Ellipse, 50.0, 50.0, 150.0, 100.0))], &mid, None)
        .expect("add");
    let objs = list(&pdf, &mid);
    let (t, i, p) = (first_of(&objs, ObjectKind::Text), first_of(&objs, ObjectKind::Image), first_of(&objs, ObjectKind::Path));
    ff_engine::apply_edits(
        &pdf,
        &mid,
        0,
        &[
            EditOp::Duplicate { index: t.index, src_page: None, dx: 10.0, dy: -10.0 },
            EditOp::Duplicate { index: i.index, src_page: None, dx: 10.0, dy: -10.0 },
            EditOp::Duplicate { index: p.index, src_page: None, dx: 10.0, dy: -10.0 },
        ],
        &out,
        None,
    )
    .expect("duplicate");
    let after = list(&pdf, &out);
    assert_eq!(after.len(), objs.len() + 3);
    let texts: Vec<&ObjectInfo> = after.iter().filter(|o| o.kind == ObjectKind::Text).collect();
    assert_eq!(texts.len(), 2);
    assert!(texts.iter().all(|o| o.text.as_deref().map(str::trim) == Some("Hello object")), "{texts:?}");
    for kind in [ObjectKind::Text, ObjectKind::Image, ObjectKind::Path] {
        let v: Vec<&ObjectInfo> = after.iter().filter(|o| o.kind == kind).collect();
        assert_eq!(v.len(), 2, "{kind:?}");
        assert!(near(v[1].rect.left - v[0].rect.left, 10.0, 0.6), "{kind:?} dx: {:?} {:?}", v[0].rect, v[1].rect);
        assert!(near(v[1].rect.top - v[0].rect.top, -10.0, 0.6), "{kind:?} dy");
        assert!(near(v[1].rect.right - v[1].rect.left, v[0].rect.right - v[0].rect.left, 0.6), "{kind:?} cùng cỡ");
    }
}

#[test]
fn duplicate_across_pages_copies_text() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let out = tmp("ffx_dup_xpage.pdf");
    let src = ff_engine::list_objects(&pdf, &input, 0, None).expect("list p0");
    let t = src
        .iter()
        .find(|o| o.kind == ObjectKind::Text && o.text.as_deref().map_or(false, |s| !s.trim().is_empty()))
        .expect("text p0")
        .clone();
    let before = ff_engine::list_objects(&pdf, &input, 1, None).expect("list p1").len();
    ff_engine::apply_edits(&pdf, &input, 1, &[EditOp::Duplicate { index: t.index, src_page: Some(0), dx: 0.0, dy: 0.0 }], &out, None)
        .expect("dup cross page");
    let after = ff_engine::list_objects(&pdf, &out, 1, None).expect("list p1 out");
    assert_eq!(after.len(), before + 1);
    let copy = after.last().unwrap();
    assert_eq!(copy.text.as_deref().map(str::trim), t.text.as_deref().map(str::trim), "bản sao sang trang khác giữ nội dung");
    assert!(near(copy.rect.left, t.rect.left, 0.6) && near(copy.rect.top, t.rect.top, 0.6));
}

#[test]
fn multi_move_translates_all_selected() {
    let pdf = pdfium();
    let input = image_fixture("ffx_mmove_in.pdf");
    let out = tmp("ffx_mmove_out.pdf");
    let objs = list(&pdf, &input);
    let (t, i) = (first_of(&objs, ObjectKind::Text), first_of(&objs, ObjectKind::Image));
    ff_engine::apply_edits(
        &pdf,
        &input,
        0,
        &[
            EditOp::Transform { index: t.index, dx: 25.0, dy: 15.0, sx: 1.0, sy: 1.0 },
            EditOp::Transform { index: i.index, dx: 25.0, dy: 15.0, sx: 1.0, sy: 1.0 },
        ],
        &out,
        None,
    )
    .expect("multi move");
    let after = list(&pdf, &out);
    let (t2, i2) = (first_of(&after, ObjectKind::Text), first_of(&after, ObjectKind::Image));
    assert!(near(t2.rect.left - t.rect.left, 25.0, 0.5) && near(t2.rect.bottom - t.rect.bottom, 15.0, 0.5));
    assert!(near(i2.rect.left - i.rect.left, 25.0, 0.5) && near(i2.rect.bottom - i.rect.bottom, 15.0, 0.5));
}

// ---------------- Sắp lớp (z-order) ----------------

/// 3 hình vuông CHỒNG nhau ở cùng chỗ: đỏ, xanh lá, xanh dương (vẽ theo thứ tự).
const RGB_SQUARES: [&[u8]; 3] = [
    b"q 1 0 0 rg 100 100 100 100 re f Q\n",
    b"q 0 1 0 rg 120 120 100 100 re f Q\n",
    b"q 0 0 1 rg 140 140 100 100 re f Q\n",
];

fn fill_order(objs: &[ObjectInfo]) -> Vec<[u8; 3]> {
    objs.iter()
        .filter(|o| o.kind == ObjectKind::Path)
        .map(|o| {
            let c = o.fill_color.expect("fill");
            [c[0], c[1], c[2]]
        })
        .collect()
}

fn arrange_case(multi_stream: bool, name: &str) {
    let pdf = pdfium();
    let input = tmp(&format!("ffx_z_{name}_in.pdf"));
    if multi_stream {
        build_pdf(&input, &RGB_SQUARES);
    } else {
        let joined: Vec<u8> = RGB_SQUARES.concat();
        build_pdf(&input, &[&joined]);
    }
    let objs = list(&pdf, &input);
    assert_eq!(fill_order(&objs), vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
    let idx = |objs: &[ObjectInfo], c: [u8; 3]| {
        objs.iter().find(|o| o.fill_color.map(|f| [f[0], f[1], f[2]]) == Some(c)).unwrap().index
    };

    // Xanh dương xuống DƯỚI CÙNG → điểm chồng cả 3 (170,170) lộ màu xanh lá.
    let back = tmp(&format!("ffx_z_{name}_back.pdf"));
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::Arrange { indices: vec![idx(&objs, [0, 0, 255])], mode: ArrangeMode::Back }], &back, None)
        .expect("send to back");
    let o2 = list(&pdf, &back);
    assert_eq!(fill_order(&o2), vec![[0, 0, 255], [255, 0, 0], [0, 255, 0]], "{name}");
    let p = px_at(&render(&pdf, &back), 170.0, 170.0);
    assert!(p[1] > 200 && p[2] < 60, "{name}: điểm chồng phải xanh lá: {p:?}");

    // Đỏ lên TRÊN CÙNG.
    let front = tmp(&format!("ffx_z_{name}_front.pdf"));
    ff_engine::apply_edits(&pdf, &back, 0, &[EditOp::Arrange { indices: vec![idx(&o2, [255, 0, 0])], mode: ArrangeMode::Front }], &front, None)
        .expect("bring to front");
    let o3 = list(&pdf, &front);
    assert_eq!(fill_order(&o3), vec![[0, 0, 255], [0, 255, 0], [255, 0, 0]], "{name}");
    let p = px_at(&render(&pdf, &front), 170.0, 170.0);
    assert!(p[0] > 200 && p[1] < 60, "{name}: điểm chồng phải đỏ: {p:?}");

    // Xanh dương lên 1 lớp (vượt xanh lá đang chồng nó).
    let fwd = tmp(&format!("ffx_z_{name}_fwd.pdf"));
    ff_engine::apply_edits(&pdf, &front, 0, &[EditOp::Arrange { indices: vec![idx(&o3, [0, 0, 255])], mode: ArrangeMode::Forward }], &fwd, None)
        .expect("forward");
    assert_eq!(fill_order(&list(&pdf, &fwd)), vec![[0, 255, 0], [0, 0, 255], [255, 0, 0]], "{name}");

    // Đỏ xuống 1 lớp.
    let bwd = tmp(&format!("ffx_z_{name}_bwd.pdf"));
    let o4 = list(&pdf, &fwd);
    ff_engine::apply_edits(&pdf, &fwd, 0, &[EditOp::Arrange { indices: vec![idx(&o4, [255, 0, 0])], mode: ArrangeMode::Backward }], &bwd, None)
        .expect("backward");
    assert_eq!(fill_order(&list(&pdf, &bwd)), vec![[0, 255, 0], [255, 0, 0], [0, 0, 255]], "{name}");
}

#[test]
fn arrange_reorders_objects_single_stream() {
    arrange_case(false, "single");
}

#[test]
fn arrange_reorders_objects_across_content_streams() {
    arrange_case(true, "multi");
}

#[test]
fn arrange_new_shape_to_back_after_save() {
    // Luồng thật trong app: vẽ hình (PDFium thêm stream MỚI) → lưu → đưa xuống dưới.
    let pdf = pdfium();
    let input = image_fixture("ffx_z_new_in.pdf");
    let mid = tmp("ffx_z_new_mid.pdf");
    let out = tmp("ffx_z_new_out.pdf");
    let mut sq = shape(ShapeKind::Rect, 120.0, 520.0, 180.0, 580.0);
    sq.fill = Some([0, 255, 0]);
    sq.stroke = None;
    ff_engine::apply_edits(&pdf, &input, 0, &[EditOp::AddShape(sq)], &mid, None).expect("add");
    let p = px_at(&render(&pdf, &mid), 150.0, 550.0);
    assert!(p[1] > 200 && p[0] < 60, "hình mới nằm trên ảnh: {p:?}");
    let shape_idx = first_of(&list(&pdf, &mid), ObjectKind::Path).index;
    ff_engine::apply_edits(&pdf, &mid, 0, &[EditOp::Arrange { indices: vec![shape_idx], mode: ArrangeMode::Back }], &out, None)
        .expect("to back");
    assert_eq!(list(&pdf, &out)[0].kind, ObjectKind::Path, "hình phải thành object đầu tiên");
    let p = px_at(&render(&pdf, &out), 150.0, 550.0);
    assert!(p[0] > 200 && p[1] < 60, "sau khi đưa xuống dưới, ảnh đỏ che hình: {p:?}");
}

// ---------------- Căn lề đoạn văn (reflow giữ căn phải) ----------------

#[test]
fn reflow_preserves_right_alignment() {
    let pdf = pdfium();
    let input = tmp("ffx_ralign_in.pdf");
    // 2 dòng căn PHẢI tại x=500 (Helvetica 20: đo bằng list_objects rồi dựng lại).
    build_pdf(&input, &[b"BT /F1 20 Tf 300 500 Td (Right aligned long line) Tj ET BT /F1 20 Tf 300 475 Td (Short) Tj ET"]);
    let objs = list(&pdf, &input);
    let w: Vec<f32> = objs.iter().filter(|o| o.kind == ObjectKind::Text).map(|o| o.rect.right - o.rect.left).collect();
    let fx = tmp("ffx_ralign_fx.pdf");
    let c = format!(
        "BT /F1 20 Tf {} 500 Td (Right aligned long line) Tj ET BT /F1 20 Tf {} 475 Td (Short) Tj ET",
        500.0 - w[0],
        500.0 - w[1]
    );
    build_pdf(&fx, &[c.as_bytes()]);
    let idxs: Vec<u16> = list(&pdf, &fx).iter().filter(|o| o.kind == ObjectKind::Text).map(|o| o.index).collect();
    let out = tmp("ffx_ralign_out.pdf");
    ff_engine::apply_edits(
        &pdf,
        &fx,
        0,
        &[EditOp::ReflowText { indices: idxs, text: "Right aligned edited\nTiny".into(), rich: None }],
        &out,
        None,
    )
    .expect("reflow");
    let after: Vec<ObjectInfo> = list(&pdf, &out).into_iter().filter(|o| o.kind == ObjectKind::Text).collect();
    assert_eq!(after.len(), 2, "{after:?}");
    for o in &after {
        assert!(near(o.rect.right, 500.0, 4.0), "dòng mới phải thẳng mép phải 500: {:?} {:?}", o.text, o.rect);
    }
}
