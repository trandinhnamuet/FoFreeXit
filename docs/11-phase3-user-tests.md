# 11 — Checklist test thủ công Phase 3 (Tổ chức trang)

Phần kéo-thả/kéo-chuột không tự động test đầy đủ được trong WebView2 (xem ghi chú ở [08-phase2-summary.md](08-phase2-summary.md)), nên cần bạn thử bằng chuột thật, đặc biệt phần Lưu (đường GHI file là rủi ro nhất).

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| O1 | Bấm **🗂 Tổ chức trang** | Viewport đổi thành lưới thumbnail lớn | |
| O2 | Click 1 trang, Ctrl+click thêm 1 trang | Cả 2 viền xanh (đã chọn) | |
| O3 | Shift+click trang khác | Chọn cả dải liên tục | |
| O4 | Kéo-thả 1 thumbnail sang vị trí khác | Thứ tự trang đổi ngay trên lưới | |
| O5 | Chọn 1 trang, bấm **Xoay phải/trái** | Thumbnail xoay 90° ngay | |
| O6 | Bấm **Hoàn tác** sau O5 | Xoay về như cũ | |
| O7 | Chọn trang, bấm **Xoá** | Trang biến mất khỏi lưới | |
| O8 | Chọn hết tất cả trang, bấm **Xoá** | Bị chặn (báo không thể xoá hết) | |
| O9 | Bấm **➕ Chèn** → Trang trắng → Cuối tài liệu | Thêm 1 trang trắng cuối lưới | |
| O10 | Bấm **➕ Chèn** → Từ file… → chọn 1 PDF khác | Trang từ file đó chèn đúng vị trí | |
| O11 | Chọn 1+ trang, bấm **📤 Trích** → chọn nơi lưu | Tạo file PDF mới chỉ chứa trang đã chọn | |
| O12 | Chọn 1+ trang, bấm **🔁 Thay** → chọn file khác | Nội dung trang đã chọn đổi thành trang từ file mới | |
| O13 | Bấm **🔀 Trộn file** → thêm 2-3 file, sắp thứ tự, Trộn | Tạo 1 file PDF mới ghép đúng thứ tự | |
| O14 | Bấm **✂ Tách file** → nhập số trang/file, chọn thư mục | Sinh nhiều file `_part1.pdf, _part2.pdf...` | |
| O15 | Bấm **💧 Watermark**, nhập chữ, bấm **Xem trước** | Ảnh xem trước hiện watermark đúng vị trí/màu/góc xoay | |
| O16 | Bấm **Áp dụng** ở Watermark → chọn nơi lưu | File mới có watermark trên các trang đã chọn | |
| O17 | Bấm **📋 Header/Footer**, gõ "Trang {page}/{total}" vào ô dưới-giữa, **Xem trước** | Ảnh xem trước hiện đúng số trang/tổng số trang | |
| O18 | Chọn tool **✂️ Crop** (thanh Chú thích), kéo 1 khung trên trang | Dialog Crop hiện 4 số margin đúng theo khung đã kéo | |
| O19 | Áp dụng Crop, vào lại Tổ chức trang | Thumbnail trang đó có dấu chấm crop ở góc | |
| O20 | Sau khi sửa vài thao tác, bấm **💾 Lưu thay đổi tổ chức trang** | Hỏi nơi lưu, tạo file mới đúng với mọi thay đổi đã làm | |
| O21 | Mở file đã lưu ở O20 bằng **Foxit/Adobe/Chrome** | Thứ tự/xoay/crop/nội dung trang đúng như trên lưới | |
| O22 | Mở 1 file PDF bị hỏng (xref lỗi) nếu có | App tự sửa (qua QPDF) và mở được, không báo lỗi cho người dùng | |

## Mẫu phản hồi
```
[Mã] (vd O21) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Đặc biệt cần biết **O20/O21** (lưu tổ chức trang ra file mới, mở lại ở phần mềm khác có đúng không) vì đó là đường GHI file rủi ro nhất. Engine đã test round-trip nội bộ cho từng thao tác riêng lẻ, nhưng tổ hợp nhiều thao tác liên tiếp + cross-viewer chưa có test tự động.
