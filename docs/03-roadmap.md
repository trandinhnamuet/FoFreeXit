# 03 — Lộ trình dự án (Roadmap)

## Mục tiêu tổng (North Star)
Một ứng dụng desktop có thể **xem, chú thích, tổ chức trang, và chỉnh sửa text/ảnh trực tiếp** trên PDF, kèm bảo mật cơ bản — đủ để một người dùng Foxit phổ thông **bỏ Foxit** cho công việc hằng ngày.

## Phương pháp làm việc
- **Engine-first, UI-second** trong mỗi tính năng: làm được ở tầng core + test CLI trước, rồi mới gắn UI.
- Mỗi phase kết thúc bằng **bản chạy được (demo) + test tự động + cập nhật docs**.
- **Bộ PDF mẫu** (test corpus) tích luỹ dần: file thường, file scan, file mã hoá, file hỏng, CJK/Việt/RTL, form, file nghìn trang.
- Quy ước: nhánh/commit theo phase; mỗi tính năng có "DoD" (Definition of Done) checklist.
- **Không** chuyển phase khi DoD chưa đạt và test chưa xanh.

---

## Phase 0 — Nghiên cứu & Kế hoạch ✅ HOÀN TẤT
**DoD:** docs 01–03 hoàn tất; chốt ngôn ngữ/framework; dựng skeleton repo.

## Phase 1 — Nền tảng & Viewer (MVP xem được) ✅ HOÀN TẤT
**Mục tiêu:** Mở và hiển thị PDF chính xác, mượt.
- Tích hợp PDFium (prebuilt) qua FFI; dựng Core API tối thiểu (`open`, `pageCount`, `renderPage`).
- Viewer: scroll liên tục, zoom/fit, thumbnails, outline/bookmarks, **tìm kiếm text**, chọn & copy text.
- Mở file lớn (lazy render + cache tile).
**DoD:** mở 20 file mẫu (gồm file 1000+ trang, CJK, Việt) không crash; render khớp mắt thường với Chrome; search & copy hoạt động; có test render-hash cho trang mẫu.

## Phase 2 — Chú thích (Annotate) ✅ HOÀN TẤT (lõi) — xem [docs/08-phase2-summary.md](08-phase2-summary.md)
**Mục tiêu:** Bộ comment dùng hằng ngày.
- Highlight/underline/strikethrough, sticky note, text box, free draw, shapes, stamp.
- Lưu annotation vào PDF (annotation dict), import/export **XFDF**.
- Hiển thị & sửa/xoá annotation; danh sách comment ở sidebar.
**DoD:** tạo/sửa/xoá mọi loại trên; lưu & mở lại giữ nguyên; Acrobat/Foxit đọc được annotation ta ghi; export/import XFDF round-trip.

## Phase 3 — Tổ chức trang (Organize) + Lưu file vững ✅ HOÀN TẤT (lõi, 36/36 test) — xem [docs/10-phase3-summary.md](10-phase3-summary.md)
**Mục tiêu:** Quản lý trang & ghi file an toàn.
- Chèn/xoá/xoay/trích/thay/đảo trang; **merge & split**; crop; đánh số; header/footer; watermark/background.
- Engine ghi file: incremental update **và** full rewrite (qua QPDF); giữ không hỏng file; **undo/redo** toàn cục.
**DoD:** mọi thao tác trang round-trip đúng; merge 50 file OK; ghi không làm hỏng file mã hoá; undo/redo ổn định; test so khớp cấu trúc trang.

## Phase 4 — ⭐ Chỉnh sửa nội dung (Edit) — TÍNH NĂNG LÕI ✅ Iteration 1 HOÀN TẤT (43/43 test) — xem [docs/12-phase4-summary.md](12-phase4-summary.md)
**Mục tiêu:** Sửa text & object trực tiếp — moat chính.
- **Page Object Model**: parse content stream → cây object (text/path/image/xobject).
- Sửa **ảnh/object**: di chuyển, resize, xoá, thêm ảnh (dễ hơn → làm trước).
- Sửa **text**: gom glyph → text run/line/paragraph (heuristic theo baseline & spacing); edit inline; đo lại width theo font (FreeType/HarfBuzz); **reflow** trong text box; xử lý font nhúng/subset + font substitution; hỗ trợ **tiếng Việt** (tổ hợp dấu) sớm, CJK/RTL sau.
- Thêm text box mới, đổi font/size/màu.
**DoD:** sửa được text 1 dòng & 1 đoạn trên ≥80% file mẫu thường; thêm/xoá/di chuyển ảnh; lưu mở lại đúng; tiếng Việt hiển thị & sửa đúng dấu; có test corpus chuyên cho edit.

