# 07 — Checklist test thủ công cho Phase 1 (dành cho người dùng)

Mục tiêu: bạn tự kiểm tra viewer, ghi lại chỗ nào chưa ổn để cải thiện **trước khi sang Phase 2**.

## Cách chạy app
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```
(Lần đầu mở sẵn `corpus/sample-multipage.pdf`. Nếu thiếu `pdfium.dll`: chạy `scripts/fetch-pdfium.ps1`.)

## Bảng test — đánh dấu ✅/❌ và ghi chú

### A. Mở file
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| A1 | Bấm **📂 Mở**, chọn 1 PDF thật của bạn | Mở & hiển thị đúng | |
| A2 | Mở file **rất lớn** (vài trăm–1000+ trang) | Mở nhanh, không treo | |
| A3 | Mở file **scan** (ảnh, không có text) | Hiển thị được (sẽ không chọn được text — đúng) | |
| A4 | Mở file **tiếng Việt có dấu** | Chữ + dấu hiển thị đúng | |
| A5 | Mở file **mã hoá/đặt mật khẩu** | (Dự kiến lỗi — Phase 5 mới hỗ trợ) ghi lại hiện tượng | |
| A6 | Mở file **hỏng/khác thường** | Không crash; báo lỗi gọn | |

### B. Xem & điều hướng
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| B1 | Cuộn lên/xuống nhiều trang | Mượt, trang hiện dần | |
| B2 | Chỉ số "Trang x/y" khi cuộn | Cập nhật đúng trang đang xem | |
| B3 | Bấm thumbnail | Nhảy đúng trang | |
| B4 | Tab **Outline**, bấm mục | Nhảy đúng trang đích | |
| B5 | So sánh hiển thị với Foxit/Chrome cùng file | Trông giống nhau | |

### C. Zoom
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| C1 | Bấm ＋ / − nhiều lần | Phóng to/thu nhỏ, chữ vẫn nét | |
| C2 | Bấm **Fit** | Vừa bề rộng cửa sổ | |
| C3 | Zoom rồi cuộn | Không vỡ layout, không lag nặng | |

### D. Tìm kiếm
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| D1 | Gõ 1 từ có trong file | Hiện "k/n", nhảy tới kết quả, **ô vàng/cam đúng chỗ** | |
| D2 | Bấm ◀ ▶ duyệt kết quả | Nhảy lần lượt, đổi highlight | |
| D3 | Tìm từ tiếng Việt có dấu | Tìm đúng | |
| D4 | Tìm từ không tồn tại | Hiện 0, không lỗi | |

### E. Chọn & copy text
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| E1 | **Kéo chuột** chọn 1 đoạn text | Bôi xanh đúng vùng | |
| E2 | Ctrl+C rồi dán ra Notepad | Đúng nội dung, đúng thứ tự | |
| E3 | **Double-click** 1 từ | (Hiện chỉ chọn 1 ký tự — đã biết, sẽ sửa) | |
| E4 | Nút **Copy** | Copy text cả trang hiện tại | |
| E5 | Copy đoạn **tiếng Việt có dấu** | Dấu không bị mất/lỗi | |

### F. Ổn định & hiệu năng
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| F1 | Mở/đóng/đổi nhiều file liên tiếp | Không rò bộ nhớ rõ rệt, không crash | |
| F2 | Mở file lớn rồi cuộn nhanh | RAM/CPU chấp nhận được | |
| F3 | Phóng to hết cỡ file lớn | Không treo | |

## Mẫu phản hồi gửi lại
Với mỗi mục ❌ hoặc chưa hài lòng, ghi theo mẫu:
```
[Mã test] (vd D1) — File: <tên/loại file> — Hiện tượng: <mô tả> —
Kỳ vọng: <bạn muốn gì> — Mức ưu tiên: cao/vừa/thấp
```
Ví dụ:
```
B5 — File: hop_dong.pdf — Hiện tượng: chữ mờ hơn Foxit khi zoom 100% —
Kỳ vọng: nét như Foxit — Ưu tiên: vừa
```

> Gửi danh sách phản hồi này lại, tôi sẽ sửa hết các điểm chưa đạt của Phase 1 **trước khi** bắt đầu Phase 2 (Annotate).
