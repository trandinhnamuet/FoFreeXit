//! Làm sạch tài liệu (Foxit: Protect › Sanitize Document / Remove Hidden
//! Information). `examine_document` đếm những gì tìm thấy theo từng hạng mục
//! (như bảng "Remove Hidden Information"), `sanitize_document` xoá THẬT các
//! hạng mục được chọn rồi dọn mọi object không còn được tham chiếu (nội dung
//! đã gỡ không còn nằm trong file) và ghi file mới (lopdf ghi lại toàn bộ →
//! lịch sử lưu tăng dần cũng biến mất).
//!
//! Hạng mục: metadata (Info + XMP + /Metadata từng object), chú thích &
//! markup, trường form (làm phẳng hoặc xoá), tệp đính kèm, JavaScript & hành
//! động nguy hiểm, liên kết, lớp ẩn (nội dung OC đang tắt), dấu trang, chữ ẩn
//! (Tr 3), ảnh thu nhỏ trang, dữ liệu riêng của ứng dụng (/PieceInfo), object
//! không tham chiếu + phiên bản lưu cũ.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::pdfobj::{self, deref, dict_get, dict_of, name_of, obj_dict, obj_dict_mut};
use crate::EngineError;

/// Cách xử lý trường form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormAction {
    #[default]
    Keep,
    /// "In" giao diện hiện tại của field vào nội dung trang rồi bỏ field.
    Flatten,
    Remove,
}

/// Hạng mục cần làm sạch.
#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
    pub metadata: bool,
    pub comments: bool,
    pub forms: FormAction,
    pub attachments: bool,
    pub javascript: bool,
    pub links: bool,
    pub hidden_layers: bool,
    pub bookmarks: bool,
    pub hidden_text: bool,
    pub thumbnails: bool,
    pub private_data: bool,
}

impl SanitizeOptions {
    /// Mặc định kiểu Foxit "Sanitize Document": mọi hạng mục trừ chữ ẩn
    /// (tránh mất lớp chữ OCR) — form được làm phẳng.
    pub fn all() -> Self {
        SanitizeOptions {
            metadata: true,
            comments: true,
            forms: FormAction::Flatten,
            attachments: true,
            javascript: true,
            links: true,
            hidden_layers: true,
            bookmarks: true,
            hidden_text: false,
            thumbnails: true,
            private_data: true,
        }
    }
}

/// Số lượng tìm thấy (examine) / đã xoá (sanitize) theo hạng mục.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SanitizeReport {
    pub metadata: usize,
    pub comments: usize,
    pub form_fields: usize,
    pub attachments: usize,
    pub javascript: usize,
    pub links: usize,
    pub hidden_layers: usize,
    pub bookmarks: usize,
    pub hidden_text: usize,
    pub thumbnails: usize,
    pub private_data: usize,
    /// Object không còn được tham chiếu (kể cả phần vừa gỡ).
    pub unreferenced: usize,
    /// Số lần lưu tăng dần (incremental update) trong file gốc.
    pub previous_versions: usize,
}

/// Loại action bị coi là "JavaScript & hành động" (chạy mã / mở chương trình /
/// gửi-nhận dữ liệu ra ngoài).
const RISKY_ACTIONS: &[&[u8]] = &[
    b"JavaScript",
    b"Launch",
    b"SubmitForm",
    b"ImportData",
    b"Rendition",
    b"GoToE",
    b"RichMediaExecute",
    b"Sound",
    b"Movie",
];

/// Annotation KHÔNG phải "bình luận" (giữ khi xoá comment).
const NON_COMMENT: &[&[u8]] = &[b"Widget", b"Link", b"Watermark", b"PrinterMark", b"TrapNet"];

fn subtype(d: &Dictionary) -> Vec<u8> {
    d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()).map(|n| n.to_vec()).unwrap_or_default()
}

// ---------------------------------------------------------------- examine

/// Đếm những gì có trong tài liệu (không sửa file).
pub fn examine_document(input: &Path) -> Result<SanitizeReport, EngineError> {
    let raw = std::fs::read(input)?;
    let mut doc = pdfobj::load(input)?;
    let mut r = run(&mut doc, &SanitizeOptions::default(), true)?;
    r.previous_versions = count_incremental_updates(&raw);
    let reachable = doc.traverse_objects(|_| {}).into_iter().collect::<BTreeSet<_>>();
    r.unreferenced = doc.objects.keys().filter(|id| !reachable.contains(id)).count();
    Ok(r)
}

/// Xoá các hạng mục được chọn, ghi `output`. Trả số đã xoá theo hạng mục.
pub fn sanitize_document(input: &Path, output: &Path, opts: &SanitizeOptions) -> Result<SanitizeReport, EngineError> {
    let raw = std::fs::read(input)?;
    let mut doc = pdfobj::load(input)?;
    let mut r = run(&mut doc, opts, false)?;
    r.previous_versions = count_incremental_updates(&raw);
    r.unreferenced = doc.prune_objects().len();
    // Bỏ phiên bản PDF 2.0 AF… đã xử lý; lưu nén lại các stream mới.
    doc.compress();
    pdfobj::save(&mut doc, output)?;
    Ok(r)
}

