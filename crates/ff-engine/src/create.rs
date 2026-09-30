//! Tạo PDF (như "Create PDF" của Foxit): từ ảnh (JPG/PNG/BMP/GIF/TIFF nhiều
//! trang/WebP), trang trắng, tệp văn bản (.txt UTF-8, tiếng Việt), trang web /
//! tệp HTML (Edge/Chrome headless), và "Gộp nhiều tệp thành PDF" (PDF + ảnh +
//! Office + văn bản lẫn lộn → chuyển từng tệp rồi trộn).
//!
//! PDF được dựng TRỰC TIẾP ở cấp object bằng lopdf (không qua PDFium) để kiểm
//! soát chất lượng/kích thước: JPEG được nhúng NGUYÊN VẸN (DCTDecode, không nén
//! lại — không mất chất lượng, không phình file); ảnh khác nén Flate, ảnh
//! trắng-đen thuần (scan nhị phân) ghi 1 bit/điểm, kênh alpha → /SMask.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use image::{DynamicImage, GenericImageView};
use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId, Stream, StringFormat};
use pdfium_render::prelude::*;

use crate::EngineError;

fn err(msg: impl Into<String>) -> EngineError {
    EngineError::Pdfium(msg.into())
}

// ---------------------------------------------------------------------------
// Khổ giấy & tuỳ chọn
// ---------------------------------------------------------------------------

/// Khổ giấy chuẩn (điểm PDF, dọc): a3/a4/a5/b5/letter/legal/tabloid.
pub fn standard_page_size(name: &str) -> Option<(f32, f32)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "a3" => (841.89, 1190.55),
        "a4" => (595.28, 841.89),
        "a5" => (419.53, 595.28),
        "b5" => (498.90, 708.66),
        "letter" => (612.0, 792.0),
        "legal" => (612.0, 1008.0),
        "tabloid" => (792.0, 1224.0),
        _ => return None,
    })
}

/// Hướng trang khi dùng khổ cố định.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Theo ảnh: ảnh ngang → trang ngang.
    Auto,
    Portrait,
    Landscape,
}

/// Kích thước trang cho PDF tạo từ ảnh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageSizeMode {
    /// Trang vừa khít ảnh (theo DPI của ảnh) + lề.
    FitImage,
    /// Khổ cố định (điểm PDF, dọc) — ảnh thu nhỏ cho vừa, căn giữa.
    Fixed { width_pt: f32, height_pt: f32 },
}

#[derive(Clone, Debug)]
pub struct ImagePdfOptions {
    pub page: PageSizeMode,
    pub orientation: Orientation,
    /// Lề mỗi phía (điểm PDF).
    pub margin_pt: f32,
    /// Nhúng JPEG nguyên bản (không giải nén/nén lại).
    pub jpeg_passthrough: bool,
    /// Phóng to ảnh nhỏ cho đầy khổ cố định (mặc định chỉ thu nhỏ).
    pub enlarge_small: bool,
}

impl Default for ImagePdfOptions {
    fn default() -> Self {
        ImagePdfOptions {
            page: PageSizeMode::FitImage,
            orientation: Orientation::Auto,
            margin_pt: 0.0,
            jpeg_passthrough: true,
            enlarge_small: false,
        }
    }
}

/// Áp hướng lên khổ dọc (w ≤ h) theo tỉ lệ nội dung.
fn oriented(w: f32, h: f32, orientation: Orientation, content_landscape: bool) -> (f32, f32) {
    let (short, long) = (w.min(h), w.max(h));
    let landscape = match orientation {
        Orientation::Auto => content_landscape,
        Orientation::Portrait => false,
        Orientation::Landscape => true,
    };
    if landscape { (long, short) } else { (short, long) }
}

// ---------------------------------------------------------------------------
// Bộ dựng PDF tối giản bằng lopdf
// ---------------------------------------------------------------------------

struct PdfBuilder {
    doc: LoDoc,
    pages_id: ObjectId,
    kids: Vec<Object>,
}

impl PdfBuilder {
    fn new() -> Self {
        let mut doc = LoDoc::with_version("1.7");
        let pages_id = doc.new_object_id();
        PdfBuilder { doc, pages_id, kids: Vec::new() }
    }

