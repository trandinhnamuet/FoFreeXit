# 20 — Checklist test thủ công Phase 7 (OCR & Convert)

Chuẩn bị trên Windows:
- **Tesseract**: cài bản UB Mannheim (github tesseract-ocr), tick thêm gói
  ngôn ngữ **Vietnamese**; hoặc đặt `FOFREEXIT_TESSERACT_PATH`.
- **LibreOffice** (cho Office↔PDF chất lượng cao): cài bản thường; hoặc đặt
  `FOFREEXIT_SOFFICE_PATH` trỏ tới soffice.exe.

## Chạy
```powershell
cd c:\Project\FoFreeXit\app\src-tauri
cargo run
```

## Bảng test
| # | Thao tác | Kỳ vọng | KQ |
|---|----------|---------|----|
| C1 | Bấm **🔁 Chuyển đổi** | Hiện thanh; nếu thiếu Tesseract/LibreOffice có ghi chú + nút tương ứng mờ | |
| C2 | Mở 1 PDF **scan** (hoặc ảnh chụp tài liệu in ra PDF) → **🔍 OCR** (Việt + Anh) → lưu | Chạy xong báo số từ; file mới nhìn Y HỆT bản gốc | |
| C3 | Trong file C2: Ctrl+F tìm 1 từ có trong ảnh; bôi chọn & copy đoạn chữ | Tìm THẤY, vị trí highlight đúng chỗ; copy ra đúng chữ (kể cả **tiếng Việt có dấu**) | |
| C4 | Mở file C2 ở Foxit/Adobe → search/copy | Hoạt động như C3 (lớp text ẩn chuẩn) | |
| C5 | **🖼 Xuất PNG** → chọn thư mục | Mỗi trang 1 file PNG rõ nét (150 DPI) | |
| C6 | **📄 Xuất TXT** | File .txt chứa đủ text các trang | |
| C7 | **📘 Xuất Word** (máy CÓ LibreOffice) → mở .docx bằng Word | Mở được; chữ + bố cục gần bản gốc; status ghi "LibreOffice" | |
| C8 | **📘 Xuất Word** (máy KHÔNG LibreOffice) → mở .docx bằng Word | Mở được; đủ text đúng thứ tự, cỡ chữ tương đối, ngắt trang đúng; status ghi "bộ chuyển cơ bản" | |
| C9 | **📥 Office → PDF**: chọn 1 file .docx → thư mục ra | PDF tạo ra mở được trong app, nội dung đúng | |
| C10 | OCR file 20+ trang | Chạy hết (có thể vài phút), không crash, số từ hợp lý | |

## Mẫu phản hồi
```
[Mã] (vd C3) — Phần mềm mở: <Foxit/Adobe/Chrome/Word> — Hiện tượng: <mô tả> —
Kỳ vọng: <...> — Ưu tiên: cao/vừa/thấp
```

> Giới hạn đã biết: OCR chưa tiền xử lý scan nghiêng/mờ; PDF→Word bộ cơ bản
> không tái tạo cột/bảng/ảnh (cài LibreOffice để có layout tốt); PDF→Excel
> chưa làm — xem `docs/19-phase7-summary.md` mục 6.
