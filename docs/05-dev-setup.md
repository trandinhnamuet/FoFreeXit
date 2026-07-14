# 05 — Hướng dẫn build & chạy (dev)

> Cập nhật ở cuối Phase 1. Áp dụng cho Windows x64 (môi trường dev hiện tại).

## Yêu cầu môi trường
- **Rust** (MSVC toolchain), cài qua rustup. Kiểm: `rustc --version` (hiện 1.96.0).
- **Visual Studio Build Tools** + workload **Desktop development with C++** (cung cấp `cl.exe`/`link.exe` cho linker MSVC).
- **WebView2 Runtime** (cho Tauri; Windows 11 thường có sẵn).
- **PDFium**: `pdfium.dll` prebuilt — tải bằng `scripts/fetch-pdfium.ps1` (đặt vào gốc workspace + `pdfium/`).
- **QPDF** (từ Phase 3): `qpdf.exe` prebuilt — tải bằng `scripts/fetch-qpdf.ps1`. Dùng để sửa file hỏng (repair) trước khi PDFium mở + mã hoá lại file sau khi chỉnh sửa.
- (Tauri) Node không bắt buộc: app dùng frontend tĩnh, chạy thẳng bằng `cargo run`.

## Lấy PDFium
```powershell
# Tải pdfium.dll (bblanchon/pdfium-binaries) về gốc workspace
powershell -File scripts/fetch-pdfium.ps1
```
Engine tìm `pdfium.dll` theo thứ tự: `FOFREEXIT_PDFIUM_PATH` → cwd → `./pdfium` → thư mục exe → thư viện hệ thống.

## Lấy QPDF
```powershell
# Tải qpdf.exe (prebuilt) về workspace
powershell -File scripts/fetch-qpdf.ps1
```
Engine tìm `qpdf.exe` theo thứ tự: `FOFREEXIT_QPDF_PATH` → `<cwd>/qpdf/bin/qpdf.exe` → cạnh file exe → `qpdf` trong PATH hệ thống (xem `crates/ff-engine/src/qpdf.rs`).

## Cấu trúc
```
crates/ff-cos        # COS layer (khung)
crates/ff-pdmodel    # PD layer (khung)
crates/ff-engine     # render + chỉnh sửa qua PDFium/QPDF
  render.rs          #   Phase 1: render trang, page_dims, extract_text, search, outline
  annot.rs           #   Phase 2: chú thích (highlight/note/shape/free-draw...)
  organize.rs        #   Phase 3: tổ chức trang (insert/delete/rotate/crop/merge/split)
  watermark.rs       #   Phase 3: watermark + header/footer
  qpdf.rs            #   Phase 3: repair file hỏng + mã hoá lại (QPDF CLI)
  edit.rs            #   Phase 4: sửa nội dung (text run/object) trực tiếp
crates/ff-cli        # CLI "ff" (info, render, pages, text, search, outline)
app/src              # frontend tĩnh (HTML/CSS/JS)
app/src-tauri        # backend Tauri (crate riêng, workspace tách)
corpus/              # PDF mẫu để test
scripts/             # tiện ích (fetch-pdfium, fetch-qpdf)
```

## CLI
```powershell
# Đếm trang
cargo run -p ff-cli -- info corpus\hello.pdf
# Render trang 0 ra PNG, rộng 800px
cargo run -p ff-cli -- render corpus\hello.pdf out.png --page 0 --width 800
# Kích thước mọi trang
cargo run -p ff-cli -- pages corpus\sample-multipage.pdf
# Trích text 1 trang
cargo run -p ff-cli -- text corpus\sample-multipage.pdf --page 1
# Tìm kiếm (in vị trí + toạ độ)
cargo run -p ff-cli -- search corpus\sample-multipage.pdf content
# Outline / bookmarks
cargo run -p ff-cli -- outline corpus\sample-multipage.pdf
```

## Test
```powershell
cargo test -p ff-engine          # toàn bộ test engine (cần pdfium.dll + qpdf.exe)
```
Bao gồm: render smoke, viewer features, big-doc, annot round-trip (Phase 1–2),
organize/watermark/qpdf-safety (Phase 3), edit round-trip (Phase 4). Hiện **43/43 xanh**.

## Chạy app desktop (Tauri)
```powershell
cd app\src-tauri
cargo run                        # mở cửa sổ, hiển thị corpus\hello.pdf
```
> App là một Cargo workspace **riêng** (có `[workspace]` trống trong `app/src-tauri/Cargo.toml`)
> nên lệnh `cargo` ở gốc repo không build app, và ngược lại. Đây là chủ ý để tránh
> xung đột profile build giữa thư viện và Tauri.

## Ghi chú
- Bản dev set `FOFREEXIT_PDFIUM_PATH` tự động về gốc workspace (xem `app/src-tauri/src/main.rs`).
  Bản release sẽ bundle `pdfium.dll` như resource (việc của Phase 8).
