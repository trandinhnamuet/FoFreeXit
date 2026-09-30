//! Liên kết (link annotation): liệt kê theo trang (vùng + đích đã giải: trang
//! / URL / named dest), tạo mới (vùng → trang hoặc URL), xoá. Thao tác tầng
//! PDF object bằng lopdf. Toạ độ vùng trả/nhận theo "không gian hiển thị":
//! gốc dưới-trái của trang ĐÃ xoay /Rotate, trừ gốc CropBox — khớp với toạ độ
//! PDFium (page.width/height) mà viewer đang dùng cho chú thích.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::bookmarks::{
    load_doc, lopdf_err, make_dest, page_index_map, resolve_action, resolve_dest, write_outline_doc,
    Bookmark,
};
use crate::text::Rect;
use crate::EngineError;

/// Một liên kết đọc từ tài liệu.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkInfo {
    /// Định danh để xoá: "num gen" (annotation gián tiếp) hoặc "p{trang}:{vị trí}".
    pub id: String,
    pub page_index: u32,
    /// Vùng bấm (không gian hiển thị, điểm PDF).
    pub rect: Rect,
    /// Đích trong tài liệu (0-based).
    pub dest_page: Option<u32>,
    /// Toạ độ top của view đích, theo không gian hiển thị của trang đích.
    pub dest_top: Option<f32>,
    pub uri: Option<String>,
    /// Hành động khác (mở tệp, named action…) — chỉ để hiển thị.
    pub other: Option<String>,
}

/// Liên kết mới cần tạo.
#[derive(Debug, Clone, PartialEq)]
pub struct NewLink {
    pub page_index: u32,
    /// Vùng bấm (không gian hiển thị).
    pub rect: Rect,
    /// Đích trang (0-based) — dùng khi `uri` rỗng.
    pub dest_page: Option<u32>,
    /// Top của view đích (không gian hiển thị trang đích); None = đầu trang.
    pub dest_top: Option<f32>,
    pub uri: Option<String>,
    /// Viền hiện (màu RGB) — None = khung ẩn (mặc định như Foxit).
    pub border: Option<[u8; 3]>,
}

/// Hình học trang: hộp hiển thị (CropBox ∩ kế thừa) + góc xoay.
#[derive(Debug, Clone, Copy)]
struct Geom {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    rot: i64,
}

fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = Some(page);
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(doc.dereference(v).map(|(_, o)| o).unwrap_or(v));
        }
        cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

