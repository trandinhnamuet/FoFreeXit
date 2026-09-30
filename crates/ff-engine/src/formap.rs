//! Dựng appearance stream (/AP) cho widget AcroForm — để field hiển thị ĐÚNG
//! trong mọi viewer mà không cần `NeedAppearances` (chuẩn Foxit/Acrobat).
//!
//! - Chữ ASCII: font chuẩn `/Helv` (Helvetica, WinAnsi) — bảng độ rộng AFM.
//! - Chữ có dấu tiếng Việt / Unicode: font TrueType NHÚNG dạng Type0/Identity-H
//!   (`/FXUni` trong `/AcroForm /DR`) với `/W` đủ mọi glyph + `/ToUnicode` —
//!   dùng lại cho mọi field của tài liệu (nhúng 1 lần).
//! - Checkbox/radio: vẽ ký hiệu bằng path vector (check/cross/circle/square/
//!   diamond/star) — không phụ thuộc ZapfDingbats của viewer; `/MK /CA` vẫn ghi
//!   mã ZapfDingbats tương ứng để viewer khác dựng lại cùng kiểu.

use std::collections::BTreeMap;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::EngineError;

/// Tên resource của font Unicode nhúng trong /DR.
pub(crate) const UNI_FONT: &str = "FXUni";

/// Độ rộng Helvetica (AFM, 1/1000 em) cho ASCII 32..=126.
const HELV_W: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32-47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48-63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64-79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80-95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96-111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112-126
];

/// Màu PDF (1 = xám, 3 = RGB, 4 = CMYK thành phần).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Color(pub Vec<f32>);

impl Color {
    pub fn black() -> Self {
        Color(vec![0.0])
    }
    pub fn rgb(c: [f32; 3]) -> Self {
        Color(c.to_vec())
    }
    fn op(&self, stroke: bool) -> String {
        let nums: Vec<String> = self.0.iter().map(|v| fmt(*v)).collect();
        let op = match (self.0.len(), stroke) {
            (1, false) => "g",
            (1, true) => "G",
            (4, false) => "k",
            (4, true) => "K",
            (_, false) => "rg",
            (_, true) => "RG",
        };
        format!("{} {}", nums.join(" "), op)
    }
    /// Màu tối hơn (dùng cho trạng thái "nhấn" /D).
    fn darker(&self) -> Self {
        if self.0.len() == 4 {
            return Color(self.0.iter().map(|v| (v + 0.25).min(1.0)).collect());
        }
        Color(self.0.iter().map(|v| (v * 0.75).max(0.0)).collect())
    }
    pub fn to_rgb(&self) -> [f32; 3] {
        match self.0.len() {
            1 => [self.0[0]; 3],
            4 => {
                let k = self.0[3];
                [
                    (1.0 - self.0[0]) * (1.0 - k),
                    (1.0 - self.0[1]) * (1.0 - k),
                    (1.0 - self.0[2]) * (1.0 - k),
                ]
            }
            3 => [self.0[0], self.0[1], self.0[2]],
            _ => [0.0; 3],
        }
    }
    pub fn from_array(arr: &[Object]) -> Option<Self> {
        let v: Vec<f32> = arr.iter().filter_map(|o| o.as_float().ok()).collect();
        if v.is_empty() || v.len() == 2 || v.len() > 4 {
            return None; // mảng rỗng = trong suốt
        }
        Some(Color(v))
    }
    pub fn to_object(&self) -> Object {
        Object::Array(self.0.iter().map(|v| Object::Real(*v)).collect())
    }
}

pub(crate) fn fmt(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s
    }
}

/// Kiểu viền /BS /S.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Border {
    Solid,
    Dashed,
    Beveled,
    Inset,
    Underline,
}

impl Border {
    pub fn from_name(n: &[u8]) -> Self {
        match n {
            b"D" => Border::Dashed,
            b"B" => Border::Beveled,
            b"I" => Border::Inset,
            b"U" => Border::Underline,
            _ => Border::Solid,
        }
    }
    pub fn name(&self) -> &'static [u8] {
        match self {
            Border::Solid => b"S",
            Border::Dashed => b"D",
            Border::Beveled => b"B",
            Border::Inset => b"I",
            Border::Underline => b"U",
        }
    }
}

/// Kiểu dáng chung của 1 widget (đọc từ /MK, /BS, /DA, /Q).
#[derive(Clone, Debug)]
pub(crate) struct WStyle {
    pub w: f32,
    pub h: f32,
    pub bg: Option<Color>,
    pub bc: Option<Color>,
    pub bw: f32,
    pub border: Border,
    /// Tên font trong DA (vd "Helv").
    #[allow(dead_code)]
    pub font: String,
    /// 0 = tự co.
    pub font_size: f32,
    pub color: Color,
    /// 0 trái, 1 giữa, 2 phải.
    pub align: u8,
}

