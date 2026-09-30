//! Form mức Foxit (formx): tạo MỌI loại field → mở lại bằng PDFium (form API)
//! và lopdf; điền tiếng Việt round-trip + appearance thật; radio loại trừ lẫn
//! nhau; list đa chọn; field ngày có JS; sửa/xoá field; reset; XFDF round-trip;
//! nhận diện field tự động trên PDF có gạch dưới / ô vuông / khung.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ff_engine::{ButtonAction, FieldEdit, FieldKind, FieldSpec, FillValue};
use lopdf::{dictionary, Document, Object, Stream};
use pdfium_render::prelude::*;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
}

fn pdfium() -> Pdfium {
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

/// PDF 1 trang A4 với nội dung vẽ tuỳ ý (Helvetica /F1).
fn build_pdf(path: &Path, content: &[u8]) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("lưu fixture");
}

fn blank(name: &str) -> PathBuf {
    let p = tmp(name);
    build_pdf(&p, b"BT /F1 14 Tf 50 800 Td (Form test) Tj ET");
    p
}

fn spec(name: &str, kind: FieldKind, rect: [f32; 4]) -> FieldSpec {
    FieldSpec { name: name.into(), kind, page_index: 0, rect, ..Default::default() }
}

/// Mọi loại field Foxit tạo được.
fn all_specs() -> Vec<FieldSpec> {
    let mut v = vec![];
    let mut t = spec("hoTen", FieldKind::Text, [50.0, 740.0, 300.0, 762.0]);
    t.tooltip = "Họ và tên".into();
    t.required = true;
    t.max_len = 40;
    v.push(t);
    let mut ml = spec("ghiChu", FieldKind::Text, [50.0, 650.0, 300.0, 720.0]);
    ml.multiline = true;
    ml.font_size = 10.0;
    v.push(ml);
    let mut cb = spec("dongY", FieldKind::Checkbox, [320.0, 740.0, 334.0, 754.0]);
    cb.export_value = "CoDongY".into();
    cb.check_style = "cross".into();
    v.push(cb);
    for (i, ex) in ["Nam", "Nu", "Khac"].iter().enumerate() {
        let x = 50.0 + i as f32 * 60.0;
        let mut r = spec("gioiTinh", FieldKind::Radio, [x, 620.0, x + 14.0, 634.0]);
        r.export_value = ex.to_string();
        v.push(r);
    }
    let mut co = spec("tinh", FieldKind::Combo, [50.0, 580.0, 250.0, 600.0]);
    co.options = vec!["Hà Nội".into(), "Huế".into(), "TP. Hồ Chí Minh".into()];
    co.option_exports = vec!["HN".into(), "HUE".into(), "HCM".into()];
    co.editable = true;
    v.push(co);
    let mut li = spec("soThich", FieldKind::List, [50.0, 480.0, 250.0, 560.0]);
    li.options = vec!["Đọc sách".into(), "Bóng đá".into(), "Âm nhạc".into(), "Du lịch".into()];
    li.multi_select = true;
    v.push(li);
    let mut bt = spec("btnReset", FieldKind::Button, [300.0, 480.0, 400.0, 504.0]);
    bt.caption = "Làm lại".into();
    bt.fill_color = Some([0.8, 0.8, 0.8]);
    bt.border_style = "beveled".into();
    bt.action = ButtonAction { kind: "reset".into(), target: String::new() };
    v.push(bt);
    let mut url = spec("btnWeb", FieldKind::Button, [410.0, 480.0, 510.0, 504.0]);
    url.caption = "Website".into();
    url.action = ButtonAction { kind: "url".into(), target: "https://example.com".into() };
    v.push(url);
    let mut d = spec("ngaySinh", FieldKind::Text, [300.0, 580.0, 420.0, 600.0]);
    d.date_format = "dd/mm/yyyy".into();
    v.push(d);
    v.push(spec("chuKy", FieldKind::Signature, [300.0, 380.0, 500.0, 440.0]));
    v
}

fn make_all(name: &str) -> PathBuf {
    let src = blank(&format!("{name}_src.pdf"));
    let out = tmp(&format!("{name}.pdf"));
    let n = ff_engine::create_fields(&src, &all_specs(), &out).expect("create fields");
    assert_eq!(n, all_specs().len());
    out
}