/// Số "%%EOF" thừa = số lần lưu tăng dần sau bản đầu.
fn count_incremental_updates(raw: &[u8]) -> usize {
    let n = raw.windows(5).filter(|w| *w == b"%%EOF").count();
    n.saturating_sub(1)
}

/// Thực thi (hoặc chỉ đếm khi `count_only`) mọi hạng mục.
fn run(doc: &mut Document, o: &SanitizeOptions, count_only: bool) -> Result<SanitizeReport, EngineError> {
    let mut r = SanitizeReport::default();
    let apply = |on: bool| on && !count_only;
    let mut removed: BTreeSet<ObjectId> = BTreeSet::new();

    // Thứ tự quan trọng: làm phẳng form TRƯỚC khi xoá lớp ẩn/chữ ẩn (nội dung
    // mới chèn cũng được lọc), đính kèm/comment trước khi quét tham chiếu.
    r.form_fields = forms(doc, if count_only { FormAction::Keep } else { o.forms }, &mut removed);
    r.comments = annots_where(doc, apply(o.comments), &mut removed, |d| !NON_COMMENT.contains(&subtype(d).as_slice()));
    r.links = annots_where(doc, apply(o.links), &mut removed, |d| subtype(d) == b"Link");
    r.attachments = attachments(doc, apply(o.attachments), &mut removed);
    r.javascript = javascript(doc, apply(o.javascript));
    r.hidden_layers = hidden_layers(doc, apply(o.hidden_layers), &mut removed);
    r.hidden_text = hidden_text(doc, apply(o.hidden_text));
    r.bookmarks = bookmarks(doc, apply(o.bookmarks));
    r.thumbnails = strip_key_everywhere(doc, b"Thumb", apply(o.thumbnails), |d| d.has_type(b"Page"));
    r.private_data = strip_key_everywhere(doc, b"PieceInfo", apply(o.private_data), |_| true);
    r.metadata = metadata(doc, apply(o.metadata));
    if !count_only && !removed.is_empty() {
        scrub_references(doc, &removed);
    }
    Ok(r)
}

// ---------------------------------------------------------------- metadata

fn metadata(doc: &mut Document, apply: bool) -> usize {
    let mut n = 0;
    if let Some(info) = pdfobj::info_dict(doc) {
        if !info.is_empty() {
            n += 1;
        }
    }
    // /Metadata ở catalog + mọi object khác (trang, ảnh, font…).
    n += doc
        .objects
        .values()
        .filter(|o| obj_dict(o).map(|d| d.has(b"Metadata")).unwrap_or(false))
        .count();
    if apply {
        doc.trailer.remove(b"Info");
        for obj in doc.objects.values_mut() {
            if let Some(d) = obj_dict_mut(obj) {
                d.remove(b"Metadata");
            }
        }
    }
    n
}

// ---------------------------------------------------------------- annots

/// Xoá (hoặc đếm) annotation thoả `pred` trên mọi trang. Popup đi theo cha.
fn annots_where(
    doc: &mut Document,
    apply: bool,
    removed: &mut BTreeSet<ObjectId>,
    pred: impl Fn(&Dictionary) -> bool,
) -> usize {
    let mut count = 0;
    for pid in pdfobj::page_ids(doc) {
        let raw = pdfobj::page_annots_raw(doc, pid);
        if raw.is_empty() {
            continue;
        }
        let mut keep = Vec::with_capacity(raw.len());
        let mut changed = false;
        for a in raw {
            let d = obj_dict(deref(doc, &a)).cloned();
            let hit = d.as_ref().map(|d| pred(d)).unwrap_or(false);
            let is_popup = d.as_ref().map(|d| subtype(d) == b"Popup").unwrap_or(false);
            if hit && !is_popup {
                count += 1;
            }
            if hit && apply {
                changed = true;
                if let Object::Reference(id) = a {
                    removed.insert(id);
                    // Popup gắn kèm cũng đi.
                    if let Some(Object::Reference(pp)) = d.as_ref().and_then(|d| d.get(b"Popup").ok()) {
                        removed.insert(*pp);
                    }
                }
                continue;
            }
            keep.push(a);
        }
        if changed {
            // Popup mồ côi (cha đã xoá).
            let keep: Vec<Object> = keep
                .into_iter()
                .filter(|a| !matches!(a, Object::Reference(id) if removed.contains(id)))
                .collect();
            pdfobj::set_page_annots(doc, pid, keep);
        }
    }
    count
}

// ---------------------------------------------------------------- forms

fn forms(doc: &mut Document, action: FormAction, removed: &mut BTreeSet<ObjectId>) -> usize {
    // Đếm widget (mỗi widget = một ô nhập hiển thị).
    let mut n = 0;
    for pid in pdfobj::page_ids(doc) {
        n += pdfobj::page_annots(doc, pid).iter().filter(|(_, d)| subtype(d) == b"Widget").count();
    }
    let has_acro = doc.catalog().map(|c| c.has(b"AcroForm")).unwrap_or(false);
    if n == 0 && has_acro {
        n = 1; // AcroForm rỗng/XFA vẫn là dữ liệu form
    }
    if action == FormAction::Keep {
        return n;
    }
    if action == FormAction::Flatten {
        flatten_widgets(doc);
    }
    annots_where(doc, true, removed, |d| subtype(d) == b"Widget");
    if let Ok(cat) = doc.catalog_mut() {
        cat.remove(b"AcroForm");
    }
    n
}

