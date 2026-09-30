# Premium Ribbon UI + i18n VI/EN + Theme — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dựng lại shell giao diện FoFreeXit theo kiểu ribbon hiện đại (ảnh tham chiếu), chuẩn hoá 100% chuỗi UI sang 2 ngôn ngữ VI/EN, theme Sáng/Tối/Hệ thống — không đổi logic nghiệp vụ.

**Architecture:** Frontend tĩnh (vanilla JS, classic scripts dùng chung global scope) trong Tauri 2. Shell mới ở `index.html`/`styles.css`, các module nhỏ `i18n.js`, `theme.js`, `icons.js`, `shell.js`, `shell-extras.js`. `main.js` giữ nguyên logic, chỉ thay chuỗi cứng bằng `t()` + 1 hook `Shell.onDocLoaded`. Mọi ID `main.js` dùng được giữ nguyên.

**Tech Stack:** HTML/CSS/vanilla JS, Tauri 2 (`window.__TAURI__.core.invoke`, `window.__TAURI__.window`), Node (chỉ cho script kiểm tra), Rust/cargo (build app).

Spec: `docs/superpowers/specs/2026-09-30-premium-ribbon-ui-design.md`

---

## File map

| File | Trách nhiệm | Ai sửa |
|---|---|---|
| `app/src/index.html` | Shell: titlebar, tabrow, ribbon panels, sidebar rail, viewport, bottombar, Files menu, popovers | Nền (T2) |
| `app/src/styles.css` | Tokens + toàn bộ style | Nền (T3), Agent 7 (chỉ khối `/* == ribbon == */`) |
| `app/src/i18n.js` | Engine i18n | Nền (T1) |
| `app/src/i18n/part-0-shell.js` | Từ điển chuỗi của HTML + shell + `common.*` | Nền (T2) |
| `app/src/i18n/part-{1..6}.js` | Từ điển từng vùng `main.js` | Agent 1–6 |
| `app/src/theme.js` | Theme | Nền (T1) |
| `app/src/icons.js` | Bộ icon SVG + `applyIcons()` | Nền (stub, T2) → Agent 7 |
| `app/src/shell.js` | Titlebar, tab router, quick save, popover ngôn ngữ/theme, sidebar collapse, bottombar first/last, collapse ribbon | Nền (T4) |
| `app/src/shell-extras.js` | Files menu, File gần đây, Thuộc tính tài liệu, Phím tắt, Giới thiệu, Chế độ đọc, Toàn màn hình | Agent 8 |
| `app/src/main.js` | Logic (chỉ đổi chuỗi → `t()`) | Nền (hook, T4), Agent 1–6 theo khoảng dòng |
| `scripts/check-i18n.mjs`, `scripts/check-ids.mjs` | Kiểm tra tĩnh | Agent 8 |
| `app/src-tauri/tauri.conf.json`, `capabilities/default.json` | decorations:false + quyền window | Nền (T4) |

Thứ tự `<script>`: `i18n.js`, `i18n/part-0-shell.js`, `i18n/part-1.js` … `part-6.js`, `theme.js`, `icons.js`, `main.js`, `shell.js`, `shell-extras.js`.

## Hợp đồng (contract) dùng chung — mọi agent phải tuân thủ

```js
// i18n.js (global)
I18N.add(lang, obj)          // lang: "vi" | "en"; obj phẳng { "key": "text" }
t(key, vars?)                // "Đã lưu {n} trang" + {n:3} → "Đã lưu 3 trang"; key thiếu → trả key
I18N.lang                    // "vi" | "en"
I18N.setLang(lang)           // lưu localStorage "ff.lang", applyI18n(document), dispatch "langchange" trên document
applyI18n(root)              // [data-i18n] textContent, [data-i18n-title] title, [data-i18n-placeholder], [data-i18n-aria] aria-label
// theme.js
Theme.set("light"|"dark"|"system"); Theme.pref; Theme.effective   // <html data-theme="light|dark">, localStorage "ff.theme"
// icons.js
applyIcons(root)             // <i data-icon="name"></i> → inline SVG 24x24, stroke currentColor, stroke-width 1.6
// shell.js
Shell.onDocLoaded(path, meta)   // gọi từ main.js sau khi mở tài liệu thành công
Shell.setTab(tab)               // "home"|"edit"|"annotate"|"page"|"form"|"convert"|"protect"|"tools"|"help"
Shell.currentTab
Shell.onLangChange(fn)          // đăng ký re-render khi đổi ngôn ngữ
```

