//! So sánh hai tài liệu PDF (Compare Files — như Foxit: Xem/Công cụ > So sánh).
//!
//! - So sánh VĂN BẢN: trích từ (kèm hộp bao) theo trang, căn trang cũ ↔ mới
//!   (xử lý chèn/xoá trang bằng Needleman–Wunsch trên độ giống MinHash), rồi
//!   diff cấp TỪ (Myers O(ND)) trên từng "dải" trang khớp liên tiếp — chữ trôi
//!   sang trang sau vẫn khớp. Dải quá khác → diff từng cặp trang.
//! - So sánh HÌNH ẢNH: render cặp trang bằng PDFium, diff điểm ảnh có ngưỡng →
//!   gom ô lưới liên thông → khung bao theo toạ độ PDF. Chế độ "cả hai" loại
//!   vùng chữ ra khỏi diff ảnh (chữ đã có diff văn bản) → chỉ còn đồ hoạ.
//!
//! Kết quả: danh sách thay đổi {loại, trang cũ/mới, hộp cũ/mới, text cũ/mới}
//! + bảng căn trang để UI hiện 2 cột đồng bộ.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;

use crate::text::Rect;
use crate::EngineError;

/// Thông điệp lỗi khi người dùng huỷ (UI nhận ra để không báo lỗi).
pub const COMPARE_CANCELLED: &str = "compare-cancelled";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareMode {
    /// Chỉ văn bản.
    Text,
    /// Chỉ hình ảnh (diff điểm ảnh toàn trang — cả chữ lẫn đồ hoạ).
    Visual,
    /// Văn bản + đồ hoạ (diff ảnh bỏ qua vùng chữ).
    Both,
}

#[derive(Clone, Debug)]
pub struct CompareOptions {
    pub mode: CompareMode,
    /// Trang (0-based) của tài liệu cũ cần so sánh; None = tất cả.
    pub old_pages: Option<Vec<u16>>,
    pub new_pages: Option<Vec<u16>>,
    pub ignore_case: bool,
    /// Bỏ qua khác biệt chỉ do khoảng trắng ("ab c" ≡ "a bc").
    pub ignore_whitespace: bool,
    /// Bỏ qua dấu câu (".,;:!?…" không tính là khác).
    pub ignore_punctuation: bool,
    /// DPI render cho so sánh hình ảnh.
    pub visual_dpi: f32,
    /// Ngưỡng lệch kênh màu (0–255) coi là khác.
    pub visual_tolerance: u8,
    pub old_password: Option<String>,
    pub new_password: Option<String>,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            mode: CompareMode::Both,
            old_pages: None,
            new_pages: None,
            ignore_case: false,
            ignore_whitespace: true,
            ignore_punctuation: false,
            visual_dpi: 72.0,
            visual_tolerance: 48,
            old_password: None,
            new_password: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Inserted,
    Deleted,
    Replaced,
    PageInserted,
    PageDeleted,
    /// Đồ hoạ/hình ảnh thay đổi (so sánh điểm ảnh).
    Graphics,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Inserted => "inserted",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Replaced => "replaced",
            ChangeKind::PageInserted => "pageInserted",
            ChangeKind::PageDeleted => "pageDeleted",
            ChangeKind::Graphics => "graphics",
        }
    }
}

/// Một thay đổi giữa hai tài liệu.
#[derive(Clone, Debug)]
pub struct Change {
    pub id: usize,
    pub kind: ChangeKind,
    /// Trang cũ liên quan (với "chèn" = vị trí dấu mũ nơi chữ được chèn).
    pub old_page: Option<u16>,
    /// Hộp (mỗi dòng 1 hộp) trên trang cũ. "Chèn" → 1 dấu mũ mảnh.
    pub old_rects: Vec<Rect>,
    pub new_page: Option<u16>,
    pub new_rects: Vec<Rect>,
    pub old_text: String,
    pub new_text: String,
}

/// Một hàng trong bảng căn trang (None = trang bị xoá/chèn ở phía kia).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePair {
    pub old: Option<u16>,
    pub new: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompareSummary {
    pub replaced: usize,
    pub inserted: usize,
    pub deleted: usize,
    pub page_inserted: usize,
    pub page_deleted: usize,
    pub graphics: usize,
}

impl CompareSummary {
    pub fn total(&self) -> usize {
        self.replaced + self.inserted + self.deleted + self.page_inserted + self.page_deleted + self.graphics
    }
}

#[derive(Clone, Debug)]
pub struct CompareResult {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    /// Kích thước (pt) MỌI trang của từng tài liệu (UI dựng khung trang).
    pub old_sizes: Vec<(f32, f32)>,
    pub new_sizes: Vec<(f32, f32)>,
    pub page_map: Vec<PagePair>,
    pub changes: Vec<Change>,
    pub summary: CompareSummary,
}

