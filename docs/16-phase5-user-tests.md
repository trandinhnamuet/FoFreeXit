# 16 — Checklist test thủ công Phase 5 (Bảo mật)

Quan trọng nhất: xác nhận **redaction xoá thật** (S4–S6) và **file mã hoá mở
đúng ở Foxit/Adobe** (S8) — đây là tính năng an toàn, sai là lộ dữ liệu.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| S1 | Bấm **🔒 Bảo mật** | Hiện thanh Bảo mật (redact / mật khẩu / metadata) | |
| S2 | Bấm **⬛ Đánh dấu redact** → kéo quét 1 vùng chữ | Khối đen mờ viền đỏ dashed phủ vùng; đếm ở nút "Áp dụng redact (n)" tăng | |
| S3 | Quét thêm vùng thứ 2 (trang khác cũng được) → bấm vào 1 khối | Khối bị bấm biến mất (bỏ đánh dấu); đếm giảm | |
| S4 | **Áp dụng redact** → lưu file mới | File mới mở lên: vùng redact là khối ĐEN | |
| S5 | Trong file kết quả: Ctrl+F tìm đúng chữ đã redact; thử chọn & copy vùng đó | **KHÔNG tìm thấy, không copy được** — nội dung đã bị xoá thật | |
| S6 | Redact đè lên 1 phần ẢNH rồi áp dụng; mở file kết quả ở Foxit, dùng Edit Object xem ảnh | Phần ảnh bị redact ĐEN NGAY TRONG ẢNH (kéo ảnh ra vẫn đen), không phải hình chữ nhật đè | |
| S7 | **🔑 Đặt mật khẩu**: nhập mật khẩu + bỏ tick "In" → lưu | File mới đòi mật khẩu khi mở | |
| S8 | Mở file S7 ở **Foxit/Adobe**: nhập đúng mật khẩu; thử In | Mở được bằng đúng mật khẩu; lệnh In bị chặn/mờ | |
| S9 | Nhập SAI mật khẩu ở app | Báo lỗi, không mở | |
| S10 | **🔓 Gỡ mật khẩu**: nhập mật khẩu hiện tại → chọn file S7 → lưu | File kết quả mở KHÔNG cần mật khẩu, nội dung nguyên vẹn | |
| S11 | Gỡ mật khẩu với mật khẩu SAI | Báo lỗi rõ ràng, không tạo file hỏng | |
| S12 | **🧹 Xoá metadata** → lưu → mở file kết quả ở Foxit: File › Properties | Author/Producer/Created… trống; app vẫn mở đọc bình thường | |

## Mẫu phản hồi
```
[Mã] (vd S5) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Giới hạn đã biết (Iteration 1): redact xoá NGUYÊN text run bị vùng quét
> chạm vào (an toàn — thừa còn hơn sót; tỉa theo ký tự ở iteration 2); chữ ký
> số PAdES là Iteration 2 — xem `docs/15-phase5-summary.md` mục 5.
