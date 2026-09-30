//! Chú thích mở rộng (tab Comment kiểu Foxit): Pencil/Ink, Line, Arrow,
//! Rectangle, Oval, Polygon, Polyline, Stamp (mẫu có sẵn / ảnh) + đọc chi tiết,
//! sửa, xoá chú thích CÓ SẴN trong file.
//!
//! Tất cả ghi bằng lopdf ở cấp object PDF, mỗi annotation đều có appearance
//! stream (/AP /N) tự dựng → hiển thị giống nhau trên PDFium, Acrobat, Foxit,
//! trình duyệt (không phụ thuộc viewer tự sinh AP). Mọi annotation mới đều có
//! /T (tác giả), /M, /CreationDate, /NM (id duy nhất) như Foxit.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lopdf::{Dictionary, Document as LoDoc, Object as LoObj, ObjectId, StringFormat};
use pdfium_render::prelude::*;

use crate::annot::{
    apply_annotations, attach_annot, build_unicode_ap, char_adv, embed_type0_font, encode_cid,
    find_font_bytes, pdf_text_string, AnnotKind, AnnotSpec,
};
use crate::text::Rect;
use crate::EngineError;

// ======================= Kiểu dữ liệu =======================

/// Loại chú thích vẽ/đặt tự do (ngoài markup theo text của `annot.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// Bút chì (vẽ tay, nhiều nét) — /Ink.
    Ink,
    /// Đoạn thẳng — /Line.
    Line,
    /// Mũi tên — /Line với /LE [/None /OpenArrow].
    Arrow,
    /// Hình chữ nhật — /Square.
    Square,
    /// Hình oval/tròn — /Circle.
    Circle,
    /// Đa giác kín — /Polygon.
    Polygon,
    /// Đường gấp khúc — /PolyLine.
    PolyLine,
    /// Đa giác dạng đám mây — /Polygon với /BE <</S /C>>.
    Cloud,
    /// Con dấu — /Stamp (mẫu chữ có sẵn hoặc ảnh PNG/JPG).
    Stamp,
}

/// Nội dung của con dấu.
#[derive(Clone, Debug, Default)]
pub struct StampSpec {
    /// Tên icon chuẩn (/Name) — vd "Approved", "Draft", "Confidential".
    pub name: String,
    /// Dòng chữ chính (đã bản địa hoá, vd "ĐÃ DUYỆT").
    pub label: String,
    /// Dòng phụ (vd "30/09/2026 14:05 · Tuyen") — con dấu ngày giờ.
    pub sub: Option<String>,
    /// Con dấu ảnh: đường dẫn PNG/JPG (bỏ qua label/sub).
    pub image: Option<PathBuf>,
}

/// Một chú thích vẽ tự do cần tạo.
#[derive(Clone, Debug)]
pub struct ShapeSpec {
    pub kind: ShapeKind,
    pub page_index: u16,
    /// Square/Circle/Stamp: khung. Các loại khác: suy ra từ điểm (có thể bỏ trống).
    pub rect: Rect,
    /// Line/Arrow: 2 điểm (đầu → cuối). Polygon/PolyLine/Cloud: các đỉnh.
    pub points: Vec<[f32; 2]>,
    /// Ink: mỗi nét là 1 dãy điểm.
    pub strokes: Vec<Vec<[f32; 2]>>,
    /// Màu viền/nét (RGB 0–255).
    pub color: [u8; 3],
    /// Màu tô (Square/Circle/Polygon/Cloud; None = trong suốt).
    pub fill: Option<[u8; 3]>,
    /// Độ dày nét (pt).
    pub width: f32,
    /// Độ mờ 0–1 (/CA).
    pub opacity: f32,
    pub contents: Option<String>,
    pub stamp: Option<StampSpec>,
}

impl ShapeSpec {
    /// Giá trị mặc định tiện cho test.
    pub fn new(kind: ShapeKind, page_index: u16, color: [u8; 3]) -> Self {
        ShapeSpec {
            kind,
            page_index,
            rect: Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 },
            points: Vec::new(),
            strokes: Vec::new(),
            color,
            fill: None,
            width: 2.0,
            opacity: 1.0,
            contents: None,
            stamp: None,
        }
    }
}

/// Thông tin gắn vào MỌI chú thích mới/sửa (như Foxit: tác giả + thời điểm).
#[derive(Clone, Debug, Default)]
pub struct AnnotMeta {
    /// /T — tên tác giả.
    pub author: Option<String>,
    /// Ngày giờ dạng PDF ("D:20260930140500+07'00'"); None = giờ UTC hiện tại.
    pub date: Option<String>,
}

/// Chỉ định 1 annotation có sẵn: trang + vị trí trong mảng /Annots của trang
/// (trùng chỉ số PDFium dùng); `nm` (nếu có) để kiểm tra đúng đối tượng.
#[derive(Clone, Debug)]
pub struct AnnotRef {
    pub page_index: u16,
    pub annot_index: usize,
    pub nm: Option<String>,
}

/// Thay đổi cho 1 annotation có sẵn. Trường None = giữ nguyên.
#[derive(Clone, Debug)]
pub struct AnnotUpdate {
    pub target: AnnotRef,
    /// Khung mới — hình học (QuadPoints/Vertices/InkList/L) co giãn theo.
    pub rect: Option<Rect>,
    pub color: Option<[u8; 3]>,
    /// Some(None) = bỏ màu tô.
    pub fill: Option<Option<[u8; 3]>>,
    pub width: Option<f32>,
    pub opacity: Option<f32>,
    pub contents: Option<String>,
    pub author: Option<String>,
}

impl AnnotUpdate {
    pub fn new(target: AnnotRef) -> Self {
        AnnotUpdate {
            target,
            rect: None,
            color: None,
            fill: None,
            width: None,
            opacity: None,
            contents: None,
            author: None,
        }
    }
}

/// Chi tiết 1 annotation đọc từ file (cho danh sách Comments).
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotDetail {
    pub page_index: u16,
    pub annot_index: usize,
    /// /Subtype gốc (Highlight, Square, Ink, Stamp, FreeText, Text...).
    pub subtype: String,
    pub rect: Rect,
    pub color: Option<[u8; 3]>,
    pub fill: Option<[u8; 3]>,
    pub width: Option<f32>,
    pub opacity: f32,
    pub contents: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub modified: Option<String>,
    pub created: Option<String>,
    pub nm: Option<String>,
    pub quads: Vec<Rect>,
    pub vertices: Vec<[f32; 2]>,
    pub ink: Vec<Vec<[f32; 2]>>,
    pub line: Option<[f32; 4]>,
    pub line_endings: Option<[String; 2]>,
    pub font_size: Option<f32>,
    /// /Name (icon của Note/Stamp).
    pub icon: Option<String>,
    /// /IRT → chỉ số annotation cha trên cùng trang (trả lời).
    pub in_reply_to: Option<usize>,
    pub hidden: bool,
    /// /BE /S /C (viền đám mây).
    pub cloudy: bool,
}

/// Toàn bộ thay đổi chú thích của 1 lần Lưu.
#[derive(Clone, Debug, Default)]
pub struct AnnotSaveRequest {
    /// Markup/Text box/Note (đường cũ của `annot.rs`).
    pub specs: Vec<AnnotSpec>,
    pub shapes: Vec<ShapeSpec>,
    pub updates: Vec<AnnotUpdate>,
    pub deletes: Vec<AnnotRef>,
    pub meta: AnnotMeta,
}

// ======================= Tiện ích chung =======================

fn perr<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> EngineError + '_ {
    move |e| EngineError::Pdfium(format!("{ctx}: {e}"))
}

/// Mở bằng lopdf; file có xref/trailer lệch (lopdf khắt khe, PDFium/Acrobat thì
/// tự sửa) → để PDFium đọc rồi ghi ra bộ nhớ và mở lại bằng lopdf. PDFium giữ
/// nguyên thứ tự /Annots nên chỉ số annotation vẫn khớp.
fn load_lo(pdfium: &Pdfium, path: &Path) -> Result<LoDoc, EngineError> {
    match LoDoc::load(path) {
        Ok(d) => Ok(d),
        Err(first) => {
            let doc = pdfium
                .load_pdf_from_file(path, None)
                .map_err(|e| EngineError::Pdfium(format!("lopdf load: {first}; PDFium: {e}")))?;
            let bytes = doc
                .save_to_bytes()
                .map_err(|e| EngineError::Pdfium(format!("PDFium lưu tạm: {e}")))?;
            LoDoc::load_mem(&bytes).map_err(perr("lopdf load"))
        }
    }
}

fn name(s: &str) -> LoObj {
    LoObj::Name(s.as_bytes().to_vec())
}

fn real(v: f32) -> LoObj {
    // Làm tròn 3 chữ số để file gọn & ổn định.
    LoObj::Real((v * 1000.0).round() / 1000.0)
}

