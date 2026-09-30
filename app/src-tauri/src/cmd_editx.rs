//! Lệnh Tauri + DTO cho "Edit Object" chuẩn Foxit (engine: ff_engine::editobj):
//! xoay/lật/cắt/độ mờ/trích ảnh, vẽ hình, viền-nền path, nhân bản, sắp lớp.
//! Các op mới đi chung đường `edit_apply*` của main.rs — field riêng của
//! chúng nằm trong `EditExtraDto` (flatten vào `EditOpDto`).

/// Field bổ sung của op sửa object (camelCase, mọi field tuỳ chọn).
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EditExtraDto {
    /// rotate: độ, ngược chiều kim đồng hồ.
    #[serde(default)]
    degrees: f32,
    /// flip: true = lật ngang (trái↔phải).
    #[serde(default)]
    horizontal: bool,
    /// rotate/flip: tâm quay (điểm PDF); thiếu = tâm của chính object.
    cx: Option<f32>,
    cy: Option<f32>,
    /// cropImage: vùng giữ [left, bottom, right, top] (điểm PDF).
    keep: Option<[f32; 4]>,
    /// setOpacity / addShape: 0..1.
    opacity: Option<f32>,
    /// setPathStyle / addShape: màu viền / nền RGB; noStroke/noFill = bỏ.
    stroke: Option<[u8; 3]>,
    fill: Option<[u8; 3]>,
    #[serde(default)]
    no_stroke: bool,
    #[serde(default)]
    no_fill: bool,
    /// Độ dày nét (điểm PDF).
    width: Option<f32>,
    /// Mẫu nét đứt; [] = nét liền.
    dash: Option<Vec<f32>>,
    /// addShape: "rect" | "roundRect" | "ellipse" | "line" | "arrow" + 2 điểm.
    shape: Option<String>,
    #[serde(default)]
    x0: f32,
    #[serde(default)]
    y0: f32,
    #[serde(default)]
    x1: f32,
    #[serde(default)]
    y1: f32,
    /// duplicate: trang nguồn (None = trang đang sửa).
    src_page: Option<u16>,
    /// duplicate: độ dịch bản sao.
    #[serde(default)]
    ox: f32,
    #[serde(default)]
    oy: f32,
    /// arrange: "front" | "back" | "forward" | "backward".
    mode: Option<String>,
}

/// Dựng EditOp cho các op object mới; `index`/`indices` lấy từ DTO chung.
pub fn edit_op_extra(
    op: &str,
    index: u16,
    indices: Vec<u16>,
    x: &EditExtraDto,
) -> Result<ff_engine::EditOp, String> {
    use ff_engine::EditOp;
    Ok(match op {
        "rotate" => EditOp::Rotate { index, degrees: x.degrees, center: x.cx.zip(x.cy) },
        "flip" => EditOp::Flip { index, horizontal: x.horizontal, center: x.cx.zip(x.cy) },
        "cropImage" => {
            let k = x.keep.ok_or("cropImage thiếu keep")?;
            EditOp::CropImage {
                index,
                keep: ff_engine::Rect { left: k[0], bottom: k[1], right: k[2], top: k[3] },
            }
        }
        "setOpacity" => EditOp::SetOpacity { index, opacity: x.opacity.unwrap_or(1.0) },
        "setPathStyle" => EditOp::SetPathStyle {
            index,
            style: ff_engine::PathStyle {
                stroke: x.stroke,
                no_stroke: x.no_stroke,
                fill: x.fill,
                no_fill: x.no_fill,
                width: x.width,
                dash: x.dash.clone(),
            },
        },
        "addShape" => {
            let kind = x
                .shape
                .as_deref()
                .and_then(ff_engine::EditShapeKind::parse)
                .ok_or("addShape: loại hình không hợp lệ")?;
            EditOp::AddShape(ff_engine::EditShapeSpec {
                kind,
                x0: x.x0,
                y0: x.y0,
                x1: x.x1,
                y1: x.y1,
                stroke: if x.no_stroke { None } else { x.stroke },
                fill: if x.no_fill { None } else { x.fill },
                width: x.width.unwrap_or(1.0),
                dash: x.dash.clone().unwrap_or_default(),
                opacity: x.opacity.unwrap_or(1.0),
            })
        }
        "duplicate" => EditOp::Duplicate { index, src_page: x.src_page, dx: x.ox, dy: x.oy },
        "arrange" => EditOp::Arrange {
            indices,
            mode: x
                .mode
                .as_deref()
                .and_then(ff_engine::ArrangeMode::parse)
                .ok_or("arrange: mode không hợp lệ")?,
        },
        other => return Err(format!("op sửa nội dung không hỗ trợ: {other}")),
    })
}

/// Thuộc tính Format bổ sung của object (flatten vào ObjectInfoDto).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjExtraDto {
    stroke_color: Option<[u8; 4]>,
    fill_color: Option<[u8; 4]>,
    stroke_width: Option<f32>,
    opacity: Option<f32>,
    rotation: Option<f32>,
    image_px: Option<[u32; 2]>,
}

pub fn obj_extra(o: &ff_engine::ObjectInfo) -> ObjExtraDto {
    ObjExtraDto {
        stroke_color: o.stroke_color,
        fill_color: o.fill_color,
        stroke_width: o.stroke_width,
        opacity: o.opacity,
        rotation: o.rotation,
        image_px: o.image_px.map(|(w, h)| [w, h]),
    }
}

/// Lưu ảnh gốc của image object ra PNG (Foxit "Lưu ảnh thành..."). Trả về [rộng, cao] px.
#[tauri::command]
pub fn editx_extract_image(
    path: String,
    page: u16,
    index: u16,
    output: String,
    password: Option<String>,
) -> Result<[u32; 2], String> {
    let pdfium = crate::pdfium()?;
    let (w, h) = ff_engine::extract_image(
        &pdfium,
        std::path::Path::new(&path),
        page,
        index,
        std::path::Path::new(&output),
        password.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    Ok([w, h])
}
