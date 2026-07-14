//! Annotation (chú thích): tạo/ghi & đọc lại. Phase 2.
//!
//! Dùng PDFium (qua pdfium-render) để tạo annotation rồi lưu ra file mới.
//! Đường GHI file được test round-trip (ghi → mở lại → còn nguyên) vì lưu sai
//! có thể hỏng file người dùng.

use std::collections::BTreeMap;
use std::path::Path;

use pdfium_render::prelude::*;

use crate::text::Rect;
use crate::EngineError;

/// Loại annotation hỗ trợ ở Phase 2 (mở rộng dần).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotKind {
    Highlight,
    Underline,
    Strikeout,
    Square,
    /// Hộp văn bản (free text) — `contents` là nội dung hiển thị.
    FreeText,
    /// Ghi chú dán (sticky note) — `contents` là nội dung popup.
    Note,
}

/// Mô tả một annotation cần tạo.
#[derive(Clone, Debug)]
pub struct AnnotSpec {
    pub kind: AnnotKind,
    pub page_index: u16,
    /// Bao toàn bộ annotation (Square/FreeText/Note dùng trực tiếp; với
    /// Highlight/Underline/Strikeout đây là hộp bao toàn bộ — bounds thực tế
    /// vẽ ra theo `quads`).
    pub rect: Rect,
    /// Highlight/Underline/Strikeout theo TEXT thật thường trải nhiều dòng —
    /// mỗi dòng là 1 quad riêng (giống Foxit: chọn văn bản nhiều dòng tạo
    /// nhiều quad, không phải 1 khối chữ nhật phủ luôn cả khoảng trắng giữa
    /// dòng). Rỗng = suy ra 1 quad duy nhất từ `rect` (tương thích cũ).
    pub quads: Vec<Rect>,
    /// Màu RGBA (0–255). Với text = màu chữ; markup = màu tô/viền.
    pub color: [u8; 4],
    pub contents: Option<String>,
    /// Định dạng cho FreeText (text box).
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl AnnotSpec {
    /// Giá trị mặc định tiện cho test/markup (không phải text box). `quads`
    /// rỗng → 1 quad duy nhất suy ra từ `rect`.
    pub fn markup(kind: AnnotKind, page_index: u16, rect: Rect, color: [u8; 4]) -> Self {
        AnnotSpec {
            kind,
            page_index,
            rect,
            quads: Vec::new(),
            color,
            contents: None,
            font_size: 14.0,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

/// Thông tin một annotation đọc được từ file.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotInfo {
    pub page_index: u16,
    pub kind: String,
    pub rect: Rect,
    pub contents: Option<String>,
    /// Số attachment point (quad) — markup nhiều dòng sẽ >1.
    pub quad_count: usize,
}

fn to_pdf_rect(r: &Rect) -> PdfRect {
    PdfRect::new(
        PdfPoints::new(r.bottom),
        PdfPoints::new(r.left),
        PdfPoints::new(r.top),
        PdfPoints::new(r.right),
    )
}

/// QuadPoints cho text-markup theo thứ tự PDFium/Acrobat mong đợi:
/// (top-left, top-right, bottom-left, bottom-right). Lưu ý `PdfQuadPoints::from_rect`
/// dùng thứ tự khác (LL,LR,UR,UL) khiến highlight bị xoắn thành sọc mỏng.
fn quad_from_rect(r: &PdfRect) -> PdfQuadPoints {
    PdfQuadPoints::new(
        r.left(), r.top(),     // x1,y1 = top-left
        r.right(), r.top(),    // x2,y2 = top-right
        r.left(), r.bottom(),  // x3,y3 = bottom-left
        r.right(), r.bottom(), // x4,y4 = bottom-right
    )
}

fn from_pdf_rect(r: &PdfRect) -> Rect {
    Rect {
        left: r.left().value,
        bottom: r.bottom().value,
        right: r.right().value,
        top: r.top().value,
    }
}

fn is_text_kind(k: AnnotKind) -> bool {
    matches!(k, AnnotKind::FreeText | AnnotKind::Note)
}

/// Quad thật sự cần vẽ cho markup: dùng `quads` (1 quad/dòng) nếu có,
/// nếu không suy ra 1 quad duy nhất từ `rect` (tương thích spec cũ/đơn giản).
fn effective_quads(spec: &AnnotSpec) -> Vec<PdfRect> {
    if spec.quads.is_empty() {
        vec![to_pdf_rect(&spec.rect)]
    } else {
        spec.quads.iter().map(to_pdf_rect).collect()
    }
}

/// Hộp bao nhỏ nhất chứa toàn bộ quad — dùng làm `/Rect` của annotation.
fn union_pdf_rect(quads: &[PdfRect]) -> PdfRect {
    let mut left = quads[0].left().value;
    let mut bottom = quads[0].bottom().value;
    let mut right = quads[0].right().value;
    let mut top = quads[0].top().value;
    for q in &quads[1..] {
        left = left.min(q.left().value);
        bottom = bottom.min(q.bottom().value);
        right = right.max(q.right().value);
        top = top.max(q.top().value);
    }
    PdfRect::new(
        PdfPoints::new(bottom),
        PdfPoints::new(left),
        PdfPoints::new(top),
        PdfPoints::new(right),
    )
}

/// Tạo các annotation theo `specs` rồi lưu sang `output`. Không sửa `input`.
///
/// - Markup/shape (Highlight/Underline/Strikeout/Square): tạo bằng PDFium.
/// - Text (FreeText/Note): ghi bằng lopdf với /DA + /DR đầy đủ (font, cỡ, màu,
///   đậm/nghiêng) để GIỮ ĐỊNH DẠNG khi lưu — đúng như Foxit.
pub fn apply_annotations(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    specs: &[AnnotSpec],
) -> Result<(), EngineError> {
    // ----- Pass 1: PDFium tạo markup/shape, luôn ghi ra `output` (kể cả copy) -----
    {
        let document = pdfium
            .load_pdf_from_file(input, None)
            .map_err(|e| EngineError::Pdfium(e.to_string()))?;

        let mut by_page: BTreeMap<u16, Vec<&AnnotSpec>> = BTreeMap::new();
        for s in specs.iter().filter(|s| !is_text_kind(s.kind)) {
            by_page.entry(s.page_index).or_default().push(s);
        }

        for (page_index, page_specs) in by_page {
            let mut page = document
                .pages()
                .get(page_index)
                .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;

            for spec in page_specs {
                let color =
                    PdfColor::new(spec.color[0], spec.color[1], spec.color[2], spec.color[3]);
                let annots = page.annotations_mut();
                let err = |e: PdfiumError| EngineError::Pdfium(format!("tạo annotation: {e}"));

                // Mỗi quad = 1 dòng văn bản đã chọn (như Foxit: chọn văn bản nhiều
                // dòng tạo nhiều quad trong CÙNG 1 annotation, không phải 1 khối
                // chữ nhật phủ cả khoảng trắng giữa dòng). `create_*_annotation()`
                // trả về kiểu cụ thể khác nhau nên không gộp match-arm được —
                // tách riêng từng loại, logic giống nhau.
                match spec.kind {
                    AnnotKind::Highlight => {
                        let quads = effective_quads(spec);
                        let bounds = union_pdf_rect(&quads);
                        let mut a = annots.create_highlight_annotation().map_err(err)?;
                        a.set_bounds(bounds).map_err(err)?;
                        for q in &quads {
                            a.attachment_points_mut()
                                .create_attachment_point_at_end(quad_from_rect(q))
                                .map_err(err)?;
                        }
                        a.set_fill_color(color).map_err(err)?;
                    }
                    AnnotKind::Underline => {
                        let quads = effective_quads(spec);
                        let bounds = union_pdf_rect(&quads);
                        let mut a = annots.create_underline_annotation().map_err(err)?;
                        a.set_bounds(bounds).map_err(err)?;
                        for q in &quads {
                            a.attachment_points_mut()
                                .create_attachment_point_at_end(quad_from_rect(q))
                                .map_err(err)?;
                        }
                        a.set_stroke_color(color).map_err(err)?;
                    }
                    AnnotKind::Strikeout => {
                        let quads = effective_quads(spec);
                        let bounds = union_pdf_rect(&quads);
                        let mut a = annots.create_strikeout_annotation().map_err(err)?;
                        a.set_bounds(bounds).map_err(err)?;
                        for q in &quads {
                            a.attachment_points_mut()
                                .create_attachment_point_at_end(quad_from_rect(q))
                                .map_err(err)?;
                        }
                        a.set_stroke_color(color).map_err(err)?;
                    }
                    AnnotKind::Square => {
                        let rect = to_pdf_rect(&spec.rect);
                        let mut a = annots.create_square_annotation().map_err(err)?;
                        a.set_bounds(rect).map_err(err)?;
                        a.set_stroke_color(color).map_err(err)?;
                    }
                    AnnotKind::FreeText | AnnotKind::Note => unreachable!(),
                }
            }
        }

        document
            .save_to_file(output)
            .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    }

    // ----- Pass 2: lopdf ghi FreeText/Note với định dạng đầy đủ -----
    let text: Vec<&AnnotSpec> = specs.iter().filter(|s| is_text_kind(s.kind)).collect();
    if !text.is_empty() {
        add_text_annotations(output, &text)?;
    }
    Ok(())
}

// ---- Ghi FreeText/Note bằng lopdf (giữ định dạng qua /DA + /DR) ----

use lopdf::{Dictionary, Document as LoDoc, Object as LoObj, ObjectId, StringFormat};

// ---- Type1 fonts (dự phòng cho ASCII) ----

struct DrFonts {
    helv: ObjectId,
    bold: ObjectId,
    obl: ObjectId,
    boldobl: ObjectId,
}

fn ensure_type1_fonts(doc: &mut LoDoc) -> DrFonts {
    let mut mk = |bf: &str| {
        let mut d = Dictionary::new();
        d.set("Type", LoObj::Name(b"Font".to_vec()));
        d.set("Subtype", LoObj::Name(b"Type1".to_vec()));
        d.set("BaseFont", LoObj::Name(bf.as_bytes().to_vec()));
        d.set("Encoding", LoObj::Name(b"WinAnsiEncoding".to_vec()));
        doc.add_object(LoObj::Dictionary(d))
    };
    DrFonts {
        helv: mk("Helvetica"),
        bold: mk("Helvetica-Bold"),
        obl: mk("Helvetica-Oblique"),
        boldobl: mk("Helvetica-BoldOblique"),
    }
}

fn pdf_text_string(s: &str) -> LoObj {
    if s.is_ascii() {
        LoObj::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        // UTF-16BE với BOM 0xFEFF
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.push((u >> 8) as u8);
            bytes.push((u & 0xFF) as u8);
        }
        LoObj::String(bytes, StringFormat::Hexadecimal)
    }
}

// ---- Unicode font (Type0/CIDFontType2) cho tiếng Việt ----

/// Tìm file TTF có Unicode trên hệ thống theo đậm/nghiêng (Windows/Linux/Mac).
/// Thử đúng file biến thể (vd `arialbd.ttf`) trước, rồi mới hạ cấp dần về regular
/// của family khác — để FreeText giữ đúng đậm/nghiêng như Foxit.
pub(crate) fn find_font_bytes(bold: bool, italic: bool) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        // [regular, bold, italic, bold-italic] cho từng family phổ biến trên Windows.
        const FAMILIES: &[[&str; 4]] = &[
            ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
            ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"],
            ["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"],
            ["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"],
            ["tahoma.ttf", "tahomabd.ttf", "tahoma.ttf", "tahomabd.ttf"],
        ];
        let idx = match (bold, italic) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        };
        for fam in FAMILIES {
            if let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{}", fam[idx])) {
                return Some(bytes);
            }
        }
        // Không có biến thể đúng → hạ cấp về regular của family đầu tiên có sẵn.
        for fam in FAMILIES {
            if let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{}", fam[0])) {
                return Some(bytes);
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (bold, italic);
        let candidates: &[&str] = &[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ];
        candidates.iter().find_map(|p| std::fs::read(p).ok())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // [regular, bold, italic, bold-italic] cho các family Linux phổ biến.
        const FAMILIES: &[[&str; 4]] = &[
            [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-BoldOblique.ttf",
            ],
            [
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Italic.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-BoldItalic.ttf",
            ],
            [
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Oblique.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-BoldOblique.ttf",
            ],
            [
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans-Oblique.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans-BoldOblique.ttf",
            ],
        ];
        let idx = match (bold, italic) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        };
        for fam in FAMILIES {
            if let Ok(bytes) = std::fs::read(fam[idx]) {
                return Some(bytes);
            }
        }
        // Không có biến thể đúng → hạ cấp về regular đầu tiên có sẵn.
        for fam in FAMILIES {
            if let Ok(bytes) = std::fs::read(fam[0]) {
                return Some(bytes);
            }
        }
        None
    }
}