**Quy tắc key:** mỗi agent chỉ tạo key dưới prefix được giao; được DÙNG (không tạo mới) các key `common.*` có sẵn trong `part-0-shell.js`:
`common.ok, common.cancel, common.apply, common.close, common.save, common.saveAs, common.browse, common.delete, common.yes, common.no, common.page, common.pages, common.error, common.done, common.loading, common.file, common.from, common.to, common.all, common.none, common.selected, common.password, common.name, common.value, common.color, common.size, common.position, common.opacity, common.text, common.font`.

**Quy tắc dịch:** giữ nguyên nghĩa hiện tại của bản VI; bản EN là tiếng Anh chuẩn phần mềm (Adobe/Foxit terminology). Không dịch: tên file, đường dẫn, dữ liệu người dùng, tên font, "Aa", "B", "I", "PDF", "FDF", "CSV", "PNG", "TXT", "OCR", "AES-256", "RSA-2048". Comment code giữ nguyên.

---

## Giai đoạn 1 — Nền (tuần tự, người điều phối làm)

### Task 1: `i18n.js` + `theme.js`

**Files:** Create `app/src/i18n.js`, `app/src/theme.js`

- [ ] **Step 1: Viết `i18n.js`**

```js
// i18n: từ điển VI/EN phẳng, key có ngữ cảnh. Đổi ngôn ngữ áp dụng tức thì.
(function () {
  const dict = { vi: {}, en: {} };
  let lang = null;
  try { lang = localStorage.getItem("ff.lang"); } catch (_) {}
  if (lang !== "vi" && lang !== "en") {
    lang = /^vi\b/i.test(navigator.language || "") ? "vi" : "en";
  }
  function fmt(s, vars) {
    return vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s;
  }
  window.t = function (key, vars) {
    const s = dict[lang][key] ?? dict.vi[key] ?? key;
    return fmt(s, vars);
  };
  window.applyI18n = function (root) {
    const r = root || document;
    r.querySelectorAll("[data-i18n]").forEach((el) => { el.textContent = t(el.dataset.i18n); });
    r.querySelectorAll("[data-i18n-title]").forEach((el) => { el.title = t(el.dataset.i18nTitle); });
    r.querySelectorAll("[data-i18n-placeholder]").forEach((el) => { el.placeholder = t(el.dataset.i18nPlaceholder); });
    r.querySelectorAll("[data-i18n-aria]").forEach((el) => { el.setAttribute("aria-label", t(el.dataset.i18nAria)); });
  };
  window.I18N = {
    dict,
    get lang() { return lang; },
    add(l, obj) { Object.assign(dict[l], obj); },
    setLang(l) {
      if (l !== "vi" && l !== "en") return;
      lang = l;
      try { localStorage.setItem("ff.lang", l); } catch (_) {}
      document.documentElement.lang = l;
      applyI18n(document);
      document.dispatchEvent(new CustomEvent("langchange", { detail: l }));
    },
  };
  document.documentElement.lang = lang;
})();
```

- [ ] **Step 2: Viết `theme.js`**

```js
// Theme: light | dark | system (theo Windows, đổi trực tiếp khi hệ thống đổi).
(function () {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  let pref = "system";
  try { pref = localStorage.getItem("ff.theme") || "system"; } catch (_) {}
  function apply() {
    const eff = pref === "system" ? (mq.matches ? "dark" : "light") : pref;
    document.documentElement.dataset.theme = eff;
    document.documentElement.style.colorScheme = eff;
    document.dispatchEvent(new CustomEvent("themechange", { detail: { pref, effective: eff } }));
  }
  mq.addEventListener("change", () => { if (pref === "system") apply(); });
  window.Theme = {
    get pref() { return pref; },
    get effective() { return document.documentElement.dataset.theme; },
    set(p) {
      if (!["light", "dark", "system"].includes(p)) return;
      pref = p;
      try { localStorage.setItem("ff.theme", p); } catch (_) {}
      apply();
    },
  };
  apply();
})();
```

- [ ] **Step 3: Commit** — `git add app/src/i18n.js app/src/theme.js && git commit -m "UI: i18n + theme engine"`

