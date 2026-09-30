//! Test Form AcroForm (Phase 6): tạo field → liệt kê → điền → export/import
//! FDF & CSV → flatten. Cần binary `qpdf` để chuẩn hoá fixture cho lopdf.

use std::path::PathBuf;

use ff_engine::{FieldKind, FieldValue, NewField};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

// KHÔNG đè biến môi trường nếu đã có (CI set sẵn); mặc định thử qpdf/ ở root
// workspace (fetch-qpdf.ps1 đặt ở đó), fallback /usr/bin cho máy Linux.
fn ensure_qpdf_path() {
    if std::env::var("FOFREEXIT_QPDF_PATH").is_err() {
        let ws = workspace_root().join("qpdf").join("bin");
        if ws.exists() {
            std::env::set_var("FOFREEXIT_QPDF_PATH", ws);
        } else {
            std::env::set_var("FOFREEXIT_QPDF_PATH", "/usr/bin");
        }
    }
}

fn pdfium() -> pdfium_render::prelude::Pdfium {
    if std::env::var("FOFREEXIT_PDFIUM_PATH").is_err() {
        std::env::set_var("FOFREEXIT_PDFIUM_PATH", workspace_root());
    }
    ensure_qpdf_path();
    ff_engine::bind_pdfium().expect("nạp PDFium")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    p
}

/// Dựng fixture form: chuẩn hoá sample rồi thêm 1 text + 1 checkbox + 1 combo.
fn make_form_fixture(out: &std::path::Path) {
    ensure_qpdf_path();
    let norm = tmp("ff_form_norm.pdf");
    ff_engine::repair(&workspace_root().join("corpus").join("sample-multipage.pdf"), &norm)
        .expect("normalize");
    let fields = vec![
        NewField {
            name: "hoTen".into(),
            kind: FieldKind::Text,
            page_index: 0,
            rect: [80.0, 700.0, 320.0, 720.0],
            value: String::new(),
            options: vec![],
        },
        NewField {
            name: "dongY".into(),
            kind: FieldKind::Checkbox,
            page_index: 0,
            rect: [80.0, 660.0, 96.0, 676.0],
            value: "off".into(),
            options: vec![],
        },
        NewField {
            name: "gioiTinh".into(),
            kind: FieldKind::Combo,
            page_index: 0,
            rect: [80.0, 620.0, 240.0, 640.0],
            value: String::new(),
            options: vec!["Nam".into(), "Nữ".into(), "Khác".into()],
        },
    ];
    ff_engine::create_form_fields(&norm, &fields, out).expect("create fields");
}

#[test]
fn create_and_list_fields() {
    let _ = pdfium();
    let fx = tmp("ff_form_fx.pdf");
    make_form_fixture(&fx);

    let fields = ff_engine::list_form_fields(&fx).expect("list");
    assert_eq!(fields.len(), 3, "phải có 3 field: {:?}", fields.iter().map(|f| &f.name).collect::<Vec<_>>());
    let by_name = |n: &str| fields.iter().find(|f| f.name == n).unwrap_or_else(|| panic!("thiếu {n}"));
    assert_eq!(by_name("hoTen").kind, FieldKind::Text);
    assert_eq!(by_name("dongY").kind, FieldKind::Checkbox);
    let combo = by_name("gioiTinh");
    assert_eq!(combo.kind, FieldKind::Combo);
    assert_eq!(combo.options, vec!["Nam".to_string(), "Nữ".into(), "Khác".into()]);
    // Field nằm đúng trang 0 và có rect.
    assert_eq!(by_name("hoTen").page_index, Some(0));
    assert!(by_name("hoTen").rect.is_some());
}

