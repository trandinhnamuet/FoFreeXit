# 02 — Lựa chọn ngôn ngữ, framework & engine

## Nguyên tắc quyết định
1. **Permissive license** cho toàn bộ thành phần ship (BSD/Apache/MIT) → tránh AGPL.
2. **Không viết lại engine PDF từ đầu** ở giai đoạn đầu → dùng PDFium + QPDF.
3. Tách **Engine (không UI)** khỏi **App (UI)** để test & tái dùng.
4. Tối ưu cho **Windows trước** (thị trường Foxit), nhưng giữ đường ra **đa nền tảng**.
5. Dự án do AI hỗ trợ qua nhiều session → ưu tiên **ngôn ngữ có ecosystem PDF mạnh + tooling tốt + dễ test**, hơn là "ngôn ngữ dev người quen tay".

---

## Tầng ENGINE (lõi xử lý PDF) — quyết định gần như bắt buộc

**Chọn: PDFium (BSD-3) làm engine render + edit object, + QPDF (Apache-2.0) cho cấu trúc/mã hoá/repair.**

Lý do: cả hai permissive, trưởng thành, được hậu thuẫn lớn (Google/Chromium). PDFium chính là thứ Chrome/Edge dùng để hiển thị PDF → render chất lượng production. Có binary prebuilt (bblanchon/pdfium-binaries) → khỏi build từ nguồn (vốn rất khổ). PDFium là **C/C++** → FFI tự nhiên nhất từ C++; binding sẵn cho .NET (PDFiumCore/PdfiumViewer), Rust (pdfium-render), Python (pypdfium2).

Bổ trợ chuyên biệt: **Tesseract** (OCR), **HarfBuzz** (shaping), **FreeType** (font), **LibreOffice headless** (convert Office). Tham khảo thuật toán: **Apache PDFBox** (đọc mã được, permissive).

---

## ✅ QUYẾT ĐỊNH ĐÃ CHỐT (2026-06-16): **Rust + Tauri**

Tầng App/UI dùng **Rust + Tauri**; engine **PDFium (pdfium-render) + QPDF**; viewer có thể nhúng **PDF.js** trong webview cho tốc độ, dùng PDFium cho edit/save. Lý do chốt: an toàn bộ nhớ khi xử lý file không tin cậy, hiệu năng cao, license sạch, UI hiện đại phát triển nhanh, phù hợp dự án dài hơi nhiều session. Các lựa chọn A/C/D bên dưới giữ lại để tham khảo/đối chiếu.

## Tầng APP (ngôn ngữ + UI framework) — các lựa chọn đã cân nhắc

So sánh 4 lựa chọn khả thi:

### Lựa chọn A — C++ + Qt (giống Foxit/Adobe nhất)
- **+** FFI với PDFium = 0 ma sát (cùng C++); hiệu năng tối đa; Qt là framework desktop mạnh nhất, đa nền tảng, in ấn/đồ hoạ tốt.
- **+** Đây là con đường "đẳng cấp Foxit" thực sự.
- **−** Tốc độ phát triển chậm, build phức tạp (CMake/vcpkg), quản lý bộ nhớ thủ công, refactor tốn công.
- **License Qt:** LGPL (dùng được miễn phí nếu link động) hoặc thương mại.

### Lựa chọn B — Rust + Tauri (hiện đại, an toàn) ⭐ *khuyến nghị nếu muốn cân bằng*
- **+** An toàn bộ nhớ (ít crash/CVE — rất hợp với phần mềm "đọc file lạ"). `pdfium-render` (binding PDFium) trưởng thành. Tauri = UI web (HTML/CSS/TS) nhẹ, bundle nhỏ.
- **+** Có thể nhúng **PDF.js** trong webview cho viewer nhanh, dùng PDFium (Rust) cho edit/save.
- **+** Cargo/tooling tuyệt vời, test dễ, cross-platform.
- **−** Ecosystem PDF Rust non hơn C++; cần FFI sang PDFium (đã có sẵn binding).
- **License:** Rust crates đa số MIT/Apache; Tauri MIT/Apache.