### Task 2: `index.html` shell mới + `part-0-shell.js` + stub `icons.js`

**Files:** Rewrite `app/src/index.html`; Create `app/src/i18n/part-0-shell.js`, `app/src/icons.js`

Yêu cầu bắt buộc:
- Giữ **mọi ID** hiện có (danh sách lấy bằng `grep -oE 'id="[^"]+"' app/src/index.html` trước khi sửa); không đổi `class="atool" data-tool=...`.
- Các thanh cũ trở thành panel ribbon: `<div class="rpanel" data-tab="annotate" id="annobar">`, `data-tab="page" id="organizeBar"`, `data-tab="edit" id="editBar"`, `data-tab="form" id="formBar"`, `data-tab="convert" id="convBar"`, `data-tab="protect" id="secBar"`; panel mới `data-tab="home"`, `"tools"`, `"help"`. Giữ `hidden` ban đầu như cũ trên các thanh cũ.
- Nút mode cũ `organizeModeBtn, editModeBtn, formModeBtn, secModeBtn, convModeBtn` nằm trong `<div hidden class="legacy">`.
- Nút ribbon: `<button class="rbtn" id="..." data-i18n-title="..."><i data-icon="..."></i><span data-i18n="..."></span></button>`; nhóm `<div class="rgroup">`, vạch `<span class="rsep"></span>`; mỗi panel có `<span class="spacer"></span><span class="status" id="xxxHint"></span>` ở cuối.
- Nút trùng chức năng ở tab khác dùng `data-proxy="<id thật>"` (shell gọi `$(id).click()`); công cụ chú thích trùng dùng lại `class="atool" data-tool=...` (main.js đã bind mọi `.atool`).
- Search (`searchBox, searchCase, searchPrev, searchCount, searchNext`) ở phải tabrow; `copyPage` ở Home; `undoBtn/redoBtn/openBtn` ở quick access; `status, pagePrev, pageInput, pageTotal, pageNext, zoomOut, zoomSelect, zoomIn` ở bottombar; `zoomFit, zoomFitPage` ở Home; `secOptimize` ở Tools.
- Mọi chuỗi hiển thị dùng `data-i18n*`; không chữ cứng.

- [ ] **Step 1:** Ghi danh sách ID cũ ra scratchpad `ids-before.txt`.
- [ ] **Step 2:** Viết `index.html` mới theo yêu cầu trên.
- [ ] **Step 3:** Viết `part-0-shell.js` (`I18N.add("vi", {...}); I18N.add("en", {...});`) với mọi key HTML + `common.*` + `tab.*` + `files.*` + `shell.*`.
- [ ] **Step 4:** Viết stub `icons.js` (API `ICONS`, `applyIcons`) với icon tối thiểu, fallback ô vuông bo góc cho tên chưa có.
- [ ] **Step 5:** Kiểm tra: mọi ID trong `ids-before.txt` có trong HTML mới.

```bash
for id in $(cat ids-before.txt); do grep -q "id=\"$id\"" app/src/index.html || echo MISSING $id; done
```
Expected: không in gì.
- [ ] **Step 6: Commit** `UI: shell ribbon HTML + tu dien shell`

### Task 3: `styles.css` trên design tokens

- [ ] **Step 1:** Viết lại `styles.css`: tokens `:root[data-theme=light]` / `[data-theme=dark]` (accent `#E8474C`, accent-soft, bg, surface, surface-2, canvas, border, text, text-muted, shadow), titlebar/tabrow/ribbon/sidebar/viewport/bottombar/modal/popover/menu, giữ nguyên các class main.js tạo động (`page-slot`, `textlayer`, `annotlayer`, `hl`, `org-card`, `edit-*`, `color-popover`, `note-popup`, `modal-*`, `boot-*`...). Lấy danh sách class động bằng `grep -oE 'className = "[^"]+"|classList\.(add|toggle|remove)\("[^"]+"' app/src/main.js` và đảm bảo mỗi class cũ có style tương đương.
- [ ] **Step 2:** Khối ribbon đánh dấu `/* == ribbon == */ ... /* == /ribbon == */`.
- [ ] **Step 3: Commit** `UI: styles tren design tokens (sang/toi)`

### Task 4: `shell.js` + hook main.js + Tauri window config