/// Nhúng TTF vào doc như Type0/CIDFontType2 (Identity-H). `used_text` gồm toàn bộ
/// nội dung sẽ hiển thị bằng font này — dùng để dựng `/W` (độ rộng glyph) đúng;
/// PDF spec quy định widths của Type0/CIDFont LUÔN lấy từ /W + /DW, KHÔNG lấy từ
/// bảng hmtx trong font nhúng — thiếu /W khiến mọi glyph bị coi là rộng /DW (1 em),
/// chữ bị dãn cách quá rộng. Trả về object ID Type0 font.
fn embed_type0_font(doc: &mut LoDoc, font_bytes: &[u8], used_text: &str) -> Result<ObjectId, EngineError> {
    let face = ttf_parser::Face::parse(font_bytes, 0)
        .map_err(|e| EngineError::Pdfium(format!("ttf-parser: {:?}", e)))?;

    let upem = face.units_per_em() as f32;
    let scale = |v: i16| (v as f32 * 1000.0 / upem) as i64;
    let bb = face.global_bounding_box();

    // Độ rộng (1000 units/em) cho mọi glyph thực sự dùng tới, kể cả dấu cách và
    // ký tự '?' (dự phòng khi thiếu glyph) — đủ để mọi Tj trong AP stream khớp.
    let mut widths: BTreeMap<u16, i64> = BTreeMap::new();
    for ch in used_text.chars().chain([' ', '?']) {
        let gid = face
            .glyph_index(ch)
            .unwrap_or_else(|| face.glyph_index('?').unwrap_or(ttf_parser::GlyphId(0)));
        let w = face
            .glyph_hor_advance(gid)
            .map_or(1000, |a| (a as f32 * 1000.0 / upem) as i64);
        widths.insert(gid.0, w);
    }
    let w_array: Vec<LoObj> = widths
        .into_iter()
        .flat_map(|(gid, w)| vec![LoObj::Integer(gid as i64), LoObj::Array(vec![LoObj::Integer(w)])])
        .collect();

    // 1. Font file stream (CIDFontType2 dùng /FontFile2)
    let mut fs_dict = Dictionary::new();
    fs_dict.set("Length1", LoObj::Integer(font_bytes.len() as i64));
    let font_file_id = doc.add_object(LoObj::Stream(lopdf::Stream::new(
        fs_dict,
        font_bytes.to_vec(),
    )));

    // 2. FontDescriptor
    let mut fd = Dictionary::new();
    fd.set("Type", LoObj::Name(b"FontDescriptor".to_vec()));
    fd.set("FontName", LoObj::Name(b"ArialMT".to_vec()));
    fd.set("Flags", LoObj::Integer(32)); // Nonsymbolic
    fd.set("Ascent", LoObj::Integer(scale(face.ascender())));
    fd.set("Descent", LoObj::Integer(scale(face.descender())));
    fd.set("CapHeight", LoObj::Integer(scale(face.ascender()) * 7 / 10));
    fd.set("StemV", LoObj::Integer(80));
    fd.set("ItalicAngle", LoObj::Integer(0));
    fd.set(
        "FontBBox",
        LoObj::Array(vec![
            LoObj::Integer(scale(bb.x_min)),
            LoObj::Integer(scale(bb.y_min)),
            LoObj::Integer(scale(bb.x_max)),
            LoObj::Integer(scale(bb.y_max)),
        ]),
    );
    fd.set("FontFile2", LoObj::Reference(font_file_id));
    let fd_id = doc.add_object(LoObj::Dictionary(fd));

    // 3. CIDFont (CIDFontType2 = TrueType)
    let mut cidfont = Dictionary::new();
    cidfont.set("Type", LoObj::Name(b"Font".to_vec()));
    cidfont.set("Subtype", LoObj::Name(b"CIDFontType2".to_vec()));
    cidfont.set("BaseFont", LoObj::Name(b"ArialMT".to_vec()));
    let mut cidsys = Dictionary::new();
    cidsys.set("Registry", LoObj::String(b"Adobe".to_vec(), StringFormat::Literal));
    cidsys.set("Ordering", LoObj::String(b"Identity".to_vec(), StringFormat::Literal));
    cidsys.set("Supplement", LoObj::Integer(0));
    cidfont.set("CIDSystemInfo", LoObj::Dictionary(cidsys));
    cidfont.set("FontDescriptor", LoObj::Reference(fd_id));
    cidfont.set("DW", LoObj::Integer(1000));
    cidfont.set("W", LoObj::Array(w_array));
    cidfont.set("CIDToGIDMap", LoObj::Name(b"Identity".to_vec()));
    let cidfont_id = doc.add_object(LoObj::Dictionary(cidfont));

    // 4. Type0 wrapper
    let mut font0 = Dictionary::new();
    font0.set("Type", LoObj::Name(b"Font".to_vec()));
    font0.set("Subtype", LoObj::Name(b"Type0".to_vec()));
    font0.set("BaseFont", LoObj::Name(b"ArialMT-Identity-H".to_vec()));
    font0.set("Encoding", LoObj::Name(b"Identity-H".to_vec()));
    font0.set("DescendantFonts", LoObj::Array(vec![LoObj::Reference(cidfont_id)]));
    Ok(doc.add_object(LoObj::Dictionary(font0)))
}

