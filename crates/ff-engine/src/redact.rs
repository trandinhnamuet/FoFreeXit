//! Redaction THẬT (Phase 5): xoá nội dung khỏi file — không phải vẽ đè.
//!
//! Nguyên tắc an toàn (nghiêng về XOÁ THỪA, không bao giờ xoá thiếu):
//! - Text/path/shading/form GIAO vùng redact → xoá NGUYÊN object (v1 xoá cả
//!   run text bị chạm — cắt theo ký tự là nâng cấp v2; xoá thừa an toàn hơn
//!   sót chữ).
//! - Ảnh giao vùng → đọc pixel gốc, BÔI ĐEN đúng phần giao rồi thay ảnh
//!   (nội dung gốc không còn trong file); không đọc được pixel → xoá cả ảnh.
//! - Sau khi xoá, vẽ khối đen phủ mỗi vùng (dấu hiệu thị giác chuẩn redaction).
//!
//! Kiểm chứng: caller (test/UI) extract_text + render lại để xác nhận nội
//! dung đã biến mất thật — xem `tests/redact_roundtrip.rs`.

use std::path::Path;

use pdfium_render::prelude::*;

use crate::text::{CharBox, Rect};
use crate::{fontmatch, EngineError};

/// 1 đoạn chữ GIỮ LẠI sau khi tỉa các ký tự thuộc vùng redact.
struct KeptSegment {
    text: String,
    left: f32,
    baseline: f32,
}

fn rect_hits_box(r: &Rect, b: &CharBox) -> bool {
    r.left < b.right && b.left < r.right && r.bottom < b.top && b.bottom < r.top
}

fn intersection(a: &Rect, b: &Rect) -> Option<Rect> {
    let r = Rect {
        left: a.left.max(b.left),
        bottom: a.bottom.max(b.bottom),
        right: a.right.min(b.right),
        top: a.top.min(b.top),
    };
    (r.left < r.right && r.bottom < r.top).then_some(r)
}

fn obj_rect(obj: &PdfPageObject) -> Option<Rect> {
    obj.bounds().ok().map(|q| Rect {
        left: q.left().value,
        bottom: q.bottom().value,
        right: q.right().value,
        top: q.top().value,
    })
}