/// Loại field theo tên, đọc bằng form API của PDFium.
fn pdfium_fields(pdf: &Pdfium, path: &Path) -> Vec<(String, PdfFormFieldType)> {
    let doc = pdf.load_pdf_from_file(path, None).expect("pdfium mở");
    assert!(doc.form().is_some(), "PDFium phải thấy AcroForm");
    let mut out = vec![];
    for page in doc.pages().iter() {
        for a in page.annotations().iter() {
            if let Some(f) = a.as_form_field() {
                out.push((f.name().unwrap_or_default(), f.field_type()));
            }
        }
    }
    out
}

fn widget_dict<'a>(doc: &'a Document, id: &str) -> &'a lopdf::Dictionary {
    let mut it = id.split_whitespace();
    let oid = (it.next().unwrap().parse::<u32>().unwrap(), it.next().unwrap().parse::<u16>().unwrap());
    doc.get_object(oid).unwrap().as_dict().unwrap()
}

#[test]
fn create_every_field_type_reopens_in_pdfium_and_lopdf() {
    let pdf = pdfium();
    let out = make_all("fx_all");

    let fields = pdfium_fields(&pdf, &out);
    let ty = |n: &str| fields.iter().find(|f| f.0 == n).map(|f| f.1).unwrap_or_else(|| panic!("PDFium thiếu field {n}: {fields:?}"));
    assert_eq!(ty("hoTen"), PdfFormFieldType::Text);
    assert_eq!(ty("ghiChu"), PdfFormFieldType::Text);
    assert_eq!(ty("dongY"), PdfFormFieldType::Checkbox);
    assert_eq!(ty("gioiTinh"), PdfFormFieldType::RadioButton);
    assert_eq!(ty("tinh"), PdfFormFieldType::ComboBox);
    assert_eq!(ty("soThich"), PdfFormFieldType::ListBox);
    assert_eq!(ty("btnReset"), PdfFormFieldType::PushButton);
    assert_eq!(ty("ngaySinh"), PdfFormFieldType::Text);
    assert_eq!(ty("chuKy"), PdfFormFieldType::Signature);
    assert_eq!(fields.iter().filter(|f| f.0 == "gioiTinh").count(), 3, "nhóm radio 3 nút");

    // lopdf: mọi widget có /AP /N thật; NeedAppearances không bật.
    let doc = Document::load(&out).unwrap();
    let ws = ff_engine::list_widgets(&out).unwrap();
    assert_eq!(ws.len(), 12, "{:?}", ws.iter().map(|w| &w.name).collect::<Vec<_>>());
    for w in &ws {
        let d = widget_dict(&doc, &w.id);
        let ap = d.get(b"AP").and_then(Object::as_dict).unwrap_or_else(|_| panic!("{} thiếu /AP", w.name));
        assert!(ap.has(b"N"), "{} thiếu /AP /N", w.name);
    }
    let cat = doc.catalog().unwrap();
    let acro = doc.get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap()).unwrap().as_dict().unwrap();
    assert!(acro.get(b"NeedAppearances").and_then(Object::as_bool).map(|b| !b).unwrap_or(true));

    let by = |n: &str| ws.iter().find(|w| w.name == n).unwrap();
    let t = by("hoTen");
    assert_eq!(t.tooltip, "Họ và tên");
    assert!(t.required);
    assert_eq!(t.max_len, 40);
    assert!(by("ghiChu").multiline);
    assert_eq!(by("ghiChu").font_size, 10.0);
    assert_eq!(by("dongY").export_value, "CoDongY");
    assert_eq!(by("dongY").check_style, "cross");
    let radios: Vec<&str> = ws.iter().filter(|w| w.name == "gioiTinh").map(|w| w.export_value.as_str()).collect();
    assert_eq!(radios, vec!["Nam", "Nu", "Khac"]);
    assert_eq!(by("tinh").options, vec!["Hà Nội", "Huế", "TP. Hồ Chí Minh"]);
    assert_eq!(by("tinh").option_exports, vec!["HN", "HUE", "HCM"]);
    assert!(by("tinh").editable);
    assert!(by("soThich").multi_select);
    assert_eq!(by("btnReset").caption, "Làm lại");
    assert_eq!(by("btnReset").action.kind, "reset");
    assert_eq!(by("btnWeb").action, ButtonAction { kind: "url".into(), target: "https://example.com".into() });
    assert_eq!(by("chuKy").kind, FieldKind::Signature);

    // Field ngày có JS định dạng /AA /F + /K.
    let dw = widget_dict(&doc, &by("ngaySinh").id);
    let aa = dw.get(b"AA").and_then(Object::as_dict).expect("date phải có /AA");
    for k in [&b"F"[..], &b"K"[..]] {
        let js = aa.get(k).unwrap().as_dict().unwrap().get(b"JS").unwrap().as_str().unwrap();
        let js = String::from_utf8_lossy(js);
        assert!(js.contains("AFDate_") && js.contains("dd/mm/yyyy"), "JS {js}");
    }
    assert_eq!(by("ngaySinh").date_format, "dd/mm/yyyy");
}

