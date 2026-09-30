//! Tách tệp kiểu Foxit ngoài "tối đa N trang/tệp" (`organize::split_by_page_count`):
//! theo dải trang ("1-3, 4-10, 11-"), theo bookmark cấp cao nhất (mỗi bookmark
//! một tệp, đặt tên theo tiêu đề, giữ bookmark con), theo dung lượng (xấp xỉ).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId};
use pdfium_render::prelude::*;

use crate::bookmarks::{read_outline_doc, set_outline, Bookmark};
use crate::EngineError;

/// Parse chuỗi dải trang kiểu người dùng (1-based): "1-3, 4-10, 11-", "5",
/// "-3" (= 1-3), "7-" (= 7 đến hết). Mỗi phần tử = MỘT tệp đầu ra.
/// Trả dải 0-based [đầu, cuối] (bao gồm cả hai đầu).
pub fn parse_page_ranges(spec: &str, page_count: u16) -> Result<Vec<(u16, u16)>, EngineError> {
    let bad = |p: &str| EngineError::Pdfium(format!("dải trang không hợp lệ: \"{p}\""));
    let mut out = Vec::new();
    for part in spec.split([',', ';']) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let (a, b) = match p.split_once('-') {
            Some((a, b)) => {
                let a = a.trim();
                let b = b.trim();
                let a = if a.is_empty() { 1 } else { a.parse::<u32>().map_err(|_| bad(p))? };
                let b = if b.is_empty() { page_count as u32 } else { b.parse::<u32>().map_err(|_| bad(p))? };
                (a, b)
            }
            None => {
                let n = p.parse::<u32>().map_err(|_| bad(p))?;
                (n, n)
            }
        };
        if a == 0 || b == 0 || a > b {
            return Err(bad(p));
        }
        if a > page_count as u32 {
            return Err(EngineError::Pdfium(format!(
                "dải \"{p}\" vượt quá số trang ({page_count})"
            )));
        }
        let b = b.min(page_count as u32);
        out.push(((a - 1) as u16, (b - 1) as u16));
    }
    if out.is_empty() {
        return Err(EngineError::Pdfium("chưa nhập dải trang nào".into()));
    }
    Ok(out)
}

/// Tên tệp an toàn trên Windows/Linux từ tiêu đề bookmark.
pub fn sanitize_file_name(title: &str) -> String {
    let mut s: String = title
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = s.chars().take(120).collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let upper = s.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or("");
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem) {
        s.insert(0, '_');
    }
    s
}

/// Ghi từng phần (dải trang 0-based bao gồm) ra `out_dir/<tên>.pdf`.
/// `outlines[i]` (nếu có) là cây bookmark ghi vào phần i (trang đã dời về gốc 0).
fn write_parts(
    pdfium: &Pdfium,
    input: &Path,
    password: Option<&str>,
    parts: &[(u16, u16, String)],
    outlines: Option<&[Vec<Bookmark>]>,
    out_dir: &Path,
) -> Result<Vec<PathBuf>, EngineError> {
    let src = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(format!("mở {}: {e}", input.display())))?;
    let count = src.pages().len();
    let mut used: HashSet<String> = HashSet::new();
    let mut outputs = Vec::new();
    for (pi, (a, b, name)) in parts.iter().enumerate() {
        if *a > *b || *b >= count {
            return Err(EngineError::Pdfium(format!("dải trang {}-{} không hợp lệ", a + 1, b + 1)));
        }
        let mut dest = pdfium
            .create_new_pdf()
            .map_err(|e| EngineError::Pdfium(format!("tạo tài liệu mới: {e}")))?;
        for (k, i) in (*a..=*b).enumerate() {
            dest.pages_mut()
                .copy_page_from_document(&src, i, k as u16)
                .map_err(|e| EngineError::Pdfium(format!("copy trang {}: {e}", i + 1)))?;
        }
        // Trùng tên (2 bookmark cùng tiêu đề) → "Tên (2)".
        let mut fname = name.clone();
        let mut n = 2;
        while !used.insert(fname.to_lowercase()) {
            fname = format!("{name} ({n})");
            n += 1;
        }
        let out = out_dir.join(format!("{fname}.pdf"));
        dest.save_to_file(&out)
            .map_err(|e| EngineError::Pdfium(format!("lưu {}: {e}", out.display())))?;
        if let Some(tree) = outlines.and_then(|o| o.get(pi)).filter(|t| !t.is_empty()) {
            set_outline(&out, &out, tree)?;
        }
        outputs.push(out);
    }
    Ok(outputs)
}

