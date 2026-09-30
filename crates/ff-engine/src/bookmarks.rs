//! Bookmarks (outline) mức Foxit: đọc ĐỦ cây (tiêu đề, trang/view đích,
//! kiểu chữ /F, màu /C, trạng thái mở /Count) và ghi LẠI cả cây mới ở tầng
//! PDF object bằng lopdf (/Outlines với /First /Last /Next /Prev /Parent
//! /Count, /Dest [page /XYZ l t z] hoặc [page /Fit], tiêu đề UTF-16BE cho
//! tiếng Việt). Kèm "tự tạo bookmark từ tiêu đề" (cỡ chữ lớn → cấp).
//!
//! Hàm đọc/giải đích ở đây (`resolve_dest`, `page_index_map`, `decode_text`)
//! dùng chung cho `links` và `split`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
use pdfium_render::prelude::*;

use crate::EngineError;

/// Một bookmark (nút cây outline).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bookmark {
    pub title: String,
    /// Trang đích 0-based (None = không có đích trong tài liệu, vd. URI).
    pub page_index: Option<u32>,
    /// Hành động mở URL thay cho đích trang (bookmark kiểu "liên kết web").
    pub uri: Option<String>,
    /// View đích /XYZ (toạ độ user space của trang). None = giữ nguyên (null).
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub zoom: Option<f32>,
    /// true = /Fit (vừa trang) thay cho /XYZ.
    pub fit: bool,
    pub bold: bool,
    pub italic: bool,
    /// Màu chữ /C (RGB 0..255). None = mặc định (đen).
    pub color: Option<[u8; 3]>,
    /// Nút có con đang mở (Count dương).
    pub open: bool,
    pub children: Vec<Bookmark>,
}

impl Bookmark {
    /// Bookmark trỏ tới đầu trang `page` (view /XYZ null null null).
    pub fn to_page(title: &str, page: u32) -> Self {
        Bookmark { title: title.to_string(), page_index: Some(page), ..Default::default() }
    }
}

pub(crate) fn lopdf_err(what: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Pdfium(format!("{what}: {e}"))
}

pub(crate) fn load_doc(input: &Path) -> Result<Document, EngineError> {
    let doc = Document::load(input).map_err(|e| lopdf_err("mở PDF (lopdf)", e))?;
    if doc.is_encrypted() {
        return Err(EngineError::Pdfium(
            "tài liệu đang mã hoá — hãy gỡ mật khẩu trước khi sửa bookmark/liên kết".into(),
        ));
    }
    Ok(doc)
}

/// ObjectId trang → index 0-based.
pub(crate) fn page_index_map(doc: &Document) -> HashMap<ObjectId, u32> {
    doc.get_pages().into_iter().map(|(n, id)| (id, n.saturating_sub(1))).collect()
}

/// Giải mã "text string" PDF: UTF-16BE (BOM FEFF), UTF-8 (BOM EFBBBF) hoặc
/// PDFDocEncoding. Bỏ ký tự điều khiển rác ở cuối (nhiều file có \0).
pub(crate) fn decode_text(obj: &Object) -> String {
    let bytes = match obj.as_str() {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let s = if bytes.starts_with(b"\xEF\xBB\xBF") {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        lopdf::decode_text_string(obj).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
    };
    s.trim_end_matches(|c: char| c == '\0').to_string()
}

/// Mã hoá text string: ASCII in được → literal; còn lại (tiếng Việt…) →
/// UTF-16BE có BOM, viết dạng hex cho an toàn.
pub(crate) fn encode_text(s: &str) -> Object {
    if s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut v = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(v, StringFormat::Hexadecimal)
    }
}

fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    doc.dereference(obj).map(|(_, o)| o).unwrap_or(obj)
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Đích đã giải: trang + view.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ResolvedDest {
    pub page: Option<u32>,
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub zoom: Option<f32>,
    pub fit: bool,
}

