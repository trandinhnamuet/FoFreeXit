# 01 — Nghiên cứu: Tính năng, Khó khăn, Thuật toán, Thư viện

## A. Tính năng của Foxit PDF Editor

Foxit có 3 dòng: **PDF Editor**, **PDF Editor Pro**, **PDF Editor Pro+** (subscription, kèm AI/cloud).
Giá tham khảo (2025–2026): subscription từ ~$11.99/tháng; **lifetime/perpetual ~$159–$210**; Pro+ chỉ bán subscription. Enterprise báo giá riêng.

### Nhóm tính năng (theo mức độ quan trọng với người dùng)

#### 1. Xem & điều hướng (View) — *table stakes, miễn phí ở mọi reader*
- Render PDF chính xác, mượt; zoom, fit, thumbnails, bookmarks/outline, search full-text.
- Continuous scroll, two-page, reading mode, dark mode.
- Mở file lớn nhanh (lazy load từng trang).

#### 2. Chú thích (Annotate / Comment) — *quan trọng, dùng hằng ngày*
- Highlight, underline, strikethrough, squiggly.
- Sticky notes, text box, callout, stamp, drawing (pencil/shapes), measure.
- Reply/threads cho comment, import/export annotation (FDF/XFDF).

#### 3. **Chỉnh sửa nội dung (Edit) — TÍNH NĂNG "ĐÁNG TIỀN" SỐ 1** ⭐
- **Sửa text trực tiếp** trên trang (reflow trong đoạn, đổi font/size/màu).
- Thêm/xoá/di chuyển/đổi kích thước **ảnh** và **đối tượng** (object).
- Edit cả paragraph, link/unlink text box, OCR rồi sửa text trên ảnh scan.
- Đây là rào cản kỹ thuật lớn nhất và là lý do chính người dùng trả tiền — các reader miễn phí (đa số) **không** làm được.

#### 4. Tổ chức trang (Organize) — *quan trọng, đáng tiền*
- Chèn/xoá/xoay/trích/thay/đảo trang, **merge & split**, crop, đánh số, header/footer, watermark, background.
- Sắp xếp kéo-thả thumbnail.

#### 5. Chuyển đổi (Convert) — *rất đáng tiền* ⭐
- PDF ↔ Word/Excel/PowerPoint, PDF → ảnh, ảnh/Office → PDF.
- Chất lượng chuyển đổi (giữ layout) là điểm bán hàng then chốt.

#### 6. **OCR** — *rất đáng tiền* ⭐
- Nhận dạng text trên scan (đa ngôn ngữ), tạo "searchable PDF".
- Foxit 2025.2 quảng cáo OCR vượt Adobe ở chữ số/checkbox/handwriting.

#### 7. Form (AcroForm / XFA) — *đáng tiền với doanh nghiệp*
- Điền form, **tạo form** (text field, checkbox, radio, dropdown, button), nhận dạng field tự động, import/export dữ liệu form.

#### 8. Bảo mật & chữ ký — *đáng tiền với doanh nghiệp* ⭐
- Mật khẩu (mở/permission), **mã hoá AES**, redaction (bôi đen vĩnh viễn), xoá metadata ẩn.
- **Chữ ký số** (digital signature/PKI), chứng thực, timestamp; **eSign** (cloud).

#### 9. So sánh & cộng tác
- Compare 2 phiên bản PDF, shared review, cloud collaboration (Pro+).

#### 10. AI (Pro+) — *xu hướng mới*
- Tóm tắt, hỏi-đáp tài liệu, dịch, chat với PDF.

### ➜ Kết luận: Tính năng khiến người dùng phải mua license
Theo thứ tự "moat" (khó làm + người dùng sẵn sàng trả tiền):
1. **Edit text/object trực tiếp** (rào cản kỹ thuật cao nhất).
2. **Convert PDF→Office giữ layout** (Word/Excel).
3. **OCR** chất lượng cao, đa ngôn ngữ.
4. **Bảo mật**: redaction thật, mã hoá, chữ ký số.
5. **Tạo & xử lý form** (AcroForm).
6. **Organize**: merge/split/crop hàng loạt.

> Chiến lược FoFreeXit: chiếm các tính năng (1), (4), (6), (organize) trước — vốn có thể tự làm tốt trên nền engine mở. (2) Convert và (3) OCR sẽ tận dụng dự án mở khác (LibreOffice headless, Tesseract). AI để sau cùng.

---

## B. Khó khăn kỹ thuật (xếp theo độ khó)

### B1. Bản chất định dạng PDF
- PDF **không phải định dạng để chỉnh sửa**: nó mô tả "vẽ gì ở toạ độ nào", không lưu cấu trúc đoạn/dòng/bảng như Word. Text là chuỗi lệnh `Tj/TJ` đặt glyph theo toạ độ → **không có khái niệm "đoạn văn"**.
- Đặc tả PDF (ISO 32000) cực lớn; vô số file "hợp lệ một phần", file hỏng, file sinh bởi công cụ lạ → cần **độ bền (robustness)** rất cao.

