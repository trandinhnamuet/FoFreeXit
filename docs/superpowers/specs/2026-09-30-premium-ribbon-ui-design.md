# Thiết kế: Giao diện Ribbon cao cấp + i18n VI/EN + Theme Sáng/Tối/Hệ thống

Ngày: 2026-09-30 · Phạm vi: `app/src/*`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json`

## 1. Mục tiêu

- Bố cục theo ảnh tham chiếu (kiểu ribbon PDF editor hiện đại): title bar tự vẽ, hàng tab dạng pill, ribbon icon lớn + nhãn, menu **Files** thả xuống, thanh điều hướng đáy.
- Chỉ hiển thị **nút có chức năng thật** (không nút giả, không "Upgrade"/AI/Translate).
- Chuẩn hoá ngôn ngữ: toàn bộ UI có đúng 2 ngôn ngữ **Tiếng Việt / English**, không lẫn lộn.
- Theme: **Sáng / Tối / Hệ thống**.
- Không làm hỏng bất kỳ chức năng hiện có nào.
- Giữ tên thương hiệu **FoFreeXit** (không dùng tên/brand của sản phẩm trong ảnh).

## 2. Kiến trúc (Hướng A — dựng lại shell, giữ logic)

| File | Vai trò |
|---|---|
| `index.html` | Shell mới: titlebar, tabrow, ribbon (1 panel/tab), sidebar icon rail, viewport, bottombar, Files menu. **Giữ nguyên mọi ID** mà `main.js` dùng. |
| `styles.css` | Viết lại hoàn toàn trên design tokens (`:root[data-theme]`). |
| `i18n.js` | `I18N.dict = {vi:{}, en:{}}`, `t(key, vars)`, `applyI18n(root)`, `setLang()`, sự kiện `langchange`. |
| `i18n/part-*.js` | Mảnh từ điển (mỗi phần do một agent viết), gộp bằng `I18N.add(lang, obj)`. |
| `theme.js` | `setTheme('light'|'dark'|'system')`, nghe `prefers-color-scheme`, lưu localStorage. |
| `icons.js` | Bộ icon SVG line 24px (stroke 1.6, `currentColor`), gắn qua `data-icon="name"`. |
| `shell.js` | Titlebar (window controls Tauri), tab router `setMode(tab)`, Files menu, recent files, bottombar sync, collapse ribbon, reading mode, fullscreen, phím tắt/giới thiệu. |
| `main.js` | Logic hiện có; chỉ thay chuỗi cứng bằng `t()` và gọi hook nhỏ (vd. `Shell.onDocLoaded(path)`). |

Thứ tự nạp script: `i18n.js` → `i18n/part-*.js` → `theme.js` → `icons.js` → `main.js` → `shell.js`.

## 3. Bố cục & phong cách

- **Titlebar 32px**: logo + "FoFreeXit — tên_file.pdf" (drag region, double-click maximize), nút min/max/close kiểu Win11 (close hover đỏ). Nền pha nhẹ accent.
- **Tab row 44px**: pill **Files** (accent) · quick access (Mở, Lưu, Hoàn tác, Làm lại) · tabs Home/Edit/Annotate/Page/Form/Convert/Protect/Tools/Help — tab active = pill accent. Phải: ngôn ngữ VI/EN, theme, chế độ đọc, toàn màn hình, thu gọn ribbon.
- **Ribbon ~84px**: nút dọc (icon 24px + nhãn 12px), nhóm cách bằng vạch dọc, `▾` khi có menu con. Nút toggle active = nền accent-soft.
- **Phân bổ tab** (chỉ nút thật):
  - Home: Chọn/Kéo, Zoom (select, −, +, Vừa rộng, Vừa trang), Tô sáng, Crop, Copy text, Tìm, OCR.
  - Edit (= edit mode): Thêm chữ, Thêm ảnh, Xoá, Thay ảnh, Font, Cỡ, B, I, Màu, Huỷ thay đổi, Lưu. (Watermark / Header & Footer đặt ở tab Page vì dùng pagePlan của chế độ Tổ chức trang — đặt ở Edit sẽ xung đột editBase.)
  - Annotate: Tô sáng, Gạch chân, Gạch ngang, Khung, Text box, Ghi chú, Màu, Lưu chú thích (n).
  - Page (= organize mode): Chèn, Xoá, Xoay trái/phải, Trích, Thay, Trộn PDF, Tách PDF, Watermark, Header & Footer, Lưu.
  - Form: Điền form (n), Thêm field, Làm phẳng form, Xuất FDF, Xuất CSV, Nhập FDF.
  - Convert: Ngôn ngữ OCR, OCR, PDF→PNG, PDF→TXT, PDF→Word, Office→PDF.
  - Protect: Đánh dấu redact, Áp dụng redact (n), Bỏ đánh dấu, Đặt/Gỡ mật khẩu, Xoá metadata, Tạo Digital ID, Ký số, Kiểm tra chữ ký.
  - Tools: Lưu tối ưu (nén), Tìm, Copy text, Chế độ đọc.
  - Help: Phím tắt, Giới thiệu.
- **Files menu**: Mở file, File gần đây ▸ (localStorage, tối đa 10), Trộn & Tách ▸, Nén PDF, Xuất PDF ▸ (PNG/TXT/Word), Thuộc tính tài liệu, Thoát.
- **Sidebar**: rail icon dọc (Thumbnails/Outline/Chú thích) + panel thu gọn được.
- **Bottombar**: ⏮ ◀ [trang / tổng] ▶ ⏭ (giữa), zoom − [%] + (phải), status (trái).
- **Tokens**: accent `#E8474C`; sáng: nền `#FFFFFF`, canvas `#EEF0F3`, text `#1F2328`; tối: nền `#1E1F22`, bề mặt `#2B2D31`, canvas `#141517`, text `#E6E6E6`. Font "Segoe UI Variable", "Segoe UI"; radius 8px; transition 150ms. Trang PDF luôn nền trắng + shadow mềm.