    fn add_page(&mut self, w: f32, h: f32, content: Vec<u8>, resources: Dictionary) {
        let mut sd = Dictionary::new();
        let body = compress(&content);
        let stream = if body.len() + 20 < content.len() {
            sd.set("Filter", Object::Name(b"FlateDecode".to_vec()));
            Stream::new(sd, body)
        } else {
            Stream::new(sd, content)
        };
        let content_id = self.doc.add_object(Object::Stream(stream));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(self.pages_id));
        page.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]),
        );
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Reference(content_id));
        let id = self.doc.add_object(Object::Dictionary(page));
        self.kids.push(Object::Reference(id));
    }

    fn finish(mut self, output: &Path) -> Result<u32, EngineError> {
        let count = self.kids.len() as u32;
        if count == 0 {
            return Err(err("không có trang nào để ghi"));
        }
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(count as i64));
        pages.set("Kids", Object::Array(self.kids));
        self.doc.objects.insert(self.pages_id, Object::Dictionary(pages));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(self.pages_id));
        let catalog_id = self.doc.add_object(Object::Dictionary(catalog));
        let mut info = Dictionary::new();
        info.set("Producer", Object::String(b"FoFreeXit".to_vec(), StringFormat::Literal));
        info.set("Creator", Object::String(b"FoFreeXit".to_vec(), StringFormat::Literal));
        info.set("CreationDate", Object::String(pdf_date().into_bytes(), StringFormat::Literal));
        let info_id = self.doc.add_object(Object::Dictionary(info));
        self.doc.trailer.set("Root", Object::Reference(catalog_id));
        self.doc.trailer.set("Info", Object::Reference(info_id));
        let mut f = std::fs::File::create(output)?;
        self.doc
            .save_to(&mut f)
            .map_err(|e| err(format!("ghi PDF: {e}")))?;
        Ok(count)
    }
}

fn pdf_date() -> String {
    let (y, m, d, hh, mm, ss) = utc_now();
    format!("D:{y:04}{m:02}{d:02}{hh:02}{mm:02}{ss:02}Z")
}

/// Thời điểm hiện tại dạng W3CDTF (docProps/core.xml của OOXML).
pub(crate) fn iso_now() -> String {
    let (y, m, d, hh, mm, ss) = utc_now();
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Tem thời gian ns — đặt tên tệp/thư mục tạm không trùng.
pub(crate) fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// (năm, tháng, ngày, giờ, phút, giây) UTC — ngày dương lịch từ số ngày kể từ
/// 1970 (thuật toán civil_from_days).
fn utc_now() -> (i64, i64, i64, i64, i64, i64) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Đọc ảnh
// ---------------------------------------------------------------------------

/// Dữ liệu 1 "trang ảnh" sẵn sàng nhúng.
enum ImgData {
    /// JPEG nguyên bản: số kênh màu + cờ Adobe (CMYK đảo).
    Jpeg { data: Vec<u8>, components: u8, adobe: bool },
    Raw(DynamicImage),
}

struct PageImage {
    data: ImgData,
    w: u32,
    h: u32,
    /// DPI (x, y) nếu tệp có khai báo.
    dpi: Option<(f32, f32)>,
}

/// Thông tin rút từ header JPEG.
struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    precision: u8,
    sof: u8,
    adobe: bool,
    orientation: u16,
    dpi: Option<(f32, f32)>,
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// Đọc orientation (tag 0x0112) từ khối EXIF (bắt đầu bằng header TIFF).
fn exif_orientation(tiff: &[u8]) -> Option<u16> {
    let le = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r16 = |i: usize| -> Option<u16> {
        let s = tiff.get(i..i + 2)?;
        Some(if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    };
    let r32 = |i: usize| -> Option<u32> {
        let s = tiff.get(i..i + 4)?;
        Some(if le {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };
    let ifd = r32(4)? as usize;
    let n = r16(ifd)? as usize;
    for k in 0..n.min(512) {
        let e = ifd + 2 + k * 12;
        if r16(e)? == 0x0112 {
            return r16(e + 8);
        }
    }
    None
}

fn parse_jpeg(b: &[u8]) -> Option<JpegInfo> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut info = JpegInfo {
        width: 0,
        height: 0,
        components: 0,
        precision: 0,
        sof: 0,
        adobe: false,
        orientation: 1,
        dpi: None,
    };
    let mut i = 2usize;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = be16(b, i + 2)? as usize;
        let seg = b.get(i + 4..i + 2 + len)?;
        match marker {
            0xE0 if seg.starts_with(b"JFIF\0") && seg.len() >= 12 => {
                let units = seg[7];
                let (xd, yd) = (be16(seg, 8)? as f32, be16(seg, 10)? as f32);
                if xd > 1.0 && yd > 1.0 {
                    info.dpi = match units {
                        1 => Some((xd, yd)),
                        2 => Some((xd * 2.54, yd * 2.54)),
                        _ => None,
                    };
                }
            }
            0xE1 if seg.starts_with(b"Exif\0\0") => {
                if let Some(o) = exif_orientation(&seg[6..]) {
                    info.orientation = o;
                }
            }
            0xEE if seg.starts_with(b"Adobe") => info.adobe = true,
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => {
                info.sof = marker;
                info.precision = *seg.first()?;
                info.height = be16(seg, 1)? as u32;
                info.width = be16(seg, 3)? as u32;
                info.components = *seg.get(5)?;
            }
            0xDA => break,
            _ => {}
        }
        i += 2 + len;
    }
    if info.width == 0 || info.height == 0 {
        return None;
    }
    Some(info)
}

/// DPI từ chunk pHYs của PNG (đơn vị mét).
fn png_dpi(b: &[u8]) -> Option<(f32, f32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut i = 8usize;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize;
        let ty = b.get(i + 4..i + 8)?;
        if ty == b"pHYs" && len >= 9 {
            let d = b.get(i + 8..i + 17)?;
            let x = u32::from_be_bytes(d[0..4].try_into().ok()?) as f32;
            let y = u32::from_be_bytes(d[4..8].try_into().ok()?) as f32;
            if d[8] == 1 && x > 0.0 && y > 0.0 {
                return Some((x * 0.0254, y * 0.0254));
            }
            return None;
        }
        if ty == b"IDAT" {
            return None;
        }
        i += 12 + len;
    }
    None
}

fn is_tiff(b: &[u8]) -> bool {
    b.starts_with(b"II*\0") || b.starts_with(b"MM\0*")
}

/// Giải 1 trang TIFF (vị trí hiện tại của decoder) thành DynamicImage.
fn tiff_page<R: std::io::Read + std::io::Seek>(
    dec: &mut tiff::decoder::Decoder<R>,
) -> Result<DynamicImage, EngineError> {
    use tiff::decoder::DecodingResult as D;
    use tiff::ColorType as C;
    let (w, h) = dec.dimensions().map_err(|e| err(format!("TIFF: {e}")))?;
    let ct = dec.colortype().map_err(|e| err(format!("TIFF: {e}")))?;
    let data = dec.read_image().map_err(|e| err(format!("TIFF: {e}")))?;
    let bad = || err("TIFF: dữ liệu ảnh không khớp kích thước");
    let to8 = |v: Vec<u16>| v.into_iter().map(|x| (x >> 8) as u8).collect::<Vec<u8>>();
    let img = match (ct, data) {
        (C::Gray(1), D::U8(v)) => {
            // 1 bit/điểm, mỗi hàng đệm tới byte; decoder đã đảo WhiteIsZero.
            let row = (w as usize).div_ceil(8);
            let mut out = Vec::with_capacity((w * h) as usize);
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let byte = v.get(y * row + x / 8).copied().unwrap_or(0);
                    out.push(if byte & (0x80 >> (x % 8)) != 0 { 255 } else { 0 });
                }
            }
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, out).ok_or_else(bad)?)
        }
        (C::Gray(8), D::U8(v)) => DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, v).ok_or_else(bad)?),
        (C::Gray(16), D::U16(v)) => {
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, to8(v)).ok_or_else(bad)?)
        }
        (C::GrayA(8), D::U8(v)) => {
            DynamicImage::ImageLumaA8(image::GrayAlphaImage::from_raw(w, h, v).ok_or_else(bad)?)
        }
        (C::RGB(8), D::U8(v)) => DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, v).ok_or_else(bad)?),
        (C::RGB(16), D::U16(v)) => DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, to8(v)).ok_or_else(bad)?),
        (C::RGBA(8), D::U8(v)) => DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, v).ok_or_else(bad)?),
        (C::RGBA(16), D::U16(v)) => {
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, to8(v)).ok_or_else(bad)?)
        }
        (C::CMYK(8), D::U8(v)) => {
            let mut out = Vec::with_capacity((w * h * 3) as usize);
            for px in v.chunks_exact(4) {
                let k = 255 - px[3] as u32;
                for c in &px[..3] {
                    out.push(((255 - *c as u32) * k / 255) as u8);
                }
            }
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, out).ok_or_else(bad)?)
        }
        (ct, _) => return Err(err(format!("TIFF: kiểu màu chưa hỗ trợ {ct:?}"))),
    };
    Ok(img)
}

