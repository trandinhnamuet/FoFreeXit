# FoFreeXit

**Mục tiêu dự án:** Xây dựng một phần mềm chỉnh sửa PDF trên desktop có thể thay thế Foxit PDF Editor (và một phần Adobe Acrobat) cho các nhu cầu phổ biến — miễn phí / mã nguồn mở.

> Dự án được thực hiện qua nhiều session Claude Code. Token và thời gian không phải là ràng buộc. Ưu tiên số 1: **tạo ra được một sản phẩm dùng được, thay thế Foxit cho các tác vụ chính**.

## Tài liệu

| File | Nội dung |
|------|----------|
| [docs/01-research.md](docs/01-research.md) | Khảo sát tính năng Foxit, tính năng "đáng tiền", khó khăn kỹ thuật, thuật toán Foxit/Adobe, khảo sát thư viện PDF |
| [docs/02-tech-stack.md](docs/02-tech-stack.md) | Phân tích & lựa chọn ngôn ngữ / framework / engine |
| [docs/03-roadmap.md](docs/03-roadmap.md) | Lộ trình theo phase, mục tiêu, phương pháp, định nghĩa hoàn thành (DoD) |
| [docs/04-architecture.md](docs/04-architecture.md) | Kiến trúc kỹ thuật chi tiết (sẽ chi tiết hóa dần) |
| [docs/14-foxit-gap-analysis.md](docs/14-foxit-gap-analysis.md) | Đánh giá định hướng/công nghệ/tiến độ so với tham vọng thay Foxit; các khoảng cách còn lại + giải pháp |

## Trạng thái hiện tại

- [x] Phase 0 — Nghiên cứu & lập kế hoạch
- [x] **Đã chốt stack:** **Rust + Tauri** (App/UI) trên engine **PDFium + QPDF** (xem [docs/02-tech-stack.md](docs/02-tech-stack.md))
- [x] **Phase 1 — Engine core + Viewer** ✅ HOÀN TẤT (10/10 test xanh)
  - [x] Cargo workspace + app Tauri; engine render qua PDFium
  - [x] CLI `ff`: `info`/`render`/`pages`/`text`/`search`/`outline`
  - [x] Engine: render, `page_dims`, `extract_text`, `search`(+toạ độ), `outline`, `page_char_boxes`
  - [x] Viewer: mở file (hộp thoại), cuộn liên tục + lazy render (file 1000+ trang), zoom/Fit, thumbnails, outline, tìm kiếm + highlight, **chọn & copy text** (text-layer)
  - Tổng kết: [docs/06-phase1-summary.md](docs/06-phase1-summary.md) · **Checklist test cho bạn**: [docs/07-phase1-user-tests.md](docs/07-phase1-user-tests.md)
- [x] **Phase 2 — Annotate** ✅ lõi hoàn tất (13/13 test xanh)
  - [x] Engine: 6 loại annotation (Highlight/Underline/Strikeout/Square/FreeText/Note) + lưu file + đọc lại — round-trip test
  - [x] UI: thanh công cụ + vẽ bằng chuột + preview + chọn màu + tab Chú thích + lưu qua hộp thoại
  - [ ] Follow-up: Ink, sửa/di chuyển annotation đã lưu, XFDF — xem [docs/08-phase2-summary.md](docs/08-phase2-summary.md)
  - **Checklist test cho bạn**: [docs/09-phase2-user-tests.md](docs/09-phase2-user-tests.md)
- [x] **Phase 3 — Organize trang + lưu file vững** ✅ lõi hoàn tất (36/36 test xanh)
  - [x] Engine: chèn/xoá/xoay/trích/thay/đảo trang, merge/split, crop, watermark, header/footer + đánh số trang
  - [x] Lưu file vững: QPDF repair file hỏng + mã hoá lại an toàn (`qpdf.rs`)
  - [x] UI: chế độ "Tổ chức trang" (lưới + kéo-thả + multi-select), undo/redo toàn cục (chú thích + tổ chức trang chung 1 stack), dialog cho mọi thao tác (có xem trước thật cho Watermark/Header-Footer)
  - [ ] Follow-up: preview thật cho Insert/Extract/Replace/Crop, tách theo outline, watermark ảnh — xem [docs/10-phase3-summary.md](docs/10-phase3-summary.md)
  - **Checklist test cho bạn**: [docs/11-phase3-user-tests.md](docs/11-phase3-user-tests.md)