fn fill_all(input: &Path, output: &Path) -> usize {
    let vals = vec![
        FillValue { name: "hoTen".into(), value: "Nguyễn Văn Ánh".into(), values: vec![] },
        FillValue { name: "ghiChu".into(), value: "Dòng một khá dài để phải xuống dòng trong ô nhiều dòng\nDòng hai".into(), values: vec![] },
        FillValue { name: "dongY".into(), value: "on".into(), values: vec![] },
        FillValue { name: "gioiTinh".into(), value: "Nu".into(), values: vec![] },
        FillValue { name: "tinh".into(), value: "Huế".into(), values: vec![] },
        FillValue { name: "soThich".into(), value: String::new(), values: vec!["Đọc sách".into(), "Âm nhạc".into()] },
        FillValue { name: "ngaySinh".into(), value: "02/09/1945".into(), values: vec![] },
    ];
    ff_engine::fill_form(input, &vals, output).expect("fill")
}

#[test]
fn fill_vietnamese_round_trips_with_real_appearance() {
    let pdf = pdfium();
    let fx = make_all("fx_fill_src");
    let out = tmp("fx_fill_out.pdf");
    assert_eq!(fill_all(&fx, &out), 7);

    // lopdf / engine.
    let ws = ff_engine::list_widgets(&out).unwrap();
    let by = |n: &str| ws.iter().find(|w| w.name == n).unwrap();
    assert_eq!(by("hoTen").value, "Nguyễn Văn Ánh");
    assert_eq!(by("tinh").value, "HUE", "combo lưu export value");
    assert_eq!(by("soThich").values, vec!["Đọc sách".to_string(), "Âm nhạc".into()]);
    assert!(by("dongY").checked);
    assert_eq!(by("dongY").value, "CoDongY");

    // PDFium đọc lại đúng giá trị tiếng Việt.
    let doc = pdf.load_pdf_from_file(&out, None).unwrap();
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut radio_on = vec![];
    for page in doc.pages().iter() {
        for a in page.annotations().iter() {
            let Some(f) = a.as_form_field() else { continue };
            let name = f.name().unwrap_or_default();
            if let Some(t) = f.as_text_field() {
                texts.insert(name.clone(), t.value().unwrap_or_default());
            }
            if let Some(r) = f.as_radio_button_field() {
                if r.is_checked().unwrap_or(false) {
                    radio_on.push(r.group_value().unwrap_or_default());
                }
            }
            if let Some(l) = f.as_list_box_field() {
                assert!(l.is_multiselect(), "list phải đa chọn");
            }
        }
    }
    assert_eq!(texts.get("hoTen").map(|s| s.as_str()), Some("Nguyễn Văn Ánh"));
    assert_eq!(radio_on.len(), 1, "radio: đúng 1 nút bật, được {radio_on:?}");

    // Appearance tiếng Việt dùng font Unicode NHÚNG (FontFile2).
    let ld = Document::load(&out).unwrap();
    let w = widget_dict(&ld, &by("hoTen").id);
    let n = w.get(b"AP").unwrap().as_dict().unwrap().get(b"N").unwrap().as_reference().unwrap();
    let xo = ld.get_object(n).unwrap().as_stream().unwrap();
    let fonts = xo.dict.get(b"Resources").unwrap().as_dict().unwrap().get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"FXUni"), "AP tiếng Việt phải dùng /FXUni");
    let content = String::from_utf8_lossy(&xo.decompressed_content().unwrap()).into_owned();
    assert!(content.contains("/FXUni") && content.contains("Tj"), "{content}");

    // Render: vùng ô hoTen có mực (appearance hiển thị được, không cần NeedAppearances).
    let page = doc.pages().get(0).unwrap();
    let bmp = page.render_with_config(&PdfRenderConfig::new().set_target_width(595)).unwrap();
    let img = bmp.as_image().to_rgba8();
    let (x0, x1) = (55u32, 200u32);
    let (y0, y1) = (842 - 760, 842 - 742);
    let dark = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| img.get_pixel(*x, *y).0[0] < 120)
        .count();
    assert!(dark > 40, "ô hoTen phải hiện chữ khi render, pixel tối = {dark}");
}