/// Tách theo các dải trang (0-based, bao gồm). Tên: `{base}_part{n}.pdf`.
pub fn split_by_ranges(
    pdfium: &Pdfium,
    input: &Path,
    ranges: &[(u16, u16)],
    out_dir: &Path,
    base_name: &str,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    if ranges.is_empty() {
        return Err(EngineError::Pdfium("chưa có dải trang nào".into()));
    }
    let parts: Vec<(u16, u16, String)> = ranges
        .iter()
        .enumerate()
        .map(|(i, (a, b))| (*a, *b, format!("{base_name}_part{}", i + 1)))
        .collect();
    write_parts(pdfium, input, password, &parts, None, out_dir)
}

/// Dời cây bookmark về phần [a, b]: trang - a; đích ngoài phần thì bỏ đích.
fn shift_tree(items: &[Bookmark], a: u16, b: u16) -> Vec<Bookmark> {
    items
        .iter()
        .map(|it| {
            let mut n = it.clone();
            n.page_index = it
                .page_index
                .filter(|p| *p >= a as u32 && *p <= b as u32)
                .map(|p| p - a as u32);
            n.children = shift_tree(&it.children, a, b);
            n
        })
        .collect()
}

/// Tách theo bookmark cấp cao nhất: mỗi bookmark (có trang đích) một tệp,
/// từ trang của nó tới trước trang của bookmark kế tiếp; các trang trước
/// bookmark đầu tiên gộp vào tệp đầu. Tên tệp = tiêu đề (đã làm sạch); mỗi tệp
/// giữ bookmark đó cùng các bookmark con.
pub fn split_by_bookmarks(
    pdfium: &Pdfium,
    input: &Path,
    out_dir: &Path,
    base_name: &str,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    let doc = Document::load(input).map_err(|e| EngineError::Pdfium(format!("đọc bookmark: {e}")))?;
    let count = doc.get_pages().len() as u32;
    let tree = read_outline_doc(&doc);
    drop(doc);
    let mut tops: Vec<(u32, &Bookmark)> = tree
        .iter()
        .filter_map(|b| b.page_index.filter(|p| *p < count).map(|p| (p, b)))
        .collect();
    tops.sort_by_key(|(p, _)| *p);
    // Hai bookmark cùng trang bắt đầu → chỉ tách một lần (giữ cái đầu).
    tops.dedup_by_key(|(p, _)| *p);
    if tops.is_empty() {
        return Err(EngineError::Pdfium("tài liệu không có bookmark cấp cao nhất trỏ tới trang nào".into()));
    }
    let mut parts = Vec::new();
    let mut outlines = Vec::new();
    for (i, (p, bm)) in tops.iter().enumerate() {
        let a = if i == 0 { 0 } else { *p } as u16;
        let b = tops.get(i + 1).map(|(n, _)| n - 1).unwrap_or(count - 1) as u16;
        let mut name = sanitize_file_name(&bm.title);
        if name.is_empty() {
            name = format!("{base_name}_part{}", i + 1);
        }
        parts.push((a, b, name));
        outlines.push(shift_tree(std::slice::from_ref(*bm), a, b));
    }
    write_parts(pdfium, input, password, &parts, Some(&outlines), out_dir)
}

