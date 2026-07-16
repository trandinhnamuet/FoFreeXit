//! Chuyển đổi (Phase 7): PDF→ảnh PNG, PDF→TXT, PDF→DOCX (tự viết, mức "giữ
//! text & bố cục cơ bản": mỗi dòng thị giác = 1 đoạn, cỡ chữ xấp xỉ), và cầu
//! nối LibreOffice headless cho Office↔PDF chất lượng cao khi máy có cài.
//!
//! DOCX = zip chứa XML; ta tự ghi zip (method STORE, CRC-32 tự tính) để không
//! thêm dependency — Word/LibreOffice đọc bình thường.

use std::path::{Path, PathBuf};
use std::process::Command;

use pdfium_render::prelude::*;

use crate::EngineError;

/// PDF → mỗi trang 1 file PNG `<stem>-p<N>.png` trong `out_dir`, theo DPI.
pub fn export_images(
    pdfium: &Pdfium,
    input: &Path,
    out_dir: &Path,
    dpi: f32,
    password: Option<&str>,
) -> Result<Vec<PathBuf>, EngineError> {
    std::fs::create_dir_all(out_dir)?;
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".into());
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let mut out = Vec::new();
    for d in &dims {
        let width_px = (d.width_pt / 72.0 * dpi).round().max(16.0) as u32;
        let path = out_dir.join(format!("{stem}-p{}.png", d.index + 1));
        crate::render::render_page_png(pdfium, input, d.index, &path, width_px, password)?;
        out.push(path);
    }
    Ok(out)
}

/// PDF → file text thuần (mỗi trang cách nhau 1 dòng trống).
pub fn export_text(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let mut all = String::new();
    for d in &dims {
        let t = crate::text::extract_text(pdfium, input, d.index, password)?;
        all.push_str(&t);
        all.push_str("\n\n");
    }
    std::fs::write(output, all)?;
    Ok(())
}

/// 1 dòng văn bản đã dựng từ char boxes: nội dung + cỡ chữ xấp xỉ (pt).
struct DocLine {
    text: String,
    size_pt: f32,
}

/// Gom char boxes của 1 trang thành các dòng thị giác (cụm theo baseline,
/// trái→phải, chèn space khi hở ≥ 0.28 em) — cùng heuristic với UI edit.
fn page_lines(
    pdfium: &Pdfium,
    input: &Path,
    page_index: u16,
    password: Option<&str>,
) -> Result<Vec<DocLine>, EngineError> {
    let boxes = crate::text::page_char_boxes(pdfium, input, page_index, password)?;
    // Cụm dòng theo bottom (dung sai nửa chiều cao ký tự).
    struct Line {
        bottom: f32,
        h: f32,
        chars: Vec<(f32, f32, String)>, // (left, right, ch)
    }
    let mut lines: Vec<Line> = Vec::new();
    for b in &boxes {
        if b.ch.trim().is_empty() && b.ch != " " {
            continue;
        }
        let h = (b.top - b.bottom).max(1.0);
        let found = lines.iter_mut().find(|l| (l.bottom - b.bottom).abs() <= l.h.max(h) * 0.5);
        match found {
            Some(l) => {
                l.chars.push((b.left, b.right, b.ch.clone()));
                l.h = l.h.max(h);
            }
            None => lines.push(Line { bottom: b.bottom, h, chars: vec![(b.left, b.right, b.ch.clone())] }),
        }
    }
    // Trên → dưới, trong dòng trái → phải.
    lines.sort_by(|a, b| b.bottom.partial_cmp(&a.bottom).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    for mut l in lines {
        l.chars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut text = String::new();
        let mut prev_right: Option<f32> = None;
        for (left, right, ch) in &l.chars {
            if let Some(pr) = prev_right {
                if left - pr > l.h * 0.28 && !text.ends_with(' ') && ch != " " {
                    text.push(' ');
                }
            }
            text.push_str(ch);
            prev_right = Some(*right);
        }
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(DocLine { text, size_pt: l.h });
        }
    }
    Ok(out)
}