## Phase 5 — Bảo mật & Chữ ký ✅ Iteration 1 HOÀN TẤT (mã hoá+quyền, gỡ mật khẩu, redaction thật, xoá metadata — xem [docs/15-phase5-summary.md](15-phase5-summary.md)) · Iteration 2: chữ ký số
**Mục tiêu:** Tính năng đáng tiền cho doanh nghiệp.
- Mật khẩu mở/permission; mã hoá **AES-256**; xoá mật khẩu.
- **Redaction thật** (xoá nội dung + ảnh + metadata, không chỉ vẽ đè) + kiểm chứng đã xoá.
- Xoá metadata/ẩn; **chữ ký số** (PAdES, byte range, incremental update giữ chữ ký).
**DoD:** mã hoá/giải mã round-trip; redaction qua kiểm chứng không còn text bị bôi; ký số được Acrobat xác thực hợp lệ.

## Phase 6 — Form (AcroForm)
**Mục tiêu:** Điền & tạo form.
- Điền form có sẵn, lưu; flatten form.
- Tạo field: text/checkbox/radio/dropdown/button; import/export dữ liệu (FDF/CSV).
**DoD:** điền & lưu giữ dữ liệu; tạo form mới mở được ở Acrobat; export/import round-trip.

## Phase 7 — OCR & Convert
**Mục tiêu:** Hai tính năng "đáng tiền" còn lại, qua dự án mở.
- **OCR**: Tesseract + tiền xử lý ảnh + layout → tạo searchable PDF (lớp text vô hình khớp toạ độ). Đa ngôn ngữ gồm tiếng Việt.
- **Convert**: PDF→ảnh (sẵn từ PDFium); Office↔PDF qua **LibreOffice headless**; PDF→Word/Excel (giữ layout — bài toán khó, làm bản "đủ dùng" trước).
**DoD:** OCR scan tạo PDF search được (Việt + Anh); convert Office↔PDF ổn; PDF→Word giữ được text & bố cục cơ bản.

## Phase 8 — Hoàn thiện & Phát hành
- So sánh 2 PDF (compare); in ấn đúng; preferences; đa ngôn ngữ UI (Việt/Anh); accessibility.
- Installer (Windows MSI/winget; sau đó mac/Linux); auto-update; crash reporting.
- Tối ưu hiệu năng & bộ nhớ; hardening bảo mật (fuzzing parser).
**DoD:** cài đặt sạch trên máy mới; vượt bộ test hồi quy đầy đủ; tài liệu người dùng.

## Phase 9 (tuỳ chọn) — AI
- Tóm tắt, hỏi-đáp tài liệu, dịch (dùng Claude API / model cục bộ). Làm sau cùng vì không phải lý do chính thay thế Foxit.

---

## Thứ tự ưu tiên giá trị (nếu phải cắt giảm)
**1→2→3→4** là "core sản phẩm thay Foxit cho cá nhân". **5,6** thêm giá trị doanh nghiệp. **7** ngang hàng giá trị nhưng phụ thuộc dự án ngoài. **8** bắt buộc để phát hành. **9** là bonus.

## Rủi ro & giảm thiểu
| Rủi ro | Giảm thiểu |
|---|---|
| Edit text quá khó (Phase 4) | Làm object/ảnh trước; text bắt đầu từ ca đơn giản; tận dụng PDFium edit API + PDFBox tham khảo |
| Build/FFI PDFium phức tạp | Dùng binary prebuilt + binding có sẵn |
| Convert PDF→Word kém | Hạ kỳ vọng: "đủ dùng" trước; dựa LibreOffice |
| File lạ làm crash | Test corpus file hỏng + fuzzing; chọn ngôn ngữ an toàn bộ nhớ |
| Phạm vi phình to | DoD nghiêm ngặt; không nhảy phase khi chưa xanh |

## Bộ chỉ số thành công sản phẩm
- Mở đúng ≥99% file PDF đời thực trong corpus.
- Người dùng Foxit phổ thông làm được: xem, comment, sắp trang, sửa text/ảnh, đặt mật khẩu — **không cần Foxit**.
- Không thua Foxit rõ rệt về tốc độ mở & render.
