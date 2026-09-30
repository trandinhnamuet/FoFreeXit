//! Xử lý hàng loạt / Action Wizard (kiểu Foxit): áp MỘT CHUỖI BƯỚC lên
//! nhiều tệp. Mỗi tệp chạy tuần tự các bước qua file tạm (bước sau đọc kết
//! quả bước trước), ghi kết quả cuối vào thư mục đích với hậu tố tên tệp;
//! báo tiến độ từng tệp/bước qua callback; huỷ được giữa chừng (AtomicBool).
//!
//! THÊM BƯỚC MỚI: thêm 1 biến thể vào `BatchStep` (serde tag `kind`), mô tả
//! nó trong `label()`, và 1 nhánh trong `apply()` — không phải đụng chỗ khác.
//! Bước "kết thúc" (xuất ra định dạng khác PDF, hoặc mã hoá) phải đứng cuối:
//! khai báo trong `is_terminal()`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use pdfium_render::prelude::Pdfium;
use serde::{Deserialize, Serialize};

use crate::EngineError;

fn d_font_size() -> f32 {
    48.0
}
fn d_opacity() -> f32 {
    0.3
}
fn d_rotation() -> f32 {
    45.0
}
fn d_grey() -> [u8; 3] {
    [128, 128, 128]
}
fn d_black() -> [u8; 3] {
    [0, 0, 0]
}
fn d_hf_size() -> f32 {
    10.0
}
fn d_margin() -> f32 {
    28.0
}
fn d_dpi() -> f32 {
    150.0
}
fn d_lang() -> String {
    "vie+eng".into()
}

/// Định dạng xuất của bước Chuyển đổi.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConvertFormat {
    Word,
    Text,
    Images,
}

/// Một bước trong chuỗi hành động.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BatchStep {
    /// Nhận dạng chữ (thêm lớp chữ ẩn tìm/chọn được).
    #[serde(rename_all = "camelCase")]
    Ocr {
        #[serde(default = "d_lang")]
        lang: String,
    },
    /// Watermark chữ ở giữa trang.
    #[serde(rename_all = "camelCase")]
    Watermark {
        text: String,
        #[serde(default = "d_font_size")]
        font_size: f32,
        #[serde(default = "d_opacity")]
        opacity: f32,
        #[serde(default = "d_rotation")]
        rotation: f32,
        #[serde(default = "d_grey")]
        color: [u8; 3],
    },
    /// Header/footer — token {page} {total} {date}.
    #[serde(rename_all = "camelCase")]
    HeaderFooter {
        #[serde(default)]
        top_left: String,
        #[serde(default)]
        top_center: String,
        #[serde(default)]
        top_right: String,
        #[serde(default)]
        bottom_left: String,
        #[serde(default)]
        bottom_center: String,
        #[serde(default)]
        bottom_right: String,
        #[serde(default = "d_hf_size")]
        font_size: f32,
        #[serde(default = "d_margin")]
        margin: f32,
        #[serde(default = "d_black")]
        color: [u8; 3],
        #[serde(default)]
        date: String,
    },
    /// Tối ưu / nén (object stream + nén stream).
    Optimize,
    /// Mã hoá bằng mật khẩu (bước cuối).
    #[serde(rename_all = "camelCase")]
    Encrypt {
        user_password: String,
        #[serde(default)]
        owner_password: String,
    },
    /// Xoá metadata (/Info + XMP).
    RemoveMetadata,
    /// Làm phẳng form (giá trị field thành nội dung trang).
    FlattenForm,
    /// Xoay mọi trang (bội số 90, dương = theo chiều kim đồng hồ).
    #[serde(rename_all = "camelCase")]
    Rotate { degrees: i32 },
    /// Chuyển sang Word/Text/Ảnh (bước cuối).
    #[serde(rename_all = "camelCase")]
    Convert {
        format: ConvertFormat,
        #[serde(default = "d_dpi")]
        dpi: f32,
    },
}

