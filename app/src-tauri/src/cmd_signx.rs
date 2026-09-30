//! Lệnh Tauri cho: Điền & Ký (Fill & Sign), chữ ký số nâng cao (PFX, chữ ký
//! hiển thị, ký vào ô có sẵn, bảng xác thực chi tiết), In (render trang cho
//! hộp thoại In) và Batch / Action Wizard. Việc nặng chạy ở `spawn_blocking`
//! để không đơ giao diện.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;

static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// data URL ảnh (PNG/JPG) → file tạm `ff_signx_*`. Trả đường dẫn.
fn data_url_to_temp(url: &str, tag: &str) -> Result<PathBuf, String> {
    let (head, b64) = url.split_once(',').ok_or("ảnh không hợp lệ (data URL)")?;
    let ext = if head.contains("jpeg") || head.contains("jpg") { "jpg" } else { "png" };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("giải mã ảnh: {e}"))?;
    let p = std::env::temp_dir().join(format!("ff_signx_{tag}_{}_{}.{ext}", std::process::id(), nanos()));
    std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
    Ok(p)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Đọc ảnh (PNG/JPG/BMP…) → data URL PNG, thu nhỏ cạnh dài ≤ `max_side` —
/// dùng cho "Nhập ảnh chữ ký" (UI tự tách nền trắng trên canvas).
#[tauri::command]
pub async fn signx_read_image(path: String, max_side: Option<u32>) -> Result<String, String> {
    blocking(move || {
        let img = image::open(&path).map_err(|e| format!("đọc ảnh: {e}"))?;
        let m = max_side.unwrap_or(1600).clamp(64, 4096);
        let img = if img.width().max(img.height()) > m { img.thumbnail(m, m) } else { img };
        let mut buf = std::io::Cursor::new(Vec::new());
        img.to_rgba8().write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.get_ref())))
    })
    .await
}

// ---------- Digital ID ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdInfoDto {
    common_name: String,
    subject: String,
    issuer: String,
    email: String,
    not_before: String,
    not_after: String,
    self_signed: bool,
    expired: bool,
}

/// Đọc Digital ID (PEM/PFX) — kiểm luôn mật khẩu PFX.
#[tauri::command]
pub async fn signx_id_info(path: String, password: Option<String>) -> Result<IdInfoDto, String> {
    blocking(move || {
        let i = ff_engine::identity_info(Path::new(&path), password.as_deref()).map_err(|e| e.to_string())?;
        Ok(IdInfoDto {
            common_name: i.common_name,
            subject: i.subject,
            issuer: i.issuer,
            email: i.email,
            not_before: i.not_before,
            not_after: i.not_after,
            self_signed: i.self_signed,
            expired: i.expired,
        })
    })
    .await
}

