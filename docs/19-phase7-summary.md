# 19 — Tổng kết Phase 7 (OCR & Convert)

> Trạng thái: **HOÀN TẤT lõi.** OCR Tesseract (Việt + Anh) tạo lớp text ẨN
> khớp toạ độ lên trang gốc; PDF→PNG/TXT/DOCX; Office↔PDF qua LibreOffice.
> 6 test mới, tổng engine **73/73 xanh** (ngoài 2 fixture qpdf Linux).

## 1. Khảo sát Foxit (chuẩn nghiệm thu)
- **OCR (Recognize Text)**: chọn ngôn ngữ → chạy trên tài liệu scan → file
  thành searchable/copy được, hình ảnh gốc GIỮ NGUYÊN (text ẩn dưới ảnh).
- **Convert**: PDF→ảnh/Word/Excel/text; Office→PDF.

## 2. Kiến trúc — sidecar CLI như qpdf
- **Tesseract** (OCR) và **LibreOffice headless** (Office↔PDF) là công cụ
  ngoài, tìm qua env (`FOFREEXIT_TESSERACT_PATH`/`FOFREEXIT_SOFFICE_PATH`) →
  PATH. Không có thì các tính năng khác vẫn chạy, nút liên quan báo rõ.
- PDF→PNG/TXT/DOCX cơ bản tự làm bằng PDFium + code riêng — không phụ thuộc gì.

## 3. Đã làm — Engine

### `ocr.rs`
- `ocr_page_words`: render trang **300 DPI** → `tesseract ... tsv` → parse
  bảng từ (bbox pixel + confidence, lọc <30) → quy đổi pixel→điểm PDF.
- `ocr_add_text_layer`: OCR các trang rồi thêm text object **render mode
  Invisible** đúng khung từng từ lên CHÍNH trang gốc (không đổi hình ảnh) —
  đúng cơ chế searchable PDF chuẩn. Font Unicode (đủ dấu tiếng Việt) nhúng 1
  lần. Ngôn ngữ `vie+eng` mặc định.

### `convert.rs`
- `export_images(dpi)`: mỗi trang 1 PNG `<tên>-pN.png`.
- `export_text`: toàn bộ text các trang → .txt.
- `export_docx` (tự viết, "đủ dùng"): gom char boxes thành DÒNG thị giác
  (cùng heuristic UI edit) → mỗi dòng 1 đoạn Word với **cỡ chữ xấp xỉ** +
  ngắt trang giữa các trang; DOCX = zip tự ghi (method STORE, CRC-32 tự tính,
  không thêm dependency) — Word/LibreOffice/unzip đọc chuẩn.
- `office_to_pdf` / `pdf_to_docx_via_soffice`: LibreOffice headless với
  profile riêng (`-env:UserInstallation`) để không đụng LibreOffice đang mở;
  PDF→DOCX dùng `--infilter=writer_pdf_import` (layout tốt hơn bản tự viết).

## 4. Tauri + UI (thanh "🔁 Chuyển đổi")
- Commands: `ocr_run` (chọn ngôn ngữ), `convert_images`, `convert_txt`,
  `convert_docx` (**tự chọn engine**: LibreOffice nếu có → fallback bộ cơ bản,
  báo rõ đã dùng gì), `office_convert`, `convert_tools_status` (UI disable
  nút + hiện thiếu công cụ nào).
- Nút: 🔍 OCR (select Việt+Anh/Việt/Anh), 🖼 Xuất PNG (150 DPI), 📄 Xuất TXT,
  📘 Xuất Word, 📥 Office→PDF (mở luôn PDF kết quả).

## 5. Test (6 mới → engine 73/73)
- `ocr_makes_scanned_pdf_searchable_english` — dựng PDF-scan giả lập THẬT
  (xoá hết text, chỉ còn ảnh render — extract rỗng như scan) → OCR → từ tìm
  được qua extract + search, **toạ độ lớp ẩn khớp vùng chữ** trên ảnh.
- `ocr_vietnamese_diacritics` — "Việt Nam đất nước" nhận dạng ĐÚNG DẤU.
- `export_images_per_page` (3 trang → 3 PNG đúng cỡ); `export_text_all_pages`.
- `export_docx_basic_layout` — docx là zip chuẩn (unzip hệ thống giải được),
  document.xml chứa text + ngắt trang.
- `libreoffice_round_trip_when_available` — PDF→DOCX (LibreOffice) và
  DOCX→PDF: PDF kết quả mở được, nội dung giữ nguyên. Tự bỏ qua nếu máy
  không có soffice.

## 6. Giới hạn (v-sau, không chặn dùng)
- OCR: chưa tiền xử lý ảnh (deskew/denoise) cho scan nghiêng/mờ; chưa chọn
  vùng OCR; lớp ẩn đặt theo khung TỪ (chuẩn phổ biến), chưa scale ngang từng
  glyph (Tz) nên bôi chọn có thể lệch nhẹ ở từ rất dài.
- PDF→DOCX bản tự viết: giữ text + thứ tự đọc + cỡ chữ tương đối; KHÔNG tái
  tạo cột/bảng/ảnh (LibreOffice lo phần đó khi máy có cài). PDF→Excel chưa làm.
- Windows: cần cài Tesseract (UB Mannheim build, tick gói Vietnamese) và/hoặc
  LibreOffice; ghi trong docs/05 + checklist docs/20.