/// Tra named destination: /Root/Names/Dests (name tree) rồi /Root/Dests.
fn lookup_named(doc: &Document, name: &[u8]) -> Option<Object> {
    let cat = doc.catalog().ok()?;
    if let Ok(names) = cat.get(b"Names").map(|o| deref(doc, o)).and_then(|o| o.as_dict()) {
        if let Ok(tree) = names.get(b"Dests").map(|o| deref(doc, o)) {
            let mut seen = HashSet::new();
            if let Some(v) = name_tree_find(doc, tree, name, &mut seen, 0) {
                return Some(v);
            }
        }
    }
    if let Ok(dests) = cat.get(b"Dests").map(|o| deref(doc, o)).and_then(|o| o.as_dict()) {
        if let Ok(v) = dests.get(name) {
            return Some(deref(doc, v).clone());
        }
    }
    None
}

fn name_tree_find(doc: &Document, node: &Object, key: &[u8], seen: &mut HashSet<ObjectId>, depth: u32) -> Option<Object> {
    if depth > 64 {
        return None;
    }
    let dict = node.as_dict().ok()?;
    if let Ok(arr) = dict.get(b"Names").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
        let mut i = 0;
        while i + 1 < arr.len() {
            if let Ok(k) = deref(doc, &arr[i]).as_str() {
                if k == key {
                    return Some(deref(doc, &arr[i + 1]).clone());
                }
            }
            i += 2;
        }
    }
    if let Ok(kids) = dict.get(b"Kids").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
        for k in kids {
            if let Ok(id) = k.as_reference() {
                if !seen.insert(id) {
                    continue;
                }
            }
            if let Some(v) = name_tree_find(doc, deref(doc, k), key, seen, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// Giải một giá trị đích (/Dest hoặc /D của GoTo): mảng, tên, chuỗi, dict {/D}.
pub(crate) fn resolve_dest(doc: &Document, pages: &HashMap<ObjectId, u32>, dest: &Object) -> Option<ResolvedDest> {
    resolve_dest_depth(doc, pages, dest, 0)
}

fn resolve_dest_depth(doc: &Document, pages: &HashMap<ObjectId, u32>, dest: &Object, depth: u32) -> Option<ResolvedDest> {
    if depth > 8 {
        return None;
    }
    let dest = deref(doc, dest);
    match dest {
        Object::Array(arr) => {
            let first = arr.first()?;
            let page = match first {
                Object::Reference(id) => pages.get(id).copied(),
                // File lỗi/đích từ xa ghi số trang thay cho tham chiếu.
                Object::Integer(i) if *i >= 0 => Some(*i as u32),
                _ => None,
            };
            let kind = arr.get(1).and_then(|o| o.as_name().ok()).unwrap_or(b"XYZ");
            let a = |i: usize| arr.get(i).and_then(num);
            let mut r = ResolvedDest { page, ..Default::default() };
            match kind {
                b"XYZ" => {
                    r.left = a(2);
                    r.top = a(3);
                    r.zoom = a(4).filter(|z| *z > 0.0);
                }
                b"FitH" | b"FitBH" => r.top = a(2),
                b"FitV" | b"FitBV" => r.left = a(2),
                b"FitR" => {
                    r.left = a(2);
                    r.top = a(5);
                }
                _ => r.fit = true,
            }
            Some(r)
        }
        Object::Name(n) => lookup_named(doc, n).and_then(|v| resolve_dest_depth(doc, pages, &v, depth + 1)),
        Object::String(s, _) => lookup_named(doc, s).and_then(|v| resolve_dest_depth(doc, pages, &v, depth + 1)),
        Object::Dictionary(d) => d.get(b"D").ok().and_then(|v| resolve_dest_depth(doc, pages, v, depth + 1)),
        _ => None,
    }
}

/// Giải /A (action) của outline/link → (đích trang, URI, mô tả hành động khác).
pub(crate) fn resolve_action(
    doc: &Document,
    pages: &HashMap<ObjectId, u32>,
    action: &Object,
) -> (Option<ResolvedDest>, Option<String>, Option<String>) {
    let Ok(a) = deref(doc, action).as_dict() else { return (None, None, None) };
    let s = a.get(b"S").and_then(|o| o.as_name()).unwrap_or(b"");
    match s {
        b"GoTo" => (a.get(b"D").ok().and_then(|d| resolve_dest(doc, pages, d)), None, None),
        b"URI" => {
            let uri = a.get(b"URI").map(|o| deref(doc, o)).map(|o| {
                // URI là chuỗi ASCII 7-bit theo spec, nhưng file thực tế có cả UTF-8.
                o.as_str().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
            });
            (None, uri.ok().filter(|u| !u.is_empty()), None)
        }
        b"GoToR" | b"Launch" => {
            let f = a.get(b"F").map(|o| deref(doc, o)).ok().map(|o| match o {
                Object::Dictionary(d) => d
                    .get(b"UF")
                    .or_else(|_| d.get(b"F"))
                    .map(decode_text)
                    .unwrap_or_default(),
                other => decode_text(other),
            });
            (None, None, Some(format!("{}: {}", String::from_utf8_lossy(s), f.unwrap_or_default())))
        }
        b"Named" => {
            // NextPage/PrevPage/FirstPage/LastPage — hành động điều hướng của viewer.
            let n = a.get(b"N").and_then(|o| o.as_name()).map(|n| String::from_utf8_lossy(n).into_owned());
            (None, None, Some(format!("Named: {}", n.unwrap_or_default())))
        }
        other => (None, None, Some(String::from_utf8_lossy(other).into_owned())),
    }
}

// ---------------- Đọc cây ----------------

/// Đọc toàn bộ cây bookmark của `input`.
pub fn get_outline(input: &Path) -> Result<Vec<Bookmark>, EngineError> {
    let doc = load_doc(input)?;
    Ok(read_outline_doc(&doc))
}

pub(crate) fn read_outline_doc(doc: &Document) -> Vec<Bookmark> {
    let pages = page_index_map(doc);
    let Ok(cat) = doc.catalog() else { return Vec::new() };
    let Ok(root) = cat.get(b"Outlines").map(|o| deref(doc, o)).and_then(|o| o.as_dict()) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    read_siblings(doc, &pages, root.get(b"First").ok(), &mut seen, 0)
}

fn read_siblings(
    doc: &Document,
    pages: &HashMap<ObjectId, u32>,
    first: Option<&Object>,
    seen: &mut HashSet<ObjectId>,
    depth: u32,
) -> Vec<Bookmark> {
    let mut out = Vec::new();
    if depth > 64 {
        return out;
    }
    let mut cur = first.and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        // Chống vòng lặp /Next (file hỏng) và nút dùng lại.
        if !seen.insert(id) || out.len() > 100_000 {
            break;
        }
        let Ok(d) = doc.get_dictionary(id) else { break };
        out.push(read_item(doc, pages, d, seen, depth));
        cur = d.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
    out
}

fn read_item(doc: &Document, pages: &HashMap<ObjectId, u32>, d: &Dictionary, seen: &mut HashSet<ObjectId>, depth: u32) -> Bookmark {
    let title = d.get(b"Title").map(|o| decode_text(deref(doc, o))).unwrap_or_default();
    let mut bm = Bookmark { title, ..Default::default() };
    let dest = if let Ok(dst) = d.get(b"Dest") {
        resolve_dest(doc, pages, dst)
    } else if let Ok(a) = d.get(b"A") {
        let (dst, uri, _) = resolve_action(doc, pages, a);
        bm.uri = uri;
        dst
    } else {
        None
    };
    if let Some(r) = dest {
        bm.page_index = r.page;
        bm.left = r.left;
        bm.top = r.top;
        bm.zoom = r.zoom;
        bm.fit = r.fit;
    }
    let flags = d.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
    bm.italic = flags & 1 != 0;
    bm.bold = flags & 2 != 0;
    if let Ok(c) = d.get(b"C").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
        if c.len() == 3 {
            let v: Vec<f32> = c.iter().map(|o| num(o).unwrap_or(0.0).clamp(0.0, 1.0)).collect();
            let rgb = [(v[0] * 255.0).round() as u8, (v[1] * 255.0).round() as u8, (v[2] * 255.0).round() as u8];
            if rgb != [0, 0, 0] {
                bm.color = Some(rgb);
            }
        }
    }
    bm.open = d.get(b"Count").ok().and_then(|o| o.as_i64().ok()).map(|c| c > 0).unwrap_or(false);
    bm.children = read_siblings(doc, pages, d.get(b"First").ok(), seen, depth + 1);
    bm
}

// ---------------- Ghi cây ----------------

/// Ghi `tree` làm TOÀN BỘ outline mới của `input` → `output` (cây rỗng = xoá
/// outline). Object outline cũ bị gỡ khỏi file. `output` có thể trùng `input`.
pub fn set_outline(input: &Path, output: &Path, tree: &[Bookmark]) -> Result<(), EngineError> {
    let mut doc = load_doc(input)?;
    write_outline_doc(&mut doc, tree)?;
    doc.save(output).map_err(|e| lopdf_err("lưu PDF", e))?;
    Ok(())
}

/// Thu mọi object thuộc cây outline cũ (để xoá).
fn collect_outline_ids(doc: &Document, first: Option<ObjectId>, acc: &mut HashSet<ObjectId>, depth: u32) {
    if depth > 64 {
        return;
    }
    let mut cur = first;
    while let Some(id) = cur {
        if !acc.insert(id) {
            break;
        }
        let Ok(d) = doc.get_dictionary(id) else { break };
        // Action gián tiếp riêng của nút (không dùng chung) cũng bỏ theo.
        if let Ok(a) = d.get(b"A").and_then(|o| o.as_reference()) {
            acc.insert(a);
        }
        collect_outline_ids(doc, d.get(b"First").ok().and_then(|o| o.as_reference().ok()), acc, depth + 1);
        cur = d.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
}

pub(crate) fn write_outline_doc(doc: &mut Document, tree: &[Bookmark]) -> Result<(), EngineError> {
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    // Gỡ cây cũ.
    let old_root = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(rid) = old_root {
        let first = doc.get_dictionary(rid).ok().and_then(|d| d.get(b"First").ok()).and_then(|o| o.as_reference().ok());
        let mut ids = HashSet::new();
        collect_outline_ids(doc, first, &mut ids, 0);
        // Không xoá action nếu object khác (link annotation…) cũng trỏ tới —
        // hiếm, nhưng xoá nhầm làm hỏng link. Chỉ giữ lại action dạng dict /S.
        for id in ids {
            let is_action = doc
                .get_dictionary(id)
                .map(|d| d.has(b"S") && !d.has(b"Title"))
                .unwrap_or(false);
            if !is_action {
                doc.objects.remove(&id);
            }
        }
        doc.objects.remove(&rid);
    }
    let catalog = doc.catalog_mut().map_err(|e| lopdf_err("catalog", e))?;
    catalog.remove(b"Outlines");
    if tree.is_empty() {
        if catalog.get(b"PageMode").and_then(|o| o.as_name()).map(|n| n == b"UseOutlines").unwrap_or(false) {
            catalog.remove(b"PageMode");
        }
        return Ok(());
    }

    let root_id = doc.new_object_id();
    let (first, last, count) = write_level(doc, tree, root_id, &page_ids)?;
    let mut root = Dictionary::new();
    root.set("Type", Object::Name(b"Outlines".to_vec()));
    root.set("First", Object::Reference(first));
    root.set("Last", Object::Reference(last));
    root.set("Count", Object::Integer(count));
    doc.objects.insert(root_id, Object::Dictionary(root));
    doc.catalog_mut()
        .map_err(|e| lopdf_err("catalog", e))?
        .set("Outlines", Object::Reference(root_id));
    Ok(())
}

/// Ghi một cấp anh em dưới `parent`; trả (first, last, số nút HIỂN THỊ tính
/// cho /Count của cha nếu cha mở).
fn write_level(
    doc: &mut Document,
    items: &[Bookmark],
    parent: ObjectId,
    page_ids: &[ObjectId],
) -> Result<(ObjectId, ObjectId, i64), EngineError> {
    let ids: Vec<ObjectId> = items.iter().map(|_| doc.new_object_id()).collect();
    let mut visible = 0i64;
    for (i, it) in items.iter().enumerate() {
        let mut d = Dictionary::new();
        let title = if it.title.trim().is_empty() { "Untitled" } else { it.title.as_str() };
        d.set("Title", encode_text(title));
        d.set("Parent", Object::Reference(parent));
        if i > 0 {
            d.set("Prev", Object::Reference(ids[i - 1]));
        }
        if i + 1 < ids.len() {
            d.set("Next", Object::Reference(ids[i + 1]));
        }
        if let Some(uri) = it.uri.as_ref().filter(|u| !u.trim().is_empty()) {
            let mut a = Dictionary::new();
            a.set("S", Object::Name(b"URI".to_vec()));
            a.set("URI", Object::String(uri.trim().as_bytes().to_vec(), StringFormat::Literal));
            d.set("A", Object::Dictionary(a));
        } else if let Some(p) = it.page_index {
            let pid = *page_ids.get(p as usize).ok_or_else(|| {
                EngineError::Pdfium(format!("bookmark \"{}\" trỏ tới trang {} không tồn tại", it.title, p + 1))
            })?;
            d.set("Dest", make_dest(pid, it.fit, it.left, it.top, it.zoom));
        }
        let flags = (it.italic as i64) | ((it.bold as i64) << 1);
        if flags != 0 {
            d.set("F", Object::Integer(flags));
        }
        if let Some(c) = it.color.filter(|c| *c != [0, 0, 0]) {
            d.set(
                "C",
                Object::Array(c.iter().map(|v| Object::Real(*v as f32 / 255.0)).collect()),
            );
        }
        visible += 1;
        if !it.children.is_empty() {
            let (f, l, n) = write_level(doc, &it.children, ids[i], page_ids)?;
            d.set("First", Object::Reference(f));
            d.set("Last", Object::Reference(l));
            // Mở: Count = số hậu duệ hiển thị; đóng: âm số đó.
            d.set("Count", Object::Integer(if it.open { n } else { -n }));
            if it.open {
                visible += n;
            }
        }
        doc.objects.insert(ids[i], Object::Dictionary(d));
    }
    Ok((ids[0], *ids.last().unwrap(), visible))
}

pub(crate) fn make_dest(page: ObjectId, fit: bool, left: Option<f32>, top: Option<f32>, zoom: Option<f32>) -> Object {
    let opt = |v: Option<f32>| v.map(Object::Real).unwrap_or(Object::Null);
    if fit {
        Object::Array(vec![Object::Reference(page), Object::Name(b"Fit".to_vec())])
    } else {
        Object::Array(vec![
            Object::Reference(page),
            Object::Name(b"XYZ".to_vec()),
            opt(left),
            opt(top),
            opt(zoom.filter(|z| *z > 0.0)),
        ])
    }
}

// ---------------- Tự tạo bookmark từ tiêu đề ----------------

/// Một dòng chữ trên trang (gom từ ký tự cùng baseline).
struct Line {
    text: String,
    size: f32,
    top: f32,
    bottom: f32,
    left: f32,
    bold: bool,
}

fn page_lines(page: &PdfPage) -> Vec<Line> {
    let Ok(text) = page.text() else { return Vec::new() };
    let mut lines: Vec<Line> = Vec::new();
    let mut cur: Option<Line> = None;
    let mut last_right = f32::MIN;
    for ch in text.chars().iter() {
        let c = ch.unicode_char().unwrap_or(' ');
        if c == '\r' || c == '\n' {
            if let Some(l) = cur.take() {
                lines.push(l);
            }
            continue;
        }
        let Ok(b) = ch.loose_bounds().or_else(|_| ch.tight_bounds()) else { continue };
        let (bt, bb, bl, br) = (b.top().value, b.bottom().value, b.left().value, b.right().value);
        let size = match ch.scaled_font_size().value {
            v if v > 0.5 && v < 500.0 => v,
            _ => (bt - bb).abs(),
        };
        let bold = ch.font_weight().map(|w| matches!(w,
            PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900
        )).unwrap_or(false) || ch.font_name().to_ascii_lowercase().contains("bold");
        let new_line = match &cur {
            None => true,
            Some(l) => {
                let h = (l.top - l.bottom).max(1.0);
                // Khác dòng: baseline lệch quá nửa chiều cao, hoặc nhảy lùi sang trái xa.
                (bb - l.bottom).abs() > 0.5 * h || bl < last_right - 3.0 * h
            }
        };
        if new_line {
            if let Some(l) = cur.take() {
                lines.push(l);
            }
            cur = Some(Line { text: String::new(), size, top: bt, bottom: bb, left: bl, bold });
        }
        let l = cur.as_mut().unwrap();
        l.text.push(c);
        if !c.is_whitespace() {
            l.size = l.size.max(size);
            l.bold &= bold;
        }
        l.top = l.top.max(bt);
        l.bottom = l.bottom.min(bb);
        l.left = l.left.min(bl);
        last_right = br;
    }
    if let Some(l) = cur.take() {
        lines.push(l);
    }
    for l in &mut lines {
        l.text = l.text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    lines.retain(|l| !l.text.is_empty());
    lines
}

/// Tự tạo cây bookmark từ tiêu đề (dòng có cỡ chữ lớn hơn chữ thân bài rõ rệt;
/// các cỡ lớn khác nhau → cấp 1/2/3). `max_levels` 1..=4. Dòng tiêu đề liền kề
/// cùng cỡ (tiêu đề xuống dòng) được gộp. Không ghi file — UI xem/chỉnh trước.
pub fn auto_bookmarks_from_headings(
    pdfium: &Pdfium,
    input: &Path,
    max_levels: u32,
    password: Option<&str>,
) -> Result<Vec<Bookmark>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let max_levels = max_levels.clamp(1, 4) as usize;
    let mut all: Vec<(u32, Line)> = Vec::new();
    // Cỡ chữ thân bài = cỡ có NHIỀU KÝ TỰ nhất (làm tròn 0.5pt).
    let mut hist: HashMap<i32, usize> = HashMap::new();
    for (pi, page) in document.pages().iter().enumerate() {
        for l in page_lines(&page) {
            *hist.entry((l.size * 2.0).round() as i32).or_default() += l.text.chars().count();
            all.push((pi as u32, l));
        }
    }
    let body = hist.iter().max_by_key(|(_, n)| **n).map(|(k, _)| *k as f32 / 2.0).unwrap_or(10.0);

    let is_heading = |l: &Line| {
        let n = l.text.chars().count();
        let letters = l.text.chars().filter(|c| c.is_alphabetic()).count();
        n >= 2 && n <= 150 && letters >= 2 && l.size >= body * 1.15 + 0.3
            // Số trang / chú thích lớn đứng một mình không phải tiêu đề.
            && !l.text.chars().all(|c| c.is_ascii_digit() || c.is_whitespace() || c == '.')
    };
    // Gộp tiêu đề nhiều dòng: dòng kế tiếp cùng trang, cùng cỡ, sát bên dưới.
    let mut heads: Vec<(u32, Line)> = Vec::new();
    for (p, l) in all.into_iter() {
        if !is_heading(&l) {
            continue;
        }
        if let Some((lp, last)) = heads.last_mut() {
            let gap = last.bottom - l.top;
            if *lp == p && (last.size - l.size).abs() < 0.6 && gap >= -1.0 && gap < l.size * 0.9 {
                last.text.push(' ');
                last.text.push_str(&l.text);
                last.bottom = l.bottom;
                continue;
            }
        }
        heads.push((p, l));
    }
    // Cấp theo cỡ chữ: các cỡ phân biệt (≥ 1pt) giảm dần; dư thì dồn vào cấp cuối.
    let mut sizes: Vec<f32> = Vec::new();
    let mut s_sorted: Vec<f32> = heads.iter().map(|(_, l)| l.size).collect();
    s_sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
    for s in s_sorted {
        if sizes.last().map(|x| x - s >= 1.0).unwrap_or(true) {
            sizes.push(s);
        }
    }
    let level_of = |s: f32| -> usize {
        let i = sizes.iter().position(|x| s >= x - 0.99).unwrap_or(sizes.len().saturating_sub(1));
        i.min(max_levels - 1)
    };
    let mut roots: Vec<Bookmark> = Vec::new();
    for (p, l) in heads {
        let lvl = level_of(l.size);
        let bm = Bookmark {
            title: l.text.chars().take(200).collect(),
            page_index: Some(p),
            left: None,
            top: Some(l.top + 2.0),
            bold: lvl == 0 && l.bold,
            open: true,
            ..Default::default()
        };
        // Chèn vào nhánh cuối ở độ sâu lvl (thiếu cấp cha thì gắn vào cấp gần nhất).
        let mut list = &mut roots;
        for _ in 0..lvl {
            if list.is_empty() {
                break;
            }
            let last = list.len() - 1;
            list = &mut list[last].children;
        }
        list.push(bm);
    }
    Ok(roots)
}
