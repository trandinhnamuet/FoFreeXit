//! "PHẪU THUẬT" content stream của Form XObject bằng lopdf (Phase 4 iter 4b):
//! xoá đúng các op vẽ chữ (Tj/TJ/'/") của những text-child bị sửa — mọi byte
//! khác của form giữ NGUYÊN. Đây là đường duy nhất sửa nội dung trong form an
//! toàn: PDFium không ghi lại được stream form, còn generator ghi cả trang
//! thì lossy với file phức tạp (font subset trùng tên, ảnh inline — docs/12).
//!
//! Nguyên tắc map: parser PDFium tạo object theo ĐÚNG thứ tự op trong stream
//! → text-child thứ k của 1 form ↔ show-op thứ k trong stream form đó; form
//! con thứ n ↔ Do-trỏ-tới-form thứ n. Mỗi bước đều có BẤT BIẾN kiểm đếm
//! (số op == số object PDFium thấy) — lệch là từ chối ngay, không đoán mò.

use std::collections::HashMap;
use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId};

use crate::EngineError;

fn le<E: std::fmt::Display>(e: E) -> EngineError {
    EngineError::Pdfium(format!("phẫu thuật form (lopdf): {e}"))
}

/// 1 việc xoá text trong 1 form (đường đi từ trang xuống form trong cùng).
pub(crate) struct SurgeryJob {
    /// Tại mỗi mức: (thứ tự FORM trong container, tổng số form PDFium thấy
    /// trong container đó — bất biến kiểm đếm).
    pub chain: Vec<(usize, usize)>,
    /// Tổng số text-child PDFium thấy trong form trong cùng (bất biến).
    pub expected_texts: usize,
    /// Các show-op (thứ tự trong form trong cùng) cần xoá.
    pub delete_ordinals: Vec<usize>,
}

fn is_text_show(op: &str) -> bool {
    matches!(op, "Tj" | "TJ" | "'" | "\"")
}

/// Resolve reference (tối đa 8 nấc).
fn deref<'a>(doc: &'a LoDoc, mut o: &'a Object) -> &'a Object {
    for _ in 0..8 {
        match o {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(x) => o = x,
                Err(_) => break,
            },
            _ => break,
        }
    }
    o
}

/// Id các XObject FORM theo đúng thứ tự Do trong `content` (chỉ đếm Do trỏ
/// tới stream /Subtype /Form qua `resources`).
fn form_dos_in_order(
    doc: &LoDoc,
    content: &Content,
    resources: Option<&Dictionary>,
) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let xobjects = resources
        .and_then(|r| r.get(b"XObject").ok())
        .map(|x| deref(doc, x))
        .and_then(|x| x.as_dict().ok());
    let Some(xobjects) = xobjects else { return out };
    for op in &content.operations {
        if op.operator != "Do" {
            continue;
        }
        let Some(Object::Name(name)) = op.operands.first() else { continue };
        let Ok(entry) = xobjects.get(name) else { continue };
        let Object::Reference(id) = entry else { continue };
        let id = *id;
        let Ok(obj) = doc.get_object(id) else { continue };
        let Ok(stream) = obj.as_stream() else { continue };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|s| s.as_name().ok());
        if subtype == Some(b"Form") {
            out.push(id);
        }
    }
    out
}

/// /Resources của 1 stream form (fallback: resources của container cha).
fn stream_resources<'a>(
    doc: &'a LoDoc,
    stream_id: ObjectId,
    parent: Option<&'a Dictionary>,
) -> Option<&'a Dictionary> {
    let stream = doc.get_object(stream_id).ok()?.as_stream().ok()?;
    match stream.dict.get(b"Resources") {
        Ok(r) => deref(doc, r).as_dict().ok().or(parent),
        Err(_) => parent,
    }
}

