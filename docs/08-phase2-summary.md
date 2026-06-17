# 08 — Tổng kết Phase 2 (Annotate)

> Trạng thái: **lõi HOÀN TẤT**. 13/13 test tự động xanh. Đường GHI file (rủi ro nhất) đã round-trip + verify ảnh.

## 1. Đã làm

### Engine (`crates/ff-engine/src/annot.rs`)
- `apply_annotations(input, output, specs)`: tạo annotation rồi **lưu sang file mới** (không sửa file gốc), qua `document.save_to_file`.
- 6 loại: **Highlight, Underline, Strikeout** (text-markup, dùng QuadPoints), **Square** (khung), **FreeText** (hộp văn bản), **Note** (ghi chú dán).
- `list_annotations` / `count_annotations`: đọc lại annotation (loại, vùng, nội dung).
- Bài học: QuadPoints highlight phải theo thứ tự TL,TR,BL,BR (helper `quad_from_rect`) — `PdfQuadPoints::from_rect` của thư viện dùng thứ tự khác làm highlight bị xoắn.

### Tauri commands (`app/src-tauri/src/main.rs`)
- `apply_annotations(input, output, specs)` — map DTO (JS) → engine.
- `pick_save_pdf()` — hộp thoại Lưu.

### UI (`app/src`)
- Thanh **Chú thích**: 6 công cụ (Tô sáng, Gạch chân, Gạch ngang, Khung, Text box, Ghi chú) + chọn **màu** + nút **Lưu chú thích (n)**.
- **Vẽ bằng chuột**: kéo vùng trên trang → tạo chú thích, **preview tức thì** (lớp `.annotlayer`), toạ độ màn hình→điểm PDF chính xác (cùng phép biến đổi đã kiểm ở search/text-layer).
- Tab **Chú thích** (sidebar): danh sách chú thích chưa lưu, click để nhảy trang, ✕ để xoá.
- **Lưu**: nút → hộp thoại → ghi file mới (annotation thật) rồi mở lại để render.
- CLI: `ff annots <pdf>` liệt kê annotation; `ff chars <pdf>` (debug).

## 2. Test tự động (13/13 xanh)
- `annot_roundtrip` (3): Highlight+Square; Underline+Strikeout; FreeText+Note — ghi → mở lại → đúng loại/nội dung + render đúng màu.
- Cùng các test Phase 1 (viewer_features 6, big_doc 3, render_smoke 1).

## 3. Đã kiểm bằng tay/ảnh
- Round-trip render: highlight vàng phủ khít "content", khung đỏ (ảnh `tmp-out/annot-*.png`).
- UI: vẽ highlight/underline/strikeout/box hiển thị preview đúng vị trí, đổi màu/đổi công cụ, đếm số chú thích (ảnh `tmp-out/annot-draw.png`).

> Ghi chú kỹ thuật: tự-động-hoá chuột Win32 KHÔNG mô phỏng được thao tác "kéo" trong WebView2 (mousemove giả lập bị lọc) — nên drag-vẽ và drag-chọn-text chỉ kiểm được bằng ảnh khi events lọt, hoặc bằng thao tác rời (click/shift-click). Với chuột thật của người dùng, kéo hoạt động bình thường.

## 4. Còn lại / follow-up (chưa làm)
- **Ink** (vẽ tay tự do).
- **Sửa/di chuyển/xoá** annotation đã lưu (hiện chỉ xoá cái *chưa* lưu).
- Liệt kê annotation **đã lưu** trong tab Chú thích (hiện chỉ liệt kê cái chưa lưu).
- **Import/Export XFDF**.
- Độ trung thực cross-viewer (appearance stream) cho FreeText/Note ở Acrobat.

## 5. Cần người dùng xác nhận (vì kéo-chuột thật không tự test được)
Xem checklist: [docs/09-phase2-user-tests.md](09-phase2-user-tests.md).
