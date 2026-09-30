//! Tệp đính kèm (Foxit: bảng Attachments): liệt kê / thêm / trích xuất / xoá /
//! sửa mô tả các tệp nhúng cấp tài liệu (name tree /Names /EmbeddedFiles →
//! /Filespec /EF /F + /UF UTF-16, /Desc, /Params Size/ModDate/CheckSum) — kèm
//! danh sách chỉ-đọc các chú thích FileAttachment trên trang.

use std::path::Path;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use md5::{Digest, Md5};

use crate::pdfobj::{self, deref, dict_get, dict_of, dict_text, obj_dict};
use crate::EngineError;

/// Một tệp đính kèm.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// Khoá trong name tree (dùng để trích/xoá/sửa). Với FileAttachment annot:
    /// "annot:<trang>:<chỉ số>".
    pub key: String,
    pub file_name: String,
    pub description: String,
    pub size: Option<u64>,
    pub mod_date: Option<String>,
    pub creation_date: Option<String>,
    pub mime: Option<String>,
    /// None = cấp tài liệu; Some(trang) = chú thích FileAttachment.
    pub page_index: Option<u16>,
}

/// Thay đổi chờ ghi.
#[derive(Debug, Clone)]
pub enum AttachmentOp {
    Add { path: std::path::PathBuf, name: Option<String>, description: String },
    Delete { key: String },
    SetDescription { key: String, description: String },
}

fn filespec_info(doc: &Document, fs: &Dictionary) -> (String, String, Option<u64>, Option<String>, Option<String>, Option<String>) {
    let name = dict_text(doc, fs, b"UF")
        .or_else(|| dict_text(doc, fs, b"F"))
        .or_else(|| dict_text(doc, fs, b"Unix"))
        .or_else(|| dict_text(doc, fs, b"DOS"))
        .unwrap_or_default();
    let desc = dict_text(doc, fs, b"Desc").unwrap_or_default();
    let ef = dict_of(doc, fs, b"EF");
    let stream = ef.and_then(|e| dict_get(doc, e, b"UF").or_else(|| dict_get(doc, e, b"F")));
    let (mut size, mut md, mut cd, mut mime) = (None, None, None, None);
    if let Some(Object::Stream(s)) = stream {
        if let Some(p) = dict_of(doc, &s.dict, b"Params") {
            size = dict_get(doc, p, b"Size").and_then(|o| o.as_i64().ok()).map(|v| v as u64);
            md = dict_text(doc, p, b"ModDate").and_then(|d| pdfobj::pdf_date_to_iso(&d));
            cd = dict_text(doc, p, b"CreationDate").and_then(|d| pdfobj::pdf_date_to_iso(&d));
        }
        mime = pdfobj::name_of(doc, &s.dict, b"Subtype").map(|m| m.replace("#2F", "/").replace("#2f", "/"));
        if size.is_none() {
            size = Some(pdfobj::stream_data(s).len() as u64);
        }
    }
    (name, desc, size, md, cd, mime)
}

fn embedded_tree(doc: &Document) -> Vec<(Vec<u8>, Object)> {
    let Ok(cat) = doc.catalog() else { return Vec::new() };
    let Some(names) = dict_of(doc, cat, b"Names") else { return Vec::new() };
    let Some(ef) = dict_of(doc, names, b"EmbeddedFiles") else { return Vec::new() };
    pdfobj::name_tree_entries(doc, ef)
}

fn key_string(k: &[u8]) -> String {
    pdfobj::decode_text(k)
}

/// Liệt kê đính kèm cấp tài liệu + FileAttachment annot.
pub fn list_attachments(input: &Path) -> Result<Vec<Attachment>, EngineError> {
    let doc = pdfobj::load(input)?;
    Ok(list_in(&doc))
}

fn list_in(doc: &Document) -> Vec<Attachment> {
    let mut out = Vec::new();
    for (k, v) in embedded_tree(doc) {
        let Some(fs) = obj_dict(deref(doc, &v)) else { continue };
        let (name, desc, size, md, cd, mime) = filespec_info(doc, fs);
        let key = key_string(&k);
        out.push(Attachment {
            file_name: if name.is_empty() { key.clone() } else { name },
            key,
            description: desc,
            size,
            mod_date: md,
            creation_date: cd,
            mime,
            page_index: None,
        });
    }
    for (pi, pid) in pdfobj::page_ids(doc).into_iter().enumerate() {
        for (ai, (_, d)) in pdfobj::page_annots(doc, pid).into_iter().enumerate() {
            if d.get(b"Subtype").and_then(|o| o.as_name()).ok() != Some(b"FileAttachment") {
                continue;
            }
            let Some(fs) = dict_get(doc, &d, b"FS").and_then(obj_dict) else { continue };
            let (name, desc, size, md, cd, mime) = filespec_info(doc, fs);
            let desc = if desc.is_empty() { dict_text(doc, &d, b"Contents").unwrap_or_default() } else { desc };
            out.push(Attachment {
                key: format!("annot:{pi}:{ai}"),
                file_name: name,
                description: desc,
                size,
                mod_date: md,
                creation_date: cd,
                mime,
                page_index: Some(pi as u16),
            });
        }
    }
    out
}