// ---------- Font ----------

/// Font dùng để vẽ chữ: Helvetica chuẩn (ASCII) hoặc TrueType nhúng (Unicode).
pub(crate) struct ApFonts {
    pub helv: ObjectId,
    uni: Option<(ObjectId, Vec<u8>)>,
    uni_tried: bool,
}

impl ApFonts {
    /// Đảm bảo /DR có /Helv (+ /ZaDb cho viewer dựng lại checkbox) và nạp font
    /// Unicode đã nhúng sẵn (nếu có) để dùng lại.
    pub fn prepare(doc: &mut Document, acro_id: ObjectId) -> Result<Self, EngineError> {
        let dr_fonts = dr_font_dict(doc, acro_id);
        let mut helv = dr_fonts.get("Helv").copied();
        let mut zadb = dr_fonts.get("ZaDb").copied();
        if helv.is_none() {
            let mut f = Dictionary::new();
            f.set("Type", Object::Name(b"Font".to_vec()));
            f.set("Subtype", Object::Name(b"Type1".to_vec()));
            f.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
            f.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
            helv = Some(doc.add_object(Object::Dictionary(f)));
        }
        if zadb.is_none() {
            let mut f = Dictionary::new();
            f.set("Type", Object::Name(b"Font".to_vec()));
            f.set("Subtype", Object::Name(b"Type1".to_vec()));
            f.set("BaseFont", Object::Name(b"ZapfDingbats".to_vec()));
            zadb = Some(doc.add_object(Object::Dictionary(f)));
        }
        set_dr_font(doc, acro_id, "Helv", helv.unwrap());
        set_dr_font(doc, acro_id, "ZaDb", zadb.unwrap());
        let mut me = ApFonts { helv: helv.unwrap(), uni: None, uni_tried: false };
        if let Some(&uid) = dr_fonts.get(UNI_FONT) {
            if let Some(bytes) = embedded_font_bytes(doc, uid) {
                me.uni = Some((uid, bytes));
                me.uni_tried = true;
            }
        }
        Ok(me)
    }

    /// Font Unicode (nhúng lần đầu khi cần). None nếu máy không có font TTF nào.
    fn uni(&mut self, doc: &mut Document, acro_id: ObjectId) -> Option<(ObjectId, &[u8])> {
        if !self.uni_tried {
            self.uni_tried = true;
            if let Some(bytes) = crate::annot::find_font_bytes(false, false) {
                if let Ok(id) = embed_uni_font(doc, &bytes) {
                    set_dr_font(doc, acro_id, UNI_FONT, id);
                    self.uni = Some((id, bytes));
                }
            }
        }
        self.uni.as_ref().map(|(id, b)| (*id, b.as_slice()))
    }
}

/// Bộ đo + mã hoá chữ cho 1 lần vẽ.
enum Face<'a> {
    Helv,
    Uni(ttf_parser::Face<'a>),
}

impl Face<'_> {
    fn width(&self, ch: char, size: f32) -> f32 {
        match self {
            Face::Helv => {
                let c = ch as u32;
                let w = if (32..=126).contains(&c) { HELV_W[(c - 32) as usize] } else { 556 };
                w as f32 * size / 1000.0
            }
            Face::Uni(f) => crate::fontmatch::char_advance(f, ch, size),
        }
    }
    fn text_width(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.width(c, size)).sum()
    }
    /// (ascent, descent) theo em (descent âm).
    fn metrics(&self) -> (f32, f32) {
        match self {
            Face::Helv => (0.718, -0.207),
            Face::Uni(f) => {
                let u = f.units_per_em() as f32;
                (f.ascender() as f32 / u, f.descender() as f32 / u)
            }
        }
    }
    /// Toán hạng chuỗi cho Tj.
    fn encode(&self, s: &str) -> String {
        match self {
            Face::Helv => {
                let mut out = String::from("(");
                for c in s.chars() {
                    let c = if (c as u32) < 32 || (c as u32) > 126 { '?' } else { c };
                    if matches!(c, '(' | ')' | '\\') {
                        out.push('\\');
                    }
                    out.push(c);
                }
                out.push(')');
                out
            }
            Face::Uni(f) => {
                let mut out = String::from("<");
                for c in s.chars() {
                    let g = f.glyph_index(c).or_else(|| f.glyph_index('?')).map(|g| g.0).unwrap_or(0);
                    out.push_str(&format!("{g:04X}"));
                }
                out.push('>');
                out
            }
        }
    }
}

