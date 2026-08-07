//! Chỉnh sửa nội dung trang (Phase 4 — moat chính): liệt kê & sửa trực tiếp
//! các page object (text run / ảnh / path) trên 1 trang.
//!
//! PDFium đã expose page object cấp cao nên KHÔNG cần tự parse content stream:
//! mỗi `PdfPageTextObject` là 1 text run sẵn để sửa (có text/font/size/matrix/
//! màu riêng).
//!
//! ## Giữ font gốc khi sửa (chuẩn Foxit — iteration 2)
//! Sửa text KHÔNG được đổi font. Quyết định theo 3 tầng cho mỗi `SetText`:
//! 1. **Giữ nguyên font gốc, sửa tại chỗ** (`FPDFText_SetText` re-encode theo
//!    charmap của font hiện tại) khi chắc chắn an toàn: text mới chỉ dùng ký tự
//!    đã có trong text cũ, HOẶC cmap của font (đọc qua `FPDFFont_GetFontData`)
//!    phủ đủ mọi ký tự mới. Đổi cỡ chữ tại chỗ = scale matrix (không đụng font).
//! 2. Font gốc thiếu glyph (subset) → thay bằng font hệ thống **cùng họ, đúng
//!    đậm/nghiêng** (`fontmatch::find_family_font_bytes`) — gần như không nhìn
//!    ra khác biệt.
//! 3. Bất đắc dĩ mới rơi về font mặc định (`annot::find_font_bytes`).
//!
//! Cỡ chữ trong `SetText`/`ObjectInfo` theo nghĩa **hiển thị** (đã nhân scale
//! của matrix — đúng như UI thấy). Tạo lại object phải quy đổi ngược về cỡ
//! "unscaled" trước khi áp matrix gốc, nếu không chữ sẽ phóng đại kép.
//!
//! Bài học PDFium giữ nguyên: `regenerate_content()` trước khi lưu; object gỡ
//! bằng `remove_object_at_index` phải `std::mem::forget` (Drop gọi
//! FPDFPageObj_Destroy → SEGFAULT).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use pdfium_render::prelude::*;

use crate::annot::find_font_bytes;
use crate::fontmatch;
use crate::text::Rect;
use crate::EngineError;

/// Loại page object (rút gọn cho UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Text,
    Image,
    Path,
    Shading,
    XObjectForm,
    Unsupported,
}

impl ObjectKind {
    fn from_pdfium(t: PdfPageObjectType) -> Self {
        match t {
            PdfPageObjectType::Text => ObjectKind::Text,
            PdfPageObjectType::Image => ObjectKind::Image,
            PdfPageObjectType::Path => ObjectKind::Path,
            PdfPageObjectType::Shading => ObjectKind::Shading,
            PdfPageObjectType::XObjectForm => ObjectKind::XObjectForm,
            PdfPageObjectType::Unsupported => ObjectKind::Unsupported,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectKind::Text => "text",
            ObjectKind::Image => "image",
            ObjectKind::Path => "path",
            ObjectKind::Shading => "shading",
            ObjectKind::XObjectForm => "form",
            ObjectKind::Unsupported => "unsupported",
        }
    }
}

/// Thông tin 1 page object trả về cho UI (để vẽ overlay & sửa).
#[derive(Clone, Debug)]
pub struct ObjectInfo {
    /// Vị trí trong DANH SÁCH PHẲNG object của trang (0-based, thứ tự vẽ,
    /// gồm cả object nằm trong Form XObject). Trang không có form → trùng
    /// index trang như cũ. EditOp cũng đánh địa chỉ theo index này.
    pub index: u16,
    pub kind: ObjectKind,
    /// Khung bao (AABB) theo điểm PDF — object trong form đã quy về TRANG.
    pub rect: Rect,
    /// Object nằm BÊN TRONG một Form XObject (file Canva/Illustrator...).
    /// Sửa text được (in-place / reflow); di chuyển/resize chưa hỗ trợ.
    pub nested: bool,
    /// Chỉ với kind = XObjectForm: các object con đã được liệt kê riêng
    /// (UI bỏ khung của form để khỏi che con).
    pub expanded: bool,
    /// Chỉ với text: nội dung hiện tại.
    pub text: Option<String>,
    /// Chỉ với text: tên font gốc trong PDF (có thể kèm prefix subset).
    pub font_name: Option<String>,
    /// Chỉ với text: family đã làm sạch (để UI hiển thị + CSS xấp xỉ).
    pub font_family: Option<String>,
    /// Chỉ với text: font đậm / nghiêng (từ weight/italic-angle + tên font).
    pub font_bold: Option<bool>,
    pub font_italic: Option<bool>,
    /// Chỉ với text: font có nhúng trong file không.
    pub font_embedded: Option<bool>,
    /// Chỉ với text: cỡ chữ hiển thị (đã tính scale của matrix).
    pub font_size: Option<f32>,
    /// Chỉ với text: màu chữ RGBA.
    pub color: Option<[u8; 4]>,
}

/// Một thao tác sửa nội dung. UI dàn dựng danh sách op rồi áp 1 lần khi lưu.
#[derive(Clone, Debug)]
pub enum EditOp {
    /// Dịch (dx,dy) và/hoặc scale (sx,sy) object `index` — scale quanh góc
    /// dưới-trái của object để giữ neo góc khi resize.
    Transform { index: u16, dx: f32, dy: f32, sx: f32, sy: f32 },
    /// Sửa text object `index`. Mặc định GIỮ NGUYÊN font/cỡ/màu/kiểu gốc:
    /// mọi field `None` nghĩa là "không đổi". `font_size` theo cỡ hiển thị.
    /// `font_family`/`bold`/`italic` = Some(...) chủ động đổi font (match font
    /// hệ thống cùng họ).
    SetText {
        index: u16,
        text: String,
        font_size: Option<f32>,
        color: Option<[u8; 4]>,
        font_family: Option<String>,
        bold: Option<bool>,
        italic: Option<bool>,
    },
    /// Xoá object `index`.
    Delete { index: u16 },
    /// Thay ảnh của image object `index` bằng ảnh từ `image_path`, giữ khung cũ.
    ReplaceImage { index: u16, image_path: String },
    /// Thêm text box mới tại (x,y) (gốc dưới-trái, điểm PDF).
    AddText {
        x: f32,
        y: f32,
        text: String,
        font_size: f32,
        color: [u8; 4],
        font_family: Option<String>,
        bold: bool,
        italic: bool,
    },
    /// Thêm ảnh mới từ file, khung width_pt × height_pt tại (x,y).
    AddImage { x: f32, y: f32, width_pt: f32, height_pt: f32, image_path: String },
    /// Sửa CẢ ĐOẠN nhiều dòng với reflow "như Word" (iteration 3): mọi run
    /// trong `indices` bị thay bằng `text` mới, tự bẻ dòng theo bề rộng khối
    /// (đo bằng hmtx của font), giữ baseline spacing + font/cỡ/màu của run
    /// neo (run có baseline cao nhất). `\n` trong `text` = ngắt dòng cứng.
    ReflowText { indices: Vec<u16>, text: String },
}

fn quad_to_rect(q: &PdfQuadPoints) -> Rect {
    Rect {
        left: q.left().value,
        bottom: q.bottom().value,
        right: q.right().value,
        top: q.top().value,
    }
}

// ---- Object trong Form XObject (Phase 4 iteration 4) ----
//
// File Canva/Illustrator/InDesign hay GÓI toàn bộ nội dung trang vào 1 Form
// XObject — trang chỉ có 1 object "form", chữ nằm bên trong. Để sửa được:
// duyệt PHẲNG đệ quy vào form (FPDFFormObj_CountObjects/GetObject qua
// pdfium-render), mỗi mục có "path" ([i] = cấp trang, [i,j,...] = con trong
// form) + tích ma trận form tổ tiên để quy toạ độ về trang. `index` trong
// ObjectInfo/EditOp = vị trí trong danh sách phẳng này — trang không có form
// thì trùng index trang như cũ (tương thích ngược).

