//! Round-trip test cho annotation (đường GHI file — phần rủi ro nhất Phase 2):
//! tạo annotation → lưu file mới → mở lại → còn đúng số lượng/loại → render ra
//! đúng màu. Dùng sample-multipage.pdf.

use std::path::PathBuf;

use ff_engine::{AnnotKind, AnnotSpec, Rect};

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

#[test]
fn highlight_and_square_roundtrip() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = std::env::temp_dir().join("ff_annot_roundtrip.pdf");
    let _ = std::fs::remove_file(&output);

    // Highlight vàng phủ từ "content" ở trang 0 (toạ độ đã biết từ test search),
    // và một khung vuông đỏ ở trang 1.
    let specs = vec![
        AnnotSpec::markup(
            AnnotKind::Highlight,
            0,
            Rect { left: 155.0, bottom: 690.0, right: 213.0, top: 706.0 },
            [255, 255, 0, 255],
        ),
        AnnotSpec::markup(
            AnnotKind::Square,
            1,
            Rect { left: 100.0, bottom: 600.0, right: 300.0, top: 700.0 },
            [255, 0, 0, 255],
        ),
    ];

    ff_engine::apply_annotations(&pdf, &input, &output, &specs).expect("apply_annotations");
    assert!(output.exists(), "file output chưa được tạo");

    // Mở lại: đúng số lượng.
    assert_eq!(
        ff_engine::count_annotations(&pdf, &output, 0).expect("count p0"),
        1,
        "trang 0 phải có 1 annotation"
    );
    assert_eq!(
        ff_engine::count_annotations(&pdf, &output, 1).expect("count p1"),
        1,
        "trang 1 phải có 1 annotation"
    );

    // Đúng loại + contents giữ lại.
    let list = ff_engine::list_annotations(&pdf, &output).expect("list");
    assert_eq!(list.len(), 2, "tổng phải 2 annotation: {list:?}");
    let kinds: Vec<&str> = list.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.iter().any(|k| k.contains("Highlight")), "thiếu Highlight: {kinds:?}");
    assert!(kinds.iter().any(|k| k.contains("Square")), "thiếu Square: {kinds:?}");

    // Render trang 0 (có annotation) -> phải có pixel vàng (highlight).
    let img = ff_engine::render::render_page(&pdf, &output, 0, 800, None).expect("render");
    let rgba = img.image.to_rgba8();
    let yellow = rgba
        .pixels()
        .filter(|p| p.0[0] > 200 && p.0[1] > 200 && p.0[2] < 120)
        .count();
    assert!(yellow > 50, "không thấy highlight vàng được render ({yellow} px)");
}