#[test]
fn fill_text_checkbox_combo_round_trips() {
    let _ = pdfium();
    let fx = tmp("ff_form_fill_fx.pdf");
    let out = tmp("ff_form_filled.pdf");
    make_form_fixture(&fx);

    let values = vec![
        FieldValue { name: "hoTen".into(), value: "Nguyễn Văn A".into() },
        FieldValue { name: "dongY".into(), value: "on".into() },
        FieldValue { name: "gioiTinh".into(), value: "Nữ".into() },
    ];
    let n = ff_engine::fill_form_fields(&fx, &values, &out).expect("fill");
    assert_eq!(n, 3, "phải điền 3 field");

    let fields = ff_engine::list_form_fields(&out).expect("list out");
    let get = |n: &str| fields.iter().find(|f| f.name == n).unwrap();
    assert_eq!(get("hoTen").value.as_deref(), Some("Nguyễn Văn A"), "text (tiếng Việt) round-trip");
    assert_eq!(get("gioiTinh").value.as_deref(), Some("Nữ"));
    // Checkbox bật → /V là tên on-state (khác "Off").
    let cb = get("dongY").value.clone().unwrap_or_default();
    assert!(cb != "Off" && !cb.is_empty(), "checkbox bật phải có on-state, được {cb:?}");
}

#[test]
fn fdf_export_import_round_trip() {
    let _ = pdfium();
    let fx = tmp("ff_form_fdf_fx.pdf");
    let filled = tmp("ff_form_fdf_filled.pdf");
    let fdf = tmp("ff_form_out.fdf");
    let reimported = tmp("ff_form_reimport.pdf");
    make_form_fixture(&fx);

    let values = vec![
        FieldValue { name: "hoTen".into(), value: "Trần Thị B".into() },
        FieldValue { name: "gioiTinh".into(), value: "Khác".into() },
    ];
    ff_engine::fill_form_fields(&fx, &values, &filled).expect("fill");
    ff_engine::export_fdf(&filled, &fdf).expect("export fdf");

    // FDF chứa tên + giá trị.
    let parsed = ff_engine::parse_fdf(&fdf).expect("parse fdf");
    let hoten = parsed.iter().find(|v| v.name == "hoTen").expect("hoTen trong fdf");
    assert_eq!(hoten.value, "Trần Thị B");

    // Import FDF vào fixture rỗng → giá trị được điền lại.
    let n = ff_engine::import_fdf(&fx, &fdf, &reimported).expect("import fdf");
    assert!(n >= 2, "import phải điền ≥2 field");
    let fields = ff_engine::list_form_fields(&reimported).expect("list reimport");
    assert_eq!(
        fields.iter().find(|f| f.name == "hoTen").unwrap().value.as_deref(),
        Some("Trần Thị B")
    );
}

#[test]
fn export_csv_has_rows() {
    let _ = pdfium();
    let fx = tmp("ff_form_csv_fx.pdf");
    let filled = tmp("ff_form_csv_filled.pdf");
    let csv = tmp("ff_form_out.csv");
    make_form_fixture(&fx);
    ff_engine::fill_form_fields(
        &fx,
        &[FieldValue { name: "hoTen".into(), value: "CSV Test".into() }],
        &filled,
    )
    .expect("fill");
    ff_engine::export_csv(&filled, &csv).expect("export csv");

    let text = std::fs::read_to_string(&csv).expect("đọc csv");
    assert!(text.starts_with("name,value"), "CSV phải có header");
    assert!(text.contains("hoTen,CSV Test"), "CSV phải chứa giá trị: {text}");
}

#[test]
fn flatten_removes_interactive_fields() {
    let pdf = pdfium();
    let fx = tmp("ff_form_flat_fx.pdf");
    let filled = tmp("ff_form_flat_filled.pdf");
    let flat = tmp("ff_form_flat.pdf");
    make_form_fixture(&fx);
    ff_engine::fill_form_fields(
        &fx,
        &[FieldValue { name: "hoTen".into(), value: "Flatten Me".into() }],
        &filled,
    )
    .expect("fill");

    ff_engine::flatten_form(&pdf, &filled, &flat, None).expect("flatten");
    // Sau flatten không còn field tương tác.
    let fields = ff_engine::list_form_fields(&flat).expect("list flat");
    assert!(fields.is_empty(), "flatten phải bỏ hết field tương tác, còn: {:?}", fields.iter().map(|f| &f.name).collect::<Vec<_>>());
}

