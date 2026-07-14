# 13 — Checklist test thủ công Phase 4 (Sửa nội dung)

Kéo-chuột (di chuyển/resize) không tự động test đầy đủ trong WebView2 (xem `docs/08`), nên cần bạn thử bằng chuột thật. Đặc biệt cần xác nhận **lưu ra file mở lại đúng** và **tiếng Việt đúng dấu** ở phần mềm khác.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E1 | Bấm **✏️ Sửa nội dung** | Viewport đổi thành trang lớn + khung mờ quanh từng đối tượng (text/ảnh) | |
| E2 | Bấm 1 lần vào 1 khung text | Khung viền xanh (đã chọn); ô Cỡ chữ + Màu bật lên | |
| E3 | Bấm đúp (double-click) 1 khung text | Hiện ô sửa tại chỗ với nội dung cũ | |
| E4 | Sửa thành chữ có dấu tiếng Việt, Enter | Trang render lại đúng nội dung mới, **đúng dấu** | |
| E5 | Chọn text rồi đổi **Cỡ chữ** | Chữ đổi kích thước | |
| E6 | Chọn text rồi đổi **Màu** | Chữ đổi màu | |
| E7 | Bấm **Thêm chữ** → click lên trang → gõ → Enter | Chữ mới xuất hiện đúng chỗ | |
| E8 | Bấm **Thêm ảnh** → chọn ảnh → click lên trang | Ảnh xuất hiện tại chỗ click | |
| E9 | Chọn 1 ảnh → **Thay ảnh** → chọn ảnh khác | Ảnh được thay, giữ khung cũ | |
| E10 | Chọn 1 đối tượng → **Kéo** để di chuyển | Đối tượng dời theo chuột | |
| E11 | Chọn 1 đối tượng → kéo **ô vuông góc** để resize | Đối tượng to/nhỏ lại | |
| E12 | Chọn 1 đối tượng → phím **Delete** (hoặc nút Xoá) | Đối tượng biến mất | |
| E13 | **Hoàn tác (Ctrl+Z)** sau vài thao tác | Lùi lại từng bước đúng | |
| E14 | **Làm lại (Ctrl+Y)** | Tiến lại bước vừa hoàn tác | |
| E15 | **Lưu thay đổi nội dung** → chọn nơi lưu | Tạo file PDF mới; app mở lại file đó | |
| E16 | Mở file đã lưu ở **Foxit/Adobe/Chrome** | Nội dung đã sửa hiển thị đúng (text, ảnh, **tiếng Việt đúng dấu**), file không hỏng | |

## Mẫu phản hồi
```
[Mã] (vd E16) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Quan trọng nhất: **E4** (sửa tiếng Việt), **E15/E16** (lưu + mở lại cross-viewer) — đường GHI nội dung là rủi ro nhất. Engine đã test round-trip nội bộ (gồm tiếng Việt), nhưng xác nhận cross-viewer của bạn rất giá trị.

> Giới hạn đã biết (Iteration 1): sửa text ở mức **dòng/run** (chưa reflow cả đoạn như Word); thêm ảnh mặc định khung 150×112pt (kéo handle để chỉnh); lưu áp cho trang đang sửa. Xem `docs/12-phase4-summary.md` mục 5.
