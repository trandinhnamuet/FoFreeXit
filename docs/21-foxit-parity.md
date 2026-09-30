# 21 — Đối chiếu tính năng với Foxit PDF Editor (đợt hoàn thiện 2026-09-30)

> Đối chiếu từng nhóm tính năng Foxit PDF Editor / Editor+ với FoFreeXit sau đợt
> làm song song 10 mảng (nhánh `feat/foxit-parity`). Cột "Vào ở đâu" là đường đi
> trong giao diện. Test engine: `scripts/docker-test/run.ps1` (Linux Docker —
> máy dev Windows bị Smart App Control chặn build script của cargo).

## 1. Bảng đối chiếu

| # | Nhóm Foxit | FoFreeXit | Vào ở đâu | Module |
|---|---|---|---|---|
| 1 | Sửa chữ/font/cỡ/màu, thêm/xoá chữ, đoạn văn | ✅ (có từ Phase 4) + căn trái/giữa/phải | Chỉnh sửa | `edit.rs` |
| 1 | Sửa ảnh: thêm/xoá/di chuyển/đổi cỡ/**xoay/lật/cắt**/thay/**trích ảnh**/độ mờ | ✅ | Chỉnh sửa → Ảnh & đối tượng | `editobj.rs` |
| 1 | Đối tượng đồ hoạ: **thêm hình** (chữ nhật, bo góc, elip, đường, mũi tên), viền/nền/nét/độ đục, **sắp lớp, căn chỉnh, phân bố**, chọn nhiều, copy/dán/nhân bản | ✅ | Chỉnh sửa | `editobj.rs`, `features/editx.js` |
| 2 | Tạo PDF từ Word/Excel/PPT | ✅ (cần LibreOffice) | Chuyển đổi → Tạo PDF | `convert.rs` |
| 2 | Từ ảnh (JPG/PNG/BMP/GIF/WebP/TIFF nhiều trang), văn bản, **trang web/HTML** (Edge/Chrome headless), **máy quét (WIA)**, clipboard, trang trắng, **gộp tệp hỗn hợp** | ✅ | Chuyển đổi → Tạo PDF / Tệp → Mới | `create.rs` |
| 2 | Foxit PDF Printer (máy in ảo) | ❌ cần driver máy in hệ thống — ngoài phạm vi | — | — |
| 2 | PDF/A | ⚠️ chưa tạo/kiểm tra chuẩn PDF/A | — | — |
| 3 | PDF → Word / **Excel** / **PowerPoint** / PNG / **JPEG** / **TIFF** / **HTML** / **RTF** / TXT | ✅ (writer tự viết, LibreOffice nếu có) | Chuyển đổi → Xuất | `export.rs`, `convert.rs` |
| 4 | OCR (Việt + Anh), **OCR ảnh trực tiếp**, phạm vi trang, **bỏ qua trang đã có chữ**, **nắn nghiêng + nhị phân hoá** | ✅ (Tesseract) | Chuyển đổi → OCR | `ocr.rs` |
| 5 | Chèn/xoá/xoay/trích/thay/đảo trang, **nhân bản**, **đảo thứ tự**, trang trắng theo khổ | ✅ | Trang | `organize.rs` |
| 5 | Gộp / tách: số trang, **dung lượng, bookmark, dải trang** | ✅ | Trang → Tách | `split.rs` |
| 6 | **Bookmark: tạo/sửa/kéo-thả/lồng cấp, tự tạo từ tiêu đề** | ✅ | Thanh bên → Mục lục | `bookmarks.rs` |
| 6 | **Liên kết**: bấm được trong viewer, tạo liên kết tới trang/URL | ✅ | Chỉnh sửa / Chú thích → Liên kết | `links.rs` |
| 6 | Đầu/chân trang, **số trang nhiều định dạng**, ngày | ✅ | Trang → Đầu & chân trang | `watermark.rs` |
| 7 | Highlight/gạch chân/gạch ngang/text box/sticky note | ✅ | Chú thích | `annot.rs` |
| 7 | **Bút vẽ tay, đường, mũi tên, hình chữ nhật, elip, đa giác, đường gấp khúc, đám mây, con dấu (preset/ngày/ảnh)** | ✅ | Chú thích | `annot_ext.rs` |
| 7 | **Sửa/di chuyển/xoá chú thích có sẵn trong file, trả lời, tác giả/ngày** | ✅ | Thanh bên → Chú thích | `annot_ext.rs` |
| 8 | Form: **điền trực tiếp trên trang**, text/checkbox/**radio/combo/list/nút bấm/ngày/chữ ký**, thuộc tính, sửa trường | ✅ | Biểu mẫu | `formx.rs` |
| 8 | **Nhận diện trường tự động** | ✅ | Biểu mẫu → Nhận diện trường | `formrecog.rs` |
| 8 | FDF/**XFDF**/**XML**/CSV/TXT, reset, flatten | ✅ | Biểu mẫu → Nhập/Xuất | `formdata.rs` |
| 9 | **Chữ ký tay (vẽ/gõ/ảnh), chữ viết tắt, ✓ ✗ ●, ngày** | ✅ | Điền & Ký | `fillsign.rs` |
| 9 | Chữ ký số PKCS#7, **PFX/P12**, **chữ ký hiển thị**, ký vào ô có sẵn, **ký nhiều lần (incremental)**, xác thực + phát hiện sửa sau ký, chi tiết chứng chỉ | ✅ | Bảo vệ → Ký số | `sign.rs` |
| 9 | Foxit eSign (gửi người khác ký, theo dõi) | ❌ cần dịch vụ đám mây | — | — |
| 10 | Mật khẩu mở/quyền, AES-256 | ✅ | Bảo vệ | `qpdf.rs` |
| 11 | Redaction thật, **Tìm và che** (từ, regex, mẫu CCCD/CMND/MST/điện thoại/email/thẻ Luhn/IBAN/ngày/URL/IP), **che trang**, **giao diện che** (màu, chữ phủ) | ✅ | Bảo vệ | `redact.rs`, `redact_search.rs` |
| 11 | **AI Smart Redact** | ✅ (qua Claude API, người dùng duyệt trước khi áp dụng) | Trợ lý AI → Bôi đen thông minh | `cmd_ai.rs` |
| 12 | **Làm sạch tài liệu** (metadata, chú thích, form, đính kèm, JavaScript, liên kết, lớp ẩn, bookmark, chữ ẩn, thumbnail, PieceInfo) | ✅ | Bảo vệ → Làm sạch | `sanitize.rs` |
| 13 | **Bates numbering** (nhiều tệp, nối tiếp) + gỡ | ✅ | Trang → Bates | `watermark.rs` |
| 14 | **So sánh 2 PDF** (chữ + đồ hoạ, xem song song, danh sách khác biệt, xuất báo cáo) | ✅ | Công cụ → So sánh | `compare.rs` |
| 15 | **AI: tóm tắt, hỏi đáp có trích trang, trích xuất, dịch, viết lại/sửa chính tả, nhiều tài liệu, lệnh ngôn ngữ tự nhiên** | ✅ (Claude API, key của người dùng) | Trợ lý AI | `cmd_ai.rs`, `features/ai.js` |
| 15 | **Đọc to (Text-to-Speech)** | ✅ (giọng Windows, không cần key) | Trợ lý AI / Công cụ | `features/ai.js` |
| 16 | Chèn audio/video | ⚠️ chỉ qua tệp đính kèm; chưa có RichMedia/Screen annotation | Thanh bên → Đính kèm | `attachments.rs` |
| 17 | Chia sẻ & cộng tác trực tuyến | ❌ cần dịch vụ đám mây | — | — |
| 18 | **Trợ năng**: kiểm tra đầy đủ/nhanh (~34 mục), sửa tự động, văn bản thay thế | ✅ (chưa có Autotag / thứ tự đọc) | Trợ năng | `a11y.rs` |
| 19 | DMS (quản lý tài liệu, version, check-in/out) | ❌ hướng doanh nghiệp/máy chủ | — | — |
| 20 | **Xử lý hàng loạt (Action Wizard)**: OCR, hình mờ, đầu/chân trang, tối ưu, metadata, flatten, xoay, mã hoá, chuyển đổi | ✅ | Công cụ → Xử lý hàng loạt | `batch.rs` |
| 21 | Hình mờ chữ/**ảnh/trang PDF**, **nền trang**, cập nhật/**gỡ** | ✅ | Trang → Hình mờ / Nền | `watermark.rs` |
| 22 | PDF Portfolio | ⚠️ tệp đính kèm (thêm/mở/lưu/xoá/mô tả); chưa có giao diện Portfolio | Thanh bên → Đính kèm | `attachments.rs` |
| 23 | Print production / Preflight | ⚠️ chưa có | — | — |
| 24 | AI tạo ảnh | ❌ chưa làm | — | — |
| — | **In** (phạm vi, số bản, tỉ lệ, kèm chú thích/form, thang xám) | ✅ (in dạng ảnh qua hộp thoại in Windows) | Tệp → In / Ctrl+P | `features/signx.js` |
| — | **Thuộc tính tài liệu** (sửa Title/Author/…, font, bảo mật, chế độ xem ban đầu) | ✅ | Tệp → Thuộc tính | `docprops.rs` |

✅ = đã làm, có test engine · ⚠️ = một phần · ❌ = chưa/ngoài phạm vi (lý do ghi kèm)

## 2. Giao diện

- 12 tab ribbon: Trang chủ · Chỉnh sửa · Chú thích · Trang · Biểu mẫu · Điền & Ký ·
  Chuyển đổi · Bảo vệ · Trợ năng · Công cụ · Trợ lý AI · Trợ giúp.
- Ribbon tự co gọn 2 bậc khi cửa sổ hẹp (`features/ribbon-fit.js`): thu nhỏ nút →
  chỉ icon (nhãn ở tooltip). Hàng tab vừa khít 1366px cả tiếng Việt; ô tìm kiếm
  thu về icon ở ≤1520px.
- Mỗi tính năng lớn một tệp `app/src/features/<tên>.js` + từ điển
  `app/src/i18n/part-<tên>.js` (VI/EN đầy đủ, `check-i18n` PASS).

## 3. Kiểm thử

- Engine: 24 bộ test integration + unit, tất cả xanh trên Linux, trừ các test phụ
  thuộc môi trường đã biết từ trước: `qpdf_safety` (bản qpdf Linux),
  `vietnamese_on_base14_uses_matched_family` và
  `rich_seg_bold_italic_override_uses_variant_font` (font Windows).
- App Tauri: `cargo test` (13 unit test AI) + `cargo check` cả target Linux và
  `x86_64-pc-windows-gnu` (biên dịch được code riêng Windows: WIA, mở tệp…).
- UI: `check-i18n`, `check-ids`, `node --check` PASS; chụp màn hình các tab bằng
  Chrome headless với backend giả lập.
- **Chưa làm:** chạy thử toàn bộ trên app thật (GUI) — cần kiểm tay theo checklist
  của từng phase trên máy Windows.

## 4. Còn thiếu so với Foxit (kế hoạch)

1. PDF/A (tạo + kiểm tra), Preflight.
2. Trợ năng: Autotag, công cụ thứ tự đọc.
3. Chữ ký số: kiểm tra chuỗi CA / kho chứng chỉ Windows, OCSP/CRL, timestamp/LTV, ECDSA, DocMDP.
4. Multimedia (RichMedia/Screen), giao diện Portfolio.
5. So sánh: khác biệt định dạng chữ, chú thích; trang xoay.
6. AI: "Thay trong tài liệu" cho kết quả viết lại; lưu API key bằng DPAPI; tạo PDF bản dịch.
7. Đám mây (eSign, cộng tác, DMS), máy in ảo, AI tạo ảnh — ngoài phạm vi ứng dụng desktop offline.