/// Mã hoá chuỗi thành CID bytes (Identity-H: glyph ID 2 bytes big-endian mỗi char).
fn encode_cid(text: &str, face: &ttf_parser::Face) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.chars().count() * 2);
    for ch in text.chars() {
        let gid = face
            .glyph_index(ch)
            .unwrap_or_else(|| face.glyph_index('?').unwrap_or(ttf_parser::GlyphId(0)));
        out.push((gid.0 >> 8) as u8);
        out.push((gid.0 & 0xFF) as u8);
    }
    out
}

/// Advance width (point) cho một ký tự theo font_size.
fn char_adv(ch: char, face: &ttf_parser::Face, fs: f32) -> f32 {
    let upem = face.units_per_em() as f32;
    face.glyph_index(ch)
        .and_then(|gid| face.glyph_hor_advance(gid))
        .map_or(fs * 0.5, |adv| adv as f32 / upem * fs)
}

/// Ngắt văn bản thành dòng không vượt quá max_w (pt).
fn wrap_lines(text: &str, face: &ttf_parser::Face, fs: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }
        let sp = char_adv(' ', face, fs);
        let mut cur = String::new();
        let mut cur_w = 0.0f32;
        for word in words {
            let ww: f32 = word.chars().map(|c| char_adv(c, face, fs)).sum();
            if cur.is_empty() {
                cur.push_str(word);
                cur_w = ww;
            } else if cur_w + sp + ww <= max_w {
                cur.push(' ');
                cur.push_str(word);
                cur_w += sp + ww;
            } else {
                lines.push(cur);
                cur = word.to_string();
                cur_w = ww;
            }
        }
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Dựng Form XObject (AP /N) cho FreeText với font Unicode.
/// Vẽ nền trắng + viền xám + text nhiều dòng, có word-wrap.
fn build_unicode_ap(
    doc: &mut LoDoc,
    spec: &AnnotSpec,
    font0_id: ObjectId,
    font_bytes: &[u8],
) -> Result<ObjectId, EngineError> {
    let face = ttf_parser::Face::parse(font_bytes, 0)
        .map_err(|e| EngineError::Pdfium(format!("ttf-parser AP: {:?}", e)))?;

    let box_w = spec.rect.right - spec.rect.left;
    let box_h = spec.rect.top - spec.rect.bottom;
    let fs = spec.font_size;
    let upem = face.units_per_em() as f32;
    let ascender = face.ascender() as f32 / upem * fs;
    let (cr, cg, cb) = (
        spec.color[0] as f32 / 255.0,
        spec.color[1] as f32 / 255.0,
        spec.color[2] as f32 / 255.0,
    );

    let margin = 2.0f32;
    let usable_w = (box_w - margin * 2.0).max(1.0);
    let text = spec.contents.as_deref().unwrap_or("");
    let lines = wrap_lines(text, &face, fs, usable_w);
    let line_h = fs * 1.2;
    let y_start = box_h - margin - ascender;

    // Dựng content stream
    let mut cs = String::new();
    // Nền trắng + viền xám
    cs.push_str("q\n1 1 1 rg\n");
    cs.push_str(&format!("0 0 {:.2} {:.2} re f\n", box_w, box_h));
    cs.push_str("0.5 0.5 0.5 RG\n0.5 w\n");
    cs.push_str(&format!("0 0 {:.2} {:.2} re S\n", box_w, box_h));
    // Text
    cs.push_str("BT\n");
    cs.push_str(&format!("/F0 {:.2} Tf\n", fs));
    cs.push_str(&format!("{:.4} {:.4} {:.4} rg\n", cr, cg, cb));
    cs.push_str(&format!("{:.2} TL\n", line_h));
    cs.push_str(&format!("1 0 0 1 {:.2} {:.2} Tm\n", margin, y_start));

    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            cs.push_str("T*\n");
        }
        let cid_bytes = encode_cid(line, &face);
        let hex: String = cid_bytes.iter().map(|b| format!("{:02X}", b)).collect();
        cs.push_str(&format!("<{}> Tj\n", hex));

        // Gạch chân nếu cần (tính từ baseline xuống ~0.1*fs)
        if spec.underline && !line.is_empty() {
            let line_w: f32 = line.chars().map(|c| char_adv(c, &face, fs)).sum();
            let ul_y = y_start - i as f32 * line_h - fs * 0.1;
            cs.push_str("ET\n");
            cs.push_str(&format!("{:.4} {:.4} {:.4} RG\n", cr, cg, cb));
            cs.push_str(&format!("0.5 w\n{:.2} {:.2} m {:.2} {:.2} l S\n",
                margin, ul_y, margin + line_w, ul_y));
            cs.push_str("BT\n");
            cs.push_str(&format!("/F0 {:.2} Tf\n", fs));
            cs.push_str(&format!("{:.4} {:.4} {:.4} rg\n", cr, cg, cb));
            cs.push_str(&format!("{:.2} TL\n", line_h));
            cs.push_str(&format!("1 0 0 1 {:.2} {:.2} Tm\n",
                margin, y_start - i as f32 * line_h));
        }
    }
    cs.push_str("ET\nQ\n");

    let content = cs.into_bytes();

    // Resources
    let mut fr = Dictionary::new();
    fr.set("F0", LoObj::Reference(font0_id));
    let mut res = Dictionary::new();
    res.set("Font", LoObj::Dictionary(fr));

    // Form XObject
    let mut xd = Dictionary::new();
    xd.set("Type", LoObj::Name(b"XObject".to_vec()));
    xd.set("Subtype", LoObj::Name(b"Form".to_vec()));
    xd.set("FormType", LoObj::Integer(1));
    xd.set(
        "BBox",
        LoObj::Array(vec![
            LoObj::Real(0.0), LoObj::Real(0.0),
            LoObj::Real(box_w), LoObj::Real(box_h),
        ]),
    );
    xd.set("Resources", LoObj::Dictionary(res));
    xd.set("Length", LoObj::Integer(content.len() as i64));
    Ok(doc.add_object(LoObj::Stream(lopdf::Stream::new(xd, content))))
}