/// Chọn font cho chuỗi: ASCII → Helv; còn lại → font Unicode nhúng.
/// Trả (tên resource, object font, bytes TTF nếu Unicode).
fn pick_font<'a>(
    doc: &mut Document,
    acro_id: ObjectId,
    fonts: &'a mut ApFonts,
    text: &str,
) -> (String, ObjectId, Option<&'a [u8]>) {
    let needs_uni = text.chars().any(|c| (c as u32) > 126 || ((c as u32) < 32 && c != '\n' && c != '\r' && c != '\t'));
    if needs_uni {
        let helv = fonts.helv;
        if let Some((id, bytes)) = fonts.uni(doc, acro_id) {
            return (UNI_FONT.into(), id, Some(bytes));
        }
        return ("Helv".into(), helv, None);
    }
    ("Helv".into(), fonts.helv, None)
}

fn make_face(bytes: Option<&[u8]>) -> Face<'_> {
    match bytes.and_then(|b| ttf_parser::Face::parse(b, 0).ok()) {
        Some(f) => Face::Uni(f),
        None => Face::Helv,
    }
}

// ---------- Khung (nền + viền) ----------

fn frame(st: &WStyle, round: bool, down: bool) -> String {
    let mut s = String::new();
    let (w, h, bw) = (st.w, st.h, st.bw);
    if let Some(bg) = &st.bg {
        let bg = if down { bg.darker() } else { bg.clone() };
        s.push_str(&format!("{}\n", bg.op(false)));
        if round {
            s.push_str(&circle(w / 2.0, h / 2.0, (w.min(h) / 2.0 - bw / 2.0).max(0.5)));
            s.push_str("f\n");
        } else {
            s.push_str(&format!("0 0 {} {} re f\n", fmt(w), fmt(h)));
        }
    } else if down {
        s.push_str("0.75 g\n");
        if round {
            s.push_str(&circle(w / 2.0, h / 2.0, (w.min(h) / 2.0 - bw / 2.0).max(0.5)));
            s.push_str("f\n");
        } else {
            s.push_str(&format!("0 0 {} {} re f\n", fmt(w), fmt(h)));
        }
    }
    let Some(bc) = &st.bc else { return s };
    if bw <= 0.0 {
        return s;
    }
    s.push_str(&format!("{} {} w\n", bc.op(true), fmt(bw)));
    if st.border == Border::Dashed {
        s.push_str("[3] 0 d\n");
    }
    if round {
        s.push_str(&circle(w / 2.0, h / 2.0, (w.min(h) / 2.0 - bw / 2.0).max(0.5)));
        s.push_str("S\n");
    } else if st.border == Border::Underline {
        s.push_str(&format!("0 {} m {} {} l S\n", fmt(bw / 2.0), fmt(w), fmt(bw / 2.0)));
    } else {
        s.push_str(&format!(
            "{} {} {} {} re S\n",
            fmt(bw / 2.0),
            fmt(bw / 2.0),
            fmt((w - bw).max(0.0)),
            fmt((h - bw).max(0.0))
        ));
        if matches!(st.border, Border::Beveled | Border::Inset) && !round {
            // Vát: cạnh trên-trái sáng, dưới-phải tối (Inset đảo lại).
            let (tl, br) = if st.border == Border::Beveled { ("1 g", "0.5 g") } else { ("0.5 g", "0.75 g") };
            let (a, b2) = (bw, 2.0 * bw);
            s.push_str(&format!(
                "{tl}\n{a} {a} m {a} {h1} l {w1} {h1} l {w2} {h2} l {b2} {h2} l {b2} {b2} l f\n",
                a = fmt(a),
                b2 = fmt(b2),
                h1 = fmt(h - a),
                w1 = fmt(w - a),
                w2 = fmt(w - b2),
                h2 = fmt(h - b2),
                tl = tl
            ));
            s.push_str(&format!(
                "{br}\n{w1} {h1} m {w1} {a} l {a} {a} l {b2} {b2} l {w2} {b2} l {w2} {h2} l f\n",
                a = fmt(a),
                b2 = fmt(b2),
                h1 = fmt(h - a),
                w1 = fmt(w - a),
                w2 = fmt(w - b2),
                h2 = fmt(h - b2),
                br = br
            ));
        }
    }
    if st.border == Border::Dashed {
        s.push_str("[] 0 d\n");
    }
    s
}