fn num(o: &LoObj) -> Option<f32> {
    o.as_float().ok()
}

fn deref<'a>(doc: &'a LoDoc, o: &'a LoObj) -> &'a LoObj {
    match o {
        LoObj::Reference(id) => doc.get_object(*id).unwrap_or(o),
        _ => o,
    }
}

fn nums(doc: &LoDoc, o: &LoObj) -> Vec<f32> {
    match deref(doc, o) {
        LoObj::Array(a) => a.iter().filter_map(|x| num(deref(doc, x))).collect(),
        _ => Vec::new(),
    }
}

fn dict_nums(doc: &LoDoc, d: &Dictionary, key: &[u8]) -> Vec<f32> {
    d.get(key).map(|o| nums(doc, o)).unwrap_or_default()
}

fn rect_of(v: &[f32]) -> Option<Rect> {
    if v.len() < 4 {
        return None;
    }
    Some(Rect {
        left: v[0].min(v[2]),
        bottom: v[1].min(v[3]),
        right: v[0].max(v[2]),
        top: v[1].max(v[3]),
    })
}

fn rect_obj(r: &Rect) -> LoObj {
    LoObj::Array(vec![real(r.left), real(r.bottom), real(r.right), real(r.top)])
}

fn color_obj(c: [u8; 3]) -> LoObj {
    LoObj::Array(c.iter().map(|v| real(*v as f32 / 255.0)).collect())
}

/// /C, /IC: 1 thành phần (xám), 3 (RGB), 4 (CMYK) → RGB. Mảng rỗng = không màu.
fn color_of(v: &[f32]) -> Option<[u8; 3]> {
    let to = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    match v.len() {
        1 => Some([to(v[0]); 3]),
        3 => Some([to(v[0]), to(v[1]), to(v[2])]),
        4 => {
            let k = v[3];
            Some([
                to((1.0 - v[0]) * (1.0 - k)),
                to((1.0 - v[1]) * (1.0 - k)),
                to((1.0 - v[2]) * (1.0 - k)),
            ])
        }
        _ => None,
    }
}

/// Giải mã text string PDF: UTF-16BE (BOM) / UTF-8 (BOM) / PDFDocEncoding≈Latin-1.
fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16)
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    bytes.iter().map(|&b| b as char).collect()
}

fn dict_text(doc: &LoDoc, d: &Dictionary, key: &[u8]) -> Option<String> {
    match d.get(key).map(|o| deref(doc, o)) {
        Ok(LoObj::String(b, _)) => Some(decode_text(b)),
        _ => None,
    }
}