impl BatchStep {
    /// Tên máy (khớp `kind`) — UI tự dịch.
    pub fn label(&self) -> &'static str {
        match self {
            BatchStep::Ocr { .. } => "ocr",
            BatchStep::Watermark { .. } => "watermark",
            BatchStep::HeaderFooter { .. } => "headerFooter",
            BatchStep::Optimize => "optimize",
            BatchStep::Encrypt { .. } => "encrypt",
            BatchStep::RemoveMetadata => "removeMetadata",
            BatchStep::FlattenForm => "flattenForm",
            BatchStep::Rotate { .. } => "rotate",
            BatchStep::Convert { .. } => "convert",
        }
    }

    /// Bước chỉ được đứng cuối chuỗi.
    pub fn is_terminal(&self) -> bool {
        matches!(self, BatchStep::Encrypt { .. } | BatchStep::Convert { .. })
    }

    /// Chạy bước trên `input` (PDF). Bước PDF→PDF ghi ra `output`; bước
    /// Chuyển đổi ghi thẳng vào `final_dir` với tên `final_stem` và trả các
    /// tệp đã tạo.
    fn apply(
        &self,
        pdfium: &Pdfium,
        input: &Path,
        output: &Path,
        final_dir: &Path,
        final_stem: &str,
    ) -> Result<Option<Vec<PathBuf>>, EngineError> {
        match self {
            BatchStep::Ocr { lang } => {
                crate::ocr::ocr_add_text_layer(pdfium, input, &[], lang, output, None)?;
            }
            BatchStep::Watermark { text, font_size, opacity, rotation, color } => {
                if text.trim().is_empty() {
                    return Err(EngineError::Pdfium("watermark: chưa nhập nội dung".into()));
                }
                let a = (opacity.clamp(0.02, 1.0) * 255.0).round() as u8;
                let spec = crate::WatermarkSpec {
                    text: text.clone(),
                    font_size: font_size.max(4.0),
                    color: [color[0], color[1], color[2], a],
                    bold: true,
                    italic: false,
                    rotation_deg: *rotation,
                    anchor: crate::Anchor::Center,
                    pages: vec![],
                };
                crate::add_watermark(pdfium, input, &spec, output, None)?;
            }
            BatchStep::HeaderFooter {
                top_left,
                top_center,
                top_right,
                bottom_left,
                bottom_center,
                bottom_right,
                font_size,
                margin,
                color,
                date,
            } => {
                let spec = crate::HeaderFooterSpec {
                    top_left: top_left.clone(),
                    top_center: top_center.clone(),
                    top_right: top_right.clone(),
                    bottom_left: bottom_left.clone(),
                    bottom_center: bottom_center.clone(),
                    bottom_right: bottom_right.clone(),
                    font_size: font_size.max(4.0),
                    color: [color[0], color[1], color[2], 255],
                    margin_pt: margin.max(0.0),
                    bold: false,
                    italic: false,
                    date: date.clone(),
                    pages: vec![],
                };
                crate::add_header_footer(pdfium, input, &spec, output, None)?;
            }
            BatchStep::Optimize => crate::optimize_save(input, output)?,
            BatchStep::Encrypt { user_password, owner_password } => {
                if user_password.is_empty() && owner_password.is_empty() {
                    return Err(EngineError::Pdfium("mã hoá: chưa nhập mật khẩu".into()));
                }
                let owner = if owner_password.is_empty() { user_password } else { owner_password };
                crate::encrypt_with_password(input, output, user_password, owner)?;
            }
            BatchStep::RemoveMetadata => {
                // lopdf khó tính với xref lệch → chuẩn hoá qua qpdf rồi thử lại.
                if crate::strip_metadata(input, output).is_err() {
                    let fixed = output.with_extension("fixed.pdf");
                    crate::repair(input, &fixed)?;
                    let r = crate::strip_metadata(&fixed, output);
                    let _ = std::fs::remove_file(&fixed);
                    r?;
                }
            }
            BatchStep::FlattenForm => crate::flatten_form(pdfium, input, output, None)?,
            BatchStep::Rotate { degrees } => {
                let d = degrees.rem_euclid(360);
                if d % 90 != 0 {
                    return Err(EngineError::Pdfium("xoay: góc phải là bội số 90".into()));
                }
                if d == 0 {
                    std::fs::copy(input, output)?;
                } else {
                    crate::rotate_pages(pdfium, input, &[], d, output, None)?;
                }
            }
            BatchStep::Convert { format, dpi } => {
                std::fs::create_dir_all(final_dir)?;
                let outs = match format {
                    ConvertFormat::Word => {
                        let o = unique_path(&final_dir.join(format!("{final_stem}.docx")));
                        crate::export_docx(pdfium, input, &o, None)?;
                        vec![o]
                    }
                    ConvertFormat::Text => {
                        let o = unique_path(&final_dir.join(format!("{final_stem}.txt")));
                        crate::export_text(pdfium, input, &o, None)?;
                        vec![o]
                    }
                    ConvertFormat::Images => {
                        // export_images đặt tên theo tên tệp nguồn → đặt tên tạm đúng stem.
                        let named = std::env::temp_dir().join(format!("ff_batch_{}_{}", std::process::id(), nanos()));
                        std::fs::create_dir_all(&named)?;
                        let src = named.join(format!("{final_stem}.pdf"));
                        std::fs::copy(input, &src)?;
                        let r = crate::export_images(pdfium, &src, final_dir, dpi.clamp(36.0, 1200.0), None);
                        let _ = std::fs::remove_dir_all(&named);
                        r?
                    }
                };
                return Ok(Some(outs));
            }
        }
        Ok(None)
    }
}

