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
  const LOADS_DOC = new Set(["orgMerge", "secOptimize"]);
  let current = "home";
  let routing = false;

  function syncTabs() {
    document.querySelectorAll(".tab-pill").forEach((b) => {
      const on = b.dataset.tab === current;
      b.classList.toggle("on", on);
      b.setAttribute("aria-selected", on ? "true" : "false");
      if (on) b.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
    document.querySelectorAll(".rpanel").forEach((p) => p.classList.toggle("on", p.dataset.tab === current));
  }

  function activeMode() {
    return Object.keys(TOGGLE_OF).find((m) => state[m]) || null;
  }

  // Còn thay đổi nội dung chưa lưu (kể cả thao tác đang áp dụng dở)?
  function editDirty() {
    return state.editMode &&
      (state.editUndo.length > 0 || (state.editPending || 0) > 0 || (state.editBase && state.editBase !== state.path));
  }
  // true = được phép bỏ chế độ sửa (không có gì chưa lưu, hoặc người dùng đồng ý bỏ).
  function confirmDiscardEdits() {
    return !editDirty() || confirm(t("shell.confirmLeaveEdit"));
  }

  // Thay đổi CHƯA LƯU của tài liệu đang mở mà việc mở tệp khác / đóng cửa sổ
  // sẽ làm mất: sửa nội dung, chú thích, tổ chức trang, vùng bôi đen.
  function orgDirty() {
    // Plan tạm (trước khi organize_identity_plan trả về) chưa có 'kind' → chưa đổi gì.
    return typeof planIsDirty === "function" && state.pages.length > 0 &&
      Array.isArray(state.pagePlan) && !state.pagePlan.some((e) => e.kind === undefined) && planIsDirty();
  }
  function dirtyParts() {
    const parts = [];
    if (editDirty()) parts.push(t("shell.dirtyEdit"));
    if (state.annotSpecs.length > 0 && !$("saveAnnots").disabled) parts.push(t("shell.dirtyAnnots", { n: state.annotSpecs.length }));
    if (orgDirty()) parts.push(t("shell.dirtyPages"));
    if (state.redactMarks.length > 0) parts.push(t("shell.dirtyRedact", { n: state.redactMarks.length }));
    if (window.Bookmarks && Bookmarks.isDirty()) parts.push(t("bm.dirtyPart"));
    return parts;
  }
  // true = không có gì chưa lưu, hoặc người dùng đồng ý bỏ (một hộp xác nhận duy nhất).
  function confirmDiscardChanges() {
    const parts = dirtyParts();
    return !parts.length || confirm(t("shell.confirmDiscard", { list: parts.map((p) => "• " + p).join("\n") }));
  }

  function setTab(tab) {
    if (routing) return;
    const want = MODE_OF[tab] || null;
    if (state.editMode && want !== "editMode" && !confirmDiscardEdits()) { syncTabs(); return; }
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
    mirrorHint(); // rời Annotate khi đang có hint → hiện nó ở thanh trạng thái
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
      // Gộp PDF / Nén: hỏi trước ở guard capture của chính nút đích (bên dưới).
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
    const redactReady = !$("secRedactApply").disabled;
    const id = state.editMode ? "edSave"
      : state.organizeMode ? "orgSave"
      : (current === "protect" || state.secMode) && redactReady ? "secRedactApply"
      // Không có chú thích nào nhưng còn vùng bôi đen chờ áp dụng → áp dụng chúng.
      : $("saveAnnots").disabled && redactReady ? "secRedactApply"
      // Chỉ còn bookmark/liên kết chưa lưu → lưu chúng (features/bookmarks.js).
      : $("saveAnnots").disabled && window.Bookmarks && Bookmarks.isDirty() ? "bmSave"
      : "saveAnnots";
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
    // close() phát CloseRequested như nút đóng của Windows → đi qua guard bên dưới.
    $("winClose").addEventListener("click", () => appWin.close());
    // Đúp vào vùng kéo (khoảng trống hàng tab) = phóng to/khôi phục: script
    // drag-region của Tauri đã tự làm — không thêm handler (sẽ nháy).
    let rt = 0;
    window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(refreshMaxIcon, 120); });
    refreshMaxIcon();
  }

  // ---------- Đóng cửa sổ: hỏi nếu còn thay đổi chưa lưu ----------
  // Có listener JS cho "tauri://close-requested" thì Rust luôn chặn việc đóng
  // (api.prevent_close) và để JS quyết định — nút X, Alt+F4, close() của nút
  // riêng / mục Thoát đều qua đây. Không dùng onCloseRequested vì wrapper đó
  // gọi destroy() và nếu thiếu quyền core:window:allow-destroy thì cửa sổ không
  // bao giờ đóng được; ở đây destroy lỗi → gỡ listener rồi close() lại (không
  // còn listener = Rust đóng bình thường, chỉ cần allow-close).
  if (appWin && typeof appWin.listen === "function") {
    const EV = (window.__TAURI__.event && window.__TAURI__.event.TauriEvent &&
      window.__TAURI__.event.TauriEvent.WINDOW_CLOSE_REQUESTED) || "tauri://close-requested";
    let unlistenClose = null;
    let closing = false;
    appWin.listen(EV, async () => {
      if (closing) return;
      if (!confirmDiscardChanges()) return; // đã bị Rust chặn → cửa sổ ở lại
      closing = true;
      try {
        await appWin.destroy();
      } catch (_) {
        try {
          if (unlistenClose) { const un = unlistenClose; unlistenClose = null; await un(); }
          await appWin.close();
        } catch (err) { console.error(err); closing = false; }
      }
    }).then((un) => { unlistenClose = un; }).catch((err) => console.error(err));
  }
  // Chỉ hiện nút cửa sổ riêng khi không có thanh tiêu đề của Windows; lỗi
  // (thiếu quyền, chạy ngoài Tauri) → coi như có thanh gốc, không vẽ trùng.
  (async () => {
    let decorated = true;
    try { if (appWin) decorated = await appWin.isDecorated(); } catch (_) {}
    $("winControls").hidden = decorated;
    document.body.classList.toggle("frameless", !decorated);
  })();

  // ---------- Menus (ngôn ngữ / theme; Files do shell-extras lo) ----------
  function closeMenus(except) {
    document.querySelectorAll(".menu:not(.submenu)").forEach((m) => { if (m !== except) m.classList.add("hidden"); });
    document.querySelectorAll("#langBtn.active, #themeBtn.active").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".files-btn.open").forEach((b) => { if (!except || except.id !== "filesMenu") b.classList.remove("open"); });
  }
  function openMenuAt(menu, anchor, alignRight) {
    const willOpen = menu.classList.contains("hidden");
    closeMenus();
    if (!willOpen) return false;
    menu.classList.remove("hidden");
    if (anchor.id === "langBtn" || anchor.id === "themeBtn") anchor.classList.add("active");
    const r = anchor.getBoundingClientRect();
    menu.style.top = r.bottom + 6 + "px";
    if (alignRight) { menu.style.left = ""; menu.style.right = Math.max(8, window.innerWidth - r.right) + "px"; }
    else { menu.style.right = ""; menu.style.left = r.left + "px"; }
    return true;
  }
  document.addEventListener("mousedown", (e) => {
    if (!e.target.closest(".menu") && !e.target.closest("#filesBtn,#langBtn,#themeBtn")) closeMenus();
  });
  // Esc khi đang mở menu: chỉ đóng menu (không để main.js bỏ công cụ đang chọn).
  window.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (document.querySelector(".menu:not(.submenu):not(.hidden)")) {
      closeMenus();
      e.stopImmediatePropagation();
    }
  }, true);
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
  // Bấm tab khi ribbon đang thu gọn → mở lại; đúp tab = thu gọn/mở (như Office).
  document.querySelectorAll(".tab-pill").forEach((b) => {
    b.addEventListener("click", () => {
      if (document.body.classList.contains("ribbon-collapsed")) setRibbonCollapsed(false);
    });
    b.addEventListener("dblclick", () => setRibbonCollapsed(!document.body.classList.contains("ribbon-collapsed")));
  });

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

  // Hướng dẫn công cụ (setTool ghi vào #annotHint nằm trong panel Annotate):
  // chọn Highlight/Crop từ Home hay Redact từ Protect thì panel đó không hiện →
  // chép sang thanh trạng thái. Xoá hint → trả lại trạng thái cũ, nhưng chỉ khi
  // thanh trạng thái vẫn đang hiện đúng hint đó (không xoá thông điệp khác).
  let hintMirror = null; // { text, prev }
  function mirrorHint() {
    const txt = $("annotHint").textContent;
    const st = $("status");
    const hidden = current !== "annotate" || document.body.classList.contains("ribbon-collapsed");
    if (txt && hidden) {
      const prev = hintMirror && st.textContent === hintMirror.text ? hintMirror.prev : st.textContent;
      hintMirror = { text: txt, prev };
      if (st.textContent !== txt) st.textContent = txt;
    } else if (!txt && hintMirror) {
      if (st.textContent === hintMirror.text) st.textContent = hintMirror.prev;
      hintMirror = null;
    }
  }
  new MutationObserver(mirrorHint)
    .observe($("annotHint"), { childList: true, characterData: true, subtree: true });

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
  // Dịch lại một thông điệp tạm thời nếu nó đúng nguyên văn một chuỗi KHÔNG có
  // tham số của ngôn ngữ cũ (hoặc dòng thông tin tài liệu). Chuỗi có tham số
  // ({e}, {name}…) không khôi phục được tham số → giữ nguyên đến lần hiện sau.
  function retranslateText(txt, other) {
    if (!txt) return null;
    if (state.path && txt === docInfoIn(other)) return docInfoIn(I18N.lang);
    const src = I18N.dict[other];
    for (const k in src) if (src[k] === txt && !/\{\w+\}/.test(txt)) return t(k);
    return null;
  }
  langHandlers.push(() => {
    buildComments();
    redrawAllAnnotPages();
    if (state.outline) buildOutline(state.outline);
    closeNotePopup();
    closeColorPopover();
    const other = I18N.lang === "vi" ? "en" : "vi";
    const showingMirror = hintMirror && $("status").textContent === hintMirror.text;
    for (const id of ["status", "editHint", "organizeHint", "formHint", "convHint", "secHint"]) {
      const nt = retranslateText($(id).textContent, other);
      if (nt != null) $(id).textContent = nt;
    }
    if (hintMirror) {
      if (showingMirror) hintMirror.text = $("status").textContent;
      const np = retranslateText(hintMirror.prev, other);
      if (np != null) hintMirror.prev = np;
    }
    if (state.tool) { const tl = state.tool; setTool(tl); setTool(tl); } // setTool là toggle: tắt rồi bật để vẽ lại hint
    if (state.formMode && state.path) refreshFormCount();
    if (state.convMode) { toggleConvMode(); toggleConvMode(); }
    if (state.editMode && state.editArm === "text") $("editHint").textContent = t("ev.addTextHint");
    if (state.editMode && state.editSel != null) selectEditObject(state.editSel, state.editSelRuns);
  });

  // ---------- Tài liệu ----------
  // Mở tệp khác khi đang ở chế độ Sửa: hỏi trước (capture — chạy trước openFile
  // của main.js), mở xong thì thoát chế độ sửa (editBase còn trỏ tệp cũ).
  // Cùng kiểu guard cho Gộp PDF / Nén (mở tệp kết quả) — bắt cả khi bấm trực
  // tiếp lẫn qua nút ủy quyền (proxy gọi target.click()).
  function guardLoad(e) {
    if (!confirmDiscardChanges()) { e.stopImmediatePropagation(); e.preventDefault(); }
  }
  $("openBtn").addEventListener("click", guardLoad, true);
  for (const id of LOADS_DOC) $(id).addEventListener("click", guardLoad, true);

  function onDocLoaded(path) {
    if (state.editMode) exitEditMode();
    const name = path.split(/[\\/]/).pop();
    document.title = `FoFreeXit — ${name}`;
    if (appWin) appWin.setTitle(`FoFreeXit — ${name}`).catch(() => {});
    document.dispatchEvent(new CustomEvent("docloaded", { detail: { path } }));
  }

  window.Shell = {
    setTab,
    get currentTab() { return current; },
    onDocLoaded,
    onLangChange(fn) { langHandlers.push(fn); },
    confirmDiscardEdits,     // chỉ chế độ Sửa (đổi tab)
    confirmDiscardChanges,   // mọi thay đổi chưa lưu (mở tệp khác, kéo-thả, đóng cửa sổ)
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
