// Files menu, tệp gần đây, thuộc tính tài liệu, phím tắt, giới thiệu, chế độ đọc, toàn màn hình.
I18N.add("vi", {
  "x.recentEmpty": "Chưa có tệp nào gần đây",
  "x.recentMissing": "Không mở được tệp — đã bỏ khỏi danh sách gần đây: {name}",
  "x.propsTitle": "Thuộc tính tài liệu",
  "x.propsName": "Tên tệp",
  "x.propsPath": "Đường dẫn",
  "x.propsPages": "Số trang",
  "x.propsSize": "Cỡ trang 1",
  "x.propsSizeVal": "{w} × {h} mm",
  "x.close": "Đóng",
  "x.shortcutsTitle": "Phím tắt",
  "x.kOpen": "Mở tệp PDF",
  "x.kSave": "Lưu thay đổi của chế độ hiện tại",
  "x.kFind": "Tìm trong tài liệu",
  "x.kFindNext": "Kết quả sau / trước",
  "x.kUndo": "Hoàn tác",
  "x.kRedo": "Làm lại",
  "x.kZoom": "Phóng to / thu nhỏ",
  "x.kZoomReset": "Cỡ thật (100%)",
  "x.kPage": "Trang trước / trang sau",
  "x.kFirstLast": "Trang đầu / trang cuối",
  "x.kDelete": "Xoá đối tượng đang chọn",
  "x.kBoldItalic": "Đậm / nghiêng phần bôi đen (khi sửa chữ)",
  "x.kEsc": "Huỷ thao tác, đóng hộp thoại, thoát chế độ đọc",
  "x.kFullscreen": "Toàn màn hình",
  "x.aboutTitle": "Giới thiệu FoFreeXit",
  "x.aboutTagline": "Trình đọc và chỉnh sửa PDF miễn phí, chạy hoàn toàn trên máy của bạn.",
  "x.aboutVersion": "Phiên bản {v}",
  "x.aboutStack": "Xây dựng trên Tauri, PDFium và QPDF.",
  "x.noDoc": "Hãy mở một tệp PDF trước",
});
I18N.add("en", {
  "x.recentEmpty": "No recent files",
  "x.recentMissing": "Could not open the file — removed from recent files: {name}",
  "x.propsTitle": "Document properties",
  "x.propsName": "File name",
  "x.propsPath": "Location",
  "x.propsPages": "Pages",
  "x.propsSize": "Page 1 size",
  "x.propsSizeVal": "{w} × {h} mm",
  "x.close": "Close",
  "x.shortcutsTitle": "Keyboard shortcuts",
  "x.kOpen": "Open PDF file",
  "x.kSave": "Save changes in the current mode",
  "x.kFind": "Search document",
  "x.kFindNext": "Next / previous result",
  "x.kUndo": "Undo",
  "x.kRedo": "Redo",
  "x.kZoom": "Zoom in / out",
  "x.kZoomReset": "Actual size (100%)",
  "x.kPage": "Previous / next page",
  "x.kFirstLast": "First / last page",
  "x.kDelete": "Delete selected object",
  "x.kBoldItalic": "Bold / italic selection (while editing text)",
  "x.kEsc": "Cancel, close dialog, exit reading mode",
  "x.kFullscreen": "Full screen",
  "x.aboutTitle": "About FoFreeXit",
  "x.aboutTagline": "A free PDF reader and editor that runs entirely on your computer.",
  "x.aboutVersion": "Version {v}",
  "x.aboutStack": "Built on Tauri, PDFium and QPDF.",
  "x.noDoc": "Open a PDF file first",
});