fn circle(cx: f32, cy: f32, r: f32) -> String {
    let k = 0.5523 * r;
    format!(
        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c\n",
        fmt(cx + r), fmt(cy),
        fmt(cx + r), fmt(cy + k), fmt(cx + k), fmt(cy + r), fmt(cx), fmt(cy + r),
        fmt(cx - k), fmt(cy + r), fmt(cx - r), fmt(cy + k), fmt(cx - r), fmt(cy),
        fmt(cx - r), fmt(cy - k), fmt(cx - k), fmt(cy - r), fmt(cx), fmt(cy - r),
        fmt(cx + k), fmt(cy - r), fmt(cx + r), fmt(cy - k), fmt(cx + r), fmt(cy),
    )
}

fn inset(st: &WStyle) -> f32 {
    let b = if matches!(st.border, Border::Beveled | Border::Inset) { st.bw * 2.0 } else { st.bw };
    if st.bc.is_some() { b } else { 0.0 }
}

// ---------- Text / Combo ----------

/// Tham số riêng của field text.
pub(crate) struct TextOpts<'a> {
    pub text: &'a str,
    pub multiline: bool,
    pub comb: Option<u32>,
    pub password: bool,
}

/// Nội dung + resources AP cho text field (và combo — 1 dòng).
pub(crate) fn text_ap(
    doc: &mut Document,
    acro_id: ObjectId,
    fonts: &mut ApFonts,
    st: &WStyle,
    o: &TextOpts,
) -> (Vec<u8>, Dictionary) {
    let shown: String = if o.password { "*".repeat(o.text.chars().count()) } else { o.text.to_string() };
    let (fname, fid, bytes) = pick_font(doc, acro_id, fonts, &shown);
    let face = make_face(bytes);
    let (asc, desc) = face.metrics();
    let pad = inset(st) + 2.0;
    let avail_w = (st.w - 2.0 * pad).max(1.0);
    let avail_h = (st.h - 2.0 * inset(st)).max(1.0);

    let mut body = String::new();
    body.push_str(&frame(st, false, false));
    body.push_str("/Tx BMC\nq\n");
    let ib = inset(st);
    body.push_str(&format!(
        "{} {} {} {} re W n\n",
        fmt(ib + 1.0),
        fmt(ib + 1.0),
        fmt((st.w - 2.0 * ib - 2.0).max(0.0)),
        fmt((st.h - 2.0 * ib - 2.0).max(0.0))
    ));

    if !shown.is_empty() {
        body.push_str("BT\n");
        if let (Some(n), false) = (o.comb.filter(|n| *n > 0), o.multiline) {
            // Comb: mỗi ký tự 1 ô đều nhau, căn giữa trong ô.
            let cell = st.w / n as f32;
            let mut size = st.font_size;
            if size <= 0.0 {
                size = ((avail_h - 2.0) / (asc - desc)).min(cell * 1.4).clamp(4.0, 24.0);
            }
            let base = (st.h - (asc - desc) * size) / 2.0 - desc * size;
            body.push_str(&format!("/{} {} Tf {}\n", fname, fmt(size), st.color.op(false)));
            let mut prev_x = 0.0f32;
            let mut first = true;
            for (i, ch) in shown.chars().take(n as usize).enumerate() {
                let cw = face.width(ch, size);
                let x = i as f32 * cell + (cell - cw) / 2.0;
                if first {
                    body.push_str(&format!("{} {} Td\n", fmt(x), fmt(base)));
                    first = false;
                } else {
                    body.push_str(&format!("{} 0 Td\n", fmt(x - prev_x)));
                }
                prev_x = x;
                body.push_str(&format!("{} Tj\n", face.encode(&ch.to_string())));
            }
        } else if o.multiline {
            let mut size = if st.font_size > 0.0 { st.font_size } else { 12.0 };
            let mut lines;
            loop {
                lines = crate::fontmatch::wrap_lines(&shown, avail_w, &|c| face.width(c, size));
                let need = lines.len() as f32 * size * 1.15;
                if st.font_size > 0.0 || need <= avail_h - 2.0 || size <= 4.0 {
                    break;
                }
                size -= 0.5;
            }
            let lh = size * 1.15;
            body.push_str(&format!("/{} {} Tf {}\n{} TL\n", fname, fmt(size), st.color.op(false), fmt(lh)));
            let top = st.h - inset(st) - 2.0 - asc * size;
            let mut prev_x = 0.0f32;
            for (i, line) in lines.iter().enumerate() {
                let lw = face.text_width(line, size);
                let x = align_x(st.align, pad, st.w, lw);
                if i == 0 {
                    body.push_str(&format!("{} {} Td\n", fmt(x), fmt(top)));
                } else {
                    body.push_str(&format!("{} {} Td\n", fmt(x - prev_x), fmt(-lh)));
                }
                prev_x = x;
                body.push_str(&format!("{} Tj\n", face.encode(line)));
            }
        } else {
            let line: String = shown.replace(['\r', '\n'], " ");
            let mut size = st.font_size;
            if size <= 0.0 {
                size = ((avail_h - 2.0) / ((asc - desc) * 1.08)).clamp(4.0, 12.0 * (avail_h / 16.0).max(1.0));
                let tw = face.text_width(&line, size);
                if tw > avail_w {
                    size = (size * avail_w / tw).max(4.0);
                }
            }
            let lw = face.text_width(&line, size);
            let x = align_x(st.align, pad, st.w, lw);
            let base = (st.h - (asc - desc) * size) / 2.0 - desc * size;
            body.push_str(&format!(
                "/{} {} Tf {}\n{} {} Td\n{} Tj\n",
                fname,
                fmt(size),
                st.color.op(false),
                fmt(x),
                fmt(base),
                face.encode(&line)
            ));
        }
        body.push_str("ET\n");
    }
    body.push_str("Q\nEMC\n");
    if let (Some(n), Some(bc)) = (o.comb.filter(|n| *n > 1), &st.bc) {
        if !o.multiline && st.bw > 0.0 {
            let cell = st.w / n as f32;
            body.push_str(&format!("{} {} w\n", bc.op(true), fmt(st.bw)));
            for i in 1..n {
                let x = cell * i as f32;
                body.push_str(&format!("{} 0 m {} {} l S\n", fmt(x), fmt(x), fmt(st.h)));
            }
        }
    }
    (body.into_bytes(), font_res(&fname, fid))
}