// ---- Orchestration ----

fn add_text_annotations(path: &Path, specs: &[&AnnotSpec]) -> Result<(), EngineError> {
    let mut doc =
        LoDoc::load(path).map_err(|e| EngineError::Pdfium(format!("lopdf load: {e}")))?;

    let type1 = ensure_type1_fonts(&mut doc);

    // Nhúng font hệ thống (Type0/CIDFont) cho MỌI FreeText (ASCII lẫn Unicode) — để
    // AP stream tự dựng (đo độ rộng glyph từ font thật, hỗ trợ word-wrap & underline
    // nhất quán) áp dụng cho tất cả, không chỉ tiếng Việt. Nhúng riêng theo từng tổ
    // hợp đậm/nghiêng thực sự dùng tới (đúng biến thể font, vd arialbd.ttf).
    let mut combos: BTreeMap<(bool, bool), String> = BTreeMap::new();
    for s in specs.iter().filter(|s| matches!(s.kind, AnnotKind::FreeText)) {
        let entry = combos.entry((s.bold, s.italic)).or_default();
        if let Some(c) = &s.contents {
            entry.push_str(c);
            entry.push('\n');
        }
    }
    let mut text_fonts: BTreeMap<(bool, bool), (ObjectId, Vec<u8>)> = BTreeMap::new();
    for (key, combined) in combos {
        if let Some(bytes) = find_font_bytes(key.0, key.1) {
            if let Ok(id) = embed_type0_font(&mut doc, &bytes, &combined) {
                text_fonts.insert(key, (id, bytes));
            }
        }
    }

    let pages = doc.get_pages();
    for spec in specs {
        let page_id = match pages.get(&(spec.page_index as u32 + 1)) {
            Some(id) => *id,
            None => continue,
        };
        let annot_id = build_text_annot(
            &mut doc,
            spec,
            &type1,
            text_fonts
                .get(&(spec.bold, spec.italic))
                .map(|(id, b)| (*id, b.as_slice())),
        )?;
        attach_annot(&mut doc, page_id, annot_id)?;
    }

    doc.save(path)
        .map_err(|e| EngineError::Pdfium(format!("lopdf save: {e}")))?;
    Ok(())
}