## 4. i18n

- Key có ngữ cảnh: `tab.*`, `ribbon.<tab>.*`, `files.*`, `modal.<name>.*`, `status.*`, `hint.*`, `err.*`, `confirm.*`.
- HTML: `data-i18n`, `data-i18n-title`, `data-i18n-placeholder`, `data-i18n-aria`.
- JS: `t("status.savedTo", {path})`; placeholder `{name}`.
- Ngôn ngữ mặc định: localStorage → `navigator.language` bắt đầu `vi` ⇒ VI, ngược lại EN.
- Đổi ngôn ngữ áp dụng tức thì (`applyI18n(document)`, phát `langchange`; phần UI động re-render khi cần).
- Không chuỗi hiển thị nào được viết cứng trong JS/HTML (trừ tên file, dữ liệu người dùng, tên font, mã như "Aa", "B", "I").
- Chuỗi lỗi từ backend Rust giữ nguyên nhưng được bọc bởi tiền tố đã dịch (vd. `t("err.generic", {msg})`).
- Script kiểm tra `scripts/check-i18n.mjs`: mọi key dùng trong HTML/JS có ở cả `vi` và `en`; hai ngôn ngữ cùng tập key; không còn chuỗi có dấu tiếng Việt trong `main.js` ngoài comment.

## 5. Theme

- `data-theme` trên `<html>` = `light|dark`, tính từ lựa chọn `light|dark|system`.
- `system` nghe `matchMedia('(prefers-color-scheme: dark)')` thay đổi trực tiếp.
- Không mã màu cứng ngoài token (trừ màu chú thích/màu người dùng chọn).

## 6. Giữ logic đúng

- Mọi ID mà `main.js` tra cứu tồn tại trong HTML mới. Nút mode cũ (`organizeModeBtn`, `editModeBtn`, `formModeBtn`, `secModeBtn`, `convModeBtn`) và các container thanh cũ (`annobar`, `organizeBar`, `editBar`, `formBar`, `convBar`, `secBar`) vẫn tồn tại: các thanh trở thành panel ribbon; các nút mode được giữ ẩn và chỉ được gọi qua router.
- `setMode(tab)`: thoát mode hiện tại bằng toggle hiện có, vào mode mới bằng toggle hiện có, rồi **đọc lại `state.*Mode`** để đặt tab active — nếu logic cũ từ chối (còn thay đổi chưa lưu), tab không đổi.
- Panel ribbon hiển thị theo tab đang chọn (không phụ thuộc `hidden` do main.js set trên thanh cũ; shell điều khiển hiển thị bằng class trên ribbon).
- Titlebar: `decorations: false`; capability thêm `core:window:allow-minimize`, `core:window:allow-toggle-maximize`, `core:window:allow-close`, `core:window:allow-start-dragging`, `core:window:allow-set-title`, `core:window:allow-is-maximized`.

## 7. Kiểm thử

- `node scripts/check-i18n.mjs` và `node scripts/check-ids.mjs` (mọi `$("id")`/`getElementById` trong JS có trong HTML hoặc được tạo động) — PASS.
- `cargo test` workspace — PASS (không đổi backend).
- Chạy app: chụp mỗi tab × 2 theme × 2 ngôn ngữ; kiểm tra luồng: mở file, chú thích + lưu, tổ chức trang + lưu, sửa chữ + lưu, form điền, đặt mật khẩu, OCR/xuất.

## 8. Thực thi song song

1. Nền (tuần tự): shell HTML/CSS, `i18n.js`, `theme.js`, `shell.js`, stub `icons.js`.
2. Song song (worktree riêng, vùng không chồng lấn): agent 1–6 dịch `main.js` theo khoảng dòng + ghi `i18n/part-N.js`; agent 7 bộ icon + hoàn thiện ribbon; agent 8 Files menu/recent/bottombar/rà dark theme.
3. Gộp bằng git, chạy kiểm tra, review cuối.