fn align_x(align: u8, pad: f32, w: f32, lw: f32) -> f32 {
    match align {
        1 => ((w - lw) / 2.0).max(pad.min(w / 2.0)),
        2 => (w - pad - lw).max(pad),
        _ => pad,
    }
}

fn font_res(name: &str, id: ObjectId) -> Dictionary {
    let mut f = Dictionary::new();
    f.set(name.as_bytes().to_vec(), Object::Reference(id));
    let mut r = Dictionary::new();
    r.set("Font", Object::Dictionary(f));
    r
}

/// AP list box: vẽ các dòng lựa chọn, dòng đang chọn tô xanh (chuẩn Acrobat).
pub(crate) fn list_ap(
    doc: &mut Document,
    acro_id: ObjectId,
    fonts: &mut ApFonts,
    st: &WStyle,
    items: &[String],
    selected: &[usize],
    top_index: usize,
) -> (Vec<u8>, Dictionary) {
    let all: String = items.concat();
    let (fname, fid, bytes) = pick_font(doc, acro_id, fonts, &all);
    let face = make_face(bytes);
    let (asc, desc) = face.metrics();
    let size = if st.font_size > 0.0 { st.font_size } else { 12.0 };
    let lh = size * (asc - desc).max(1.0) * 1.05;
    let ib = inset(st);
    let pad = ib + 2.0;
    let mut body = frame(st, false, false);
    body.push_str("/Tx BMC\nq\n");
    body.push_str(&format!(
        "{} {} {} {} re W n\n",
        fmt(ib),
        fmt(ib),
        fmt((st.w - 2.0 * ib).max(0.0)),
        fmt((st.h - 2.0 * ib).max(0.0))
    ));
    let mut y_top = st.h - ib;
    for (i, item) in items.iter().enumerate().skip(top_index) {
        if y_top < ib {
            break;
        }
        if selected.contains(&i) {
            body.push_str(&format!(
                "0.6 0.757 0.855 rg\n{} {} {} {} re f\n",
                fmt(ib),
                fmt(y_top - lh),
                fmt((st.w - 2.0 * ib).max(0.0)),
                fmt(lh)
            ));
        }
        let base = y_top - lh + (lh - (asc - desc) * size) / 2.0 - desc * size;
        let x = align_x(st.align, pad, st.w, face.text_width(item, size));
        body.push_str(&format!(
            "BT\n/{} {} Tf {}\n{} {} Td\n{} Tj\nET\n",
            fname,
            fmt(size),
            st.color.op(false),
            fmt(x),
            fmt(base),
            face.encode(item)
        ));
        y_top -= lh;
    }
    body.push_str("Q\nEMC\n");
    (body.into_bytes(), font_res(&fname, fid))
}

// ---------- Checkbox / radio ----------