/// Tiến độ: `stage` ∈ "text" | "diff" | "visual".
#[derive(Clone, Copy, Debug)]
pub struct CompareProgress {
    pub stage: &'static str,
    pub done: usize,
    pub total: usize,
}

// ---------------------------------------------------------------------------
// Trích từ
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Word {
    text: String,
    /// Dạng chuẩn hoá (theo tuỳ chọn) — so sánh & kiểm "chỉ khác khoảng trắng".
    norm: String,
    key: u64,
    page: u16,
    rect: Rect,
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
        || ('\u{2000}'..='\u{206F}').contains(&c)
        || matches!(c, '«' | '»' | '¡' | '¿' | '·' | '、' | '。' | '，' | '：' | '；' | '！' | '？')
}

fn normalize(s: &str, o: &CompareOptions) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if o.ignore_punctuation && is_punct(c) {
            continue;
        }
        if o.ignore_case {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        left: a.left.min(b.left),
        bottom: a.bottom.min(b.bottom),
        right: a.right.max(b.right),
        top: a.top.max(b.top),
    }
}

fn pdf_rect(r: &PdfRect) -> Rect {
    Rect { left: r.left().value, bottom: r.bottom().value, right: r.right().value, top: r.top().value }
}

/// Tách text trang thành từ (theo khoảng trắng), mỗi từ có hộp bao hợp các ký tự.
fn page_words(page: &PdfPage, page_idx: u16, o: &CompareOptions) -> Vec<Word> {
    let text = match page.text() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut rect: Option<Rect> = None;
    let flush = |cur: &mut String, rect: &mut Option<Rect>, words: &mut Vec<Word>| {
        if !cur.is_empty() {
            let norm = normalize(cur, o);
            if !norm.is_empty() {
                let r = rect.unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
                words.push(Word { key: hash_str(&norm), norm, text: std::mem::take(cur), page: page_idx, rect: r });
            }
        }
        cur.clear();
        *rect = None;
    };
    for ch in text.chars().iter() {
        let c = ch.unicode_char();
        let space = match c {
            None => true,
            Some(c) => c.is_whitespace() || c == '\u{0}' || c == '\u{2}' || c == '\u{FFFE}',
        };
        if space {
            flush(&mut cur, &mut rect, &mut words);
            continue;
        }
        cur.push(c.unwrap());
        // loose_bounds: cao theo ascent/descent của font → khung dòng đều nhau.
        if let Ok(b) = ch.loose_bounds().or_else(|_| ch.tight_bounds()) {
            let r = pdf_rect(&b);
            rect = Some(match rect {
                None => r,
                Some(a) => union(a, r),
            });
        }
    }
    flush(&mut cur, &mut rect, &mut words);
    words
}

// ---------------------------------------------------------------------------
// Căn trang (MinHash + Needleman–Wunsch)
// ---------------------------------------------------------------------------

const MH_K: usize = 48;

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Chữ ký MinHash của tập từ; None = trang không có chữ.
fn minhash(words: &[Word]) -> Option<[u64; MH_K]> {
    if words.is_empty() {
        return None;
    }
    let mut sig = [u64::MAX; MH_K];
    for w in words {
        for (i, s) in sig.iter_mut().enumerate() {
            let h = splitmix(w.key ^ (i as u64).wrapping_mul(0xA24B_AED4_963E_E407));
            if h < *s {
                *s = h;
            }
        }
    }
    Some(sig)
}