fn tiff_dpi<R: std::io::Read + std::io::Seek>(dec: &mut tiff::decoder::Decoder<R>) -> Option<(f32, f32)> {
    use tiff::decoder::ifd::Value;
    use tiff::tags::Tag;
    let rat = |v: Option<Value>| -> Option<f32> {
        match v? {
            Value::Rational(n, d) if d != 0 => Some(n as f32 / d as f32),
            Value::Short(n) => Some(n as f32),
            Value::Unsigned(n) => Some(n as f32),
            _ => None,
        }
    };
    let x = rat(dec.find_tag(Tag::XResolution).ok().flatten())?;
    let y = rat(dec.find_tag(Tag::YResolution).ok().flatten()).unwrap_or(x);
    let unit = dec.find_tag_unsigned::<u16>(Tag::ResolutionUnit).ok().flatten().unwrap_or(2);
    let k = match unit {
        3 => 2.54,
        2 => 1.0,
        _ => return None,
    };
    if x > 1.0 && y > 1.0 { Some((x * k, y * k)) } else { None }
}

/// Số trang (frame) của 1 tệp ảnh — TIFF nhiều trang > 1, còn lại 1.
pub fn image_page_count(path: &Path) -> Result<usize, EngineError> {
    let bytes = std::fs::read(path)?;
    if !is_tiff(&bytes) {
        return Ok(1);
    }
    let mut dec = tiff::decoder::Decoder::new(Cursor::new(&bytes)).map_err(|e| err(format!("TIFF: {e}")))?;
    let mut n = 1;
    while dec.more_images() {
        dec.next_image().map_err(|e| err(format!("TIFF: {e}")))?;
        n += 1;
    }
    Ok(n)
}