fn dict_name(doc: &LoDoc, d: &Dictionary, key: &[u8]) -> Option<String> {
    match d.get(key).map(|o| deref(doc, o)) {
        Ok(LoObj::Name(n)) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Ngày giờ UTC hiện tại dạng PDF ("D:YYYYMMDDHHmmSSZ").
pub fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("D:{y:04}{m:02}{d:02}{:02}{:02}{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

static NM_SEQ: AtomicU64 = AtomicU64::new(1);

/// Id duy nhất cho /NM (dạng Foxit/Acrobat: chuỗi hex không trùng).
fn new_nm() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = NM_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("ff-{nanos:x}-{seq:x}")
}

fn meta_date(meta: &AnnotMeta) -> String {
    meta.date.clone().filter(|s| !s.is_empty()).unwrap_or_else(pdf_date_now)
}

/// Gắn /T /M /CreationDate /NM cho annotation mới (không ghi đè giá trị có sẵn).
fn stamp_meta(d: &mut Dictionary, meta: &AnnotMeta, date: &str) {
    if let Some(a) = meta.author.as_deref().filter(|s| !s.is_empty()) {
        if !d.has(b"T") {
            d.set("T", pdf_text_string(a));
        }
    }
    if !d.has(b"M") {
        d.set("M", LoObj::String(date.as_bytes().to_vec(), StringFormat::Literal));
    }
    if !d.has(b"CreationDate") {
        d.set("CreationDate", LoObj::String(date.as_bytes().to_vec(), StringFormat::Literal));
    }
    if !d.has(b"NM") {
        d.set("NM", LoObj::String(new_nm().into_bytes(), StringFormat::Literal));
    }
}

fn fmt(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn rgb_ops(c: [u8; 3], stroke: bool) -> String {
    format!(
        "{} {} {} {}\n",
        fmt(c[0] as f32 / 255.0),
        fmt(c[1] as f32 / 255.0),
        fmt(c[2] as f32 / 255.0),
        if stroke { "RG" } else { "rg" }
    )
}

// ======================= Trang & mảng /Annots =======================

fn page_id(doc: &LoDoc, page_index: u16) -> Result<ObjectId, EngineError> {
    doc.get_pages()
        .get(&(page_index as u32 + 1))
        .copied()
        .ok_or_else(|| EngineError::Pdfium(format!("không có trang {}", page_index + 1)))
}

/// Mảng /Annots của trang (giữ nguyên vị trí — trùng chỉ số PDFium). Phần tử
/// là dict trực tiếp (hiếm) được chuyển thành object gián tiếp để tham chiếu được.
fn annots_ids(doc: &mut LoDoc, pid: ObjectId) -> Result<Vec<Option<ObjectId>>, EngineError> {
    let arr_ref = {
        let page = doc.get_dictionary(pid).map_err(perr("đọc trang"))?;
        match page.get(b"Annots") {
            Ok(LoObj::Reference(r)) => Some(*r),
            Ok(LoObj::Array(_)) => None,
            _ => return Ok(Vec::new()),
        }
    };
    let items: Vec<LoObj> = match arr_ref {
        Some(r) => doc.get_object(r).and_then(|o| o.as_array()).cloned().unwrap_or_default(),
        None => doc
            .get_dictionary(pid)
            .and_then(|p| p.get(b"Annots"))
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default(),
    };
    let mut changed = false;
    let mut out = Vec::with_capacity(items.len());
    let mut new_items = items.clone();
    for (i, it) in items.iter().enumerate() {
        match it {
            LoObj::Reference(id) => out.push(Some(*id)),
            LoObj::Dictionary(d) => {
                let id = doc.add_object(LoObj::Dictionary(d.clone()));
                new_items[i] = LoObj::Reference(id);
                changed = true;
                out.push(Some(id));
            }
            _ => out.push(None),
        }
    }
    if changed {
        match arr_ref {
            Some(r) => {
                if let Ok(a) = doc.get_object_mut(r).and_then(|o| o.as_array_mut()) {
                    *a = new_items;
                }
            }
            None => {
                if let Ok(p) = doc.get_object_mut(pid).and_then(|o| o.as_dict_mut()) {
                    p.set("Annots", LoObj::Array(new_items));
                }
            }
        }
    }
    Ok(out)
}

/// Bỏ các phần tử có id thuộc `remove` khỏi /Annots của trang.
fn remove_from_annots(doc: &mut LoDoc, pid: ObjectId, remove: &HashSet<ObjectId>) {
    let arr_ref = match doc.get_dictionary(pid).and_then(|p| p.get(b"Annots")) {
        Ok(LoObj::Reference(r)) => Some(*r),
        Ok(LoObj::Array(_)) => None,
        _ => return,
    };
    let keep = |o: &LoObj| !matches!(o, LoObj::Reference(id) if remove.contains(id));
    match arr_ref {
        Some(r) => {
            if let Ok(a) = doc.get_object_mut(r).and_then(|o| o.as_array_mut()) {
                a.retain(keep);
            }
        }
        None => {
            if let Ok(a) = doc
                .get_object_mut(pid)
                .and_then(|o| o.as_dict_mut())
                .and_then(|p| p.get_mut(b"Annots"))
                .and_then(|o| o.as_array_mut())
            {
                a.retain(keep);
            }
        }
    }
}

fn resolve_ref(doc: &mut LoDoc, r: &AnnotRef) -> Result<ObjectId, EngineError> {
    let pid = page_id(doc, r.page_index)?;
    let ids = annots_ids(doc, pid)?;
    let nm_of = |doc: &LoDoc, id: ObjectId| {
        doc.get_dictionary(id).ok().and_then(|d| dict_text(doc, d, b"NM"))
    };
    let at = ids.get(r.annot_index).copied().flatten();
    match (&r.nm, at) {
        (None, Some(id)) => Ok(id),
        (Some(nm), Some(id)) if nm_of(doc, id).as_deref() == Some(nm.as_str()) => Ok(id),
        (Some(nm), _) => ids
            .iter()
            .flatten()
            .copied()
            .find(|id| nm_of(doc, *id).as_deref() == Some(nm.as_str()))
            .ok_or_else(|| EngineError::Pdfium(format!("không tìm thấy chú thích /NM {nm}"))),
        (None, None) => Err(EngineError::Pdfium(format!(
            "trang {} không có chú thích #{}",
            r.page_index + 1,
            r.annot_index
        ))),
    }
}

// ======================= Appearance stream =======================

struct Ap {
    content: String,
    bbox: Rect,
    /// ExtGState /GS0 (độ mờ, blend).
    opacity: f32,
    multiply: bool,
    /// Tài nguyên bổ sung (/Font, /XObject).
    fonts: Vec<(String, ObjectId)>,
    xobjects: Vec<(String, ObjectId)>,
}

impl Ap {
    fn new(bbox: Rect, opacity: f32) -> Self {
        Ap {
            content: String::new(),
            bbox,
            opacity,
            multiply: false,
            fonts: Vec::new(),
            xobjects: Vec::new(),
        }
    }
}

fn write_ap(doc: &mut LoDoc, ap: Ap) -> ObjectId {
    let mut res = Dictionary::new();
    let mut body = String::from("q\n");
    if ap.opacity < 0.999 || ap.multiply {
        let mut gs = Dictionary::new();
        gs.set("Type", name("ExtGState"));
        gs.set("CA", real(ap.opacity.clamp(0.0, 1.0)));
        gs.set("ca", real(ap.opacity.clamp(0.0, 1.0)));
        if ap.multiply {
            gs.set("BM", name("Multiply"));
        }
        let mut egs = Dictionary::new();
        egs.set("GS0", LoObj::Dictionary(gs));
        res.set("ExtGState", LoObj::Dictionary(egs));
        body.push_str("/GS0 gs\n");
    }
    if !ap.fonts.is_empty() {
        let mut f = Dictionary::new();
        for (k, id) in &ap.fonts {
            f.set(k.as_bytes().to_vec(), LoObj::Reference(*id));
        }
        res.set("Font", LoObj::Dictionary(f));
    }
    if !ap.xobjects.is_empty() {
        let mut x = Dictionary::new();
        for (k, id) in &ap.xobjects {
            x.set(k.as_bytes().to_vec(), LoObj::Reference(*id));
        }
        res.set("XObject", LoObj::Dictionary(x));
    }
    body.push_str(&ap.content);
    body.push_str("Q\n");
    let mut xd = Dictionary::new();
    xd.set("Type", name("XObject"));
    xd.set("Subtype", name("Form"));
    xd.set("FormType", LoObj::Integer(1));
    xd.set("BBox", rect_obj(&ap.bbox));
    xd.set("Matrix", LoObj::Array([1, 0, 0, 1, 0, 0].iter().map(|v| LoObj::Integer(*v)).collect()));
    xd.set("Resources", LoObj::Dictionary(res));
    let mut st = lopdf::Stream::new(xd, body.into_bytes());
    let _ = st.compress();
    doc.add_object(LoObj::Stream(st))
}

fn set_ap(doc: &mut LoDoc, annot_id: ObjectId, ap_id: ObjectId) {
    if let Ok(d) = doc.get_object_mut(annot_id).and_then(|o| o.as_dict_mut()) {
        let mut apd = Dictionary::new();
        apd.set("N", LoObj::Reference(ap_id));
        d.set("AP", LoObj::Dictionary(apd));
        d.remove(b"AS");
    }
}

fn bounds_of(points: impl Iterator<Item = [f32; 2]>) -> Option<Rect> {
    let mut r: Option<Rect> = None;
    for [x, y] in points {
        r = Some(match r {
            None => Rect { left: x, bottom: y, right: x, top: y },
            Some(r) => Rect {
                left: r.left.min(x),
                bottom: r.bottom.min(y),
                right: r.right.max(x),
                top: r.top.max(y),
            },
        });
    }
    r
}

fn pad(r: Rect, p: f32) -> Rect {
    Rect { left: r.left - p, bottom: r.bottom - p, right: r.right + p, top: r.top + p }
}

const KAPPA: f32 = 0.552_284_8;

fn ellipse_path(r: &Rect) -> String {
    let cx = (r.left + r.right) / 2.0;
    let cy = (r.bottom + r.top) / 2.0;
    let rx = (r.right - r.left) / 2.0;
    let ry = (r.top - r.bottom) / 2.0;
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let f = fmt;
    format!(
        "{} {} m\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\nh\n",
        f(cx + rx), f(cy),
        f(cx + rx), f(cy + ky), f(cx + kx), f(cy + ry), f(cx), f(cy + ry),
        f(cx - kx), f(cy + ry), f(cx - rx), f(cy + ky), f(cx - rx), f(cy),
        f(cx - rx), f(cy - ky), f(cx - kx), f(cy - ry), f(cx), f(cy - ry),
        f(cx + kx), f(cy - ry), f(cx + rx), f(cy - ky), f(cx + rx), f(cy),
    )
}

fn poly_path(pts: &[[f32; 2]], close: bool) -> String {
    let mut s = String::new();
    for (i, p) in pts.iter().enumerate() {
        s.push_str(&format!("{} {} {}\n", fmt(p[0]), fmt(p[1]), if i == 0 { "m" } else { "l" }));
    }
    if close {
        s.push_str("h\n");
    }
    s
}

/// Đường viền "đám mây": các cung tròn lồi ra ngoài dọc theo đa giác.
fn cloud_path(pts: &[[f32; 2]], width: f32) -> String {
    let n = pts.len();
    if n < 3 {
        return poly_path(pts, true);
    }
    // Chiều quay của đa giác để cung luôn lồi ra ngoài.
    let area: f32 = (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let outward = if area >= 0.0 { 1.0 } else { -1.0 };
    let r_target = (4.0 + width * 2.0).max(5.0);
    let mut s = format!("{} {} m\n", fmt(pts[0][0]), fmt(pts[0][1]));
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.01 {
            continue;
        }
        let k = ((len / (r_target * 1.6)).round() as usize).max(1);
        // Pháp tuyến hướng ra ngoài.
        let (nx, ny) = (dy / len * outward, -dx / len * outward);
        for j in 0..k {
            let t0 = j as f32 / k as f32;
            let t1 = (j + 1) as f32 / k as f32;
            let p0 = [a[0] + dx * t0, a[1] + dy * t0];
            let p1 = [a[0] + dx * t1, a[1] + dy * t1];
            let seg = len / k as f32;
            let bulge = seg * 0.55;
            let c0 = [p0[0] + nx * bulge + dx / len * seg * 0.05, p0[1] + ny * bulge + dy / len * seg * 0.05];
            let c1 = [p1[0] + nx * bulge - dx / len * seg * 0.05, p1[1] + ny * bulge - dy / len * seg * 0.05];
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fmt(c0[0]), fmt(c0[1]), fmt(c1[0]), fmt(c1[1]), fmt(p1[0]), fmt(p1[1])
            ));
        }
    }
    s.push_str("h\n");
    s
}

fn arrow_len(width: f32) -> f32 {
    6.0 + width * 3.0
}

/// Đầu mũi tên tại `tip`, hướng từ `from` → `tip`. `closed` = tam giác kín.
fn arrow_head(from: [f32; 2], tip: [f32; 2], width: f32, closed: bool) -> String {
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    let l = arrow_len(width);
    let (c, s) = (0.5_f32.cos() * l, 0.5_f32.sin() * l); // ~28.6°
    let a = [tip[0] - ux * c + uy * s, tip[1] - uy * c - ux * s];
    let b = [tip[0] - ux * c - uy * s, tip[1] - uy * c + ux * s];
    format!(
        "{} {} m\n{} {} l\n{} {} l\n{}",
        fmt(a[0]), fmt(a[1]), fmt(tip[0]), fmt(tip[1]), fmt(b[0]), fmt(b[1]),
        if closed { "h\nB\n" } else { "S\n" }
    )
}

/// Hình học đọc từ dict annotation — dùng chung cho TẠO mới và DỰNG LẠI AP.
struct Geo {
    subtype: String,
    rect: Rect,
    color: Option<[u8; 3]>,
    fill: Option<[u8; 3]>,
    width: f32,
    opacity: f32,
    quads: Vec<[f32; 8]>,
    vertices: Vec<[f32; 2]>,
    ink: Vec<Vec<[f32; 2]>>,
    line: Option<[f32; 4]>,
    le: [String; 2],
    cloudy: bool,
}

fn pairs(v: &[f32]) -> Vec<[f32; 2]> {
    v.chunks(2).filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).collect()
}

fn border_width(doc: &LoDoc, d: &Dictionary) -> Option<f32> {
    if let Ok(bs) = d.get(b"BS").map(|o| deref(doc, o)) {
        if let Ok(bsd) = bs.as_dict() {
            if let Some(w) = bsd.get(b"W").ok().and_then(|o| num(deref(doc, o))) {
                return Some(w);
            }
            return Some(1.0);
        }
    }
    let b = dict_nums(doc, d, b"Border");
    if b.len() >= 3 {
        return Some(b[2]);
    }
    None
}