fn similarity(a: &Option<[u64; MH_K]>, b: &Option<[u64; MH_K]>) -> f32 {
    match (a, b) {
        (None, None) => 1.0,
        (Some(x), Some(y)) => x.iter().zip(y.iter()).filter(|(p, q)| p == q).count() as f32 / MH_K as f32,
        _ => 0.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Match(usize, usize),
    OldOnly(usize),
    NewOnly(usize),
}

/// Căn 2 dãy trang. Điểm khớp = sim − 0.35, khoảng trống = −0.1 → cặp trang
/// giống nhau > ~15% được ghép, còn lại coi là trang bị xoá/chèn.
fn align_pages(a: &[Option<[u64; MH_K]>], b: &[Option<[u64; MH_K]>]) -> Vec<Align> {
    let (p, q) = (a.len(), b.len());
    if (p + 1) * (q + 1) > 4_000_000 {
        // Tài liệu cực lớn: ghép theo thứ tự, phần dư là chèn/xoá.
        let mut out = Vec::new();
        for i in 0..p.max(q) {
            match (i < p, i < q) {
                (true, true) => out.push(Align::Match(i, i)),
                (true, false) => out.push(Align::OldOnly(i)),
                (false, true) => out.push(Align::NewOnly(i)),
                _ => {}
            }
        }
        return out;
    }
    const GAP: f32 = -0.1;
    let w = q + 1;
    let mut s = vec![0f32; (p + 1) * w];
    for i in 1..=p {
        s[i * w] = GAP * i as f32;
    }
    for j in 1..=q {
        s[j] = GAP * j as f32;
    }
    for i in 1..=p {
        for j in 1..=q {
            let m = s[(i - 1) * w + j - 1] + similarity(&a[i - 1], &b[j - 1]) - 0.35;
            let d = s[(i - 1) * w + j] + GAP;
            let n = s[i * w + j - 1] + GAP;
            s[i * w + j] = m.max(d).max(n);
        }
    }
    let (mut i, mut j) = (p, q);
    let mut out = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let m = s[(i - 1) * w + j - 1] + similarity(&a[i - 1], &b[j - 1]) - 0.35;
            if (s[i * w + j] - m).abs() < 1e-5 {
                out.push(Align::Match(i - 1, j - 1));
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if i > 0 && (j == 0 || (s[i * w + j] - (s[(i - 1) * w + j] + GAP)).abs() < 1e-5) {
            out.push(Align::OldOnly(i - 1));
            i -= 1;
        } else {
            out.push(Align::NewOnly(j - 1));
            j -= 1;
        }
    }
    out.reverse();
    out
}

// ---------------------------------------------------------------------------
// Myers diff
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hunk {
    a0: usize,
    a1: usize,
    b0: usize,
    b1: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Del,
    Ins,
}

/// Myers O((N+M)·D), bộ nhớ O(D²). None nếu cần > `max_d` bước sửa.
fn myers_ops(a: &[u64], b: &[u64], max_d: usize) -> Option<Vec<Op>> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    if n == 0 {
        return Some(vec![Op::Ins; m as usize]);
    }
    if m == 0 {
        return Some(vec![Op::Del; n as usize]);
    }
    let limit = ((n + m) as usize).min(max_d) as isize;
    let off = limit + 1;
    let mut v = vec![0i32; (2 * limit + 3) as usize];
    let mut trace: Vec<Vec<i32>> = Vec::new();
    let mut found: Option<isize> = None;
    'outer: for d in 0..=limit {
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(k - 1 + off) as usize] < v[(k + 1 + off) as usize]) {
                v[(k + 1 + off) as usize] as isize
            } else {
                v[(k - 1 + off) as usize] as isize + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(k + off) as usize] = x as i32;
            if x >= n && y >= m {
                trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
    }
    let dmax = found?;
    let mut ops = Vec::with_capacity((n + m) as usize);
    let (mut x, mut y) = (n, m);
    for d in (1..=dmax).rev() {
        let vp = &trace[(d - 1) as usize];
        let get = |k: isize| vp[(k + d - 1) as usize] as isize;
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) { k + 1 } else { k - 1 };
        let prev_x = get(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(Op::Eq);
            x -= 1;
            y -= 1;
        }
        if prev_k == k + 1 {
            ops.push(Op::Ins);
            y -= 1;
        } else {
            ops.push(Op::Del);
            x -= 1;
        }
    }
    while x > 0 && y > 0 {
        ops.push(Op::Eq);
        x -= 1;
        y -= 1;
    }
    ops.reverse();
    Some(ops)
}

/// Diff 2 dãy khoá → các khối khác biệt. Cắt phần đầu/cuối trùng trước (nhanh
/// cho tài liệu gần giống nhau).
fn diff_hunks(a: &[u64], b: &[u64], max_d: usize) -> Option<Vec<Hunk>> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] {
        suf += 1;
    }
    let ops = myers_ops(&a[pre..a.len() - suf], &b[pre..b.len() - suf], max_d)?;
    let mut hunks = Vec::new();
    let (mut i, mut j) = (pre, pre);
    let mut open: Option<Hunk> = None;
    for op in ops {
        match op {
            Op::Eq => {
                if let Some(h) = open.take() {
                    hunks.push(h);
                }
                i += 1;
                j += 1;
            }
            Op::Del => {
                let h = open.get_or_insert(Hunk { a0: i, a1: i, b0: j, b1: j });
                i += 1;
                h.a1 = i;
            }
            Op::Ins => {
                let h = open.get_or_insert(Hunk { a0: i, a1: i, b0: j, b1: j });
                j += 1;
                h.b1 = j;
            }
        }
    }
    if let Some(h) = open {
        hunks.push(h);
    }
    Some(hunks)
}

// ---------------------------------------------------------------------------
// Dựng bản ghi thay đổi
// ---------------------------------------------------------------------------

const MAX_TEXT: usize = 4000;

fn join_text(words: &[&Word]) -> String {
    let mut s = String::new();
    for w in words {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&w.text);
        if s.len() > MAX_TEXT {
            s.push('…');
            break;
        }
    }
    s
}