/// Đọc mọi "trang" của tệp ảnh (TIFF nhiều trang → nhiều trang).
fn load_image_pages(path: &Path, passthrough: bool) -> Result<Vec<PageImage>, EngineError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(path)?;

    if let Some(info) = parse_jpeg(&bytes) {
        let ok_sof = matches!(info.sof, 0xC0..=0xC2);
        if passthrough
            && ok_sof
            && info.precision == 8
            && matches!(info.components, 1 | 3 | 4)
            && info.orientation <= 1
        {
            return Ok(vec![PageImage {
                w: info.width,
                h: info.height,
                dpi: info.dpi,
                data: ImgData::Jpeg { components: info.components, adobe: info.adobe, data: bytes },
            }]);
        }
    }

    if is_tiff(&bytes) {
        let mut dec = tiff::decoder::Decoder::new(Cursor::new(&bytes))
            .map_err(|e| err(format!("{name}: TIFF: {e}")))?
            .with_limits(tiff::decoder::Limits::unlimited());
        let mut pages = Vec::new();
        loop {
            let dpi = tiff_dpi(&mut dec);
            let img = match tiff_page(&mut dec) {
                Ok(i) => i,
                // Trang đầu có kiểu lạ → thử bộ giải của crate image.
                Err(e) if pages.is_empty() => image::load_from_memory(&bytes).map_err(|_| e)?,
                Err(e) => return Err(err(format!("{name}: {e}"))),
            };
            let (w, h) = img.dimensions();
            pages.push(PageImage { data: ImgData::Raw(img), w, h, dpi });
            if !dec.more_images() {
                break;
            }
            dec.next_image().map_err(|e| err(format!("{name}: TIFF: {e}")))?;
        }
        return Ok(pages);
    }

    let dpi = png_dpi(&bytes).or_else(|| parse_jpeg(&bytes).and_then(|i| i.dpi));
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| err(format!("{name}: {e}")))?;
    use image::ImageDecoder;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| err(format!("không đọc được ảnh {name}: {e}")))?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| err(format!("không đọc được ảnh {name}: {e}")))?;
    img.apply_orientation(orientation);
    let (w, h) = img.dimensions();
    Ok(vec![PageImage { data: ImgData::Raw(img), w, h, dpi }])
}

/// Tạo object ảnh (XObject) trong doc, trả id.
fn add_image_xobject(doc: &mut LoDoc, img: &PageImage) -> ObjectId {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Image".to_vec()));
    d.set("Width", Object::Integer(img.w as i64));
    d.set("Height", Object::Integer(img.h as i64));
    match &img.data {
        ImgData::Jpeg { data, components, adobe } => {
            let cs: &[u8] = match components {
                1 => b"DeviceGray",
                4 => b"DeviceCMYK",
                _ => b"DeviceRGB",
            };
            d.set("ColorSpace", Object::Name(cs.to_vec()));
            d.set("BitsPerComponent", Object::Integer(8));
            d.set("Filter", Object::Name(b"DCTDecode".to_vec()));
            if *components == 4 && *adobe {
                // JPEG CMYK của Photoshop lưu đảo — như mọi trình đọc PDF.
                d.set(
                    "Decode",
                    Object::Array([1, 0, 1, 0, 1, 0, 1, 0].iter().map(|&v| Object::Integer(v)).collect()),
                );
            }
            doc.add_object(Object::Stream(Stream::new(d, data.clone()).with_compression(false)))
        }
        ImgData::Raw(im) => {
            let has_alpha = im.color().has_alpha();
            let gray = !im.color().has_color();
            let mut smask_id = None;
            if has_alpha {
                let rgba = im.to_rgba8();
                let alpha: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();
                if alpha.iter().any(|&a| a != 255) {
                    let mut md = Dictionary::new();
                    md.set("Type", Object::Name(b"XObject".to_vec()));
                    md.set("Subtype", Object::Name(b"Image".to_vec()));
                    md.set("Width", Object::Integer(img.w as i64));
                    md.set("Height", Object::Integer(img.h as i64));
                    md.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
                    md.set("BitsPerComponent", Object::Integer(8));
                    md.set("Filter", Object::Name(b"FlateDecode".to_vec()));
                    smask_id = Some(doc.add_object(Object::Stream(
                        Stream::new(md, compress(&alpha)).with_compression(false),
                    )));
                }
            }
            let (cs, bpc, raw): (&[u8], i64, Vec<u8>) = if gray {
                let l = im.to_luma8();
                if l.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255) {
                    // Ảnh nhị phân (scan trắng-đen): đóng gói 1 bit/điểm.
                    let row = (img.w as usize).div_ceil(8);
                    let mut packed = vec![0u8; row * img.h as usize];
                    for (x, y, p) in l.enumerate_pixels() {
                        if p.0[0] == 255 {
                            packed[y as usize * row + x as usize / 8] |= 0x80 >> (x % 8);
                        }
                    }
                    (b"DeviceGray", 1, packed)
                } else {
                    (b"DeviceGray", 8, l.into_raw())
                }
            } else {
                (b"DeviceRGB", 8, im.to_rgb8().into_raw())
            };
            d.set("ColorSpace", Object::Name(cs.to_vec()));
            d.set("BitsPerComponent", Object::Integer(bpc));
            d.set("Filter", Object::Name(b"FlateDecode".to_vec()));
            if let Some(id) = smask_id {
                d.set("SMask", Object::Reference(id));
            }
            doc.add_object(Object::Stream(Stream::new(d, compress(&raw)).with_compression(false)))
        }
    }
}

