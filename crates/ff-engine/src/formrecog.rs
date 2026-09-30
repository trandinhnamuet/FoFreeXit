//! Nhận diện field tự động (như Foxit "Run Form Field Recognition"):
//! heuristic trên text + đồ hoạ vector của trang.
//!
//! - Chuỗi gạch dưới `____` / dấu chấm dẫn `.....` / `…` → ô text trên đoạn đó.
//! - Ký tự ô vuông `☐ □ ▢ ❏` (kể cả mã Wingdings PUA) → checkbox.
//! - Hình chữ nhật vector nhỏ gần vuông, trống → checkbox; hình chữ nhật dài
//!   trống → ô text; đường kẻ ngang trống phía trên → ô text trên đường kẻ.
//! - Nhãn bên trái (hoặc phía trên / bên phải với checkbox) thành tên + tooltip.
//!
//! Kết quả là ĐỀ XUẤT — UI cho người dùng giữ/bỏ trước khi tạo field.

use std::path::Path;

use pdfium_render::prelude::*;

use crate::EngineError;

/// 1 field đề xuất.
#[derive(Clone, Debug)]
pub struct FieldProposal {
    pub page_index: u16,
    /// "text" | "checkbox"
    pub kind: String,
    /// [left, bottom, right, top]
    pub rect: [f32; 4],
    pub name: String,
    pub tooltip: String,
}

#[derive(Clone, Debug)]
struct Ch {
    c: char,
    l: f32,
    b: f32,
    r: f32,
    t: f32,
}

impl Ch {
    fn cy(&self) -> f32 {
        (self.b + self.t) / 2.0
    }
    fn cx(&self) -> f32 {
        (self.l + self.r) / 2.0
    }
    fn h(&self) -> f32 {
        self.t - self.b
    }
}

const BOX_CHARS: &[char] = &[
    '\u{2610}', '\u{2611}', '\u{2612}', '\u{25A1}', '\u{25A2}', '\u{25FB}', '\u{25FD}', '\u{274F}', '\u{2750}',
    '\u{2751}', '\u{2752}', '\u{F0A8}', '\u{F06F}', '\u{F071}', '\u{F0FE}',
];

type Mat = [f32; 6];
const ID: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mat_of(m: &PdfMatrix) -> Mat {
    [m.a(), m.b(), m.c(), m.d(), m.e(), m.f()]
}
/// m1 rồi m2 (điểm * m1 * m2).
fn mul(m1: Mat, m2: Mat) -> Mat {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}
fn apply(m: Mat, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Hình học lấy từ path: hình chữ nhật (trục thẳng) và đường kẻ ngang.
#[derive(Default)]
struct Shapes {
    rects: Vec<[f32; 4]>,
    hlines: Vec<[f32; 3]>, // x0, x1, y
}

fn collect_path(path: &PdfPagePathObject, m: Mat, out: &mut Shapes) {
    let mut sub: Vec<(f32, f32)> = Vec::new();
    let mut has_curve = false;
    let flush = |sub: &mut Vec<(f32, f32)>, has_curve: &mut bool, out: &mut Shapes| {
        if sub.len() >= 2 && !*has_curve {
            classify_subpath(sub, out);
        }
        sub.clear();
        *has_curve = false;
    };
    for seg in path.segments().iter() {
        let (x, y) = apply(m, seg.x().value, seg.y().value);
        match seg.segment_type() {
            PdfPathSegmentType::MoveTo => {
                flush(&mut sub, &mut has_curve, out);
                sub.push((x, y));
            }
            PdfPathSegmentType::LineTo => sub.push((x, y)),
            PdfPathSegmentType::BezierTo => {
                has_curve = true;
                sub.push((x, y));
            }
            PdfPathSegmentType::Unknown => {}
        }
        if seg.is_close() {
            if let Some(&p0) = sub.first() {
                sub.push(p0);
            }
        }
    }
    flush(&mut sub, &mut has_curve, out);
}

fn classify_subpath(pts: &[(f32, f32)], out: &mut Shapes) {
    // Bỏ điểm trùng liên tiếp.
    let mut p: Vec<(f32, f32)> = Vec::new();
    for &q in pts {
        if p.last().map(|l: &(f32, f32)| (l.0 - q.0).abs() < 0.3 && (l.1 - q.1).abs() < 0.3).unwrap_or(false) {
            continue;
        }
        p.push(q);
    }
    let (minx, maxx) = p.iter().fold((f32::MAX, f32::MIN), |a, q| (a.0.min(q.0), a.1.max(q.0)));
    let (miny, maxy) = p.iter().fold((f32::MAX, f32::MIN), |a, q| (a.0.min(q.1), a.1.max(q.1)));
    let (w, h) = (maxx - minx, maxy - miny);
    // Mọi cạnh thẳng trục?
    let axis = p.windows(2).all(|s| (s[0].0 - s[1].0).abs() < 0.6 || (s[0].1 - s[1].1).abs() < 0.6);
    if !axis {
        return;
    }
    if h < 2.0 && w >= 30.0 {
        out.hlines.push([minx, maxx, (miny + maxy) / 2.0]);
    } else if (4..=6).contains(&p.len()) && w >= 5.0 && h >= 5.0 {
        out.rects.push([minx, miny, maxx, maxy]);
    } else if p.len() == 2 && h < 2.0 && w >= 30.0 {
        out.hlines.push([minx, maxx, miny]);
    }
}

fn walk_objects<'a>(objs: impl Iterator<Item = PdfPageObject<'a>>, m: Mat, out: &mut Shapes, depth: u32) {
    for obj in objs {
        let om = obj.matrix().map(|x| mat_of(&x)).unwrap_or(ID);
        match &obj {
            PdfPageObject::Path(p) => collect_path(p, mul(om, m), out),
            PdfPageObject::XObjectForm(f) if depth < 4 => {
                let fm = mul(om, m);
                let kids: Vec<PdfPageObject> = (0..f.len()).filter_map(|i| f.get(i).ok()).collect();
                walk_objects(kids.into_iter(), fm, out, depth + 1);
            }
            _ => {}
        }
    }
}