/// Điền → flatten → giá trị đã điền phải còn là CONTENT của trang (trước đây
/// fill xoá /AP còn Flatten của PDFium chỉ in /AP có sẵn → giá trị biến mất).
#[test]
fn flatten_after_fill_keeps_text_and_combo_values() {
    let pdf = pdfium();
    let fx = tmp("ff_form_keep_fx.pdf");
    let filled = tmp("ff_form_keep_filled.pdf");
    let flat = tmp("ff_form_keep_flat.pdf");
    make_form_fixture(&fx);
    let values = vec![
        FieldValue { name: "hoTen".into(), value: "Nguyen Van A".into() },
        FieldValue { name: "dongY".into(), value: "on".into() },
        FieldValue { name: "gioiTinh".into(), value: "Nam".into() },
    ];
    ff_engine::fill_form_fields(&fx, &values, &filled).expect("fill");
    ff_engine::flatten_form(&pdf, &filled, &flat, None).expect("flatten");

    let text = ff_engine::extract_text(&pdf, &flat, 0, None).expect("text");
    assert!(text.contains("Nguyen Van A"), "giá trị text phải còn sau flatten: {text:?}");
    assert!(text.contains("Nam"), "giá trị combo phải còn sau flatten: {text:?}");
    assert!(ff_engine::list_form_fields(&flat).expect("list").is_empty(), "flatten phải bỏ field tương tác");
}

#[test]
fn flatten_after_fill_keeps_vietnamese_values() {
    let pdf = pdfium();
    let fx = tmp("ff_form_keep_vi_fx.pdf");
    let filled = tmp("ff_form_keep_vi_filled.pdf");
    let flat = tmp("ff_form_keep_vi_flat.pdf");
    make_form_fixture(&fx);
    let values = vec![
        FieldValue { name: "hoTen".into(), value: "Nguyễn Văn A".into() },
        FieldValue { name: "gioiTinh".into(), value: "Nữ".into() },
    ];
    ff_engine::fill_form_fields(&fx, &values, &filled).expect("fill");
    // Giá trị /V vẫn round-trip đúng (appearance không làm hỏng dữ liệu field).
    let fields = ff_engine::list_form_fields(&filled).expect("list filled");
    assert_eq!(
        fields.iter().find(|f| f.name == "hoTen").and_then(|f| f.value.clone()).as_deref(),
        Some("Nguyễn Văn A")
    );
    ff_engine::flatten_form(&pdf, &filled, &flat, None).expect("flatten");

    let text = ff_engine::extract_text(&pdf, &flat, 0, None).expect("text");
    assert!(text.contains("Nguyễn Văn A"), "giá trị tiếng Việt phải còn sau flatten: {text:?}");
    assert!(text.contains("Nữ"), "combo tiếng Việt phải còn sau flatten: {text:?}");
}

/// Số pixel tối trong ô checkbox của fixture (rect [80,660,96,676] điểm PDF,
/// render đúng 612px = 1px/pt trên trang Letter).
fn dark_pixels_in_checkbox(pdf: &pdfium_render::prelude::Pdfium, path: &std::path::Path) -> usize {
    let img = ff_engine::render::render_page(pdf, path, 0, 612, None).expect("render").image.to_luma8();
    let mut n = 0;
    for y in (792 - 676)..(792 - 660) {
        for x in 80..96 {
            if x < img.width() && y < img.height() && img.get_pixel(x, y)[0] < 128 {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn flatten_after_fill_keeps_checkbox_mark() {
    let pdf = pdfium();
    let fx = tmp("ff_form_cb_fx.pdf");
    let on = tmp("ff_form_cb_on.pdf");
    let off = tmp("ff_form_cb_off.pdf");
    let flat_on = tmp("ff_form_cb_flat_on.pdf");
    let flat_off = tmp("ff_form_cb_flat_off.pdf");
    make_form_fixture(&fx);
    ff_engine::fill_form_fields(&fx, &[FieldValue { name: "dongY".into(), value: "on".into() }], &on)
        .expect("fill on");
    ff_engine::fill_form_fields(&fx, &[FieldValue { name: "dongY".into(), value: "off".into() }], &off)
        .expect("fill off");
    ff_engine::flatten_form(&pdf, &on, &flat_on, None).expect("flatten on");
    ff_engine::flatten_form(&pdf, &off, &flat_off, None).expect("flatten off");

    let dark_on = dark_pixels_in_checkbox(&pdf, &flat_on);
    let dark_off = dark_pixels_in_checkbox(&pdf, &flat_off);
    assert!(
        dark_on > dark_off + 10,
        "checkbox đã tick phải còn dấu tick sau flatten (tối: on={dark_on}, off={dark_off})"
    );
}
