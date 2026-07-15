# 12 — Tổng kết Phase 4 (Chỉnh sửa nội dung — Edit) · Iteration 1 + 2 + 3

> Trạng thái sau Iteration 3: **sửa CẢ ĐOẠN với reflow "như Word" + GIỮ FONT**.
> 52/52 test engine xanh ngoài qpdf (17 test edit + 6 unit fontmatch + 29 test cũ).

## Iteration 3 — Reflow đoạn văn "như Word" (mới nhất)

Đóng gap trải nghiệm cuối cùng của tính năng moat (mục 3.1 của
`docs/14-foxit-gap-analysis.md`): double-click vào đoạn nhiều dòng → sửa cả
đoạn trong 1 ô, chữ tự bẻ dòng lại theo bề rộng khối khi commit.

### Engine — `EditOp::ReflowText { indices, text }`
- Engine TỰ suy hình học từ các run: lề trái/bề rộng khối từ bounds, baseline
  từ `matrix.f` (gom cụm ±1pt), khoảng cách dòng = median hiệu baseline
  (1 dòng → 1.25× cỡ hiển thị) — UI chỉ cần gửi indices + text mới.
- **Bẻ dòng đo bằng hmtx thật** (`fontmatch::wrap_lines` + `char_advance` qua
  ttf-parser): greedy theo từ, `\n` = ngắt cứng, từ quá dài cắt theo ký tự,
  dòng rỗng giữ nhịp baseline. Không kerning (chấp nhận v1, nới 2%).
- **Giữ font theo thang 4 mức** (nhất quán triết lý iteration 2):
  1. Font NHÚNG parse được + phủ đủ glyph → nhúng lại chính bytes đó (glyph y hệt);
  2. Family nhóm base-14 (Helvetica/Times/Courier + Arial alias) + text ASCII →
     **font chuẩn PDF qua `FPDFText_LoadStandardFont`** — BaseFont giữ tên chuẩn,
     KHÔNG nhúng, file không phình; đo width bằng font metric-compatible
     (Liberation/Arial);
  3. Font hệ thống cùng họ (coverage-checked); 4. fallback mặc định.
- Dòng mới giữ phần tuyến tính matrix của run neo (scale/nghiêng), gốc tại
  (lề trái, baseline thứ i); Tf = unscaled của run neo → cỡ hiển thị không đổi.
- Đoạn dài ra thì các dòng mới nối xuống dưới theo đúng nhịp baseline (hành vi
  tương tự Foxit khi khối text nở ra).

### UI
- Double-click: **đoạn ≥2 dòng → textarea sửa cả đoạn** (`paragraphLines` gom
  dòng baseline cách đều ±25%, cỡ chữ tương đồng, giao ngang ≥30%); 1 dòng →
  ô sửa dòng như iteration 2.
- Textarea WYSIWYG: đúng font/cỡ/màu/kiểu + line-height đúng nhịp baseline;
  nội dung khởi tạo = các dòng nối bằng khoảng trắng (đoạn chảy tự nhiên);
  **Enter = ngắt cứng, Ctrl+Enter hoặc bấm ra ngoài = áp dụng, Esc = huỷ**.
- 1 commit = 1 op `reflowText` = 1 nấc undo.

### Test mới (3 integration + 3 unit)
- `reflow_wraps_and_keeps_embedded_font` — đoạn 3 dòng font nhúng + text Việt
  dài gấp mấy lần → bẻ >3 dòng, mọi dòng trong bề rộng khối, **font nhúng giữ
  nguyên**, baseline đều 15pt±2, text cũ biến mất.
- `reflow_hard_break_creates_new_line` — `\n` ra đúng 2 dòng cách 1 nhịp.
- `reflow_base14_ascii_keeps_standard_font` — Helvetica không nhúng + ASCII
  dài → nhiều dòng, **BaseFont vẫn "Helvetica" chuẩn** (builtin, không nhúng).
- Unit `wrap_lines`: greedy theo từ / ngắt cứng + dòng rỗng / cắt từ quá dài.

### Giới hạn ghi nhận (v1, sẽ nâng ở vòng sau)
- Đoạn justify (giãn đều 2 lề) reflow về căn trái; chưa kerning; khối text
  XOAY chưa reflow theo hướng xoay (dòng mới đặt theo trục ngang).