/// Gộp hộp các từ liên tiếp cùng dòng → 1 hộp/dòng (như Foxit tô theo dòng).
fn line_rects(words: &[&Word]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for w in words {
        let r = w.rect;
        if let Some(last) = out.last_mut() {
            let h = (last.top - last.bottom).max(r.top - r.bottom).max(1.0);
            let same_line = (last.bottom - r.bottom).abs() < h * 0.5
                && (last.top - r.top).abs() < h * 0.6
                && r.left >= last.left - h
                && r.left - last.right < h * 8.0;
            if same_line {
                *last = union(*last, r);
                continue;
            }
        }
        out.push(r);
    }
    out
}

/// Dấu mũ (vị trí chèn/xoá ở phía kia): cạnh phải từ trước, hoặc cạnh trái từ sau.
fn caret_at(words: &[Word], pos: usize) -> Option<(u16, Rect)> {
    if pos > 0 && pos <= words.len() {
        let w = &words[pos - 1];
        Some((w.page, Rect { left: w.rect.right, right: w.rect.right + 1.5, bottom: w.rect.bottom, top: w.rect.top }))
    } else if pos < words.len() {
        let w = &words[pos];
        Some((w.page, Rect { left: w.rect.left - 1.5, right: w.rect.left, bottom: w.rect.bottom, top: w.rect.top }))
    } else {
        None
    }
}

fn group_by_page<'a>(words: &'a [Word]) -> Vec<(u16, Vec<&'a Word>)> {
    let mut out: Vec<(u16, Vec<&Word>)> = Vec::new();
    for w in words {
        match out.last_mut() {
            Some((p, v)) if *p == w.page => v.push(w),
            _ => out.push((w.page, vec![w])),
        }
    }
    out
}

struct RunCtx<'a> {
    a: &'a [Word],
    b: &'a [Word],
    /// Trang dự phòng khi 1 phía không có chữ (dải có trang trắng).
    old_fallback: u16,
    new_fallback: u16,
}

