# 04 — Kiến trúc kỹ thuật (khung — sẽ chi tiết hoá sau khi chốt stack)

> Tài liệu này sẽ được hoàn thiện ở đầu Phase 1, sau khi đã chốt ngôn ngữ/framework ở [02-tech-stack.md](02-tech-stack.md).

## Mô hình 2 tầng (học từ Adobe COS/PD và Apache PDFBox)

- **`cos` (Carousel Object System)** — tầng object thấp: `Object`, `Dictionary`, `Array`, `Stream`, `Name`, `Ref`, `XRef`. Đọc/ghi cấu trúc file PDF thô. (Chủ yếu tựa QPDF.)
- **`pdmodel`** — tầng tài liệu cao: `Document`, `Page`, `Resources`, `ContentStream`, `TextRun`, `ImageObject`, `PathObject`, `Annotation`, `AcroForm`, `Field`. (Tựa PDFium + logic riêng.)

## Các module chính
- `engine/render` — bọc PDFium render (tile, cache, đa luồng).
- `engine/text` — gom glyph → run/line/paragraph; chọn/copy; tìm kiếm.
- `engine/edit` — Page Object Model; sửa text/object; serialize content stream.
- `engine/font` — FreeType + HarfBuzz: đo width, shaping, subset, substitution.
- `engine/io` — mở/lưu (incremental + full rewrite via QPDF), repair, linearize, encrypt.
- `engine/annot` — annotation CRUD + XFDF.
- `engine/security` — mã hoá, redaction, chữ ký số (PAdES).
- `engine/form` — AcroForm điền/tạo/flatten.
- `engine/ocr` — Tesseract + tiền xử lý + tạo lớp text.
- `engine/convert` — cầu nối LibreOffice headless; PDF→ảnh; PDF→Office.
- `core-api` — biên giới ổn định (FFI/IPC) cho UI.
- `app/*` — UI theo framework đã chọn.
- `cli` — công cụ dòng lệnh cho test/automation (chạy mọi tính năng không cần UI).

## Nguyên tắc
- Engine **không phụ thuộc UI**; mọi tính năng phải gọi được từ `cli` → test tự động.
- Edit dùng **command pattern** → undo/redo.
- Mọi thao tác ghi đều có đường **không phá file gốc** (atomic write / backup).

## Test strategy
- Unit test cho `cos`/`pdmodel`.
- Golden/render-hash test cho viewer.
- Round-trip test cho annotate/organize/form/security.
- Corpus test (file đời thực, file hỏng, đa ngôn ngữ) + fuzzing parser.

## Hạng mục cần quyết khi vào Phase 1
- [x] Ngôn ngữ/UI: **Rust + Tauri** (chốt 2026-06-16).
- [ ] Phân phối PDFium: dùng prebuilt `bblanchon/pdfium-binaries` qua crate `pdfium-render` (chốt version cụ thể đầu Phase 1).
- [ ] Biên giới core-api: trong Tauri, engine Rust chạy cùng tiến trình; cân nhắc tách tiến trình parser để **sandbox** file không tin cậy (quyết định khi làm hardening).
- [ ] Bố cục repo (mono-repo Cargo workspace, xem dưới).

## Bố cục repo dự kiến (Rust + Tauri, Cargo workspace)
```
FoFreeXit/
├─ Cargo.toml              # workspace
├─ crates/
│  ├─ ff-cos/              # tầng object thấp (cấu trúc PDF)
│  ├─ ff-pdmodel/          # tầng tài liệu cao (page/text/annot/...)
│  ├─ ff-engine/           # render/edit/io/font/ocr... (bọc PDFium/QPDF/Tesseract)
│  └─ ff-cli/              # CLI chạy mọi tính năng không cần UI (để test)
├─ app/                    # Tauri app
│  ├─ src-tauri/           # Rust backend (gọi ff-engine)
│  └─ src/                 # frontend (TS/HTML/CSS, PDF.js viewer)
├─ tests/                  # integration tests
├─ corpus/                 # PDF mẫu (thường/scan/mã hoá/hỏng/CJK/Việt/form)
└─ docs/
```

## Bước khởi động Phase 1 (gợi ý cho session sau)
1. Khởi tạo Cargo workspace + crate `ff-cli`, `ff-engine`.
2. Thêm `pdfium-render` + tải prebuilt PDFium; viết lệnh CLI `render <pdf> <page> <out.png>` → xác nhận engine chạy.
3. Khởi tạo Tauri app, hiển thị 1 trang PDF render từ engine.
4. Dựng `corpus/` ban đầu (vài file đa dạng) + test render-hash đầu tiên.
