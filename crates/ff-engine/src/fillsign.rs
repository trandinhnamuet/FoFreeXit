//! Điền & Ký (Fill & Sign kiểu Foxit): đóng dấu chữ ký/chữ viết tắt (ảnh),
//! chữ, dấu ✓ ✗ ●, đường kẻ, ngày… vào NỘI DUNG trang (flatten như Foxit —
//! không còn là annotation sửa/xoá được).
//!
//! - Chữ: đi qua `edit::apply_edits` (AddText) để dùng chung cơ chế chọn/nhúng
//!   font Unicode (tiếng Việt, CJK…) của chế độ Sửa.
//! - Ảnh chữ ký: AddImage (PNG có alpha → PDFium tạo SMask, nền trong suốt).
//! - Dấu ✓ ✗ ● và đường kẻ: path VECTOR (nét tròn đầu) — sắc nét mọi mức zoom/in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;
use serde::Deserialize;

use crate::{EditOp, EngineError};

/// Loại dấu vector.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarkKind {
    Check,
    Cross,
    Dot,
    Line,
}

/// 1 phần tử Điền & Ký đã đặt trên trang. Toạ độ điểm PDF, gốc dưới-trái.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FillItem {
    /// Ảnh (chữ ký / chữ viết tắt) trong khung (x,y,w,h).
    #[serde(rename_all = "camelCase")]
    Image { page: u16, x: f32, y: f32, width: f32, height: f32, image_path: String },
    /// Chữ: (x,y) = góc dưới-trái dòng ĐẦU (baseline); `\n` = xuống dòng.
    #[serde(rename_all = "camelCase")]
    Text {
        page: u16,
        x: f32,
        y: f32,
        text: String,
        font_size: f32,
        #[serde(default = "black")]
        color: [u8; 4],
        #[serde(default)]
        font_family: Option<String>,
        #[serde(default)]
        line_height: Option<f32>,
    },
    /// Dấu vector trong khung (x,y,w,h). Line: từ (x, y+h/2) tới (x+w, y+h/2).
    #[serde(rename_all = "camelCase")]
    Mark {
        page: u16,
        mark: MarkKind,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        #[serde(default = "black")]
        color: [u8; 4],
        #[serde(default)]
        stroke: Option<f32>,
    },
}

fn black() -> [u8; 4] {
    [0, 0, 0, 255]
}

impl FillItem {
    fn page(&self) -> u16 {
        match self {
            FillItem::Image { page, .. } | FillItem::Text { page, .. } | FillItem::Mark { page, .. } => *page,
        }
    }
}

fn tmp_path(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("ff_fill_{tag}_{}_{n}.pdf", std::process::id()))
}

