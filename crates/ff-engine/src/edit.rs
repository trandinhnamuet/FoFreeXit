//! Chỉnh sửa nội dung trang (Phase 4 — moat chính): liệt kê & sửa trực tiếp
//! các page object (text run / ảnh / path) trên 1 trang.
//!
//! PDFium đã expose page object cấp cao nên KHÔNG cần tự parse content stream:
//! mỗi `PdfPageTextObject` là 1 text run sẵn để sửa (có text/font/size/matrix/
//! màu riêng). Xem khảo sát trong kế hoạch Phase 4.
//!
//! Bài học tái dùng:
//! - Sửa/Thêm text dùng FULL font nhúng (`find_font_bytes` +
//!   `load_true_type_from_bytes(..., true)`) để tiếng Việt hiển thị đúng dấu —
//!   `set_text()` của PDFium re-encode theo font HIỆN TẠI (thường là subset)
//!   nên glyph ngoài subset sẽ mất; vì vậy SetText = tạo lại object bằng full
//!   font, giữ nguyên matrix/cỡ/màu gốc.
//! - PDFium cần `regenerate_content()` trước khi lưu, nếu không mất thay đổi.

use std::collections::HashMap;
use std::path::Path;

use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::text::Rect;
use crate::EngineError;

/// Loại page object (rút gọn cho UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Text,
    Image,
    Path,
    Shading,
    XObjectForm,
    Unsupported,
}

impl ObjectKind {
    fn from_pdfium(t: PdfPageObjectType) -> Self {
        match t {
            PdfPageObjectType::Text => ObjectKind::Text,
            PdfPageObjectType::Image => ObjectKind::Image,
            PdfPageObjectType::Path => ObjectKind::Path,
            PdfPageObjectType::Shading => ObjectKind::Shading,
            PdfPageObjectType::XObjectForm => ObjectKind::XObjectForm,
            PdfPageObjectType::Unsupported => ObjectKind::Unsupported,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectKind::Text => "text",
            ObjectKind::Image => "image",
            ObjectKind::Path => "path",
            ObjectKind::Shading => "shading",
            ObjectKind::XObjectForm => "form",
            ObjectKind::Unsupported => "unsupported",
        }
    }
}

/// Thông tin 1 page object trả về cho UI (để vẽ overlay & sửa).
#[derive(Clone, Debug)]
pub struct ObjectInfo {
    /// Index trong danh sách object của trang (0-based, theo thứ tự vẽ/z-order).
    pub index: u16,
    pub kind: ObjectKind,
    /// Khung bao (AABB) theo điểm PDF.
    pub rect: Rect,
    /// Chỉ với text: nội dung hiện tại.
    pub text: Option<String>,
    /// Chỉ với text: tên font.
    pub font_name: Option<String>,
    /// Chỉ với text: cỡ chữ hiệu dụng (đã tính scale của matrix).
    pub font_size: Option<f32>,
    /// Chỉ với text: màu chữ RGBA.
    pub color: Option<[u8; 4]>,
}

/// Một thao tác sửa nội dung. UI dàn dựng danh sách op rồi áp 1 lần khi lưu.
#[derive(Clone, Debug)]
pub enum EditOp {
    /// Dịch (dx,dy) và/hoặc scale (sx,sy) object `index` — scale quanh góc
    /// dưới-trái của object để giữ neo góc khi resize.
    Transform { index: u16, dx: f32, dy: f32, sx: f32, sy: f32 },
    /// Sửa nội dung text object `index` (tạo lại bằng full font, giữ matrix gốc).
    SetText { index: u16, text: String, font_size: Option<f32>, color: Option<[u8; 4]>, bold: bool, italic: bool },
    /// Xoá object `index`.
    Delete { index: u16 },
    /// Thay ảnh của image object `index` bằng ảnh từ `image_path`, giữ khung cũ.
    ReplaceImage { index: u16, image_path: String },
    /// Thêm text box mới tại (x,y) (gốc dưới-trái, điểm PDF).
    AddText { x: f32, y: f32, text: String, font_size: f32, color: [u8; 4], bold: bool, italic: bool },
    /// Thêm ảnh mới từ file, khung width_pt × height_pt tại (x,y).
    AddImage { x: f32, y: f32, width_pt: f32, height_pt: f32, image_path: String },
}

fn quad_to_rect(q: &PdfQuadPoints) -> Rect {
    Rect {
        left: q.left().value,
        bottom: q.bottom().value,
        right: q.right().value,
        top: q.top().value,
    }
}

