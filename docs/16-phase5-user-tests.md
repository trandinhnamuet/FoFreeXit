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

## Bổ sung Iteration 2 — Chữ ký số + redact ký tự + lưu tối ưu
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| S13 | **🪪 Tạo Digital ID** → nhập tên → lưu .pem | Tạo file .pem thành công | |
| S14 | **✍️ Ký số** → nhập tên/lý do → chọn .pem vừa tạo → lưu | File mới được ký; tự hiện bảng kiểm tra: **Hợp lệ** ✓ | |
| S15 | Mở file đã ký ở **Foxit/Adobe** | Hiện có chữ ký; xác thực toán học hợp lệ (có thể cảnh báo "chưa tin cậy CA" vì tự ký — bình thường) | |
| S16 | Sửa 1 chữ trong file đã ký (chế độ Sửa nội dung) rồi lưu, sau đó **🔎 Kiểm tra chữ ký** | Chữ ký chuyển **KHÔNG hợp lệ** (nội dung đã bị sửa) | |
| S17 | **🔎 Kiểm tra chữ ký** trên file chưa ký | Báo "chưa có chữ ký số nào" | |
| S18 | Redact 1 CỤM TỪ ở giữa dòng (không phải cả dòng) → Áp dụng | Chỉ cụm đó bị đen/biến mất; phần còn lại của dòng **vẫn hiển thị và copy được** | |
| S19 | **📦 Lưu tối ưu** file lớn → so dung lượng | File kết quả nhỏ hơn/không phình, mở đọc bình thường | |

## Mẫu phản hồi
```
[Mã] (vd S5) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Giới hạn đã biết (Iteration 1): redact xoá NGUYÊN text run bị vùng quét
> chạm vào (an toàn — thừa còn hơn sót; tỉa theo ký tự ở iteration 2); chữ ký
> số PAdES là Iteration 2 — xem `docs/15-phase5-summary.md` mục 5.