/// Ma trận PDF (a,b,c,d,e,f) dạng giá trị thuần — quy ước hàng: p' = p·M.
type Mat6 = (f32, f32, f32, f32, f32, f32);

const MAT_ID: Mat6 = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

fn mat_of(m: &PdfMatrix) -> Mat6 {
    (m.a(), m.b(), m.c(), m.d(), m.e(), m.f())
}

/// K = M rồi N (p·K = (p·M)·N).
fn mat_mul(m: Mat6, n: Mat6) -> Mat6 {
    (
        m.0 * n.0 + m.1 * n.2,
        m.0 * n.1 + m.1 * n.3,
        m.2 * n.0 + m.3 * n.2,
        m.2 * n.1 + m.3 * n.3,
        m.4 * n.0 + m.5 * n.2 + n.4,
        m.4 * n.1 + m.5 * n.3 + n.5,
    )
}

fn mat_apply(m: Mat6, x: f32, y: f32) -> (f32, f32) {
    (m.0 * x + m.2 * y + m.4, m.1 * x + m.3 * y + m.5)
}

/// AABB của rect sau biến đổi (4 góc → bao).
fn mat_rect(m: Mat6, r: &Rect) -> Rect {
    let pts = [
        mat_apply(m, r.left, r.bottom),
        mat_apply(m, r.right, r.bottom),
        mat_apply(m, r.left, r.top),
        mat_apply(m, r.right, r.top),
    ];
    Rect {
        left: pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
        bottom: pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
        right: pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max),
        top: pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max),
    }
}

/// Hệ số scale DỌC của ma trận (độ dài ảnh của vector đơn vị Y) — dùng quy
/// đổi cỡ chữ hiển thị của object trong form về không gian trang.
fn mat_vscale(m: Mat6) -> f32 {
    (m.2 * m.2 + m.3 * m.3).sqrt().max(0.01)
}

/// 1 mục trong danh sách phẳng object của trang (gồm cả con trong form).
struct FlatEntry {
    /// Địa chỉ: [i] = object cấp trang; [i, j, ...] = con lồng trong form.
    path: Vec<u16>,
    /// Tích ma trận các form tổ tiên (đưa toạ độ trong form về trang).
    acc: Mat6,
    kind: ObjectKind,
}

/// Chặn duyệt form quá sâu/quá nhiều (file lỗi/đệ quy).
const FORM_WALK_MAX_DEPTH: usize = 3;
const FORM_WALK_MAX_ENTRIES: usize = 4000;