/// Bytes của tệp đính kèm theo key.
fn attachment_bytes(doc: &Document, key: &str) -> Option<Vec<u8>> {
    let fs: Dictionary = if let Some(rest) = key.strip_prefix("annot:") {
        let mut it = rest.split(':');
        let pi: usize = it.next()?.parse().ok()?;
        let ai: usize = it.next()?.parse().ok()?;
        let pid = *pdfobj::page_ids(doc).get(pi)?;
        let (_, d) = pdfobj::page_annots(doc, pid).into_iter().nth(ai)?;
        dict_get(doc, &d, b"FS").and_then(obj_dict)?.clone()
    } else {
        let (_, v) = embedded_tree(doc).into_iter().find(|(k, _)| key_string(k) == key)?;
        obj_dict(deref(doc, &v))?.clone()
    };
    let ef = dict_of(doc, &fs, b"EF")?;
    match dict_get(doc, ef, b"UF").or_else(|| dict_get(doc, ef, b"F"))? {
        Object::Stream(s) => Some(pdfobj::stream_data(s)),
        _ => None,
    }
}

/// Trích tệp đính kèm `key` ra `output`.
pub fn extract_attachment(input: &Path, key: &str, output: &Path) -> Result<(), EngineError> {
    let doc = pdfobj::load(input)?;
    let bytes = attachment_bytes(&doc, key)
        .ok_or_else(|| EngineError::Pdfium(format!("không tìm thấy tệp đính kèm \"{key}\"")))?;
    std::fs::write(output, bytes)?;
    Ok(())
}

fn hex_string(bytes: &[u8]) -> Object {
    Object::String(bytes.to_vec(), StringFormat::Hexadecimal)
}

/// Tạo /Filespec + /EmbeddedFile cho tệp trên đĩa.
fn make_filespec(doc: &mut Document, path: &Path, name: &str, description: &str) -> Result<ObjectId, EngineError> {
    let data = std::fs::read(path)?;
    let meta = std::fs::metadata(path).ok();
    let mut params = Dictionary::new();
    params.set("Size", Object::Integer(data.len() as i64));
    params.set("CheckSum", hex_string(&Md5::digest(&data)));
    if let Some(m) = &meta {
        if let Ok(t) = m.modified() {
            params.set("ModDate", Object::string_literal(pdfobj::system_time_to_pdf(t)));
        }
        if let Ok(t) = m.created() {
            params.set("CreationDate", Object::string_literal(pdfobj::system_time_to_pdf(t)));
        }
    }
    let mut sd = Dictionary::new();
    sd.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
    if let Some(mime) = guess_mime(name) {
        sd.set("Subtype", Object::Name(mime.as_bytes().to_vec()));
    }
    sd.set("Params", Object::Dictionary(params));
    let mut stream = Stream::new(sd, data);
    let _ = stream.compress();
    let sid = doc.add_object(stream);
    let mut ef = Dictionary::new();
    ef.set("F", Object::Reference(sid));
    ef.set("UF", Object::Reference(sid));
    let mut fs = Dictionary::new();
    fs.set("Type", Object::Name(b"Filespec".to_vec()));
    // /F: tên "cổ điển" (byte) — ASCII giữ nguyên, Unicode dùng UTF-16 như Acrobat.
    fs.set("F", pdfobj::text_obj(name));
    fs.set("UF", pdfobj::text_obj(name));
    if !description.is_empty() {
        fs.set("Desc", pdfobj::text_obj(description));
    }
    fs.set("EF", Object::Dictionary(ef));
    fs.set("AFRelationship", Object::Name(b"Unspecified".to_vec()));
    Ok(doc.add_object(fs))
}

/// MIME theo đuôi tệp (Name trong PDF: "/" viết thành #2F khi ghi).
fn guess_mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "html" | "htm" => "text/html",
        _ => return None,
    })
}