/// Làm phẳng widget ở tầng PDF object: vẽ appearance /N (theo /AS) vào nội
/// dung trang dưới dạng Form XObject đặt khớp /Rect (thuật toán chuẩn như
/// Acrobat) — bỏ qua widget ẩn/NoView.
fn flatten_widgets(doc: &mut Document) {
    let mut seq = 0u32;
    for pid in pdfobj::page_ids(doc) {
        let mut draws: Vec<(ObjectId, [f32; 4])> = Vec::new();
        for (_, d) in pdfobj::page_annots(doc, pid) {
            if subtype(&d) != b"Widget" {
                continue;
            }
            let flags = d.get(b"F").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
            if flags & (2 | 32) != 0 {
                continue;
            }
            let Some(rect) = pdfobj::rect_of(doc, &d, b"Rect") else { continue };
            let Some(ap) = dict_of(doc, &d, b"AP") else { continue };
            let Some(nobj) = ap.get(b"N").ok() else { continue };
            let stream_id = match deref(doc, nobj) {
                Object::Stream(_) => nobj.as_reference().ok(),
                Object::Dictionary(states) => {
                    let st = name_of(doc, &d, b"AS");
                    st.and_then(|s| states.get(s.as_bytes()).ok()).and_then(|o| o.as_reference().ok())
                }
                _ => None,
            };
            if let Some(sid) = stream_id {
                draws.push((sid, rect));
            }
        }
        if draws.is_empty() {
            continue;
        }
        let mut content = Vec::new();
        let mut xobjs: Vec<(String, ObjectId)> = Vec::new();
        for (sid, rect) in draws {
            let Ok(Object::Stream(s)) = doc.get_object_mut(sid) else { continue };
            s.dict.set("Type", Object::Name(b"XObject".to_vec()));
            s.dict.set("Subtype", Object::Name(b"Form".to_vec()));
            let bbox = rect_arr(s.dict.get(b"BBox").ok()).unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let m = matrix_arr(s.dict.get(b"Matrix").ok());
            // BBox sau Matrix → hộp trục.
            let pts = [(bbox[0], bbox[1]), (bbox[2], bbox[1]), (bbox[0], bbox[3]), (bbox[2], bbox[3])]
                .map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
            let x0 = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
            let x1 = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
            let y0 = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
            let y1 = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
            if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                continue;
            }
            let sx = (rect[2] - rect[0]) / (x1 - x0);
            let sy = (rect[3] - rect[1]) / (y1 - y0);
            let tx = rect[0] - x0 * sx;
            let ty = rect[1] - y0 * sy;
            seq += 1;
            let name = format!("FFFlat{seq}");
            content.extend_from_slice(format!("q {sx:.6} 0 0 {sy:.6} {tx:.4} {ty:.4} cm /{name} Do Q\n").as_bytes());
            xobjs.push((name, sid));
        }
        if xobjs.is_empty() {
            continue;
        }
        add_xobjects(doc, pid, &xobjs);
        append_content_isolated(doc, pid, content);
    }
}