fn read_geo(doc: &LoDoc, d: &Dictionary) -> Geo {
    let subtype = dict_name(doc, d, b"Subtype").unwrap_or_default();
    let rect = rect_of(&dict_nums(doc, d, b"Rect"))
        .unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
    let quads = dict_nums(doc, d, b"QuadPoints")
        .chunks(8)
        .filter(|c| c.len() == 8)
        .map(|c| {
            let mut q = [0.0; 8];
            q.copy_from_slice(c);
            q
        })
        .collect();
    let ink = match d.get(b"InkList").map(|o| deref(doc, o)) {
        Ok(LoObj::Array(strokes)) => strokes.iter().map(|s| pairs(&nums(doc, s))).collect(),
        _ => Vec::new(),
    };
    let l = dict_nums(doc, d, b"L");
    let le = match d.get(b"LE").map(|o| deref(doc, o)) {
        Ok(LoObj::Array(a)) if a.len() >= 2 => {
            let n = |o: &LoObj| deref(doc, o).as_name().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_else(|_| "None".into());
            [n(&a[0]), n(&a[1])]
        }
        _ => ["None".into(), "None".into()],
    };
    let cloudy = match d.get(b"BE").map(|o| deref(doc, o)) {
        Ok(LoObj::Dictionary(be)) => matches!(be.get(b"S"), Ok(LoObj::Name(n)) if n == b"C"),
        _ => false,
    };
    Geo {
        subtype,
        rect,
        color: color_of(&dict_nums(doc, d, b"C")),
        fill: color_of(&dict_nums(doc, d, b"IC")),
        width: border_width(doc, d).unwrap_or(1.0),
        opacity: d.get(b"CA").ok().and_then(|o| num(deref(doc, o))).unwrap_or(1.0),
        quads,
        vertices: pairs(&dict_nums(doc, d, b"Vertices")),
        ink,
        line: if l.len() >= 4 { Some([l[0], l[1], l[2], l[3]]) } else { None },
        le,
        cloudy,
    }
}

/// Các loại có AP dựng lại được hoàn toàn từ hình học trong dict.
fn geo_regenerable(subtype: &str) -> bool {
    matches!(
        subtype,
        "Square" | "Circle" | "Line" | "PolyLine" | "Polygon" | "Ink" | "Highlight" | "Underline"
            | "StrikeOut" | "Squiggly"
    )
}

/// Dựng AP từ hình học. Trả về (AP, Rect đề xuất bao trọn nét vẽ).
fn geo_ap(g: &Geo) -> Option<(Ap, Rect)> {
    let w = g.width.max(0.0);
    let stroke = g.color;
    let mut s = String::new();
    let rect: Rect;
    let mut multiply = false;
    let line_setup = |s: &mut String| {
        if let Some(c) = stroke {
            s.push_str(&rgb_ops(c, true));
        }
        if let Some(f) = g.fill {
            s.push_str(&rgb_ops(f, false));
        }
        s.push_str(&format!("{} w\n", fmt(w)));
    };
    let paint = |has_fill: bool| -> &'static str {
        match (stroke.is_some() && w > 0.0, has_fill) {
            (true, true) => "B\n",
            (true, false) => "S\n",
            (false, true) => "f\n",
            (false, false) => "n\n",
        }
    };
    match g.subtype.as_str() {
        "Square" | "Circle" => {
            rect = g.rect;
            let inner = pad(rect, -w / 2.0);
            if inner.right <= inner.left || inner.top <= inner.bottom {
                return None;
            }
            line_setup(&mut s);
            if g.subtype == "Square" {
                s.push_str(&format!(
                    "{} {} {} {} re\n",
                    fmt(inner.left), fmt(inner.bottom), fmt(inner.right - inner.left), fmt(inner.top - inner.bottom)
                ));
            } else {
                s.push_str(&ellipse_path(&inner));
            }
            s.push_str(paint(g.fill.is_some()));
        }
        "Line" => {
            let l = g.line?;
            let (p0, p1) = ([l[0], l[1]], [l[2], l[3]]);
            let head = |n: &str| matches!(n, "OpenArrow" | "ClosedArrow");
            let extra = if head(&g.le[0]) || head(&g.le[1]) { arrow_len(w) } else { 0.0 };
            rect = pad(bounds_of([p0, p1].into_iter())?, w / 2.0 + extra + 1.0);
            s.push_str("1 J 1 j\n");
            line_setup(&mut s);
            if let Some(c) = stroke {
                // Mũi tên kín tô bằng /IC, không có thì dùng màu nét.
                if g.fill.is_none() {
                    s.push_str(&rgb_ops(c, false));
                }
            }
            s.push_str(&format!("{} {} m\n{} {} l\nS\n", fmt(p0[0]), fmt(p0[1]), fmt(p1[0]), fmt(p1[1])));
            for (end, (from, tip)) in [(&g.le[0], (p1, p0)), (&g.le[1], (p0, p1))] {
                match end.as_str() {
                    "OpenArrow" => s.push_str(&arrow_head(from, tip, w, false)),
                    "ClosedArrow" => s.push_str(&arrow_head(from, tip, w, true)),
                    _ => {}
                }
            }
        }
        "PolyLine" | "Polygon" => {
            if g.vertices.len() < 2 {
                return None;
            }
            let close = g.subtype == "Polygon";
            let cloud_pad = if g.cloudy { 6.0 + w * 3.0 } else { 0.0 };
            rect = pad(bounds_of(g.vertices.iter().copied())?, w / 2.0 + cloud_pad + 1.0);
            s.push_str("1 J 1 j\n");
            line_setup(&mut s);
            if close && g.cloudy {
                s.push_str(&cloud_path(&g.vertices, w));
            } else {
                s.push_str(&poly_path(&g.vertices, close));
            }
            s.push_str(paint(close && g.fill.is_some()));
        }
        "Ink" => {
            let all = g.ink.iter().flatten().copied();
            rect = pad(bounds_of(all)?, w / 2.0 + 1.0);
            s.push_str("1 J 1 j\n");
            line_setup(&mut s);
            for st in &g.ink {
                if st.len() == 1 {
                    // Chấm 1 điểm: đoạn thẳng dài 0 với đầu tròn.
                    s.push_str(&format!("{0} {1} m\n{0} {1} l\n", fmt(st[0][0]), fmt(st[0][1])));
                } else {
                    s.push_str(&poly_path(st, false));
                }
            }
            s.push_str("S\n");
        }
        "Highlight" | "Underline" | "StrikeOut" | "Squiggly" => {
            if g.quads.is_empty() {
                return None;
            }
            let c = stroke.unwrap_or([255, 230, 0]);
            let all = g.quads.iter().flat_map(|q| pairs(q));
            rect = pad(bounds_of(all)?, 1.0);
            for q in &g.quads {
                // QuadPoints: (x1,y1)=TL (x2,y2)=TR (x3,y3)=BL (x4,y4)=BR.
                let (tl, tr, bl, br) = ([q[0], q[1]], [q[2], q[3]], [q[4], q[5]], [q[6], q[7]]);
                let h = ((tl[0] - bl[0]).powi(2) + (tl[1] - bl[1]).powi(2)).sqrt();
                let lerp = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                match g.subtype.as_str() {
                    "Highlight" => {
                        multiply = true;
                        s.push_str(&rgb_ops(c, false));
                        s.push_str(&poly_path(&[tl, tr, br, bl], true));
                        s.push_str("f\n");
                    }
                    "Underline" | "StrikeOut" => {
                        let lw = (h / 14.0).max(0.8);
                        let t = if g.subtype == "Underline" { 0.92 } else { 0.5 };
                        let a = lerp(tl, bl, t);
                        let b = lerp(tr, br, t);
                        s.push_str(&rgb_ops(c, true));
                        s.push_str(&format!("{} w\n{} {} m\n{} {} l\nS\n", fmt(lw), fmt(a[0]), fmt(a[1]), fmt(b[0]), fmt(b[1])));
                    }
                    _ => {
                        // Squiggly: răng cưa sát đáy dòng.
                        let lw = (h / 18.0).max(0.6);
                        let amp = (h / 12.0).max(1.0);
                        let a = lerp(tl, bl, 0.9);
                        let b = lerp(tr, br, 0.9);
                        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                        let n = ((len / (amp * 2.0)).ceil() as usize).max(1);
                        s.push_str(&rgb_ops(c, true));
                        s.push_str(&format!("{} w\n", fmt(lw)));
                        for i in 0..=n {
                            let p = lerp(a, b, i as f32 / n as f32);
                            let off = if i % 2 == 0 { -amp / 2.0 } else { amp / 2.0 };
                            s.push_str(&format!("{} {} {}\n", fmt(p[0]), fmt(p[1] + off), if i == 0 { "m" } else { "l" }));
                        }
                        s.push_str("S\n");
                    }
                }
            }
        }
        _ => return None,
    }
    let mut ap = Ap::new(rect, g.opacity);
    ap.multiply = multiply;
    ap.content = s;
    Some((ap, rect))
}