/// Liệt kê các page object của 1 trang để UI vẽ overlay chỉnh sửa.
pub fn list_objects(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    password: Option<&str>,
) -> Result<Vec<ObjectInfo>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;

    let mut out = Vec::new();
    for (i, object) in page.objects().iter().enumerate() {
        let kind = ObjectKind::from_pdfium(object.object_type());
        let rect = object
            .bounds()
            .map(|q| quad_to_rect(&q))
            .unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });

        let (text, font_name, font_size, color) = if let Some(t) = object.as_text_object() {
            let c = t.fill_color().ok();
            (
                Some(t.text()),
                Some(t.font().name()),
                Some(t.scaled_font_size().value),
                c.map(|c| [c.red(), c.green(), c.blue(), c.alpha()]),
            )
        } else {
            (None, None, None, None)
        };

        out.push(ObjectInfo {
            index: i as u16,
            kind,
            rect,
            text,
            font_name,
            font_size,
            color,
        });
    }
    Ok(out)
}

/// Dữ liệu chụp lại từ 1 object trước khi xoá để tạo lại bản thay thế.
struct Captured {
    matrix: PdfMatrix,
    unscaled_font_size: f32,
    color: PdfColor,
    rect: Rect,
}

/// Áp danh sách `ops` lên trang `page_index` của `input`, ghi ra `output`.
/// Không sửa `input`. Thứ tự xử lý giữ index gốc hợp lệ: Transform in-place →
/// chụp dữ liệu object sắp thay/xoá → xoá theo index GIẢM DẦN → thêm bản thay
/// thế → thêm object mới → regenerate → lưu.
pub fn apply_edits(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    ops: &[EditOp],
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    // Nạp trước token cho mọi tổ hợp (bold,italic) cần dùng (SetText/AddText) —
    // phải làm TRƯỚC khi mượn page vì cần `document.fonts_mut()`.
    let mut needed_fonts: Vec<(bool, bool)> = Vec::new();
    for op in ops {
        match op {
            EditOp::SetText { bold, italic, .. } | EditOp::AddText { bold, italic, .. } => {
                if !needed_fonts.contains(&(*bold, *italic)) {
                    needed_fonts.push((*bold, *italic));
                }
            }
            _ => {}
        }
    }
    let mut tokens: HashMap<(bool, bool), PdfFontToken> = HashMap::new();
    for (bold, italic) in needed_fonts {
        let bytes = find_font_bytes(bold, italic)
            .ok_or_else(|| EngineError::Pdfium("không tìm được font hệ thống để sửa/thêm chữ".into()))?;
        let token = document
            .fonts_mut()
            .load_true_type_from_bytes(&bytes, true)
            .map_err(|e| EngineError::Pdfium(format!("nạp font: {e}")))?;
        tokens.insert((bold, italic), token);
    }

    let err = |e: PdfiumError| EngineError::Pdfium(format!("sửa nội dung: {e}"));

    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        let obj_count = page.objects().len();
        let valid = |i: u16| (i as usize) < obj_count;

        // (1) Transform in-place (không đổi cấu trúc/index).
        for op in ops {
            if let EditOp::Transform { index, dx, dy, sx, sy } = op {
                if !valid(*index) {
                    continue;
                }
                let mut obj = page.objects().get(*index as usize).map_err(err)?;
                if (*sx - 1.0).abs() > f32::EPSILON || (*sy - 1.0).abs() > f32::EPSILON {
                    let b = obj.bounds().map_err(err)?;
                    let (x0, y0) = (b.left().value, b.bottom().value);
                    // Scale quanh góc dưới-trái: về gốc → scale → về chỗ cũ.
                    obj.translate(PdfPoints::new(-x0), PdfPoints::new(-y0)).map_err(err)?;
                    obj.scale(*sx, *sy).map_err(err)?;
                    obj.translate(PdfPoints::new(x0), PdfPoints::new(y0)).map_err(err)?;
                }
                if *dx != 0.0 || *dy != 0.0 {
                    obj.translate(PdfPoints::new(*dx), PdfPoints::new(*dy)).map_err(err)?;
                }
            }
        }

        // (2) Chụp dữ liệu cho object sắp THAY (SetText/ReplaceImage) — sau khi
        // đã Transform để bản thay thế kế thừa cả phép biến đổi đó.
        let mut captured: HashMap<u16, Captured> = HashMap::new();
        for op in ops {
            let idx = match op {
                EditOp::SetText { index, .. } | EditOp::ReplaceImage { index, .. } => *index,
                _ => continue,
            };
            if !valid(idx) || captured.contains_key(&idx) {
                continue;
            }
            let obj = page.objects().get(idx as usize).map_err(err)?;
            let rect = obj.bounds().map(|q| quad_to_rect(&q)).unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
            let (matrix, unscaled, color) = if let Some(t) = obj.as_text_object() {
                (
                    obj.matrix().map_err(err)?,
                    t.unscaled_font_size().value,
                    t.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
                )
            } else {
                (obj.matrix().map_err(err)?, 0.0, PdfColor::new(0, 0, 0, 255))
            };
            captured.insert(idx, Captured { matrix, unscaled_font_size: unscaled, color, rect });
        }

        // (3) Xoá mọi index thuộc SetText/ReplaceImage/Delete theo GIẢM DẦN.
        let mut to_remove: Vec<u16> = ops
            .iter()
            .filter_map(|op| match op {
                EditOp::SetText { index, .. }
                | EditOp::ReplaceImage { index, .. }
                | EditOp::Delete { index } => Some(*index),
                _ => None,
            })
            .filter(|i| valid(*i))
            .collect();
        to_remove.sort_unstable();
        to_remove.dedup();
        for idx in to_remove.into_iter().rev() {
            let removed = page.objects_mut().remove_object_at_index(idx as usize).map_err(err)?;
            // BẪY PDFium: object vừa tách khỏi trang bị đánh dấu "unowned" → Drop
            // gọi FPDFPageObj_Destroy, mà destroy object vốn thuộc document gây
            // SEGFAULT (chính pdfium-render cũng cảnh báo điều này trong Drop của
            // PdfPageObject). Object đã được FPDFPage_RemoveObject tách ra nên sẽ
            // không còn render/lưu; ta `forget` để KHỎI gọi destroy. Rò rỉ không
            // đáng kể (giải phóng khi document đóng ngay sau khi lưu).
            std::mem::forget(removed);
        }

        // (4) Thêm bản thay thế cho SetText / ReplaceImage.
        for op in ops {
            match op {
                EditOp::SetText { index, text, font_size, color, bold, italic } => {
                    let cap = match captured.get(index) {
                        Some(c) => c,
                        None => continue,
                    };
                    let token = tokens.get(&(*bold, *italic)).copied().ok_or_else(|| {
                        EngineError::Pdfium("thiếu token font cho SetText".into())
                    })?;
                    let size = font_size.unwrap_or(cap.unscaled_font_size).max(1.0);
                    let mut obj = page
                        .objects_mut()
                        .create_text_object(PdfPoints::ZERO, PdfPoints::ZERO, text.clone(), token, PdfPoints::new(size))
                        .map_err(err)?;
                    // Object mới tạo tại gốc (0,0) cỡ unscaled → matrix ~identity,
                    // nên apply_matrix(matrix gốc) tương đương đặt lại đúng vị trí/scale.
                    obj.apply_matrix(cap.matrix).map_err(err)?;
                    let c = color
                        .map(|c| PdfColor::new(c[0], c[1], c[2], c[3]))
                        .unwrap_or(cap.color);
                    obj.set_fill_color(c).map_err(err)?;
                }
                EditOp::ReplaceImage { index, image_path } => {
                    let cap = match captured.get(index) {
                        Some(c) => c,
                        None => continue,
                    };
                    let img = image::open(image_path)
                        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh {image_path}: {e}")))?;
                    let w = (cap.rect.right - cap.rect.left).max(1.0);
                    let h = (cap.rect.top - cap.rect.bottom).max(1.0);
                    page.objects_mut()
                        .create_image_object(
                            PdfPoints::new(cap.rect.left),
                            PdfPoints::new(cap.rect.bottom),
                            &img,
                            Some(PdfPoints::new(w)),
                            Some(PdfPoints::new(h)),
                        )
                        .map_err(err)?;
                }
                _ => {}
            }
        }

        // (5) Thêm object MỚI (AddText / AddImage).
        for op in ops {
            match op {
                EditOp::AddText { x, y, text, font_size, color, bold, italic } => {
                    let token = tokens.get(&(*bold, *italic)).copied().ok_or_else(|| {
                        EngineError::Pdfium("thiếu token font cho AddText".into())
                    })?;
                    let mut obj = page
                        .objects_mut()
                        .create_text_object(
                            PdfPoints::new(*x),
                            PdfPoints::new(*y),
                            text.clone(),
                            token,
                            PdfPoints::new(font_size.max(1.0)),
                        )
                        .map_err(err)?;
                    obj.set_fill_color(PdfColor::new(color[0], color[1], color[2], color[3]))
                        .map_err(err)?;
                }
                EditOp::AddImage { x, y, width_pt, height_pt, image_path } => {
                    let img = image::open(image_path)
                        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh {image_path}: {e}")))?;
                    page.objects_mut()
                        .create_image_object(
                            PdfPoints::new(*x),
                            PdfPoints::new(*y),
                            &img,
                            Some(PdfPoints::new(width_pt.max(1.0))),
                            Some(PdfPoints::new(height_pt.max(1.0))),
                        )
                        .map_err(err)?;
                }
                _ => {}
            }
        }

        page.regenerate_content().map_err(err)?;
    }

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(())
}