fn build_text_annot(
    doc: &mut LoDoc,
    spec: &AnnotSpec,
    type1: &DrFonts,
    unicode_font: Option<(ObjectId, &[u8])>,
) -> Result<ObjectId, EngineError> {
    let r = &spec.rect;
    let (cr, cg, cb) = (
        spec.color[0] as f32 / 255.0,
        spec.color[1] as f32 / 255.0,
        spec.color[2] as f32 / 255.0,
    );
    let contents = pdf_text_string(spec.contents.as_deref().unwrap_or(""));

    let mut d = Dictionary::new();
    d.set("Type", LoObj::Name(b"Annot".to_vec()));

    match spec.kind {
        AnnotKind::FreeText => {
            d.set("Subtype", LoObj::Name(b"FreeText".to_vec()));
            d.set(
                "Rect",
                LoObj::Array(vec![
                    LoObj::Real(r.left),
                    LoObj::Real(r.bottom),
                    LoObj::Real(r.right),
                    LoObj::Real(r.top),
                ]),
            );
            d.set("Contents", contents);
            d.set("F", LoObj::Integer(4));

            // Mọi FreeText (ASCII lẫn Unicode) dùng AP stream tự dựng từ font hệ
            // thống thật — đo độ rộng glyph chính xác nên word-wrap & underline
            // nhất quán. Chỉ fallback Type1/DA-only khi không tìm được font nào.
            if let Some((font0_id, font_bytes)) = unicode_font {
                let da = format!("/F0 {:.2} Tf {:.4} {:.4} {:.4} rg", spec.font_size, cr, cg, cb);
                let mut dr_font = Dictionary::new();
                dr_font.set("F0", LoObj::Reference(font0_id));
                let mut dr = Dictionary::new();
                dr.set("Font", LoObj::Dictionary(dr_font));
                d.set("DA", LoObj::String(da.into_bytes(), StringFormat::Literal));
                d.set("DR", LoObj::Dictionary(dr));

                let ap_id = build_unicode_ap(doc, spec, font0_id, font_bytes)?;
                let mut ap_dict = Dictionary::new();
                ap_dict.set("N", LoObj::Reference(ap_id));
                d.set("AP", LoObj::Dictionary(ap_dict));
            } else {
                set_type1_da(&mut d, spec, type1, cr, cg, cb);
            }
        }
        AnnotKind::Note => {
            d.set("Subtype", LoObj::Name(b"Text".to_vec()));
            d.set(
                "Rect",
                LoObj::Array(vec![
                    LoObj::Real(r.left),
                    LoObj::Real(r.top - 18.0),
                    LoObj::Real(r.left + 18.0),
                    LoObj::Real(r.top),
                ]),
            );
            d.set("Contents", contents);
            d.set(
                "C",
                LoObj::Array(vec![LoObj::Real(cr), LoObj::Real(cg), LoObj::Real(cb)]),
            );
            d.set("Name", LoObj::Name(b"Note".to_vec()));
            d.set("Open", LoObj::Boolean(false));
        }
        _ => unreachable!(),
    }
    Ok(doc.add_object(LoObj::Dictionary(d)))
}

