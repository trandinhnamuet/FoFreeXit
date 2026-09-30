//! Tầng PDF-object (lopdf) cho Page Marks: hình mờ / nền / đầu-chân trang / Bates.
//!
//! Cách Acrobat/Foxit ghi các dấu trang: nội dung nằm trong content stream của
//! trang, bọc bằng marked-content
//! `/Artifact <</Subtype /Watermark /Type /Pagination>> BDC ... EMC`
//! (Subtype = Watermark | Header | Footer | Background). Nhờ dấu này mà
//! "Gỡ hình mờ" / "Gỡ đầu & chân trang" tìm và cắt đúng phần đã thêm, không
//! đụng nội dung gốc. Ta ghi y hệt (Bates dùng Subtype riêng `/BatesN`), mỗi
//! dấu là 1 Form XObject vẽ bằng `cm` + `Do`.
//!
//! Gỡ dấu KHÔNG decode/encode lại toàn bộ content bằng lopdf (encoder của
//! lopdf ghi sai inline image BI/ID/EI) mà dùng bộ lexer tối giản bên dưới để
//! tìm khoảng byte BDC…EMC rồi cắt đúng khoảng đó — phần còn lại giữ nguyên
//! từng byte.

use std::collections::HashMap;
use std::path::Path;

use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId, Stream};
use pdfium_render::prelude::*;

use crate::EngineError;

/// Loại dấu trang (khớp Subtype của `/Artifact` Pagination).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkKind {
    Watermark,
    Header,
    Footer,
    Background,
    Bates,
}

impl MarkKind {
    pub(crate) fn subtype(self) -> &'static str {
        match self {
            MarkKind::Watermark => "Watermark",
            MarkKind::Header => "Header",
            MarkKind::Footer => "Footer",
            MarkKind::Background => "Background",
            MarkKind::Bates => "BatesN",
        }
    }
    fn from_subtype(s: &[u8]) -> Option<MarkKind> {
        match s {
            b"Watermark" => Some(MarkKind::Watermark),
            b"Header" => Some(MarkKind::Header),
            b"Footer" => Some(MarkKind::Footer),
            b"Background" => Some(MarkKind::Background),
            b"BatesN" => Some(MarkKind::Bates),
            _ => None,
        }
    }
}

pub(crate) fn lerr(ctx: &'static str) -> impl Fn(lopdf::Error) -> EngineError {
    move |e| EngineError::Pdfium(format!("{ctx}: {e}"))
}

// ---------------------------------------------------------------- ma trận

