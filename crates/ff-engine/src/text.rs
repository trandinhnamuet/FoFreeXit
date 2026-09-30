//! Trích xuất & tìm kiếm text trên trang PDF.

use std::path::Path;

use pdfium_render::prelude::*;

use crate::EngineError;

/// Hình chữ nhật theo điểm PDF (gốc toạ độ ở góc dưới-trái trang).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

/// Một kết quả tìm kiếm.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub page_index: u16,
    /// Vị trí ký tự bắt đầu trong text của trang.
    pub char_start: usize,
    /// Số ký tự khớp.
    pub char_len: usize,
    /// Khung bao (hợp của các ký tự khớp). Có thể None nếu không lấy được bounds.
    pub rect: Option<Rect>,
}

/// Hộp bao một ký tự trên trang (cho text-layer / selection).
#[derive(Debug, Clone, PartialEq)]
pub struct CharBox {
    /// Ký tự (chuỗi để xử lý cả ký tự ngoài BMP / không có glyph).
    pub ch: String,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

/// Lấy hộp bao từng ký tự của một trang, theo thứ tự đọc (giống `extract_text`).
/// Dùng để dựng "text layer" cho phép chọn & copy text trên ảnh render.
pub fn page_char_boxes(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    password: Option<&str>,
) -> Result<Vec<CharBox>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
    let text = page
        .text()
        .map_err(|e| EngineError::Pdfium(format!("đọc text trang {page_index}: {e}")))?;

    let mut boxes: Vec<CharBox> = Vec::new();
    for ch in text.chars().iter() {
        let s = ch
            .unicode_char()
            .map(|c| c.to_string())
            .unwrap_or_default();

        // Ưu tiên loose_bounds (gồm advance/khoảng cách) cho ký tự như dấu cách.
        let bounds = ch.loose_bounds().or_else(|_| ch.tight_bounds()).ok();
        let bx = match bounds {
            Some(b) => CharBox {
                ch: s,
                left: b.left().value,
                bottom: b.bottom().value,
                right: b.right().value,
                top: b.top().value,
            },
            None => {
                // Không có hộp (vd. dấu cách ở vài file): suy ra hộp mảnh ngay sau
                // ký tự trước để giữ thứ tự & vị trí hợp lý cho selection.
                let prev = boxes.last();
                let (l, b, r, t) = match prev {
                    Some(p) => {
                        let h = (p.top - p.bottom).max(6.0);
                        (p.right, p.bottom, p.right + h * 0.3, p.top)
                    }
                    None => (0.0, 0.0, 0.0, 0.0),
                };
                CharBox { ch: s, left: l, bottom: b, right: r, top: t }
            }
        };
        boxes.push(bx);
    }
    Ok(boxes)
}

/// Trích toàn bộ text của một trang.
pub fn extract_text(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    password: Option<&str>,
) -> Result<String, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
    let text = page
        .text()
        .map_err(|e| EngineError::Pdfium(format!("đọc text trang {page_index}: {e}")))?;
    Ok(all_page_text(&text))
}

/// Toàn bộ text của trang, KHÔNG phụ thuộc hộp trang.
///
/// `PdfPageText::all()` của pdfium-render 0.8.37 chỉ lấy text trong hộp
/// `(0, 0, width, height)` (`page_size()`), trong khi toạ độ ký tự của PDFium
/// ở không gian người dùng CHƯA xoay: trang xoay 90/270 (width/height bị đổi
/// chỗ) hoặc có CropBox gốc khác (0,0) sẽ mất một phần/toàn bộ text. Dùng cùng
/// hàm PDFium (`FPDFText_GetBoundedText`) với một hộp rất lớn để giữ nguyên cách
/// chèn xuống dòng/dấu cách như `all()` trên trang thường.
fn all_page_text(text: &PdfPageText) -> String {
    const HUGE: f32 = 1.0e7;
    text.inside_rect(PdfRect::new_from_values(-HUGE, -HUGE, HUGE, HUGE))
}

/// Tìm `query` trong toàn tài liệu. `case_sensitive=false` -> không phân biệt hoa thường.
pub fn search(
    pdfium: &Pdfium,
    input: &Path,
    query: &str,
    case_sensitive: bool,
    password: Option<&str>,
) -> Result<Vec<SearchHit>, EngineError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let needle: Vec<char> = query
        .chars()
        .map(|c| fold(c, case_sensitive))
        .collect();

    let mut hits = Vec::new();
    for (pi, page) in document.pages().iter().enumerate() {
        let text = match page.text() {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Ghép theo TỪNG ký tự của text page (không lọc theo hộp trang — trang
        // xoay/crop vẫn tìm được); vị trí trong `hay` trùng đúng chỉ số ký tự
        // PDFium nên `char_range_rect` lấy đúng khung của đoạn khớp.
        let hay: Vec<char> = text
            .chars()
            .iter()
            .map(|ch| fold(ch.unicode_char().unwrap_or('\u{FFFD}'), case_sensitive))
            .collect();

        // Tìm tất cả vị trí khớp (sliding window đơn giản).
        if hay.len() < needle.len() {
            continue;
        }
        let mut start = 0usize;
        while start + needle.len() <= hay.len() {
            if hay[start..start + needle.len()] == needle[..] {
                let rect = char_range_rect(&text, start, needle.len());
                hits.push(SearchHit {
                    page_index: pi as u16,
                    char_start: start,
                    char_len: needle.len(),
                    rect,
                });
                start += needle.len();
            } else {
                start += 1;
            }
        }
    }
    Ok(hits)
}

fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        c
    } else {
        // Hạ thường đơn giản; với ASCII là đủ, Unicode dùng to_lowercase ký tự đầu.
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Khung bao hợp của các ký tự [start, start+len) trên trang.
fn char_range_rect(text: &PdfPageText, start: usize, len: usize) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for ch in text.chars().iter() {
        let idx = ch.index();
        if idx < start || idx >= start + len {
            continue;
        }
        if let Ok(b) = ch.tight_bounds() {
            let r = Rect {
                left: b.left().value,
                bottom: b.bottom().value,
                right: b.right().value,
                top: b.top().value,
            };
            acc = Some(match acc {
                None => r,
                Some(a) => Rect {
                    left: a.left.min(r.left),
                    bottom: a.bottom.min(r.bottom),
                    right: a.right.max(r.right),
                    top: a.top.max(r.top),
                },
            });
        }
    }
    acc
}