- [ ] **Step 1:** `shell.js`: titlebar (min/toggleMaximize/close qua `window.__TAURI__.window.getCurrentWindow()`, đổi icon max/restore theo `isMaximized` khi `resize`), tab router, proxy click, quick save, popover ngôn ngữ/theme, sidebar collapse, first/last page, collapse ribbon (localStorage `ff.ribbonCollapsed`).
- Router `Shell.setTab(tab)`:
  ```js
  const MODE_OF = { edit: "editMode", page: "organizeMode", form: "formMode", convert: "convMode", protect: "secMode" };
  const TOGGLE_OF = { editMode: "editModeBtn", organizeMode: "organizeModeBtn", formMode: "formModeBtn", convMode: "convModeBtn", secMode: "secModeBtn" };
  async function setTab(tab) {
    const want = MODE_OF[tab] || null;
    if (state.editMode && want !== "editMode" && state.editUndo.length &&
        !confirm(t("shell.confirmLeaveEdit"))) return syncTabs();
    for (const m of Object.keys(TOGGLE_OF)) if (m !== want && state[m]) $(TOGGLE_OF[m]).click();
    if (want && !state[want]) $(TOGGLE_OF[want]).click();
    if (state.tool && !(tab === "protect" && state.tool === "redact") && tab !== "home" && tab !== "annotate") setTool(null);
    current = (want && !state[want]) ? current : tab;   // logic cũ từ chối → giữ tab
    syncTabs();
  }
  ```
  `syncTabs()` đặt `.on` cho `.tab[data-tab=current]` và `.rpanel[data-tab=current]`.
- [ ] **Step 2:** main.js: sau dòng `updateZoomLabel();` trong `loadDocument` thêm `if (window.Shell) Shell.onDocLoaded(path, meta);`.
- [ ] **Step 3:** `tauri.conf.json`: `"decorations": false`, title `"FoFreeXit"`. `capabilities/default.json` thêm `core:window:allow-minimize`, `core:window:allow-toggle-maximize`, `core:window:allow-close`, `core:window:allow-start-dragging`, `core:window:allow-set-title`, `core:window:allow-is-maximized`, `core:window:allow-set-fullscreen`, `core:window:allow-is-fullscreen`.
- [ ] **Step 4:** Kiểm tra nhanh bằng harness trình duyệt (scratchpad `harness.html` mock `window.__TAURI__`): không lỗi console, đổi tab/theme/ngôn ngữ hoạt động.
- [ ] **Step 5: Commit** `UI: shell router + titlebar tu ve + hook main.js`

---

## Giai đoạn 2 — 8 agent song song (mỗi agent một git worktree, tách từ commit nền)

Mọi agent: commit bằng `git -c user.name="Tuyen" -c user.email="nguyen.ba@ics.vn" commit ...`; không sửa file ngoài phạm vi; báo cáo cuối gồm: danh sách key đã tạo, hàm nào dựng UI **bền** (cần re-render khi đổi ngôn ngữ), điểm nghi ngờ.

### Agent 1–6: chuyển chuỗi `main.js` sang `t()`

Khoảng dòng tính trên `main.js` **tại commit nền** (agent tự xác định lại ranh giới bằng tiêu đề section, không sửa ngoài vùng):

| Agent | Vùng (từ tiêu đề → trước tiêu đề) | Prefix key | File từ điển |
|---|---|---|---|
| 1 | đầu file → trước `// ===== Sửa Text box tại chỗ` | `viewer.` | `i18n/part-1.js` |
| 2 | `// ===== Sửa Text box tại chỗ` → trước `// ---------- Phase 3: Tổ chức trang` | `annot.` | `i18n/part-2.js` |
| 3 | `// ---------- Phase 3: Tổ chức trang` → trước `// ---------- Phase 4: Sửa nội dung` | `org.` | `i18n/part-3.js` |
| 4 | `// ---------- Phase 4: Sửa nội dung` → trước `// ---------- Phase 7: OCR` | `edit.` | `i18n/part-4.js` |
| 5 | `// ---------- Phase 7: OCR` → trước `// --- Chữ ký số` | `conv.` `form.` `sec.` | `i18n/part-5.js` |
| 6 | `// --- Chữ ký số` → hết file | `sig.` `ev.` | `i18n/part-6.js` |

