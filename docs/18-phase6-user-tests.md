# 18 — Checklist test thủ công Phase 6 (Form)

Quan trọng nhất: **điền form lưu giữ dữ liệu** (F3) và **file mở đúng ở
Foxit/Adobe** (F4, F8), vì đây là dữ liệu người dùng nhập.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| F1 | Mở 1 PDF, bấm **📝 Form** | Hiện thanh Form; nút “Điền form (n)” cho biết số field | |
| F2 | Bấm **➕ Thêm field** → tạo 1 Text “hoTen” trang 1 → lưu | File mới mở lên, thanh Form đếm tăng | |
| F3 | Bấm **📋 Điền form** → nhập “Nguyễn Văn A” vào hoTen → Lưu | File mới; mở lại thấy giá trị đã điền | |
| F4 | Mở file F3 ở **Foxit/Adobe** | Field hoTen hiển thị đúng “Nguyễn Văn A” (tiếng Việt đúng dấu) | |
| F5 | Thêm 1 **Checkbox** và 1 **Combo** (Nam/Nữ/Khác) → Điền form: tick checkbox, chọn combo → Lưu | Mở lại: checkbox đang tick, combo đúng lựa chọn | |
| F6 | **⬆ Xuất FDF** → mở file .fdf bằng Notepad | Có tên field + giá trị (tiếng Việt dạng `<FEFF...>`) | |
| F7 | Trên 1 file form rỗng: **⬇ Nhập FDF** (chọn file F6) → Lưu | Các field được điền lại từ FDF | |
| F8 | **🧷 Flatten** file đã điền → mở ở Foxit | Giá trị vẫn hiển thị nhưng KHÔNG còn là field bấm/sửa được | |
| F9 | **⬆ Xuất CSV** → mở bằng Excel | 2 cột name,value đúng dữ liệu | |
| F10 | Mở form PDF THẬT (mẫu Acrobat) → **📋 Điền form** | Liệt kê đúng các field có sẵn, điền & lưu giữ được | |

## Mẫu phản hồi
```
[Mã] (vd F4) — Phần mềm mở: <Foxit/Adobe/Chrome> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Giới hạn đã biết: tạo field mới hỗ trợ text/checkbox/combo (radio-group/
> list/pushbutton chưa tạo — đọc/điền thì được); dựa vào NeedAppearances để
> viewer dựng hình field (Flatten cho bản “cứng”); mới FDF+CSV, chưa XFDF —
> xem `docs/17-phase6-summary.md` mục 6.
