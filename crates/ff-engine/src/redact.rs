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

use crate::text::Rect;
use crate::EngineError;

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
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let touched;
    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        // (1) Phân loại object bị chạm: ảnh → bôi đen pixel; còn lại → xoá.
        let mut to_remove: Vec<usize> = Vec::new();
        struct ImgFix {
            idx: usize,
            bounds: Rect,
            cuts: Vec<Rect>,
        }
        let mut img_fixes: Vec<ImgFix> = Vec::new();
        for (i, obj) in page.objects().iter().enumerate() {
            let Some(b) = obj_rect(&obj) else { continue };
            let cuts: Vec<Rect> = areas.iter().filter_map(|a| intersection(a, &b)).collect();
            if cuts.is_empty() {
                continue;
            }
            if obj.object_type() == PdfPageObjectType::Image {
                img_fixes.push(ImgFix { idx: i, bounds: b, cuts });
            } else {
                to_remove.push(i);
            }
        }

        // (2) Ảnh: đọc pixel gốc, tô đen phần giao. Không đọc được → xoá cả ảnh.
        struct NewImg {
            bounds: Rect,
            img: image::DynamicImage,
        }
        let mut new_imgs: Vec<NewImg> = Vec::new();
        for fix in &img_fixes {
            let obj = page.objects().get(fix.idx).map_err(err)?;
            let raw = obj.as_image_object().and_then(|io| io.get_raw_image().ok());
            match raw {
                Some(img) => {
                    let mut rgba = img.to_rgba8();
                    let (w, h) = (rgba.width() as f32, rgba.height() as f32);
                    let bw = (fix.bounds.right - fix.bounds.left).max(0.01);
                    let bh = (fix.bounds.top - fix.bounds.bottom).max(0.01);
                    for cut in &fix.cuts {
                        // Trang → pixel: gốc pixel (0,0) là góc TRÊN-trái của ảnh.
                        let x0 = (((cut.left - fix.bounds.left) / bw) * w).floor().max(0.0) as u32;
                        let x1 = (((cut.right - fix.bounds.left) / bw) * w).ceil().min(w) as u32;
                        let y0 = (((fix.bounds.top - cut.top) / bh) * h).floor().max(0.0) as u32;
                        let y1 = (((fix.bounds.top - cut.bottom) / bh) * h).ceil().min(h) as u32;
                        for y in y0..y1 {
                            for x in x0..x1 {
                                rgba.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                            }
                        }
                    }
                    new_imgs.push(NewImg { bounds: fix.bounds, img: image::DynamicImage::ImageRgba8(rgba) });
                    to_remove.push(fix.idx);
                }
                None => to_remove.push(fix.idx),
            }
        }

        // (3) Xoá theo index GIẢM DẦN (bẫy Drop của PDFium → mem::forget).
        to_remove.sort_unstable();
        to_remove.dedup();
        touched = to_remove.len();
        for idx in to_remove.into_iter().rev() {
            let removed = page.objects_mut().remove_object_at_index(idx).map_err(err)?;
            std::mem::forget(removed);
        }

        // (4) Đặt lại ảnh đã bôi đen (giữ khung cũ).
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

        // (5) Khối đen phủ mỗi vùng — dấu hiệu thị giác chuẩn của redaction.
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