- [x] **Phase 4 — Chỉnh sửa nội dung (Edit)** ⭐ tính năng lõi/moat ✅ Iteration 1 + 2 + 3 hoàn tất (52/52 test xanh ngoài qpdf)
  - [x] Engine `edit.rs`: liệt kê object + sửa text run (tiếng Việt), xoá, di chuyển/resize, thêm chữ/ảnh, thay ảnh — round-trip test
  - [x] UI chế độ "Sửa nội dung": overlay đối tượng, sửa text tại chỗ WYSIWYG, thêm chữ/ảnh, xoá, cỡ chữ/màu, undo/redo, lưu
  - [x] **Iteration 2 — GIỮ FONT khi sửa (chuẩn Foxit)**: sửa tại chỗ giữ nguyên font gốc/nhúng (kể cả tiếng Việt); thiếu glyph mới thay font CÙNG HỌ (`fontmatch.rs`); đổi cỡ/màu không đụng font (fix bug phóng đại kép); sửa CẢ DÒNG (gộp run); WYSIWYG khi gõ; B/I + đổi font family; kéo-thả live; dọn file tạm
  - [x] **Iteration 3 — Reflow đoạn "như Word"**: double-click đoạn nhiều dòng → sửa cả đoạn, tự bẻ dòng theo bề rộng khối (đo hmtx bằng ttf-parser), giữ font (nhúng lại bytes gốc / font chuẩn base-14 / cùng họ), giữ nhịp baseline; Enter = ngắt cứng
  - [ ] Follow-up: reflow v2 (justify, khối xoay, đa font), xoay/lật/clip, viền/opacity/căn lề, convert text→path — xem [docs/12-phase4-summary.md](docs/12-phase4-summary.md)
  - **Checklist test cho bạn**: [docs/13-phase4-user-tests.md](docs/13-phase4-user-tests.md) (mới: E17–E30)
- [x] **Phase 5 — Bảo mật** ✅ Iteration 1 + 2 hoàn tất (62/62 test engine xanh)
  - [x] Engine: mã hoá AES-256 + quyền hạn (in/sửa/copy/chú thích), gỡ mật khẩu (`qpdf.rs`); **redaction THẬT** (`redact.rs`: xoá object, bôi đen pixel ảnh trong chính dữ liệu ảnh, **tỉa theo ký tự** giữ phần ngoài vùng — test kiểm bytes/pixel/extract); xoá metadata /Info + XMP (`meta.rs`)
  - [x] **Chữ ký số PKCS#7/PAdES** (`sign.rs`): tạo Digital ID tự ký (RSA-2048), ký (CMS SignedData detached, ByteRange/Contents đúng chuẩn Adobe), **xác thực + phát hiện giả mạo**; lưu tối ưu (nén + dọn object rác)
  - [x] UI thanh "🔒 Bảo mật": redact 2 bước như Foxit, Đặt/Gỡ mật khẩu + quyền, Xoá metadata, **Tạo Digital ID / Ký số / Kiểm tra chữ ký / Lưu tối ưu**
  - [ ] Về sau: timestamp PAdES-T/LTV, đọc PFX/PKCS#12, multi-sig incremental — xem [docs/15-phase5-summary.md](docs/15-phase5-summary.md) mục 6.6
  - **Checklist test cho bạn**: [docs/16-phase5-user-tests.md](docs/16-phase5-user-tests.md) (mới: S13–S19)
- [ ] **Phase 6 — Form (AcroForm)** (kế tiếp) — xem [docs/03-roadmap.md](docs/03-roadmap.md)

Build & chạy: xem [docs/05-dev-setup.md](docs/05-dev-setup.md).

## Nguyên tắc làm việc qua nhiều session

1. **Tài liệu là nguồn chân lý.** Mọi quyết định kiến trúc phải được ghi vào `docs/`.
2. Mỗi phase có **Definition of Done** rõ ràng và **test** kèm theo.
3. Không viết lại engine PDF từ con số 0 ở giai đoạn đầu — đứng trên vai người khổng lồ (PDFium/QPDF), tự xây phần "editor" và "UX".
4. Ưu tiên giấy phép **permissive** (BSD/Apache/MIT) để sản phẩm có thể tự do phân phối.