fn set_type1_da(d: &mut Dictionary, spec: &AnnotSpec, fonts: &DrFonts, cr: f32, cg: f32, cb: f32) {
    let (fname, fid) = match (spec.bold, spec.italic) {
        (true, true) => ("HeBO", fonts.boldobl),
        (true, false) => ("HeBo", fonts.bold),
        (false, true) => ("HeOb", fonts.obl),
        (false, false) => ("Helv", fonts.helv),
    };
    let da = format!("/{} {:.2} Tf {:.4} {:.4} {:.4} rg", fname, spec.font_size, cr, cg, cb);
    let mut dr_font = Dictionary::new();
    dr_font.set(fname, LoObj::Reference(fid));
    let mut dr = Dictionary::new();
    dr.set("Font", LoObj::Dictionary(dr_font));
    d.set("DA", LoObj::String(da.into_bytes(), StringFormat::Literal));
    d.set("DR", LoObj::Dictionary(dr));
}

enum AnnotsLoc {
    Ref(ObjectId),
    Inline,
    None,
}

fn attach_annot(doc: &mut LoDoc, page_id: ObjectId, annot_id: ObjectId) -> Result<(), EngineError> {
    let loc = {
        let page = doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .map_err(|e| EngineError::Pdfium(format!("đọc page dict: {e}")))?;
        match page.get(b"Annots") {
            Ok(LoObj::Reference(r)) => AnnotsLoc::Ref(*r),
            Ok(LoObj::Array(_)) => AnnotsLoc::Inline,
            _ => AnnotsLoc::None,
        }
    };
    let me = |e: lopdf::Error| EngineError::Pdfium(format!("sửa Annots: {e}"));
    match loc {
        AnnotsLoc::Ref(rid) => {
            let arr = doc.get_object_mut(rid).and_then(|o| o.as_array_mut()).map_err(me)?;
            arr.push(LoObj::Reference(annot_id));
        }
        AnnotsLoc::Inline => {
            let page = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()).map_err(me)?;
            let arr = page.get_mut(b"Annots").and_then(|o| o.as_array_mut()).map_err(me)?;
            arr.push(LoObj::Reference(annot_id));
        }
        AnnotsLoc::None => {
            let page = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()).map_err(me)?;
            page.set("Annots", LoObj::Array(vec![LoObj::Reference(annot_id)]));
        }
    }
    Ok(())
}

/// Đọc danh sách annotation trong toàn tài liệu.
pub fn list_annotations(
    pdfium: &Pdfium,
    input: &Path,
) -> Result<Vec<AnnotInfo>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, None)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let mut out = Vec::new();
    for (pi, page) in document.pages().iter().enumerate() {
        for annot in page.annotations().iter() {
            let rect = annot.bounds().map(|r| from_pdf_rect(&r)).unwrap_or(Rect {
                left: 0.0,
                bottom: 0.0,
                right: 0.0,
                top: 0.0,
            });
            out.push(AnnotInfo {
                page_index: pi as u16,
                kind: format!("{:?}", annot.annotation_type()),
                rect,
                contents: annot.contents(),
                quad_count: annot.attachment_points().len(),
            });
        }
    }
    Ok(out)
}

/// Đếm số annotation trên một trang.
pub fn count_annotations(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
) -> Result<usize, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, None)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("trang {page_index}: {e}")))?;
    Ok(page.annotations().len() as usize)
}