/// Tính khổ trang + vị trí vẽ ảnh: (page_w, page_h, x, y, draw_w, draw_h).
fn place_image(img_w_pt: f32, img_h_pt: f32, opts: &ImagePdfOptions) -> (f32, f32, f32, f32, f32, f32) {
    let m = opts.margin_pt.max(0.0);
    match opts.page {
        PageSizeMode::FitImage => (img_w_pt + 2.0 * m, img_h_pt + 2.0 * m, m, m, img_w_pt, img_h_pt),
        PageSizeMode::Fixed { width_pt, height_pt } => {
            let (pw, ph) = oriented(width_pt, height_pt, opts.orientation, img_w_pt > img_h_pt);
            let (aw, ah) = ((pw - 2.0 * m).max(1.0), (ph - 2.0 * m).max(1.0));
            let mut s = (aw / img_w_pt).min(ah / img_h_pt);
            if s > 1.0 && !opts.enlarge_small {
                s = 1.0;
            }
            let (dw, dh) = (img_w_pt * s, img_h_pt * s);
            (pw, ph, (pw - dw) / 2.0, (ph - dh) / 2.0, dw, dh)
        }
    }
}

/// Ảnh → PDF: mỗi ảnh (mỗi trang TIFF) một trang, theo đúng thứ tự `images`.
/// Trả số trang đã ghi.
pub fn images_to_pdf(images: &[PathBuf], opts: &ImagePdfOptions, output: &Path) -> Result<u32, EngineError> {
    if images.is_empty() {
        return Err(err("chưa chọn ảnh nào"));
    }
    let mut b = PdfBuilder::new();
    for path in images {
        for page in load_image_pages(path, opts.jpeg_passthrough)? {
            let (dx, dy) = page.dpi.unwrap_or((96.0, 96.0));
            let img_w_pt = page.w as f32 * 72.0 / dx.clamp(10.0, 4800.0);
            let img_h_pt = page.h as f32 * 72.0 / dy.clamp(10.0, 4800.0);
            // PDF giới hạn trang 14400pt (200 inch) — ảnh cực lớn thì thu lại.
            let k = (14000.0 / img_w_pt.max(img_h_pt)).min(1.0);
            let (pw, ph, x, y, w, h) = place_image(img_w_pt * k, img_h_pt * k, opts);
            let id = add_image_xobject(&mut b.doc, &page);
            let mut xo = Dictionary::new();
            xo.set("Im1", Object::Reference(id));
            let mut res = Dictionary::new();
            res.set("XObject", Object::Dictionary(xo));
            let content = format!("q {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /Im1 Do Q");
            b.add_page(pw, ph, content.into_bytes(), res);
        }
    }
    b.finish(output)
}

/// PDF trắng: `count` trang khổ `width_pt`×`height_pt`.
pub fn blank_pdf(width_pt: f32, height_pt: f32, count: u32, output: &Path) -> Result<u32, EngineError> {
    if !(1..=2000).contains(&count) {
        return Err(err("số trang phải từ 1 đến 2000"));
    }
    let (w, h) = (width_pt.clamp(72.0, 14400.0), height_pt.clamp(72.0, 14400.0));
    let mut b = PdfBuilder::new();
    for _ in 0..count {
        b.add_page(w, h, Vec::new(), Dictionary::new());
    }
    b.finish(output)
}

// ---------------------------------------------------------------------------
// Văn bản → PDF
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TextPdfOptions {
    /// Khổ trang (đã tính hướng), điểm PDF.
    pub width_pt: f32,
    pub height_pt: f32,
    pub margin_pt: f32,
    pub font_size: f32,
    /// Khoảng cách dòng (bội số cỡ chữ).
    pub line_spacing: f32,
    /// Font TTF tuỳ chọn; None = font đóng gói (Noto) / font hệ thống.
    pub font_path: Option<PathBuf>,
}