#[test]
fn radio_group_is_mutually_exclusive() {
    let fx = make_all("fx_radio_src");
    let a = tmp("fx_radio_a.pdf");
    let b = tmp("fx_radio_b.pdf");
    ff_engine::fill_form(&fx, &[FillValue { name: "gioiTinh".into(), value: "Nam".into(), values: vec![] }], &a).unwrap();
    ff_engine::fill_form(&a, &[FillValue { name: "gioiTinh".into(), value: "Khac".into(), values: vec![] }], &b).unwrap();
    let ws = ff_engine::list_widgets(&b).unwrap();
    let on: Vec<&str> = ws.iter().filter(|w| w.name == "gioiTinh" && w.checked).map(|w| w.export_value.as_str()).collect();
    assert_eq!(on, vec!["Khac"], "chỉ nút vừa chọn được bật");
    let doc = Document::load(&b).unwrap();
    for w in ws.iter().filter(|w| w.name == "gioiTinh") {
        let d = widget_dict(&doc, &w.id);
        let as_ = d.get(b"AS").unwrap().as_name().unwrap();
        assert_eq!(as_ == w.export_value.as_bytes(), w.export_value == "Khac");
    }
    // PDFium cũng thấy đúng 1 nút bật.
    let pdf = pdfium();
    let pd = pdf.load_pdf_from_file(&b, None).unwrap();
    let mut n_on = 0;
    for page in pd.pages().iter() {
        for a in page.annotations().iter() {
            if let Some(r) = a.as_form_field().and_then(|f| f.as_radio_button_field()) {
                if r.is_checked().unwrap_or(false) {
                    n_on += 1;
                }
            }
        }
    }
    assert_eq!(n_on, 1);
}

#[test]
fn list_box_multi_select_stores_array_and_indices() {
    let fx = make_all("fx_list_src");
    let out = tmp("fx_list_out.pdf");
    ff_engine::fill_form(
        &fx,
        &[FillValue { name: "soThich".into(), value: String::new(), values: vec!["Bóng đá".into(), "Du lịch".into()] }],
        &out,
    )
    .unwrap();
    let ws = ff_engine::list_widgets(&out).unwrap();
    let l = ws.iter().find(|w| w.name == "soThich").unwrap();
    assert_eq!(l.values, vec!["Bóng đá".to_string(), "Du lịch".into()]);
    let doc = Document::load(&out).unwrap();
    let d = widget_dict(&doc, &l.id);
    let v = d.get(b"V").unwrap().as_array().expect("/V phải là mảng");
    assert_eq!(v.len(), 2);
    let idx: Vec<i64> = d.get(b"I").unwrap().as_array().unwrap().iter().map(|o| o.as_i64().unwrap()).collect();
    assert_eq!(idx, vec![1, 3]);
}

#[test]
fn edit_properties_move_and_delete_fields() {
    let fx = make_all("fx_edit_src");
    let out = tmp("fx_edit_out.pdf");
    let ws = ff_engine::list_widgets(&fx).unwrap();
    let t = ws.iter().find(|w| w.name == "hoTen").unwrap();
    let r_mid = ws.iter().find(|w| w.name == "gioiTinh" && w.export_value == "Nu").unwrap();
    let mut props = spec("hoVaTen", FieldKind::Text, t.rect);
    props.tooltip = "Tên đầy đủ".into();
    props.read_only = true;
    props.align = 1;
    props.value = "Mặc định".into();
    let edits = vec![
        FieldEdit { widget_id: t.id.clone(), rect: Some([60.0, 700.0, 320.0, 724.0]), props: Some(props), delete: false },
        FieldEdit { widget_id: r_mid.id.clone(), delete: true, ..Default::default() },
    ];
    // Thêm 1 nút radio mới vào nhóm cùng lúc.
    let mut extra = spec("gioiTinh", FieldKind::Radio, [250.0, 620.0, 264.0, 634.0]);
    extra.export_value = "KhongNoi".into();
    ff_engine::apply_form_changes(&fx, &[extra], &edits, &[], &out).unwrap();

    let ws2 = ff_engine::list_widgets(&out).unwrap();
    let t2 = ws2.iter().find(|w| w.name == "hoVaTen").expect("đổi tên");
    assert_eq!(t2.tooltip, "Tên đầy đủ");
    assert!(t2.read_only && t2.align == 1);
    assert_eq!(t2.default_value, "Mặc định");
    assert_eq!(t2.value, "Mặc định");
    assert!((t2.rect[0] - 60.0).abs() < 0.01 && (t2.rect[3] - 724.0).abs() < 0.01);
    assert!(!ws2.iter().any(|w| w.name == "hoTen"));
    let radios: Vec<&str> = ws2.iter().filter(|w| w.name == "gioiTinh").map(|w| w.export_value.as_str()).collect();
    assert_eq!(radios, vec!["Nam", "Khac", "KhongNoi"]);
    // Widget đã xoá không còn trong /Annots.
    let doc = Document::load(&out).unwrap();
    let page = doc.get_object(*doc.get_pages().get(&1).unwrap()).unwrap().as_dict().unwrap();
    let annots = page.get(b"Annots").unwrap().as_array().unwrap();
    assert!(!annots.iter().any(|a| ff_engine::formx::id_str(a.as_reference().unwrap()) == r_mid.id));
}