/// PDF → DOCX "đủ dùng": mỗi dòng thị giác = 1 đoạn Word với cỡ chữ xấp xỉ;
/// giữa các trang chèn ngắt trang. Giữ text + thứ tự đọc + cỡ tương đối —
/// không tái tạo cột/bảng/ảnh (ghi rõ trong docs).
pub fn export_docx(
    pdfium: &Pdfium,
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<(), EngineError> {
    let dims = crate::meta::page_dims(pdfium, input, password)?;
    let mut body = String::new();
    for (i, d) in dims.iter().enumerate() {
        if i > 0 {
            body.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
        }
        for line in page_lines(pdfium, input, d.index, password)? {
            // w:sz theo half-point.
            let half_points = (line.size_pt * 2.0).round().clamp(8.0, 144.0) as u32;
            body.push_str(&format!(
                r#"<w:p><w:r><w:rPr><w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
                xml_escape(&line.text),
                hp = half_points,
            ));
        }
    }
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    let entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("word/document.xml", document_xml.as_bytes()),
    ];
    let zip = build_zip_stored(&entries);
    std::fs::write(output, zip)?;
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Zip tối giản, method STORE (0), đủ chuẩn để Word/LibreOffice/unzip đọc.
fn build_zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();

    for (name, data) in entries {
        offsets.push(out.len() as u32);
        let crc = crc32(data);
        let n = name.as_bytes();
        // Local file header.
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method STORE
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(n.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(n);
        out.extend_from_slice(data);
    }
    for (i, (name, data)) in entries.iter().enumerate() {
        let crc = crc32(data);
        let n = name.as_bytes();
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(n.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offsets[i].to_le_bytes());
        central.extend_from_slice(n);
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// CRC-32 (IEEE 802.3) — bảng dựng 1 lần.
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFF
}

// ---- LibreOffice headless (tùy chọn — chất lượng cao khi máy có cài) ----

/// Tìm LibreOffice: env `FOFREEXIT_SOFFICE_PATH` → PATH (`soffice`).
pub fn find_soffice() -> Result<PathBuf, EngineError> {
    let exe = if cfg!(windows) { "soffice.exe" } else { "soffice" };
    if let Ok(p) = std::env::var("FOFREEXIT_SOFFICE_PATH") {
        let p = PathBuf::from(p);
        let candidate = if p.is_dir() { p.join(exe) } else { p };
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if Command::new(exe).arg("--version").output().is_ok() {
        return Ok(PathBuf::from(exe));
    }
    Err(EngineError::Pdfium(
        "không tìm thấy LibreOffice (soffice). Cài LibreOffice hoặc đặt FOFREEXIT_SOFFICE_PATH".into(),
    ))
}

fn run_soffice(args: &[&str]) -> Result<(), EngineError> {
    let soffice = find_soffice()?;
    // Profile riêng để không đụng LibreOffice đang mở của người dùng.
    let profile = std::env::temp_dir().join("ff_soffice_profile");
    let profile_arg = format!(
        "-env:UserInstallation=file://{}",
        profile.to_string_lossy().replace('\\', "/")
    );
    let output = Command::new(&soffice)
        .arg(&profile_arg)
        .args(["--headless", "--norestore"])
        .args(args)
        .output()
        .map_err(|e| EngineError::Pdfium(format!("chạy soffice: {e}")))?;
    if !output.status.success() {
        return Err(EngineError::Pdfium(format!(
            "soffice lỗi (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

/// File kết quả mà soffice --convert-to tạo ra trong out_dir.
fn converted_path(input: &Path, out_dir: &Path, ext: &str) -> PathBuf {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    out_dir.join(format!("{stem}.{ext}"))
}

/// Office (docx/xlsx/pptx/odt...) → PDF. Trả đường dẫn PDF tạo ra.
pub fn office_to_pdf(input: &Path, out_dir: &Path) -> Result<PathBuf, EngineError> {
    std::fs::create_dir_all(out_dir)?;
    run_soffice(&[
        "--convert-to",
        "pdf",
        "--outdir",
        &out_dir.to_string_lossy(),
        &input.to_string_lossy(),
    ])?;
    let out = converted_path(input, out_dir, "pdf");
    if !out.is_file() {
        return Err(EngineError::Pdfium("soffice không tạo được PDF".into()));
    }
    Ok(out)
}

/// PDF → DOCX qua LibreOffice (import filter PDF của Draw/Writer) — chất lượng
/// layout tốt hơn bản tự viết; dùng khi máy có LibreOffice.
pub fn pdf_to_docx_via_soffice(input: &Path, out_dir: &Path) -> Result<PathBuf, EngineError> {
    std::fs::create_dir_all(out_dir)?;
    run_soffice(&[
        "--infilter=writer_pdf_import",
        "--convert-to",
        "docx:MS Word 2007 XML",
        "--outdir",
        &out_dir.to_string_lossy(),
        &input.to_string_lossy(),
    ])?;
    let out = converted_path(input, out_dir, "docx");
    if !out.is_file() {
        return Err(EngineError::Pdfium("soffice không tạo được DOCX".into()));
    }
    Ok(out)
}