/// Sự kiện tiến độ.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BatchEvent {
    #[serde(rename_all = "camelCase")]
    FileStart { index: usize, total: usize, file: String },
    #[serde(rename_all = "camelCase")]
    Step { index: usize, step: usize, steps: usize, kind: String },
    #[serde(rename_all = "camelCase")]
    FileDone { index: usize, ok: bool, error: Option<String>, outputs: Vec<String> },
    #[serde(rename_all = "camelCase")]
    Finished { ok: usize, failed: usize, cancelled: bool },
}

/// Kết quả từng tệp.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFileResult {
    pub input: String,
    pub outputs: Vec<String>,
    pub error: Option<String>,
}

/// Kiểm chuỗi bước hợp lệ (không rỗng, bước kết thúc ở cuối).
pub fn validate_steps(steps: &[BatchStep]) -> Result<(), EngineError> {
    if steps.is_empty() {
        return Err(EngineError::Pdfium("chuỗi hành động chưa có bước nào".into()));
    }
    let terminals = steps.iter().filter(|s| s.is_terminal()).count();
    if terminals > 1 {
        return Err(EngineError::Pdfium("chỉ được 1 bước Mã hoá hoặc Chuyển đổi, đặt ở cuối".into()));
    }
    if let Some(pos) = steps.iter().position(|s| s.is_terminal()) {
        if pos != steps.len() - 1 {
            return Err(EngineError::Pdfium(format!(
                "bước \"{}\" phải đứng cuối chuỗi hành động",
                steps[pos].label()
            )));
        }
    }
    Ok(())
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Không ghi đè: "a.pdf" có rồi → "a (2).pdf", "a (3).pdf"…
fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().unwrap_or(Path::new("."));
    for i in 2..10000 {
        let c = dir.join(format!("{stem} ({i}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    p.to_path_buf()
}

/// Chạy chuỗi `steps` lên từng tệp trong `files`, ghi kết quả vào `out_dir`
/// với tên `<tên gốc><suffix>.<đuôi>`. Tệp lỗi không dừng cả lô.
pub fn run_batch(
    pdfium: &Pdfium,
    files: &[PathBuf],
    steps: &[BatchStep],
    out_dir: &Path,
    suffix: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(BatchEvent),
) -> Result<Vec<BatchFileResult>, EngineError> {
    validate_steps(steps)?;
    std::fs::create_dir_all(out_dir)?;
    let mut results = Vec::new();
    let (mut ok, mut failed) = (0usize, 0usize);
    let total = files.len();
    for (index, file) in files.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        progress(BatchEvent::FileStart { index, total, file: file.to_string_lossy().into_owned() });
        let r = run_one(pdfium, index, file, steps, out_dir, suffix, cancel, progress);
        let (outputs, error) = match r {
            Ok(o) => (o.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(), None),
            Err(e) => (vec![], Some(e.to_string())),
        };
        if error.is_none() {
            ok += 1;
        } else {
            failed += 1;
        }
        progress(BatchEvent::FileDone { index, ok: error.is_none(), error: error.clone(), outputs: outputs.clone() });
        results.push(BatchFileResult { input: file.to_string_lossy().into_owned(), outputs, error });
    }
    progress(BatchEvent::Finished { ok, failed, cancelled: cancel.load(Ordering::SeqCst) });
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
fn run_one(
    pdfium: &Pdfium,
    index: usize,
    file: &Path,
    steps: &[BatchStep],
    out_dir: &Path,
    suffix: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(BatchEvent),
) -> Result<Vec<PathBuf>, EngineError> {
    if !file.is_file() {
        return Err(EngineError::Pdfium(format!("không tìm thấy tệp {}", file.display())));
    }
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let final_stem = format!("{stem}{suffix}");
    let work = std::env::temp_dir().join(format!("ff_batch_{}_{}_{index}", std::process::id(), nanos()));
    std::fs::create_dir_all(&work)?;
    let res = (|| {
        // Mở được bằng PDFium (tự sửa file hỏng qua qpdf nếu cần).
        let mut cur = crate::ensure_openable(pdfium, file, None)?;
        for (si, step) in steps.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                return Err(EngineError::Pdfium("đã huỷ".into()));
            }
            progress(BatchEvent::Step { index, step: si, steps: steps.len(), kind: step.label().to_string() });
            let next = work.join(format!("s{si}.pdf"));
            if let Some(outs) = step.apply(pdfium, &cur, &next, out_dir, &final_stem)? {
                return Ok(outs);
            }
            cur = next;
        }
        let dest = unique_path(&out_dir.join(format!("{final_stem}.pdf")));
        // Không bao giờ ghi đè tệp nguồn.
        let dest = if dest == file { unique_path(&out_dir.join(format!("{final_stem} (out).pdf"))) } else { dest };
        std::fs::copy(&cur, &dest)?;
        Ok(vec![dest])
    })();
    let _ = std::fs::remove_dir_all(&work);
    res
}