- Khối lẫn NHIỀU font/cỡ trong 1 đoạn: dòng mới thống nhất theo run neo.

> Iteration 1: 43/43 test, đã chạy app thật + chụp ảnh xác minh.

## Iteration 2 — Giữ font gốc + trải nghiệm Foxit (mới)

Đóng khoảng cách lớn nhất so với Foxit được chỉ ra khi review: **mọi lần sửa
text đều bị đổi sang font hệ thống** (Helvetica → Arial/DejaVu), kể cả khi chỉ
đổi cỡ/màu; bold/italic gốc bị mất; sửa theo mảnh run thay vì cả dòng.

### Engine (`edit.rs` viết lại + `fontmatch.rs` mới)
- **`SetText` 3 tầng, ưu tiên GIỮ FONT GỐC** (sửa tại chỗ bằng `FPDFText_SetText`,
  không xoá/tạo lại):
  1. *In-place an toàn chắc chắn*: text mới chỉ dùng ký tự đã có trong run; hoặc
     font **không nhúng** (base-14 Helvetica/Times…) + text mới toàn ASCII
     (BaseFont khai báo giữ nguyên trong file — đúng hành vi Foxit); hoặc cmap
     của font (đọc qua `FPDFFont_GetFontData` + `ttf-parser`) phủ đủ ký tự mới
     → **font giữ nguyên 100%**, gồm cả font NHÚNG với tiếng Việt.
  2. *Thiếu glyph thật sự* → thay bằng font hệ thống **CÙNG HỌ, đúng đậm/nghiêng**
     (`fontmatch::find_family_font_bytes`: bảng family Windows/macOS, Liberation
     metric-compatible + `fc-match` trên Linux; alias Helvetica→Arial,
     Times→Times New Roman…), có kiểm coverage trước khi nhận.
  3. Bất đắc dĩ mới rơi về font mặc định (`find_font_bytes` — nay có đủ biến thể
     đậm/nghiêng trên cả Linux).
- **Đổi cỡ chữ tại chỗ không đụng font**: nhân matrix `[k,0,0,k, e(1−k), f(1−k)]`
  (neo baseline như Foxit). **Fix bug phóng đại kép**: cỡ chữ nghĩa "hiển thị"
  (đã nhân matrix scale) — trước đây đặt cỡ 20 lên text có matrix scale ×2 ra 40pt.
- `ObjectInfo` thêm `font_family` (đã làm sạch tên PostScript/subset),
  `font_bold`, `font_italic`, `font_embedded` — nguồn từ `name()` (BaseFont) vì
  `family()` của font không nhúng trả tên stub nội bộ PDFium ("Chrom Sans OTF").
- `SetText`/`AddText` nhận `font_family`/`bold`/`italic` dạng Option — **None =
  giữ nguyên**; Some = chủ động đổi (đổi font qua tầng 2).
- Bẫy mới ghi nhận: `set_matrix` của pdfium-render 0.8.37 là alias deprecated
  của `apply_matrix` (NHÂN DỒN, không thay thế) — phải tự dựng matrix delta.

### UI (`main.js` + toolbar)
- **Sửa CẢ DÒNG như Foxit**: double-click gom các run cùng baseline liền kề
  (PDF hay cắt 1 dòng thành nhiều run) → 1 ô sửa cho cả dòng; commit = SetText
  run đầu + Delete các run còn lại (1 batch, 1 nấc undo).
- **WYSIWYG khi gõ**: ô sửa dùng đúng family (CSS xấp xỉ theo `font_family`),
  cỡ, màu, đậm/nghiêng của run gốc; nền trắng che text cũ.
- **Giữ nguyên mặc định**: mọi commit sửa text gửi `null` cho font/cỡ/màu/kiểu
  — chỉ field người dùng chủ động đổi mới được gửi.
