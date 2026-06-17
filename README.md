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
- [ ] **Phase 3 — Organize trang + lưu file vững** (kế tiếp) — xem [docs/03-roadmap.md](docs/03-roadmap.md)

Build & chạy: xem [docs/05-dev-setup.md](docs/05-dev-setup.md).

## Nguyên tắc làm việc qua nhiều session

1. **Tài liệu là nguồn chân lý.** Mọi quyết định kiến trúc phải được ghi vào `docs/`.
2. Mỗi phase có **Definition of Done** rõ ràng và **test** kèm theo.
3. Không viết lại engine PDF từ con số 0 ở giai đoạn đầu — đứng trên vai người khổng lồ (PDFium/QPDF), tự xây phần "editor" và "UX".
4. Ưu tiên giấy phép **permissive** (BSD/Apache/MIT) để sản phẩm có thể tự do phân phối.