Các bước (mỗi agent):
- [ ] Liệt kê mọi chuỗi hiển thị trong vùng: `"..."`, `'...'`, template literal, HTML trong `openModal(...)`, `confirm/alert/prompt`, `title=`, `placeholder=`, `textContent=`, `innerHTML=` (kể cả chuỗi không dấu nhưng là tiếng Việt, và chuỗi tiếng Anh lẫn như "Text box", "Flatten", "Thumbnails").
- [ ] Thay bằng `t("prefix.key", {vars})`; trong template HTML dùng `${t("...")}`; nối chuỗi `"Lỗi: " + e` → `t("prefix.errX", { e })`.
- [ ] Viết `i18n/part-N.js`: `I18N.add("vi", {...}); I18N.add("en", {...});` cùng tập key.
- [ ] Kiểm tra: `node -e "..."` load `i18n.js`-style dict và so tập key vi/en bằng nhau; `grep -nP "[àáảãạăâđèéêìíòóôơùúưỳýẽếềệ]" app/src/main.js` trong vùng chỉ còn dòng comment; `node --check app/src/main.js` PASS.
- [ ] Commit `i18n: main.js vung <ten> -> t()`.

### Agent 7: bộ icon + ribbon polish
- [ ] Viết đầy đủ `icons.js`: mọi tên `data-icon` xuất hiện trong `index.html`/`shell*.js` (lấy bằng `grep -ohE 'data-icon="[^"]+"|icon\("[^"]+"' app/src/*.js app/src/index.html | sort -u`). Phong cách line 24px, stroke 1.6, round cap/join, `currentColor`, nhất quán kiểu Lucide/Fluent, vẽ tay (không tải ngoài).
- [ ] Chỉ sửa khối `/* == ribbon == */` trong `styles.css` để đạt cảm giác ảnh mẫu (icon 24px, nhãn 12px, hover nền mềm, active accent-soft, ▾ cho dropdown).
- [ ] Commit `UI: bo icon SVG + tinh chinh ribbon`.

### Agent 8: shell-extras + script kiểm tra
- [ ] `shell-extras.js`: Files menu (mở/đóng, click ngoài, Esc, submenu), File gần đây (localStorage `ff.recent`, tối đa 10, bỏ file không mở được khi lỗi), Thuộc tính tài liệu (tên, đường dẫn, số trang, cỡ trang 1 theo mm), Phím tắt, Giới thiệu, Chế độ đọc (body.reading; Esc thoát), Toàn màn hình (`setFullscreen`), Thoát (`close`). Key prefix `x.`, từ điển trong chính file (`I18N.add` ở đầu file).
- [ ] `scripts/check-i18n.mjs`: nạp mọi `app/src/i18n/*.js` + `shell-extras.js` trong sandbox `vm` với stub `I18N.add`, so tập key vi/en; quét `t("...")` và `data-i18n*="..."` trong `app/src/**` — key nào thiếu → exit 1; quét `main.js` các dòng không phải comment còn ký tự có dấu → exit 1.
- [ ] `scripts/check-ids.mjs`: mọi `$("id")` / `getElementById("id")` trong `app/src/*.js` phải có `id="id"` trong `index.html` hoặc được tạo động (`id = "..."`/`id="..."` trong template JS) → nếu không exit 1.
- [ ] Commit `UI: Files menu, recent, doc props, reading/fullscreen + scripts kiem tra`.

---

## Giai đoạn 3 — Gộp & kiểm chứng

- [ ] Merge lần lượt 8 nhánh worktree vào `feat/premium-ribbon-ui`; giải xung đột (dự kiến không có).
- [ ] Thêm handler `Shell.onLangChange` cho các hàm dựng UI bền mà agent báo cáo.
- [ ] `node scripts/check-i18n.mjs` → PASS; `node scripts/check-ids.mjs` → PASS; `node --check` mọi file JS → PASS.
- [ ] `cargo test --workspace` → PASS (backend không đổi).
- [ ] Build app `cargo build --release` trong `app/src-tauri`, chạy với `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, qua CDP: chụp mỗi tab × {light, dark} × {vi, en}; chạy luồng mở file → highlight → lưu; Page → xoay → lưu; Edit → sửa chữ → lưu; Protect → đặt mật khẩu; Convert → xuất TXT. Không lỗi console.
- [ ] Agent review cuối (code-review) trên diff toàn nhánh; sửa phát hiện.
- [ ] Commit + báo cáo.