/// Highlight phải bám theo TỪNG DÒNG văn bản (như chọn text rồi tô sáng trong
/// Foxit), không phải 1 khối chữ nhật phủ luôn khoảng trắng giữa 2 dòng.
/// Dùng 2 quad cho 2 dòng "FoFreeXit Test Document" (y≈716-736) và "Page one
/// content alpha" (y≈690-706) — khoảng trống giữa 2 dòng (y≈707-715) phải
/// KHÔNG bị tô vàng.
#[test]
fn multiline_highlight_uses_per_line_quads() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = std::env::temp_dir().join("ff_annot_multiline_hl.pdf");
    let _ = std::fs::remove_file(&output);

    let line1 = Rect { left: 72.0, bottom: 716.0, right: 280.0, top: 736.0 };
    let line2 = Rect { left: 72.0, bottom: 690.0, right: 280.0, top: 706.0 };
    let bounds = Rect { left: 72.0, bottom: 690.0, right: 280.0, top: 736.0 };
    let spec = AnnotSpec {
        kind: AnnotKind::Highlight,
        page_index: 0,
        rect: bounds,
        quads: vec![line1, line2],
        color: [255, 255, 0, 255],
        contents: None,
        font_size: 14.0,
        bold: false,
        italic: false,
        underline: false,
    };
    ff_engine::apply_annotations(&pdf, &input, &output, &[spec]).expect("apply");

    let list = ff_engine::list_annotations(&pdf, &output).expect("list");
    assert_eq!(list.len(), 1, "phải đúng 1 annotation: {list:?}");
    assert_eq!(
        list[0].quad_count, 2,
        "highlight 2 dòng phải có 2 quad (attachment points): {:?}", list[0]
    );

    // Render & kiểm 2 dải có vàng, dải trống giữa 2 dòng thì không.
    let img = ff_engine::render::render_page(&pdf, &output, 0, 800, None).expect("render");
    let rgba = img.image.to_rgba8();
    let (iw, ih) = rgba.dimensions();
    let scale = 800.0 / 612.0f32;
    let is_yellow = |x: u32, y: u32| {
        let p = rgba.get_pixel(x.min(iw - 1), y.min(ih - 1));
        p.0[0] > 200 && p.0[1] > 200 && p.0[2] < 150
    };
    let count_band = |top_pt: f32, bot_pt: f32| -> u32 {
        let y0 = ((792.0 - top_pt) * scale) as u32;
        let y1 = ((792.0 - bot_pt) * scale) as u32;
        let x0 = (72.0 * scale) as u32;
        let x1 = (280.0 * scale) as u32;
        (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).filter(|&(x, y)| is_yellow(x, y)).count() as u32
    };

    let band_line1 = count_band(736.0, 716.0);
    let band_line2 = count_band(706.0, 690.0);
    let band_gap = count_band(715.0, 707.0); // khoảng trống giữa 2 dòng

    assert!(band_line1 > 50, "dòng 1 không có pixel vàng ({band_line1}px)");
    assert!(band_line2 > 50, "dòng 2 không có pixel vàng ({band_line2}px)");
    assert!(
        band_gap < band_line1.min(band_line2) / 4,
        "khoảng trống giữa 2 dòng bị tô vàng như 1 khối liền ({band_gap}px so với dòng1={band_line1}px, dòng2={band_line2}px) — highlight đang phủ cả khoảng trắng, không bám theo dòng"
    );
}

#[test]
fn underline_and_strikeout_roundtrip() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = std::env::temp_dir().join("ff_annot_markup.pdf");
    let _ = std::fs::remove_file(&output);

    let specs = vec![
        AnnotSpec::markup(
            AnnotKind::Underline,
            0,
            Rect { left: 155.0, bottom: 690.0, right: 213.0, top: 706.0 },
            [0, 0, 255, 255],
        ),
        AnnotSpec::markup(
            AnnotKind::Strikeout,
            0,
            Rect { left: 72.0, bottom: 716.0, right: 151.0, top: 736.0 },
            [255, 0, 0, 255],
        ),
    ];
    ff_engine::apply_annotations(&pdf, &input, &output, &specs).expect("apply");

    let list = ff_engine::list_annotations(&pdf, &output).expect("list");
    let kinds: Vec<&str> = list.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.iter().any(|k| k.contains("Underline")), "thiếu Underline: {kinds:?}");
    assert!(kinds.iter().any(|k| k.contains("Strikeout")), "thiếu Strikeout: {kinds:?}");

    // Render được, không trắng trơn (có nét màu của underline/strikeout).
    let img = ff_engine::render::render_page(&pdf, &output, 0, 800, None).expect("render");
    let colored = img
        .image
        .to_rgba8()
        .pixels()
        .filter(|p| (p.0[2] > 180 && p.0[0] < 100) || (p.0[0] > 180 && p.0[1] < 100 && p.0[2] < 100))
        .count();
    assert!(colored > 20, "không thấy nét màu underline/strikeout ({colored} px)");
}