/// Đóng dấu `items` vào `input`, ghi ra `output`. Trả số phần tử đã đóng.
pub fn apply_fill_sign(
    pdfium: &Pdfium,
    input: &Path,
    items: &[FillItem],
    output: &Path,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    if items.is_empty() {
        return Err(EngineError::Pdfium("chưa có gì để đóng vào tài liệu".into()));
    }
    let npages = crate::render::page_count(pdfium, input, password)?;
    for it in items {
        if it.page() >= npages {
            return Err(EngineError::Pdfium(format!("trang {} không tồn tại", it.page() + 1)));
        }
    }

    // (1) Chữ + ảnh: gom theo trang → apply_edits nối tiếp qua file tạm.
    let mut by_page: BTreeMap<u16, Vec<EditOp>> = BTreeMap::new();
    for it in items {
        match it {
            FillItem::Image { page, x, y, width, height, image_path } => {
                by_page.entry(*page).or_default().push(EditOp::AddImage {
                    x: *x,
                    y: *y,
                    width_pt: *width,
                    height_pt: *height,
                    image_path: image_path.clone(),
                });
            }
            FillItem::Text { page, x, y, text, font_size, color, font_family, line_height } => {
                let lh = line_height.unwrap_or(font_size * 1.2);
                for (i, line) in text.split('\n').enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    by_page.entry(*page).or_default().push(EditOp::AddText {
                        x: *x,
                        y: *y - lh * i as f32,
                        text: line.to_string(),
                        font_size: font_size.max(1.0),
                        color: *color,
                        font_family: font_family.clone().filter(|f| !f.trim().is_empty()),
                        bold: false,
                        italic: false,
                    });
                }
            }
            FillItem::Mark { .. } => {}
        }
    }
    let mut temps: Vec<PathBuf> = Vec::new();
    let mut cur: PathBuf = input.to_path_buf();
    let mut cur_pw = password;
    for (page, ops) in &by_page {
        if ops.is_empty() {
            continue;
        }
        let next = tmp_path("t");
        let r = crate::edit::apply_edits(pdfium, &cur, *page, ops, &next, cur_pw);
        if let Err(e) = r {
            for t in &temps {
                let _ = std::fs::remove_file(t);
            }
            return Err(e);
        }
        temps.push(next.clone());
        cur = next;
        cur_pw = None; // PDFium lưu lại không mã hoá
    }

    // (2) Dấu vector.
    let marks: Vec<&FillItem> = items.iter().filter(|i| matches!(i, FillItem::Mark { .. })).collect();
    let res = (|| -> Result<(), EngineError> {
        if marks.is_empty() {
            if cur == input {
                std::fs::copy(input, output)?;
            } else {
                std::fs::copy(&cur, output)?;
            }
            return Ok(());
        }
        let err = |e: PdfiumError| EngineError::Pdfium(format!("điền & ký: {e}"));
        let document = pdfium.load_pdf_from_file(&cur, cur_pw).map_err(err)?;
        for m in marks {
            let FillItem::Mark { page, mark, x, y, width, height, color, stroke } = m else { continue };
            let mut pg = document.pages().get(*page).map_err(err)?;
            let c = PdfColor::new(color[0], color[1], color[2], color[3]);
            let (x, y, w, h) = (*x, *y, width.max(1.0), height.max(1.0));
            let sw = stroke.unwrap_or((w.min(h) * 0.12).max(0.8));
            let p = |v: f32| PdfPoints::new(v);
            let mut path = match mark {
                MarkKind::Dot => {
                    let r = w.min(h) / 2.0;
                    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
                    let mut o = PdfPagePathObject::new(&document, p(cx + r), p(cy), None, None, Some(c)).map_err(err)?;
                    // 4 cung Bézier (k = 0.5523) — hình tròn đặc.
                    let k = 0.5523 * r;
                    o.bezier_to(p(cx), p(cy + r), p(cx + r), p(cy + k), p(cx + k), p(cy + r)).map_err(err)?;
                    o.bezier_to(p(cx - r), p(cy), p(cx - k), p(cy + r), p(cx - r), p(cy + k)).map_err(err)?;
                    o.bezier_to(p(cx), p(cy - r), p(cx - r), p(cy - k), p(cx - k), p(cy - r)).map_err(err)?;
                    o.bezier_to(p(cx + r), p(cy), p(cx + k), p(cy - r), p(cx + r), p(cy - k)).map_err(err)?;
                    o.close_path().map_err(err)?;
                    o
                }
                MarkKind::Check => {
                    let mut o = PdfPagePathObject::new(&document, p(x + w * 0.08), p(y + h * 0.52), Some(c), Some(p(sw)), None)
                        .map_err(err)?;
                    o.line_to(p(x + w * 0.38), p(y + h * 0.15)).map_err(err)?;
                    o.line_to(p(x + w * 0.92), p(y + h * 0.88)).map_err(err)?;
                    o
                }
                MarkKind::Cross => {
                    let i = sw / 2.0 + w.min(h) * 0.08;
                    let mut o = PdfPagePathObject::new(&document, p(x + i), p(y + i), Some(c), Some(p(sw)), None).map_err(err)?;
                    o.line_to(p(x + w - i), p(y + h - i)).map_err(err)?;
                    o.move_to(p(x + i), p(y + h - i)).map_err(err)?;
                    o.line_to(p(x + w - i), p(y + i)).map_err(err)?;
                    o
                }
                MarkKind::Line => {
                    let sw = stroke.unwrap_or(1.0);
                    let mut o = PdfPagePathObject::new(&document, p(x), p(y + h / 2.0), Some(c), Some(p(sw)), None).map_err(err)?;
                    o.line_to(p(x + w), p(y + h / 2.0)).map_err(err)?;
                    o
                }
            };
            let _ = path.set_line_cap(PdfPageObjectLineCap::Round);
            let _ = path.set_line_join(PdfPageObjectLineJoin::Round);
            pg.objects_mut().add_path_object(path).map_err(err)?;
        }
        document.save_to_file(output).map_err(err)?;
        Ok(())
    })();
    for t in &temps {
        let _ = std::fs::remove_file(t);
    }
    res?;
    Ok(items.len())
}