/// Xoá thật nội dung trong các vùng `areas` (điểm PDF) trên trang `page_index`,
/// ghi ra `output`. Trả về số object đã xoá/bôi đen (để UI báo cáo).
pub fn redact_areas(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    areas: &[Rect],
    output: &Path,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    let err = |e: PdfiumError| EngineError::Pdfium(format!("redact: {e}"));

    // Char boxes của trang (đọc TRƯỚC khi mở document ghi) — để tỉa text theo
    // KÝ TỰ: chỉ xoá ký tự nằm trong vùng redact, giữ phần còn lại đúng chỗ.
    let char_boxes = crate::text::page_char_boxes(pdfium, input, page_index, password)?;

    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    struct NewImg {
        bounds: Rect,
        img: image::DynamicImage,
    }
    let mut to_remove: Vec<usize> = Vec::new();
    let mut new_imgs: Vec<NewImg> = Vec::new();
    let mut text_trims: Vec<TextTrimPlan> = Vec::new();

    // ---- Pha ĐỌC (mượn trang, chỉ đọc): phân loại + chụp dữ liệu cần dựng lại.
    {
        let page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        for (i, obj) in page.objects().iter().enumerate() {
            let Some(b) = obj_rect(&obj) else { continue };
            let cuts: Vec<Rect> = areas.iter().filter_map(|a| intersection(a, &b)).collect();
            if cuts.is_empty() {
                continue;
            }
            match obj.object_type() {
                PdfPageObjectType::Image => {
                    // Đọc pixel gốc, tô đen phần giao. Không đọc được → xoá cả ảnh.
                    match obj.as_image_object().and_then(|io| io.get_raw_image().ok()) {
                        Some(img) => {
                            let mut rgba = img.to_rgba8();
                            let (w, h) = (rgba.width() as f32, rgba.height() as f32);
                            let bw = (b.right - b.left).max(0.01);
                            let bh = (b.top - b.bottom).max(0.01);
                            for cut in &cuts {
                                let x0 = (((cut.left - b.left) / bw) * w).floor().max(0.0) as u32;
                                let x1 = (((cut.right - b.left) / bw) * w).ceil().min(w) as u32;
                                let y0 = (((b.top - cut.top) / bh) * h).floor().max(0.0) as u32;
                                let y1 = (((b.top - cut.bottom) / bh) * h).ceil().min(h) as u32;
                                for y in y0..y1 {
                                    for x in x0..x1 {
                                        rgba.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                                    }
                                }
                            }
                            new_imgs.push(NewImg { bounds: b, img: image::DynamicImage::ImageRgba8(rgba) });
                            to_remove.push(i);
                        }
                        None => to_remove.push(i),
                    }
                }
                PdfPageObjectType::Text => {
                    // Tỉa theo KÝ TỰ; không chắc chắn (xoay/không đọc được font)
                    // → xoá NGUYÊN object (an toàn: thà thừa còn hơn sót).
                    to_remove.push(i);
                    if let Some(trim) = plan_text_trim(&obj, &b, areas, &char_boxes) {
                        if !trim.segments.is_empty() {
                            text_trims.push(trim);
                        }
                    }
                }
                _ => to_remove.push(i),
            }
        }
    }

    // ---- Nạp font cho các đoạn chữ giữ lại (cần fonts_mut → ngoài mượn trang).
    let mut trim_tokens: Vec<PdfFontToken> = Vec::with_capacity(text_trims.len());
    for trim in &text_trims {
        let token = document
            .fonts_mut()
            .load_true_type_from_bytes(&trim.font_bytes, true)
            .map_err(|e| EngineError::Pdfium(format!("nạp font tỉa redact: {e}")))?;
        trim_tokens.push(token);
    }

    let touched = to_remove.len();
    // ---- Pha GHI (mượn trang mut).
    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        // Xoá theo index GIẢM DẦN (bẫy Drop của PDFium → mem::forget).
        to_remove.sort_unstable();
        to_remove.dedup();
        for idx in to_remove.into_iter().rev() {
            let removed = page.objects_mut().remove_object_at_index(idx).map_err(err)?;
            std::mem::forget(removed);
        }

        // Tạo lại các đoạn chữ GIỮ LẠI (tỉa theo ký tự).
        for (trim, token) in text_trims.iter().zip(trim_tokens.iter()) {
            let (a, b, c, d) = trim.linear;
            for seg in &trim.segments {
                let mut o = page
                    .objects_mut()
                    .create_text_object(
                        PdfPoints::ZERO,
                        PdfPoints::ZERO,
                        seg.text.clone(),
                        *token,
                        PdfPoints::new(trim.tf),
                    )
                    .map_err(err)?;
                o.apply_matrix(PdfMatrix::new(a, b, c, d, seg.left, seg.baseline)).map_err(err)?;
                o.set_fill_color(trim.color).map_err(err)?;
            }
        }

        // Đặt lại ảnh đã bôi đen (giữ khung cũ).
        for ni in &new_imgs {
            let w = (ni.bounds.right - ni.bounds.left).max(1.0);
            let h = (ni.bounds.top - ni.bounds.bottom).max(1.0);
            page.objects_mut()
                .create_image_object(
                    PdfPoints::new(ni.bounds.left),
                    PdfPoints::new(ni.bounds.bottom),
                    &ni.img,
                    Some(PdfPoints::new(w)),
                    Some(PdfPoints::new(h)),
                )
                .map_err(err)?;
        }

        // Khối đen phủ mỗi vùng — dấu hiệu thị giác chuẩn của redaction.
        for a in areas {
            page.objects_mut()
                .create_path_object_rect(
                    PdfRect::new_from_values(a.bottom, a.left, a.top, a.right),
                    None,
                    None,
                    Some(PdfColor::new(0, 0, 0, 255)),
                )
                .map_err(err)?;
        }

        page.regenerate_content().map_err(err)?;
    }
    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(touched)
}