### B2. Sửa text trực tiếp (khó nhất) ⭐
- Phải **tái dựng dòng/đoạn** từ các glyph rời rạc (gom theo toạ độ, baseline, khoảng cách) → suy ra "text run".
- Khi sửa: cần **đo lại chiều rộng glyph** theo font thật, **reflow** phần còn lại của dòng/đoạn, xử lý ligature, kerning, justification.
- **Font**: file có thể nhúng font subset (chỉ chứa glyph đã dùng) → muốn gõ ký tự mới phải có glyph đó; nếu không, phải **substitute font** giống nhất rồi nhúng/subset lại. Encoding phức tạp (CID, CMap, Type0, Type3).
- Văn bản phức tạp: RTL (Ả Rập/Do Thái), chữ Đông Á (CJK), **tiếng Việt dấu** (tổ hợp dấu), shaping (HarfBuzz).

### B3. Render chính xác
- Render giống Acrobat: blend modes, transparency groups, soft mask, shading/gradient, pattern, overprint, ICC color management, CMYK.
- Hiệu năng: file hàng nghìn trang, ảnh độ phân giải cao → cần cache, tile, đa luồng, GPU (tuỳ chọn).

### B4. OCR
- Tích hợp engine (Tesseract), tiền xử lý ảnh (deskew, denoise, binarize), bố cục (layout analysis), khớp toạ độ text vào trang để tạo lớp text vô hình → searchable PDF.

### B5. Convert PDF → Office
- "Bài toán ngược": từ glyph dựng lại đoạn/bảng/cột/heading của Word. **Rất khó giữ layout**. Đây là nơi cả Adobe lẫn Foxit đều chưa hoàn hảo.

### B6. Bảo mật làm đúng
- **Redaction**: phải **xoá thật** nội dung khỏi content stream + metadata + ảnh, không chỉ vẽ hộp đen lên trên (lỗi này từng làm lộ tài liệu chính phủ).
- Mã hoá RC4/AES, permission, public-key security handler; **chữ ký số** đúng chuẩn PAdES (byte range, /ByteRange, incremental update không phá chữ ký cũ).

### B7. Ghi file an toàn
- **Incremental update** (ghi thêm cuối file, giữ chữ ký) vs **full rewrite** (gọn, tối ưu). Cần xử lý xref/xref-stream, object stream, linearization ("fast web view").
- Không làm hỏng file gốc; undo/redo cho thao tác sửa.

### B8. UX desktop chất lượng cao
- Tương tác mượt (chọn text qua nhiều trang, kéo object), đa nền tảng, in ấn đúng, accessibility, đa ngôn ngữ UI.

---

## C. Thuật toán & kiến trúc của Foxit / Adobe (học hỏi được gì)

> Foxit & Adobe là mã đóng; dưới đây là suy luận từ tài liệu SDK công khai, cấu trúc đặc tả PDF, và kiến trúc các engine mở tương đương. Mục tiêu: **học mô hình kiến trúc**, không sao chép.

### C1. Foxit
- **Core engine viết bằng C++**; cung cấp SDK với binding C++, C#, C, Java, Python. Kiến trúc tách lớp rõ: **Core (parser + render + edit) → API → UI**.
- Render engine riêng (không dùng PDFium công khai), tối ưu tốc độ & bộ nhớ — Foxit nổi tiếng "nhẹ và nhanh".
- Mô hình edit dựa trên **page object model**: trang = tập object (text/path/image/form-xobject), edit = thao tác trên object rồi serialize lại content stream.

### C2. Adobe Acrobat
- Lõi C/C++ lâu đời (PDF do Adobe phát minh). **PDFL (PDF Library)** + Acrobat core; tách "Cos layer" (Carousel Object System — model object thấp tầng: dict/array/stream) và "PD layer" (Page Description — trang, annotation, form).
- Bài học quan trọng: **mô hình 2 tầng**:
  - *Tầng object thấp (COS)*: đọc/ghi cấu trúc PDF (dictionary, stream, xref) — tương ứng QPDF/pikepdf.
  - *Tầng tài liệu cao (PD)*: trang, annotation, form, content stream parsed thành object — tương ứng PDFium/MuPDF.
- Liquid Mode (mobile reflow) của Adobe dùng ML để suy ra cấu trúc đọc → cho thấy hướng "tái dựng cấu trúc" là bài toán AI/heuristic.

### ➜ Bài học kiến trúc áp dụng cho FoFreeXit
1. **Tách 2 tầng**: `cos` (cấu trúc) và `pdmodel` (trang/object/annotation) — đúng như Adobe/PDFBox làm.
2. **Page Object Model** cho editing: parse content stream → cây object → sửa → serialize.
3. **Undo/redo** ở tầng model bằng command pattern.
4. Tách hẳn **engine** (không phụ thuộc UI) khỏi **UI**, để test tự động và tái dùng (CLI, server).