/// Mã ZapfDingbats (/MK /CA) theo kiểu ký hiệu.
pub(crate) fn style_char(style: &str) -> &'static str {
    match style {
        "cross" => "8",
        "circle" => "l",
        "square" => "n",
        "diamond" => "u",
        "star" => "H",
        _ => "4",
    }
}

pub(crate) fn style_from_char(ca: &str) -> &'static str {
    match ca {
        "8" => "cross",
        "l" => "circle",
        "n" => "square",
        "u" => "diamond",
        "H" => "star",
        _ => "check",
    }
}

/// AP của checkbox/radio ở 1 trạng thái. `on` = vẽ ký hiệu.
pub(crate) fn check_ap(st: &WStyle, style: &str, on: bool, round: bool, down: bool) -> Vec<u8> {
    let mut s = frame(st, round, down);
    if on {
        let (w, h) = (st.w, st.h);
        let side = if st.font_size > 0.0 { st.font_size.min(w.min(h)) } else { w.min(h) * 0.7 };
        let cx = w / 2.0;
        let cy = h / 2.0;
        let r = side / 2.0;
        let col = st.color.clone();
        match style {
            "cross" => {
                let d = r * 0.8;
                s.push_str(&format!(
                    "{} {} w 1 J\n{} {} m {} {} l S {} {} m {} {} l S\n",
                    col.op(true),
                    fmt(side * 0.14),
                    fmt(cx - d), fmt(cy - d), fmt(cx + d), fmt(cy + d),
                    fmt(cx - d), fmt(cy + d), fmt(cx + d), fmt(cy - d)
                ));
            }
            "circle" => {
                s.push_str(&format!("{}\n", col.op(false)));
                s.push_str(&circle(cx, cy, r * if round { 0.55 } else { 0.7 }));
                s.push_str("f\n");
            }
            "square" => {
                let d = r * 0.7;
                s.push_str(&format!("{}\n{} {} {} {} re f\n", col.op(false), fmt(cx - d), fmt(cy - d), fmt(2.0 * d), fmt(2.0 * d)));
            }
            "diamond" => {
                let d = r * 0.85;
                s.push_str(&format!(
                    "{}\n{} {} m {} {} l {} {} l {} {} l h f\n",
                    col.op(false),
                    fmt(cx), fmt(cy + d), fmt(cx + d), fmt(cy), fmt(cx), fmt(cy - d), fmt(cx - d), fmt(cy)
                ));
            }
            "star" => {
                let mut pts = String::new();
                for i in 0..10 {
                    let ang = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let rr = if i % 2 == 0 { r * 0.9 } else { r * 0.38 };
                    let (x, y) = (cx + rr * ang.cos(), cy + rr * ang.sin());
                    pts.push_str(&format!("{} {} {} ", fmt(x), fmt(y), if i == 0 { "m" } else { "l" }));
                }
                s.push_str(&format!("{}\n{}h f\n", col.op(false), pts));
            }
            _ => {
                // Dấu tick: 3 điểm, nét tròn đầu.
                let x0 = cx - r * 0.75;
                let y0 = cy + r * 0.02;
                let x1 = cx - r * 0.2;
                let y1 = cy - r * 0.55;
                let x2 = cx + r * 0.8;
                let y2 = cy + r * 0.6;
                s.push_str(&format!(
                    "{} {} w 1 J 1 j\n{} {} m {} {} l {} {} l S\n",
                    col.op(true),
                    fmt(side * 0.15),
                    fmt(x0), fmt(y0), fmt(x1), fmt(y1), fmt(x2), fmt(y2)
                ));
            }
        }
    }
    s.into_bytes()
}

// ---------- Push button / chữ ký ----------

pub(crate) fn button_ap(
    doc: &mut Document,
    acro_id: ObjectId,
    fonts: &mut ApFonts,
    st: &WStyle,
    caption: &str,
    down: bool,
) -> (Vec<u8>, Dictionary) {
    let (fname, fid, bytes) = pick_font(doc, acro_id, fonts, caption);
    let face = make_face(bytes);
    let (asc, desc) = face.metrics();
    let mut s = frame(st, false, down);
    if !caption.is_empty() {
        let ib = inset(st);
        let avail_w = (st.w - 2.0 * ib - 4.0).max(1.0);
        let avail_h = (st.h - 2.0 * ib - 2.0).max(1.0);
        let mut size = st.font_size;
        if size <= 0.0 {
            size = (avail_h / ((asc - desc) * 1.1)).min(12.0).max(4.0);
            let tw = face.text_width(caption, size);
            if tw > avail_w {
                size = (size * avail_w / tw).max(4.0);
            }
        }
        let tw = face.text_width(caption, size);
        let off = if down { 1.0 } else { 0.0 };
        let x = (st.w - tw) / 2.0 + off;
        let base = (st.h - (asc - desc) * size) / 2.0 - desc * size - off;
        s.push_str(&format!(
            "q\nBT\n/{} {} Tf {}\n{} {} Td\n{} Tj\nET\nQ\n",
            fname,
            fmt(size),
            st.color.op(false),
            fmt(x),
            fmt(base),
            face.encode(caption)
        ));
    }
    (s.into_bytes(), font_res(&fname, fid))
}