#[test]
fn freetext_and_note_roundtrip() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = std::env::temp_dir().join("ff_annot_text.pdf");
    let _ = std::fs::remove_file(&output);

    // FreeText màu xanh, cỡ 22, đậm (ASCII để kiểm định dạng render được);
    // Note nội dung tiếng Việt (kiểm round-trip UTF-16).
    let specs = vec![
        AnnotSpec {
            kind: AnnotKind::FreeText,
            page_index: 0,
            rect: Rect { left: 90.0, bottom: 560.0, right: 360.0, top: 620.0 },
            quads: vec![],
            color: [0, 0, 255, 255],
            contents: Some("Hello 22 blue".into()),
            font_size: 22.0,
            bold: true,
            italic: false,
            underline: false,
        },
        AnnotSpec {
            kind: AnnotKind::Note,
            page_index: 0,
            rect: Rect { left: 400.0, bottom: 600.0, right: 418.0, top: 618.0 },
            quads: vec![],
            color: [255, 200, 0, 255],
            contents: Some("ghi chú dán tiếng Việt".into()),
            font_size: 14.0,
            bold: false,
            italic: false,
            underline: false,
        },
    ];
    ff_engine::apply_annotations(&pdf, &input, &output, &specs).expect("apply");

    let list = ff_engine::list_annotations(&pdf, &output).expect("list");
    let kinds: Vec<&str> = list.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.iter().any(|k| k.contains("FreeText")), "thiếu FreeText: {kinds:?}");
    assert!(kinds.iter().any(|k| k.contains("Text")), "thiếu Text(note): {kinds:?}");
    // Nội dung được giữ lại (gồm tiếng Việt qua UTF-16).
    assert!(
        list.iter().any(|a| a.contents.as_deref() == Some("Hello 22 blue")),
        "mất contents FreeText: {list:?}"
    );
    assert!(
        list.iter().any(|a| a.contents.as_deref() == Some("ghi chú dán tiếng Việt")),
        "mất contents Note (tiếng Việt): {list:?}"
    );

    // Render trang 0: chữ FreeText màu xanh phải xuất hiện (DA hoạt động).
    let img = ff_engine::render::render_page(&pdf, &output, 0, 800, None).expect("render");
    let blue = img
        .image
        .to_rgba8()
        .pixels()
        .filter(|p| p.0[2] > 180 && p.0[0] < 90 && p.0[1] < 90)
        .count();
    assert!(blue > 40, "không thấy chữ FreeText màu xanh được render ({blue} px)");
}

/// Kiểm tra FreeText tiếng Việt render được (font Unicode nhúng + AP stream).
/// Annotation [72, 400, 360, 470] trên trang 612×792pt, render 800px.
#[test]
fn vietnamese_freetext_renders() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");
    let output = std::env::temp_dir().join("ff_viet_freetext.pdf");
    let _ = std::fs::remove_file(&output);

    let specs = vec![AnnotSpec {
        kind: AnnotKind::FreeText,
        page_index: 0,
        rect: Rect { left: 72.0, bottom: 400.0, right: 360.0, top: 470.0 },
        quads: vec![],
        color: [0, 0, 0, 255], // chữ đen để dễ kiểm
        contents: Some("Tiếng Việt: thử nghiệm font Unicode".into()),
        font_size: 14.0,
        bold: false,
        italic: false,
        underline: false,
    }];
    ff_engine::apply_annotations(&pdf, &input, &output, &specs).expect("apply");
    assert!(output.exists(), "không tạo được file");

    // Kiểm contents vẫn đúng
    let list = ff_engine::list_annotations(&pdf, &output).expect("list");
    assert!(
        list.iter().any(|a| a.contents.as_deref() == Some("Tiếng Việt: thử nghiệm font Unicode")),
        "mất contents tiếng Việt: {list:?}"
    );

    // Render và kiểm có pixels đen trong vùng annotation:
    // trang 612×792pt, render 800px → scale ≈ 1.307
    // pixel_y_top = (792 - 470) * scale ≈ 421, pixel_y_bot = (792 - 400) * scale ≈ 513
    // pixel_x_left = 72 * scale ≈ 94, pixel_x_right = 360 * scale ≈ 471
    let img = ff_engine::render::render_page(&pdf, &output, 0, 800, None).expect("render");
    let rgba = img.image.to_rgba8();
    let (iw, ih) = rgba.dimensions();
    let scale = 800.0 / 612.0f32;
    let x0 = (72.0 * scale) as u32;
    let x1 = (360.0 * scale) as u32;
    let y0 = ((792.0 - 470.0) * scale) as u32;
    let y1 = ((792.0 - 400.0) * scale) as u32;
    let x1 = x1.min(iw - 1);
    let y1 = y1.min(ih - 1);

    // Đếm pixels không phải trắng trong vùng annotation
    let dark: u32 = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = rgba.get_pixel(x, y);
            // pixel tối (nền trắng mà có glyph → lọc non-white)
            (p.0[0] as u32 + p.0[1] as u32 + p.0[2] as u32) < 700
        })
        .count() as u32;

    assert!(
        dark > 50,
        "tiếng Việt không render trong vùng annotation ({dark} dark px trong [{x0},{y0}]-[{x1},{y1}] của {iw}×{ih})"
    );
}