(function () {
  const TAURI_WIN = window.__TAURI__ && window.__TAURI__.window;
  const appWin = TAURI_WIN ? TAURI_WIN.getCurrentWindow() : null;
  const RECENT_KEY = "ff.recent";
  const RECENT_MAX = 10;

  const esc = (s) => String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const baseName = (p) => String(p).split(/[\\/]/).pop();
  const modalOpen = () => !$("modalOverlay").classList.contains("hidden");

  // ---------- Files menu ----------
  function toggleFilesMenu() {
    if (Shell.openMenuAt($("filesMenu"), $("filesBtn"), false)) {
      $("filesBtn").classList.add("open");
      renderRecent();
      applyIcons($("filesMenu"));
    }
  }
  $("filesBtn").addEventListener("click", toggleFilesMenu);
  // Submenu mở bằng click (ngoài hover) — tiện khi dùng bàn phím/màn hình cảm ứng.
  $("filesMenu").addEventListener("click", (e) => {
    const sub = e.target.closest(".has-sub");
    if (sub && !e.target.closest(".submenu")) {
      $("filesMenu").querySelectorAll(".has-sub.open").forEach((s) => { if (s !== sub) s.classList.remove("open"); });
      sub.classList.toggle("open");
    }
  });
  // Đóng menu (bởi shell.js hoặc bởi hành động) → gỡ trạng thái submenu mở.
  new MutationObserver(() => {
    if ($("filesMenu").classList.contains("hidden")) {
      $("filesBtn").classList.remove("open");
      $("filesMenu").querySelectorAll(".has-sub.open").forEach((s) => s.classList.remove("open"));
    }
  }).observe($("filesMenu"), { attributes: true, attributeFilter: ["class"] });

  // ---------- Tệp gần đây ----------
  function readRecent() {
    try {
      const v = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
      return Array.isArray(v) ? v.filter((p) => typeof p === "string" && p) : [];
    } catch (_) { return []; }
  }
  function writeRecent(list) {
    try { localStorage.setItem(RECENT_KEY, JSON.stringify(list.slice(0, RECENT_MAX))); } catch (_) {}
  }
  const samePath = (a, b) => a.toLowerCase() === b.toLowerCase(); // Windows: không phân biệt hoa/thường
  function addRecent(path) {
    writeRecent([path, ...readRecent().filter((p) => !samePath(p, path))]);
  }
  function removeRecent(path) {
    writeRecent(readRecent().filter((p) => !samePath(p, path)));
  }
  function renderRecent() {
    const menu = $("recentMenu");
    const list = readRecent();
    if (!list.length) {
      menu.innerHTML = `<div class="mempty">${esc(t("x.recentEmpty"))}</div>`;
      return;
    }
    menu.innerHTML = list.map((p) =>
      `<button class="mitem" data-path="${esc(p)}" title="${esc(p)}"><i data-icon="file-pdf"></i><span>${esc(baseName(p))}</span></button>`
    ).join("");
    applyIcons(menu);
  }

  // lastLoaded: tệp mở thành công gần nhất (state.path bị ghi đè cả khi mở lỗi).
  let lastLoaded = null;
  document.addEventListener("docloaded", (e) => {
    lastLoaded = e.detail && e.detail.path;
    if (lastLoaded) addRecent(lastLoaded);
  });
  $("recentMenu").addEventListener("click", async (e) => {
    const it = e.target.closest("[data-path]");
    if (!it) return;
    e.stopPropagation();
    Shell.closeMenus();
    // Còn thay đổi chưa lưu → hỏi trước khi mở tệp khác.
    if (!Shell.confirmDiscardChanges()) return;
    const path = it.dataset.path;
    const prev = lastLoaded;
    lastLoaded = null;
    const result = await loadDocument(path);
    // loadDocument trả "ok" | "cancelled" | "missing" | "error". Chỉ bỏ khỏi
    // danh sách khi tệp thật sự không còn (huỷ nhập mật khẩu thì giữ lại).
    if (!lastLoaded || !samePath(lastLoaded, path)) {
      if (!lastLoaded) lastLoaded = prev;
      if (result === "missing") {
        removeRecent(path);
        $("status").textContent = t("x.recentMissing", { name: baseName(path) });
      }
    }
  });

  // ---------- Thuộc tính tài liệu ----------
  $("docPropsItem").addEventListener("click", () => {
    Shell.closeMenus();
    // Hộp thoại đầy đủ, sửa được (features/docsec.js); bản chỉ-đọc dưới đây là dự phòng.
    if (window.DocSec) { DocSec.openProperties(); return; }
    const docPath = lastLoaded || state.path;
    if (!docPath || !state.pages.length) { $("status").textContent = t("x.noDoc"); return; }
    const p0 = state.pages[0];
    const mm = (pt) => (pt * 25.4 / 72).toFixed(1);
    const rows = [
      [t("x.propsName"), baseName(docPath)],
      [t("x.propsPath"), docPath],
      [t("x.propsPages"), String(state.pages.length)],
      [t("x.propsSize"), t("x.propsSizeVal", { w: mm(p0.widthPt), h: mm(p0.heightPt) })],
    ];
    const box = openModal(esc(t("x.propsTitle")), `
      <table class="kv">${rows.map(([k, v]) => `<tr><td>${esc(k)}</td><td style="word-break:break-all">${esc(v)}</td></tr>`).join("")}</table>
      <div class="foot"><button class="primary" data-x-close>${esc(t("x.close"))}</button></div>`);
    box.querySelector("[data-x-close]").addEventListener("click", closeModal);
  });

  // ---------- Phím tắt ----------
  const SHORTCUTS = [
    [["Ctrl+O"], "x.kOpen"],
    [["Ctrl+S"], "x.kSave"],
    [["Ctrl+F"], "x.kFind"],
    [["Enter", "Shift+Enter"], "x.kFindNext"],
    [["Ctrl+Z"], "x.kUndo"],
    [["Ctrl+Y", "Ctrl+Shift+Z"], "x.kRedo"],
    [["Ctrl++", "Ctrl+-"], "x.kZoom"],
    [["Ctrl+0"], "x.kZoomReset"],
    [["Page Up", "Page Down"], "x.kPage"],
    [["Home", "End"], "x.kFirstLast"],
    [["Delete"], "x.kDelete"],
    [["Ctrl+B", "Ctrl+I"], "x.kBoldItalic"],
    [["Esc"], "x.kEsc"],
    [["F11"], "x.kFullscreen"],
  ];
  $("helpShortcuts").addEventListener("click", () => {
    Shell.closeMenus();
    const rows = SHORTCUTS.map(([keys, k]) =>
      `<tr><td>${keys.map((x) => `<kbd>${esc(x)}</kbd>`).join(" ")}</td><td>${esc(t(k))}</td></tr>`).join("");
    const box = openModal(esc(t("x.shortcutsTitle")), `
      <table class="kv">${rows}</table>
      <div class="foot"><button class="primary" data-x-close>${esc(t("x.close"))}</button></div>`);
    box.querySelector("[data-x-close]").addEventListener("click", closeModal);
  });

  // ---------- Giới thiệu ----------
  $("helpAbout").addEventListener("click", async () => {
    Shell.closeMenus();
    let ver = "";
    try { if (window.__TAURI__ && window.__TAURI__.app) ver = await window.__TAURI__.app.getVersion(); } catch (_) {}
    const box = openModal(esc(t("x.aboutTitle")), `
      <p><b>FoFreeXit</b>${ver ? ` · <span class="muted">${esc(t("x.aboutVersion", { v: ver }))}</span>` : ""}</p>
      <p>${esc(t("x.aboutTagline"))}</p>
      <p class="muted">${esc(t("x.aboutStack"))}</p>
      <div class="foot"><button class="primary" data-x-close>${esc(t("x.close"))}</button></div>`);
    box.querySelector("[data-x-close]").addEventListener("click", closeModal);
  });

  // ---------- Chế độ đọc ----------
  function setReading(on) {
    document.body.classList.toggle("reading", on);
    $("readingBtn").classList.toggle("active", on);
  }
  $("readingBtn").addEventListener("click", () => setReading(!document.body.classList.contains("reading")));

  // ---------- Toàn màn hình ----------
  async function toggleFullscreen() {
    if (!appWin) return;
    try {
      const on = !(await appWin.isFullscreen());
      await appWin.setFullscreen(on);
      $("fullscreenBtn").classList.toggle("active", on);
    } catch (err) { console.error(err); }
  }
  $("fullscreenBtn").addEventListener("click", toggleFullscreen);
  if (!appWin) $("fullscreenBtn").disabled = true;

  // Capture: xét trạng thái TRƯỚC khi main.js/shell.js đóng hộp thoại/menu, để một lần Esc
  // chỉ đóng thứ trên cùng; chỉ thoát chế độ đọc khi không còn gì mở.
  window.addEventListener("keydown", (e) => {
    if (e.key === "F11") { e.preventDefault(); toggleFullscreen(); return; }
    if (e.key === "Escape" && document.body.classList.contains("reading") && !modalOpen() &&
        !document.querySelector(".menu:not(.submenu):not(.hidden)")) setReading(false);
  }, true);

  // ---------- Thoát ----------
  $("quitItem").addEventListener("click", () => {
    Shell.closeMenus();
    // close() phát CloseRequested → guard "thay đổi chưa lưu" trong shell.js.
    if (appWin) appWin.close();
  });
})();