/// Tạo Digital ID tự ký dạng PFX có mật khẩu.
#[tauri::command]
pub async fn signx_create_pfx(
    common_name: String,
    organization: String,
    email: String,
    password: String,
    output: String,
) -> Result<(), String> {
    blocking(move || {
        ff_engine::generate_self_signed_pfx(&common_name, &organization, &email, &password, Path::new(&output))
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn signx_pick_id(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Digital ID (PFX, P12, PEM)", &["pfx", "p12", "pem"])
        .blocking_pick_file()
        .map(|fp| fp.to_string())
}

#[tauri::command]
pub fn signx_pick_save_pfx(app: tauri::AppHandle, name: String) -> Option<String> {
    let n = if name.trim().is_empty() { "digital-id".to_string() } else { name.trim().replace(' ', "_") };
    app.dialog()
        .file()
        .add_filter("Digital ID (PFX)", &["pfx"])
        .set_file_name(format!("{n}.pfx"))
        .blocking_save_file()
        .map(|fp| fp.to_string())
}

// ---------- Ký ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigFieldDto {
    name: String,
    page_index: Option<u16>,
    rect: Option<[f32; 4]>,
    signed: bool,
}

#[tauri::command]
pub fn signx_list_fields(input: String) -> Result<Vec<SigFieldDto>, String> {
    // Tài liệu lopdf không đọc được → coi như không có ô chữ ký.
    let fields = ff_engine::list_signature_fields(Path::new(&input)).unwrap_or_default();
    Ok(fields
        .into_iter()
        .map(|f| SigFieldDto { name: f.name, page_index: f.page_index, rect: f.rect, signed: f.signed })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceDto {
    page: u16,
    rect: [f32; 4],
    image_data_url: Option<String>,
    #[serde(default)]
    big_text: String,
    #[serde(default)]
    lines: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignReqDto {
    id_path: String,
    password: Option<String>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    contact_info: String,
    #[serde(default)]
    signer_name: String,
    #[serde(default)]
    tz_offset_min: i32,
    field_name: Option<String>,
    appearance: Option<AppearanceDto>,
}

/// Ký số `input` → `output` (PFX/PEM, chữ ký ẩn hoặc hiển thị, ô có sẵn).
#[tauri::command]
pub async fn signx_sign(input: String, output: String, req: SignReqDto) -> Result<(), String> {
    blocking(move || {
        if Path::new(&input) == Path::new(&output) {
            return Err("hãy lưu bản đã ký thành tệp mới".into());
        }
        let id = ff_engine::load_identity_file(Path::new(&req.id_path), req.password.as_deref())
            .map_err(|e| e.to_string())?;
        let mut temp_img: Option<PathBuf> = None;
        let appearance = match req.appearance {
            Some(a) => {
                let image_path = match a.image_data_url.as_deref() {
                    Some(u) if !u.is_empty() => {
                        let p = data_url_to_temp(u, "sigimg")?;
                        temp_img = Some(p.clone());
                        Some(p.to_string_lossy().into_owned())
                    }
                    _ => None,
                };
                Some(ff_engine::SigAppearance { page: a.page, rect: a.rect, image_path, big_text: a.big_text, lines: a.lines })
            }
            None => None,
        };
        let r = ff_engine::SignRequest {
            reason: req.reason,
            location: req.location,
            contact_info: req.contact_info,
            signer_name: req.signer_name,
            tz_offset_min: req.tz_offset_min,
            field_name: req.field_name.filter(|s| !s.is_empty()),
            appearance,
        };
        let pdfium = crate::pdfium()?;
        let res = ff_engine::sign_pdf_ex(Some(&pdfium), Path::new(&input), &id, &r, Path::new(&output))
            .map_err(|e| e.to_string());
        if let Some(p) = temp_img {
            let _ = std::fs::remove_file(p);
        }
        res
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigCheckDto {
    revision: usize,
    signer: String,
    name: String,
    reason: String,
    location: String,
    contact_info: String,
    sign_time: String,
    sub_filter: String,
    crypto_valid: bool,
    digest_matches: bool,
    covers_document: bool,
    later_changes_are_signatures: bool,
    intact: bool,
    valid: bool,
    cert_subject: String,
    cert_issuer: String,
    cert_serial: String,
    cert_not_before: String,
    cert_not_after: String,
    cert_self_signed: bool,
    cert_key_bits: usize,
    cert_valid_at_signing: bool,
    chain_len: usize,
}

/// Xác thực chi tiết mọi chữ ký.
#[tauri::command]
pub async fn signx_verify(input: String) -> Result<Vec<SigCheckDto>, String> {
    blocking(move || {
        let checks = ff_engine::verify_signatures(Path::new(&input)).map_err(|e| e.to_string())?;
        Ok(checks
            .into_iter()
            .map(|c| SigCheckDto {
                revision: c.revision,
                intact: c.intact(),
                valid: c.is_valid(),
                signer: c.signer,
                name: c.name,
                reason: c.reason,
                location: c.location,
                contact_info: c.contact_info,
                sign_time: c.sign_time,
                sub_filter: c.sub_filter,
                crypto_valid: c.crypto_valid,
                digest_matches: c.digest_matches,
                covers_document: c.covers_document,
                later_changes_are_signatures: c.later_changes_are_signatures,
                cert_subject: c.cert.subject,
                cert_issuer: c.cert.issuer,
                cert_serial: c.cert.serial,
                cert_not_before: c.cert.not_before,
                cert_not_after: c.cert.not_after,
                cert_self_signed: c.cert.self_signed,
                cert_key_bits: c.cert.key_bits,
                cert_valid_at_signing: c.cert_valid_at_signing,
                chain_len: c.chain_len,
            })
            .collect())
    })
    .await
}

// ---------- Điền & Ký ----------

/// Đóng dấu các phần tử Điền & Ký vào nội dung trang. Ảnh gửi dạng data URL
/// (`imageDataUrl`) — ghi ra file tạm rồi xoá sau khi xong.
#[tauri::command]
pub async fn fillsign_apply(
    input: String,
    output: String,
    items: Vec<serde_json::Value>,
    password: Option<String>,
) -> Result<usize, String> {
    blocking(move || {
        if Path::new(&input) == Path::new(&output) {
            return Err("hãy lưu thành tệp mới".into());
        }
        let mut temps: Vec<PathBuf> = Vec::new();
        let mut parsed: Vec<ff_engine::FillItem> = Vec::new();
        let res = (|| -> Result<usize, String> {
            for mut v in items {
                if let Some(obj) = v.as_object_mut() {
                    if let Some(url) = obj.remove("imageDataUrl").and_then(|u| u.as_str().map(String::from)) {
                        let p = data_url_to_temp(&url, "fill")?;
                        obj.insert("imagePath".into(), serde_json::Value::String(p.to_string_lossy().into_owned()));
                        temps.push(p);
                    }
                }
                parsed.push(serde_json::from_value(v).map_err(|e| format!("phần tử không hợp lệ: {e}"))?);
            }
            let pdfium = crate::pdfium()?;
            ff_engine::apply_fill_sign(&pdfium, Path::new(&input), &parsed, Path::new(&output), password.as_deref())
                .map_err(|e| e.to_string())
        })();
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
        res
    })
    .await
}

// ---------- In ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintPageDto {
    data_url: String,
    width_px: u32,
    height_px: u32,
}

/// Render 1 trang để in ở `dpi`, tuỳ chọn kèm chú thích / dữ liệu form.
#[tauri::command]
pub async fn print_render_page(
    path: String,
    page: u16,
    dpi: f32,
    annotations: bool,
    forms: bool,
    grayscale: Option<bool>,
) -> Result<PrintPageDto, String> {
    blocking(move || {
        use pdfium_render::prelude::*;
        let pdfium = crate::pdfium()?;
        let doc = pdfium.load_pdf_from_file(&path, None).map_err(|e| e.to_string())?;
        let pg = doc.pages().get(page).map_err(|e| format!("trang {}: {e}", page + 1))?;
        let dpi = dpi.clamp(72.0, 600.0);
        // Bề rộng hiển thị (đã tính /Rotate) theo điểm → pixel.
        let w_px = (pg.width().value / 72.0 * dpi).round().max(1.0) as i32;
        let mut cfg = PdfRenderConfig::new()
            .set_target_width(w_px)
            .set_maximum_height(w_px * 8)
            .render_annotations(annotations)
            .render_form_data(forms)
            .use_print_quality(true);
        if grayscale.unwrap_or(false) {
            cfg = cfg.use_grayscale_rendering(true);
        }
        let bmp = pg.render_with_config(&cfg).map_err(|e| e.to_string())?;
        let img = bmp.as_image();
        let (wp, hp) = (img.width(), img.height());
        let mut buf = Vec::new();
        {
            use image::ImageEncoder;
            let rgb = img.to_rgb8();
            image::codecs::png::PngEncoder::new_with_quality(
                &mut buf,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::Sub,
            )
            .write_image(rgb.as_raw(), wp, hp, image::ExtendedColorType::Rgb8)
            .map_err(|e| e.to_string())?;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        Ok(PrintPageDto { data_url: format!("data:image/png;base64,{b64}"), width_px: wp, height_px: hp })
    })
    .await
}

// ---------- Batch / Action Wizard ----------

/// Chạy chuỗi hành động lên nhiều tệp; tiến độ qua sự kiện `batch://progress`.
#[tauri::command]
pub async fn batch_run(
    app: tauri::AppHandle,
    files: Vec<String>,
    steps: Vec<ff_engine::BatchStep>,
    out_dir: String,
    suffix: String,
) -> Result<Vec<ff_engine::BatchFileResult>, String> {
    BATCH_CANCEL.store(false, Ordering::SeqCst);
    blocking(move || {
        let pdfium = crate::pdfium()?;
        let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
        let mut emit = |e: ff_engine::BatchEvent| {
            let _ = app.emit("batch://progress", &e);
        };
        ff_engine::run_batch(&pdfium, &files, &steps, Path::new(&out_dir), &suffix, &BATCH_CANCEL, &mut emit)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn batch_cancel() {
    BATCH_CANCEL.store(true, Ordering::SeqCst);
}

/// Kiểm chuỗi bước trước khi chạy (thứ tự bước kết thúc…).
#[tauri::command]
pub fn batch_validate(steps: Vec<ff_engine::BatchStep>) -> Result<(), String> {
    ff_engine::validate_steps(&steps).map_err(|e| e.to_string())
}

/// Chọn nhiều tệp PDF.
#[tauri::command]
pub fn batch_pick_files(app: tauri::AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .blocking_pick_files()
        .map(|v| v.into_iter().map(|f| f.to_string()).collect())
        .unwrap_or_default()
}

/// Liệt kê PDF trong thư mục (tuỳ chọn đệ quy), sắp theo tên.
#[tauri::command]
pub fn batch_list_folder(dir: String, recursive: bool) -> Result<Vec<String>, String> {
    fn walk(d: &Path, rec: bool, depth: u32, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if rec && depth < 16 {
                    walk(&p, rec, depth + 1, out);
                }
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("pdf")).unwrap_or(false) {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new(&dir), recursive, 0, &mut out);
    out.sort();
    Ok(out)
}
