// Lệnh Tauri cho bảo mật/làm sạch tài liệu (docsec): Search & Redact, redact
// có giao diện tuỳ chọn, Sanitize, Tệp đính kèm, Thuộc tính tài liệu, Trợ năng.
// Việc nặng (quét toàn tài liệu, ghi file) chạy trong spawn_blocking để UI
// không đơ.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn es<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- Search & Redact

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactSearchDto {
    #[serde(default)]
    terms: Vec<String>,
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    patterns: Vec<String>,
    #[serde(default)]
    match_case: bool,
    #[serde(default)]
    whole_word: bool,
    #[serde(default)]
    pages: Vec<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactHitDto {
    page_index: u16,
    text: String,
    before: String,
    after: String,
    source: String,
    rects: Vec<[f32; 4]>,
}

/// Tìm theo từ/cụm từ, regex, mẫu dựng sẵn trên toàn tài liệu.
#[tauri::command]
pub async fn redact_search(input: String, spec: RedactSearchDto, password: Option<String>) -> Result<Vec<RedactHitDto>, String> {
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let s = ff_engine::RedactSearchSpec {
            terms: spec.terms,
            regex: spec.regex,
            patterns: spec.patterns.iter().filter_map(|p| ff_engine::RedactPattern::from_key(p)).collect(),
            match_case: spec.match_case,
            whole_word: spec.whole_word,
            pages: spec.pages,
        };
        let hits = ff_engine::search_redact(&pdfium, Path::new(&input), &s, password.as_deref()).map_err(es)?;
        Ok(hits
            .into_iter()
            .map(|h| RedactHitDto {
                page_index: h.page_index,
                text: h.text,
                before: h.before,
                after: h.after,
                source: h.source,
                rects: h.rects.iter().map(|r| [r.left, r.bottom, r.right, r.top]).collect(),
            })
            .collect())
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactAreaDto {
    page: u16,
    #[serde(default)]
    rects: Vec<[f32; 4]>,
    /// true = bôi đen nguyên trang.
    #[serde(default)]
    whole: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RedactStyleDto {
    #[serde(default)]
    fill: Option<[u8; 3]>,
    #[serde(default)]
    overlay_text: Option<String>,
    #[serde(default)]
    text_color: Option<[u8; 3]>,
    #[serde(default)]
    font_size: Option<f32>,
    #[serde(default)]
    repeat: bool,
    #[serde(default)]
    align: Option<String>,
}

/// Redact THẬT nhiều trang với giao diện tuỳ chọn (màu tô, chữ phủ, lặp, căn
/// lề) + bôi đen nguyên trang. Áp tuần tự qua file tạm, trả tổng số đã xoá.
#[tauri::command]
pub async fn redact_apply_styled(
    input: String,
    areas: Vec<RedactAreaDto>,
    style: Option<RedactStyleDto>,
    output: String,
    password: Option<String>,
) -> Result<usize, String> {
    blocking(move || {
        if areas.is_empty() {
            return Err("chưa đánh dấu vùng redact nào".into());
        }
        let pdfium = crate::pdfium()?;
        let st = style.unwrap_or_default();
        let d = ff_engine::RedactStyle::default();
        let style = ff_engine::RedactStyle {
            fill: st.fill.unwrap_or(d.fill),
            overlay_text: st.overlay_text.filter(|s| !s.trim().is_empty()),
            text_color: st.text_color.unwrap_or(d.text_color),
            font_size: st.font_size.filter(|s| *s > 0.0),
            repeat: st.repeat,
            align: match st.align.as_deref() {
                Some("left") => ff_engine::RedactAlign::Left,
                Some("right") => ff_engine::RedactAlign::Right,
                _ => ff_engine::RedactAlign::Center,
            },
        };
        // Gộp theo trang (nguyên trang thắng vùng lẻ).
        let mut pages: std::collections::BTreeMap<u16, (bool, Vec<ff_engine::Rect>)> = Default::default();
        for a in &areas {
            let e = pages.entry(a.page).or_default();
            e.0 |= a.whole;
            e.1.extend(a.rects.iter().map(|r| ff_engine::Rect { left: r[0], bottom: r[1], right: r[2], top: r[3] }));
        }
        let mut total = 0usize;
        let mut cur = PathBuf::from(&input);
        let mut temps: Vec<PathBuf> = Vec::new();
        let n = pages.len();
        for (i, (page, (whole, rects))) in pages.into_iter().enumerate() {
            let dst = if i + 1 == n {
                PathBuf::from(&output)
            } else {
                std::env::temp_dir().join(format!("ff_redact_st_{}_{}.pdf", std::process::id(), i))
            };
            // Mật khẩu chỉ cần cho tệp gốc; file tạm trung gian không mã hoá.
            let pw = if i == 0 { password.as_deref() } else { None };
            total += ff_engine::redact_areas_styled(&pdfium, &cur, page, &rects, whole, &style, &dst, pw).map_err(es)?;
            if i > 0 {
                temps.push(cur.clone());
            }
            cur = dst;
        }
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
        Ok(total)
    })
    .await
}

// ---------------------------------------------------------------- Sanitize

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizeReportDto {
    metadata: usize,
    comments: usize,
    form_fields: usize,
    attachments: usize,
    javascript: usize,
    links: usize,
    hidden_layers: usize,
    bookmarks: usize,
    hidden_text: usize,
    thumbnails: usize,
    private_data: usize,
    unreferenced: usize,
    previous_versions: usize,
}

impl From<ff_engine::SanitizeReport> for SanitizeReportDto {
    fn from(r: ff_engine::SanitizeReport) -> Self {
        SanitizeReportDto {
            metadata: r.metadata,
            comments: r.comments,
            form_fields: r.form_fields,
            attachments: r.attachments,
            javascript: r.javascript,
            links: r.links,
            hidden_layers: r.hidden_layers,
            bookmarks: r.bookmarks,
            hidden_text: r.hidden_text,
            thumbnails: r.thumbnails,
            private_data: r.private_data,
            unreferenced: r.unreferenced,
            previous_versions: r.previous_versions,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizeOptionsDto {
    #[serde(default)]
    metadata: bool,
    #[serde(default)]
    comments: bool,
    /// "keep" | "flatten" | "remove"
    #[serde(default)]
    forms: String,
    #[serde(default)]
    attachments: bool,
    #[serde(default)]
    javascript: bool,
    #[serde(default)]
    links: bool,
    #[serde(default)]
    hidden_layers: bool,
    #[serde(default)]
    bookmarks: bool,
    #[serde(default)]
    hidden_text: bool,
    #[serde(default)]
    thumbnails: bool,
    #[serde(default)]
    private_data: bool,
}

/// Đếm những gì "ẩn" trong tài liệu (Examine Document).
#[tauri::command]
pub async fn sanitize_examine(input: String) -> Result<SanitizeReportDto, String> {
    blocking(move || ff_engine::examine_document(Path::new(&input)).map(Into::into).map_err(es)).await
}

/// Xoá các hạng mục đã chọn, ghi `output`.
#[tauri::command]
pub async fn sanitize_apply(input: String, output: String, options: SanitizeOptionsDto) -> Result<SanitizeReportDto, String> {
    blocking(move || {
        let o = ff_engine::SanitizeOptions {
            metadata: options.metadata,
            comments: options.comments,
            forms: match options.forms.as_str() {
                "flatten" => ff_engine::FormAction::Flatten,
                "remove" => ff_engine::FormAction::Remove,
                _ => ff_engine::FormAction::Keep,
            },
            attachments: options.attachments,
            javascript: options.javascript,
            links: options.links,
            hidden_layers: options.hidden_layers,
            bookmarks: options.bookmarks,
            hidden_text: options.hidden_text,
            thumbnails: options.thumbnails,
            private_data: options.private_data,
        };
        ff_engine::sanitize_document(Path::new(&input), Path::new(&output), &o).map(Into::into).map_err(es)
    })
    .await
}

// ---------------------------------------------------------------- Attachments

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    key: String,
    file_name: String,
    description: String,
    size: Option<u64>,
    mod_date: Option<String>,
    creation_date: Option<String>,
    mime: Option<String>,
    page_index: Option<u16>,
    risky: bool,
}

fn att_dto(a: ff_engine::Attachment) -> AttachmentDto {
    AttachmentDto {
        risky: ff_engine::is_risky_file_name(&a.file_name),
        key: a.key,
        file_name: a.file_name,
        description: a.description,
        size: a.size,
        mod_date: a.mod_date,
        creation_date: a.creation_date,
        mime: a.mime,
        page_index: a.page_index,
    }
}

#[tauri::command]
pub async fn attach_list(input: String) -> Result<Vec<AttachmentDto>, String> {
    blocking(move || Ok(ff_engine::list_attachments(Path::new(&input)).map_err(es)?.into_iter().map(att_dto).collect())).await
}

#[tauri::command]
pub async fn attach_extract(input: String, key: String, output: String) -> Result<(), String> {
    blocking(move || ff_engine::extract_attachment(Path::new(&input), &key, Path::new(&output)).map_err(es)).await
}

/// Tên tệp an toàn cho hệ thống tệp (bỏ ký tự cấm / đường dẫn).
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s: String = base.chars().map(|c| if "<>:\"|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "attachment".into() } else { s }
}

/// Mở tệp bằng ứng dụng mặc định của hệ điều hành.
fn open_with_os(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    r.map(|_| ()).map_err(|e| format!("không mở được tệp: {e}"))
}

/// Trích tệp đính kèm ra thư mục tạm rồi mở bằng ứng dụng mặc định. Trả
/// đường dẫn tệp tạm.
#[tauri::command]
pub async fn attach_open(input: String, key: String, file_name: String) -> Result<String, String> {
    blocking(move || {
        let dir = std::env::temp_dir().join("FoFreeXit-attachments").join(format!(
            "{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).map_err(es)?;
        let out = dir.join(safe_file_name(&file_name));
        ff_engine::extract_attachment(Path::new(&input), &key, &out).map_err(es)?;
        open_with_os(&out)?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum AttachmentOpDto {
    #[serde(rename = "add")]
    Add { path: String, name: Option<String>, #[serde(default)] description: String },
    #[serde(rename = "delete")]
    Delete { key: String },
    #[serde(rename = "describe")]
    Describe { key: String, description: String },
}

/// Ghi các thay đổi đính kèm ra tệp mới; trả danh sách mới.
#[tauri::command]
pub async fn attach_apply(input: String, ops: Vec<AttachmentOpDto>, output: String) -> Result<Vec<AttachmentDto>, String> {
    blocking(move || {
        let ops: Vec<ff_engine::AttachmentOp> = ops
            .into_iter()
            .map(|o| match o {
                AttachmentOpDto::Add { path, name, description } => ff_engine::AttachmentOp::Add { path: PathBuf::from(path), name, description },
                AttachmentOpDto::Delete { key } => ff_engine::AttachmentOp::Delete { key },
                AttachmentOpDto::Describe { key, description } => ff_engine::AttachmentOp::SetDescription { key, description },
            })
            .collect();
        // Ghi đè chính tệp đang mở: ghi ra tạm rồi thay.
        let same = Path::new(&input) == Path::new(&output);
        let dst = if same { std::env::temp_dir().join(format!("ff_attach_{}.pdf", std::process::id())) } else { PathBuf::from(&output) };
        let list = ff_engine::apply_attachment_ops(Path::new(&input), &ops, &dst).map_err(es)?;
        if same {
            std::fs::copy(&dst, &output).map_err(es)?;
            let _ = std::fs::remove_file(&dst);
        }
        Ok(list.into_iter().map(att_dto).collect())
    })
    .await
}

/// Chọn nhiều tệp bất kỳ (Thêm đính kèm).
#[tauri::command]
pub fn pick_files(app: tauri::AppHandle) -> Vec<String> {
    app.dialog().file().blocking_pick_files().map(|v| v.into_iter().map(|f| f.to_string()).collect()).unwrap_or_default()
}

/// Lưu một tệp với tên gợi ý (Lưu đính kèm thành…).
#[tauri::command]
pub fn pick_save_file(app: tauri::AppHandle, name: String) -> Option<String> {
    let mut d = app.dialog().file().set_file_name(&name);
    if let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_string()).filter(|e| !e.is_empty() && e.len() <= 8) {
        d = d.add_filter(ext.to_uppercase(), &[ext.as_str()]);
    }
    d.blocking_save_file().map(|f| f.to_string())
}

// ---------------------------------------------------------------- Properties

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InitialViewDto {
    #[serde(default)]
    page_layout: String,
    #[serde(default)]
    navigation: String,
    #[serde(default)]
    full_screen: bool,
    #[serde(default)]
    open_page: u32,
    #[serde(default)]
    magnification: String,
    #[serde(default)]
    fit_window: bool,
    #[serde(default)]
    center_window: bool,
    #[serde(default)]
    display_doc_title: bool,
    #[serde(default)]
    hide_menubar: bool,
    #[serde(default)]
    hide_toolbar: bool,
    #[serde(default)]
    hide_window_ui: bool,
}

impl From<ff_engine::InitialView> for InitialViewDto {
    fn from(v: ff_engine::InitialView) -> Self {
        InitialViewDto {
            page_layout: v.page_layout,
            navigation: v.navigation,
            full_screen: v.full_screen,
            open_page: v.open_page,
            magnification: v.magnification,
            fit_window: v.fit_window,
            center_window: v.center_window,
            display_doc_title: v.display_doc_title,
            hide_menubar: v.hide_menubar,
            hide_toolbar: v.hide_toolbar,
            hide_window_ui: v.hide_window_ui,
        }
    }
}

impl From<InitialViewDto> for ff_engine::InitialView {
    fn from(v: InitialViewDto) -> Self {
        ff_engine::InitialView {
            page_layout: v.page_layout,
            navigation: v.navigation,
            full_screen: v.full_screen,
            open_page: v.open_page,
            magnification: if v.magnification.is_empty() { "default".into() } else { v.magnification },
            fit_window: v.fit_window,
            center_window: v.center_window,
            display_doc_title: v.display_doc_title,
            hide_menubar: v.hide_menubar,
            hide_toolbar: v.hide_toolbar,
            hide_window_ui: v.hide_window_ui,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontDto {
    name: String,
    font_type: String,
    encoding: Option<String>,
    embedded: bool,
    subset: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityDto {
    method: String,
    has_user_password: bool,
    allow_print: bool,
    allow_print_high: bool,
    allow_modify: bool,
    allow_copy: bool,
    allow_annotate: bool,
    allow_fill_forms: bool,
    allow_accessibility: bool,
    allow_assemble: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPropsDto {
    title: String,
    author: String,
    subject: String,
    keywords: String,
    creator: String,
    producer: String,
    creation_date: Option<String>,
    mod_date: Option<String>,
    pdf_version: String,
    page_count: usize,
    page_width: f32,
    page_height: f32,
    file_size: u64,
    tagged: bool,
    has_xmp: bool,
    lang: String,
    security: Option<SecurityDto>,
    fonts: Vec<FontDto>,
    view: InitialViewDto,
}

/// Đọc thuộc tính. `original` = tệp gốc (dung lượng + bảo mật).
#[tauri::command]
pub async fn docprops_read(path: String, original: Option<String>) -> Result<DocPropsDto, String> {
    blocking(move || {
        let pdfium = crate::pdfium().ok();
        let orig = original.map(PathBuf::from);
        let p = ff_engine::read_properties(pdfium.as_ref(), Path::new(&path), orig.as_deref()).map_err(es)?;
        Ok(DocPropsDto {
            title: p.title,
            author: p.author,
            subject: p.subject,
            keywords: p.keywords,
            creator: p.creator,
            producer: p.producer,
            creation_date: p.creation_date,
            mod_date: p.mod_date,
            pdf_version: p.pdf_version,
            page_count: p.page_count,
            page_width: p.page_width,
            page_height: p.page_height,
            file_size: p.file_size,
            tagged: p.tagged,
            has_xmp: p.has_xmp,
            lang: p.lang,
            security: p.security.map(|s| SecurityDto {
                method: s.method,
                has_user_password: s.has_user_password,
                allow_print: s.allow_print,
                allow_print_high: s.allow_print_high,
                allow_modify: s.allow_modify,
                allow_copy: s.allow_copy,
                allow_annotate: s.allow_annotate,
                allow_fill_forms: s.allow_fill_forms,
                allow_accessibility: s.allow_accessibility,
                allow_assemble: s.allow_assemble,
            }),
            fonts: p
                .fonts
                .into_iter()
                .map(|f| FontDto { name: f.name, font_type: f.font_type, encoding: f.encoding, embedded: f.embedded, subset: f.subset })
                .collect(),
            view: p.view.into(),
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPropsUpdateDto {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    lang: Option<String>,
    view: Option<InitialViewDto>,
}

#[tauri::command]
pub async fn docprops_write(input: String, output: String, update: DocPropsUpdateDto) -> Result<(), String> {
    blocking(move || {
        let u = ff_engine::DocPropsUpdate {
            title: update.title,
            author: update.author,
            subject: update.subject,
            keywords: update.keywords,
            lang: update.lang,
            view: update.view.map(Into::into),
        };
        ff_engine::write_properties(Path::new(&input), Path::new(&output), &u).map_err(es)
    })
    .await
}

// ---------------------------------------------------------------- Accessibility

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A11yCheckDto {
    id: String,
    category: String,
    status: String,
    count: usize,
    details: Vec<String>,
    fixable: bool,
}

#[tauri::command]
pub async fn a11y_check(input: String, quick: bool) -> Result<Vec<A11yCheckDto>, String> {
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let r = ff_engine::check_accessibility(&pdfium, Path::new(&input), quick).map_err(es)?;
        Ok(r.into_iter()
            .map(|c| A11yCheckDto {
                id: c.id.into(),
                category: c.category.into(),
                status: c.status.as_str().into(),
                count: c.count,
                details: c.details,
                fixable: c.fixable,
            })
            .collect())
    })
    .await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A11yFixesDto {
    title: Option<String>,
    lang: Option<String>,
    #[serde(default)]
    display_doc_title: bool,
    #[serde(default)]
    tab_order: bool,
    #[serde(default)]
    field_tooltips: bool,
    link_contents: Option<String>,
    #[serde(default)]
    bookmarks_from_headings: bool,
}

#[tauri::command]
pub async fn a11y_fix(input: String, output: String, fixes: A11yFixesDto) -> Result<usize, String> {
    blocking(move || {
        let f = ff_engine::A11yFixes {
            title: fixes.title,
            lang: fixes.lang,
            display_doc_title: fixes.display_doc_title,
            tab_order: fixes.tab_order,
            field_tooltips: fixes.field_tooltips,
            link_contents: fixes.link_contents,
            bookmarks_from_headings: fixes.bookmarks_from_headings,
        };
        ff_engine::fix_accessibility(Path::new(&input), Path::new(&output), &f).map_err(es)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigureDto {
    id: String,
    page_index: Option<u16>,
    rect: Option<[f32; 4]>,
    alt: String,
    actual_text: String,
}

#[tauri::command]
pub async fn a11y_figures(input: String) -> Result<Vec<FigureDto>, String> {
    blocking(move || {
        Ok(ff_engine::list_figures(Path::new(&input))
            .map_err(es)?
            .into_iter()
            .map(|f| FigureDto { id: f.id, page_index: f.page_index, rect: f.rect, alt: f.alt, actual_text: f.actual_text })
            .collect())
    })
    .await
}

#[derive(Deserialize)]
pub struct AltDto {
    id: String,
    alt: String,
}

#[tauri::command]
pub async fn a11y_set_alt(input: String, output: String, alts: Vec<AltDto>) -> Result<usize, String> {
    blocking(move || {
        let v: Vec<(String, String)> = alts.into_iter().map(|a| (a.id, a.alt)).collect();
        ff_engine::set_alt_texts(Path::new(&input), Path::new(&output), &v).map_err(es)
    })
    .await
}