/// Áp danh sách job xoá-text-trong-form lên `input`, ghi ra `output`.
pub(crate) fn delete_form_text_ops(
    input: &Path,
    page_index: u16,
    jobs: &[SurgeryJob],
    output: &Path,
) -> Result<(), EngineError> {
    let mut doc = LoDoc::load(input).map_err(le)?;
    let pages = doc.get_pages();
    let page_id = *pages
        .get(&(page_index as u32 + 1))
        .ok_or_else(|| le(format!("không có trang {page_index}")))?;

    // Content + resources CẤP TRANG (đủ để tìm Do form mức 1).
    let page_content_bytes = doc.get_page_content(page_id).map_err(le)?;
    let page_res_dict: Option<Dictionary> = {
        let (res, ids) = doc.get_page_resources(page_id).map_err(le)?;
        match res {
            Some(d) => Some(d.clone()),
            None => ids
                .first()
                .and_then(|id| doc.get_dictionary(*id).ok())
                .cloned(),
        }
    };

    // Gom job theo form trong cùng: resolve chain → stream id (kèm kiểm đếm).
    let mut per_stream: HashMap<ObjectId, (usize, Vec<usize>)> = HashMap::new();
    for job in jobs {
        let mut content = Content::decode(&page_content_bytes).map_err(le)?;
        let mut res = page_res_dict.clone();
        let mut stream_id: Option<ObjectId> = None;
        for (level, &(ordinal, expected_forms)) in job.chain.iter().enumerate() {
            let forms = form_dos_in_order(&doc, &content, res.as_ref());
            if forms.len() != expected_forms {
                return Err(le(format!(
                    "mức {level}: thấy {} Do-form trong stream nhưng PDFium thấy {} form — cấu trúc không khớp, từ chối phẫu thuật",
                    forms.len(),
                    expected_forms
                )));
            }
            let id = *forms.get(ordinal).ok_or_else(|| {
                le(format!("mức {level}: không có form thứ {ordinal}"))
            })?;
            let stream = doc
                .get_object(id)
                .map_err(le)?
                .as_stream()
                .map_err(le)?;
            let bytes = stream.decompressed_content().map_err(le)?;
            content = Content::decode(&bytes).map_err(le)?;
            res = stream_resources(&doc, id, res.as_ref()).cloned();
            stream_id = Some(id);
        }
        let id = stream_id.ok_or_else(|| le("chain rỗng"))?;
        let entry = per_stream.entry(id).or_insert((job.expected_texts, Vec::new()));
        if entry.0 != job.expected_texts {
            return Err(le("job cùng form nhưng expected_texts khác nhau"));
        }
        entry.1.extend_from_slice(&job.delete_ordinals);
    }

    // Xoá show-op trong từng stream. Giữ hiệu ứng phụ về VỊ TRÍ của '/" :
    // ' = T* + Tj → thay bằng T*; " = aw ac T* Tj → thay bằng Tw/Tc/T*.
    for (stream_id, (expected_texts, ordinals)) in per_stream {
        let bytes = {
            let stream = doc
                .get_object(stream_id)
                .map_err(le)?
                .as_stream()
                .map_err(le)?;
            stream.decompressed_content().map_err(le)?
        };
        let content = Content::decode(&bytes).map_err(le)?;
        let total_shows = content
            .operations
            .iter()
            .filter(|o| is_text_show(&o.operator))
            .count();
        if total_shows != expected_texts {
            return Err(le(format!(
                "form có {total_shows} show-op nhưng PDFium thấy {expected_texts} text object — cấu trúc không khớp, từ chối phẫu thuật"
            )));
        }
        let del: std::collections::HashSet<usize> = ordinals.into_iter().collect();
        let mut new_ops: Vec<Operation> = Vec::with_capacity(content.operations.len());
        let mut show_i = 0usize;
        for op in content.operations.into_iter() {
            if !is_text_show(&op.operator) {
                new_ops.push(op);
                continue;
            }
            let doomed = del.contains(&show_i);
            show_i += 1;
            if !doomed {
                new_ops.push(op);
                continue;
            }
            match op.operator.as_str() {
                "Tj" | "TJ" => {} // bỏ hẳn — không có hiệu ứng phụ vị trí
                "'" => new_ops.push(Operation::new("T*", vec![])),
                "\"" => {
                    let mut operands = op.operands.into_iter();
                    if let (Some(aw), Some(ac)) = (operands.next(), operands.next()) {
                        new_ops.push(Operation::new("Tw", vec![aw]));
                        new_ops.push(Operation::new("Tc", vec![ac]));
                    }
                    new_ops.push(Operation::new("T*", vec![]));
                }
                _ => unreachable!(),
            }
        }
        let encoded = Content { operations: new_ops }.encode().map_err(le)?;
        let stream = doc
            .get_object_mut(stream_id)
            .map_err(le)?
            .as_stream_mut()
            .map_err(le)?;
        stream.dict.remove(b"Filter");
        stream.dict.remove(b"DecodeParms");
        stream.set_content(encoded);
    }

    doc.save(output).map_err(le)?;
    Ok(())
}