/// Áp các thay đổi đính kèm và ghi `output`. Name tree được ghi lại phẳng,
/// sắp theo khoá (thứ tự byte như chuẩn yêu cầu). Trùng tên khi thêm → thêm
/// hậu tố " (2)", " (3)"…
pub fn apply_attachment_ops(input: &Path, ops: &[AttachmentOp], output: &Path) -> Result<Vec<Attachment>, EngineError> {
    let mut doc = pdfobj::load(input)?;
    let mut entries: Vec<(String, Object)> = embedded_tree(&doc).into_iter().map(|(k, v)| (key_string(&k), v)).collect();
    for op in ops {
        match op {
            AttachmentOp::Delete { key } => {
                if key.starts_with("annot:") {
                    return Err(EngineError::Pdfium("tệp đính kèm dạng chú thích chỉ xoá được qua chú thích".into()));
                }
                entries.retain(|(k, _)| k != key);
            }
            AttachmentOp::SetDescription { key, description } => {
                let Some((_, v)) = entries.iter().find(|(k, _)| k == key) else { continue };
                let target = v.as_reference().ok();
                let set = |d: &mut Dictionary| {
                    if description.is_empty() {
                        d.remove(b"Desc");
                    } else {
                        d.set("Desc", pdfobj::text_obj(description));
                    }
                };
                match target {
                    Some(id) => {
                        if let Ok(d) = doc.get_dictionary_mut(id) {
                            set(d);
                        }
                    }
                    None => {
                        if let Some((_, Object::Dictionary(d))) = entries.iter_mut().find(|(k, _)| k == key) {
                            set(d);
                        }
                    }
                }
            }
            AttachmentOp::Add { path, name, description } => {
                let base = name
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| path.file_name().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "attachment".into());
                let mut key = base.clone();
                let mut i = 2;
                while entries.iter().any(|(k, _)| *k == key) {
                    key = match base.rsplit_once('.') {
                        Some((stem, ext)) => format!("{stem} ({i}).{ext}"),
                        None => format!("{base} ({i})"),
                    };
                    i += 1;
                }
                let id = make_filespec(&mut doc, path, &key, description)?;
                entries.push((key, Object::Reference(id)));
            }
        }
    }
    // Sắp theo byte của khoá đã mã hoá.
    let mut encoded: Vec<(Vec<u8>, Object)> = entries
        .into_iter()
        .map(|(k, v)| (match pdfobj::text_obj(&k) { Object::String(b, _) => b, _ => k.into_bytes() }, v))
        .collect();
    encoded.sort_by(|a, b| a.0.cmp(&b.0));
    let rid = pdfobj::root_id(&doc).ok_or_else(|| EngineError::Pdfium("thiếu catalog".into()))?;
    let names_ref = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"Names").ok()).and_then(|o| o.as_reference().ok());
    let mut names = match names_ref {
        Some(id) => doc.get_dictionary(id).ok().cloned().unwrap_or_default(),
        None => doc.get_dictionary(rid).ok().and_then(|c| dict_of(&doc, c, b"Names").cloned()).unwrap_or_default(),
    };
    if encoded.is_empty() {
        names.remove(b"EmbeddedFiles");
    } else {
        let mut arr = Vec::with_capacity(encoded.len() * 2);
        for (k, v) in encoded {
            arr.push(Object::String(k, StringFormat::Hexadecimal));
            arr.push(v);
        }
        let mut tree = Dictionary::new();
        tree.set("Names", Object::Array(arr));
        let tid = doc.add_object(tree);
        names.set("EmbeddedFiles", Object::Reference(tid));
    }
    match names_ref {
        Some(id) => {
            if let Ok(d) = doc.get_dictionary_mut(id) {
                *d = names;
            }
        }
        None => {
            if let Ok(c) = doc.get_dictionary_mut(rid) {
                if names.is_empty() {
                    c.remove(b"Names");
                } else {
                    c.set("Names", Object::Dictionary(names));
                }
            }
        }
    }
    doc.prune_objects();
    pdfobj::save(&mut doc, output)?;
    Ok(list_in(&doc))
}

/// Đuôi tệp có thể chạy mã (mở bằng ứng dụng mặc định cần cảnh báo).
pub fn is_risky_file_name(name: &str) -> bool {
    const RISKY: &[&str] = &[
        "exe", "com", "bat", "cmd", "scr", "pif", "msi", "msp", "cpl", "js", "jse", "vbs", "vbe", "wsf", "wsh", "ps1",
        "psm1", "hta", "jar", "lnk", "reg", "dll", "sys", "sh", "app", "apk", "gadget", "inf", "url", "iso", "vhd",
    ];
    name.rsplit('.').next().map(|e| RISKY.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false) && name.contains('.')
}