/// Danh sách phẳng object của trang theo thứ tự vẽ, đi sâu vào Form XObject.
/// Thứ tự này là HỢP ĐỒNG giữa `list_objects` và `apply_edits` — index trong
/// EditOp tra vào đúng danh sách này.
fn collect_flat(page: &PdfPage<'_>) -> Vec<FlatEntry> {
    fn walk_form(
        form: &PdfPageXObjectFormObject<'_>,
        base: &[u16],
        acc: Mat6,
        depth: usize,
        out: &mut Vec<FlatEntry>,
    ) {
        for j in 0..form.len() {
            if out.len() >= FORM_WALK_MAX_ENTRIES {
                return;
            }
            let Ok(child) = form.get(j) else { continue };
            let mut path = base.to_vec();
            path.push(j as u16);
            out.push(FlatEntry {
                path: path.clone(),
                acc,
                kind: ObjectKind::from_pdfium(child.object_type()),
            });
            if depth < FORM_WALK_MAX_DEPTH {
                if let PdfPageObject::XObjectForm(f2) = &child {
                    let m = child.matrix().map(|m| mat_of(&m)).unwrap_or(MAT_ID);
                    walk_form(f2, &path, mat_mul(m, acc), depth + 1, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    let n = page.objects().len();
    for i in 0..n {
        if out.len() >= FORM_WALK_MAX_ENTRIES {
            break;
        }
        let Ok(obj) = page.objects().get(i) else { continue };
        let path = vec![i as u16];
        out.push(FlatEntry {
            path: path.clone(),
            acc: MAT_ID,
            kind: ObjectKind::from_pdfium(obj.object_type()),
        });
        if let PdfPageObject::XObjectForm(f) = &obj {
            let m = obj.matrix().map(|m| mat_of(&m)).unwrap_or(MAT_ID);
            walk_form(f, &path, m, 1, &mut out);
        }
    }
    out
}

/// Lấy object theo path (đi sâu vào form theo từng nấc).
fn object_at_path<'a>(page: &PdfPage<'a>, path: &[u16]) -> Result<PdfPageObject<'a>, EngineError> {
    let err = |e: PdfiumError| EngineError::Pdfium(format!("lấy object theo path: {e}"));
    let mut obj = page
        .objects()
        .get(path[0] as usize)
        .map_err(err)?;
    for &j in &path[1..] {
        let child = match &obj {
            PdfPageObject::XObjectForm(f) => f.get(j as usize).map_err(err)?,
            _ => {
                return Err(EngineError::Pdfium(
                    "path object lồng không trỏ vào form".into(),
                ))
            }
        };
        obj = child;
    }
    Ok(obj)
}

/// "MỞ GÓI" mọi Form XObject cấp trang thành object cấp trang: rút từng con
/// khỏi form (`FPDFFormObj_RemoveObject` — cần feature pdfium_7350), nhân ma
/// trận form vào con, chèn lại vào TRANG, xoá form rỗng. Hiển thị y hệt,
/// nhưng mọi object thành cấp trang — điều kiện để SỬA được.
///
/// Vì sao bắt buộc: PDFium hiện KHÔNG ghi lại được stream của form —
/// `CPDF_PageContentManager` chỉ biết key /Contents (trang), còn form là
/// stream tự thân → mọi "regenerate" nội dung form rơi vào key /Contents vô
/// nghĩa trên dict form, stream thật giữ nguyên. Đã kiểm chứng bằng test
/// round-trip 3 vòng CI + đọc source cpdf_pagecontentmanager.cpp.
///
/// Form lồng form: lặp tối đa 4 vòng (con là form sẽ thành cấp trang rồi
/// được mở tiếp). Trả về số form đã mở; 0 = trang không có form (KHÔNG ghi
/// `output`).
pub fn flatten_form_xobjects(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    output: &Path,
    password: Option<&str>,
) -> Result<usize, EngineError> {
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let err = |e: PdfiumError| EngineError::Pdfium(format!("mở gói form: {e}"));

    let mut flattened = 0usize;
    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        for _round in 0..4 {
            let n = page.objects().len();
            let mut form_idxs: Vec<usize> = Vec::new();
            for i in 0..n {
                if let Ok(o) = page.objects().get(i) {
                    if matches!(o, PdfPageObject::XObjectForm(_)) {
                        form_idxs.push(i);
                    }
                }
            }
            if form_idxs.is_empty() {
                break;
            }
            // Duyệt NGƯỢC: xoá form không làm lệch index các form đứng trước.
            for &fi in form_idxs.iter().rev() {
                let fm = {
                    let o = page.objects().get(fi).map_err(err)?;
                    o.matrix().map_err(err)?
                };
                // Rút từng con (luôn con 0) → nhân ma trận form → chèn vào trang
                // (chèn ở CUỐI nên không lệch index form đang xử lý).
                loop {
                    let mut child = {
                        let mut parent = page.objects().get(fi).map_err(err)?;
                        let form = match &mut parent {
                            PdfPageObject::XObjectForm(f) => f,
                            _ => break,
                        };
                        if form.is_empty() {
                            break;
                        }
                        let c = form.get(0).map_err(err)?;
                        form.remove_object(c).map_err(err)?
                    };
                    child.apply_matrix(fm).map_err(err)?;
                    page.objects_mut().add_object(child).map_err(err)?;
                }
                let removed =
                    page.objects_mut().remove_object_at_index(fi).map_err(err)?;
                // Bẫy Drop quen thuộc: object đã tách khỏi trang → forget.
                std::mem::forget(removed);
                flattened += 1;
            }
        }
        if flattened > 0 {
            page.regenerate_content().map_err(err)?;
        }
    }
    if flattened > 0 {
        document
            .save_to_file(output)
            .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    }
    Ok(flattened)
}

/// Kiểu chữ (đậm/nghiêng) của 1 text object, tổng hợp từ weight, italic-angle
/// và tên font. Family lấy từ `name()` (BaseFont khai báo trong PDF) — với
/// font KHÔNG nhúng, `family()` trả tên font stub nội bộ của PDFium ("Chrom
/// Sans OTF") chứ không phải font thật, nên chỉ dùng làm fallback.
fn text_object_style(t: &PdfPageTextObject) -> (String, bool, bool) {
    let raw_name = t.font().name();
    let family_src = if raw_name.trim().is_empty() {
        t.font().family()
    } else {
        raw_name
    };
    let (family, name_bold, name_italic) = fontmatch::clean_font_name(&family_src);
    let weight_bold = matches!(
        t.font().weight(),
        Ok(PdfFontWeight::Weight600)
            | Ok(PdfFontWeight::Weight700Bold)
            | Ok(PdfFontWeight::Weight800)
            | Ok(PdfFontWeight::Weight900)
    ) || matches!(t.font().weight(), Ok(PdfFontWeight::Custom(v)) if v >= 600);
    let angle_italic = t.font().italic_angle().map(|a| a != 0).unwrap_or(false);

    let mut bold = weight_bold || name_bold;
    let mut italic = angle_italic || name_italic;
    // Fallback: nhiều file không khai weight trong descriptor và tên không
    // chứa "Bold"/"Italic" → đọc thẳng từ bytes font nhúng (OS/2 usWeightClass
    // / fsSelection, kể cả khi subset đã vứt OS/2 thì còn head.macStyle).
    if (!bold || !italic) && t.font().is_embedded().unwrap_or(false) {
        if let Ok(bytes) = t.font().data() {
            let (data_bold, data_italic) = fontmatch::style_from_font_bytes(&bytes);
            bold = bold || data_bold;
            italic = italic || data_italic;
        }
    }
    (family, bold, italic)
}

/// Liệt kê các page object của 1 trang để UI vẽ overlay chỉnh sửa.
pub fn list_objects(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    password: Option<&str>,
) -> Result<Vec<ObjectInfo>, EngineError> {
    let document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;
    let page = document
        .pages()
        .get(page_index)
        .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;

    // Danh sách PHẲNG (đi sâu vào Form XObject) — index = vị trí trong list.
    let entries = collect_flat(&page);
    // Form nào có con được liệt kê → expanded (UI bỏ khung của form đó).
    let mut parents_with_children: HashSet<Vec<u16>> = HashSet::new();
    for e in &entries {
        if e.path.len() > 1 {
            parents_with_children.insert(e.path[..e.path.len() - 1].to_vec());
        }
    }

    let mut out = Vec::new();
    // Cache kiểu chữ theo tên font: fallback đậm/nghiêng phải đọc bytes font
    // nhúng — file Word xuất hay có 200+ run chung vài font, không đọc lại.
    let mut style_cache: HashMap<String, (String, bool, bool)> = HashMap::new();
    for (flat, entry) in entries.iter().enumerate() {
        let mut info = ObjectInfo {
            index: flat as u16,
            kind: entry.kind,
            rect: Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 },
            nested: entry.path.len() > 1,
            expanded: parents_with_children.contains(&entry.path),
            text: None,
            font_name: None,
            font_family: None,
            font_bold: None,
            font_italic: None,
            font_embedded: None,
            font_size: None,
            color: None,
        };
        // Không resolve được vẫn giữ 1 chỗ trong list — index phải khớp vị trí.
        let Ok(object) = object_at_path(&page, &entry.path) else {
            out.push(info);
            continue;
        };
        if let Ok(q) = object.bounds() {
            info.rect = mat_rect(entry.acc, &quad_to_rect(&q));
        }
        if let Some(t) = object.as_text_object() {
            let raw_name = t.font().name();
            // Tên rỗng không cache được (nhiều font khác nhau cùng key "").
            let (family, bold, italic) = if raw_name.trim().is_empty() {
                text_object_style(t)
            } else {
                match style_cache.get(&raw_name) {
                    Some(s) => s.clone(),
                    None => {
                        let s = text_object_style(t);
                        style_cache.insert(raw_name.clone(), s.clone());
                        s
                    }
                }
            };
            info.text = Some(t.text());
            info.font_name = Some(raw_name);
            info.font_family = Some(family);
            info.font_bold = Some(bold);
            info.font_italic = Some(italic);
            info.font_embedded = t.font().is_embedded().ok();
            // Cỡ hiển thị = cỡ đã scale của run × scale dọc của form chứa nó.
            info.font_size = Some(t.scaled_font_size().value * mat_vscale(entry.acc));
            info.color = t
                .fill_color()
                .ok()
                .map(|c| [c.red(), c.green(), c.blue(), c.alpha()]);
        }
        out.push(info);
    }
    Ok(out)
}

/// Key nhận diện 1 font thay thế cần nạp: (family chuẩn hoá — rỗng = font mặc
/// định, đậm, nghiêng). Builtin base-14 dùng prefix "b14:" trong family.
type FontKey = (String, bool, bool);

/// Nguồn font cần nạp vào document ở pha (B).
enum FontLoad {
    /// Nhúng TTF/OTF từ bytes (font hệ thống hoặc copy font nhúng gốc).
    Bytes(Vec<u8>),
    /// Font chuẩn base-14 của PDF — KHÔNG nhúng, BaseFont giữ tên chuẩn.
    Builtin(PdfFontBuiltin),
}

/// Map family (đã chuẩn hoá) + kiểu chữ → font chuẩn base-14 tương ứng, nếu
/// family là (hoặc metric-compatible với) một trong Helvetica/Times/Courier.
fn builtin_for(family_key: &str, bold: bool, italic: bool) -> Option<PdfFontBuiltin> {
    use PdfFontBuiltin::*;
    let group = if matches!(family_key, "helvetica" | "helveticaneue" | "arial") {
        0
    } else if matches!(family_key, "times" | "timesroman" | "timesnewroman") {
        1
    } else if matches!(family_key, "courier" | "couriernew") {
        2
    } else {
        return None;
    };
    Some(match (group, bold, italic) {
        (0, false, false) => Helvetica,
        (0, true, false) => HelveticaBold,
        (0, false, true) => HelveticaOblique,
        (0, true, true) => HelveticaBoldOblique,
        (1, false, false) => TimesRoman,
        (1, true, false) => TimesBold,
        (1, false, true) => TimesItalic,
        (1, true, true) => TimesBoldItalic,
        (_, false, false) => Courier,
        (_, true, false) => CourierBold,
        (_, false, true) => CourierOblique,
        (_, true, true) => CourierBoldOblique,
    })
}

/// Font để VẼ các dòng reflow mới.
enum ReflowFont {
    /// Dùng lại CHÍNH font object gốc của PDF (không nạp lại) — giữ nguyên
    /// glyph + mapping charcode gốc. Đường ưu tiên với font nhúng: PDFium
    /// `FPDFText_LoadFont` nạp lại font subset (Word xuất) hay map dấu cách
    /// ra .notdef (□), còn font object gốc thì không. Chỉ chọn sau khi probe
    /// round-trip toàn bộ text thành công.
    Original(PdfFontToken),
    /// Font nạp mới (nhúng lại bytes / base-14 / hệ thống) theo key trong
    /// bảng `tokens`.
    Loaded(FontKey),
}

/// Cỡ chữ 1 dòng gốc (theo baseline, trên→dưới): Tf unscaled + cỡ hiển thị.
#[derive(Clone, Copy)]
struct LineStyle {
    tf: f32,
    scaled: f32,
}

/// Kế hoạch reflow 1 đoạn, dựng ở pha (A) từ hình học các run gốc.
struct ReflowPlan {
    /// Run text hợp lệ sẽ bị thay (đã sort, dedup).
    indices: Vec<u16>,
    /// Font vẽ các dòng mới.
    draw_font: ReflowFont,
    /// Bytes để ĐO bề rộng (hmtx) — có thể khác font vẽ (vd base-14 đo bằng
    /// font metric-compatible). None = xấp xỉ 0.5em/ký tự.
    measure_bytes: Option<Vec<u8>>,
    /// Khối: lề trái + bề rộng bẻ dòng (điểm PDF).
    left: f32,
    width: f32,
    /// Khối gốc căn giữa → dòng mới đặt x = tâm khối − w/2.
    centered: bool,
    /// Baseline dòng đầu (y của gốc text) + khoảng cách baseline giữa các dòng.
    first_baseline: f32,
    line_advance: f32,
    /// Phần tuyến tính matrix của run neo (giữ scale/nghiêng khi tạo dòng mới).
    linear: (f32, f32, f32, f32),
    /// Cỡ Tf cho object mới (unscaled) + cỡ hiển thị (để đo) của run neo —
    /// fallback khi dòng không có style riêng.
    tf: f32,
    scaled: f32,
    /// Cỡ chữ TỪNG DÒNG GỐC (trên→dưới) — dòng cứng thứ i của text mới giữ
    /// đúng cỡ dòng gốc thứ i (tiêu đề 22.5/20pt không bị ép về 1 cỡ).
    line_styles: Vec<LineStyle>,
    color: PdfColor,
}

/// Probe: font `token` có ghi được `text` tròn trịa qua PDFium không?
/// Tạo text object nháp → đọc lại → xoá ngay (mem::forget vì object đã
/// thuộc trang). Bắt được các case PDFium âm thầm vứt ký tự (dấu cách với
/// font subset nạp lại, chữ ngoài ToUnicode...).
fn probe_font_roundtrip(page: &mut PdfPage<'_>, token: PdfFontToken, text: &str) -> bool {
    let probe: String = text.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    let ok = match page.objects_mut().create_text_object(
        PdfPoints::ZERO,
        PdfPoints::ZERO,
        &probe,
        token,
        PdfPoints::new(10.0),
    ) {
        Ok(obj) => obj.as_text_object().map(|t| t.text() == probe).unwrap_or(false),
        Err(_) => return false,
    };
    let idx = page.objects().len().saturating_sub(1);
    match page.objects_mut().remove_object_at_index(idx as usize) {
        Ok(removed) => std::mem::forget(removed),
        Err(_) => return false,
    }
    ok
}

/// Cách xử lý 1 op SetText, quyết định ở pha lập kế hoạch.
enum SetTextMode {
    /// Giữ nguyên font gốc — sửa text/cỡ/màu ngay trên object.
    InPlace,
    /// Phải thay font (glyph thiếu hoặc người dùng đổi family/kiểu) — xoá và
    /// tạo lại object với font `key`.
    Substitute(FontKey),
}

/// Op sửa nhắm vào object TRONG Form XObject → lỗi rõ ràng: PDFium không ghi
/// lại được stream form, mọi sửa trong form là vô nghĩa. UI phải "mở gói"
/// trang bằng `flatten_form_xobjects` trước khi sửa (loadEditPage tự làm).
fn nested_target_err() -> EngineError {
    EngineError::Pdfium(
        "object nằm trong Form XObject — trang chưa được mở gói (flatten) trước khi sửa".into(),
    )
}

/// Dữ liệu chụp lại từ 1 object trước khi xoá để tạo lại bản thay thế.
struct Captured {
    matrix: PdfMatrix,
    unscaled_font_size: f32,
    scaled_font_size: f32,
    color: PdfColor,
    rect: Rect,
}

/// Chọn font thay thế cho (family mong muốn, kiểu chữ, text cần hiển thị):
/// font hệ thống cùng họ nếu có + đủ glyph, ngược lại font mặc định.
fn resolve_substitute_font(
    family: &str,
    bold: bool,
    italic: bool,
    text: &str,
) -> Result<(FontKey, Vec<u8>), EngineError> {
    if !family.trim().is_empty() {
        if let Some(bytes) = fontmatch::find_family_font_bytes(family, bold, italic) {
            if fontmatch::coverage_ok(&bytes, text) {
                return Ok(((fontmatch::normalize_key(family), bold, italic), bytes));
            }
        }
    }
    let bytes = find_font_bytes(bold, italic).ok_or_else(|| {
        EngineError::Pdfium("không tìm được font hệ thống để sửa/thêm chữ".into())
    })?;
    Ok(((String::new(), bold, italic), bytes))
}

/// Áp danh sách `ops` lên trang `page_index` của `input`, ghi ra `output`.
/// Không sửa `input`.
///
/// Trình tự: (A) lập kế hoạch font cho SetText/AddText (đọc trang lần 1) →
/// (B) nạp các font thay thế cần thiết vào document → (C) áp thay đổi (đọc
/// trang lần 2): Transform in-place → SetText in-place (giữ font gốc) → chụp
/// dữ liệu object sắp thay → xoá theo index GIẢM DẦN → thêm bản thay thế →
/// thêm object mới → regenerate → lưu.
pub fn apply_edits(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    ops: &[EditOp],
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let mut document = pdfium
        .load_pdf_from_file(input, password)
        .map_err(|e| EngineError::Pdfium(e.to_string()))?;

    let err = |e: PdfiumError| EngineError::Pdfium(format!("sửa nội dung: {e}"));

    // ---- (A) Lập kế hoạch font (mượn trang lần 1, chỉ đọc) ----
    let mut set_text_modes: HashMap<usize, SetTextMode> = HashMap::new();
    let mut add_text_keys: HashMap<usize, FontKey> = HashMap::new();
    let mut reflow_plans: HashMap<usize, ReflowPlan> = HashMap::new();
    let mut font_needed: HashMap<FontKey, FontLoad> = HashMap::new();
    // Danh sách PHẲNG object (gồm cả con trong Form XObject) — index của mọi
    // op tra vào đây; dùng chung cho cả pha (A) lẫn (C). Probe font ở pha A
    // chỉ thêm+xoá object nháp ở CUỐI trang nên không làm lệch index.
    let entries: Vec<FlatEntry>;
    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        // Pha A chỉ đọc + probe font (tạo/xoá object nháp) — không đụng content.
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        entries = collect_flat(&page);
        let valid = |i: u16| (i as usize) < entries.len();

        for (opi, op) in ops.iter().enumerate() {
            match op {
                EditOp::SetText { index, text, font_family, bold, italic, .. } => {
                    if !valid(*index) {
                        continue;
                    }
                    if entries[*index as usize].path.len() > 1 {
                        return Err(nested_target_err());
                    }
                    let obj = object_at_path(&page, &entries[*index as usize].path)?;
                    let t = match obj.as_text_object() {
                        Some(t) => t,
                        None => continue, // không phải text → bỏ qua op
                    };
                    let (cur_family, cur_bold, cur_italic) = text_object_style(t);
                    let target_bold = bold.unwrap_or(cur_bold);
                    let target_italic = italic.unwrap_or(cur_italic);
                    let style_change = target_bold != cur_bold || target_italic != cur_italic;
                    let family_change = font_family.as_deref().map_or(false, |f| {
                        !f.trim().is_empty()
                            && fontmatch::normalize_key(f) != fontmatch::normalize_key(&cur_family)
                    });

                    let mode = if !style_change && !family_change {
                        let old_text = t.text();
                        let old_chars: HashSet<char> = old_text.chars().collect();
                        let new_covered_by_old = text
                            .chars()
                            .all(|c| c.is_control() || old_chars.contains(&c));
                        // Font KHÔNG nhúng (base-14 Helvetica/Times...): viewer nào
                        // cũng tự thay bằng font hệ thống đủ Latin → re-encode text
                        // ASCII tại chỗ chắc chắn an toàn, BaseFont khai báo giữ
                        // nguyên trong file (đúng hành vi Foxit với base-14).
                        let non_embedded = !t.font().is_embedded().unwrap_or(true);
                        let ascii_only = text
                            .chars()
                            .all(|c| c.is_control() || (' '..='~').contains(&c));
                        if *text == old_text || new_covered_by_old {
                            SetTextMode::InPlace
                        } else if non_embedded && ascii_only {
                            SetTextMode::InPlace
                        } else if t
                            .font()
                            .data()
                            .ok()
                            .map_or(false, |bytes| fontmatch::coverage_ok(&bytes, text))
                        {
                            SetTextMode::InPlace
                        } else {
                            let (key, bytes) = resolve_substitute_font(
                                &cur_family,
                                target_bold,
                                target_italic,
                                text,
                            )?;
                            font_needed.entry(key.clone()).or_insert(FontLoad::Bytes(bytes));
                            SetTextMode::Substitute(key)
                        }
                    } else {
                        let family = font_family
                            .clone()
                            .filter(|f| !f.trim().is_empty())
                            .unwrap_or(cur_family);
                        let (key, bytes) =
                            resolve_substitute_font(&family, target_bold, target_italic, text)?;
                        font_needed.entry(key.clone()).or_insert(FontLoad::Bytes(bytes));
                        SetTextMode::Substitute(key)
                    };
                    set_text_modes.insert(opi, mode);
                }
                EditOp::AddText { text, font_family, bold, italic, .. } => {
                    let family = font_family.clone().unwrap_or_default();
                    let (key, bytes) = resolve_substitute_font(&family, *bold, *italic, text)?;
                    font_needed.entry(key.clone()).or_insert(FontLoad::Bytes(bytes));
                    add_text_keys.insert(opi, key);
                }
                EditOp::ReflowText { indices, text } => {
                    // Run text hợp lệ của khối (sort + dedup).
                    let mut idxs: Vec<u16> = indices.iter().copied().filter(|i| valid(*i)).collect();
                    idxs.sort_unstable();
                    idxs.dedup();
                    if idxs.iter().any(|&i| entries[i as usize].path.len() > 1) {
                        return Err(nested_target_err());
                    }
                    // Hình học từng run: gốc text (e,f của matrix) + bounds.
                    struct RunGeo {
                        idx: u16,
                        f: f32,
                        left: f32,
                        right: f32,
                        tf: f32,
                        scaled: f32,
                    }
                    // Mọi toạ độ/cỡ quy về KHÔNG GIAN TRANG (run trong form
                    // nhân thêm ma trận form) — layout khối tính đúng chỗ.
                    let geo_of = |page: &PdfPage<'_>, i: u16| -> Result<Option<RunGeo>, EngineError> {
                        let entry = &entries[i as usize];
                        let obj = object_at_path(page, &entry.path)?;
                        let Some(t) = obj.as_text_object() else {
                            return Ok(None);
                        };
                        let vs = mat_vscale(entry.acc);
                        let (tf, scaled) =
                            (t.unscaled_font_size().value, t.scaled_font_size().value * vs);
                        let m = obj.matrix().map_err(err)?;
                        let (_, fy) = mat_apply(entry.acc, m.e(), m.f());
                        let b_raw = obj.bounds().map(|q| quad_to_rect(&q)).unwrap_or(Rect {
                            left: m.e(),
                            bottom: m.f(),
                            right: m.e(),
                            top: m.f(),
                        });
                        let b = mat_rect(entry.acc, &b_raw);
                        Ok(Some(RunGeo { idx: i, f: fy, left: b.left, right: b.right, tf, scaled }))
                    };
                    let mut geos: Vec<RunGeo> = Vec::new();
                    for &i in &idxs {
                        if let Some(g) = geo_of(&page, i)? {
                            geos.push(g);
                        }
                    }
                    if geos.is_empty() {
                        continue;
                    }

                    // NỞ danh sách run theo bbox khối (an toàn — chống sót): PDF từ
                    // Word hay cắt 1 dòng thành run theo TỪNG TỪ/KÝ TỰ xen run rỗng,
                    // bbox chữ có dấu tụt thấp — UI có thể sót vài run. Mọi text
                    // object có TÂM nằm trong bbox khối (nới 2pt) đều thuộc khối →
                    // đưa hết vào để xoá sạch, không còn chữ cũ đè dưới chữ mới.
                    {
                        // Rect trang của 1 entry text (None nếu không phải text
                        // hoặc nằm trong form — sau flatten không còn, guard).
                        let text_rect = |i: u16| -> Option<Rect> {
                            let entry = &entries[i as usize];
                            if entry.kind != ObjectKind::Text || entry.path.len() > 1 {
                                return None;
                            }
                            let obj = object_at_path(&page, &entry.path).ok()?;
                            obj.bounds().ok().map(|q| mat_rect(entry.acc, &quad_to_rect(&q)))
                        };
                        let mut bb = Rect {
                            left: f32::INFINITY,
                            bottom: f32::INFINITY,
                            right: f32::NEG_INFINITY,
                            top: f32::NEG_INFINITY,
                        };
                        for &i in &idxs {
                            if let Some(r) = text_rect(i) {
                                bb.left = bb.left.min(r.left);
                                bb.bottom = bb.bottom.min(r.bottom);
                                bb.right = bb.right.max(r.right);
                                bb.top = bb.top.max(r.top);
                            }
                        }
                        let pad = 2.0;
                        for i in 0..entries.len() as u16 {
                            if idxs.contains(&i) {
                                continue;
                            }
                            let Some(r) = text_rect(i) else { continue };
                            let (cx, cy) = ((r.left + r.right) / 2.0, (r.bottom + r.top) / 2.0);
                            if cx >= bb.left - pad
                                && cx <= bb.right + pad
                                && cy >= bb.bottom - pad
                                && cy <= bb.top + pad
                            {
                                idxs.push(i);
                                if let Some(g) = geo_of(&page, i)? {
                                    geos.push(g);
                                }
                            }
                        }
                        idxs.sort_unstable();
                    }
                    // Run neo: baseline cao nhất, trái nhất.
                    let anchor_idx = geos
                        .iter()
                        .max_by(|a, b| {
                            a.f.partial_cmp(&b.f)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then(b.left.partial_cmp(&a.left).unwrap_or(std::cmp::Ordering::Equal))
                        })
                        .map(|g| g.idx)
                        .unwrap_or(idxs[0]);
                    // Chụp mọi thuộc tính của run neo trong 1 block để nhả mượn
                    // trang (probe font phía dưới cần &mut page). Ma trận/cỡ
                    // COMPOSE với ma trận form (run neo có thể nằm trong form;
                    // dòng mới vẽ ở CẤP TRANG nên cần toạ độ trang).
                    let (family, bold, italic, mc, unscaled, scaled, color, font_embedded, orig_token, font_data) = {
                        let entry = &entries[anchor_idx as usize];
                        let anchor = object_at_path(&page, &entry.path)?;
                        let t = anchor.as_text_object().expect("anchor là text");
                        let (family, bold, italic) = text_object_style(t);
                        let m = anchor.matrix().map_err(err)?;
                        let mc = mat_mul(mat_of(&m), entry.acc);
                        let tok = t.font().token();
                        let unscaled = t.unscaled_font_size().value;
                        let scaled =
                            (t.scaled_font_size().value * mat_vscale(entry.acc)).max(1.0);
                        let color = t.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
                        let embedded = t.font().is_embedded().unwrap_or(false);
                        let data = t.font().data().ok();
                        (family, bold, italic, mc, unscaled, scaled, color, embedded, tok, data)
                    };

                    // Baseline: gom cụm giá trị f (dung sai 1pt) → dòng; advance =
                    // median hiệu 2 cụm kề nhau; 1 dòng → 1.25 × cỡ hiển thị.
                    let mut baselines: Vec<f32> = Vec::new();
                    let mut fs: Vec<f32> = geos.iter().map(|g| g.f).collect();
                    fs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                    for f in fs {
                        if baselines.last().map_or(true, |&p| (p - f).abs() > 1.0) {
                            baselines.push(f);
                        }
                    }
                    let line_advance = if baselines.len() >= 2 {
                        let mut diffs: Vec<f32> =
                            baselines.windows(2).map(|w| (w[0] - w[1]).abs()).collect();
                        diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        diffs[diffs.len() / 2].max(1.0)
                    } else {
                        scaled * 1.25
                    };
                    let first_baseline = baselines[0];
                    let left = geos.iter().map(|g| g.left).fold(f32::INFINITY, f32::min);
                    let right = geos.iter().map(|g| g.right).fold(f32::NEG_INFINITY, f32::max);
                    let width = (right - left).max(20.0);

                    // Phát hiện CĂN GIỮA (tiêu đề...): mọi dòng gốc có tâm ≈ tâm
                    // khối và có ít nhất 1 dòng thụt vào so với lề trái → các dòng
                    // mới cũng đặt căn giữa (giữ bố cục như Foxit).
                    let block_center = (left + right) / 2.0;
                    let centered = {
                        let tol = (width * 0.02).max(2.0);
                        let mut any_indented = false;
                        let mut all_centered = true;
                        for &bl in &baselines {
                            let (mut l, mut r) = (f32::INFINITY, f32::NEG_INFINITY);
                            for g in geos.iter().filter(|g| (g.f - bl).abs() <= 1.0) {
                                l = l.min(g.left);
                                r = r.max(g.right);
                            }
                            if !l.is_finite() || r - l < 1.0 {
                                continue;
                            }
                            if ((l + r) / 2.0 - block_center).abs() > tol {
                                all_centered = false;
                            }
                            if l - left > tol {
                                any_indented = true;
                            }
                        }
                        all_centered && any_indented
                    };

                    // Cỡ chữ TỪNG DÒNG gốc (theo baseline, trên→dưới): dòng cứng
                    // thứ i của text mới giữ đúng cỡ dòng gốc thứ i (tiêu đề
                    // 22.5/20pt không bị ép hết về cỡ run neo).
                    let line_styles: Vec<LineStyle> = baselines
                        .iter()
                        .map(|&bl| {
                            geos.iter()
                                .filter(|g| (g.f - bl).abs() <= 1.0 && g.scaled > 0.5)
                                .max_by(|a, b| {
                                    a.scaled
                                        .partial_cmp(&b.scaled)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .map(|g| LineStyle { tf: g.tf.max(1.0), scaled: g.scaled.max(1.0) })
                                .unwrap_or(LineStyle { tf: unscaled.max(1.0), scaled })
                        })
                        .collect();

                    // Thang font cho các dòng mới (giữ font như SetText):
                    // (0) font NHÚNG + probe ghi/đọc lại tròn trịa → dùng LẠI chính
                    //     font object gốc (glyph + charcode y hệt, file không phình;
                    //     tránh lỗi PDFium nạp lại subset làm dấu cách thành □);
                    // (1) font NHÚNG parse được + phủ đủ glyph → nhúng lại bytes đó;
                    // (2) family thuộc nhóm base-14 + text ASCII → font chuẩn PDF;
                    // (3) font hệ thống CÙNG HỌ; (4) fallback mặc định.
                    let fam_key = fontmatch::normalize_key(&family);
                    let ascii_only =
                        text.chars().all(|c| c.is_control() || (' '..='~').contains(&c));
                    let original_ok =
                        font_embedded && probe_font_roundtrip(&mut page, orig_token, text);
                    let embedded_bytes = if font_embedded {
                        font_data.clone().filter(|b| fontmatch::coverage_ok(b, text))
                    } else {
                        None
                    };
                    let (draw_font, measure_bytes) = if original_ok {
                        // Đo vẫn bằng bytes font nhúng (hmtx) nếu parse được.
                        let measure =
                            font_data.filter(|b| ttf_parser::Face::parse(b, 0).is_ok());
                        (ReflowFont::Original(orig_token), measure)
                    } else if let Some(bytes) = embedded_bytes {
                        let key = (format!("emb:{fam_key}:{anchor_idx}"), bold, italic);
                        font_needed.entry(key.clone()).or_insert(FontLoad::Bytes(bytes.clone()));
                        (ReflowFont::Loaded(key), Some(bytes))
                    } else if let (Some(builtin), true) =
                        (builtin_for(&fam_key, bold, italic), ascii_only)
                    {
                        // Đo bằng font hệ thống metric-compatible (Liberation/Arial…).
                        let measure = fontmatch::find_family_font_bytes(&family, bold, italic)
                            .or_else(|| find_font_bytes(bold, italic));
                        let key = (format!("b14:{fam_key}"), bold, italic);
                        font_needed.entry(key.clone()).or_insert(FontLoad::Builtin(builtin));
                        (ReflowFont::Loaded(key), measure)
                    } else {
                        let (key, bytes) = resolve_substitute_font(&family, bold, italic, text)?;
                        font_needed.entry(key.clone()).or_insert(FontLoad::Bytes(bytes.clone()));
                        (ReflowFont::Loaded(key), Some(bytes))
                    };

                    reflow_plans.insert(
                        opi,
                        ReflowPlan {
                            indices: idxs,
                            draw_font,
                            measure_bytes,
                            left,
                            width,
                            centered,
                            first_baseline,
                            line_advance,
                            linear: (mc.0, mc.1, mc.2, mc.3),
                            tf: unscaled.max(1.0),
                            scaled,
                            line_styles,
                            color,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    // ---- (B) Nạp các font cần dùng (cần fonts_mut → ngoài mượn trang) ----
    let mut tokens: HashMap<FontKey, PdfFontToken> = HashMap::new();
    for (key, load) in &font_needed {
        let token = match load {
            FontLoad::Bytes(bytes) => document
                .fonts_mut()
                .load_true_type_from_bytes(bytes, true)
                .map_err(|e| EngineError::Pdfium(format!("nạp font: {e}")))?,
            FontLoad::Builtin(builtin) => document.fonts_mut().new_built_in(*builtin),
        };
        tokens.insert(key.clone(), token);
    }

    // ---- (C) Áp thay đổi (mượn trang lần 2) ----
    {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| EngineError::Pdfium(format!("không lấy được trang {page_index}: {e}")))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        let valid = |i: u16| (i as usize) < entries.len();
        let is_nested = |i: u16| entries[i as usize].path.len() > 1;

        // (1) Transform in-place (không đổi cấu trúc/index). Object TRONG form
        // chưa hỗ trợ di chuyển/resize (dx/dy tính theo trang còn matrix con
        // theo không gian form) — bỏ qua, UI cũng đã chặn kéo.
        for op in ops {
            if let EditOp::Transform { index, dx, dy, sx, sy } = op {
                if !valid(*index) || is_nested(*index) {
                    continue;
                }
                let mut obj = object_at_path(&page, &entries[*index as usize].path)?;
                if (*sx - 1.0).abs() > f32::EPSILON || (*sy - 1.0).abs() > f32::EPSILON {
                    let b = obj.bounds().map_err(err)?;
                    let (x0, y0) = (b.left().value, b.bottom().value);
                    // Scale quanh góc dưới-trái: về gốc → scale → về chỗ cũ.
                    obj.translate(PdfPoints::new(-x0), PdfPoints::new(-y0)).map_err(err)?;
                    obj.scale(*sx, *sy).map_err(err)?;
                    obj.translate(PdfPoints::new(x0), PdfPoints::new(y0)).map_err(err)?;
                }
                if *dx != 0.0 || *dy != 0.0 {
                    obj.translate(PdfPoints::new(*dx), PdfPoints::new(*dy)).map_err(err)?;
                }
            }
        }

        // (2) SetText IN-PLACE — giữ nguyên font gốc (chuẩn Foxit). Đổi cỡ chữ
        // = scale matrix quanh gốc text (giữ điểm neo baseline e,f).
        for (opi, op) in ops.iter().enumerate() {
            let EditOp::SetText { index, text, font_size, color, .. } = op else {
                continue;
            };
            if !matches!(set_text_modes.get(&opi), Some(SetTextMode::InPlace)) {
                continue;
            }
            let entry = &entries[*index as usize];
            let mut obj = object_at_path(&page, &entry.path)?;
            // Cỡ hiển thị so theo KHÔNG GIAN TRANG (nhân scale form) — target
            // từ UI cũng theo trang; hệ số k không đổi khi áp trong form.
            let (old_text, scaled) = match obj.as_text_object() {
                Some(t) => (t.text(), t.scaled_font_size().value * mat_vscale(entry.acc)),
                None => continue,
            };
            if *text != old_text {
                obj.as_text_object_mut()
                    .ok_or_else(|| EngineError::Pdfium("object không phải text".into()))?
                    .set_text(text)
                    .map_err(err)?;
            }
            if let Some(target) = font_size {
                if scaled > 0.01 && (*target - scaled).abs() > 0.01 {
                    let k = *target / scaled;
                    let m = obj.matrix().map_err(err)?;
                    // PDFium post-multiply: M' = M · [k,0,0,k, e(1−k), f(1−k)]
                    // → phần tuyến tính nhân k (đổi cỡ), translation (e,f) giữ
                    // nguyên (neo tại gốc baseline như Foxit).
                    obj.apply_matrix(PdfMatrix::new(
                        k,
                        0.0,
                        0.0,
                        k,
                        m.e() * (1.0 - k),
                        m.f() * (1.0 - k),
                    ))
                    .map_err(err)?;
                }
            }
            if let Some(c) = color {
                obj.set_fill_color(PdfColor::new(c[0], c[1], c[2], c[3])).map_err(err)?;
            }
        }

        // (3) Chụp dữ liệu cho object sắp THAY (SetText-substitute/ReplaceImage)
        // — sau Transform để bản thay thế kế thừa cả phép biến đổi đó.
        let mut captured: HashMap<u16, Captured> = HashMap::new();
        for (opi, op) in ops.iter().enumerate() {
            let idx = match op {
                EditOp::SetText { index, .. }
                    if matches!(set_text_modes.get(&opi), Some(SetTextMode::Substitute(_))) =>
                {
                    *index
                }
                EditOp::ReplaceImage { index, .. } => *index,
                _ => continue,
            };
            if !valid(idx) || captured.contains_key(&idx) {
                continue;
            }
            let entry = &entries[idx as usize];
            let obj = object_at_path(&page, &entry.path)?;
            // Rect + matrix COMPOSE về trang: bản thay thế được tạo ở cấp
            // trang nên phải mang sẵn toạ độ trang (kể cả khi bản gốc trong form).
            let rect = obj
                .bounds()
                .map(|q| mat_rect(entry.acc, &quad_to_rect(&q)))
                .unwrap_or(Rect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 });
            let mc = mat_mul(mat_of(&obj.matrix().map_err(err)?), entry.acc);
            let matrix = PdfMatrix::new(mc.0, mc.1, mc.2, mc.3, mc.4, mc.5);
            let vs = mat_vscale(entry.acc);
            let (unscaled, scaled, color) = if let Some(t) = obj.as_text_object() {
                (
                    t.unscaled_font_size().value,
                    t.scaled_font_size().value * vs,
                    t.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255)),
                )
            } else {
                (0.0, 0.0, PdfColor::new(0, 0, 0, 255))
            };
            captured.insert(
                idx,
                Captured { matrix, unscaled_font_size: unscaled, scaled_font_size: scaled, color, rect },
            );
        }

        // (4) Xoá mọi index thuộc SetText-substitute/ReplaceImage/Delete GIẢM DẦN.
        let mut to_remove: Vec<u16> = Vec::new();
        for (opi, op) in ops.iter().enumerate() {
            match op {
                EditOp::SetText { index, .. }
                    if matches!(set_text_modes.get(&opi), Some(SetTextMode::Substitute(_))) =>
                {
                    to_remove.push(*index)
                }
                EditOp::ReplaceImage { index, .. } | EditOp::Delete { index } => {
                    to_remove.push(*index)
                }
                EditOp::ReflowText { .. } => {
                    if let Some(plan) = reflow_plans.get(&opi) {
                        to_remove.extend_from_slice(&plan.indices);
                    }
                }
                _ => {}
            }
        }
        to_remove.retain(|i| valid(*i));
        to_remove.sort_unstable();
        to_remove.dedup();
        // Op xoá nhắm vào object TRONG form: không xử lý được (PDFium không
        // ghi lại stream form) — trang phải được flatten trước, lỗi rõ ràng.
        if to_remove.iter().any(|&i| is_nested(i)) {
            return Err(nested_target_err());
        }
        // Cấp trang: xoá thật theo index trang GIẢM DẦN.
        let mut page_idxs: Vec<u16> = to_remove
            .iter()
            .map(|&i| entries[i as usize].path[0])
            .collect();
        page_idxs.sort_unstable();
        page_idxs.dedup();
        for idx in page_idxs.into_iter().rev() {
            let removed = page.objects_mut().remove_object_at_index(idx as usize).map_err(err)?;
            // BẪY PDFium: object vừa tách khỏi trang bị đánh dấu "unowned" → Drop
            // gọi FPDFPageObj_Destroy, mà destroy object vốn thuộc document gây
            // SEGFAULT. Object đã tách nên không còn render/lưu; `forget` để khỏi
            // destroy (rò rỉ nhỏ, giải phóng khi đóng document).
            std::mem::forget(removed);
        }

        // (5) Thêm bản thay thế cho SetText-substitute / ReplaceImage.
        for (opi, op) in ops.iter().enumerate() {
            match op {
                EditOp::SetText { index, text, font_size, color, .. } => {
                    let key = match set_text_modes.get(&opi) {
                        Some(SetTextMode::Substitute(key)) => key,
                        _ => continue,
                    };
                    let cap = match captured.get(index) {
                        Some(c) => c,
                        None => continue,
                    };
                    let token = tokens.get(key).copied().ok_or_else(|| {
                        EngineError::Pdfium("thiếu token font cho SetText".into())
                    })?;
                    // Quy đổi cỡ hiển thị mong muốn → cỡ unscaled trước khi áp
                    // matrix gốc (matrix có thể chứa scale — tránh phóng đại kép).
                    let tf = match font_size {
                        Some(target) if cap.scaled_font_size > 0.01 => {
                            cap.unscaled_font_size * (*target / cap.scaled_font_size)
                        }
                        Some(target) => *target,
                        None => cap.unscaled_font_size,
                    }
                    .max(1.0);
                    let mut obj = page
                        .objects_mut()
                        .create_text_object(
                            PdfPoints::ZERO,
                            PdfPoints::ZERO,
                            text.clone(),
                            token,
                            PdfPoints::new(tf),
                        )
                        .map_err(err)?;
                    // Object mới tạo tại gốc (0,0) → matrix ~identity, nên
                    // apply_matrix(matrix gốc) đặt lại đúng vị trí/scale.
                    obj.apply_matrix(cap.matrix).map_err(err)?;
                    let c = color
                        .map(|c| PdfColor::new(c[0], c[1], c[2], c[3]))
                        .unwrap_or(cap.color);
                    obj.set_fill_color(c).map_err(err)?;
                }
                EditOp::ReplaceImage { index, image_path } => {
                    let cap = match captured.get(index) {
                        Some(c) => c,
                        None => continue,
                    };
                    let img = image::open(image_path)
                        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh {image_path}: {e}")))?;
                    let w = (cap.rect.right - cap.rect.left).max(1.0);
                    let h = (cap.rect.top - cap.rect.bottom).max(1.0);
                    page.objects_mut()
                        .create_image_object(
                            PdfPoints::new(cap.rect.left),
                            PdfPoints::new(cap.rect.bottom),
                            &img,
                            Some(PdfPoints::new(w)),
                            Some(PdfPoints::new(h)),
                        )
                        .map_err(err)?;
                }
                _ => {}
            }
        }

        // (6) Thêm object MỚI (AddText / AddImage).
        for (opi, op) in ops.iter().enumerate() {
            match op {
                EditOp::AddText { x, y, text, font_size, color, .. } => {
                    let token = add_text_keys
                        .get(&opi)
                        .and_then(|k| tokens.get(k))
                        .copied()
                        .ok_or_else(|| EngineError::Pdfium("thiếu token font cho AddText".into()))?;
                    let mut obj = page
                        .objects_mut()
                        .create_text_object(
                            PdfPoints::new(*x),
                            PdfPoints::new(*y),
                            text.clone(),
                            token,
                            PdfPoints::new(font_size.max(1.0)),
                        )
                        .map_err(err)?;
                    obj.set_fill_color(PdfColor::new(color[0], color[1], color[2], color[3]))
                        .map_err(err)?;
                }
                EditOp::AddImage { x, y, width_pt, height_pt, image_path } => {
                    let img = image::open(image_path)
                        .map_err(|e| EngineError::Pdfium(format!("đọc ảnh {image_path}: {e}")))?;
                    page.objects_mut()
                        .create_image_object(
                            PdfPoints::new(*x),
                            PdfPoints::new(*y),
                            &img,
                            Some(PdfPoints::new(width_pt.max(1.0))),
                            Some(PdfPoints::new(height_pt.max(1.0))),
                        )
                        .map_err(err)?;
                }
                EditOp::ReflowText { text, .. } => {
                    let Some(plan) = reflow_plans.get(&opi) else { continue };
                    let token = match &plan.draw_font {
                        ReflowFont::Original(tok) => *tok,
                        ReflowFont::Loaded(key) => tokens.get(key).copied().ok_or_else(|| {
                            EngineError::Pdfium("thiếu token font cho ReflowText".into())
                        })?,
                    };
                    // Đo bề rộng ký tự theo hmtx của font (tại cỡ hiển thị của
                    // TỪNG dòng); không parse được → xấp xỉ 0.5em.
                    let face = plan
                        .measure_bytes
                        .as_deref()
                        .and_then(|b| ttf_parser::Face::parse(b, 0).ok());
                    let measure_at = |c: char, size: f32| match &face {
                        Some(f) => fontmatch::char_advance(f, c, size),
                        None => size * 0.5,
                    };

                    // Bề rộng tối đa 1 dòng có thể NỞ thêm (không bẻ lại) khi
                    // người dùng chỉ gõ thêm vài ký tự: +35% bề rộng khối, chặn
                    // bởi mép trang — hành vi hộp text tự nở của Foxit. Vượt quá
                    // mới bẻ dòng theo đúng bề rộng khối.
                    let page_w = page.width().value;
                    let avail = if plan.centered {
                        let c = plan.left + plan.width / 2.0;
                        2.0 * (c.min(page_w - c) - 8.0).max(0.0)
                    } else {
                        (page_w - plan.left - 8.0).max(0.0)
                    };
                    let grow_limit = (plan.width * 1.35).min(avail.max(plan.width * 1.02));

                    // Dòng cứng thứ i (theo \n từ ô sửa) giữ cỡ chữ dòng gốc thứ i.
                    let mut out_lines: Vec<(String, LineStyle)> = Vec::new();
                    let anchor_style = LineStyle { tf: plan.tf, scaled: plan.scaled };
                    for (li, hard) in text.split('\n').enumerate() {
                        let style = plan
                            .line_styles
                            .get(li)
                            .or(plan.line_styles.last())
                            .copied()
                            .unwrap_or(anchor_style);
                        let w: f32 = hard.chars().map(|c| measure_at(c, style.scaled)).sum();
                        if hard.trim().is_empty() || w <= grow_limit {
                            out_lines.push((hard.trim_end().to_string(), style));
                        } else {
                            let m = |c: char| measure_at(c, style.scaled);
                            for piece in fontmatch::wrap_lines(hard, plan.width, &m) {
                                out_lines.push((piece, style));
                            }
                        }
                    }

                    let (a, b, c2, d) = plan.linear;
                    for (i, (line, style)) in out_lines.iter().enumerate() {
                        if line.is_empty() {
                            continue; // đoạn trống vẫn chiếm 1 nhịp baseline
                        }
                        let y = plan.first_baseline - (i as f32) * plan.line_advance;
                        // Khối căn giữa → từng dòng mới cũng căn giữa theo tâm khối.
                        let line_w: f32 =
                            line.chars().map(|c| measure_at(c, style.scaled)).sum();
                        let x = if plan.centered {
                            let x = plan.left + plan.width / 2.0 - line_w / 2.0;
                            x.max(8.0) // căn giữa nở đều 2 phía, không tràn mép trang
                        } else {
                            plan.left
                        };
                        // Ghi cả dòng; đọc lại NGAY để chắc PDFium không âm thầm
                        // vứt ký tự (dấu cách với font subset...). Hỏng → gỡ và
                        // ghi lại TỪNG TỪ, tự đặt vị trí bằng advance đo được —
                        // không bao giờ ghi glyph dấu cách nữa.
                        let wrote_ok = {
                            let mut obj = page
                                .objects_mut()
                                .create_text_object(
                                    PdfPoints::ZERO,
                                    PdfPoints::ZERO,
                                    line.clone(),
                                    token,
                                    PdfPoints::new(style.tf),
                                )
                                .map_err(err)?;
                            let ok = obj
                                .as_text_object()
                                .map(|t| t.text() == *line)
                                .unwrap_or(false);
                            if ok {
                                obj.apply_matrix(PdfMatrix::new(a, b, c2, d, x, y))
                                    .map_err(err)?;
                                obj.set_fill_color(plan.color).map_err(err)?;
                            }
                            ok
                        };
                        if !wrote_ok {
                            let idx = page.objects().len().saturating_sub(1);
                            let removed = page
                                .objects_mut()
                                .remove_object_at_index(idx as usize)
                                .map_err(err)?;
                            std::mem::forget(removed);
                            let space_w = measure_at(' ', style.scaled);
                            let mut cx = x;
                            for word in line.split_whitespace() {
                                let mut obj = page
                                    .objects_mut()
                                    .create_text_object(
                                        PdfPoints::ZERO,
                                        PdfPoints::ZERO,
                                        word,
                                        token,
                                        PdfPoints::new(style.tf),
                                    )
                                    .map_err(err)?;
                                obj.apply_matrix(PdfMatrix::new(a, b, c2, d, cx, y))
                                    .map_err(err)?;
                                obj.set_fill_color(plan.color).map_err(err)?;
                                cx += word.chars().map(|c| measure_at(c, style.scaled)).sum::<f32>()
                                    + space_w;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        page.regenerate_content().map_err(err)?;
    }

    document
        .save_to_file(output)
        .map_err(|e| EngineError::Pdfium(format!("lưu file: {e}")))?;
    Ok(())
}