pub(crate) fn sig_ap(st: &WStyle) -> Vec<u8> {
    frame(st, false, false).into_bytes()
}

// ---------- XObject / DR helpers ----------

/// Tạo Form XObject cho AP.
pub(crate) fn make_xobject(doc: &mut Document, w: f32, h: f32, content: Vec<u8>, res: Dictionary) -> ObjectId {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Form".to_vec()));
    d.set("FormType", Object::Integer(1));
    d.set(
        "BBox",
        Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Real(w), Object::Real(h)]),
    );
    d.set("Resources", Object::Dictionary(res));
    let mut st = Stream::new(d, content);
    let _ = st.compress();
    doc.add_object(Object::Stream(st))
}

fn dr_font_dict(doc: &Document, acro_id: ObjectId) -> BTreeMap<String, ObjectId> {
    let mut out = BTreeMap::new();
    let Ok(acro) = doc.get_object(acro_id).and_then(Object::as_dict) else { return out };
    let dr = match acro.get(b"DR") {
        Ok(Object::Reference(id)) => doc.get_object(*id).and_then(Object::as_dict).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let Some(dr) = dr else { return out };
    let fonts = match dr.get(b"Font") {
        Ok(Object::Reference(id)) => doc.get_object(*id).and_then(Object::as_dict).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    if let Some(fonts) = fonts {
        for (k, v) in fonts.iter() {
            if let Ok(id) = v.as_reference() {
                out.insert(String::from_utf8_lossy(k).into_owned(), id);
            }
        }
    }
    out
}

/// Ghi `/DR /Font /<name> id` (chuẩn hoá /DR, /Font thành dict trực tiếp).
fn set_dr_font(doc: &mut Document, acro_id: ObjectId, name: &str, id: ObjectId) {
    // Lấy /DR hiện có (có thể là reference).
    let dr_ref = doc
        .get_object(acro_id)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|a| a.get(b"DR").ok())
        .cloned();
    let mut dr = match dr_ref {
        Some(Object::Reference(r)) => doc.get_object(r).and_then(Object::as_dict).cloned().unwrap_or_default(),
        Some(Object::Dictionary(d)) => d,
        _ => Dictionary::new(),
    };
    let mut fonts = match dr.get(b"Font").cloned() {
        Ok(Object::Reference(r)) => doc.get_object(r).and_then(Object::as_dict).cloned().unwrap_or_default(),
        Ok(Object::Dictionary(d)) => d,
        _ => Dictionary::new(),
    };
    fonts.set(name.as_bytes().to_vec(), Object::Reference(id));
    dr.set("Font", Object::Dictionary(fonts));
    if let Ok(acro) = doc.get_object_mut(acro_id).and_then(Object::as_dict_mut) {
        acro.set("DR", Object::Dictionary(dr));
    }
}

/// Bytes TTF của font Type0 đã nhúng (FontFile2 của descendant).
fn embedded_font_bytes(doc: &Document, type0: ObjectId) -> Option<Vec<u8>> {
    let f = doc.get_object(type0).ok()?.as_dict().ok()?;
    let desc = f.get(b"DescendantFonts").ok()?;
    let desc = match desc {
        Object::Array(a) => a.first()?.clone(),
        Object::Reference(r) => doc.get_object(*r).ok()?.as_array().ok()?.first()?.clone(),
        _ => return None,
    };
    let cid = doc.get_object(desc.as_reference().ok()?).ok()?.as_dict().ok()?;
    let fd = doc.get_object(cid.get(b"FontDescriptor").ok()?.as_reference().ok()?).ok()?.as_dict().ok()?;
    let ff = doc.get_object(fd.get(b"FontFile2").ok()?.as_reference().ok()?).ok()?.as_stream().ok()?;
    let bytes = ff.decompressed_content().ok().unwrap_or_else(|| ff.content.clone());
    ttf_parser::Face::parse(&bytes, 0).ok()?;
    Some(bytes)
}

/// Nhúng TTF thành Type0/CIDFontType2 Identity-H, /W đủ mọi glyph (dùng lại
/// cho mọi giá trị sau này), /ToUnicode theo cmap của font.
fn embed_uni_font(doc: &mut Document, bytes: &[u8]) -> Result<ObjectId, EngineError> {
    let face = ttf_parser::Face::parse(bytes, 0).map_err(|e| EngineError::Pdfium(format!("ttf: {e:?}")))?;
    let upem = face.units_per_em() as f32;
    let sc = |v: f32| (v * 1000.0 / upem).round() as i64;
    let bb = face.global_bounding_box();
    let ps_name = face
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .and_then(|n| n.to_string())
        .unwrap_or_else(|| "FoFreeXitSans".into())
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "");
    let base = format!("FXAAAA+{ps_name}");

    let mut ff_dict = Dictionary::new();
    ff_dict.set("Length1", Object::Integer(bytes.len() as i64));
    let mut ff = Stream::new(ff_dict, bytes.to_vec());
    let _ = ff.compress();
    let ff_id = doc.add_object(Object::Stream(ff));

    let mut fd = Dictionary::new();
    fd.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    fd.set("FontName", Object::Name(base.as_bytes().to_vec()));
    fd.set("Flags", Object::Integer(32));
    fd.set(
        "FontBBox",
        Object::Array(vec![
            Object::Integer(sc(bb.x_min as f32)),
            Object::Integer(sc(bb.y_min as f32)),
            Object::Integer(sc(bb.x_max as f32)),
            Object::Integer(sc(bb.y_max as f32)),
        ]),
    );
    fd.set("ItalicAngle", Object::Integer(0));
    fd.set("Ascent", Object::Integer(sc(face.ascender() as f32)));
    fd.set("Descent", Object::Integer(sc(face.descender() as f32)));
    fd.set("CapHeight", Object::Integer(sc(face.capital_height().unwrap_or(face.ascender()) as f32)));
    fd.set("StemV", Object::Integer(80));
    fd.set("FontFile2", Object::Reference(ff_id));
    let fd_id = doc.add_object(Object::Dictionary(fd));

    let n = face.number_of_glyphs();
    let widths: Vec<Object> = (0..n)
        .map(|g| Object::Integer(sc(face.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap_or(0) as f32)))
        .collect();
    let mut sysinfo = Dictionary::new();
    sysinfo.set("Registry", Object::String(b"Adobe".to_vec(), StringFormat::Literal));
    sysinfo.set("Ordering", Object::String(b"Identity".to_vec(), StringFormat::Literal));
    sysinfo.set("Supplement", Object::Integer(0));
    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
    cid.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    cid.set("CIDSystemInfo", Object::Dictionary(sysinfo));
    cid.set("FontDescriptor", Object::Reference(fd_id));
    cid.set("DW", Object::Integer(1000));
    cid.set("W", Object::Array(vec![Object::Integer(0), Object::Array(widths)]));
    cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
    let cid_id = doc.add_object(Object::Dictionary(cid));

    // ToUnicode: glyph → codepoint (mỗi glyph lấy codepoint đầu tiên).
    let mut g2u: BTreeMap<u16, u32> = BTreeMap::new();
    if let Some(cmap) = face.tables().cmap {
        for sub in cmap.subtables {
            if !sub.is_unicode() {
                continue;
            }
            sub.codepoints(|cp| {
                if let Some(ch) = char::from_u32(cp) {
                    if let Some(g) = sub.glyph_index(cp) {
                        let _ = ch;
                        g2u.entry(g.0).or_insert(cp);
                    }
                }
            });
        }
    }
    let mut cm = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(u16, u32)> = g2u.into_iter().collect();
    for chunk in pairs.chunks(100) {
        cm.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, cp) in chunk {
            let mut hex = String::new();
            let mut buf = [0u16; 2];
            if let Some(c) = char::from_u32(*cp) {
                for u in c.encode_utf16(&mut buf) {
                    hex.push_str(&format!("{:04X}", u));
                }
            }
            cm.push_str(&format!("<{:04X}> <{}>\n", g, hex));
        }
        cm.push_str("endbfchar\n");
    }
    cm.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let mut tu = Stream::new(Dictionary::new(), cm.into_bytes());
    let _ = tu.compress();
    let tu_id = doc.add_object(Object::Stream(tu));

    let mut f0 = Dictionary::new();
    f0.set("Type", Object::Name(b"Font".to_vec()));
    f0.set("Subtype", Object::Name(b"Type0".to_vec()));
    f0.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    f0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    f0.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
    f0.set("ToUnicode", Object::Reference(tu_id));
    Ok(doc.add_object(Object::Dictionary(f0)))
}