/// Dựng lại AP của annotation `id` theo hình học hiện có; cập nhật /Rect cho
/// khớp nét vẽ. Trả về false nếu loại này không tự dựng được.
fn regen_geo_ap(doc: &mut LoDoc, id: ObjectId) -> Result<bool, EngineError> {
    let geo = {
        let d = doc.get_dictionary(id).map_err(perr("đọc annotation"))?;
        read_geo(doc, d)
    };
    if !geo_regenerable(&geo.subtype) {
        return Ok(false);
    }
    let Some((ap, rect)) = geo_ap(&geo) else { return Ok(false) };
    let ap_id = write_ap(doc, ap);
    if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
        d.set("Rect", rect_obj(&rect));
    }
    set_ap(doc, id, ap_id);
    Ok(true)
}

// ======================= Con dấu =======================

/// Rounded-rect path (toạ độ BBox của AP).
fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> String {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = r * (1.0 - KAPPA);
    let f = fmt;
    format!(
        "{} {} m\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\nh\n",
        f(x + r), f(y),
        f(x + w - r), f(y),
        f(x + w - k), f(y), f(x + w), f(y + k), f(x + w), f(y + r),
        f(x + w), f(y + h - r),
        f(x + w), f(y + h - k), f(x + w - k), f(y + h), f(x + w - r), f(y + h),
        f(x + r), f(y + h),
        f(x + k), f(y + h), f(x), f(y + h - k), f(x), f(y + h - r),
        f(x), f(y + r),
        f(x), f(y + k), f(x + k), f(y), f(x + r), f(y),
    )
}

/// Font dùng vẽ chữ con dấu: TTF Unicode (tiếng Việt) nếu có, không thì Helvetica-Bold.
enum StampFont {
    Ttf { id: ObjectId, bytes: Vec<u8> },
    Base14 { id: ObjectId },
}

impl StampFont {
    fn width(&self, text: &str, fs: f32) -> f32 {
        match self {
            StampFont::Ttf { bytes, .. } => match ttf_parser::Face::parse(bytes, 0) {
                Ok(face) => text.chars().map(|c| char_adv(c, &face, fs)).sum(),
                Err(_) => text.chars().count() as f32 * fs * 0.62,
            },
            StampFont::Base14 { .. } => text.chars().count() as f32 * fs * 0.64,
        }
    }
    fn show(&self, text: &str) -> String {
        match self {
            StampFont::Ttf { bytes, .. } => match ttf_parser::Face::parse(bytes, 0) {
                Ok(face) => {
                    let hex: String = encode_cid(text, &face).iter().map(|b| format!("{b:02X}")).collect();
                    format!("<{hex}> Tj\n")
                }
                Err(_) => "() Tj\n".into(),
            },
            StampFont::Base14 { .. } => {
                let mut s = String::from("(");
                for ch in text.chars() {
                    let b = if (ch as u32) < 256 { ch as u32 as u8 } else { b'?' };
                    match b {
                        b'(' | b')' | b'\\' => {
                            s.push('\\');
                            s.push(b as char);
                        }
                        0x20..=0x7E => s.push(b as char),
                        _ => s.push_str(&format!("\\{b:03o}")),
                    }
                }
                s.push_str(") Tj\n");
                s
            }
        }
    }
    fn id(&self) -> ObjectId {
        match self {
            StampFont::Ttf { id, .. } | StampFont::Base14 { id } => *id,
        }
    }
}

fn stamp_font(doc: &mut LoDoc, text: &str) -> StampFont {
    if let Some(bytes) = find_font_bytes(true, false) {
        if let Ok(id) = embed_type0_font(doc, &bytes, text) {
            return StampFont::Ttf { id, bytes };
        }
    }
    let mut d = Dictionary::new();
    d.set("Type", name("Font"));
    d.set("Subtype", name("Type1"));
    d.set("BaseFont", name("Helvetica-Bold"));
    d.set("Encoding", name("WinAnsiEncoding"));
    StampFont::Base14 { id: doc.add_object(LoObj::Dictionary(d)) }
}

/// AP con dấu chữ: khung bo góc + chữ căn giữa (1 hoặc 2 dòng), màu `color`.
fn text_stamp_ap(doc: &mut LoDoc, st: &StampSpec, color: [u8; 3], w: f32, h: f32, opacity: f32) -> Ap {
    let all = format!("{}{}", st.label, st.sub.clone().unwrap_or_default());
    let font = stamp_font(doc, &all);
    let bw = (h * 0.06).clamp(1.0, 4.0);
    let padx = bw + h * 0.18;
    let mut s = String::new();
    s.push_str(&rgb_ops(color, true));
    s.push_str(&rgb_ops(color, false));
    s.push_str(&format!("{} w\n", fmt(bw)));
    s.push_str(&round_rect(bw / 2.0, bw / 2.0, w - bw, h - bw, (h * 0.18).min(10.0)));
    s.push_str("S\n");
    let fit = |text: &str, want: f32| -> f32 {
        let w1 = font.width(text, 1.0).max(0.01);
        want.min((w - 2.0 * padx) / w1).max(1.0)
    };
    let cap = 0.72;
    match st.sub.as_deref().filter(|x| !x.is_empty()) {
        None => {
            let fs = fit(&st.label, h * 0.58);
            let tw = font.width(&st.label, fs);
            let y = (h - fs * cap) / 2.0;
            s.push_str(&format!("BT\n/F1 {} Tf\n{} {} Td\n", fmt(fs), fmt((w - tw) / 2.0), fmt(y)));
            s.push_str(&font.show(&st.label));
            s.push_str("ET\n");
        }
        Some(sub) => {
            let lf = fit(&st.label, h * 0.44);
            let sf = fit(sub, h * 0.2);
            let gap = h * 0.1;
            let total = lf * cap + gap + sf * cap;
            let y0 = (h - total) / 2.0;
            let tw = font.width(&st.label, lf);
            let sw = font.width(sub, sf);
            s.push_str(&format!("BT\n/F1 {} Tf\n{} {} Td\n", fmt(lf), fmt((w - tw) / 2.0), fmt(y0 + sf * cap + gap)));
            s.push_str(&font.show(&st.label));
            s.push_str("ET\n");
            s.push_str(&format!("BT\n/F1 {} Tf\n{} {} Td\n", fmt(sf), fmt((w - sw) / 2.0), fmt(y0)));
            s.push_str(&font.show(sub));
            s.push_str("ET\n");
        }
    }
    let mut ap = Ap::new(Rect { left: 0.0, bottom: 0.0, right: w, top: h }, opacity);
    ap.content = s;
    ap.fonts.push(("F1".into(), font.id()));
    ap
}

/// Nhúng ảnh PNG/JPG làm Image XObject (alpha → /SMask).
fn embed_image(doc: &mut LoDoc, path: &Path) -> Result<(ObjectId, u32, u32), EngineError> {
    let img = image::open(path)
        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh con dấu {}: {e}", path.display())))?
        .to_rgba8();
    let (iw, ih) = img.dimensions();
    let mut rgb = Vec::with_capacity((iw * ih * 3) as usize);
    let mut alpha = Vec::with_capacity((iw * ih) as usize);
    let mut has_alpha = false;
    for p in img.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
        has_alpha |= p.0[3] != 255;
    }
    let img_dict = |cs: &str| {
        let mut d = Dictionary::new();
        d.set("Type", name("XObject"));
        d.set("Subtype", name("Image"));
        d.set("Width", LoObj::Integer(iw as i64));
        d.set("Height", LoObj::Integer(ih as i64));
        d.set("ColorSpace", name(cs));
        d.set("BitsPerComponent", LoObj::Integer(8));
        d
    };
    let mut main = img_dict("DeviceRGB");
    if has_alpha {
        let mut sm = lopdf::Stream::new(img_dict("DeviceGray"), alpha);
        let _ = sm.compress();
        let sm_id = doc.add_object(LoObj::Stream(sm));
        main.set("SMask", LoObj::Reference(sm_id));
    }
    let mut st = lopdf::Stream::new(main, rgb);
    let _ = st.compress();
    Ok((doc.add_object(LoObj::Stream(st)), iw, ih))
}

// ======================= Tạo annotation mới =======================