/// Kiểm underline trong FreeText AP stream tự dựng: so sánh số pixel tối trong
/// vùng annotation giữa bản có underline=true và underline=false — bản có gạch
/// chân phải nhiều pixel tối hơn rõ rệt (thêm 1 đường kẻ ngang dưới chữ).
#[test]
fn freetext_underline_renders() {
    let pdf = pdfium();
    let input = workspace_root().join("corpus").join("sample-multipage.pdf");

    fn count_dark(pdf: &pdfium_render::prelude::Pdfium, path: &std::path::Path) -> u32 {
        let img = ff_engine::render::render_page(pdf, path, 0, 800, None).expect("render");
        let rgba = img.image.to_rgba8();
        let (iw, ih) = rgba.dimensions();
        let scale = 800.0 / 612.0f32;
        let x0 = (72.0 * scale) as u32;
        let x1 = ((300.0 * scale) as u32).min(iw - 1);
        let y0 = ((792.0 - 650.0) * scale) as u32;
        let y1 = (((792.0 - 600.0) * scale) as u32).min(ih - 1);
        (y0..=y1)
            .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = rgba.get_pixel(x, y);
                (p.0[0] as u32 + p.0[1] as u32 + p.0[2] as u32) < 700
            })
            .count() as u32
    }

    let make_spec = |underline: bool| AnnotSpec {
        kind: AnnotKind::FreeText,
        page_index: 0,
        rect: Rect { left: 72.0, bottom: 600.0, right: 300.0, top: 650.0 },
        quads: vec![],
        color: [0, 0, 0, 255],
        contents: Some("Underline".into()),
        font_size: 20.0,
        bold: false,
        italic: false,
        underline,
    };

    let no_ul = std::env::temp_dir().join("ff_freetext_no_underline.pdf");
    let with_ul = std::env::temp_dir().join("ff_freetext_with_underline.pdf");
    let _ = std::fs::remove_file(&no_ul);
    let _ = std::fs::remove_file(&with_ul);

    ff_engine::apply_annotations(&pdf, &input, &no_ul, &[make_spec(false)]).expect("apply no-ul");
    ff_engine::apply_annotations(&pdf, &input, &with_ul, &[make_spec(true)]).expect("apply with-ul");

    let dark_no_ul = count_dark(&pdf, &no_ul);
    let dark_with_ul = count_dark(&pdf, &with_ul);

    assert!(
        dark_with_ul > dark_no_ul + 50,
        "gạch chân không render rõ rệt: no_underline={dark_no_ul}px, with_underline={dark_with_ul}px"
    );
}
