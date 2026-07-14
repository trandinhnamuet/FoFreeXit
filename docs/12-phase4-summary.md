# 12 — Tổng kết Phase 4 (Chỉnh sửa nội dung — Edit) · Iteration 1

> Trạng thái: **lõi MOAT hoạt động**. 43/43 test engine xanh (36 cũ + 7 mới cho edit). Đã chạy app thật, chụp ảnh xác minh: sửa text tiếng Việt WYSIWYG, thêm chữ, xoá đối tượng, undo/redo.

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
- **Reflow đoạn nhiều dòng "như Word"** (hiện sửa ở mức text run/dòng — mỗi PdfPageTextObject; đây là điểm khác chính so với Foxit, cần gom run theo baseline + reflow).
- Xoay/lật/shear/clip object; tab Format nâng cao (viền, opacity, căn lề, đổi font-family object cũ); z-order arrange; convert text→path.
- Đặt ảnh theo đúng tỉ lệ gốc (hiện mặc định 150×112pt, resize bằng kéo handle); preview ảnh trước khi đặt.
- Sửa nhiều trang trong 1 phiên lưu (hiện lưu theo trang đang sửa); spell-check; sửa bảng; link text blocks.
- Mã hoá UI (nợ từ Phase 3).