fn base_dict(subtype: &str, page: ObjectId, spec: &ShapeSpec) -> Dictionary {
    let mut d = Dictionary::new();
    d.set("Type", name("Annot"));
    d.set("Subtype", name(subtype));
    d.set("F", LoObj::Integer(4)); // Print
    d.set("P", LoObj::Reference(page));
    d.set("C", color_obj(spec.color));
    if spec.opacity < 0.999 {
        d.set("CA", real(spec.opacity.clamp(0.0, 1.0)));
    }
    let mut bs = Dictionary::new();
    bs.set("Type", name("Border"));
    bs.set("W", real(spec.width.max(0.0)));
    bs.set("S", name("S"));
    d.set("BS", LoObj::Dictionary(bs));
    if let Some(c) = spec.contents.as_deref().filter(|s| !s.is_empty()) {
        d.set("Contents", pdf_text_string(c));
    }
    d
}

fn point_array(pts: &[[f32; 2]]) -> LoObj {
    LoObj::Array(pts.iter().flat_map(|p| [real(p[0]), real(p[1])]).collect())
}

fn build_shape(doc: &mut LoDoc, page: ObjectId, spec: &ShapeSpec) -> Result<ObjectId, EngineError> {
    let bad = |m: &str| EngineError::Pdfium(format!("chú thích {:?}: {m}", spec.kind));
    let subtype = match spec.kind {
        ShapeKind::Ink => "Ink",
        ShapeKind::Line | ShapeKind::Arrow => "Line",
        ShapeKind::Square => "Square",
        ShapeKind::Circle => "Circle",
        ShapeKind::Polygon | ShapeKind::Cloud => "Polygon",
        ShapeKind::PolyLine => "PolyLine",
        ShapeKind::Stamp => "Stamp",
    };
    let mut d = base_dict(subtype, page, spec);
    match spec.kind {
        ShapeKind::Ink => {
            let strokes: Vec<&Vec<[f32; 2]>> = spec.strokes.iter().filter(|s| !s.is_empty()).collect();
            if strokes.is_empty() {
                return Err(bad("không có nét vẽ"));
            }
            d.set("InkList", LoObj::Array(strokes.iter().map(|s| point_array(s)).collect()));
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            if spec.points.len() < 2 {
                return Err(bad("cần 2 điểm"));
            }
            let (a, b) = (spec.points[0], spec.points[spec.points.len() - 1]);
            d.set("L", point_array(&[a, b]));
            let end = if spec.kind == ShapeKind::Arrow { "OpenArrow" } else { "None" };
            d.set("LE", LoObj::Array(vec![name("None"), name(end)]));
        }
        ShapeKind::Square | ShapeKind::Circle => {
            if spec.rect.right - spec.rect.left < 0.5 || spec.rect.top - spec.rect.bottom < 0.5 {
                return Err(bad("khung quá nhỏ"));
            }
            d.set("Rect", rect_obj(&spec.rect));
            if let Some(f) = spec.fill {
                d.set("IC", color_obj(f));
            }
        }
        ShapeKind::Polygon | ShapeKind::Cloud | ShapeKind::PolyLine => {
            let need = if spec.kind == ShapeKind::PolyLine { 2 } else { 3 };
            if spec.points.len() < need {
                return Err(bad("không đủ đỉnh"));
            }
            d.set("Vertices", point_array(&spec.points));
            if spec.kind != ShapeKind::PolyLine {
                if let Some(f) = spec.fill {
                    d.set("IC", color_obj(f));
                }
            }
            if spec.kind == ShapeKind::Cloud {
                let mut be = Dictionary::new();
                be.set("S", name("C"));
                be.set("I", LoObj::Integer(1));
                d.set("BE", LoObj::Dictionary(be));
            }
        }
        ShapeKind::Stamp => {}
    }

    if spec.kind == ShapeKind::Stamp {
        let st = spec.stamp.clone().unwrap_or_default();
        let r = spec.rect;
        let (w, h) = (r.right - r.left, r.top - r.bottom);
        if w < 1.0 || h < 1.0 {
            return Err(bad("khung quá nhỏ"));
        }
        d.set("Rect", rect_obj(&r));
        d.remove(b"BS");
        let icon = if st.name.is_empty() { "Draft".to_string() } else { st.name.clone() };
        d.set("Name", LoObj::Name(icon.into_bytes()));
        if !st.label.is_empty() {
            let subj = match &st.sub {
                Some(s) if !s.is_empty() => format!("{} {}", st.label, s),
                _ => st.label.clone(),
            };
            d.set("Subj", pdf_text_string(&subj));
        }
        let ap = if let Some(img) = &st.image {
            let (img_id, _, _) = embed_image(doc, img)?;
            let mut ap = Ap::new(Rect { left: 0.0, bottom: 0.0, right: w, top: h }, spec.opacity);
            ap.content = format!("{} 0 0 {} 0 0 cm\n/Im0 Do\n", fmt(w), fmt(h));
            ap.xobjects.push(("Im0".into(), img_id));
            ap
        } else {
            text_stamp_ap(doc, &st, spec.color, w, h, spec.opacity)
        };
        let ap_id = write_ap(doc, ap);
        let id = doc.add_object(LoObj::Dictionary(d));
        set_ap(doc, id, ap_id);
        return Ok(id);
    }

    let geo = read_geo(doc, &d);
    let (ap, rect) = geo_ap(&geo).ok_or_else(|| bad("không dựng được hình"))?;
    d.set("Rect", rect_obj(&rect));
    let ap_id = write_ap(doc, ap);
    let id = doc.add_object(LoObj::Dictionary(d));
    set_ap(doc, id, ap_id);
    Ok(id)
}

/// Thêm các chú thích vẽ tự do vào `doc` (đã mở). Trả về id các object mới.
fn add_shapes(doc: &mut LoDoc, shapes: &[ShapeSpec], meta: &AnnotMeta, date: &str) -> Result<Vec<ObjectId>, EngineError> {
    let mut out = Vec::new();
    for spec in shapes {
        let pid = page_id(doc, spec.page_index)?;
        let id = build_shape(doc, pid, spec)?;
        if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
            stamp_meta(d, meta, date);
        }
        attach_annot(doc, pid, id)?;
        out.push(id);
    }
    Ok(out)
}

/// Tạo các chú thích vẽ tự do rồi lưu sang `output` (không sửa `input`).
pub fn apply_shape_annotations(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    shapes: &[ShapeSpec],
    meta: &AnnotMeta,
) -> Result<(), EngineError> {
    let mut doc = load_lo(pdfium, input)?;
    let date = meta_date(meta);
    add_shapes(&mut doc, shapes, meta, &date)?;
    doc.save(output).map_err(perr("lopdf save"))?;
    Ok(())
}

// ======================= Đọc chi tiết =======================

/// Liệt kê chú thích (bỏ Popup/Link/Widget — như danh sách Comments của Foxit)
/// kèm đủ thông tin để hiển thị & sửa.
pub fn list_annotations_detailed(pdfium: &Pdfium, input: &Path) -> Result<Vec<AnnotDetail>, EngineError> {
    let mut doc = load_lo(pdfium, input)?;
    let pages = doc.get_pages();
    let mut out = Vec::new();
    for (pno, pid) in pages {
        let ids = annots_ids(&mut doc, pid)?;
        let index_of: BTreeMap<ObjectId, usize> =
            ids.iter().enumerate().filter_map(|(i, id)| id.map(|id| (id, i))).collect();
        for (i, id) in ids.iter().enumerate() {
            let Some(id) = id else { continue };
            let Ok(d) = doc.get_dictionary(*id) else { continue };
            let subtype = dict_name(&doc, d, b"Subtype").unwrap_or_default();
            if matches!(subtype.as_str(), "Popup" | "Link" | "Widget" | "") {
                continue;
            }
            let g = read_geo(&doc, d);
            let flags = d.get(b"F").ok().and_then(|o| deref(&doc, o).as_i64().ok()).unwrap_or(0);
            let font_size = dict_text(&doc, d, b"DA").and_then(|da| {
                let toks: Vec<&str> = da.split_whitespace().collect();
                toks.iter().position(|t| *t == "Tf").and_then(|p| p.checked_sub(1)).and_then(|p| toks[p].parse().ok())
            });
            // FreeText: màu chữ nằm ở /DA (… r g b rg), /C là màu nền.
            let mut color = g.color;
            if subtype == "FreeText" {
                if let Some(da) = dict_text(&doc, d, b"DA") {
                    let toks: Vec<&str> = da.split_whitespace().collect();
                    if let Some(p) = toks.iter().rposition(|t| *t == "rg") {
                        if p >= 3 {
                            let v: Vec<f32> = toks[p - 3..p].iter().filter_map(|t| t.parse().ok()).collect();
                            color = color_of(&v).or(color);
                        }
                    } else if let Some(p) = toks.iter().rposition(|t| *t == "g") {
                        if p >= 1 {
                            let v: Vec<f32> = toks[p - 1..p].iter().filter_map(|t| t.parse().ok()).collect();
                            color = color_of(&v).or(color);
                        }
                    }
                }
            }
            let irt = match d.get(b"IRT") {
                Ok(LoObj::Reference(r)) => index_of.get(r).copied(),
                _ => None,
            };
            out.push(AnnotDetail {
                page_index: (pno - 1) as u16,
                annot_index: i,
                subtype: subtype.clone(),
                rect: g.rect,
                color,
                fill: g.fill,
                width: border_width(&doc, d),
                opacity: g.opacity,
                contents: dict_text(&doc, d, b"Contents"),
                author: dict_text(&doc, d, b"T"),
                subject: dict_text(&doc, d, b"Subj"),
                modified: dict_text(&doc, d, b"M"),
                created: dict_text(&doc, d, b"CreationDate"),
                nm: dict_text(&doc, d, b"NM"),
                quads: g
                    .quads
                    .iter()
                    .filter_map(|q| bounds_of(pairs(q).into_iter()))
                    .collect(),
                vertices: g.vertices.clone(),
                ink: g.ink.clone(),
                line: g.line,
                line_endings: if subtype == "Line" { Some(g.le.clone()) } else { None },
                font_size,
                icon: dict_name(&doc, d, b"Name"),
                in_reply_to: irt,
                hidden: flags & 2 != 0,
                cloudy: g.cloudy,
            });
        }
    }
    Ok(out)
}

