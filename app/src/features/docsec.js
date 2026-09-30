// Bảo mật & làm sạch tài liệu (docsec): Tìm và che (Search & Redact), giao diện
// vùng che, che nguyên trang, Làm sạch tài liệu (Sanitize / Remove Hidden
// Information), bảng Tệp đính kèm, Thuộc tính tài liệu (sửa được), Trợ năng
// (Kiểm tra đầy đủ / nhanh, sửa tự động, văn bản thay thế).
// Dùng biến/hàm toàn cục của main.js: invoke, $, state, t, openModal, closeModal,
// confirmModal, loadDocument, shortName, parsePageRange, applyIcons,
// openColorPopover, rgbCss, goToPage, drawAnnotsForPage, updateRedactButtons.
(function () {
  const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const status = (msg) => { $("status").textContent = msg; };
  // Hộp thoại rộng: bọc nội dung trong .ds-dlg (CSS :has) và trả về chính
  // phần tử bọc — listener gắn vào đó mất theo hộp thoại, không dính lại
  // #modalBox (dùng chung) cho hộp thoại khác.
  const openWide = (title, html) => openModal(title, `<div class="ds-dlg">${html}</div>`).querySelector(".ds-dlg");
  const needDoc = () => {
    if (!state.path || !state.pages.length) { status(t("ds.noDoc")); return false; }
    return true;
  };
  const fmtSize = (n) => {
    if (n == null) return "—";
    if (n < 1024) return t("ds.bytes", { n });
    if (n < 1024 * 1024) return t("ds.kb", { n: (n / 1024).toFixed(1) });
    return t("ds.mb", { n: (n / 1024 / 1024).toFixed(2) });
  };
  const fmtDate = (iso) => {
    if (!iso) return "—";
    const d = new Date(iso);
    return isNaN(d) ? iso : d.toLocaleString(I18N.lang === "vi" ? "vi-VN" : "en-US");
  };
  // Tệp gốc người dùng mở (state.path có thể là bản giải mã tạm).
  let originalPath = null;
  document.addEventListener("docloaded", (e) => {
    originalPath = e.detail && e.detail.path;
    Attach.reload();
  });
  const docName = () => shortName(originalPath || state.path || "");

  // ==================================================================
  // 1. Giao diện vùng che (Redaction appearance)
  // ==================================================================
  const STYLE_KEY = "ff.redactStyle";
  const defaultStyle = () => ({
    fill: [0, 0, 0], useText: false, text: t("ds.rsDefaultText"), textColor: [255, 255, 255],
    autoSize: true, fontSize: 10, repeat: false, align: "center",
  });
  function loadStyle() {
    try {
      const v = JSON.parse(localStorage.getItem(STYLE_KEY) || "null");
      if (v && typeof v === "object") return Object.assign(defaultStyle(), v);
    } catch (_) {}
    return defaultStyle();
  }
  function saveStyle(s) { try { localStorage.setItem(STYLE_KEY, JSON.stringify(s)); } catch (_) {} }

  // Gọi từ applyRedactions (main.js).
  function redactStyle() {
    const s = loadStyle();
    return {
      fill: s.fill,
      overlayText: s.useText && s.text.trim() ? s.text.trim() : null,
      textColor: s.textColor,
      fontSize: s.autoSize ? null : Number(s.fontSize) || null,
      repeat: !!s.repeat,
      align: s.align,
    };
  }

  function openRedactOptions() {
    const s = loadStyle();
    const box = openModal(esc(t("ds.rsTitle")), `
      <div class="ds-rs">
        <label>${esc(t("ds.rsFill"))}</label>
        <button id="dsRsFill" class="ds-colorbtn"><span class="sw" id="dsRsFillSw"></span><span>${esc(t("ds.rsPick"))}</span></button>
        <label><input type="checkbox" id="dsRsUseText"> ${esc(t("ds.rsUseText"))}</label>
        <input type="text" id="dsRsText" maxlength="200">
        <div class="row">
          <div><label>${esc(t("ds.rsTextColor"))}</label>
            <button id="dsRsTextColor" class="ds-colorbtn"><span class="sw" id="dsRsTextSw"></span><span>${esc(t("ds.rsPick"))}</span></button></div>
          <div><label>${esc(t("ds.rsAlign"))}</label>
            <select id="dsRsAlign">
              <option value="left">${esc(t("ds.rsAlignLeft"))}</option>
              <option value="center">${esc(t("ds.rsAlignCenter"))}</option>
              <option value="right">${esc(t("ds.rsAlignRight"))}</option>
            </select></div>
        </div>
        <div class="row">
          <label><input type="checkbox" id="dsRsAuto"> ${esc(t("ds.rsAutoSize"))}</label>
          <div><input type="number" id="dsRsSize" min="4" max="72" step="1" data-i18n-title="ds.rsSizeTip" title="${esc(t("ds.rsSizeTip"))}"></div>
        </div>
        <label><input type="checkbox" id="dsRsRepeat"> ${esc(t("ds.rsRepeat"))}</label>
        <label>${esc(t("ds.rsPreview"))}</label>
        <div class="ds-rs-preview" id="dsRsPreview"></div>
        <p class="muted">${esc(t("ds.rsNote"))}</p>
      </div>
      <div class="foot"><button id="dsRsReset">${esc(t("ds.rsReset"))}</button><button id="dsRsCancel">${esc(t("common.cancel"))}</button><button id="dsRsOk" class="primary">${esc(t("common.ok"))}</button></div>`);
    const cur = Object.assign({}, s);
    const q = (id) => box.querySelector("#" + id);
    function sync() {
      q("dsRsFillSw").style.background = rgbCss(cur.fill);
      q("dsRsTextSw").style.background = rgbCss(cur.textColor);
      q("dsRsUseText").checked = cur.useText;
      q("dsRsText").value = cur.text;
      q("dsRsAlign").value = cur.align;
      q("dsRsAuto").checked = cur.autoSize;
      q("dsRsSize").value = cur.fontSize;
      q("dsRsRepeat").checked = cur.repeat;
      for (const id of ["dsRsText", "dsRsAlign", "dsRsAuto", "dsRsRepeat"]) q(id).disabled = !cur.useText;
      q("dsRsTextColor").disabled = !cur.useText;
      q("dsRsSize").disabled = !cur.useText || cur.autoSize;
      const pv = q("dsRsPreview");
      pv.style.background = rgbCss(cur.fill);
      pv.style.color = rgbCss(cur.textColor);
      pv.style.justifyContent = cur.align === "left" ? "flex-start" : cur.align === "right" ? "flex-end" : "center";
      pv.style.fontSize = cur.autoSize ? "" : Math.max(8, Math.min(28, cur.fontSize)) + "px";
      const txt = cur.useText ? cur.text : "";
      pv.textContent = cur.useText && cur.repeat ? Array(8).fill(txt).join(" ") : txt;
      pv.classList.toggle("rep", cur.useText && cur.repeat);
    }
    q("dsRsFill").addEventListener("click", () => openColorPopover(q("dsRsFill"), cur.fill, (rgb) => { cur.fill = rgb.slice(); sync(); }));
    q("dsRsTextColor").addEventListener("click", () => openColorPopover(q("dsRsTextColor"), cur.textColor, (rgb) => { cur.textColor = rgb.slice(); sync(); }));
    q("dsRsUseText").addEventListener("change", (e) => { cur.useText = e.target.checked; sync(); });
    q("dsRsText").addEventListener("input", (e) => { cur.text = e.target.value; sync(); });
    q("dsRsAlign").addEventListener("change", (e) => { cur.align = e.target.value; sync(); });
    q("dsRsAuto").addEventListener("change", (e) => { cur.autoSize = e.target.checked; sync(); });
    q("dsRsSize").addEventListener("input", (e) => { cur.fontSize = Number(e.target.value) || 10; sync(); });
    q("dsRsRepeat").addEventListener("change", (e) => { cur.repeat = e.target.checked; sync(); });
    q("dsRsReset").addEventListener("click", () => { Object.assign(cur, defaultStyle()); sync(); });
    q("dsRsCancel").addEventListener("click", closeModal);
    q("dsRsOk").addEventListener("click", () => { saveStyle(cur); closeModal(); status(t("ds.rsSaved")); });
    sync();
  }

  // ==================================================================
  // 2. Che nguyên trang
  // ==================================================================
  function addWholePageMarks(indices) {
    let n = 0;
    for (const idx of indices) {
      const p = state.pages[idx];
      if (!p || state.redactMarks.some((m) => m.page === idx && m.whole)) continue;
      state.redactMarks.push({ page: idx, whole: true, rect: { left: 0, bottom: 0, right: p.widthPt, top: p.heightPt } });
      drawAnnotsForPage(idx);
      n++;
    }
    updateRedactButtons();
    return n;
  }

  function openMarkPages() {
    if (!needDoc()) return;
    const cur = (state.current || 0) + 1;
    const box = openModal(esc(t("ds.mpTitle")), `
      <p class="muted">${esc(t("ds.mpIntro"))}</p>
      <div class="radiorow">
        <label><input type="radio" name="dsMp" value="cur" checked> ${esc(t("ds.mpCurrent", { n: cur }))}</label>
        <label><input type="radio" name="dsMp" value="range"> ${esc(t("ds.mpRange"))}</label>
      </div>
      <input type="text" id="dsMpRange" placeholder="${esc(t("ds.mpRangePh"))}" disabled>
      <div class="err" id="dsMpErr"></div>
      <div class="foot"><button id="dsMpCancel">${esc(t("common.cancel"))}</button><button id="dsMpOk" class="primary">${esc(t("ds.mpOk"))}</button></div>`);
    const range = box.querySelector("#dsMpRange");
    box.querySelectorAll("input[name=dsMp]").forEach((r) => r.addEventListener("change", () => {
      range.disabled = box.querySelector("input[name=dsMp]:checked").value !== "range";
      if (!range.disabled) range.focus();
    }));
    box.querySelector("#dsMpCancel").addEventListener("click", closeModal);
    box.querySelector("#dsMpOk").addEventListener("click", () => {
      const mode = box.querySelector("input[name=dsMp]:checked").value;
      const idx = mode === "cur" ? [cur - 1] : parsePageRange(range.value, state.pages.length);
      if (!idx.length) { box.querySelector("#dsMpErr").textContent = t("ds.mpBad"); return; }
      closeModal();
      const n = addWholePageMarks([...new Set(idx)]);
      status(t("ds.mpDone", { n }));
      goToPage(idx[0]);
    });
  }

  // ==================================================================
  // 3. Tìm và che (Search & Redact)
  // ==================================================================
  const PATTERNS = ["email", "phone", "cccd", "cmnd", "taxCode", "bankCard", "iban", "date", "url", "ipv4"];
  let srLast = { mode: "terms", terms: "", regex: "", patterns: ["email", "phone"], matchCase: false, wholeWord: false, scope: "all", range: "" };

  function openSearchRedact() {
    if (!needDoc()) return;
    const s = srLast;
    const box = openWide(esc(t("ds.srTitle")), `
      <div class="ds-sr">
        <div class="ds-seg" role="tablist">
          <button data-mode="terms">${esc(t("ds.srModeTerms"))}</button>
          <button data-mode="patterns">${esc(t("ds.srModePatterns"))}</button>
          <button data-mode="regex">${esc(t("ds.srModeRegex"))}</button>
        </div>
        <div data-pane="terms">
          <label>${esc(t("ds.srTermsLabel"))}</label>
          <textarea id="dsSrTerms" rows="4" placeholder="${esc(t("ds.srTermsPh"))}"></textarea>
        </div>
        <div data-pane="patterns">
          <label>${esc(t("ds.srPatternsLabel"))}</label>
          <div class="ds-grid">${PATTERNS.map((p) => `<label><input type="checkbox" data-pat="${p}"> ${esc(t("ds.pat." + p))}</label>`).join("")}</div>
        </div>
        <div data-pane="regex">
          <label>${esc(t("ds.srRegexLabel"))}</label>
          <input type="text" id="dsSrRegex" placeholder="${esc(t("ds.srRegexPh"))}" spellcheck="false">
        </div>
        <div class="row ds-opts">
          <label><input type="checkbox" id="dsSrCase"> ${esc(t("ds.srMatchCase"))}</label>
          <label><input type="checkbox" id="dsSrWord"> ${esc(t("ds.srWholeWord"))}</label>
        </div>
        <div class="row ds-scope">
          <div><label>${esc(t("ds.srScope"))}</label>
            <select id="dsSrScope">
              <option value="all">${esc(t("ds.srScopeAll"))}</option>
              <option value="cur">${esc(t("ds.srScopeCur"))}</option>
              <option value="range">${esc(t("ds.srScopeRange"))}</option>
            </select></div>
          <div><label>&nbsp;</label><input type="text" id="dsSrRange" placeholder="${esc(t("ds.mpRangePh"))}"></div>
        </div>
        <div class="ds-results-head">
          <label><input type="checkbox" id="dsSrAll"> <span id="dsSrCount">${esc(t("ds.srNoSearch"))}</span></label>
        </div>
        <div class="ds-results" id="dsSrResults"></div>
        <div class="err" id="dsSrErr"></div>
      </div>
      <div class="foot"><button id="dsSrClose">${esc(t("ds.close"))}</button><button id="dsSrFind"><i data-icon="search"></i>${esc(t("ds.srFind"))}</button><button id="dsSrMark" class="primary" disabled><i data-icon="redact"></i>${esc(t("ds.srMark"))}</button></div>`);
    const q = (id) => box.querySelector("#" + id);
    let mode = s.mode;
    let hits = [];
    function setMode(m) {
      mode = m;
      box.querySelectorAll(".ds-seg button").forEach((b) => b.classList.toggle("on", b.dataset.mode === m));
      box.querySelectorAll("[data-pane]").forEach((p) => { p.hidden = p.dataset.pane !== m; });
      q("dsSrWord").disabled = m !== "terms";
      q("dsSrCase").disabled = m === "patterns";
    }
    box.querySelectorAll(".ds-seg button").forEach((b) => b.addEventListener("click", () => setMode(b.dataset.mode)));
    q("dsSrTerms").value = s.terms;
    q("dsSrRegex").value = s.regex;
    box.querySelectorAll("[data-pat]").forEach((c) => { c.checked = s.patterns.includes(c.dataset.pat); });
    q("dsSrCase").checked = s.matchCase;
    q("dsSrWord").checked = s.wholeWord;
    q("dsSrScope").value = s.scope;
    q("dsSrRange").value = s.range;
    const syncScope = () => { q("dsSrRange").disabled = q("dsSrScope").value !== "range"; };
    q("dsSrScope").addEventListener("change", syncScope);
    syncScope();
    setMode(mode);

    function selected() { return [...box.querySelectorAll("[data-hit]")].filter((c) => c.checked).map((c) => hits[Number(c.dataset.hit)]); }
    function syncSel() {
      const n = selected().length;
      q("dsSrMark").disabled = n === 0;
      q("dsSrAll").checked = hits.length > 0 && n === hits.length;
      q("dsSrAll").indeterminate = n > 0 && n < hits.length;
      q("dsSrCount").textContent = hits.length ? t("ds.srCount", { n: hits.length, k: n }) : t("ds.srNone");
    }
    function render() {
      const byPage = new Map();
      hits.forEach((h, i) => { if (!byPage.has(h.pageIndex)) byPage.set(h.pageIndex, []); byPage.get(h.pageIndex).push(i); });
      q("dsSrResults").innerHTML = [...byPage.entries()].map(([p, idx]) => `
        <div class="ds-rgroup"><div class="ds-rpage">${esc(t("ds.pageN", { n: p + 1 }))} <span class="muted">(${idx.length})</span></div>
        ${idx.map((i) => {
          const h = hits[i];
          return `<label class="ds-hit"><input type="checkbox" data-hit="${i}" checked>
            <span class="ds-ctx">…${esc(h.before)}<mark>${esc(h.text)}</mark>${esc(h.after)}…</span>
            <span class="ds-src">${esc(t("ds.src." + h.source))}</span></label>`;
        }).join("")}</div>`).join("");
      q("dsSrResults").querySelectorAll("[data-hit]").forEach((c) => c.addEventListener("change", syncSel));
      syncSel();
    }
    q("dsSrAll").addEventListener("change", (e) => {
      box.querySelectorAll("[data-hit]").forEach((c) => { c.checked = e.target.checked; });
      syncSel();
    });
    async function find() {
      const err = q("dsSrErr");
      err.textContent = "";
      const spec = { terms: [], regex: null, patterns: [], matchCase: q("dsSrCase").checked, wholeWord: false, pages: [] };
      if (mode === "terms") {
        spec.terms = q("dsSrTerms").value.split(/\r?\n/).map((x) => x.trim()).filter(Boolean);
        spec.wholeWord = q("dsSrWord").checked;
        if (!spec.terms.length) { err.textContent = t("ds.srNeedTerms"); return; }
      } else if (mode === "regex") {
        spec.regex = q("dsSrRegex").value;
        if (!spec.regex.trim()) { err.textContent = t("ds.srNeedRegex"); return; }
      } else {
        spec.patterns = [...box.querySelectorAll("[data-pat]")].filter((c) => c.checked).map((c) => c.dataset.pat);
        spec.matchCase = false;
        if (!spec.patterns.length) { err.textContent = t("ds.srNeedPattern"); return; }
      }
      const scope = q("dsSrScope").value;
      if (scope === "cur") spec.pages = [state.current || 0];
      else if (scope === "range") {
        spec.pages = parsePageRange(q("dsSrRange").value, state.pages.length);
        if (!spec.pages.length) { err.textContent = t("ds.mpBad"); return; }
      }
      srLast = {
        mode, terms: q("dsSrTerms").value, regex: q("dsSrRegex").value,
        patterns: [...box.querySelectorAll("[data-pat]")].filter((c) => c.checked).map((c) => c.dataset.pat),
        matchCase: q("dsSrCase").checked, wholeWord: q("dsSrWord").checked, scope, range: q("dsSrRange").value,
      };
      q("dsSrFind").disabled = true;
      q("dsSrCount").textContent = t("ds.searching");
      q("dsSrResults").innerHTML = "";
      try {
        hits = await invoke("redact_search", { input: state.path, spec, password: null });
        if (!q("dsSrResults")) return; // hộp thoại đã đóng
        render();
      } catch (e) {
        if (q("dsSrErr")) { err.textContent = t("ds.err", { e }); q("dsSrCount").textContent = t("ds.srNone"); }
      } finally {
        if (q("dsSrFind")) q("dsSrFind").disabled = false;
      }
    }
    q("dsSrFind").addEventListener("click", find);
    box.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && e.target.tagName === "INPUT" && e.target.type === "text") { e.preventDefault(); find(); }
    });
    q("dsSrClose").addEventListener("click", closeModal);
    q("dsSrMark").addEventListener("click", () => {
      const sel = selected();
      const pages = new Set();
      let n = 0;
      for (const h of sel) {
        for (const r of h.rects) {
          // Nới 0.5pt để phủ trọn nét chữ.
          state.redactMarks.push({ page: h.pageIndex, rect: { left: r[0] - 0.5, bottom: r[1] - 0.5, right: r[2] + 0.5, top: r[3] + 0.5 } });
          n++;
        }
        pages.add(h.pageIndex);
      }
      pages.forEach((p) => drawAnnotsForPage(p));
      updateRedactButtons();
      closeModal();
      if (window.Shell && Shell.currentTab !== "protect") Shell.setTab("protect");
      if (sel.length) goToPage(sel[0].pageIndex);
      status(t("ds.srMarked", { n: sel.length, k: n }));
    });
  }

  // ==================================================================
  // 4. Làm sạch tài liệu (Sanitize / Remove Hidden Information)
  // ==================================================================
  const SAN_ITEMS = [
    ["metadata", "metadata"], ["comments", "comments"], ["forms", "formFields"], ["attachments", "attachments"],
    ["javascript", "javascript"], ["links", "links"], ["hiddenLayers", "hiddenLayers"], ["bookmarks", "bookmarks"],
    ["hiddenText", "hiddenText"], ["thumbnails", "thumbnails"], ["privateData", "privateData"],
  ];
  async function openSanitize() {
    if (!needDoc()) return;
    const box = openWide(esc(t("ds.snTitle")), `
      <p class="muted">${esc(t("ds.snIntro"))}</p>
      <div id="dsSnBody" class="ds-san"><div class="muted">${esc(t("ds.snExamining"))}</div></div>
      <div class="err" id="dsSnErr"></div>
      <div class="foot"><button id="dsSnCancel">${esc(t("common.cancel"))}</button><button id="dsSnOk" class="primary" disabled><i data-icon="eraser"></i>${esc(t("ds.snOk"))}</button></div>`);
    const q = (id) => box.querySelector("#" + id);
    q("dsSnCancel").addEventListener("click", closeModal);
    let rep;
    try {
      rep = await invoke("sanitize_examine", { input: state.path });
    } catch (e) {
      if (q("dsSnErr")) q("dsSnErr").textContent = t("ds.err", { e });
      return;
    }
    if (!q("dsSnBody")) return;
    const rows = SAN_ITEMS.map(([key, field]) => {
      const n = rep[field] || 0;
      const extra = key === "forms"
        ? `<select id="dsSnFormMode" ${n ? "" : "disabled"}><option value="flatten">${esc(t("ds.snFormsFlatten"))}</option><option value="remove">${esc(t("ds.snFormsRemove"))}</option></select>`
        : "";
      const def = n > 0 && key !== "hiddenText";
      return `<div class="ds-srow ${n ? "" : "none"}">
        <label><input type="checkbox" data-san="${key}" ${def ? "checked" : ""} ${n ? "" : "disabled"}>
          <span><b>${esc(t("ds.san." + key))}</b> <span class="ds-badge">${n ? esc(t("ds.snFound", { n })) : esc(t("ds.snNone"))}</span>
          <span class="muted ds-sdesc">${esc(t("ds.sanDesc." + key))}</span></span></label>${extra}</div>`;
    }).join("");
    const cleanup = `<div class="ds-srow"><label><input type="checkbox" checked disabled>
      <span><b>${esc(t("ds.san.unreferenced"))}</b> <span class="ds-badge">${esc(t("ds.snFound", { n: (rep.unreferenced || 0) + (rep.previousVersions || 0) }))}</span>
      <span class="muted ds-sdesc">${esc(t("ds.sanDesc.unreferenced"))}</span></span></label></div>`;
    q("dsSnBody").innerHTML = `
      <div class="ds-srow ds-sall"><label><input type="checkbox" id="dsSnAll"> <b>${esc(t("ds.snAll"))}</b></label></div>
      ${rows}${cleanup}
      <p class="ds-warn" id="dsSnWarn" hidden>${esc(t("ds.snHiddenTextWarn"))}</p>`;
    const boxes = () => [...box.querySelectorAll("[data-san]:not(:disabled)")];
    function sync() {
      const b = boxes();
      const on = b.filter((c) => c.checked).length;
      q("dsSnAll").checked = b.length > 0 && on === b.length;
      q("dsSnAll").indeterminate = on > 0 && on < b.length;
      q("dsSnAll").disabled = b.length === 0;
      q("dsSnOk").disabled = on === 0;
      const ht = box.querySelector("[data-san=hiddenText]");
      q("dsSnWarn").hidden = !(ht && ht.checked);
      const fm = q("dsSnFormMode");
      const fc = box.querySelector("[data-san=forms]");
      if (fm && fc) fm.disabled = fc.disabled || !fc.checked;
    }
    box.querySelectorAll("[data-san]").forEach((c) => c.addEventListener("change", sync));
    q("dsSnAll").addEventListener("change", (e) => { boxes().forEach((c) => { c.checked = e.target.checked; }); sync(); });
    if (!boxes().length) q("dsSnErr").textContent = t("ds.snNothing");
    sync();
    q("dsSnOk").addEventListener("click", async () => {
      const on = (k) => { const c = box.querySelector(`[data-san=${k}]`); return !!(c && c.checked && !c.disabled); };
      const options = {
        metadata: on("metadata"), comments: on("comments"),
        forms: on("forms") ? q("dsSnFormMode").value : "keep",
        attachments: on("attachments"), javascript: on("javascript"), links: on("links"),
        hiddenLayers: on("hiddenLayers"), bookmarks: on("bookmarks"), hiddenText: on("hiddenText"),
        thumbnails: on("thumbnails"), privateData: on("privateData"),
      };
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      q("dsSnOk").disabled = true;
      q("dsSnErr").textContent = t("ds.snWorking");
      try {
        const r = await invoke("sanitize_apply", { input: state.path, output: out, options });
        closeModal();
        const total = SAN_ITEMS.reduce((a, [, f]) => a + (r[f] || 0), 0);
        await loadDocument(out);
        status(t("ds.snDone", { n: total, k: r.unreferenced || 0, file: shortName(out) }));
      } catch (e) {
        if (q("dsSnErr")) { q("dsSnErr").textContent = t("ds.err", { e }); q("dsSnOk").disabled = false; }
      }
    });
  }

  // ==================================================================
  // 5. Tệp đính kèm (sidebar)
  // ==================================================================
  const Attach = (() => {
    let base = [];      // danh sách đọc từ tệp
    let ops = [];       // thay đổi chờ lưu
    let sel = null;     // key đang chọn
    let loadedFor = null;
    let seq = 0;

    function view() {
      const del = new Set(ops.filter((o) => o.op === "delete").map((o) => o.key));
      const desc = new Map(ops.filter((o) => o.op === "describe").map((o) => [o.key, o.description]));
      const list = base.filter((a) => !del.has(a.key)).map((a) =>
        desc.has(a.key) ? Object.assign({}, a, { description: desc.get(a.key), edited: true }) : a);
      for (const o of ops) {
        if (o.op === "add") list.push({ key: o.tmpKey, fileName: o.name, description: o.description, size: o.size, pending: true, pageIndex: null });
      }
      return list;
    }
    function dirty() { return ops.length > 0; }
    function selected() { return view().find((a) => a.key === sel) || null; }

    async function reload() {
      ops = [];
      sel = null;
      base = [];
      loadedFor = state.path;
      render();
      if (!state.path) return;
      const p = state.path;
      try {
        const list = await invoke("attach_list", { input: p });
        if (state.path !== p) return;
        base = list;
      } catch (e) {
        base = [];
        console.error(e);
      }
      render();
    }

    function render() {
      const panel = $("attachments");
      if (!panel) return;
      const list = view();
      const cur = selected();
      $("tabAttach").classList.toggle("has-items", list.length > 0);
      $("tabAttach").dataset.count = list.length ? String(list.length) : "";
      const docLevel = cur && cur.pageIndex == null;
      panel.innerHTML = `
        <div class="ds-atool">
          <button class="sbtn" data-a="add" title="${esc(t("ds.atAdd"))}" ${state.path ? "" : "disabled"}><i data-icon="attach-add"></i></button>
          <button class="sbtn" data-a="open" title="${esc(t("ds.atOpen"))}" ${cur && !cur.pending ? "" : "disabled"}><i data-icon="folder-open"></i></button>
          <button class="sbtn" data-a="save" title="${esc(t("ds.atSaveAs"))}" ${cur && !cur.pending ? "" : "disabled"}><i data-icon="export"></i></button>
          <button class="sbtn" data-a="desc" title="${esc(t("ds.atDesc"))}" ${docLevel ? "" : "disabled"}><i data-icon="edit-text"></i></button>
          <button class="sbtn" data-a="del" title="${esc(t("ds.atDelete"))}" ${docLevel ? "" : "disabled"}><i data-icon="trash"></i></button>
        </div>
        ${list.length ? list.map((a) => `
          <div class="ds-aitem ${a.key === sel ? "sel" : ""} ${a.pending ? "pending" : ""}" data-key="${esc(a.key)}" tabindex="0" title="${esc(a.fileName)}">
            <i data-icon="attach"></i>
            <div class="ds-ameta"><div class="ds-aname">${esc(a.fileName)}</div>
              <div class="muted">${esc(fmtSize(a.size))}${a.pageIndex != null ? " · " + esc(t("ds.atOnPage", { n: a.pageIndex + 1 })) : ""}${a.pending ? " · " + esc(t("ds.atPending")) : a.edited ? " · " + esc(t("ds.atEdited")) : ""}</div>
              ${a.description ? `<div class="ds-adesc">${esc(a.description)}</div>` : ""}</div>
          </div>`).join("") : `<div class="empty">${esc(state.path ? t("ds.atEmpty") : t("ds.noDoc"))}</div>`}
        ${dirty() ? `<div class="ds-asave"><button class="primary" data-a="commit"><i data-icon="save"></i>${esc(t("ds.atSaveChanges", { n: ops.length }))}</button>
          <button data-a="discard">${esc(t("ds.atDiscard"))}</button></div>` : ""}`;
      applyIcons(panel);
    }

    async function add() {
      if (!state.path) return;
      const files = await invoke("pick_files");
      if (!files || !files.length) return;
      for (const f of files) {
        ops.push({ op: "add", path: f, name: shortName(f), description: "", tmpKey: "new:" + (++seq), size: null });
      }
      sel = ops[ops.length - 1].tmpKey;
      render();
      status(t("ds.atAdded", { n: files.length }));
    }
    async function openSel() {
      const a = selected();
      if (!a || a.pending) return;
      if (a.risky && !(await confirmModal(t("ds.atRiskyTitle"), t("ds.atRiskyMsg", { name: a.fileName }), t("ds.atOpenAnyway")))) return;
      try {
        await invoke("attach_open", { input: state.path, key: a.key, fileName: a.fileName });
        status(t("ds.atOpened", { name: a.fileName }));
      } catch (e) { status(t("ds.err", { e })); }
    }
    async function saveSel() {
      const a = selected();
      if (!a || a.pending) return;
      const out = await invoke("pick_save_file", { name: a.fileName });
      if (!out) return;
      try {
        await invoke("attach_extract", { input: state.path, key: a.key, output: out });
        status(t("ds.atSaved", { file: shortName(out) }));
      } catch (e) { status(t("ds.err", { e })); }
    }
    function editDesc() {
      const a = selected();
      if (!a || a.pageIndex != null) return;
      const box = openModal(esc(t("ds.atDescTitle")), `
        <p class="muted">${esc(a.fileName)}</p>
        <label>${esc(t("ds.atDescLabel"))}</label>
        <textarea id="dsAtDesc" rows="3" maxlength="1000"></textarea>
        <div class="foot"><button id="dsAtDescCancel">${esc(t("common.cancel"))}</button><button id="dsAtDescOk" class="primary">${esc(t("common.ok"))}</button></div>`);
      box.querySelector("#dsAtDesc").value = a.description || "";
      box.querySelector("#dsAtDescCancel").addEventListener("click", closeModal);
      box.querySelector("#dsAtDescOk").addEventListener("click", () => {
        const v = box.querySelector("#dsAtDesc").value.trim();
        const pend = ops.find((o) => o.op === "add" && o.tmpKey === a.key);
        if (pend) pend.description = v;
        else {
          ops = ops.filter((o) => !(o.op === "describe" && o.key === a.key));
          const orig = base.find((b) => b.key === a.key);
          if (!orig || (orig.description || "") !== v) ops.push({ op: "describe", key: a.key, description: v });
        }
        closeModal();
        render();
      });
    }
    async function delSel() {
      const a = selected();
      if (!a || a.pageIndex != null) return;
      if (!a.pending && !(await confirmModal(t("ds.atDelTitle"), t("ds.atDelMsg", { name: a.fileName }), t("ds.atDelete")))) return;
      if (a.pending) ops = ops.filter((o) => o.tmpKey !== a.key);
      else {
        ops = ops.filter((o) => !(o.op === "describe" && o.key === a.key));
        ops.push({ op: "delete", key: a.key });
      }
      sel = null;
      render();
    }
    async function commit() {
      if (!dirty() || !state.path) return;
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      const payload = ops.map((o) => o.op === "add" ? { op: "add", path: o.path, name: o.name, description: o.description } : o);
      try {
        await invoke("attach_apply", { input: state.path, ops: payload, output: out });
        ops = [];
        await loadDocument(out);
        status(t("ds.atCommitted", { file: shortName(out) }));
      } catch (e) { status(t("ds.err", { e })); }
    }
    async function discard() {
      if (!(await confirmModal(t("ds.atDiscardTitle"), t("ds.atDiscardMsg"), t("ds.atDiscard")))) return;
      ops = [];
      sel = null;
      render();
    }

    function bind() {
      const panel = $("attachments");
      panel.addEventListener("click", (e) => {
        const b = e.target.closest("[data-a]");
        if (b && !b.disabled) {
          ({ add, open: openSel, save: saveSel, desc: editDesc, del: delSel, commit, discard })[b.dataset.a]();
          return;
        }
        const it = e.target.closest(".ds-aitem");
        if (it) { sel = it.dataset.key; render(); panel.querySelector(`.ds-aitem.sel`)?.focus(); }
      });
      panel.addEventListener("dblclick", (e) => { if (e.target.closest(".ds-aitem")) openSel(); });
      panel.addEventListener("keydown", (e) => {
        if (!e.target.closest(".ds-aitem")) return;
        if (e.key === "Enter") { e.preventDefault(); openSel(); }
        else if (e.key === "Delete") { e.preventDefault(); delSel(); }
      });
      $("tabAttach").addEventListener("click", () => { if (loadedFor !== state.path) reload(); else render(); });
    }
    return { reload, render, bind, dirty, get count() { return ops.length; } };
  })();

  // ==================================================================
  // 6. Thuộc tính tài liệu
  // ==================================================================
  const LAYOUTS = ["", "SinglePage", "OneColumn", "TwoPageLeft", "TwoColumnLeft", "TwoPageRight", "TwoColumnRight"];
  const NAVS = ["", "UseOutlines", "UseThumbs", "UseAttachments", "UseOC"];
  const MAGS = ["default", "actual", "fitPage", "fitWidth", "fitHeight", "fitVisible", "25", "50", "75", "100", "125", "150", "200", "400"];
  const LANGS = ["", "vi-VN", "en-US", "en-GB", "fr-FR", "de-DE", "ja-JP", "ko-KR", "zh-CN"];

  async function openProperties() {
    if (!needDoc()) return;
    let p;
    try {
      p = await invoke("docprops_read", { path: state.path, original: originalPath || state.path });
    } catch (e) { status(t("ds.err", { e })); return; }
    const mm = (pt) => (pt * 25.4 / 72).toFixed(1);
    const yes = (b) => esc(b ? t("ds.yes") : t("ds.no"));
    const kv = (rows) => `<table class="kv">${rows.map(([k, v]) => `<tr><td>${esc(k)}</td><td class="ds-wrap">${v}</td></tr>`).join("")}</table>`;
    const opt = (vals, cur, key) => vals.map((v) => `<option value="${esc(v)}" ${v === cur ? "selected" : ""}>${esc(key(v))}</option>`).join("");
    const magLabel = (m) => /^\d+$/.test(m) ? m + "%" : t("ds.mag." + m);
    const mags = MAGS.includes(p.view.magnification) ? MAGS : MAGS.concat([p.view.magnification]);
    const sec = p.security;
    const perm = (b) => `<span class="${b ? "ds-ok" : "ds-bad"}">${esc(b ? t("ds.allowed") : t("ds.notAllowed"))}</span>`;
    const box = openWide(esc(t("ds.prTitle")), `
      <div class="ds-seg ds-tabs" role="tablist">
        <button data-tab="desc" class="on">${esc(t("ds.prTabDesc"))}</button>
        <button data-tab="sec">${esc(t("ds.prTabSecurity"))}</button>
        <button data-tab="fonts">${esc(t("ds.prTabFonts"))}</button>
        <button data-tab="view">${esc(t("ds.prTabView"))}</button>
      </div>
      <div data-pane="desc">
        <div class="row"><div><label>${esc(t("ds.prTitleF"))}</label><input type="text" id="dsPrTitle"></div>
          <div><label>${esc(t("ds.prAuthor"))}</label><input type="text" id="dsPrAuthor"></div></div>
        <div class="row"><div><label>${esc(t("ds.prSubject"))}</label><input type="text" id="dsPrSubject"></div>
          <div><label>${esc(t("ds.prKeywords"))}</label><input type="text" id="dsPrKeywords"></div></div>
        <label>${esc(t("ds.prLang"))}</label>
        <input type="text" id="dsPrLang" list="dsLangList" placeholder="vi-VN">
        <datalist id="dsLangList">${LANGS.filter(Boolean).map((l) => `<option value="${l}">`).join("")}</datalist>
        ${kv([
          [t("ds.prFile"), esc(docName())],
          [t("ds.prPath"), esc(originalPath || state.path)],
          [t("ds.prCreated"), esc(fmtDate(p.creationDate))],
          [t("ds.prModified"), esc(fmtDate(p.modDate))],
          [t("ds.prCreator"), esc(p.creator || "—")],
          [t("ds.prProducer"), esc(p.producer || "—")],
          [t("ds.prVersion"), esc(p.pdfVersion)],
          [t("ds.prPages"), esc(String(p.pageCount))],
          [t("ds.prPageSize"), esc(t("ds.prPageSizeVal", { w: mm(p.pageWidth), h: mm(p.pageHeight) }))],
          [t("ds.prFileSize"), esc(fmtSize(p.fileSize))],
          [t("ds.prTagged"), yes(p.tagged)],
          [t("ds.prEncrypted"), yes(!!sec)],
        ])}
      </div>
      <div data-pane="sec" hidden>
        ${sec ? kv([
          [t("ds.prSecMethod"), esc(t("ds.prSecPassword") + " · " + sec.method)],
          [t("ds.prSecUserPw"), yes(sec.hasUserPassword)],
          [t("ds.prPermPrint"), perm(sec.allowPrint) + (sec.allowPrint && !sec.allowPrintHigh ? ` <span class="muted">(${esc(t("ds.prPermLowRes"))})</span>` : "")],
          [t("ds.prPermModify"), perm(sec.allowModify)],
          [t("ds.prPermAssemble"), perm(sec.allowAssemble)],
          [t("ds.prPermCopy"), perm(sec.allowCopy)],
          [t("ds.prPermAccess"), perm(sec.allowAccessibility)],
          [t("ds.prPermAnnot"), perm(sec.allowAnnotate)],
          [t("ds.prPermForms"), perm(sec.allowFillForms)],
        ]) : `<p class="muted">${esc(t("ds.prNoSecurity"))}</p>`}
      </div>
      <div data-pane="fonts" hidden>
        ${p.fonts.length ? `<table class="sig-table ds-fonts"><tr><th>${esc(t("ds.prFontName"))}</th><th>${esc(t("ds.prFontType"))}</th><th>${esc(t("ds.prFontEnc"))}</th><th>${esc(t("ds.prFontEmb"))}</th></tr>
          ${p.fonts.map((f) => `<tr><td>${esc(f.name)}</td><td>${esc(f.fontType)}</td><td>${esc(f.encoding || "—")}</td>
            <td>${f.embedded ? esc(f.subset ? t("ds.prEmbSubset") : t("ds.prEmbFull")) : `<span class="ds-bad">${esc(t("ds.prNotEmb"))}</span>`}</td></tr>`).join("")}</table>`
          : `<p class="muted">${esc(t("ds.prNoFonts"))}</p>`}
      </div>
      <div data-pane="view" hidden>
        <div class="row">
          <div><label>${esc(t("ds.prNav"))}</label><select id="dsPrNav">${opt(NAVS, p.view.navigation, (v) => t("ds.nav." + (v || "default")))}</select></div>
          <div><label>${esc(t("ds.prLayout"))}</label><select id="dsPrLayout">${opt(LAYOUTS, p.view.pageLayout, (v) => t("ds.layout." + (v || "default")))}</select></div>
        </div>
        <div class="row">
          <div><label>${esc(t("ds.prMag"))}</label><select id="dsPrMag">${opt(mags, p.view.magnification, magLabel)}</select></div>
          <div><label>${esc(t("ds.prOpenPage", { n: p.pageCount }))}</label><input type="number" id="dsPrOpenPage" min="1" max="${p.pageCount}"></div>
        </div>
        <label>${esc(t("ds.prWindow"))}</label>
        <div class="ds-checks">
          <label><input type="checkbox" id="dsPrFit"> ${esc(t("ds.prFitWindow"))}</label>
          <label><input type="checkbox" id="dsPrCenter"> ${esc(t("ds.prCenter"))}</label>
          <label><input type="checkbox" id="dsPrFull"> ${esc(t("ds.prFullScreen"))}</label>
        </div>
        <label>${esc(t("ds.prShow"))}</label>
        <div class="radiorow">
          <label><input type="radio" name="dsPrShow" value="file"> ${esc(t("ds.prShowFile"))}</label>
          <label><input type="radio" name="dsPrShow" value="title"> ${esc(t("ds.prShowTitle"))}</label>
        </div>
        <label>${esc(t("ds.prUi"))}</label>
        <div class="ds-checks">
          <label><input type="checkbox" id="dsPrHideMenu"> ${esc(t("ds.prHideMenu"))}</label>
          <label><input type="checkbox" id="dsPrHideTool"> ${esc(t("ds.prHideToolbar"))}</label>
          <label><input type="checkbox" id="dsPrHideUi"> ${esc(t("ds.prHideUi"))}</label>
        </div>
      </div>
      <div class="err" id="dsPrErr"></div>
      <div class="foot"><button id="dsPrClose">${esc(t("ds.close"))}</button><button id="dsPrSave" class="primary" disabled><i data-icon="save"></i>${esc(t("ds.prSaveAs"))}</button></div>`);
    const q = (id) => box.querySelector("#" + id);
    box.querySelectorAll(".ds-tabs button").forEach((b) => b.addEventListener("click", () => {
      box.querySelectorAll(".ds-tabs button").forEach((x) => x.classList.toggle("on", x === b));
      box.querySelectorAll("[data-pane]").forEach((pn) => { pn.hidden = pn.dataset.pane !== b.dataset.tab; });
    }));
    q("dsPrTitle").value = p.title;
    q("dsPrAuthor").value = p.author;
    q("dsPrSubject").value = p.subject;
    q("dsPrKeywords").value = p.keywords;
    q("dsPrLang").value = p.lang;
    q("dsPrOpenPage").value = p.view.openPage + 1;
    q("dsPrFit").checked = p.view.fitWindow;
    q("dsPrCenter").checked = p.view.centerWindow;
    q("dsPrFull").checked = p.view.fullScreen;
    q("dsPrHideMenu").checked = p.view.hideMenubar;
    q("dsPrHideTool").checked = p.view.hideToolbar;
    q("dsPrHideUi").checked = p.view.hideWindowUi;
    box.querySelector(`input[name=dsPrShow][value=${p.view.displayDocTitle ? "title" : "file"}]`).checked = true;
    const readView = () => ({
      pageLayout: q("dsPrLayout").value, navigation: q("dsPrNav").value, fullScreen: q("dsPrFull").checked,
      openPage: Math.max(0, Math.min(p.pageCount - 1, (parseInt(q("dsPrOpenPage").value, 10) || 1) - 1)),
      magnification: q("dsPrMag").value, fitWindow: q("dsPrFit").checked, centerWindow: q("dsPrCenter").checked,
      displayDocTitle: box.querySelector("input[name=dsPrShow]:checked").value === "title",
      hideMenubar: q("dsPrHideMenu").checked, hideToolbar: q("dsPrHideTool").checked, hideWindowUi: q("dsPrHideUi").checked,
    });
    const initialView = JSON.stringify(readView());
    const fields = { title: "dsPrTitle", author: "dsPrAuthor", subject: "dsPrSubject", keywords: "dsPrKeywords", lang: "dsPrLang" };
    function changes() {
      const u = {};
      for (const [k, id] of Object.entries(fields)) {
        const v = q(id).value.trim();
        if (v !== (p[k] || "").trim()) u[k] = v;
      }
      const v = readView();
      if (JSON.stringify(v) !== initialView) u.view = v;
      return u;
    }
    const sync = () => { q("dsPrSave").disabled = Object.keys(changes()).length === 0; };
    box.addEventListener("input", sync);
    box.addEventListener("change", sync);
    q("dsPrClose").addEventListener("click", closeModal);
    q("dsPrSave").addEventListener("click", async () => {
      const update = changes();
      if (!Object.keys(update).length) return;
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      try {
        await invoke("docprops_write", { input: state.path, output: out, update });
        closeModal();
        await loadDocument(out);
        status(t("ds.prSaved", { file: shortName(out) }));
      } catch (e) { q("dsPrErr").textContent = t("ds.err", { e }); }
    });
  }

  // ==================================================================
  // 7. Trợ năng
  // ==================================================================
  const CATS = ["document", "page", "forms", "altText", "tables", "lists", "headings"];
  const FIX_OF = {
    "document.title": "title", "document.language": "lang", "document.displayDocTitle": "displayDocTitle",
    "page.tabOrder": "tabOrder", "forms.descriptions": "fieldTooltips", "page.linkContents": "linkContents",
    "document.bookmarks": "bookmarksFromHeadings",
  };
  const detailText = (d) => {
    if (!d) return "";
    if (/^\d+$/.test(d)) return t("ds.pageN", { n: d });
    const m = d.match(/^(.*)@(\d+)$/);
    if (m) return t("ds.a11yOnPage", { what: m[1], n: m[2] });
    return d;
  };

  async function runCheck(quick) {
    if (!needDoc()) return;
    const box = openWide(esc(t(quick ? "ds.a11yQuickTitle" : "ds.a11yFullTitle")), `
      <div id="dsA11yBody"><div class="muted">${esc(t("ds.a11yChecking"))}</div></div>
      <div class="err" id="dsA11yErr"></div>
      <div class="foot"><button id="dsA11yClose">${esc(t("ds.close"))}</button><button id="dsA11yAlt"><i data-icon="alt-text"></i>${esc(t("ds.a11yAltBtn"))}</button><button id="dsA11ySave" class="primary" disabled><i data-icon="save"></i>${esc(t("ds.a11ySaveFixes"))}</button></div>`);
    const q = (id) => box.querySelector("#" + id);
    q("dsA11yClose").addEventListener("click", closeModal);
    q("dsA11yAlt").addEventListener("click", () => { closeModal(); openAltText(); });
    let checks;
    try {
      checks = await invoke("a11y_check", { input: state.path, quick });
    } catch (e) {
      if (q("dsA11yErr")) q("dsA11yErr").textContent = t("ds.err", { e });
      return;
    }
    if (!q("dsA11yBody")) return;
    const fixes = {};
    const stem = docName().replace(/\.pdf$/i, "");
    const counts = { passed: 0, failed: 0, manual: 0 };
    checks.forEach((c) => { counts[c.status]++; });
    const icon = (s) => s === "passed" ? "check-circle" : s === "failed" ? "x-circle" : "help-circle";
    const row = (c) => {
      const fixKey = FIX_OF[c.id];
      let fixUi = "";
      if (c.fixable && fixKey === "title") {
        fixUi = `<div class="ds-fixin"><input type="text" data-fix-input="title" value="${esc(stem)}"><button data-fix="title">${esc(t("ds.a11yFix"))}</button></div>`;
      } else if (c.fixable && fixKey === "lang") {
        fixUi = `<div class="ds-fixin"><select data-fix-input="lang">${LANGS.filter(Boolean).map((l) => `<option value="${l}" ${l === (I18N.lang === "vi" ? "vi-VN" : "en-US") ? "selected" : ""}>${esc(t("ds.lang." + l))}</option>`).join("")}</select><button data-fix="lang">${esc(t("ds.a11yFix"))}</button></div>`;
      } else if (c.fixable && fixKey) {
        fixUi = `<button class="ds-fixbtn" data-fix="${fixKey}">${esc(t("ds.a11yFix"))}</button>`;
      }
      const details = c.details.filter(Boolean).slice(0, 30).map(detailText);
      const more = c.details.filter(Boolean).length > 30 ? `<li class="muted">${esc(t("ds.a11yMore", { n: c.details.length - 30 }))}</li>` : "";
      return `<details class="ds-chk ${c.status}" data-id="${esc(c.id)}">
        <summary><i data-icon="${icon(c.status)}"></i><span class="ds-cname">${esc(t("ds.chk." + c.id))}</span>
          ${c.count && c.status !== "passed" ? `<span class="ds-badge">${c.count}</span>` : ""}
          <span class="ds-cstat">${esc(t("ds.st." + c.status))}</span><span class="ds-fixed" hidden>${esc(t("ds.a11yWillFix"))}</span></summary>
        <div class="ds-cbody"><p>${esc(t("ds.chkDesc." + c.id))}</p>
          ${details.length ? `<ul>${details.map((d) => `<li>${esc(d)}</li>`).join("")}${more}</ul>` : ""}
          ${fixUi}</div></details>`;
    };
    q("dsA11yBody").innerHTML = `
      <div class="ds-a11ysum"><span class="ds-bad">${esc(t("ds.a11ySumFailed", { n: counts.failed }))}</span> · <span class="ds-warnc">${esc(t("ds.a11ySumManual", { n: counts.manual }))}</span> · <span class="ds-ok">${esc(t("ds.a11ySumPassed", { n: counts.passed }))}</span>
        ${checks.some((c) => c.fixable) ? `<button id="dsA11yFixAll" class="ds-fixall">${esc(t("ds.a11yFixAll"))}</button>` : ""}</div>
      <div class="ds-tree">${CATS.filter((cat) => checks.some((c) => c.category === cat)).map((cat) => {
        const cs = checks.filter((c) => c.category === cat);
        const bad = cs.filter((c) => c.status === "failed").length;
        return `<details class="ds-cat" ${bad ? "open" : ""}><summary><b>${esc(t("ds.cat." + cat))}</b>
          <span class="muted">${esc(t("ds.a11yCatSum", { f: bad, n: cs.length }))}</span></summary>${cs.map(row).join("")}</details>`;
      }).join("")}</div>
      <p class="muted ds-a11ynote">${esc(t("ds.a11yNote"))}</p>`;
    applyIcons(q("dsA11yBody"));
    function markFix(key, val) {
      fixes[key] = val;
      const id = Object.keys(FIX_OF).find((k) => FIX_OF[k] === key);
      const d = box.querySelector(`.ds-chk[data-id="${id}"]`);
      if (d) { d.classList.add("willfix"); d.querySelector(".ds-fixed").hidden = false; }
      q("dsA11ySave").disabled = Object.keys(fixes).length === 0;
    }
    function fixValue(key) {
      const inp = box.querySelector(`[data-fix-input=${key}]`);
      if (key === "title") return inp && inp.value.trim() ? inp.value.trim() : null;
      if (key === "lang") return inp ? inp.value : null;
      if (key === "linkContents") return t("ds.a11yLinkTpl");
      return true;
    }
    box.querySelectorAll("[data-fix]").forEach((b) => b.addEventListener("click", (e) => {
      e.preventDefault();
      const key = b.dataset.fix;
      const v = fixValue(key);
      if (v == null) return;
      markFix(key, v);
    }));
    const fixAll = q("dsA11yFixAll");
    if (fixAll) fixAll.addEventListener("click", () => {
      checks.filter((c) => c.fixable && FIX_OF[c.id]).forEach((c) => {
        const v = fixValue(FIX_OF[c.id]);
        if (v != null) markFix(FIX_OF[c.id], v);
      });
    });
    q("dsA11ySave").addEventListener("click", async () => {
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      const payload = {
        title: fixes.title || null, lang: fixes.lang || null, displayDocTitle: !!fixes.displayDocTitle,
        tabOrder: !!fixes.tabOrder, fieldTooltips: !!fixes.fieldTooltips,
        linkContents: fixes.linkContents || null, bookmarksFromHeadings: !!fixes.bookmarksFromHeadings,
      };
      q("dsA11ySave").disabled = true;
      try {
        const n = await invoke("a11y_fix", { input: state.path, output: out, fixes: payload });
        closeModal();
        await loadDocument(out);
        status(t("ds.a11yFixed", { n, file: shortName(out) }));
        runCheck(quick); // kiểm tra lại bản đã sửa
      } catch (e) {
        if (q("dsA11yErr")) { q("dsA11yErr").textContent = t("ds.err", { e }); q("dsA11ySave").disabled = false; }
      }
    });
  }

  // Cắt vùng hình từ ảnh trang (data URL) → data URL nhỏ.
  function cropImage(src, rect, pageW, pageH) {
    return new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        const s = img.naturalWidth / pageW;
        const pad = 4;
        const x = Math.max(0, (rect[0] - pad) * s), y = Math.max(0, (pageH - rect[3] - pad) * s);
        const w = Math.min(img.naturalWidth - x, (rect[2] - rect[0] + 2 * pad) * s);
        const h = Math.min(img.naturalHeight - y, (rect[3] - rect[1] + 2 * pad) * s);
        if (w < 2 || h < 2) { resolve(src); return; }
        const c = document.createElement("canvas");
        const scale = Math.min(1, 220 / w, 150 / h);
        c.width = Math.round(w * scale); c.height = Math.round(h * scale);
        c.getContext("2d").drawImage(img, x, y, w, h, 0, 0, c.width, c.height);
        resolve(c.toDataURL("image/png"));
      };
      img.onerror = () => resolve(null);
      img.src = src;
    });
  }

  async function openAltText() {
    if (!needDoc()) return;
    const box = openWide(esc(t("ds.altTitle")), `
      <p class="muted">${esc(t("ds.altIntro"))}</p>
      <div id="dsAltBody" class="ds-alt"><div class="muted">${esc(t("common.loading"))}</div></div>
      <div class="err" id="dsAltErr"></div>
      <div class="foot"><button id="dsAltClose">${esc(t("ds.close"))}</button><button id="dsAltSave" class="primary" disabled><i data-icon="save"></i>${esc(t("ds.altSave"))}</button></div>`);
    const q = (id) => box.querySelector("#" + id);
    q("dsAltClose").addEventListener("click", closeModal);
    let figs;
    try {
      figs = await invoke("a11y_figures", { input: state.path });
    } catch (e) {
      if (q("dsAltErr")) q("dsAltErr").textContent = t("ds.err", { e });
      return;
    }
    if (!q("dsAltBody")) return;
    if (!figs.length) {
      q("dsAltBody").innerHTML = `<p>${esc(t("ds.altNone"))}</p>`;
      return;
    }
    q("dsAltBody").innerHTML = figs.map((f, i) => `
      <div class="ds-fig">
        <div class="ds-figthumb" id="dsFig${i}">${f.pageIndex != null ? "" : "?"}</div>
        <div class="ds-figmeta">
          <div class="muted">${esc(t("ds.altFigure", { i: i + 1, n: figs.length }))}${f.pageIndex != null ? " · " + esc(t("ds.pageN", { n: f.pageIndex + 1 })) : ""}</div>
          <textarea rows="2" data-fig="${i}" placeholder="${esc(t("ds.altPh"))}"></textarea>
          ${f.pageIndex != null ? `<button class="ds-link" data-goto="${f.pageIndex}">${esc(t("ds.altGoto"))}</button>` : ""}
        </div>
      </div>`).join("");
    figs.forEach((f, i) => { box.querySelector(`[data-fig="${i}"]`).value = f.alt || ""; });
    const sync = () => {
      const changed = figs.some((f, i) => box.querySelector(`[data-fig="${i}"]`).value.trim() !== (f.alt || "").trim());
      q("dsAltSave").disabled = !changed;
    };
    box.addEventListener("input", sync);
    box.querySelectorAll("[data-goto]").forEach((b) => b.addEventListener("click", () => { closeModal(); goToPage(Number(b.dataset.goto)); }));
    // Ảnh thu nhỏ vùng hình (render trang 1 lần mỗi trang).
    const pageImgs = new Map();
    const p = state.path;
    for (let i = 0; i < figs.length; i++) {
      const f = figs[i];
      if (f.pageIndex == null) continue;
      const pg = state.pages[f.pageIndex];
      if (!pg) continue;
      try {
        if (!pageImgs.has(f.pageIndex)) pageImgs.set(f.pageIndex, await invoke("render_page", { path: p, page: f.pageIndex, width: 700 }));
        const src = pageImgs.get(f.pageIndex);
        const url = f.rect ? await cropImage(src, f.rect, pg.widthPt, pg.heightPt) : src;
        const holder = box.querySelector("#dsFig" + i);
        if (!holder || !url) continue;
        holder.innerHTML = `<img alt="" src="${url}">`;
      } catch (_) { /* thiếu ảnh thu nhỏ không chặn việc nhập */ }
    }
    q("dsAltSave").addEventListener("click", async () => {
      const alts = figs.map((f, i) => ({ id: f.id, alt: box.querySelector(`[data-fig="${i}"]`).value.trim(), old: (f.alt || "").trim() }))
        .filter((a) => a.alt !== a.old).map(({ id, alt }) => ({ id, alt }));
      if (!alts.length) return;
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      try {
        const n = await invoke("a11y_set_alt", { input: state.path, output: out, alts });
        closeModal();
        await loadDocument(out);
        status(t("ds.altSaved", { n, file: shortName(out) }));
      } catch (e) { if (q("dsAltErr")) q("dsAltErr").textContent = t("ds.err", { e }); }
    });
  }

  // ==================================================================
  // Gắn nút
  // ==================================================================
  const bind = (id, fn) => { const el = $(id); if (el) el.addEventListener("click", fn); };
  bind("secRedactSearch", openSearchRedact);
  bind("secRedactPages", openMarkPages);
  bind("secRedactOptions", openRedactOptions);
  bind("secSanitize", openSanitize);
  bind("a11yFullCheck", () => runCheck(false));
  bind("a11yQuickCheck", () => runCheck(true));
  bind("a11yAltText", openAltText);
  bind("a11yProps", openProperties);
  Attach.bind();
  if (window.Shell) Shell.onLangChange(() => Attach.render());

  window.DocSec = {
    redactStyle,
    openProperties,
    // Thay đổi đính kèm chưa lưu (cho hộp xác nhận bỏ thay đổi của shell).
    dirtyPart() { return Attach.dirty() ? t("ds.dirtyAttach", { n: Attach.count }) : null; },
  };
})();