fn rect_arr(o: Option<&Object>) -> Option<[f32; 4]> {
    let a = o?.as_array().ok()?;
    let v: Vec<f32> = a.iter().filter_map(pdfobj::num).collect();
    (v.len() >= 4).then(|| [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

fn matrix_arr(o: Option<&Object>) -> [f32; 6] {
    o.and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(pdfobj::num).collect::<Vec<_>>())
        .filter(|v| v.len() == 6)
        .map(|v| [v[0], v[1], v[2], v[3], v[4], v[5]])
        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// Bảo đảm trang có /Resources riêng (copy từ tổ tiên nếu kế thừa), trả
/// (object id chứa dict Resources hoặc None nếu dict nằm trực tiếp trong trang).
fn own_resources(doc: &mut Document, page_id: ObjectId) -> Option<ObjectId> {
    let page = doc.get_dictionary(page_id).ok()?.clone();
    match page.get(b"Resources") {
        Ok(Object::Reference(id)) => Some(*id),
        Ok(Object::Dictionary(_)) => None,
        _ => {
            // Kế thừa từ /Parent.
            let mut cur = page.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
            let mut inherited = Dictionary::new();
            for _ in 0..32 {
                let Some(pid) = cur else { break };
                let Ok(p) = doc.get_dictionary(pid) else { break };
                if let Some(r) = dict_of(doc, p, b"Resources") {
                    inherited = r.clone();
                    break;
                }
                cur = p.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
            }
            if let Ok(p) = doc.get_dictionary_mut(page_id) {
                p.set("Resources", Object::Dictionary(inherited));
            }
            None
        }
    }
}

fn add_xobjects(doc: &mut Document, page_id: ObjectId, xobjs: &[(String, ObjectId)]) {
    let res_id = own_resources(doc, page_id);
    // Lấy /XObject hiện tại (có thể là tham chiếu).
    let res = match res_id {
        Some(id) => doc.get_dictionary(id).ok().cloned(),
        None => doc.get_dictionary(page_id).ok().and_then(|p| dict_of(doc, p, b"Resources").cloned()),
    }
    .unwrap_or_default();
    let xo_ref = res.get(b"XObject").ok().and_then(|o| o.as_reference().ok());
    let mut xo = dict_of(doc, &res, b"XObject").cloned().unwrap_or_default();
    for (n, id) in xobjs {
        xo.set(n.as_bytes().to_vec(), Object::Reference(*id));
    }
    if let Some(xid) = xo_ref {
        if let Ok(d) = doc.get_dictionary_mut(xid) {
            *d = xo;
        }
        return;
    }
    let target: Option<&mut Dictionary> = match res_id {
        Some(id) => doc.get_dictionary_mut(id).ok(),
        None => doc
            .get_dictionary_mut(page_id)
            .ok()
            .and_then(|p| p.get_mut(b"Resources").ok())
            .and_then(|o| o.as_dict_mut().ok()),
    };
    if let Some(r) = target {
        r.set("XObject", Object::Dictionary(xo));
    }
}

/// Nối nội dung mới vào trang, bọc nội dung cũ trong q/Q để CTM cũ không ảnh hưởng.
fn append_content_isolated(doc: &mut Document, page_id: ObjectId, content: Vec<u8>) {
    let old: Vec<Object> = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Contents").ok())
        .map(|o| match deref(doc, o) {
            Object::Array(a) => a.clone(),
            _ => vec![o.clone()],
        })
        .unwrap_or_default();
    let q = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let qq = doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    let new = doc.add_object(Stream::new(Dictionary::new(), content));
    let mut arr = vec![Object::Reference(q)];
    arr.extend(old);
    arr.push(Object::Reference(qq));
    arr.push(Object::Reference(new));
    if let Ok(p) = doc.get_dictionary_mut(page_id) {
        p.set("Contents", Object::Array(arr));
    }
}

// ---------------------------------------------------------------- attachments

fn attachments(doc: &mut Document, apply: bool, removed: &mut BTreeSet<ObjectId>) -> usize {
    let mut n = 0;
    if let Ok(cat) = doc.catalog() {
        if let Some(names) = dict_of(doc, cat, b"Names") {
            if let Some(ef) = dict_of(doc, names, b"EmbeddedFiles") {
                n += pdfobj::name_tree_entries(doc, ef).len();
            }
        }
    }
    let af = doc.objects.values().filter(|o| obj_dict(o).map(|d| d.has(b"AF")).unwrap_or(false)).count();
    n += af;
    n += annots_where(doc, apply, removed, |d| subtype(d) == b"FileAttachment");
    if apply {
        if let Some(rid) = pdfobj::root_id(doc) {
            let names_ref = doc
                .get_dictionary(rid)
                .ok()
                .and_then(|c| c.get(b"Names").ok())
                .and_then(|o| o.as_reference().ok());
            let target = match names_ref {
                Some(id) => doc.get_dictionary_mut(id).ok(),
                None => doc
                    .get_dictionary_mut(rid)
                    .ok()
                    .and_then(|c| c.get_mut(b"Names").ok())
                    .and_then(|o| o.as_dict_mut().ok()),
            };
            if let Some(names) = target {
                names.remove(b"EmbeddedFiles");
            }
            if let Ok(c) = doc.get_dictionary_mut(rid) {
                c.remove(b"Collection");
                if c.get(b"PageMode").and_then(|o| o.as_name()).ok() == Some(b"UseAttachments") {
                    c.remove(b"PageMode");
                }
            }
        }
        for obj in doc.objects.values_mut() {
            if let Some(d) = obj_dict_mut(obj) {
                d.remove(b"AF");
            }
        }
    }
    n
}

// ---------------------------------------------------------------- javascript

fn is_risky_action(doc: &Document, o: &Object) -> bool {
    let Some(d) = obj_dict(deref(doc, o)) else { return false };
    let s = d.get(b"S").ok().and_then(|x| x.as_name().ok()).unwrap_or(b"");
    RISKY_ACTIONS.contains(&s) || d.get(b"Next").ok().map(|nx| match deref(doc, nx) {
        Object::Array(a) => a.iter().any(|x| is_risky_action(doc, x)),
        other => is_risky_action(doc, other),
    }).unwrap_or(false)
}

fn javascript(doc: &mut Document, apply: bool) -> usize {
    let mut n = 0;
    // 1) Name tree /JavaScript của tài liệu.
    let rid = pdfobj::root_id(doc);
    if let Ok(cat) = doc.catalog() {
        if let Some(names) = dict_of(doc, cat, b"Names") {
            if let Some(js) = dict_of(doc, names, b"JavaScript") {
                n += pdfobj::name_tree_entries(doc, js).len().max(1);
            }
        }
    }
    // 2) Mọi dict: /AA (hành động tự động), /A + /OpenAction nguy hiểm, XFA.
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    let mut edits: Vec<(ObjectId, Vec<Vec<u8>>)> = Vec::new();
    for id in &ids {
        let Some(d) = doc.objects.get(id).and_then(obj_dict) else { continue };
        let mut keys = Vec::new();
        if d.has(b"AA") {
            keys.push(b"AA".to_vec());
        }
        for k in [&b"A"[..], b"OpenAction"] {
            if let Ok(a) = d.get(k) {
                if is_risky_action(doc, a) {
                    keys.push(k.to_vec());
                }
            }
        }
        if d.has(b"XFA") {
            keys.push(b"XFA".to_vec());
        }
        if !keys.is_empty() {
            n += keys.len();
            edits.push((*id, keys));
        }
    }
    // Annotation dict trực tiếp (không phải object riêng) trong /Annots.
    let mut inline_hits = 0;
    for obj in doc.objects.values() {
        if let Some(Object::Array(annots)) = obj_dict(obj).and_then(|d| d.get(b"Annots").ok()) {
            for a in annots {
                if let Object::Dictionary(ad) = a {
                    if ad.has(b"AA") || ad.get(b"A").map(|x| is_risky_action(doc, x)).unwrap_or(false) {
                        inline_hits += 1;
                    }
                }
            }
        }
    }
    n += inline_hits;
    if !apply {
        return n;
    }
    for (id, keys) in edits {
        if let Some(d) = doc.objects.get_mut(&id).and_then(obj_dict_mut) {
            for k in keys {
                d.remove(&k);
            }
        }
    }
    // Inline annots.
    let risky_inline: Vec<(ObjectId, usize)> = doc
        .objects
        .iter()
        .flat_map(|(id, obj)| {
            let arr = obj_dict(obj).and_then(|d| d.get(b"Annots").ok()).and_then(|o| o.as_array().ok());
            arr.map(|a| {
                a.iter()
                    .enumerate()
                    .filter(|(_, x)| matches!(x, Object::Dictionary(ad) if ad.has(b"AA") || ad.get(b"A").map(|y| is_risky_action(doc, y)).unwrap_or(false)))
                    .map(|(i, _)| (*id, i))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
        .collect();
    for (id, i) in risky_inline {
        if let Some(Object::Array(a)) = doc.objects.get_mut(&id).and_then(obj_dict_mut).and_then(|d| d.get_mut(b"Annots").ok()) {
            if let Some(Object::Dictionary(ad)) = a.get_mut(i) {
                ad.remove(b"AA");
                ad.remove(b"A");
            }
        }
    }
    // Gỡ /JavaScript khỏi /Names.
    if let Some(rid) = rid {
        let names_ref = doc.get_dictionary(rid).ok().and_then(|c| c.get(b"Names").ok()).and_then(|o| o.as_reference().ok());
        let target = match names_ref {
            Some(id) => doc.get_dictionary_mut(id).ok(),
            None => doc.get_dictionary_mut(rid).ok().and_then(|c| c.get_mut(b"Names").ok()).and_then(|o| o.as_dict_mut().ok()),
        };
        if let Some(names) = target {
            names.remove(b"JavaScript");
        }
    }
    n
}

// ---------------------------------------------------------------- bookmarks

fn bookmarks(doc: &mut Document, apply: bool) -> usize {
    let mut n = 0;
    if let Ok(cat) = doc.catalog() {
        if let Some(root) = dict_of(doc, cat, b"Outlines") {
            n = count_outline(doc, root, 0, &mut BTreeSet::new());
            if n == 0 {
                n = 1;
            }
        }
    }
    if apply {
        if let Ok(c) = doc.catalog_mut() {
            c.remove(b"Outlines");
            if c.get(b"PageMode").and_then(|o| o.as_name()).ok() == Some(b"UseOutlines") {
                c.remove(b"PageMode");
            }
        }
    }
    n
}

pub(crate) fn count_outline(doc: &Document, node: &Dictionary, depth: u32, seen: &mut BTreeSet<ObjectId>) -> usize {
    if depth > 64 {
        return 0;
    }
    let mut n = 0;
    let mut cur = node.get(b"First").ok().and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        if !seen.insert(id) {
            break;
        }
        let Ok(d) = doc.get_dictionary(id) else { break };
        n += 1 + count_outline(doc, d, depth + 1, seen);
        cur = d.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
    n
}

// ---------------------------------------------------------------- generic key strip

fn strip_key_everywhere(doc: &mut Document, key: &[u8], apply: bool, pred: impl Fn(&Dictionary) -> bool) -> usize {
    let n = doc.objects.values().filter(|o| obj_dict(o).map(|d| d.has(key) && pred(d)).unwrap_or(false)).count();
    if apply {
        for obj in doc.objects.values_mut() {
            if let Some(d) = obj_dict_mut(obj) {
                if d.has(key) && pred(d) {
                    d.remove(key);
                }
            }
        }
    }
    n
}

// ---------------------------------------------------------------- content filtering

/// Trạng thái hiển thị của các OCG theo cấu hình mặc định /D.
struct OcState {
    off: BTreeSet<ObjectId>,
}

impl OcState {
    fn load(doc: &Document) -> Option<OcState> {
        let cat = doc.catalog().ok()?;
        let ocp = dict_of(doc, cat, b"OCProperties")?;
        let all: Vec<ObjectId> = match dict_get(doc, ocp, b"OCGs") {
            Some(Object::Array(a)) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => Vec::new(),
        };
        let d = dict_of(doc, ocp, b"D");
        let ids = |k: &[u8]| -> BTreeSet<ObjectId> {
            match d.and_then(|d| dict_get(doc, d, k)) {
                Some(Object::Array(a)) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
                _ => BTreeSet::new(),
            }
        };
        let base_off = d.and_then(|d| name_of(doc, d, b"BaseState")).as_deref() == Some("OFF");
        let on = ids(b"ON");
        let mut off = ids(b"OFF");
        if base_off {
            off.extend(all.iter().copied().filter(|id| !on.contains(id)));
        }
        Some(OcState { off })
    }

    /// /OC (OCG hoặc OCMD) có đang ẨN không.
    fn hidden(&self, doc: &Document, oc: &Object) -> bool {
        let id = oc.as_reference().ok();
        let Some(d) = obj_dict(deref(doc, oc)) else { return false };
        if d.has_type(b"OCMD") {
            let groups: Vec<ObjectId> = match dict_get(doc, d, b"OCGs") {
                Some(Object::Array(a)) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
                Some(_) => d.get(b"OCGs").ok().and_then(|o| o.as_reference().ok()).into_iter().collect(),
                None => Vec::new(),
            };
            if groups.is_empty() {
                return false;
            }
            let on = |g: &ObjectId| !self.off.contains(g);
            let p = name_of(doc, d, b"P").unwrap_or_else(|| "AnyOn".into());
            let visible = match p.as_str() {
                "AllOn" => groups.iter().all(on),
                "AnyOff" => groups.iter().any(|g| !on(g)),
                "AllOff" => groups.iter().all(|g| !on(g)),
                _ => groups.iter().any(on),
            };
            return !visible;
        }
        id.map(|i| self.off.contains(&i)).unwrap_or(false)
    }
}

/// Toán tử vẽ (bị bỏ trong vùng ẩn). Toán tử trạng thái (q/Q/cm/gs/màu/Tf…) GIỮ
/// vì theo chuẩn chúng vẫn có hiệu lực kể cả khi nội dung OC bị ẩn.
fn is_paint_op(op: &str) -> bool {
    matches!(
        op,
        "Tj" | "TJ" | "'" | "\"" | "Do" | "sh" | "BI" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
    )
}

/// Thay toán tử vẽ bằng phần tác dụng phụ của nó (xuống dòng / kết thúc path).
fn neutralize(op: &Operation) -> Vec<Operation> {
    match op.operator.as_str() {
        "'" => vec![Operation::new("T*", vec![])],
        "\"" if op.operands.len() >= 2 => vec![
            Operation::new("Tw", vec![op.operands[0].clone()]),
            Operation::new("Tc", vec![op.operands[1].clone()]),
            Operation::new("T*", vec![]),
        ],
        "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => vec![Operation::new("n", vec![])],
        _ => vec![],
    }
}

struct Filter<'a> {
    oc: Option<&'a OcState>,
    strip_hidden_text: bool,
    count_oc: usize,
    count_tr3: usize,
}

/// Lọc 1 dãy toán tử: bỏ nội dung trong vùng OC ẩn (và/hoặc chữ Tr 3). Trả
/// None nếu không có gì thay đổi.
fn filter_ops(doc: &Document, res: Option<&Dictionary>, ops: &[Operation], f: &mut Filter) -> Option<Vec<Operation>> {
    let props = res.and_then(|r| dict_of(doc, r, b"Properties"));
    let xobjs = res.and_then(|r| dict_of(doc, r, b"XObject"));
    let mut out = Vec::with_capacity(ops.len());
    let mut changed = false;
    // Ngăn xếp marked-content: true = vùng ẩn.
    let mut mc: Vec<bool> = Vec::new();
    let mut tr_stack: Vec<i64> = Vec::new();
    let mut tr: i64 = 0;
    for op in ops {
        let hidden_now = mc.iter().any(|&h| h);
        match op.operator.as_str() {
            "BDC" if f.oc.is_some() => {
                let is_oc = op.operands.first().and_then(|o| o.as_name().ok()) == Some(b"OC");
                if is_oc {
                    let target = match op.operands.get(1) {
                        Some(Object::Name(n)) => props.and_then(|p| p.get(n).ok()).cloned(),
                        Some(o @ Object::Dictionary(_)) => Some(o.clone()),
                        Some(o @ Object::Reference(_)) => Some(o.clone()),
                        _ => None,
                    };
                    let h = target.map(|t| f.oc.unwrap().hidden(doc, &t)).unwrap_or(false);
                    mc.push(h);
                    // Vùng OC hiện → giữ thành marked-content thường (bỏ ràng buộc lớp).
                    out.push(Operation::new("BMC", vec![Object::Name(b"OC".to_vec())]));
                    changed = true;
                    if h && !hidden_now {
                        f.count_oc += 1;
                    }
                    continue;
                }
                mc.push(false);
            }
            "BMC" | "BDC" => mc.push(false),
            "EMC" => {
                mc.pop();
            }
            "q" => tr_stack.push(tr),
            "Q" => tr = tr_stack.pop().unwrap_or(0),
            "Tr" => tr = op.operands.first().and_then(|o| o.as_i64().ok()).unwrap_or(tr),
            _ => {}
        }
        let op_name = op.operator.as_str();
        if hidden_now && is_paint_op(op_name) {
            out.extend(neutralize(op));
            changed = true;
            continue;
        }
        // XObject gắn /OC đang ẩn.
        if op_name == "Do" && f.oc.is_some() {
            let hidden_x = op
                .operands
                .first()
                .and_then(|o| o.as_name().ok())
                .and_then(|n| xobjs.and_then(|x| x.get(n).ok()))
                .and_then(|x| obj_dict(deref(doc, x)))
                .and_then(|xd| xd.get(b"OC").ok())
                .map(|oc| f.oc.unwrap().hidden(doc, oc))
                .unwrap_or(false);
            if hidden_x {
                f.count_oc += 1;
                changed = true;
                continue;
            }
        }
        if f.strip_hidden_text && tr == 3 && matches!(op_name, "Tj" | "TJ" | "'" | "\"") {
            f.count_tr3 += 1;
            out.extend(neutralize(op));
            changed = true;
            continue;
        }
        out.push(op.clone());
    }
    changed.then_some(out)
}

/// Đếm chữ Tr 3 (không sửa) — dùng cho examine.
fn count_tr3(ops: &[Operation]) -> usize {
    let mut n = 0;
    let mut tr = 0i64;
    let mut st: Vec<i64> = Vec::new();
    for op in ops {
        match op.operator.as_str() {
            "q" => st.push(tr),
            "Q" => tr = st.pop().unwrap_or(0),
            "Tr" => tr = op.operands.first().and_then(|o| o.as_i64().ok()).unwrap_or(tr),
            "Tj" | "TJ" | "'" | "\"" if tr == 3 => n += 1,
            _ => {}
        }
    }
    n
}

/// Duyệt mọi "đơn vị nội dung": trang (id trang, resources, bytes) và Form
/// XObject (id stream). Gọi `f` với (target, resources, ops).
enum Target {
    Page(ObjectId),
    Form(ObjectId),
}

fn content_units(doc: &Document) -> Vec<(Target, Option<Dictionary>)> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    for pid in pdfobj::page_ids(doc) {
        let res = page_resources(doc, pid);
        out.push((Target::Page(pid), res.clone()));
        collect_forms(doc, res.as_ref(), &mut seen, &mut out, 0);
    }
    out
}

fn page_resources(doc: &Document, pid: ObjectId) -> Option<Dictionary> {
    let mut cur = Some(pid);
    for _ in 0..32 {
        let id = cur?;
        let d = doc.get_dictionary(id).ok()?;
        if let Some(r) = dict_of(doc, d, b"Resources") {
            return Some(r.clone());
        }
        cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

fn collect_forms(
    doc: &Document,
    res: Option<&Dictionary>,
    seen: &mut BTreeSet<ObjectId>,
    out: &mut Vec<(Target, Option<Dictionary>)>,
    depth: u32,
) {
    if depth > 16 {
        return;
    }
    let Some(xo) = res.and_then(|r| dict_of(doc, r, b"XObject")) else { return };
    for (_, v) in xo.iter() {
        let Ok(id) = v.as_reference() else { continue };
        let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
        if s.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() != Some(b"Form") || !seen.insert(id) {
            continue;
        }
        // Form không có Resources riêng → dùng của trang (kiểu cũ).
        let own = dict_of(doc, &s.dict, b"Resources").cloned().or_else(|| res.cloned());
        out.push((Target::Form(id), own.clone()));
        collect_forms(doc, own.as_ref(), seen, out, depth + 1);
    }
}

fn unit_ops(doc: &Document, t: &Target) -> Option<Vec<Operation>> {
    let bytes = match t {
        Target::Page(pid) => doc.get_page_content(*pid).ok()?,
        Target::Form(id) => match doc.get_object(*id) {
            Ok(Object::Stream(s)) => pdfobj::stream_data(s),
            _ => return None,
        },
    };
    Content::decode(&bytes).ok().map(|c| c.operations)
}

fn write_unit(doc: &mut Document, t: &Target, ops: Vec<Operation>) {
    let Ok(bytes) = (Content { operations: ops }).encode() else { return };
    match t {
        Target::Page(pid) => {
            let id = doc.add_object(Stream::new(Dictionary::new(), bytes));
            if let Ok(p) = doc.get_dictionary_mut(*pid) {
                p.set("Contents", Object::Reference(id));
            }
        }
        Target::Form(id) => {
            if let Ok(Object::Stream(s)) = doc.get_object_mut(*id) {
                s.dict.remove(b"Filter");
                s.dict.remove(b"DecodeParms");
                s.set_plain_content(bytes);
            }
        }
    }
}

fn hidden_layers(doc: &mut Document, apply: bool, removed: &mut BTreeSet<ObjectId>) -> usize {
    let Some(state) = OcState::load(doc) else { return 0 };
    let n_off = state.off.len();
    if !apply {
        return n_off;
    }
    for (t, res) in content_units(doc) {
        let Some(ops) = unit_ops(doc, &t) else { continue };
        let mut f = Filter { oc: Some(&state), strip_hidden_text: false, count_oc: 0, count_tr3: 0 };
        if let Some(new_ops) = filter_ops(doc, res.as_ref(), &ops, &mut f) {
            write_unit(doc, &t, new_ops);
        }
    }
    // Annotation thuộc lớp ẩn → xoá; lớp hiện → bỏ ràng buộc /OC.
    let hidden_annot = |doc: &Document, d: &Dictionary| d.get(b"OC").map(|oc| state.hidden(doc, oc)).unwrap_or(false);
    for pid in pdfobj::page_ids(doc) {
        let raw = pdfobj::page_annots_raw(doc, pid);
        if raw.is_empty() {
            continue;
        }
        let mut keep = Vec::new();
        let mut changed = false;
        for a in raw {
            let hid = obj_dict(deref(doc, &a)).map(|d| hidden_annot(doc, d)).unwrap_or(false);
            if hid {
                changed = true;
                if let Object::Reference(id) = a {
                    removed.insert(id);
                }
            } else {
                keep.push(a);
            }
        }
        if changed {
            pdfobj::set_page_annots(doc, pid, keep);
        }
    }
    for obj in doc.objects.values_mut() {
        if let Some(d) = obj_dict_mut(obj) {
            if !d.has_type(b"OCMD") {
                d.remove(b"OC");
            }
        }
    }
    if let Ok(c) = doc.catalog_mut() {
        c.remove(b"OCProperties");
        if c.get(b"PageMode").and_then(|o| o.as_name()).ok() == Some(b"UseOC") {
            c.remove(b"PageMode");
        }
    }
    n_off
}

fn hidden_text(doc: &mut Document, apply: bool) -> usize {
    let mut total = 0;
    for (t, res) in content_units(doc) {
        let Some(ops) = unit_ops(doc, &t) else { continue };
        if !apply {
            total += count_tr3(&ops);
            continue;
        }
        let mut f = Filter { oc: None, strip_hidden_text: true, count_oc: 0, count_tr3: 0 };
        if let Some(new_ops) = filter_ops(doc, res.as_ref(), &ops, &mut f) {
            write_unit(doc, &t, new_ops);
        }
        total += f.count_tr3;
    }
    total
}

// ---------------------------------------------------------------- references

/// Gỡ mọi tham chiếu tới object đã xoá (StructTree OBJR, /Popup, /IRT,
/// /Parent, /Fields…) để prune dọn được và không để tham chiếu treo.
fn scrub_references(doc: &mut Document, removed: &BTreeSet<ObjectId>) {
    let mut removed = removed.clone();
    // OBJR gián tiếp trỏ tới object đã xoá → coi như đã xoá luôn.
    for (id, obj) in doc.objects.iter() {
        if let Some(d) = obj_dict(obj) {
            if d.has_type(b"OBJR") {
                if let Ok(Object::Reference(t)) = d.get(b"Obj") {
                    if removed.contains(t) {
                        removed.insert(*id);
                    }
                }
            }
        }
    }
    let bad = |o: &Object| match o {
        Object::Reference(id) => removed.contains(id),
        Object::Dictionary(d) => d.has_type(b"OBJR") && matches!(d.get(b"Obj"), Ok(Object::Reference(t)) if removed.contains(t)),
        _ => false,
    };
    fn walk(o: &mut Object, bad: &dyn Fn(&Object) -> bool) {
        match o {
            Object::Array(a) => {
                a.retain(|x| !bad(x));
                for x in a.iter_mut() {
                    walk(x, bad);
                }
            }
            Object::Dictionary(d) => walk_dict(d, bad),
            Object::Stream(s) => walk_dict(&mut s.dict, bad),
            _ => {}
        }
    }
    fn walk_dict(d: &mut Dictionary, bad: &dyn Fn(&Object) -> bool) {
        let keys: Vec<Vec<u8>> = d.iter().filter(|(_, v)| bad(v)).map(|(k, _)| k.clone()).collect();
        for k in keys {
            d.remove(&k);
        }
        for (_, v) in d.iter_mut() {
            walk(v, bad);
        }
    }
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        if removed.contains(&id) {
            continue;
        }
        if let Some(o) = doc.objects.get_mut(&id) {
            walk(o, &bad);
        }
    }
    for id in &removed {
        doc.objects.remove(id);
    }
    let mut tr = std::mem::take(&mut doc.trailer);
    walk_dict(&mut tr, &bad);
    doc.trailer = tr;
}

/// Tiện ích cho test/UI: đếm nhanh annotation theo subtype.
pub fn annotation_subtypes(input: &Path) -> Result<BTreeMap<String, usize>, EngineError> {
    let doc = pdfobj::load(input)?;
    let mut m = BTreeMap::new();
    for pid in pdfobj::page_ids(&doc) {
        for (_, d) in pdfobj::page_annots(&doc, pid) {
            *m.entry(String::from_utf8_lossy(&subtype(&d)).into_owned()).or_insert(0) += 1;
        }
    }
    Ok(m)
}