fn emit_hunk(ctx: &RunCtx, h: Hunk, o: &CompareOptions, out: &mut Vec<Change>) {
    let aw = &ctx.a[h.a0..h.a1];
    let bw = &ctx.b[h.b0..h.b1];
    if !aw.is_empty() && !bw.is_empty() && o.ignore_whitespace {
        let ca: String = aw.iter().map(|w| w.norm.as_str()).collect();
        let cb: String = bw.iter().map(|w| w.norm.as_str()).collect();
        if ca == cb {
            return; // chỉ khác cách ngắt khoảng trắng
        }
    }
    let mk = |kind, old_page, old_rects, new_page, new_rects, old_text, new_text| Change {
        id: 0,
        kind,
        old_page,
        old_rects,
        new_page,
        new_rects,
        old_text,
        new_text,
    };
    if aw.is_empty() {
        let caret = caret_at(ctx.a, h.a0);
        for (p, ws) in group_by_page(bw) {
            out.push(mk(
                ChangeKind::Inserted,
                Some(caret.map(|c| c.0).unwrap_or(ctx.old_fallback)),
                caret.map(|c| vec![c.1]).unwrap_or_default(),
                Some(p),
                line_rects(&ws),
                String::new(),
                join_text(&ws),
            ));
        }
    } else if bw.is_empty() {
        let caret = caret_at(ctx.b, h.b0);
        for (p, ws) in group_by_page(aw) {
            out.push(mk(
                ChangeKind::Deleted,
                Some(p),
                line_rects(&ws),
                Some(caret.map(|c| c.0).unwrap_or(ctx.new_fallback)),
                caret.map(|c| vec![c.1]).unwrap_or_default(),
                join_text(&ws),
                String::new(),
            ));
        }
    } else {
        let ga = group_by_page(aw);
        let gb = group_by_page(bw);
        let n = ga.len().max(gb.len());
        for i in 0..n {
            match (ga.get(i), gb.get(i)) {
                (Some((pa, wa)), Some((pb, wb))) => out.push(mk(
                    ChangeKind::Replaced,
                    Some(*pa),
                    line_rects(wa),
                    Some(*pb),
                    line_rects(wb),
                    join_text(wa),
                    join_text(wb),
                )),
                (Some((pa, wa)), None) => {
                    let (np, nr) = gb.last().map(|(p, w)| (*p, line_rects(w))).unwrap();
                    let caret = nr.last().map(|r| Rect { left: r.right, right: r.right + 1.5, ..*r });
                    out.push(mk(ChangeKind::Deleted, Some(*pa), line_rects(wa), Some(np), caret.into_iter().collect(), join_text(wa), String::new()));
                }
                (None, Some((pb, wb))) => {
                    let (op, or) = ga.last().map(|(p, w)| (*p, line_rects(w))).unwrap();
                    let caret = or.last().map(|r| Rect { left: r.right, right: r.right + 1.5, ..*r });
                    out.push(mk(ChangeKind::Inserted, Some(op), caret.into_iter().collect(), Some(*pb), line_rects(wb), String::new(), join_text(wb)));
                }
                (None, None) => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// So sánh hình ảnh
// ---------------------------------------------------------------------------

/// Ảnh RGBA thô.
pub struct RawImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Vùng khác nhau giữa 2 ảnh (x0,y0,x1,y1 theo pixel, x1/y1 loại trừ).
/// `exclude` = các ô pixel bỏ qua (vùng chữ). Pixel khác khi lệch kênh > `tol`;
/// gom theo ô lưới `cell` px, ô cách nhau ≤ 2 ô coi là cùng vùng.
pub fn diff_regions(a: &RawImage, b: &RawImage, tol: u8, exclude: &[(u32, u32, u32, u32)], cell: u32) -> Vec<(u32, u32, u32, u32)> {
    let w = a.width.min(b.width);
    let h = a.height.min(b.height);
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let cell = cell.max(1);
    let gw = w.div_ceil(cell) as usize;
    let gh = h.div_ceil(cell) as usize;
    let mut mask = vec![false; (w * h) as usize];
    for &(x0, y0, x1, y1) in exclude {
        for y in y0.min(h)..y1.min(h) {
            let row = (y * w) as usize;
            for x in x0.min(w)..x1.min(w) {
                mask[row + x as usize] = true;
            }
        }
    }
    let mut cells = vec![0u32; gw * gh];
    let mut any = false;
    for y in 0..h {
        let ra = (y * a.width * 4) as usize;
        let rb = (y * b.width * 4) as usize;
        for x in 0..w {
            if mask[(y * w + x) as usize] {
                continue;
            }
            let ia = ra + (x * 4) as usize;
            let ib = rb + (x * 4) as usize;
            let mut d = 0u8;
            for c in 0..3 {
                d = d.max(a.rgba[ia + c].abs_diff(b.rgba[ib + c]));
            }
            if d > tol {
                cells[(y / cell) as usize * gw + (x / cell) as usize] += 1;
                any = true;
            }
        }
    }
    if !any {
        return Vec::new();
    }
    // Gom ô liên thông (lân cận Chebyshev ≤ R ô).
    const R: isize = 2;
    let mut seen = vec![false; gw * gh];
    let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..gw * gh {
        if cells[start] == 0 || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut cx0, mut cy0, mut cx1, mut cy1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut px = 0u32;
        while let Some(c) = stack.pop() {
            let (cx, cy) = (c % gw, c / gw);
            px += cells[c];
            cx0 = cx0.min(cx);
            cy0 = cy0.min(cy);
            cx1 = cx1.max(cx);
            cy1 = cy1.max(cy);
            for dy in -R..=R {
                for dx in -R..=R {
                    let nx = cx as isize + dx;
                    let ny = cy as isize + dy;
                    if nx < 0 || ny < 0 || nx >= gw as isize || ny >= gh as isize {
                        continue;
                    }
                    let ni = ny as usize * gw + nx as usize;
                    if cells[ni] > 0 && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
        if px < 3 {
            continue; // nhiễu lẻ tẻ
        }
        boxes.push((
            cx0 as u32 * cell,
            cy0 as u32 * cell,
            ((cx1 as u32 + 1) * cell).min(w),
            ((cy1 as u32 + 1) * cell).min(h),
        ));
    }
    boxes
}

fn render_raw(page: &PdfPage, dpi: f32) -> Option<RawImage> {
    let w_px = ((page.width().value * dpi / 72.0).round() as i32).max(1);
    let cfg = PdfRenderConfig::new()
        .set_target_width(w_px)
        .set_maximum_height(w_px * 10)
        .render_annotations(false)
        .render_form_data(false);
    let bmp = page.render_with_config(&cfg).ok()?;
    let img = bmp.as_image().to_rgba8();
    Some(RawImage { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// Hộp pt (gốc dưới-trái) → ô pixel trên ảnh render (gốc trên-trái), nới `pad` pt.
fn pt_to_px(r: &Rect, page_h: f32, sx: f32, sy: f32, pad: f32) -> (u32, u32, u32, u32) {
    let x0 = ((r.left - pad) / sx).floor().max(0.0) as u32;
    let x1 = ((r.right + pad) / sx).ceil().max(0.0) as u32;
    let y0 = ((page_h - r.top - pad) / sy).floor().max(0.0) as u32;
    let y1 = ((page_h - r.bottom + pad) / sy).ceil().max(0.0) as u32;
    (x0, y0, x1, y1)
}

// ---------------------------------------------------------------------------
// API chính
// ---------------------------------------------------------------------------

fn load<'a>(pdfium: &'a Pdfium, p: &Path, pw: Option<&'a str>) -> Result<PdfDocument<'a>, EngineError> {
    pdfium
        .load_pdf_from_file(p, pw)
        .map_err(|e| EngineError::Pdfium(format!("{}: {e}", p.display())))
}

fn page_list(sel: &Option<Vec<u16>>, count: u16) -> Vec<u16> {
    match sel {
        Some(v) if !v.is_empty() => {
            let mut v: Vec<u16> = v.iter().copied().filter(|&i| i < count).collect();
            v.sort_unstable();
            v.dedup();
            v
        }
        _ => (0..count).collect(),
    }
}

fn cancelled() -> EngineError {
    EngineError::Pdfium(COMPARE_CANCELLED.to_string())
}

/// So sánh `old` với `new`. `progress` trả `false` để huỷ.
pub fn compare_documents(
    pdfium: &Pdfium,
    old: &Path,
    new: &Path,
    o: &CompareOptions,
    progress: &mut dyn FnMut(CompareProgress) -> bool,
) -> Result<CompareResult, EngineError> {
    let da = load(pdfium, old, o.old_password.as_deref())?;
    let db = load(pdfium, new, o.new_password.as_deref())?;
    let sizes = |d: &PdfDocument| -> Vec<(f32, f32)> {
        d.pages().iter().map(|p| (p.width().value, p.height().value)).collect()
    };
    let old_sizes = sizes(&da);
    let new_sizes = sizes(&db);
    let pa = page_list(&o.old_pages, da.pages().len());
    let pb = page_list(&o.new_pages, db.pages().len());

    // 1. Trích từ mọi trang được chọn.
    let total = pa.len() + pb.len();
    let mut done = 0;
    let mut extract = |doc: &PdfDocument, list: &[u16]| -> Result<Vec<Vec<Word>>, EngineError> {
        let mut out = Vec::with_capacity(list.len());
        for &i in list {
            let page = doc.pages().get(i).map_err(|e| EngineError::Pdfium(format!("trang {i}: {e}")))?;
            out.push(page_words(&page, i, o));
            done += 1;
            if !progress(CompareProgress { stage: "text", done, total }) {
                return Err(cancelled());
            }
        }
        Ok(out)
    };
    let wa = extract(&da, &pa)?;
    let wb = extract(&db, &pb)?;

    // 2. Căn trang.
    let sa: Vec<_> = wa.iter().map(|w| minhash(w)).collect();
    let sb: Vec<_> = wb.iter().map(|w| minhash(w)).collect();
    let align = align_pages(&sa, &sb);
    let page_map: Vec<PagePair> = align
        .iter()
        .map(|a| match *a {
            Align::Match(i, j) => PagePair { old: Some(pa[i]), new: Some(pb[j]) },
            Align::OldOnly(i) => PagePair { old: Some(pa[i]), new: None },
            Align::NewOnly(j) => PagePair { old: None, new: Some(pb[j]) },
        })
        .collect();

    let mut changes: Vec<Change> = Vec::new();
    let text_on = o.mode != CompareMode::Visual;

    // Trang lân cận ở phía kia cho trang chèn/xoá (để 2 cột cùng nhảy tới).
    let neighbor = |row: usize, want_new: bool| -> Option<u16> {
        let pick = |p: &PagePair| if want_new { p.new } else { p.old };
        page_map[..row].iter().rev().find_map(pick).or_else(|| page_map[row..].iter().find_map(pick))
    };

    // 3. Diff văn bản theo dải trang khớp liên tiếp.
    if text_on {
        let snippet = |ws: &[Word]| {
            let refs: Vec<&Word> = ws.iter().take(40).collect();
            join_text(&refs)
        };
        let mut idx = 0;
        let runs_total = align.iter().filter(|a| matches!(a, Align::Match(..))).count().max(1);
        let mut runs_done = 0;
        while idx < align.len() {
            match align[idx] {
                Align::OldOnly(i) => {
                    let (w, h) = old_sizes[pa[i] as usize];
                    changes.push(Change {
                        id: 0,
                        kind: ChangeKind::PageDeleted,
                        old_page: Some(pa[i]),
                        old_rects: vec![Rect { left: 0.0, bottom: 0.0, right: w, top: h }],
                        new_page: neighbor(idx, true),
                        new_rects: Vec::new(),
                        old_text: snippet(&wa[i]),
                        new_text: String::new(),
                    });
                    idx += 1;
                }
                Align::NewOnly(j) => {
                    let (w, h) = new_sizes[pb[j] as usize];
                    changes.push(Change {
                        id: 0,
                        kind: ChangeKind::PageInserted,
                        old_page: neighbor(idx, false),
                        old_rects: Vec::new(),
                        new_page: Some(pb[j]),
                        new_rects: vec![Rect { left: 0.0, bottom: 0.0, right: w, top: h }],
                        old_text: String::new(),
                        new_text: snippet(&wb[j]),
                    });
                    idx += 1;
                }
                Align::Match(..) => {
                    let start = idx;
                    while idx < align.len() && matches!(align[idx], Align::Match(..)) {
                        idx += 1;
                    }
                    let pairs: Vec<(usize, usize)> = align[start..idx]
                        .iter()
                        .map(|a| match *a {
                            Align::Match(i, j) => (i, j),
                            _ => unreachable!(),
                        })
                        .collect();
                    let a: Vec<Word> = pairs.iter().flat_map(|&(i, _)| wa[i].iter().cloned()).collect();
                    let b: Vec<Word> = pairs.iter().flat_map(|&(_, j)| wb[j].iter().cloned()).collect();
                    let ka: Vec<u64> = a.iter().map(|w| w.key).collect();
                    let kb: Vec<u64> = b.iter().map(|w| w.key).collect();
                    if let Some(hunks) = diff_hunks(&ka, &kb, 2000) {
                        let ctx = RunCtx { a: &a, b: &b, old_fallback: pa[pairs[0].0], new_fallback: pb[pairs[0].1] };
                        for h in hunks {
                            emit_hunk(&ctx, h, o, &mut changes);
                        }
                    } else {
                        // Dải khác quá nhiều → diff từng cặp trang (chữ trôi trang
                        // có thể thành xoá + chèn, nhưng vẫn đúng và không bùng nổ).
                        for &(i, j) in &pairs {
                            let ka: Vec<u64> = wa[i].iter().map(|w| w.key).collect();
                            let kb: Vec<u64> = wb[j].iter().map(|w| w.key).collect();
                            let ctx = RunCtx { a: &wa[i], b: &wb[j], old_fallback: pa[i], new_fallback: pb[j] };
                            let hunks = diff_hunks(&ka, &kb, 2000)
                                .unwrap_or_else(|| vec![Hunk { a0: 0, a1: ka.len(), b0: 0, b1: kb.len() }]);
                            for h in hunks {
                                emit_hunk(&ctx, h, o, &mut changes);
                            }
                        }
                    }
                    runs_done += pairs.len();
                    if !progress(CompareProgress { stage: "diff", done: runs_done, total: runs_total }) {
                        return Err(cancelled());
                    }
                }
            }
        }
    } else {
        // Chỉ hình ảnh: vẫn báo trang chèn/xoá.
        for (row, a) in align.iter().enumerate() {
            match *a {
                Align::OldOnly(i) => {
                    let (w, h) = old_sizes[pa[i] as usize];
                    changes.push(Change {
                        id: 0, kind: ChangeKind::PageDeleted, old_page: Some(pa[i]),
                        old_rects: vec![Rect { left: 0.0, bottom: 0.0, right: w, top: h }],
                        new_page: neighbor(row, true), new_rects: Vec::new(),
                        old_text: String::new(), new_text: String::new(),
                    });
                }
                Align::NewOnly(j) => {
                    let (w, h) = new_sizes[pb[j] as usize];
                    changes.push(Change {
                        id: 0, kind: ChangeKind::PageInserted, old_page: neighbor(row, false),
                        old_rects: Vec::new(), new_page: Some(pb[j]),
                        new_rects: vec![Rect { left: 0.0, bottom: 0.0, right: w, top: h }],
                        old_text: String::new(), new_text: String::new(),
                    });
                }
                Align::Match(..) => {}
            }
        }
    }

    // 4. So sánh hình ảnh trên các cặp trang khớp.
    if o.mode != CompareMode::Text {
        let dpi = o.visual_dpi.clamp(24.0, 300.0);
        let pairs: Vec<(usize, usize)> = align
            .iter()
            .filter_map(|a| if let Align::Match(i, j) = *a { Some((i, j)) } else { None })
            .collect();
        let cell = ((dpi / 24.0).round() as u32).max(2);
        for (n, &(i, j)) in pairs.iter().enumerate() {
            let (oi, ni) = (pa[i], pb[j]);
            let pg_a = da.pages().get(oi).map_err(|e| EngineError::Pdfium(e.to_string()))?;
            let pg_b = db.pages().get(ni).map_err(|e| EngineError::Pdfium(e.to_string()))?;
            if let (Some(ia), Some(ib)) = (render_raw(&pg_a, dpi), render_raw(&pg_b, dpi)) {
                let (wa_pt, ha_pt) = old_sizes[oi as usize];
                let (wb_pt, hb_pt) = new_sizes[ni as usize];
                let (sxa, sya) = (wa_pt / ia.width as f32, ha_pt / ia.height as f32);
                let (sxb, syb) = (wb_pt / ib.width as f32, hb_pt / ib.height as f32);
                let mut exclude = Vec::new();
                if o.mode == CompareMode::Both {
                    // Vùng chữ đã có diff văn bản → loại khỏi diff ảnh.
                    exclude.extend(wa[i].iter().map(|w| pt_to_px(&w.rect, ha_pt, sxa, sya, 1.0)));
                    exclude.extend(wb[j].iter().map(|w| pt_to_px(&w.rect, hb_pt, sxb, syb, 1.0)));
                }
                for (x0, y0, x1, y1) in diff_regions(&ia, &ib, o.visual_tolerance, &exclude, cell) {
                    let to_pt = |sx: f32, sy: f32, h: f32| Rect {
                        left: x0 as f32 * sx,
                        right: x1 as f32 * sx,
                        top: h - y0 as f32 * sy,
                        bottom: h - y1 as f32 * sy,
                    };
                    changes.push(Change {
                        id: 0,
                        kind: ChangeKind::Graphics,
                        old_page: Some(oi),
                        old_rects: vec![to_pt(sxa, sya, ha_pt)],
                        new_page: Some(ni),
                        new_rects: vec![to_pt(sxb, syb, hb_pt)],
                        old_text: String::new(),
                        new_text: String::new(),
                    });
                }
            }
            if !progress(CompareProgress { stage: "visual", done: n + 1, total: pairs.len() }) {
                return Err(cancelled());
            }
        }
    }

    // 5. Sắp theo hàng căn trang rồi từ trên xuống; đánh số.
    let mut row_new: HashMap<u16, usize> = HashMap::new();
    let mut row_old: HashMap<u16, usize> = HashMap::new();
    for (r, p) in page_map.iter().enumerate() {
        if let Some(n) = p.new {
            row_new.insert(n, r);
        }
        if let Some(n) = p.old {
            row_old.insert(n, r);
        }
    }
    let key = |c: &Change| -> (usize, i64) {
        let (row, rects) = match c.kind {
            ChangeKind::PageDeleted => (c.old_page.and_then(|p| row_old.get(&p).copied()), &c.old_rects),
            _ => match c.new_page.and_then(|p| row_new.get(&p).copied()) {
                Some(r) if !c.new_rects.is_empty() || c.old_page.is_none() => (Some(r), &c.new_rects),
                _ => (c.old_page.and_then(|p| row_old.get(&p).copied()), &c.old_rects),
            },
        };
        let top = rects.first().map(|r| r.top).unwrap_or(f32::MAX);
        (row.unwrap_or(usize::MAX), -(top * 10.0) as i64)
    };
    changes.sort_by_key(|c| key(c));
    let mut summary = CompareSummary::default();
    for (i, c) in changes.iter_mut().enumerate() {
        c.id = i;
        match c.kind {
            ChangeKind::Inserted => summary.inserted += 1,
            ChangeKind::Deleted => summary.deleted += 1,
            ChangeKind::Replaced => summary.replaced += 1,
            ChangeKind::PageInserted => summary.page_inserted += 1,
            ChangeKind::PageDeleted => summary.page_deleted += 1,
            ChangeKind::Graphics => summary.graphics += 1,
        }
    }

    Ok(CompareResult {
        old_path: old.to_path_buf(),
        new_path: new.to_path_buf(),
        old_sizes,
        new_sizes,
        page_map,
        changes,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &str) -> Vec<u64> {
        s.split_whitespace().map(hash_str).collect()
    }

    #[test]
    fn myers_basic_hunks() {
        let a = keys("a b c d e f");
        let b = keys("a x c d f g");
        let h = diff_hunks(&a, &b, 100).unwrap();
        assert_eq!(
            h,
            vec![
                Hunk { a0: 1, a1: 2, b0: 1, b1: 2 },
                Hunk { a0: 4, a1: 5, b0: 4, b1: 4 },
                Hunk { a0: 6, a1: 6, b0: 5, b1: 6 },
            ]
        );
    }

    #[test]
    fn myers_limit_returns_none() {
        let a = keys("a b c d e f g h");
        let b = keys("1 2 3 4 5 6 7 8");
        assert!(diff_hunks(&a, &b, 4).is_none());
        let h = diff_hunks(&a, &b, 100).unwrap();
        assert_eq!(h, vec![Hunk { a0: 0, a1: 8, b0: 0, b1: 8 }]);
    }

    #[test]
    fn diff_regions_finds_two_boxes() {
        let mk = |fill: &dyn Fn(u32, u32) -> bool| {
            let mut v = vec![255u8; 100 * 100 * 4];
            for y in 0..100 {
                for x in 0..100 {
                    if fill(x, y) {
                        let i = ((y * 100 + x) * 4) as usize;
                        v[i] = 0;
                        v[i + 1] = 0;
                        v[i + 2] = 0;
                    }
                }
            }
            RawImage { width: 100, height: 100, rgba: v }
        };
        let a = mk(&|x, y| (10..20).contains(&x) && (10..20).contains(&y));
        let b = mk(&|x, y| (70..80).contains(&x) && (10..20).contains(&y));
        let boxes = diff_regions(&a, &b, 40, &[], 3);
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        // Loại trừ vùng bên trái → còn 1.
        let boxes = diff_regions(&a, &b, 40, &[(0, 0, 50, 100)], 3);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
    }
}