// ======================= Sửa / xoá annotation có sẵn =======================

/// Ánh xạ affine từ khung cũ sang khung mới.
fn map_fn(old: Rect, new: Rect) -> impl Fn(f32, f32) -> (f32, f32) {
    let ow = old.right - old.left;
    let oh = old.top - old.bottom;
    let sx = if ow.abs() > 0.01 { (new.right - new.left) / ow } else { 1.0 };
    let sy = if oh.abs() > 0.01 { (new.top - new.bottom) / oh } else { 1.0 };
    move |x, y| (new.left + (x - old.left) * sx, new.bottom + (y - old.bottom) * sy)
}

fn map_points_key(doc: &mut LoDoc, id: ObjectId, key: &[u8], f: &dyn Fn(f32, f32) -> (f32, f32)) {
    let v = {
        let Ok(d) = doc.get_dictionary(id) else { return };
        if !d.has(key) {
            return;
        }
        dict_nums(doc, d, key)
    };
    let mut out = Vec::with_capacity(v.len());
    for c in v.chunks(2) {
        if c.len() == 2 {
            let (x, y) = f(c[0], c[1]);
            out.push(real(x));
            out.push(real(y));
        }
    }
    if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
        d.set(key.to_vec(), LoObj::Array(out));
    }
}

fn map_ink(doc: &mut LoDoc, id: ObjectId, f: &dyn Fn(f32, f32) -> (f32, f32)) {
    let strokes = {
        let Ok(d) = doc.get_dictionary(id) else { return };
        match d.get(b"InkList").map(|o| deref(doc, o)) {
            Ok(LoObj::Array(a)) => a.iter().map(|s| nums(doc, s)).collect::<Vec<_>>(),
            _ => return,
        }
    };
    let arr = strokes
        .iter()
        .map(|s| {
            LoObj::Array(
                s.chunks(2)
                    .filter(|c| c.len() == 2)
                    .flat_map(|c| {
                        let (x, y) = f(c[0], c[1]);
                        [real(x), real(y)]
                    })
                    .collect(),
            )
        })
        .collect();
    if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
        d.set("InkList", LoObj::Array(arr));
    }
}

fn da_font_size(da: &str) -> Option<f32> {
    let toks: Vec<&str> = da.split_whitespace().collect();
    toks.iter().position(|t| *t == "Tf").and_then(|p| p.checked_sub(1)).and_then(|p| toks[p].parse().ok())
}

/// Dựng lại AP của FreeText (nội dung/màu/khung đổi) bằng font Unicode hệ thống.
fn regen_freetext(doc: &mut LoDoc, id: ObjectId) -> Result<(), EngineError> {
    let (rect, contents, color, fs) = {
        let d = doc.get_dictionary(id).map_err(perr("đọc FreeText"))?;
        let rect = rect_of(&dict_nums(doc, d, b"Rect")).unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
        let da = dict_text(doc, d, b"DA").unwrap_or_default();
        let toks: Vec<&str> = da.split_whitespace().collect();
        let color = toks
            .iter()
            .rposition(|t| *t == "rg")
            .filter(|p| *p >= 3)
            .and_then(|p| color_of(&toks[p - 3..p].iter().filter_map(|t| t.parse().ok()).collect::<Vec<f32>>()))
            .unwrap_or([0, 0, 0]);
        (rect, dict_text(doc, d, b"Contents").unwrap_or_default(), color, da_font_size(&da).unwrap_or(12.0))
    };
    let Some(bytes) = find_font_bytes(false, false) else { return Ok(()) };
    let font0 = embed_type0_font(doc, &bytes, &contents)?;
    let spec = AnnotSpec {
        kind: AnnotKind::FreeText,
        page_index: 0,
        rect,
        quads: Vec::new(),
        color: [color[0], color[1], color[2], 255],
        contents: Some(contents),
        font_size: fs,
        bold: false,
        italic: false,
        underline: false,
    };
    let ap_id = build_unicode_ap(doc, &spec, font0, &bytes)?;
    // AP FreeText của annot.rs có BBox gốc 0,0 — Matrix đơn vị, ánh xạ vào Rect.
    let da = format!(
        "/F0 {} Tf {} {} {} rg",
        fmt(fs),
        fmt(color[0] as f32 / 255.0),
        fmt(color[1] as f32 / 255.0),
        fmt(color[2] as f32 / 255.0)
    );
    if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
        let mut f = Dictionary::new();
        f.set("F0", LoObj::Reference(font0));
        let mut dr = Dictionary::new();
        dr.set("Font", LoObj::Dictionary(f));
        d.set("DR", LoObj::Dictionary(dr));
        d.set("DA", LoObj::String(da.into_bytes(), StringFormat::Literal));
        d.remove(b"RC");
    }
    set_ap(doc, id, ap_id);
    Ok(())
}

fn apply_update(doc: &mut LoDoc, id: ObjectId, u: &AnnotUpdate, date: &str) -> Result<(), EngineError> {
    let (subtype, old_rect, popup) = {
        let d = doc.get_dictionary(id).map_err(perr("đọc annotation"))?;
        let r = rect_of(&dict_nums(doc, d, b"Rect")).unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
        let popup = match d.get(b"Popup") {
            Ok(LoObj::Reference(p)) => Some(*p),
            _ => None,
        };
        (dict_name(doc, d, b"Subtype").unwrap_or_default(), r, popup)
    };
    let mut resized = false;
    let mut changed_look = false;

    if let Some(nr) = u.rect {
        let nr = Rect {
            left: nr.left.min(nr.right),
            bottom: nr.bottom.min(nr.top),
            right: nr.left.max(nr.right),
            top: nr.bottom.max(nr.top),
        };
        let f = map_fn(old_rect, nr);
        for key in [&b"QuadPoints"[..], &b"Vertices"[..], &b"L"[..], &b"CL"[..]] {
            map_points_key(doc, id, key, &f);
        }
        map_ink(doc, id, &f);
        resized = ((nr.right - nr.left) - (old_rect.right - old_rect.left)).abs() > 0.05
            || ((nr.top - nr.bottom) - (old_rect.top - old_rect.bottom)).abs() > 0.05;
        if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
            d.set("Rect", rect_obj(&nr));
            // /RD (lề trong) giữ nguyên tỷ lệ không cần thiết: bỏ để AP khớp Rect.
            if resized {
                d.remove(b"RD");
            }
        }
        // Popup đi theo annotation (dời cùng độ lệch).
        if let Some(pp) = popup {
            let (dx, dy) = (nr.left - old_rect.left, nr.top - old_rect.top);
            let pr = doc.get_dictionary(pp).ok().and_then(|pd| rect_of(&dict_nums(doc, pd, b"Rect")));
            if let (Some(pr), Ok(pd)) = (pr, doc.get_object_mut(pp).and_then(|o| o.as_dict_mut())) {
                pd.set(
                    "Rect",
                    rect_obj(&Rect { left: pr.left + dx, bottom: pr.bottom + dy, right: pr.right + dx, top: pr.top + dy }),
                );
            }
        }
    }

    {
        let d = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(perr("sửa annotation"))?;
        if let Some(c) = u.color {
            changed_look = true;
            if subtype == "FreeText" {
                // Màu chữ FreeText nằm trong /DA — thay 3 số trước "rg".
                let da = d
                    .get(b"DA")
                    .ok()
                    .and_then(|o| o.as_str().ok())
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_else(|| "/Helv 12 Tf 0 0 0 rg".into());
                let mut toks: Vec<String> = da.split_whitespace().map(String::from).collect();
                let rgb = [c[0], c[1], c[2]].map(|v| fmt(v as f32 / 255.0));
                if let Some(p) = toks.iter().rposition(|t| t == "rg").filter(|p| *p >= 3) {
                    toks.splice(p - 3..p, rgb);
                } else if let Some(p) = toks.iter().rposition(|t| t == "g").filter(|p| *p >= 1) {
                    toks.splice(p - 1..=p, rgb.into_iter().chain(["rg".to_string()]));
                } else {
                    toks.extend(rgb.into_iter().chain(["rg".to_string()]));
                }
                d.set("DA", LoObj::String(toks.join(" ").into_bytes(), StringFormat::Literal));
            } else {
                d.set("C", color_obj(c));
            }
        }
        if let Some(fill) = u.fill {
            changed_look = true;
            match fill {
                Some(f) => d.set("IC", color_obj(f)),
                None => {
                    d.remove(b"IC");
                }
            }
        }
        if let Some(w) = u.width {
            changed_look = true;
            let mut bs = match d.get(b"BS") {
                Ok(LoObj::Dictionary(b)) => b.clone(),
                _ => {
                    let mut b = Dictionary::new();
                    b.set("S", name("S"));
                    b
                }
            };
            bs.set("W", real(w.max(0.0)));
            d.set("BS", LoObj::Dictionary(bs));
            d.remove(b"Border");
        }
        if let Some(o) = u.opacity {
            changed_look = true;
            d.set("CA", real(o.clamp(0.0, 1.0)));
        }
        if let Some(c) = &u.contents {
            d.set("Contents", pdf_text_string(c));
        }
        if let Some(a) = &u.author {
            d.set("T", pdf_text_string(a));
        }
        d.set("M", LoObj::String(date.as_bytes().to_vec(), StringFormat::Literal));
    }

    let content_changed = u.contents.is_some();
    if geo_regenerable(&subtype) {
        if u.rect.is_some() || changed_look {
            regen_geo_ap(doc, id)?;
        }
    } else if subtype == "FreeText" {
        if resized || u.color.is_some() || content_changed {
            regen_freetext(doc, id)?;
        }
    } else if subtype == "Text" && u.color.is_some() {
        // Icon ghi chú: bỏ AP cũ để viewer vẽ lại icon theo /C mới.
        if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
            d.remove(b"AP");
        }
    }
    // Loại khác (Stamp, ...): AP giữ nguyên (co giãn theo /Rect), /CA đã ghi.
    Ok(())
}