fn clean_label(s: &str) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_matches(|c: char| c == ':' || c == '.' || c == '_' || c == '…' || c == '-' || c == '*' || c.is_whitespace() || BOX_CHARS.contains(&c));
    let s: String = s.chars().take(48).collect();
    s.trim().to_string()
}

/// Gom ký tự thành dòng (theo thứ tự đọc của PDFium + tâm dọc).
fn lines_of(chars: &[Ch]) -> Vec<Vec<Ch>> {
    let mut lines: Vec<Vec<Ch>> = Vec::new();
    let mut cur: Vec<Ch> = Vec::new();
    for ch in chars {
        if ch.c == '\n' || ch.c == '\r' {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some(last) = cur.iter().rev().find(|c| !c.c.is_whitespace()) {
            let same = (ch.cy() - last.cy()).abs() <= last.h().max(ch.h()).max(2.0) * 0.6 && ch.l >= last.l - last.h();
            if !same {
                lines.push(std::mem::take(&mut cur));
            }
        }
        cur.push(ch.clone());
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn text_of_span(chars: &[Ch]) -> String {
    let mut s = String::new();
    let mut prev: Option<&Ch> = None;
    for c in chars {
        if let Some(p) = prev {
            if c.l - p.r > p.h().max(1.0) * 0.12 && !c.c.is_whitespace() && !p.c.is_whitespace() {
                s.push(' ');
            }
        }
        s.push(c.c);
        prev = Some(c);
    }
    s
}

fn overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let area = |r: &[f32; 4]| ((r[2] - r[0]) * (r[3] - r[1])).max(0.01);
    inter / area(a).min(area(b))
}

/// Nhãn bên trái 1 vùng: các ký tự cùng dải dọc, sát bên trái (≤ 220pt), dừng ở khoảng trống lớn.
fn left_label(all: &[Ch], r: &[f32; 4]) -> String {
    let cy = (r[1] + r[3]) / 2.0;
    let hh = ((r[3] - r[1]) / 2.0).max(6.0);
    let mut cand: Vec<&Ch> = all
        .iter()
        .filter(|c| !c.c.is_whitespace() && c.r <= r[0] + 1.0 && c.r >= r[0] - 220.0 && (c.cy() - cy).abs() <= hh)
        .collect();
    cand.sort_by(|a, b| b.r.partial_cmp(&a.r).unwrap_or(std::cmp::Ordering::Equal));
    let mut picked: Vec<Ch> = Vec::new();
    let mut edge = r[0];
    for c in cand {
        let gap = edge - c.r;
        let em = c.h().max(4.0);
        if gap > em * 2.5 && !picked.is_empty() {
            break;
        }
        if gap > em * 6.0 {
            break;
        }
        if c.c == '_' || c.c == '.' && picked.is_empty() {
            edge = c.l;
            continue;
        }
        picked.push(c.clone());
        edge = c.l;
    }
    picked.reverse();
    clean_label(&text_of_span(&picked))
}

fn right_label(all: &[Ch], r: &[f32; 4]) -> String {
    let cy = (r[1] + r[3]) / 2.0;
    let hh = ((r[3] - r[1]) / 2.0).max(6.0);
    let mut cand: Vec<&Ch> = all
        .iter()
        .filter(|c| !c.c.is_whitespace() && c.l >= r[2] - 1.0 && c.l <= r[2] + 260.0 && (c.cy() - cy).abs() <= hh)
        .collect();
    cand.sort_by(|a, b| a.l.partial_cmp(&b.l).unwrap_or(std::cmp::Ordering::Equal));
    let mut picked: Vec<Ch> = Vec::new();
    let mut edge = r[2];
    for c in cand {
        let em = c.h().max(4.0);
        let gap = c.l - edge;
        if gap > em * 2.5 || BOX_CHARS.contains(&c.c) || c.c == '_' {
            break;
        }
        picked.push(c.clone());
        edge = c.r;
    }
    clean_label(&text_of_span(&picked))
}

fn above_label(all: &[Ch], r: &[f32; 4]) -> String {
    let mut cand: Vec<&Ch> = all
        .iter()
        .filter(|c| !c.c.is_whitespace() && c.b >= r[3] - 1.0 && c.b <= r[3] + 16.0 && c.r > r[0] && c.l < r[2])
        .collect();
    cand.sort_by(|a, b| a.l.partial_cmp(&b.l).unwrap_or(std::cmp::Ordering::Equal));
    let v: Vec<Ch> = cand.into_iter().cloned().collect();
    clean_label(&text_of_span(&v))
}

fn has_text_inside(all: &[Ch], r: &[f32; 4]) -> bool {
    all.iter().any(|c| {
        !c.c.is_whitespace() && c.c != '_' && c.cx() > r[0] + 1.0 && c.cx() < r[2] - 1.0 && c.cy() > r[1] + 1.0 && c.cy() < r[3] - 1.0
    })
}

/// Chạy nhận diện trên các trang `pages` (rỗng = mọi trang).
pub fn recognize_fields(pdfium: &Pdfium, input: &Path, pages: &[u16]) -> Result<Vec<FieldProposal>, EngineError> {
    let doc = pdfium.load_pdf_from_file(input, None).map_err(|e| EngineError::Pdfium(format!("recognize: {e}")))?;
    let n = doc.pages().len();
    let list: Vec<u16> = if pages.is_empty() { (0..n).collect() } else { pages.iter().copied().filter(|p| *p < n).collect() };
    let mut out = Vec::new();
    for pi in list {
        let page = doc.pages().get(pi).map_err(|e| EngineError::Pdfium(format!("recognize trang {pi}: {e}")))?;
        let pw = page.width().value;
        // Widget đã có → không đề xuất trùng.
        let existing: Vec<[f32; 4]> = page
            .annotations()
            .iter()
            .filter(|a| a.annotation_type() == PdfPageAnnotationType::Widget)
            .filter_map(|a| a.bounds().ok())
            .map(|b| [b.left().value, b.bottom().value, b.right().value, b.top().value])
            .collect();
        let mut chars: Vec<Ch> = Vec::new();
        if let Ok(text) = page.text() {
            for c in text.chars().iter() {
                let Some(u) = c.unicode_char() else { continue };
                let b = if u == '_' || BOX_CHARS.contains(&u) { c.tight_bounds().or_else(|_| c.loose_bounds()) } else { c.loose_bounds().or_else(|_| c.tight_bounds()) };
                let Ok(b) = b else {
                    if u == '\n' || u == '\r' {
                        chars.push(Ch { c: u, l: 0.0, b: 0.0, r: 0.0, t: 0.0 });
                    }
                    continue;
                };
                chars.push(Ch { c: u, l: b.left().value, b: b.bottom().value, r: b.right().value, t: b.top().value });
            }
        }
        let mut props: Vec<FieldProposal> = Vec::new();
        let push = |props: &mut Vec<FieldProposal>, kind: &str, rect: [f32; 4], label: String| {
            if existing.iter().any(|e| overlap(e, &rect) > 0.3) || props.iter().any(|p| overlap(&p.rect, &rect) > 0.3) {
                return;
            }
            props.push(FieldProposal { page_index: pi, kind: kind.into(), rect, name: label.clone(), tooltip: label });
        };

        // 1. Gạch dưới / chấm dẫn / ô vuông ký tự.
        let lines = lines_of(&chars);
        for line in &lines {
            let line_h = line.iter().filter(|c| !c.c.is_whitespace() && c.c != '_' && c.c != '.').map(|c| c.h()).fold(0.0f32, f32::max);
            let mut i = 0;
            while i < line.len() {
                let c = line[i].c;
                if BOX_CHARS.contains(&c) {
                    let ch = &line[i];
                    let s = (ch.r - ch.l).max(ch.t - ch.b).max(6.0);
                    let (cx, cy) = (ch.cx(), ch.cy());
                    let rect = [cx - s / 2.0, cy - s / 2.0, cx + s / 2.0, cy + s / 2.0];
                    let mut label = right_label(&chars, &rect);
                    if label.is_empty() {
                        label = left_label(&chars, &rect);
                    }
                    push(&mut props, "checkbox", rect, label);
                    i += 1;
                    continue;
                }
                if c == '_' || c == '.' || c == '…' {
                    let mut j = i;
                    while j < line.len() && line[j].c == c {
                        j += 1;
                    }
                    let run = &line[i..j];
                    let enough = match c {
                        '_' => run.len() >= 3,
                        '.' => run.len() >= 5,
                        _ => run.len() >= 2,
                    };
                    let w = run.last().unwrap().r - run[0].l;
                    if enough && w >= 18.0 {
                        let base = if c == '_' { run.iter().map(|x| x.b).fold(f32::MAX, f32::min) } else { run.iter().map(|x| x.b).fold(f32::MAX, f32::min) - 1.0 };
                        let fh = if line_h > 0.0 { line_h } else { run[0].h().max(10.0) };
                        let hgt = (fh * 1.25).clamp(12.0, 26.0);
                        let rect = [run[0].l, base + 0.5, run.last().unwrap().r, base + 0.5 + hgt];
                        let label = left_label(&chars, &[rect[0], base, rect[2], base + fh]);
                        push(&mut props, "text", rect, label);
                    }
                    i = j;
                    continue;
                }
                i += 1;
            }
        }

        // 2. Hình vẽ vector.
        let mut shapes = Shapes::default();
        let objs = page.objects();
        walk_objects(objs.iter(), ID, &mut shapes, 0);
        // Ô vuông nhỏ → checkbox.
        let mut rects = shapes.rects.clone();
        rects.sort_by(|a, b| b[3].partial_cmp(&a[3]).unwrap_or(std::cmp::Ordering::Equal).then(a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal)));
        rects.dedup_by(|a, b| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0));
        for r in &rects {
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            if has_text_inside(&chars, r) {
                continue;
            }
            // Hình chứa hình khác (khung bảng, khung trang) → không phải ô nhập.
            let contains_other = rects.iter().any(|o| o != r && o[0] >= r[0] - 0.5 && o[2] <= r[2] + 0.5 && o[1] >= r[1] - 0.5 && o[3] <= r[3] + 0.5 && (o[2] - o[0]) * (o[3] - o[1]) < w * h * 0.9);
            if contains_other {
                continue;
            }
            if (5.0..=22.0).contains(&w) && (5.0..=22.0).contains(&h) && (w - h).abs() <= 3.0 {
                let mut label = right_label(&chars, r);
                if label.is_empty() {
                    label = left_label(&chars, r);
                }
                push(&mut props, "checkbox", *r, label);
            } else if w >= 30.0 && (10.0..=120.0).contains(&h) && w <= pw * 0.95 {
                let mut label = left_label(&chars, r);
                if label.is_empty() {
                    label = above_label(&chars, r);
                }
                push(&mut props, "text", [r[0] + 0.5, r[1] + 0.5, r[2] - 0.5, r[3] - 0.5], label);
            }
        }
        // Đường kẻ ngang trống phía trên → ô text.
        for l in &shapes.hlines {
            let (x0, x1, y) = (l[0], l[1], l[2]);
            if x1 - x0 > pw * 0.9 {
                continue; // đường phân cách cả trang
            }

            // Có chữ nằm ngay trên đường kẻ → đường gạch chân chữ, không phải ô nhập.
            let covered: f32 = chars
                .iter()
                .filter(|c| !c.c.is_whitespace() && c.c != '_' && c.cy() > y && c.cy() < y + 14.0 && c.r > x0 && c.l < x1)
                .map(|c| c.r.min(x1) - c.l.max(x0))
                .sum();
            if covered > (x1 - x0) * 0.3 {
                continue;
            }
            // Đường kẻ là cạnh của hình chữ nhật đã xét → bỏ.
            if rects.iter().any(|r| (r[1] - y).abs() < 1.5 && r[0] <= x0 + 1.0 && r[2] >= x1 - 1.0) {
                continue;
            }
            let rect = [x0, y + 0.5, x1, y + 0.5 + 16.0];
            let label = left_label(&chars, &[x0, y, x1, y + 12.0]);
            push(&mut props, "text", rect, label);
        }
        out.extend(props);
    }

    // Tên duy nhất: nhãn (hoặc Text/Check + số), trùng thì thêm hậu tố.
    let mut used: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let (mut nt, mut nc) = (0, 0);
    for p in out.iter_mut() {
        let base = if p.name.is_empty() {
            if p.kind == "checkbox" {
                nc += 1;
                format!("Check Box{nc}")
            } else {
                nt += 1;
                format!("Text{nt}")
            }
        } else {
            p.name.replace('.', " ")
        };
        let cnt = used.entry(base.clone()).or_insert(0);
        *cnt += 1;
        p.name = if *cnt == 1 { base } else { format!("{base} {cnt}") };
    }
    Ok(out)
}