#[test]
fn reset_and_clear_form() {
    let src = blank("fx_reset_blank.pdf");
    let fx = tmp("fx_reset_src.pdf");
    let mut t = spec("a", FieldKind::Text, [50.0, 700.0, 250.0, 720.0]);
    t.value = "Mặc định".into();
    let mut c = spec("c", FieldKind::Checkbox, [50.0, 660.0, 64.0, 674.0]);
    c.checked = true;
    ff_engine::create_fields(&src, &[t, c], &fx).unwrap();
    let filled = tmp("fx_reset_filled.pdf");
    ff_engine::fill_form(
        &fx,
        &[
            FillValue { name: "a".into(), value: "Đã sửa".into(), values: vec![] },
            FillValue { name: "c".into(), value: "off".into(), values: vec![] },
        ],
        &filled,
    )
    .unwrap();
    let reset = tmp("fx_reset_out.pdf");
    assert_eq!(ff_engine::reset_form(&filled, &reset, &[], false).unwrap(), 2);
    let ws = ff_engine::list_widgets(&reset).unwrap();
    assert_eq!(ws.iter().find(|w| w.name == "a").unwrap().value, "Mặc định");
    assert!(ws.iter().find(|w| w.name == "c").unwrap().checked);
    let cleared = tmp("fx_reset_clear.pdf");
    ff_engine::reset_form(&filled, &cleared, &[], true).unwrap();
    let ws = ff_engine::list_widgets(&cleared).unwrap();
    assert_eq!(ws.iter().find(|w| w.name == "a").unwrap().value, "");
    assert!(!ws.iter().find(|w| w.name == "c").unwrap().checked);
}

#[test]
fn xfdf_and_other_formats_round_trip() {
    let fx = make_all("fx_xfdf_src");
    let filled = tmp("fx_xfdf_filled.pdf");
    fill_all(&fx, &filled);
    for ext in ["xfdf", "xml", "fdf", "csv", "txt"] {
        let data = tmp(&format!("fx_data.{ext}"));
        ff_engine::export_form_data(&filled, &data).unwrap();
        let back = tmp(&format!("fx_back_{ext}.pdf"));
        let n = ff_engine::import_form_data(&fx, &data, &back).unwrap();
        assert!(n >= 5, "{ext}: điền {n} field");
        let ws = ff_engine::list_widgets(&back).unwrap();
        let by = |n: &str| ws.iter().find(|w| w.name == n).unwrap();
        assert_eq!(by("hoTen").value, "Nguyễn Văn Ánh", "{ext}");
        assert_eq!(by("tinh").value, "HUE", "{ext}");
        let on: Vec<&str> = ws.iter().filter(|w| w.name == "gioiTinh" && w.checked).map(|w| w.export_value.as_str()).collect();
        assert_eq!(on, vec!["Nu"], "{ext}");
        if ext == "xfdf" || ext == "xml" {
            assert_eq!(by("soThich").values, vec!["Đọc sách".to_string(), "Âm nhạc".into()], "{ext} đa chọn");
            assert!(by("ghiChu").value.contains("Dòng hai"), "{ext} nhiều dòng");
        }
    }
    // XFDF đúng cấu trúc chuẩn.
    let x = tmp("fx_check.xfdf");
    ff_engine::export_form_data(&filled, &x).unwrap();
    let s = std::fs::read_to_string(&x).unwrap();
    assert!(s.contains("<xfdf xmlns=\"http://ns.adobe.com/xfdf/\"") && s.contains("<field name=\"hoTen\">"));
    assert!(s.contains("<value>Nguyễn Văn Ánh</value>"));
}