/// Áp các thay đổi + xoá lên `doc` đã mở. Mọi đích được phân giải thành
/// object id TRƯỚC khi sửa/xoá để chỉ số trong /Annots không lệch.
fn modify_doc(doc: &mut LoDoc, updates: &[AnnotUpdate], deletes: &[AnnotRef], date: &str) -> Result<(), EngineError> {
    let mut upd_ids = Vec::with_capacity(updates.len());
    for u in updates {
        upd_ids.push(resolve_ref(doc, &u.target)?);
    }
    let mut del: Vec<(u16, ObjectId)> = Vec::with_capacity(deletes.len());
    for r in deletes {
        del.push((r.page_index, resolve_ref(doc, r)?));
    }
    let del_set: HashSet<ObjectId> = del.iter().map(|(_, id)| *id).collect();
    for (u, id) in updates.iter().zip(upd_ids) {
        if del_set.contains(&id) {
            continue;
        }
        apply_update(doc, id, u, date)?;
    }
    // Xoá: annotation + popup của nó + các trả lời (/IRT) cùng popup của chúng.
    let mut by_page: BTreeMap<u16, HashSet<ObjectId>> = BTreeMap::new();
    for (p, id) in &del {
        by_page.entry(*p).or_default().insert(*id);
    }
    for (p, mut set) in by_page {
        let pid = page_id(doc, p)?;
        let ids: Vec<ObjectId> = annots_ids(doc, pid)?.into_iter().flatten().collect();
        loop {
            let mut grew = false;
            for id in &ids {
                if set.contains(id) {
                    continue;
                }
                let Ok(d) = doc.get_dictionary(*id) else { continue };
                let linked = [&b"IRT"[..], &b"Parent"[..]].iter().any(|k| {
                    matches!(d.get(k), Ok(LoObj::Reference(r)) if set.contains(r))
                });
                if linked {
                    set.insert(*id);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        let popups: Vec<ObjectId> = set
            .iter()
            .filter_map(|id| match doc.get_dictionary(*id).ok()?.get(b"Popup") {
                Ok(LoObj::Reference(r)) => Some(*r),
                _ => None,
            })
            .collect();
        set.extend(popups);
        remove_from_annots(doc, pid, &set);
        for id in &set {
            doc.objects.remove(id);
        }
    }
    Ok(())
}

/// Sửa các annotation có sẵn rồi lưu sang `output`.
pub fn update_annotations(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    updates: &[AnnotUpdate],
    meta: &AnnotMeta,
) -> Result<(), EngineError> {
    let mut doc = load_lo(pdfium, input)?;
    modify_doc(&mut doc, updates, &[], &meta_date(meta))?;
    doc.save(output).map_err(perr("lopdf save"))?;
    Ok(())
}

/// Xoá các annotation có sẵn (kèm popup & trả lời) rồi lưu sang `output`.
pub fn delete_annotations(pdfium: &Pdfium, input: &Path, output: &Path, targets: &[AnnotRef]) -> Result<(), EngineError> {
    let mut doc = load_lo(pdfium, input)?;
    modify_doc(&mut doc, &[], targets, &pdf_date_now())?;
    doc.save(output).map_err(perr("lopdf save"))?;
    Ok(())
}

// ======================= Lưu tổng hợp =======================

fn annot_counts(doc: &mut LoDoc) -> Result<BTreeMap<u32, usize>, EngineError> {
    let pages = doc.get_pages();
    let mut out = BTreeMap::new();
    for (n, pid) in pages {
        out.insert(n, annots_ids(doc, pid)?.len());
    }
    Ok(out)
}

/// Lưu MỌI thay đổi chú thích của phiên (tạo mới markup/text/shape, sửa, xoá)
/// sang `output` trong 1 lần — `input` không bị đụng tới.
///
/// Thứ tự: sửa/xoá annotation có sẵn (lopdf) → tạo markup/FreeText/Note
/// (đường `apply_annotations` đã kiểm chứng) → thêm shape/stamp + gắn
/// tác giả/ngày/NM cho mọi annotation mới (lopdf).
pub fn save_annotations(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    req: &AnnotSaveRequest,
) -> Result<(), EngineError> {
    let date = meta_date(&req.meta);
    let tmp = output.with_extension("ffannot.tmp.pdf");
    let result = (|| {
        let mut src: &Path = input;
        if !req.updates.is_empty() || !req.deletes.is_empty() {
            let mut doc = load_lo(pdfium, input)?;
            modify_doc(&mut doc, &req.updates, &req.deletes, &date)?;
            doc.save(&tmp).map_err(perr("lopdf save"))?;
            src = &tmp;
        }
        let mut doc;
        if !req.specs.is_empty() {
            let before = {
                let mut d0 = load_lo(pdfium, src)?;
                annot_counts(&mut d0)?
            };
            apply_annotations(pdfium, src, output, &req.specs)?;
            doc = load_lo(pdfium, output)?;
            // Gắn tác giả/ngày/NM cho annotation vừa tạo (nằm cuối /Annots).
            let pages = doc.get_pages();
            for (n, pid) in pages {
                let start = before.get(&n).copied().unwrap_or(0);
                let ids = annots_ids(&mut doc, pid)?;
                for id in ids.into_iter().skip(start).flatten() {
                    if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
                        stamp_meta(d, &req.meta, &date);
                    }
                }
            }
        } else {
            doc = load_lo(pdfium, src)?;
        }
        add_shapes(&mut doc, &req.shapes, &req.meta, &date)?;
        doc.save(output).map_err(perr("lopdf save"))?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

// ======================= Render ẩn annotation đang sửa =======================

/// Render trang như `render::render_page` nhưng ẨN các annotation (chỉ số
/// trong /Annots) đang được sửa/xoá trên UI — lớp preview vẽ thay chúng.
pub fn render_page_hiding(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    target_width: u32,
    hide: &[usize],
) -> Result<image::DynamicImage, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, None)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let mut page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
    {
        let annots = page.annotations_mut();
        for &i in hide {
            if let Ok(mut a) = annots.get(i as PdfPageAnnotationIndex) {
                let _ = a.set_is_hidden(true);
            }
        }
    }
    let config = PdfRenderConfig::new()
        .set_target_width(target_width as i32)
        .set_maximum_height((target_width as i32) * 4);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| EngineError::Pdfium(format!("render trang {page_index} thất bại: {e}")))?;
    Ok(bitmap.as_image())
}