- **Toolbar Format kiểu Foxit**: dropdown Font (mặc định "(giữ nguyên: X)"),
  nút **B**/**I** toggle theo kiểu thật của run (đổi biến thể cùng họ), Cỡ chữ,
  Màu — đổi thuộc tính KHÔNG đổi font.
- **Kéo-thả live**: khung đi theo con trỏ ngay khi kéo (move + resize), thả
  chuột mới commit; resize neo góc trên-trái đúng như preview.
- **Dọn file tạm**: mọi file `ff_edit_*.pdf` của phiên sửa được xoá khi thoát
  chế độ/lưu (command `edit_cleanup` — chỉ xoá đúng pattern trong %TEMP%).
- Hint hiển thị font đang chọn: `Tên font · cỡ pt · font nhúng/hệ thống`.

### Test mới (7): `edit_roundtrip.rs` 14 test + `fontmatch` 3 unit
- `set_text_keeps_original_font` — sửa ASCII trên Helvetica base-14 GIỮ NGUYÊN
  "Helvetica" (ép ký tự ngoài charset cũ để không ăn may).
- `set_text_preserves_embedded_font_vietnamese` — **font NHÚNG + tiếng Việt
  giữ nguyên font** (case quan trọng nhất với tài liệu Việt).
- `vietnamese_on_base14_uses_matched_family` — tiếng Việt trên base-14 (không
  có glyph Việt để giữ) match đúng họ metric-compatible (Liberation/Arial),
  không rơi bừa về generic.
- `font_size_change_keeps_font_and_anchors`; `font_size_change_respects_matrix_scale`
  (hồi quy bug phóng đại kép); `bold_override_substitutes_font_and_keeps_text`;
  `line_merge_batch_set_text_plus_delete` (luồng UI gộp dòng).

### Đối chiếu FINAL TARGET & RULE
- Vướng thư viện (PDFium `set_text` re-encode theo subset) → **không cắt giảm**:
  kiểm tra coverage bằng chính font bytes + luật charset/ASCII để tận dụng
  `set_text` an toàn, chỉ thay font khi về mặt vật lý không còn glyph để giữ —
  và khi thay thì match cùng họ như Foxit. Đúng thứ tự a→c của luật.

## Iteration 1 (giữ nguyên bên dưới để tham chiếu)

Phase 4 là tính năng lõi/moat theo `docs/03-roadmap.md`: sửa text & object trực tiếp trên trang — lý do quan trọng nhất để bỏ Foxit.

## 1. Khảo sát trước khi code
- **Foxit UX**: *Edit Text* (bấm đoạn → sửa như Word, reflow, đổi font/cỡ/màu); *Edit Object* (chọn text/ảnh/path → di chuyển/resize/xoay/xoá/thay ảnh, tab Format). Nguồn ở `docs/13-phase4-user-tests.md`.
- **pdfium-render 0.8.37** (đọc source vendored): PDFium đã expose page object cấp cao — KHÔNG cần tự parse content stream. Mỗi `PdfPageTextObject` là 1 **text run** sẵn để sửa (`text()`/`set_text()`/`font()`/`unscaled_font_size()`/fill color/transform). Có `create_text_object`/`create_image_object`/`remove_object_at_index`, `PdfFont::name/family/weight`, `regenerate_content()`. → khả thi cao, vượt kỳ vọng worst-case của roadmap.

## 2. Đã làm

### Engine (`crates/ff-engine/src/edit.rs`)
- `list_objects(input, page, password) -> Vec<ObjectInfo>`: liệt kê object (kind, AABB từ `bounds()`, và với text: nội dung/font/cỡ/màu) — cấp dữ liệu cho overlay UI.
- `apply_edits(input, page, ops, output, password)` với `EditOp`: **Transform** (di chuyển + scale quanh góc dưới-trái), **SetText** (sửa text run — tạo lại bằng FULL font nhúng để tiếng Việt đúng dấu, giữ matrix/cỡ/màu gốc qua `apply_matrix`), **Delete**, **ReplaceImage**, **AddText**, **AddImage**. Thứ tự xử lý giữ index gốc hợp lệ: Transform in-place → chụp dữ liệu object sắp thay → xoá theo index GIẢM DẦN → thêm bản thay thế → thêm object mới → `regenerate_content()` → lưu.
- Tái dùng `annot.rs::find_font_bytes` + `fonts_mut().load_true_type_from_bytes(bytes, true)` (như watermark.rs) cho sửa/thêm text Unicode.
- **Bài học quan trọng (đã ghi memory):** object trả về từ `remove_object_at_index` bị đánh dấu *unowned* → `Drop` gọi `FPDFPageObj_Destroy` gây **SEGFAULT** với PDFium build hiện tại. Phải `std::mem::forget` object đã xoá (đã tách khỏi trang, không cần destroy; rò rỉ nhỏ giải phóng khi đóng document).

### Tauri commands (`app/src-tauri/src/main.rs`)
`edit_list_objects`, `edit_apply` (ghi ra output), `edit_apply_to_temp` (áp ops → file tạm mới, trả path — cho mô hình materialize tức thì ở UI), `edit_preview` (render WYSIWYG), `pick_image`. DTO `ObjectInfoDto` (reuse `RectDto`), `EditOpDto` (tagged theo field `op`).

### UI (`app/src`) — chế độ "✏️ Sửa nội dung"
- Nút toolbar bật chế độ riêng: thay viewport bằng `#editStage` (ảnh trang lớn) + `#editOverlay` (1 box/đối tượng, map pdf→css theo `scale = STAGE_W/pageWidthPt`).
- **Mô hình "materialize tức thì"**: mỗi thao tác áp NGAY vào 1 file tạm mới (`edit_apply_to_temp`) rồi đọc lại object + render ảnh từ đó → index luôn khớp ảnh đang hiện (WYSIWYG thật), không cần tự suy đoán vị trí sau biến đổi.
- Thao tác: click chọn (viền xanh + handle); **double-click text → sửa tại chỗ** (ô input, commit Enter → SetText); đổi **cỡ chữ/màu** ở thanh công cụ cho text đang chọn; **Thêm chữ** (bấm nút → click lên trang → gõ); **Thêm ảnh**/**Thay ảnh** (chọn file → đặt); **Xoá** (nút/phím Delete); **kéo di chuyển / kéo handle resize** (chuột thật).
- **Undo/Redo riêng cho edit** (stack file tạm trước mỗi op), dùng chung 2 nút Hoàn tác/Làm lại + Ctrl+Z/Ctrl+Y khi đang ở chế độ sửa.
- **Lưu**: `edit_apply` ghi file mới rồi `loadDocument`.

