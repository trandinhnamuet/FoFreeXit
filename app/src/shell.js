// Shell: title bar tự vẽ, router tab ↔ chế độ của main.js, quick access,
// popover ngôn ngữ/theme, sidebar, thanh đáy. Không chứa logic nghiệp vụ —
// mọi thao tác đi qua đúng các nút/hàm có sẵn của main.js.
(function () {
  const TAURI_WIN = window.__TAURI__ && window.__TAURI__.window;
  const appWin = TAURI_WIN ? TAURI_WIN.getCurrentWindow() : null;

  // Tab → cờ chế độ trong state; chế độ → nút toggle cũ (main.js đã bind).
  const MODE_OF = { edit: "editMode", page: "organizeMode", form: "formMode", convert: "convMode", protect: "secMode" };
  const TOGGLE_OF = {
    editMode: "editModeBtn", organizeMode: "organizeModeBtn", formMode: "formModeBtn",
    convMode: "convModeBtn", secMode: "secModeBtn",
  };
  const TAB_OF_MODE = Object.fromEntries(Object.entries(MODE_OF).map(([tab, m]) => [m, tab]));
  const langHandlers = [];
  let current = "home";
  let routing = false;

  function syncTabs() {
    document.querySelectorAll(".tab-pill").forEach((b) => {
      const on = b.dataset.tab === current;
      b.classList.toggle("on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
    });
    document.querySelectorAll(".rpanel").forEach((p) => p.classList.toggle("on", p.dataset.tab === current));
  }

  function activeMode() {
    return Object.keys(TOGGLE_OF).find((m) => state[m]) || null;
  }

  function setTab(tab) {
    if (routing) return;
    const want = MODE_OF[tab] || null;
    if (state.editMode && want !== "editMode" && state.editUndo.length &&
        !confirm(t("shell.confirmLeaveEdit"))) { syncTabs(); return; }
    routing = true;
    try {
      for (const m of Object.keys(TOGGLE_OF)) if (m !== want && state[m]) $(TOGGLE_OF[m]).click();
      if (want && !state[want]) $(TOGGLE_OF[want]).click();
    } finally {
      routing = false;
    }
    // Công cụ chú thích chỉ có nghĩa ở Home/Annotate; Redact thuộc Protect.
    const keepTool = tab === "home" || tab === "annotate" || (tab === "protect" && state.tool === "redact");
    if (state.tool && !keepTool) setTool(state.tool);
    // Logic cũ từ chối vào chế độ (vd. chưa mở file) → giữ nguyên tab.
    if (!want || state[want]) current = tab;
    syncTabs();
  }

  // Chế độ bị bật/tắt từ nơi khác (đúp chuột vào chữ → Sửa, v.v.) → bám theo.
  function syncFromState() {
    if (routing) return;
    const m = activeMode();
    if (m && current !== TAB_OF_MODE[m]) current = TAB_OF_MODE[m];
    else if (!m && MODE_OF[current]) current = "home";
    syncTabs();
  }
  const modeObserver = new MutationObserver(syncFromState);
  for (const id of Object.values(TOGGLE_OF)) modeObserver.observe($(id), { attributes: true, attributeFilter: ["class"] });

  document.querySelectorAll(".tab-pill").forEach((b) => b.addEventListener("click", () => setTab(b.dataset.tab)));

  // ---------- Nút ủy quyền / điều hướng / hành động ----------
  document.addEventListener("click", (e) => {
    const el = e.target.closest && e.target.closest("[data-proxy],[data-tab-go],[data-action]");
    if (!el || el.disabled) return;
    if (el.dataset.proxy) {
      closeMenus();
      const target = $(el.dataset.proxy);
      if (target && !target.disabled) target.click();
    } else if (el.dataset.tabGo) {
      setTab(el.dataset.tabGo);
    } else if (el.dataset.action === "select") {
      if (state.tool) setTool(state.tool);
    } else if (el.dataset.action === "find") {
      $("searchBox").focus();
      $("searchBox").select();
    }
  });

  // ---------- Quick save (Ctrl+S): lưu đúng thứ của chế độ đang mở ----------
  function quickSave() {
    const id = state.editMode ? "edSave" : state.organizeMode ? "orgSave" : "saveAnnots";
    const btn = $(id);
    if (btn && !btn.disabled) btn.click();
    else $("status").textContent = t("shell.nothingToSave");
  }
  $("quickSave").addEventListener("click", quickSave);

  window.addEventListener("keydown", (e) => {
    if (!e.ctrlKey || e.altKey || e.shiftKey) return;
    if (!$("modalOverlay").classList.contains("hidden")) return;
    if (e.key === "s" || e.key === "S") { e.preventDefault(); quickSave(); }
    else if (e.key === "o" || e.key === "O") { e.preventDefault(); $("openBtn").click(); }
  });

  // ---------- Title bar ----------
  async function refreshMaxIcon() {
    if (!appWin) return;
    try {
      const max = await appWin.isMaximized();
      const ic = $("winMax").querySelector("i[data-icon]");
      ic.dataset.icon = max ? "win-restore" : "win-max";
      applyIcons($("winMax"));
      $("winMax").dataset.i18nTitle = max ? "shell.restore" : "shell.maximize";
      $("winMax").title = t($("winMax").dataset.i18nTitle);
    } catch (_) {}
  }
  if (appWin) {
    $("winMin").addEventListener("click", () => appWin.minimize());
    $("winMax").addEventListener("click", () => appWin.toggleMaximize());
    $("winClose").addEventListener("click", () => appWin.close());
    // Đúp vào vùng kéo = phóng to/khôi phục (chuẩn Windows).
    document.querySelector(".titlebar").addEventListener("dblclick", (e) => {
      if (!e.target.closest(".tb-controls")) appWin.toggleMaximize();
    });
    let rt = 0;
    window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(refreshMaxIcon, 120); });
    refreshMaxIcon();
  } else {
    document.querySelector(".tb-controls").style.visibility = "hidden";
  }

  // ---------- Menus (ngôn ngữ / theme; Files do shell-extras lo) ----------
  function closeMenus(except) {
    document.querySelectorAll(".menu:not(.submenu)").forEach((m) => { if (m !== except) m.classList.add("hidden"); });
    document.querySelectorAll(".files-btn.open").forEach((b) => { if (!except || except.id !== "filesMenu") b.classList.remove("open"); });
  }
  function openMenuAt(menu, anchor, alignRight) {
    const willOpen = menu.classList.contains("hidden");
    closeMenus();
    if (!willOpen) return false;
    menu.classList.remove("hidden");
    const r = anchor.getBoundingClientRect();
    menu.style.top = r.bottom + 6 + "px";
    if (alignRight) { menu.style.left = ""; menu.style.right = Math.max(8, window.innerWidth - r.right) + "px"; }
    else { menu.style.right = ""; menu.style.left = r.left + "px"; }
    return true;
  }
  document.addEventListener("mousedown", (e) => {
    if (!e.target.closest(".menu") && !e.target.closest("#filesBtn,#langBtn,#themeBtn")) closeMenus();
  });
  window.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenus(); });
  window.addEventListener("blur", () => closeMenus());

  function markChecked(menu, attr, val) {
    menu.querySelectorAll(".mitem").forEach((m) => m.classList.toggle("checked", m.getAttribute(attr) === val));
  }
  $("langBtn").addEventListener("click", () => {
    markChecked($("langMenu"), "data-lang", I18N.lang);
    openMenuAt($("langMenu"), $("langBtn"), true);
  });
  $("langMenu").addEventListener("click", (e) => {
    const it = e.target.closest("[data-lang]");
    if (!it) return;
    I18N.setLang(it.dataset.lang);
    closeMenus();
  });
  $("themeBtn").addEventListener("click", () => {
    markChecked($("themeMenu"), "data-theme-set", Theme.pref);
    openMenuAt($("themeMenu"), $("themeBtn"), true);
  });
  $("themeMenu").addEventListener("click", (e) => {
    const it = e.target.closest("[data-theme-set]");
    if (!it) return;
    Theme.set(it.dataset.themeSet);
    closeMenus();
  });
  function refreshThemeIcon() {
    const ic = $("themeIcon");
    ic.dataset.icon = "theme-" + Theme.pref;
    applyIcons($("themeBtn"));
  }
  document.addEventListener("themechange", refreshThemeIcon);

  // ---------- Thu gọn ribbon ----------
  function setRibbonCollapsed(on) {
    document.body.classList.toggle("ribbon-collapsed", on);
    const b = $("collapseRibbonBtn");
    b.querySelector("i[data-icon]").dataset.icon = on ? "chevron-down" : "chevron-up";
    b.dataset.i18nTitle = on ? "shell.expandRibbon" : "shell.collapseRibbon";
    b.title = t(b.dataset.i18nTitle);
    applyIcons(b);
    try { localStorage.setItem("ff.ribbonCollapsed", on ? "1" : "0"); } catch (_) {}
  }
  $("collapseRibbonBtn").addEventListener("click", () =>
    setRibbonCollapsed(!document.body.classList.contains("ribbon-collapsed")));
  // Bấm tab khi ribbon đang thu gọn → mở lại.
  document.querySelectorAll(".tab-pill").forEach((b) => b.addEventListener("dblclick", () =>
    setRibbonCollapsed(!document.body.classList.contains("ribbon-collapsed"))));

  // ---------- Sidebar ----------
  function setSidebarCollapsed(on) {
    document.body.classList.toggle("sb-collapsed", on);
    try { localStorage.setItem("ff.sidebarCollapsed", on ? "1" : "0"); } catch (_) {}
  }
  $("sidebarToggle").addEventListener("click", () =>
    setSidebarCollapsed(!document.body.classList.contains("sb-collapsed")));
  for (const id of ["tabThumbs", "tabOutline", "tabComments"]) {
    $(id).addEventListener("click", () => setSidebarCollapsed(false));
  }

  // ---------- Thanh đáy ----------
  $("pageFirst").addEventListener("click", () => { if (state.pages.length) goToPage(0); });
  $("pageLast").addEventListener("click", () => { if (state.pages.length) goToPage(state.pages.length - 1); });
  // Thông điệp trạng thái dài bị cắt → tooltip hiện đầy đủ.
  new MutationObserver(() => { $("status").title = $("status").textContent; })
    .observe($("status"), { childList: true, characterData: true, subtree: true });

  // ---------- Ngôn ngữ ----------
  function refreshLangLabel() { $("langLabel").textContent = I18N.lang.toUpperCase(); }
  document.addEventListener("langchange", () => {
    refreshLangLabel();
    for (const fn of langHandlers) { try { fn(); } catch (err) { console.error(err); } }
  });

  // Đổi ngôn ngữ: dựng lại các phần UI bền do main.js vẽ (thông điệp tạm
  // thời giữ nguyên đến lần hiển thị sau).
  function docInfoIn(lang) {
    const s = I18N.dict[lang]["viewer.docInfo"] || "";
    return s.replace("{n}", state.pages.length).replace("{name}", state.path ? shortName(state.path) : "");
  }
  langHandlers.push(() => {
    buildComments();
    redrawAllAnnotPages();
    if (state.outline) buildOutline(state.outline);
    closeNotePopup();
    closeColorPopover();
    const other = I18N.lang === "vi" ? "en" : "vi";
    if (state.path && $("status").textContent === docInfoIn(other)) $("status").textContent = docInfoIn(I18N.lang);
    if (state.tool) { const tl = state.tool; setTool(tl); setTool(tl); } // setTool là toggle: tắt rồi bật để vẽ lại hint
    if (state.formMode && state.path) refreshFormCount();
    if (state.convMode) { toggleConvMode(); toggleConvMode(); }
    if (state.editMode && state.editArm === "text") $("editHint").textContent = t("ev.addTextHint");
    if (state.editMode && state.editSel != null) selectEditObject(state.editSel, state.editSelRuns);
  });

  // ---------- Tài liệu ----------
  function onDocLoaded(path) {
    const name = path.split(/[\\/]/).pop();
    $("winTitle").textContent = `FoFreeXit — ${name}`;
    document.title = `FoFreeXit — ${name}`;
    if (appWin) appWin.setTitle(`FoFreeXit — ${name}`).catch(() => {});
    document.dispatchEvent(new CustomEvent("docloaded", { detail: { path } }));
  }

  window.Shell = {
    setTab,
    get currentTab() { return current; },
    onDocLoaded,
    onLangChange(fn) { langHandlers.push(fn); },
    closeMenus,
    openMenuAt,
  };

  // ---------- Khởi tạo ----------
  applyI18n(document);
  applyIcons(document);
  refreshLangLabel();
  refreshThemeIcon();
  try {
    if (localStorage.getItem("ff.ribbonCollapsed") === "1") setRibbonCollapsed(true);
    if (localStorage.getItem("ff.sidebarCollapsed") === "1") setSidebarCollapsed(true);
  } catch (_) {}
  syncTabs();
})();