#[test]
fn xfdf_nested_names_parse() {
    let p = tmp("fx_nested.xfdf");
    std::fs::write(
        &p,
        "<?xml version=\"1.0\"?><xfdf xmlns=\"http://ns.adobe.com/xfdf/\"><fields><field name=\"a\"><field name=\"b\"><value>X &amp; Y</value></field></field><field name=\"c\"><value>1</value><value>2</value></field></fields></xfdf>",
    )
    .unwrap();
    let v = ff_engine::parse_xfdf(&p).unwrap();
    assert_eq!(v[0].name, "a.b");
    assert_eq!(v[0].value, "X & Y");
    assert_eq!(v[1].name, "c");
    assert_eq!(v[1].values, vec!["1".to_string(), "2".into()]);
}

#[test]
fn legacy_create_and_fill_still_work() {
    let src = blank("fx_legacy_src.pdf");
    let out = tmp("fx_legacy.pdf");
    ff_engine::create_form_fields(
        &src,
        &[ff_engine::NewField {
            name: "x".into(),
            kind: FieldKind::Checkbox,
            page_index: 0,
            rect: [50.0, 700.0, 64.0, 714.0],
            value: "on".into(),
            options: vec![],
        }],
        &out,
    )
    .unwrap();
    let f = ff_engine::list_form_fields(&out).unwrap();
    assert_eq!(f[0].kind, FieldKind::Checkbox);
    assert_eq!(f[0].value.as_deref(), Some("Yes"));
}

#[test]
fn recognition_finds_underscores_boxes_and_lines() {
    let pdf = pdfium();
    let p = tmp("fx_recog.pdf");
    let content = b"BT /F1 12 Tf 50 760 Td (Full name: ______________________) Tj ET\n\
        BT /F1 12 Tf 50 720 Td (Address) Tj ET\n\
        0 0 0 RG 0.8 w 120 714 200 20 re S\n\
        0.5 w 50 680 10 10 re S\n\
        BT /F1 12 Tf 66 681 Td (I agree) Tj ET\n\
        BT /F1 12 Tf 50 640 Td (Signature) Tj ET\n\
        0.6 w 120 638 m 320 638 l S\n\
        BT /F1 12 Tf 50 600 Td (Plain paragraph text without any field) Tj ET\n";
    build_pdf(&p, content);
    let props = ff_engine::recognize_fields(&pdf, &p, &[]).unwrap();
    let dump = format!("{props:#?}");
    let find = |kind: &str, name: &str| props.iter().find(|x| x.kind == kind && x.name.contains(name));
    let under = find("text", "Full name").unwrap_or_else(|| panic!("thiếu ô gạch dưới: {dump}"));
    assert!(under.rect[0] > 100.0 && under.rect[1] >= 755.0 && under.rect[1] < 764.0, "{dump}");
    let bx = find("text", "Address").unwrap_or_else(|| panic!("thiếu ô khung: {dump}"));
    assert!((bx.rect[0] - 120.0).abs() < 2.0 && (bx.rect[2] - 320.0).abs() < 2.0, "{dump}");
    let cb = find("checkbox", "I agree").unwrap_or_else(|| panic!("thiếu checkbox: {dump}"));
    assert!((cb.rect[0] - 50.0).abs() < 2.0, "{dump}");
    let ln = find("text", "Signature").unwrap_or_else(|| panic!("thiếu ô đường kẻ: {dump}"));
    assert!(ln.rect[1] >= 637.0 && ln.rect[1] < 641.0, "{dump}");
    assert_eq!(props.len(), 4, "không đề xuất thừa: {dump}");

    // Chấp nhận đề xuất → tạo field thật.
    let specs: Vec<FieldSpec> = props
        .iter()
        .map(|x| FieldSpec {
            name: x.name.clone(),
            tooltip: x.tooltip.clone(),
            kind: if x.kind == "checkbox" { FieldKind::Checkbox } else { FieldKind::Text },
            page_index: x.page_index,
            rect: x.rect,
            ..Default::default()
        })
        .collect();
    let out = tmp("fx_recog_created.pdf");
    ff_engine::create_fields(&p, &specs, &out).unwrap();
    assert_eq!(ff_engine::list_widgets(&out).unwrap().len(), 4);
    // Chạy lại nhận diện trên file đã có field → không đề xuất trùng.
    let again = ff_engine::recognize_fields(&pdf, &out, &[]).unwrap();
    assert!(again.is_empty(), "{again:#?}");
}