/// Kích thước ước lượng (byte) của object khi ghi ra.
fn obj_size(o: &Object) -> usize {
    match o {
        Object::Stream(s) => s.content.len() + 40 + s.dict.len() * 16,
        Object::String(v, _) => v.len() + 2,
        Object::Array(a) => 2 + a.iter().map(obj_size).sum::<usize>(),
        Object::Dictionary(d) => 4 + d.iter().map(|(k, v)| k.len() + 2 + obj_size(v)).sum::<usize>(),
        _ => 8,
    }
}

/// Tập object (id → byte ước lượng) mà trang cần: nội dung, tài nguyên,
/// annotation… — không đi sang trang/cây trang khác.
fn page_closure(doc: &Document, page: ObjectId) -> HashMap<ObjectId, usize> {
    let mut acc: HashMap<ObjectId, usize> = HashMap::new();
    let mut stack: Vec<ObjectId> = vec![page];
    while let Some(id) = stack.pop() {
        if acc.contains_key(&id) {
            continue;
        }
        let Ok(o) = doc.get_object(id) else { continue };
        if id != page {
            if let Ok(d) = o.as_dict() {
                if d.has_type(b"Page") || d.has_type(b"Pages") {
                    continue;
                }
            }
        }
        acc.insert(id, obj_size(o) + 20);
        let mut refs = Vec::new();
        collect_refs(o, id == page, &mut refs);
        stack.extend(refs);
    }
    acc
}

fn collect_refs(o: &Object, is_page: bool, out: &mut Vec<ObjectId>) {
    match o {
        Object::Reference(r) => out.push(*r),
        Object::Array(a) => a.iter().for_each(|x| collect_refs(x, false, out)),
        Object::Dictionary(d) => d.iter().for_each(|(k, v)| {
            if !(is_page && k == b"Parent") && k != b"P" && k != b"Parent" {
                collect_refs(v, false, out)
            }
        }),
        Object::Stream(s) => s.dict.iter().for_each(|(_, v)| collect_refs(v, false, out)),
        _ => {}
    }
}

/// Chia trang theo dung lượng tối đa mỗi tệp (xấp xỉ): cộng dồn kích thước
/// object mà mỗi trang cần (object dùng chung như font chỉ tính 1 lần/tệp);
/// vượt ngưỡng thì sang tệp mới. Một trang lớn hơn ngưỡng vẫn thành một tệp.
pub fn plan_split_by_size(input: &Path, max_bytes: u64) -> Result<Vec<(u16, u16)>, EngineError> {
    if max_bytes == 0 {
        return Err(EngineError::Pdfium("dung lượng tối đa phải > 0".into()));
    }
    let doc = Document::load(input).map_err(|e| EngineError::Pdfium(format!("đọc tệp: {e}")))?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    const OVERHEAD: u64 = 2048;
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut used: HashSet<ObjectId> = HashSet::new();
    let mut total = OVERHEAD;
    for (i, pid) in pages.iter().enumerate() {
        let clo = page_closure(&doc, *pid);
        let add: u64 = clo.iter().filter(|(id, _)| !used.contains(id)).map(|(_, s)| *s as u64).sum();
        if i > start && total + add > max_bytes {
            ranges.push((start as u16, (i - 1) as u16));
            start = i;
            used.clear();
            total = OVERHEAD + clo.values().map(|s| *s as u64).sum::<u64>();
        } else {
            total += add;
        }
        used.extend(clo.keys().copied());
    }
    if start < pages.len() {
        ranges.push((start as u16, (pages.len() - 1) as u16));
    }
    Ok(ranges)
}

/// Tách theo dung lượng (xấp xỉ `max_bytes` mỗi tệp). Tên: `{base}_part{n}.pdf`.
pub fn split_by_size(
    pdfium: &Pdfium,
    input: &Path,
    max_bytes: u64,
    out_dir: &Path,
    base_name: &str,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    let ranges = plan_split_by_size(input, max_bytes)?;
    split_by_ranges(pdfium, input, &ranges, out_dir, base_name, password)
}