---

## D. Khảo sát thư viện PDF (mọi ngôn ngữ) & giấy phép

> Giấy phép là yếu tố sống còn: muốn phân phối tự do, **tránh AGPL** (MuPDF, iText) trừ khi chấp nhận copyleft hoặc mua license.

| Thư viện | Ngôn ngữ | Render | Edit text/obj | Cấu trúc/repair | Giấy phép | Ghi chú |
|----------|----------|:------:|:------:|:------:|----------|------|
| **PDFium** | C/C++ | ⭐ rất tốt | ✔ có API (`FPDFPageObj_*`, `FPDFText_*`) | một phần | **BSD-3 (permissive)** | Engine của Chrome; binary prebuilt sẵn (bblanchon/pdfium-binaries). Nền tảng lý tưởng. |
| **QPDF** | C++ | ✘ | ✘ (cấu trúc) | ⭐ xuất sắc | Apache-2.0 | Repair, linearize, encrypt, object stream. Bổ trợ cho PDFium. |
| pikepdf | Python | ✘ | ✘ | ⭐ (wrap QPDF) | MPL-2.0 | Bản Python của QPDF, tốt cho prototype. |
| MuPDF | C | ⭐ | ✔ | ✔ | **AGPL/commercial** | Mạnh nhưng copyleft → tránh cho bản đóng. |
| PyMuPDF | Python | ⭐ | ✔ | ✔ | **AGPL/commercial** | Tốt để prototype/nghiên cứu, không ship. |
| iText | Java/.NET | một phần | ✔ tạo | ✔ | **AGPL/commercial $$$** | Tránh trừ khi mua. |
| Apache PDFBox | Java | ✔ ok | ✔ | ✔ | **Apache-2.0** | Permissive, full-feature, tham khảo thuật toán rất tốt (mã đọc được). |
| PDF.js | JS | ✔ (web) | ✘ (gần như render-only) | một phần | Apache-2.0 | Của Mozilla; tốt cho web viewer, không edit sâu. |
| pdf-lib | JS/TS | ✘ | ✔ (tạo/sửa cơ bản) | ✔ | MIT | Thuần JS, tạo & sửa nhẹ. |
| pdfcpu | Go | ✘ | thao tác trang | ✔ | Apache-2.0 | CLI mạnh cho organize/encrypt. |
| lopdf / pdf-rs / printpdf | Rust | một phần | ✔ thấp tầng / tạo | ✔ | MIT/Apache | Ecosystem Rust đang lớn, chưa bằng PDFium. |
| Tesseract | C++ | — | — (OCR) | — | Apache-2.0 | Engine OCR mở tốt nhất. |
| HarfBuzz | C++ | — | text shaping | — | MIT | Bắt buộc cho shaping (Việt/CJK/RTL). |
| FreeType | C | — | font rasterize | — | FTL/GPL | Đọc & render font. |
| LibreOffice (headless) | C++ | — | convert | — | MPL-2.0 | Dùng `soffice --convert-to` cho PDF↔Office. |

### ➜ Tận dụng được gì
- **Nền engine**: PDFium (render + edit object) + QPDF (cấu trúc/encrypt/repair) — cả hai permissive → **không phải viết parser/renderer từ đầu**.
- **OCR**: Tesseract. **Shaping**: HarfBuzz. **Font**: FreeType. **Convert Office**: LibreOffice headless.
- **Tham khảo thuật toán** (đọc mã được): Apache PDFBox (Java, permissive) cho cách dựng text run, content stream, form; pdf.js cho rendering model.
- **Tránh**: MuPDF/PyMuPDF/iText cho bản phân phối (AGPL) — chỉ dùng để học/prototype.

## Nguồn tham khảo
- [Foxit vs Adobe 2025](https://pdfpro.com/blog/general/foxit-vs-adobe) · [Foxit pricing](https://www.trustradius.com/products/foxit-pdf-editor/pricing) · [Foxit Editor blog](https://www.foxit.com/blog/the-new-foxit-pdf-editor-and-pdf-editor/)
- [Foxit PDF SDK for C++](https://developers.foxit.com/developer-hub/document/developer-guide-pdf-sdk/)
- [PDFium fpdf_edit.h](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/public/fpdf_edit.h) · [PDFium LICENSE](https://pdfium.googlesource.com/pdfium/+/main/LICENSE)
- [pikepdf](https://github.com/pikepdf/pikepdf) · [Survey of Open-Source PDF Solutions (PDFA)](https://pdfa.org/wp-content/uploads/2021/06/Survey-of-OpenSource-Solutions.pdf)
- [Comparing open source PDF libraries (2025) — Joyfill](https://joyfill.io/blog/comparing-open-source-pdf-libraries-2025-edition) · [awesome-pdf](https://github.com/py-pdf/awesome-pdf)
- [MuPDF license](https://mupdf.readthedocs.io/en/1.27.0/license.html)
