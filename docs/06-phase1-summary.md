# 06 — Tổng kết Phase 1 (Engine core + Viewer)

> Trạng thái: **HOÀN TẤT** các mục lõi. 10/10 test tự động xanh. Đã xác minh UI bằng screenshot & thao tác thực.

## 1. Đã xây dựng những gì

### Hạ tầng & toolchain
- Cài Rust 1.96 (MSVC) + Visual Studio Build Tools 2026 (workload C++) + WebView2.
- Tải PDFium prebuilt (`pdfium.dll`) qua [scripts/fetch-pdfium.ps1](../scripts/fetch-pdfium.ps1).
- **Cargo workspace**: `crates/ff-cos`, `crates/ff-pdmodel`, `crates/ff-engine`, `crates/ff-cli`.
- **App Tauri** tại `app/` (workspace tách riêng) — chạy bằng `cd app/src-tauri; cargo run`.

### Engine (`ff-engine`, dùng PDFium qua `pdfium-render`)
| Hàm | Chức năng |
|-----|-----------|
| `bind_pdfium` | Nạp thư viện PDFium động |
| `render_page` / `render_page_png` | Render trang ra ảnh (RGBA/PNG) theo chiều rộng |
| `page_count`, `page_dims` | Số trang & kích thước từng trang |
| `extract_text` | Trích text một trang |
| `search` | Tìm chuỗi (có/không phân biệt hoa thường) + **toạ độ rect** từng kết quả |
| `outline` | Đọc bookmarks + trang đích |
| `page_char_boxes` | Hộp bao từng ký tự (cho text-layer chọn/copy) |

### CLI (`ff`) — chạy mọi tính năng không cần UI (phục vụ test)
`info`, `render`, `pages`, `text`, `search`, `outline`.

### App desktop (Tauri + frontend tĩnh)
- **Mở file** qua hộp thoại native (`📂 Mở`) — lọc *.pdf.
- **Xem**: cuộn liên tục nhiều trang, **lazy render** (chỉ render trang trong tầm nhìn) → mở mượt file 1000+ trang.
- **Zoom** −/＋/Fit (re-render sắc nét theo devicePixelRatio).
- **Thumbnails** sidebar (lazy-render, đánh dấu trang hiện tại).
- **Outline** sidebar (click → nhảy trang).
- **Tìm kiếm** + highlight chính xác từng kết quả, điều hướng ◀ x/y ▶, highlight bám đúng cả khi zoom.
- **Chọn & copy text**: text-layer trong suốt phủ đúng từng glyph; chọn bằng chuột/Shift+Click + Ctrl+C, hoặc nút **Copy** (cả trang).

## 2. Test tự động (10/10 xanh) — `cargo test`
- `render_smoke`: mở & render `hello.pdf`, trang không trắng trơn.
- `viewer_features` (6): `page_dims`, `extract_text`, `search` (số lượng+vị trí), phân biệt hoa thường, `outline`, `char_boxes` (ghép ký tự = nội dung + hộp nằm trong khổ trang).
- `big_doc` (3): file **1000 trang** — đúng số trang; render trang đầu/cuối < 5s & không trắng; search toàn tài liệu ra đúng 1 kết quả < 10s.

Hiệu năng đo thực tế trên `big-1000.pdf`: mở 55ms · render 1 trang 110ms · search toàn bộ 79ms.

## 3. Giới hạn đã biết của Phase 1 (sẽ tinh chỉnh)
- ~~Double-click chọn 1 ký tự~~ → **ĐÃ SỬA**: text-layer gom ký tự thành "từ", double-click chọn cả từ; copy giữ khoảng trắng (chèn text node giữa các từ). Đã verify: double-click → chọn nguyên từ "FoFreeXit".
- Chưa có: in ấn, xoay trang khi xem, nhớ file mở gần đây, kéo-thả file vào cửa sổ.
- `render_page` nạp lại tài liệu mỗi lần gọi (chưa cache document trong bộ nhớ) — đủ nhanh hiện tại, sẽ tối ưu bằng cache khi cần.
- Bản dev tìm `pdfium.dll` ở gốc workspace; bản phát hành (Phase 8) sẽ bundle.

## 4. Kiến trúc vẫn đúng định hướng
2 tầng COS/PD đã đặt khung (`ff-cos`, `ff-pdmodel`); engine tách hẳn UI, gọi được qua CLI + Tauri. Sẵn sàng cho Phase 2 (Annotate).