/// Lập kế hoạch tỉa 1 text object theo ký tự. Trả None nếu không chắc chắn
/// (xoay/nghiêng, không đọc được font, không map được char box) → caller xoá cả.
fn plan_text_trim(
    obj: &PdfPageObject,
    bounds: &Rect,
    areas: &[Rect],
    char_boxes: &[CharBox],
) -> Option<TextTrimPlan> {
    let t = obj.as_text_object()?;
    let m = obj.matrix().ok()?;
    // Guard: có xoay/nghiêng (b,c đáng kể) → không tỉa (đặt lại theo trục ngang
    // sẽ sai). Xoá nguyên object cho an toàn.
    if m.b().abs() > 0.01 || m.c().abs() > 0.01 {
        return None;
    }
    let font_bytes = pick_font_bytes(t)?;
    let tf = t.unscaled_font_size().value.max(1.0);
    let color = t.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255));

    // Char box thuộc object này ≈ box có tâm nằm trong bounds (nới nhẹ).
    let pad = 1.0;
    let mut chars: Vec<&CharBox> = char_boxes
        .iter()
        .filter(|cb| {
            let cx = (cb.left + cb.right) / 2.0;
            let cy = (cb.bottom + cb.top) / 2.0;
            cx >= bounds.left - pad
                && cx <= bounds.right + pad
                && cy >= bounds.bottom - pad
                && cy <= bounds.top + pad
        })
        .collect();
    if chars.is_empty() {
        return None;
    }
    chars.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal));

    // Gom các ký tự KHÔNG bị vùng redact chạm thành đoạn liên tiếp.
    let baseline = m.f();
    let mut segments: Vec<KeptSegment> = Vec::new();
    let mut cur = String::new();
    let mut cur_left = 0.0f32;
    for cb in &chars {
        let cut = areas.iter().any(|a| rect_hits_box(a, cb));
        if cut {
            if !cur.is_empty() {
                segments.push(KeptSegment { text: std::mem::take(&mut cur), left: cur_left, baseline });
            }
        } else {
            if cur.is_empty() {
                cur_left = cb.left;
            }
            cur.push_str(&cb.ch);
        }
    }
    if !cur.is_empty() {
        segments.push(KeptSegment { text: cur, left: cur_left, baseline });
    }

    Some(TextTrimPlan { segments, tf, linear: (m.a(), m.b(), m.c(), m.d()), color, font_bytes })
}

/// Chọn bytes font để dựng lại đoạn giữ: ưu tiên font NHÚNG gốc, sau đó font
/// hệ thống cùng họ, cuối cùng font mặc định.
fn pick_font_bytes(t: &PdfPageTextObject) -> Option<Vec<u8>> {
    if t.font().is_embedded().unwrap_or(false) {
        if let Ok(bytes) = t.font().data() {
            if ttf_parser::Face::parse(&bytes, 0).is_ok() {
                return Some(bytes);
            }
        }
    }
    let raw = t.font().name();
    let (family, bold, italic) = fontmatch::clean_font_name(&raw);
    fontmatch::find_family_font_bytes(&family, bold, italic)
        .or_else(|| crate::annot::find_font_bytes(bold, italic))
}

/// Kết quả `plan_text_trim` (đặt ngoài để dùng trong chữ ký hàm trên).
struct TextTrimPlan {
    segments: Vec<KeptSegment>,
    tf: f32,
    linear: (f32, f32, f32, f32),
    color: PdfColor,
    font_bytes: Vec<u8>,
}