## 3. Test tự động (7 mới, `tests/edit_roundtrip.rs`)
list_objects thấy text trang 1; SetText đổi nội dung (text cũ biến mất); SetText **tiếng Việt** round-trip đúng dấu; Delete giảm đúng 1 object; Transform translate dịch bounds ~+50; AddText xuất hiện trong extract_text; AddImage tăng 1 object kind Image. Tổng `ff-engine`: **43/43 xanh**.

## 4. Đã kiểm bằng ảnh (app build release thật)
Bật chế độ Sửa nội dung (overlay 2 đối tượng); double-click dòng 2 → sửa thành "Sửa: nội dung Tiếng Việt" → commit → trang render lại ĐÚNG (dấu chuẩn); Thêm chữ "Dòng chữ MỚI thêm" tại vị trí click; Hoàn tác bỏ chữ vừa thêm (giữ phần sửa); Xoá tiêu đề → còn 1 đối tượng. Ảnh: `tmp-out/phase4-*.png`.

## 5. Còn lại / follow-up (ghi nhận, không phải thiếu sót — Iteration sau)
- **Reflow đoạn nhiều dòng "như Word"** — khoảng cách chính còn lại so với Foxit
  (Iteration 2 đã sửa được CẢ DÒNG; bước tiếp: gom block nhiều dòng theo khoảng
  cách baseline + đo width bằng hmtx của font (ttf-parser đã có) + tự bẻ dòng).
  Xem kế hoạch ở `docs/14-foxit-gap-analysis.md`.
- Xoay/lật/shear/clip object; tab Format nâng cao (viền, opacity, căn lề); z-order arrange; convert text→path. (Đổi font-family/B/I cho object cũ: ĐÃ XONG ở Iteration 2.)
- Đặt ảnh theo đúng tỉ lệ gốc (hiện mặc định 150×112pt, resize bằng kéo handle); preview ảnh trước khi đặt.
- Sửa nhiều trang trong 1 phiên lưu (hiện lưu theo trang đang sửa); spell-check; sửa bảng; link text blocks.
- Mã hoá UI (nợ từ Phase 3).