impl Default for TextPdfOptions {
    fn default() -> Self {
        TextPdfOptions {
            width_pt: 595.28,
            height_pt: 841.89,
            margin_pt: 56.7,
            font_size: 11.0,
            line_spacing: 1.3,
            font_path: None,
        }
    }
}

/// Giải mã tệp văn bản: BOM UTF-8/UTF-16; không BOM thì UTF-8, sai UTF-8 thì
/// Windows-1252 (mỗi byte 1 ký tự — không bao giờ lỗi).
pub fn decode_text_bytes(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| -> String {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

/// Ngắt 1 dòng văn bản theo bề rộng: ưu tiên ngắt ở khoảng trắng, từ quá dài
/// thì cắt theo ký tự. Giữ thụt đầu dòng.
fn wrap_text_line(line: &str, width_of: &dyn Fn(&str) -> f32, max_w: f32) -> Vec<String> {
    if line.trim().is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    // Tách thành token giữ khoảng trắng (từ + khoảng trắng theo sau).
    let mut tokens: Vec<String> = Vec::new();
    let mut tok = String::new();
    let mut in_space = false;
    for ch in line.chars() {
        let sp = ch == ' ';
        if !tok.is_empty() && !sp && in_space {
            tokens.push(std::mem::take(&mut tok));
        }
        in_space = sp;
        tok.push(ch);
    }
    if !tok.is_empty() {
        tokens.push(tok);
    }
    for t in tokens {
        let cand = format!("{cur}{t}");
        if width_of(cand.trim_end()) <= max_w {
            cur = cand;
            continue;
        }
        if !cur.trim().is_empty() {
            out.push(cur.trim_end().to_string());
        }
        // Token dài hơn cả dòng → cắt theo ký tự.
        let mut piece = String::new();
        for ch in t.chars() {
            piece.push(ch);
            if width_of(piece.trim_end()) > max_w && piece.chars().count() > 1 {
                piece.pop();
                out.push(piece.trim_end().to_string());
                piece = ch.to_string();
            }
        }
        cur = piece;
    }
    if !cur.trim().is_empty() || out.is_empty() {
        out.push(cur.trim_end().to_string());
    }
    out
}

/// Dựng /ToUnicode (GID → Unicode) để trích/tìm/copy chữ đúng.
fn to_unicode_cmap(map: &BTreeMap<u16, char>) -> Vec<u8> {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = map.iter().collect();
    for chunk in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            let mut buf = [0u16; 2];
            let hex: String = ch.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{:04X}> <{hex}>\n", gid));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// Tệp văn bản (.txt…) → PDF, tự ngắt dòng + sang trang, font Unicode nhúng
/// (đủ tiếng Việt). Trả số trang.
pub fn text_to_pdf(input: &Path, opts: &TextPdfOptions, output: &Path) -> Result<u32, EngineError> {
    let bytes = std::fs::read(input)?;
    let text = decode_text_bytes(&bytes).replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
    text_string_to_pdf(&text, opts, output)
}

/// Chuỗi văn bản → PDF (lõi của `text_to_pdf`).
pub fn text_string_to_pdf(text: &str, opts: &TextPdfOptions, output: &Path) -> Result<u32, EngineError> {
    let font_bytes = match &opts.font_path {
        Some(p) => std::fs::read(p)?,
        None => crate::annot::find_font_bytes(false, false)
            .ok_or_else(|| err("không tìm được font Unicode (Noto/hệ thống) để tạo PDF văn bản"))?,
    };
    let face = ttf_parser::Face::parse(&font_bytes, 0).map_err(|e| err(format!("font lỗi: {e:?}")))?;
    let fs = opts.font_size.clamp(4.0, 72.0);
    let lead = fs * opts.line_spacing.clamp(1.0, 3.0);
    let (pw, ph) = (opts.width_pt.clamp(144.0, 14400.0), opts.height_pt.clamp(144.0, 14400.0));
    let m = opts.margin_pt.clamp(0.0, pw.min(ph) / 3.0);
    let max_w = pw - 2.0 * m;
    let width_of = |s: &str| -> f32 { s.chars().map(|c| crate::annot::char_adv(c, &face, fs)).sum() };

    let mut lines: Vec<String> = Vec::new();
    for l in text.split('\n') {
        // Ký tự điều khiển (trừ form feed = sang trang) bỏ đi.
        let clean: String = l.chars().filter(|c| !c.is_control() || *c == '\u{c}').collect();
        for part in clean.split_inclusive('\u{c}') {
            let brk = part.ends_with('\u{c}');
            let body = part.trim_end_matches('\u{c}');
            lines.extend(wrap_text_line(body, &width_of, max_w));
            if brk {
                lines.push("\u{c}".into());
            }
        }
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) && lines.len() > 1 {
        lines.pop();
    }

    let mut b = PdfBuilder::new();
    let used: String = lines.concat();
    let font_id = crate::annot::embed_type0_font(&mut b.doc, &font_bytes, &used)?;
    // Bổ sung /ToUnicode + tên font thật.
    let mut gmap: BTreeMap<u16, char> = BTreeMap::new();
    for ch in used.chars().chain([' ', '?']) {
        if let Some(g) = face.glyph_index(ch) {
            gmap.entry(g.0).or_insert(ch);
        }
    }
    let tu_id = b.doc.add_object(Object::Stream(Stream::new(Dictionary::new(), to_unicode_cmap(&gmap))));
    if let Ok(Object::Dictionary(d)) = b.doc.get_object_mut(font_id) {
        d.set("ToUnicode", Object::Reference(tu_id));
    }

    let per_page = (((ph - 2.0 * m - fs) / lead).floor() as usize + 1).max(1);
    let mut pages: Vec<Vec<&str>> = vec![Vec::new()];
    for l in &lines {
        if l == "\u{c}" {
            pages.push(Vec::new());
            continue;
        }
        if pages.last().map(|p| p.len() >= per_page).unwrap_or(false) {
            pages.push(Vec::new());
        }
        pages.last_mut().expect("có trang").push(l);
    }
    if pages.len() > 1 && pages.last().map(|p| p.is_empty()).unwrap_or(false) {
        pages.pop();
    }
    for page_lines in pages {
        let mut c = format!("BT /F1 {fs:.2} Tf {lead:.2} TL {m:.2} {:.2} Td\n", ph - m - fs);
        for (i, l) in page_lines.iter().enumerate() {
            if i > 0 {
                c.push_str("T*\n");
            }
            if l.is_empty() {
                continue;
            }
            let hex: String = crate::annot::encode_cid(l, &face).iter().map(|b| format!("{b:02X}")).collect();
            c.push_str(&format!("<{hex}> Tj\n"));
        }
        c.push_str("ET");
        let mut fonts = Dictionary::new();
        fonts.set("F1", Object::Reference(font_id));
        let mut res = Dictionary::new();
        res.set("Font", Object::Dictionary(fonts));
        b.add_page(pw, ph, c.into_bytes(), res);
    }
    b.finish(output)
}

// ---------------------------------------------------------------------------
// Trang web / HTML → PDF (Edge/Chrome headless)
// ---------------------------------------------------------------------------

/// Tìm trình duyệt Chromium (Edge ưu tiên — luôn có trên Windows 10/11):
/// env `FOFREEXIT_BROWSER_PATH` → vị trí cài chuẩn → PATH.
pub fn find_browser() -> Result<PathBuf, EngineError> {
    if let Ok(p) = std::env::var("FOFREEXIT_BROWSER_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    let mut cands: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Ok(base) = std::env::var(var) {
                let base = PathBuf::from(base);
                cands.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
                cands.push(base.join(r"Google\Chrome\Application\chrome.exe"));
                cands.push(base.join(r"Chromium\Application\chrome.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        cands.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
        cands.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
        cands.push("/Applications/Chromium.app/Contents/MacOS/Chromium".into());
    }
    if let Some(found) = cands.into_iter().find(|p| p.is_file()) {
        return Ok(found);
    }
    let names: &[&str] = if cfg!(windows) {
        &["msedge.exe", "chrome.exe"]
    } else {
        &["microsoft-edge", "microsoft-edge-stable", "google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                let p = dir.join(n);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err(err(
        "không tìm thấy Microsoft Edge hoặc Google Chrome để chuyển trang web sang PDF. Cài Edge/Chrome hoặc đặt FOFREEXIT_BROWSER_PATH",
    ))
}

/// Chuẩn hoá nguồn thành URL: tệp cục bộ → file:///, thiếu scheme → https://.
pub fn normalize_web_source(source: &str) -> Result<String, EngineError> {
    let s = source.trim();
    if s.is_empty() {
        return Err(err("chưa nhập địa chỉ trang web"));
    }
    let p = Path::new(s);
    if p.is_file() {
        let abs = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let mut path = abs.to_string_lossy().replace('\\', "/");
        if let Some(rest) = path.strip_prefix("//?/") {
            path = rest.to_string();
        }
        // Percent-encode theo byte UTF-8 (tên thư mục tiếng Việt, dấu cách…).
        let enc: String = path
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        let sep = if enc.starts_with('/') { "file://" } else { "file:///" };
        return Ok(format!("{sep}{enc}"));
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file:///") {
        return Ok(s.to_string());
    }
    if lower.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") {
        return Err(err("chỉ hỗ trợ địa chỉ http://, https:// hoặc tệp HTML trên máy"));
    }
    if s.contains(' ') || !s.contains('.') {
        return Err(err(format!("địa chỉ không hợp lệ hoặc tệp không tồn tại: {s}")));
    }
    Ok(format!("https://{s}"))
}

/// Trang web (URL) hoặc tệp HTML cục bộ → PDF bằng Edge/Chrome headless
/// (`--print-to-pdf`). `wait_ms`: thời gian cho trang chạy script/tải ảnh.
pub fn html_to_pdf(source: &str, output: &Path, wait_ms: u32) -> Result<(), EngineError> {
    let url = normalize_web_source(source)?;
    let browser = find_browser()?;
    let stamp = format!(
        "{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp_pdf = std::env::temp_dir().join(format!("ff_web_{stamp}.pdf"));
    // Hồ sơ riêng: không "bám" vào cửa sổ Edge/Chrome đang mở của người dùng.
    let profile = std::env::temp_dir().join(format!("ff_web_profile_{stamp}"));
    let mut cmd = Command::new(&browser);
    cmd.arg("--headless")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--hide-scrollbars")
        .arg("--no-pdf-header-footer")
        .arg("--print-to-pdf-no-header")
        .arg("--run-all-compositor-stages-before-draw")
        .arg(format!("--virtual-time-budget={}", wait_ms.clamp(0, 60000)))
        .arg(format!("--user-data-dir={}", profile.to_string_lossy()))
        .arg(format!("--print-to-pdf={}", tmp_pdf.to_string_lossy()));
    if !cfg!(windows) {
        cmd.arg("--no-sandbox");
    }
    cmd.arg(&url);
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| err(format!("chạy trình duyệt {}: {e}", browser.display())))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(90_000 + wait_ms as u64);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(150)),
            Err(e) => return Err(err(format!("trình duyệt: {e}"))),
        }
    };
    let _ = std::fs::remove_dir_all(&profile);
    let produced = std::fs::read(&tmp_pdf).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp_pdf);
    if status.is_none() {
        return Err(err("trình duyệt chạy quá lâu khi chuyển trang web (quá thời gian chờ)"));
    }
    if !produced.starts_with(b"%PDF") {
        let mut detail = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut detail);
        }
        let detail: String = detail.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
        return Err(err(format!("trình duyệt không tạo được PDF từ {url}. {detail}")));
    }
    std::fs::write(output, produced)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Gộp nhiều tệp (PDF + ảnh + Office + văn bản + HTML) thành 1 PDF