fn rect_of(o: &Object) -> Option<[f32; 4]> {
    let a = o.as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f32> = a.iter().filter_map(|x| x.as_float().ok()).collect();
    if v.len() != 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

fn geom(doc: &Document, page: ObjectId) -> Geom {
    let r = inherited(doc, page, b"CropBox")
        .and_then(rect_of)
        .or_else(|| inherited(doc, page, b"MediaBox").and_then(rect_of))
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let rot = inherited(doc, page, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    Geom { x0: r[0], y0: r[1], x1: r[2], y1: r[3], rot: ((rot % 360) + 360) % 360 }
}

impl Geom {
    /// user space → hiển thị.
    fn to_display(&self, x: f32, y: f32) -> (f32, f32) {
        match self.rot {
            90 => (y - self.y0, self.x1 - x),
            180 => (self.x1 - x, self.y1 - y),
            270 => (self.y1 - y, x - self.x0),
            _ => (x - self.x0, y - self.y0),
        }
    }
    /// hiển thị → user space.
    fn to_user(&self, xd: f32, yd: f32) -> (f32, f32) {
        match self.rot {
            90 => (self.x1 - yd, xd + self.y0),
            180 => (self.x1 - xd, self.y1 - yd),
            270 => (yd + self.x0, self.y1 - xd),
            _ => (xd + self.x0, yd + self.y0),
        }
    }
    fn rect_to_display(&self, r: [f32; 4]) -> Rect {
        let (ax, ay) = self.to_display(r[0], r[1]);
        let (bx, by) = self.to_display(r[2], r[3]);
        Rect { left: ax.min(bx), bottom: ay.min(by), right: ax.max(bx), top: ay.max(by) }
    }
    fn rect_to_user(&self, r: &Rect) -> [f32; 4] {
        let (ax, ay) = self.to_user(r.left, r.bottom);
        let (bx, by) = self.to_user(r.right, r.top);
        [ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)]
    }
    /// Top đích (user y, có thể kèm left) → toạ độ top hiển thị (chỉ nghĩa khi
    /// trục dọc hiển thị ứng với y của user space: xoay 0/180).
    fn top_to_display(&self, top: f32) -> Option<f32> {
        match self.rot {
            0 => Some(top - self.y0),
            180 => Some(self.y1 - top),
            _ => None,
        }
    }
    fn top_to_user(&self, top: f32) -> Option<f32> {
        match self.rot {
            0 => Some(top + self.y0),
            180 => Some(self.y1 - top),
            _ => None,
        }
    }
}

/// Danh sách /Annots (đã deref) của trang.
fn annots_of(doc: &Document, page: ObjectId) -> Vec<Object> {
    let Ok(d) = doc.get_dictionary(page) else { return Vec::new() };
    let Ok(a) = d.get(b"Annots") else { return Vec::new() };
    let a = doc.dereference(a).map(|(_, o)| o).unwrap_or(a);
    a.as_array().cloned().unwrap_or_default()
}

/// Liệt kê mọi liên kết của tài liệu (mọi trang), theo thứ tự trang.
pub fn list_links(input: &Path) -> Result<Vec<LinkInfo>, EngineError> {
    let doc = load_doc(input)?;
    Ok(list_links_doc(&doc))
}

fn list_links_doc(doc: &Document) -> Vec<LinkInfo> {
    let pages = page_index_map(doc);
    let by_index: HashMap<u32, ObjectId> = pages.iter().map(|(id, i)| (*i, *id)).collect();
    let mut out = Vec::new();
    let mut order: Vec<(u32, ObjectId)> = pages.iter().map(|(id, i)| (*i, *id)).collect();
    order.sort();
    for (pi, pid) in order {
        let g = geom(doc, pid);
        for (k, a) in annots_of(doc, pid).iter().enumerate() {
            let (id, dict) = match a {
                Object::Reference(r) => match doc.get_dictionary(*r) {
                    Ok(d) => (format!("{} {}", r.0, r.1), d),
                    Err(_) => continue,
                },
                Object::Dictionary(d) => (format!("p{pi}:{k}"), d),
                _ => continue,
            };
            if dict.get(b"Subtype").and_then(|o| o.as_name()).map(|n| n != b"Link").unwrap_or(true) {
                continue;
            }
            let Some(r) = dict.get(b"Rect").ok().and_then(rect_of) else { continue };
            let (dest, uri, other) = if let Ok(d) = dict.get(b"Dest") {
                (resolve_dest(doc, &pages, d), None, None)
            } else if let Ok(a) = dict.get(b"A") {
                resolve_action(doc, &pages, a)
            } else {
                (None, None, None)
            };
            let dest_page = dest.as_ref().and_then(|d| d.page);
            let dest_top = match (dest_page.and_then(|p| by_index.get(&p)), dest.as_ref().and_then(|d| d.top)) {
                (Some(tp), Some(top)) => geom(doc, *tp).top_to_display(top),
                _ => None,
            };
            let other = if dest.is_some() && dest_page.is_none() && uri.is_none() && other.is_none() {
                Some("GoTo".into())
            } else {
                other
            };
            out.push(LinkInfo { id, page_index: pi, rect: g.rect_to_display(r), dest_page, dest_top, uri, other });
        }
    }
    out
}

/// Xoá các liên kết `remove` (id từ `list_links`) và tạo `add` → `output`.
pub fn edit_links(input: &Path, output: &Path, remove: &[String], add: &[NewLink]) -> Result<(), EngineError> {
    save_outline_and_links(input, output, None, remove, add)
}

/// Lưu một lần: (tuỳ chọn) thay cây bookmark + xoá/tạo liên kết.
pub fn save_outline_and_links(
    input: &Path,
    output: &Path,
    tree: Option<&[Bookmark]>,
    remove: &[String],
    add: &[NewLink],
) -> Result<(), EngineError> {
    let mut doc = load_doc(input)?;
    apply_link_edits(&mut doc, remove, add)?;
    if let Some(t) = tree {
        write_outline_doc(&mut doc, t)?;
    }
    doc.save(output).map_err(|e| lopdf_err("lưu PDF", e))?;
    Ok(())
}

fn apply_link_edits(doc: &mut Document, remove: &[String], add: &[NewLink]) -> Result<(), EngineError> {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let remove: HashSet<&str> = remove.iter().map(|s| s.as_str()).collect();

    // ---- Xoá ----
    if !remove.is_empty() {
        for (pi, pid) in pages.iter().enumerate() {
            let annots = annots_of(doc, *pid);
            if annots.is_empty() {
                continue;
            }
            let mut dropped: Vec<ObjectId> = Vec::new();
            let kept: Vec<Object> = annots
                .iter()
                .enumerate()
                .filter(|(k, a)| {
                    let key = match a {
                        Object::Reference(r) => format!("{} {}", r.0, r.1),
                        _ => format!("p{pi}:{k}"),
                    };
                    let hit = remove.contains(key.as_str());
                    if hit {
                        if let Object::Reference(r) = a {
                            dropped.push(*r);
                        }
                    }
                    !hit
                })
                .map(|(_, a)| a.clone())
                .collect();
            if kept.len() == annots.len() {
                continue;
            }
            set_annots(doc, *pid, kept)?;
            for r in dropped {
                // Chỉ xoá object nếu đúng là Link (id do UI gửi có thể lệch sau khi tệp đổi).
                let is_link = doc
                    .get_dictionary(r)
                    .ok()
                    .and_then(|d| d.get(b"Subtype").ok())
                    .and_then(|o| o.as_name().ok())
                    .map(|n| n == b"Link")
                    .unwrap_or(false);
                if is_link {
                    doc.objects.remove(&r);
                }
            }
        }
    }

    // ---- Tạo ----
    for nl in add {
        let pid = *pages
            .get(nl.page_index as usize)
            .ok_or_else(|| EngineError::Pdfium(format!("trang {} không tồn tại", nl.page_index + 1)))?;
        let g = geom(doc, pid);
        let r = g.rect_to_user(&nl.rect);
        if r[2] - r[0] < 1.0 || r[3] - r[1] < 1.0 {
            return Err(EngineError::Pdfium("vùng liên kết quá nhỏ".into()));
        }
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Annot".to_vec()));
        d.set("Subtype", Object::Name(b"Link".to_vec()));
        d.set("Rect", Object::Array(r.iter().map(|v| Object::Real(*v)).collect()));
        d.set("P", Object::Reference(pid));
        d.set("F", Object::Integer(4)); // Print
        d.set("H", Object::Name(b"I".to_vec()));
        match nl.border {
            Some(c) => {
                d.set("Border", Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Integer(1)]));
                d.set("C", Object::Array(c.iter().map(|v| Object::Real(*v as f32 / 255.0)).collect()));
            }
            None => d.set("Border", Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Integer(0)])),
        }
        if let Some(uri) = nl.uri.as_ref().map(|u| u.trim()).filter(|u| !u.is_empty()) {
            let mut a = Dictionary::new();
            a.set("Type", Object::Name(b"Action".to_vec()));
            a.set("S", Object::Name(b"URI".to_vec()));
            a.set("URI", Object::String(uri.as_bytes().to_vec(), StringFormat::Literal));
            d.set("A", Object::Dictionary(a));
        } else if let Some(p) = nl.dest_page {
            let tp = *pages
                .get(p as usize)
                .ok_or_else(|| EngineError::Pdfium(format!("trang đích {} không tồn tại", p + 1)))?;
            let top = nl.dest_top.and_then(|t| geom(doc, tp).top_to_user(t));
            d.set("Dest", make_dest(tp, false, None, top, None));
        } else {
            return Err(EngineError::Pdfium("liên kết phải có trang đích hoặc URL".into()));
        }
        let aid = doc.add_object(Object::Dictionary(d));
        let mut annots = annots_of(doc, pid);
        annots.push(Object::Reference(aid));
        set_annots(doc, pid, annots)?;
    }
    Ok(())
}

/// Ghi lại /Annots của trang (giữ đúng chỗ nếu /Annots là mảng gián tiếp).
fn set_annots(doc: &mut Document, page: ObjectId, annots: Vec<Object>) -> Result<(), EngineError> {
    let indirect = doc
        .get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(aid) = indirect {
        if let Ok(o) = doc.get_object_mut(aid) {
            *o = Object::Array(annots);
            return Ok(());
        }
    }
    let d = doc.get_dictionary_mut(page).map_err(|e| lopdf_err("trang", e))?;
    if annots.is_empty() {
        d.remove(b"Annots");
    } else {
        d.set("Annots", Object::Array(annots));
    }
    Ok(())
}