### Lựa chọn C — C# / .NET 8 + Avalonia (năng suất cao, Windows-first)
- **+** Năng suất phát triển cao nhất; XAML/MVVM quen thuộc; Avalonia đa nền tảng (hoặc WPF nếu chỉ Windows). Binding PDFium .NET sẵn.
- **+** Dễ tuyển/đọc code, dễ test (xUnit).
- **−** FFI sang native qua P/Invoke (ổn nhưng có chi phí marshaling); GC pause với file rất lớn cần chú ý.
- **License:** .NET & Avalonia MIT.

### Lựa chọn D — TypeScript + Electron + PDF.js (nhanh ra mắt, web-native)
- **+** Dev UI nhanh nhất, hệ sinh thái khổng lồ, dễ làm bản web sau này.
- **−** PDF.js gần như render-only → edit/save phải tự build hoặc nhúng native (pdfium qua WASM/node-addon); Electron nặng RAM; hiệu năng kém nhất với file lớn. **Yếu nhất cho tính năng EDIT** — điểm cốt lõi của dự án.

### Bảng tổng hợp

| Tiêu chí | A: C++/Qt | B: Rust/Tauri | C: C#/Avalonia | D: TS/Electron |
|---|:--:|:--:|:--:|:--:|
| Ma sát FFI PDFium | ⭐ | ✔ | ✔ | △ |
| Hiệu năng / file lớn | ⭐ | ⭐ | ✔ | △ |
| An toàn bộ nhớ | △ | ⭐ | ✔ | ✔ |
| Tốc độ phát triển | △ | ✔ | ⭐ | ⭐ |
| Sức mạnh tính năng EDIT | ⭐ | ⭐ | ✔ | △ |
| Cross-platform | ⭐ | ⭐ | ⭐ | ⭐ |
| Độ "đẳng cấp Foxit" | ⭐ | ✔ | ✔ | △ |

### ➜ Khuyến nghị
- **Cân bằng tốt nhất (khuyến nghị chính): B — Rust + Tauri.** An toàn (quan trọng khi xử lý file không tin cậy), hiệu năng cao, binding PDFium tốt, UI hiện đại nhanh phát triển, license sạch. Phù hợp dự án dài hơi do AI hỗ trợ.
- **Nếu ưu tiên "đúng chuẩn Foxit", chấp nhận chậm: A — C++/Qt.**
- **Nếu ưu tiên ra tính năng nhanh trên Windows: C — C#/Avalonia.**
- **Không khuyến nghị D** làm kiến trúc chính vì điểm yếu ở tính năng EDIT (lõi giá trị của dự án).

> **Điểm chung mọi lựa chọn:** Tầng *Engine* (PDFium + QPDF + Tesseract...) gần như không đổi. Lựa chọn ngôn ngữ chủ yếu ảnh hưởng tầng *App/UI*. Vì vậy nên **đóng gói Engine sau một API ổn định** (FFI/IPC) để có thể đổi UI mà không đập lõi.

---

## Kiến trúc đề xuất (độc lập với lựa chọn UI)

```
┌──────────────────────────────────────────┐
│                 UI Layer                  │  (Qt / Tauri-web / Avalonia)
│   Viewer · Annotate · Edit · Organize     │
├──────────────────────────────────────────┤
│              App / Commands               │  undo/redo, document state, MVVM
├──────────────────────────────────────────┤
│        FoFreeXit Core API (stable)        │  ← biên giới ổn định (FFI/IPC)
│  pdmodel: Page, TextRun, Image, Annot...  │  ← tầng "PD" kiểu Adobe
│  cos: Object, Dict, Stream, XRef          │  ← tầng "COS" kiểu Adobe
├──────────────────────────────────────────┤
│   PDFium  │  QPDF  │ Tesseract │ HarfBuzz │  (engine permissive)
│  render+edit│ struct │  OCR    │ shaping  │
└──────────────────────────────────────────┘
```

Chi tiết hoá tiếp ở [04-architecture.md](04-architecture.md) sau khi chốt stack.