/// Ma trận affine PDF [a b c d e f] (quy ước vector hàng: p' = p·M).
pub(crate) type Mat = [f32; 6];
pub(crate) const IDENT: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` rồi tới `b` (p·a·b).
pub(crate) fn mul(a: Mat, b: Mat) -> Mat {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}
pub(crate) fn translate(x: f32, y: f32) -> Mat {
    [1.0, 0.0, 0.0, 1.0, x, y]
}
pub(crate) fn scale(k: f32) -> Mat {
    [k, 0.0, 0.0, k, 0.0, 0.0]
}
/// Xoay ngược chiều kim đồng hồ `deg` độ.
pub(crate) fn rotate(deg: f32) -> Mat {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s, -s, c, 0.0, 0.0]
}
pub(crate) fn invert(m: Mat) -> Mat {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return IDENT;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    [a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)]
}

/// Số gọn cho content stream (tối đa 4 chữ số thập phân, bỏ 0 thừa).
pub(crate) fn num(x: f32) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let s = format!("{:.4}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}
pub(crate) fn mat_str(m: Mat) -> String {
    m.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- nạp / lưu

/// Tài liệu lopdf đã mở + cách mã hoá lại khi lưu (giữ nguyên bảo mật gốc).
pub(crate) struct Loaded {
    pub doc: LoDoc,
    /// Tệp gốc có mã hoá, giải mã bằng qpdf → khi lưu dùng
    /// `qpdf --copy-encryption` để chép y nguyên thuật toán/quyền/mật khẩu.
    qpdf_encryption: Option<(std::path::PathBuf, String)>,
}

impl Loaded {
    pub fn plain(doc: LoDoc) -> Self {
        Loaded { doc, qpdf_encryption: None }
    }
}

/// Mở `input` bằng lopdf. File mã hoá → giải mã (ưu tiên qpdf: hỗ trợ đủ
/// AES-256; không có qpdf thì lopdf tự giải) và NHỚ cách mã hoá để
/// `save_lodoc` mã hoá lại như cũ. File lopdf không đọc được (xref hỏng...) →
/// để PDFium mở (tự sửa) rồi lưu lại vào bộ nhớ.
pub(crate) fn load_lodoc(pdfium: &Pdfium, input: &Path, password: Option<&str>) -> Result<Loaded, EngineError> {
    let mut doc = match LoDoc::load(input) {
        Ok(d) => d,
        Err(_) => {
            let pd = pdfium
                .load_pdf_from_file(input, password)
                .map_err(|e| EngineError::Pdfium(format!("mở {}: {e}", input.display())))?;
            let bytes = pd.save_to_bytes().map_err(|e| EngineError::Pdfium(format!("chuẩn hoá tệp: {e}")))?;
            LoDoc::load_mem(&bytes).map_err(lerr("đọc cấu trúc PDF"))?
        }
    };
    if !doc.is_encrypted() {
        return Ok(Loaded { doc, qpdf_encryption: None });
    }
    let pw = password.unwrap_or("");
    if crate::qpdf::find_qpdf().is_ok() {
        let plain = std::env::temp_dir().join(format!("ffpm_plain_{}_{}.pdf", std::process::id(), unique()));
        crate::qpdf::decrypt_remove_password(input, pw, &plain)
            .map_err(|e| EngineError::Pdfium(format!("tài liệu có mật khẩu — sai hoặc thiếu mật khẩu ({e})")))?;
        let d = LoDoc::load(&plain).map_err(lerr("đọc tệp đã giải mã"));
        let _ = std::fs::remove_file(&plain);
        return Ok(Loaded { doc: d?, qpdf_encryption: Some((input.to_path_buf(), pw.to_string())) });
    }
    doc.decrypt(pw)
        .map_err(|e| EngineError::Pdfium(format!("tài liệu có mật khẩu — sai hoặc thiếu mật khẩu ({e})")))?;
    Ok(Loaded { doc, qpdf_encryption: None })
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Ghi tài liệu ra `output` (mã hoá lại nếu tệp gốc có mã hoá). Ghi qua tệp
/// tạm rồi đổi tên để không bao giờ để lại tệp đích dở dang.
pub(crate) fn save_lodoc(loaded: &mut Loaded, output: &Path) -> Result<(), EngineError> {
    let doc = &mut loaded.doc;
    if let Some(state) = doc.encryption_state.take() {
        doc.encrypt(&state).map_err(lerr("mã hoá lại tài liệu"))?;
    }
    let tmp = output.with_extension(format!("ffpm{}.tmp", unique()));
    let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let written = doc.save_to(&mut f);
    drop(f);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(EngineError::Io(e));
    }
    if let Some((orig, pw)) = &loaded.qpdf_encryption {
        let enc = output.with_extension(format!("ffpm{}.tmp", unique()));
        let qpdf = crate::qpdf::find_qpdf()?;
        let arg = |p: &Path| {
            let s = p.to_string_lossy().into_owned();
            s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
        };
        let out = std::process::Command::new(qpdf)
            .arg(format!("--copy-encryption={}", arg(orig)))
            .arg(format!("--encryption-file-password={pw}"))
            .arg(arg(&tmp))
            .arg(arg(&enc))
            .output()
            .map_err(|e| EngineError::Pdfium(format!("chạy qpdf thất bại: {e}")))?;
        let _ = std::fs::remove_file(&tmp);
        if !matches!(out.status.code(), Some(0) | Some(3)) {
            let _ = std::fs::remove_file(&enc);
            return Err(EngineError::Pdfium(format!(
                "mã hoá lại tài liệu: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        std::fs::rename(&enc, &tmp)?;
    }
    if output.exists() {
        let _ = std::fs::remove_file(output);
    }
    std::fs::rename(&tmp, output)?;
    Ok(())
}

// ---------------------------------------------------------------- hình học trang

/// Vùng hiển thị của trang (CropBox ∩ MediaBox) + /Rotate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PageGeom {
    pub id: ObjectId,
    pub x0: f32,
    pub y0: f32,
    pub w: f32,
    pub h: f32,
    pub rotate: i32,
}

impl PageGeom {
    /// Kích thước như người dùng NHÌN thấy (đã tính xoay).
    pub fn disp(&self) -> (f32, f32) {
        if self.rotate % 180 == 90 { (self.h, self.w) } else { (self.w, self.h) }
    }
    /// Toạ độ hiển thị (gốc dưới-trái của trang đang nhìn) → user space của trang.
    pub fn disp_to_user(&self) -> Mat {
        let (x0, y0, w, h) = (self.x0, self.y0, self.w, self.h);
        match self.rotate {
            90 => [0.0, 1.0, -1.0, 0.0, x0 + w, y0],
            180 => [-1.0, 0.0, 0.0, -1.0, x0 + w, y0 + h],
            270 => [0.0, -1.0, 1.0, 0.0, x0, y0 + h],
            _ => [1.0, 0.0, 0.0, 1.0, x0, y0],
        }
    }
}

pub(crate) fn deref<'a>(doc: &'a LoDoc, o: &'a Object) -> &'a Object {
    let mut cur = o;
    for _ in 0..16 {
        match cur {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(x) => cur = x,
                Err(_) => return &Object::Null,
            },
            _ => return cur,
        }
    }
    cur
}

/// Thuộc tính trang có kế thừa từ cây /Pages (MediaBox, CropBox, Rotate, Resources).
pub(crate) fn inherited<'a>(doc: &'a LoDoc, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(v) = cur.get(key) {
            return Some(deref(doc, v));
        }
        let parent = cur.get(b"Parent").and_then(Object::as_reference).ok()?;
        cur = doc.get_dictionary(parent).ok()?;
    }
    None
}

fn as_num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn rect_of(doc: &LoDoc, o: &Object) -> Option<[f32; 4]> {
    let arr = deref(doc, o).as_array().ok()?;
    if arr.len() != 4 {
        return None;
    }
    let v: Vec<f32> = arr.iter().filter_map(|x| as_num(deref(doc, x))).collect();
    if v.len() != 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

pub(crate) fn page_geom(doc: &LoDoc, id: ObjectId) -> PageGeom {
    let media = inherited(doc, id, b"MediaBox").and_then(|o| rect_of(doc, o)).unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let mut vis = media;
    if let Some(c) = inherited(doc, id, b"CropBox").and_then(|o| rect_of(doc, o)) {
        let i = [c[0].max(media[0]), c[1].max(media[1]), c[2].min(media[2]), c[3].min(media[3])];
        if i[2] > i[0] && i[3] > i[1] {
            vis = i;
        }
    }
    let rot = inherited(doc, id, b"Rotate").and_then(as_num).unwrap_or(0.0) as i32;
    PageGeom {
        id,
        x0: vis[0],
        y0: vis[1],
        w: vis[2] - vis[0],
        h: vis[3] - vis[1],
        rotate: ((rot % 360) + 360) % 360 / 90 * 90,
    }
}

pub(crate) fn page_geoms(doc: &LoDoc) -> Vec<PageGeom> {
    doc.get_pages().values().map(|id| page_geom(doc, *id)).collect()
}

// ---------------------------------------------------------------- content

/// Nối mọi content stream của trang (đã giải nén), chèn '\n' giữa các stream
/// để token cuối stream trước không dính token đầu stream sau.
pub(crate) fn page_content(doc: &LoDoc, page: ObjectId) -> Vec<u8> {
    let mut out = Vec::new();
    for sid in doc.get_page_contents(page) {
        if let Ok(s) = doc.get_object(sid).and_then(Object::as_stream) {
            match s.decompressed_content() {
                Ok(d) => out.extend_from_slice(&d),
                Err(_) => out.extend_from_slice(&s.content),
            }
            out.push(b'\n');
        }
    }
    out
}

/// Danh sách tham chiếu content stream hiện có của trang (theo thứ tự vẽ).
fn content_refs(doc: &LoDoc, page: ObjectId) -> Vec<Object> {
    doc.get_page_contents(page).into_iter().map(Object::Reference).collect()
}

fn new_stream(doc: &mut LoDoc, dict: Dictionary, content: Vec<u8>) -> ObjectId {
    let mut s = Stream::new(dict, content);
    let _ = s.compress();
    doc.add_object(Object::Stream(s))
}

// ---------------------------------------------------------------- copy object giữa 2 tài liệu

/// Sao chép sâu `obj` từ `src` sang `dst` (memo theo ObjectId nguồn — font/ảnh
/// dùng chung chỉ copy 1 lần). Bỏ khoá /Parent để không kéo theo cây trang.
pub(crate) fn copy_obj(src: &LoDoc, dst: &mut LoDoc, obj: &Object, map: &mut HashMap<ObjectId, ObjectId>) -> Object {
    match obj {
        Object::Reference(id) => Object::Reference(copy_ref(src, dst, *id, map)),
        Object::Array(a) => Object::Array(a.iter().map(|o| copy_obj(src, dst, o, map)).collect()),
        Object::Dictionary(d) => Object::Dictionary(copy_dict(src, dst, d, map)),
        Object::Stream(s) => {
            let mut ns = s.clone();
            ns.dict = copy_dict(src, dst, &s.dict, map);
            Object::Stream(ns)
        }
        o => o.clone(),
    }
}

fn copy_dict(src: &LoDoc, dst: &mut LoDoc, d: &Dictionary, map: &mut HashMap<ObjectId, ObjectId>) -> Dictionary {
    let mut out = Dictionary::new();
    for (k, v) in d.iter() {
        if k.as_slice() == b"Parent" {
            continue;
        }
        out.set(k.clone(), copy_obj(src, dst, v, map));
    }
    out
}

fn copy_ref(src: &LoDoc, dst: &mut LoDoc, id: ObjectId, map: &mut HashMap<ObjectId, ObjectId>) -> ObjectId {
    if let Some(n) = map.get(&id) {
        return *n;
    }
    let new_id = dst.new_object_id();
    map.insert(id, new_id);
    let o = src.get_object(id).cloned().unwrap_or(Object::Null);
    let copied = copy_obj(src, dst, &o, map);
    dst.objects.insert(new_id, copied);
    new_id
}

/// Biến 1 trang của `src` thành Form XObject trong `dst`. `bbox` theo toạ độ
/// user space của trang nguồn; `matrix` (nếu có) đưa về hệ toạ độ của "con dấu".
pub(crate) fn page_to_form(
    src: &LoDoc,
    page: ObjectId,
    dst: &mut LoDoc,
    map: &mut HashMap<ObjectId, ObjectId>,
    bbox: [f32; 4],
    matrix: Option<Mat>,
) -> ObjectId {
    let content = page_content(src, page);
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", Object::Array(bbox.iter().map(|v| Object::Real(*v)).collect()));
    if let Some(m) = matrix {
        dict.set("Matrix", Object::Array(m.iter().map(|v| Object::Real(*v)).collect()));
    }
    if let Some(res) = inherited(src, page, b"Resources").cloned() {
        dict.set("Resources", copy_obj(src, dst, &res, map));
    }
    if let Ok(g) = src.get_dictionary(page).and_then(|d| d.get(b"Group")) {
        let g = g.clone();
        dict.set("Group", copy_obj(src, dst, &g, map));
    }
    new_stream(dst, dict, content)
}

// ---------------------------------------------------------------- đóng dấu lên trang

/// Nội dung 1 dấu trang.
pub(crate) enum Body {
    /// Vẽ Form XObject `id` với ma trận `cm` (hệ con dấu → user space trang).
    XObject { id: ObjectId, cm: Mat },
    /// Tô màu toàn vùng hiển thị (nền màu): `cm` = hiển thị → user, `w×h` = cỡ hiển thị.
    Fill { rgb: [u8; 3], cm: Mat, w: f32, h: f32 },
}

/// Resources hiệu lực của trang, dạng dict TRỰC TIẾP (kể cả /XObject,
/// /ExtGState) — sửa được mà không đụng dict dùng chung với trang khác.
fn direct_resources(doc: &LoDoc, page: ObjectId) -> Dictionary {
    let mut res = match inherited(doc, page, b"Resources") {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    for key in [&b"XObject"[..], b"ExtGState"] {
        if let Ok(v) = res.get(key) {
            let resolved = deref(doc, v).clone();
            match resolved {
                Object::Dictionary(d) => res.set(key.to_vec(), Object::Dictionary(d)),
                _ => {
                    res.remove(key);
                }
            }
        }
    }
    res
}

fn unique_name(res: &Dictionary, cat: &[u8], prefix: &str) -> Vec<u8> {
    let sub = res.get(cat).and_then(Object::as_dict).ok();
    for n in 0.. {
        let name = format!("{prefix}{n}").into_bytes();
        if sub.map_or(true, |d| !d.has(&name)) {
            return name;
        }
    }
    unreachable!()
}

fn res_insert(res: &mut Dictionary, cat: &[u8], name: Vec<u8>, val: Object) {
    if !matches!(res.get(cat), Ok(Object::Dictionary(_))) {
        res.set(cat.to_vec(), Object::Dictionary(Dictionary::new()));
    }
    if let Ok(Object::Dictionary(d)) = res.get_mut(cat) {
        d.set(name, val);
    }
}

/// Thêm 1 dấu trang vào `page`: nội dung bọc `/Artifact <</Subtype …>> BDC … EMC`,
/// đặt DƯỚI nội dung gốc (`behind`) hoặc TRÊN (bọc nội dung gốc trong q…Q để
/// trạng thái đồ hoạ của trang không rò sang dấu).
pub(crate) fn stamp_page(
    doc: &mut LoDoc,
    page: ObjectId,
    kind: MarkKind,
    behind: bool,
    opacity: f32,
    body: &Body,
) -> Result<(), EngineError> {
    let mut res = direct_resources(doc, page);
    let mut ops = format!("/Artifact <</Subtype /{} /Type /Pagination>> BDC\nq\n", kind.subtype());
    if opacity < 0.999 {
        let a = opacity.clamp(0.0, 1.0);
        let name = unique_name(&res, b"ExtGState", "FFpmGS");
        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("ca", Object::Real(a));
        gs.set("CA", Object::Real(a));
        ops += &format!("/{} gs\n", String::from_utf8_lossy(&name));
        res_insert(&mut res, b"ExtGState", name, Object::Dictionary(gs));
    }
    match body {
        Body::XObject { id, cm } => {
            let name = unique_name(&res, b"XObject", "FFpmX");
            ops += &format!("{} cm\n/{} Do\n", mat_str(*cm), String::from_utf8_lossy(&name));
            res_insert(&mut res, b"XObject", name, Object::Reference(*id));
        }
        Body::Fill { rgb, cm, w, h } => {
            ops += &format!(
                "{} cm\n{} {} {} rg\n0 0 {} {} re\nf\n",
                mat_str(*cm),
                num(rgb[0] as f32 / 255.0),
                num(rgb[1] as f32 / 255.0),
                num(rgb[2] as f32 / 255.0),
                num(*w),
                num(*h)
            );
        }
    }
    ops += "Q\nEMC\n";

    let old = content_refs(doc, page);
    let mut contents = Vec::new();
    if behind {
        let sid = new_stream(doc, Dictionary::new(), ops.into_bytes());
        contents.push(Object::Reference(sid));
        contents.extend(old);
    } else if old.is_empty() {
        let sid = new_stream(doc, Dictionary::new(), ops.into_bytes());
        contents.push(Object::Reference(sid));
    } else {
        let q = new_stream(doc, Dictionary::new(), b"q\n".to_vec());
        let sid = new_stream(doc, Dictionary::new(), format!("Q\n{ops}").into_bytes());
        contents.push(Object::Reference(q));
        contents.extend(old);
        contents.push(Object::Reference(sid));
    }
    let pd = doc.get_dictionary_mut(page).map_err(lerr("sửa trang"))?;
    pd.set("Contents", Object::Array(contents));
    pd.set("Resources", Object::Dictionary(res));
    Ok(())
}

// ---------------------------------------------------------------- lexer content stream

/// Giá trị operand tối giản — chỉ đủ để đọc toán hạng của BDC.
#[derive(Clone, Debug)]
enum Val {
    Name(Vec<u8>),
    Str(Vec<u8>),
    Dict(Vec<(Vec<u8>, Val)>),
    Other,
}

enum Tok {
    Val(Val),
    DictStart,
    DictEnd,
    ArrStart,
    ArrEnd,
    Op(Vec<u8>),
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0)
}
fn is_delim(c: u8) -> bool {
    b"()<>[]{}/%".contains(&c)
}

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if is_ws(c) {
                self.i += 1;
            } else if c == b'%' {
                while self.i < self.b.len() && self.b[self.i] != b'\n' && self.b[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Token kế tiếp kèm vị trí byte bắt đầu.
    fn next(&mut self) -> Option<(usize, Tok)> {
        self.skip_ws();
        if self.i >= self.b.len() {
            return None;
        }
        let start = self.i;
        let b = self.b;
        let c = b[self.i];
        let tok = match c {
            b'/' => {
                self.i += 1;
                let mut name = Vec::new();
                while self.i < b.len() && !is_ws(b[self.i]) && !is_delim(b[self.i]) {
                    if b[self.i] == b'#' && self.i + 2 < b.len() {
                        if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[self.i + 1..self.i + 3]).unwrap_or("x"), 16) {
                            name.push(v);
                            self.i += 3;
                            continue;
                        }
                    }
                    name.push(b[self.i]);
                    self.i += 1;
                }
                Tok::Val(Val::Name(name))
            }
            b'(' => {
                self.i += 1;
                let mut depth = 1;
                let mut s = Vec::new();
                while self.i < b.len() {
                    let ch = b[self.i];
                    self.i += 1;
                    match ch {
                        b'\\' => {
                            if self.i < b.len() {
                                s.push(b[self.i]);
                                self.i += 1;
                            }
                        }
                        b'(' => {
                            depth += 1;
                            s.push(ch);
                        }
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            s.push(ch);
                        }
                        _ => s.push(ch),
                    }
                }
                Tok::Val(Val::Str(s))
            }
            b'<' => {
                if b.get(self.i + 1) == Some(&b'<') {
                    self.i += 2;
                    Tok::DictStart
                } else {
                    self.i += 1;
                    let mut hex = Vec::new();
                    while self.i < b.len() && b[self.i] != b'>' {
                        if b[self.i].is_ascii_hexdigit() {
                            hex.push(b[self.i]);
                        }
                        self.i += 1;
                    }
                    self.i += 1;
                    if hex.len() % 2 == 1 {
                        hex.push(b'0');
                    }
                    let s = hex
                        .chunks(2)
                        .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                        .collect();
                    Tok::Val(Val::Str(s))
                }
            }
            b'>' => {
                self.i += if b.get(self.i + 1) == Some(&b'>') { 2 } else { 1 };
                Tok::DictEnd
            }
            b'[' => {
                self.i += 1;
                Tok::ArrStart
            }
            b']' => {
                self.i += 1;
                Tok::ArrEnd
            }
            b'{' | b'}' | b')' => {
                self.i += 1;
                Tok::Val(Val::Other)
            }
            _ => {
                while self.i < b.len() && !is_ws(b[self.i]) && !is_delim(b[self.i]) {
                    self.i += 1;
                }
                let word = &b[start..self.i];
                let numeric = word.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'+'));
                if numeric || matches!(word, b"true" | b"false" | b"null") {
                    Tok::Val(Val::Other)
                } else {
                    Tok::Op(word.to_vec())
                }
            }
        };
        Some((start, tok))
    }

    /// Sau `ID` của inline image: nhảy qua dữ liệu nhị phân tới hết `EI`.
    fn skip_inline_image_data(&mut self) {
        let b = self.b;
        let mut p = self.i + 1; // 1 ký tự trắng sau ID
        while p + 1 < b.len() {
            if b[p] == b'E' && b[p + 1] == b'I' && p > 0 && is_ws(b[p - 1]) && (p + 2 >= b.len() || is_ws(b[p + 2]) || is_delim(b[p + 2])) {
                self.i = p + 2;
                return;
            }
            p += 1;
        }
        self.i = b.len();
    }

    /// Đọc 1 giá trị hoàn chỉnh bắt đầu bằng token `first` (dict/array lồng nhau).
    fn value(&mut self, first: Tok) -> Val {
        match first {
            Tok::Val(v) => v,
            Tok::DictStart => {
                let mut items = Vec::new();
                loop {
                    let Some((_, t)) = self.next() else { break };
                    match t {
                        Tok::DictEnd => break,
                        Tok::Val(Val::Name(k)) => {
                            let Some((_, vt)) = self.next() else { break };
                            if matches!(vt, Tok::DictEnd) {
                                break;
                            }
                            let v = self.value(vt);
                            items.push((k, v));
                        }
                        _ => {}
                    }
                }
                Val::Dict(items)
            }
            Tok::ArrStart => {
                loop {
                    let Some((_, t)) = self.next() else { break };
                    if matches!(t, Tok::ArrEnd) {
                        break;
                    }
                    let _ = self.value(t);
                }
                Val::Other
            }
            _ => Val::Other,
        }
    }
}

/// Khoảng byte [start, end) của 1 khối dấu trang trong content.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MarkSpan {
    pub start: usize,
    pub end: usize,
    pub kind: MarkKind,
}

/// Phân loại toán hạng BDC. `props_named` tra /Properties của trang theo tên.
fn classify(tag: &[u8], props: &Val, props_named: &dyn Fn(&[u8]) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)>) -> Option<MarkKind> {
    // (Subtype, OCG /Name)
    let (subtype, ocg_name) = match props {
        Val::Dict(items) => {
            let st = items.iter().find(|(k, _)| k == b"Subtype").and_then(|(_, v)| match v {
                Val::Name(n) | Val::Str(n) => Some(n.clone()),
                _ => None,
            });
            (st, None)
        }
        Val::Name(n) => props_named(n).unwrap_or((None, None)),
        _ => (None, None),
    };
    match tag {
        b"Artifact" => subtype.as_deref().and_then(MarkKind::from_subtype),
        // Hình mờ kiểu lớp nội dung tuỳ chọn (OCG tên "Watermark").
        b"OC" => ocg_name.filter(|n| n.eq_ignore_ascii_case(b"Watermark")).map(|_| MarkKind::Watermark),
        _ => None,
    }
}

/// Tìm mọi khối dấu trang (không lồng nhau) trong content của trang.
fn find_spans(content: &[u8], props_named: &dyn Fn(&[u8]) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)>) -> Vec<MarkSpan> {
    let mut lx = Lexer { b: content, i: 0 };
    let mut spans = Vec::new();
    let mut operands: Vec<Val> = Vec::new();
    let mut op_start: Option<usize> = None;
    // Mỗi phần tử = 1 cấp marked-content đang mở; Some = khối dấu trang cần tìm.
    let mut stack: Vec<Option<(MarkKind, usize)>> = Vec::new();
    let mut inside = false;
    while let Some((pos, tok)) = lx.next() {
        match tok {
            Tok::Op(op) => {
                let start = op_start.take().unwrap_or(pos);
                match op.as_slice() {
                    b"BI" => {
                        // Inline image: bỏ qua dict tới ID rồi dữ liệu nhị phân.
                        while let Some((_, t)) = lx.next() {
                            if let Tok::Op(o) = &t {
                                if o == b"ID" {
                                    lx.skip_inline_image_data();
                                    break;
                                }
                            }
                        }
                    }
                    b"BMC" => stack.push(None),
                    b"BDC" => {
                        let found = if !inside && operands.len() >= 2 {
                            match (&operands[operands.len() - 2], &operands[operands.len() - 1]) {
                                (Val::Name(tag), props) => classify(tag, props, props_named),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        if let Some(k) = found {
                            inside = true;
                            stack.push(Some((k, start)));
                        } else {
                            stack.push(None);
                        }
                    }
                    b"EMC" => {
                        if let Some(Some((kind, s))) = stack.pop() {
                            spans.push(MarkSpan { start: s, end: lx.i, kind });
                            inside = false;
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
            other => {
                if op_start.is_none() {
                    op_start = Some(pos);
                }
                let v = lx.value(other);
                operands.push(v);
            }
        }
    }
    spans
}

/// Tra /Properties của trang: tên → (Subtype, /Name của OCG).
fn named_props(doc: &LoDoc, page: ObjectId) -> impl Fn(&[u8]) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)> + '_ {
    move |name: &[u8]| {
        let res = inherited(doc, page, b"Resources")?.as_dict().ok()?;
        let props = deref(doc, res.get(b"Properties").ok()?).as_dict().ok()?;
        let d = deref(doc, props.get(name).ok()?).as_dict().ok()?;
        let st = d.get(b"Subtype").ok().and_then(|o| match deref(doc, o) {
            Object::Name(n) => Some(n.clone()),
            Object::String(s, _) => Some(s.clone()),
            _ => None,
        });
        let ocg = d.get(b"Name").ok().and_then(|o| match deref(doc, o) {
            Object::String(s, _) => Some(s.clone()),
            Object::Name(n) => Some(n.clone()),
            _ => None,
        });
        Some((st, ocg))
    }
}

/// Các khối dấu trang có trên trang.
pub(crate) fn page_marks(doc: &LoDoc, page: ObjectId) -> Vec<MarkSpan> {
    let content = page_content(doc, page);
    let lookup = named_props(doc, page);
    find_spans(&content, &lookup)
}

fn has_watermark_annots(doc: &LoDoc, page: ObjectId) -> bool {
    let Ok(pd) = doc.get_dictionary(page) else { return false };
    let Ok(annots) = pd.get(b"Annots") else { return false };
    let Ok(arr) = deref(doc, annots).as_array() else { return false };
    arr.iter().any(|a| {
        deref(doc, a)
            .as_dict()
            .ok()
            .and_then(|d| d.get(b"Subtype").ok())
            .and_then(|s| s.as_name().ok())
            == Some(&b"Watermark"[..])
    })
}

/// Loại dấu nào có trên trang (kể cả annotation /Watermark).
pub(crate) fn page_mark_kinds(doc: &LoDoc, page: ObjectId) -> Vec<MarkKind> {
    let mut kinds: Vec<MarkKind> = page_marks(doc, page).into_iter().map(|s| s.kind).collect();
    if has_watermark_annots(doc, page) {
        kinds.push(MarkKind::Watermark);
    }
    kinds.sort_by_key(|k| *k as u8);
    kinds.dedup();
    kinds
}

/// Cắt khỏi trang mọi khối dấu thuộc `kinds` (+ annotation /Watermark nếu gỡ
/// hình mờ). Trả về số khối đã gỡ.
pub(crate) fn strip_page(doc: &mut LoDoc, page: ObjectId, kinds: &[MarkKind]) -> Result<usize, EngineError> {
    let content = page_content(doc, page);
    let spans: Vec<MarkSpan> = {
        let lookup = named_props(doc, page);
        find_spans(&content, &lookup).into_iter().filter(|s| kinds.contains(&s.kind)).collect()
    };
    let mut removed = spans.len();

    if !spans.is_empty() {
        let mut out = Vec::with_capacity(content.len());
        let mut at = 0;
        for s in &spans {
            out.extend_from_slice(&content[at..s.start]);
            out.push(b'\n');
            at = s.end;
        }
        out.extend_from_slice(&content[at..]);

        // Dọn resource FFpm* (do ta thêm) không còn được content tham chiếu.
        let mut res_update: Option<Dictionary> = None;
        if let Ok(Object::Dictionary(res)) = doc.get_dictionary(page).and_then(|d| d.get(b"Resources")) {
            let mut res = res.clone();
            for cat in [&b"XObject"[..], b"ExtGState"] {
                if let Ok(Object::Dictionary(sub)) = res.get_mut(cat) {
                    let dead: Vec<Vec<u8>> = sub
                        .iter()
                        .map(|(k, _)| k.clone())
                        .filter(|k| k.starts_with(b"FFpm") && !contains_name(&out, k))
                        .collect();
                    for k in dead {
                        sub.remove(&k);
                    }
                }
            }
            res_update = Some(res);
        }
        let sid = new_stream(doc, Dictionary::new(), out);
        let pd = doc.get_dictionary_mut(page).map_err(lerr("sửa trang"))?;
        pd.set("Contents", Object::Reference(sid));
        if let Some(r) = res_update {
            pd.set("Resources", Object::Dictionary(r));
        }
    }

    if kinds.contains(&MarkKind::Watermark) && has_watermark_annots(doc, page) {
        let annots = {
            let pd = doc.get_dictionary(page).map_err(lerr("đọc trang"))?;
            deref(doc, pd.get(b"Annots").map_err(lerr("đọc Annots"))?).as_array().cloned().unwrap_or_default()
        };
        let before = annots.len();
        let kept: Vec<Object> = annots
            .into_iter()
            .filter(|a| {
                deref(doc, a).as_dict().ok().and_then(|d| d.get(b"Subtype").ok()).and_then(|s| s.as_name().ok())
                    != Some(&b"Watermark"[..])
            })
            .collect();
        removed += before - kept.len();
        let pd = doc.get_dictionary_mut(page).map_err(lerr("sửa trang"))?;
        pd.set("Annots", Object::Array(kept));
    }
    Ok(removed)
}

/// `content` còn chứa token tên `/name` không.
fn contains_name(content: &[u8], name: &[u8]) -> bool {
    let mut pat = vec![b'/'];
    pat.extend_from_slice(name);
    content.windows(pat.len()).enumerate().any(|(i, w)| {
        w == pat.as_slice() && content.get(i + pat.len()).map_or(true, |c| is_ws(*c) || is_delim(*c))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &[u8]) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)> {
        None
    }

    #[test]
    fn finds_artifact_spans_and_skips_inline_images() {
        let c = b"q BT (a) Tj ET Q\nBI /W 1 /H 1 /BPC 8 /CS /G ID \x00EMC\xff EI\n/Artifact <</Subtype /Watermark /Type /Pagination>> BDC q /X Do Q EMC\n/Span <</ActualText (x)>> BDC (b) Tj EMC";
        let spans = find_spans(c, &none);
        assert_eq!(spans.len(), 1);
        let s = spans[0];
        assert_eq!(s.kind, MarkKind::Watermark);
        assert!(std::str::from_utf8(&c[s.start..s.end]).unwrap().starts_with("/Artifact"));
        assert!(std::str::from_utf8(&c[s.start..s.end]).unwrap().ends_with("EMC"));
    }

    #[test]
    fn nested_marked_content_inside_artifact() {
        let c = b"/Artifact <</Subtype /Header>> BDC /P <<>> BDC (x) Tj EMC EMC (keep) Tj";
        let spans = find_spans(c, &none);
        assert_eq!(spans.len(), 1);
        assert_eq!(&c[spans[0].end..], b" (keep) Tj");
    }

    #[test]
    fn matrix_inverse_roundtrip() {
        let m = mul(mul(rotate(33.0), scale(2.0)), translate(10.0, -4.0));
        let r = mul(m, invert(m));
        for (a, b) in r.iter().zip(IDENT.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }
}