// ---------------------------------------------------------------------------

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "bmp", "gif", "tif", "tiff", "webp"];
const TEXT_EXTS: &[&str] = &["txt", "text", "log", "md", "csv", "ini", "json", "xml"];
const OFFICE_EXTS: &[&str] = &[
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "vsd", "vsdx", "pub",
];
const HTML_EXTS: &[&str] = &["htm", "html", "xhtml", "mht", "mhtml"];

/// Loại tệp cho "Gộp tệp": "pdf" | "image" | "text" | "office" | "html" | "".
pub fn source_kind(path: &Path) -> &'static str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "pdf" {
        "pdf"
    } else if IMAGE_EXTS.contains(&ext.as_str()) {
        "image"
    } else if TEXT_EXTS.contains(&ext.as_str()) {
        "text"
    } else if OFFICE_EXTS.contains(&ext.as_str()) {
        "office"
    } else if HTML_EXTS.contains(&ext.as_str()) {
        "html"
    } else {
        ""
    }
}

#[derive(Clone, Debug, Default)]
pub struct CombineOptions {
    pub image: ImagePdfOptions,
    pub text: TextPdfOptions,
}

/// Chuyển từng tệp sang PDF (nếu cần) rồi trộn theo đúng thứ tự. Trả số trang.
pub fn combine_files(
    pdfium: &Pdfium,
    files: &[PathBuf],
    opts: &CombineOptions,
    output: &Path,
) -> Result<u32, EngineError> {
    if files.is_empty() {
        return Err(err("chưa chọn tệp nào để gộp"));
    }
    let work = std::env::temp_dir().join(format!(
        "ff_combine_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&work)?;
    let result = (|| -> Result<u32, EngineError> {
        let mut pdfs: Vec<PathBuf> = Vec::new();
        for (i, f) in files.iter().enumerate() {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let tmp = work.join(format!("part{i}.pdf"));
            let wrap = |e: EngineError| err(format!("{name}: {e}"));
            match source_kind(f) {
                "pdf" => {
                    pdfs.push(f.clone());
                    continue;
                }
                "image" => {
                    images_to_pdf(std::slice::from_ref(f), &opts.image, &tmp).map_err(wrap)?;
                }
                "text" => {
                    text_to_pdf(f, &opts.text, &tmp).map_err(wrap)?;
                }
                "html" => html_to_pdf(&f.to_string_lossy(), &tmp, 2000).map_err(wrap)?,
                "office" => {
                    let dir = work.join(format!("office{i}"));
                    let produced = crate::convert::office_to_pdf(f, &dir).map_err(wrap)?;
                    std::fs::rename(&produced, &tmp).or_else(|_| std::fs::copy(&produced, &tmp).map(|_| ()))?;
                }
                _ => return Err(err(format!("{name}: định dạng tệp chưa hỗ trợ"))),
            }
            pdfs.push(tmp);
        }
        crate::organize::merge_files(pdfium, &pdfs, output)?;
        Ok(crate::render::page_count(pdfium, output, None)? as u32)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}
