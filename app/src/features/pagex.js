// Page Marks kiểu Foxit (tab Trang): Hình mờ / Nền / Đầu & chân trang / Đánh số
// Bates — mỗi nút có menu Thêm… / Cập nhật… / Gỡ; hộp thoại 2 cột (tuỳ chọn +
// xem trước trực tiếp). Kèm Nhân bản trang và Đảo thứ tự trang cho lưới Trang.
// Engine: crates/ff-engine/src/watermark.rs; lệnh Tauri: cmd_pagex.rs.
(function () {
  const UNITS = { pt: 1, mm: 72 / 25.4, cm: 72 / 2.54, in: 72 };
  const ANCHORS = ["top-left", "top-center", "top-right", "middle-left", "center", "middle-right", "bottom-left", "bottom-center", "bottom-right"];
  const FONTS = ["sans", "serif", "mono", "helvetica", "times", "courier"];
  // Mẫu số trang chèn vào ô (dữ liệu, không dịch — người dùng thấy đúng chữ sẽ in).
  const PAGE_FORMATS = ["{page}", "Page {page}", "{page} of {total}", "Page {page} of {total}", "{page}/{total}", "Trang {page}", "Trang {page}/{total}"];
  const DATE_FORMATS = ["dd/mm/yyyy", "mm/dd/yyyy", "yyyy-mm-dd", "dd.mm.yyyy", "d/m/yy"];
  const PREVIEW_W = 360;

  // ---------- tiện ích ----------
  function loadPrefs(key, def) {
    try { return Object.assign({}, def, JSON.parse(localStorage.getItem("ff.pagex." + key) || "{}")); } catch (_) { return Object.assign({}, def); }
  }
  function savePrefs(key, v) {
    try { localStorage.setItem("ff.pagex." + key, JSON.stringify(v)); } catch (_) {}
  }
  function fmtDate(fmt, d) {
    const dd = String(d.getDate()).padStart(2, "0");
    const mm = String(d.getMonth() + 1).padStart(2, "0");
    const yyyy = String(d.getFullYear());
    return fmt.replace("dd", dd).replace("mm", mm).replace("yyyy", yyyy)
      .replace(/\bd\b/, String(d.getDate())).replace(/\bm\b/, String(d.getMonth() + 1)).replace("yy", yyyy.slice(2));
  }
  function status(msg) { $("status").textContent = msg; }
  function hint(msg) { const h = $("organizeHint"); if (h) h.textContent = msg; else status(msg); }
  function num(box, id, def) {
    const v = Number(box.querySelector("#" + id).value);
    return Number.isFinite(v) ? v : def;
  }
  function baseName() {
    return shortName(state.origPath || state.path || "");
  }
  // Tài liệu đang xem (kể cả thay đổi tổ chức trang chưa lưu) — dùng làm đầu vào.
  async function baseInput() {
    if (!state.path) { status(t("pagex.openFirst")); return null; }
    if (state.organizeMode && typeof materializeBaseInput === "function") return materializeBaseInput();
    return { path: state.path, isTemp: false, pageCount: state.pages.length };
  }

  // ---------- menu thả xuống Thêm / Cập nhật / Gỡ ----------
  const menu = $("pmMenu");
  let menuOwner = null;
  let wasOpen = false;
  function closePmMenu() { menu.classList.add("hidden"); menuOwner = null; }
  function openPmMenu(btn) {
    const kind = btn.dataset.pm;
    menu.innerHTML =
      `<button class="mitem" data-act="add"><i data-icon="plus"></i><span>${t("pagex.menu.add")}</span></button>` +
      `<button class="mitem" data-act="update"><i data-icon="refresh"></i><span>${t("pagex.menu.update")}</span></button>` +
      `<div class="msep"></div>` +
      `<button class="mitem" data-act="remove"><i data-icon="trash"></i><span>${t("pagex.menu.remove")}</span></button>`;
    applyIcons(menu);
    document.querySelectorAll(".menu:not(.submenu)").forEach((m) => { if (m !== menu) m.classList.add("hidden"); });
    menu.classList.remove("hidden");
    const r = btn.getBoundingClientRect();
    menu.style.top = r.bottom + 4 + "px";
    menu.style.left = Math.min(r.left, window.innerWidth - menu.offsetWidth - 8) + "px";
    menu.style.right = "";
    menuOwner = kind;
    menu.onclick = (e) => {
      const it = e.target.closest("[data-act]");
      if (!it) return;
      closePmMenu();
      const act = it.dataset.act;
      if (act === "remove") removeMarks(kind);
      else openAddDialog(kind, act === "update");
    };
  }
  document.querySelectorAll(".pm-drop").forEach((btn) => {
    // mousedown chung của shell đóng mọi menu trước khi click tới → ghi nhận lúc pointerdown.
    btn.addEventListener("pointerdown", () => { wasOpen = !menu.classList.contains("hidden") && menuOwner === btn.dataset.pm; });
    btn.addEventListener("click", () => {
      if (wasOpen) { closePmMenu(); wasOpen = false; return; }
      openPmMenu(btn);
    });
  });
  window.addEventListener("blur", closePmMenu);

  // Kết quả được mở như tài liệu mới → chú thích chưa lưu sẽ mất: hỏi trước.
  async function confirmUnsavedAnnots() {
    const n = (state.annotSpecs || []).length;
    if (!n || $("saveAnnots").disabled) return true;
    return confirmModal(t("pagex.annotsTitle"), t("pagex.annotsLost", { n }), t("pagex.continue"));
  }

  async function openAddDialog(kind, update) {
    if (!state.path) { status(t("pagex.openFirst")); return; }
    if (!(await confirmUnsavedAnnots())) return;
    if (kind === "watermark") openStampDialog("watermark", update);
    else if (kind === "background") openStampDialog("background", update);
    else if (kind === "headerFooter") openHfDialog(update);
    else openBatesDialog(update);
  }

  // ---------- khối HTML dùng chung ----------
  function colorBtn(id) {
    return `<button type="button" class="pm-color" id="${id}"><span class="pm-sw"></span></button>`;
  }
  function bindColor(box, id, get, set) {
    const b = box.querySelector("#" + id);
    const paint = () => { b.querySelector(".pm-sw").style.background = rgbCss(get()); };
    paint();
    b.addEventListener("click", () => openColorPopover(b, get(), (rgb) => { set(rgb); paint(); box.dispatchEvent(new Event("pm-change")); }));
  }
  function fontSelect(id, cur) {
    return `<select id="${id}">${FONTS.map((f) => `<option value="${f}" ${f === cur ? "selected" : ""}>${t("pagex.font." + f)}</option>`).join("")}</select>`;
  }
  function unitSelect(cur) {
    return `<select id="pmUnit" class="pm-unit">${Object.keys(UNITS).map((u) => `<option value="${u}" ${u === cur ? "selected" : ""}>${t("pagex.unit." + u)}</option>`).join("")}</select>`;
  }
  // Đổi đơn vị → quy đổi luôn các ô khoảng cách/lề (giữ nguyên vị trí thật như Foxit).
  function bindUnit(box) {
    const sel = box.querySelector("#pmUnit");
    if (!sel) return;
    let prev = sel.value;
    sel.addEventListener("change", () => {
      const f = UNITS[prev] / UNITS[sel.value];
      box.querySelectorAll("#pmVOff, #pmHOff, #pmMT, #pmMB, #pmML, #pmMR").forEach((inp) => {
        inp.value = String(Math.round(Number(inp.value || 0) * f * 100) / 100);
      });
      prev = sel.value;
    });
  }
  function pagesSection(p) {
    return `
      <fieldset class="pm-set"><legend>${t("pagex.pageRange")}</legend>
        <div class="radiorow">
          <label><input type="radio" name="pmRange" value="all" ${p.range ? "" : "checked"}> ${t("pagex.allPages")}</label>
          <label><input type="radio" name="pmRange" value="range" ${p.range ? "checked" : ""}> ${t("pagex.pagesLabel")}</label>
          <input type="text" id="pmRangeTxt" class="pm-range" value="${escapeHtml(p.range || "")}" placeholder="${t("pagex.rangePh")}">
        </div>
        <div class="row pm-inline"><label>${t("pagex.subset")}</label>
          <select id="pmSubset">
            <option value="all" ${p.subset === "all" ? "selected" : ""}>${t("pagex.subset.all")}</option>
            <option value="even" ${p.subset === "even" ? "selected" : ""}>${t("pagex.subset.even")}</option>
            <option value="odd" ${p.subset === "odd" ? "selected" : ""}>${t("pagex.subset.odd")}</option>
          </select>
        </div>
      </fieldset>`;
  }
  function readPages(box, count) {
    const useRange = box.querySelector('input[name=pmRange]:checked').value === "range";
    const txt = box.querySelector("#pmRangeTxt").value.trim();
    const subset = box.querySelector("#pmSubset").value;
    if (!useRange || !txt) return { pages: [], subset, range: "" };
    const pages = parsePageRange(txt, count);
    if (!pages.length) return { error: t("pagex.badRange") };
    return { pages, subset, range: txt };
  }
  function previewPane() {
    return `
      <div class="pm-preview">
        <div class="pm-prev-frame"><img id="pmPrevImg" alt=""><div class="pm-prev-busy" id="pmPrevBusy">${t("common.loading")}</div></div>
        <div class="pm-prev-nav">
          <button type="button" id="pmPrevPg" data-i18n-title="pagex.prevPage"><i data-icon="chevron-left"></i></button>
          <span id="pmPrevLbl"></span>
          <button type="button" id="pmNextPg" data-i18n-title="pagex.nextPage"><i data-icon="chevron-right"></i></button>
        </div>
        <div class="err" id="pmPrevErr"></div>
      </div>`;
  }

  // Xem trước trực tiếp: debounce + bỏ kết quả cũ (chỉ ảnh của lần gọi mới nhất được hiện).
  function livePreview(box, count, startPage, render) {
    let page = Math.max(0, Math.min(count - 1, startPage || 0));
    let seq = 0;
    let timer = null;
    const img = box.querySelector("#pmPrevImg");
    const busy = box.querySelector("#pmPrevBusy");
    const lbl = box.querySelector("#pmPrevLbl");
    const err = box.querySelector("#pmPrevErr");
    async function run() {
      const my = ++seq;
      lbl.textContent = t("pagex.prevOf", { n: page + 1, total: count });
      busy.style.visibility = "visible";
      try {
        const url = await render(page);
        if (my !== seq) return;
        if (url) img.src = url;
        err.textContent = "";
      } catch (e) {
        if (my === seq) err.textContent = String(e);
      } finally {
        if (my === seq) busy.style.visibility = "hidden";
      }
    }
    const schedule = () => { clearTimeout(timer); timer = setTimeout(run, 280); };
    box.addEventListener("input", schedule);
    box.addEventListener("change", schedule);
    box.addEventListener("pm-change", schedule);
    box.querySelector("#pmPrevPg").addEventListener("click", () => { if (page > 0) { page--; run(); } });
    box.querySelector("#pmNextPg").addEventListener("click", () => { if (page < count - 1) { page++; run(); } });
    applyI18n(box);
    run();
    return { refresh: run };
  }

  // Luồng áp dụng chung: chọn nơi lưu → gọi lệnh → mở kết quả.
  async function applyAndOpen(box, run, doneMsg) {
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    const ok = box.querySelector("#pmOk");
    ok.disabled = true;
    status(t("common.loading"));
    try {
      await run(out);
      closeModal();
      await loadDocument(out);
      status(doneMsg(out));
    } catch (e) {
      ok.disabled = false;
      box.querySelector("#pmErr").textContent = t("pagex.err", { e });
      status("");
    }
  }

  // ---------- Hình mờ / Nền ----------
  const STAMP_DEF = {
    watermark: {
      source: "text", text: "CONFIDENTIAL", font: "sans", fontSize: 48, color: [255, 0, 0], bold: false, italic: false,
      file: "", filePage: 1, absScale: 100, rotMode: "45", rotation: 45, opacity: 50, relOn: false, rel: 50,
      behind: false, v: "center", vOff: 0, h: "center", hOff: 0, unit: "mm", range: "", subset: "all",
    },
    background: {
      source: "color", text: "", font: "sans", fontSize: 48, color: [255, 250, 230], bold: false, italic: false,
      file: "", filePage: 1, absScale: 100, rotMode: "0", rotation: 0, opacity: 100, relOn: true, rel: 100,
      behind: true, v: "center", vOff: 0, h: "center", hOff: 0, unit: "mm", range: "", subset: "all",
    },
  };

  async function openStampDialog(kind, update) {
    const base = await baseInput();
    if (!base) return;
    const isWm = kind === "watermark";
    const p = loadPrefs(kind, STAMP_DEF[kind]);
    let color = p.color.slice();
    let file = p.file || "";
    const srcOpts = isWm ? ["text", "file"] : ["color", "file"];
    if (!srcOpts.includes(p.source)) p.source = srcOpts[0];
    const box = openModal(t(isWm ? (update ? "pagex.wm.updateTitle" : "pagex.wm.title") : (update ? "pagex.bg.updateTitle" : "pagex.bg.title")), `
      <div class="pm-dialog">
        <div class="pm-form">
          ${base.isTemp ? `<p class="status">${t("org.materializedNote")}</p>` : ""}
          <fieldset class="pm-set"><legend>${t("pagex.source")}</legend>
            <div class="radiorow">
              ${srcOpts.map((s) => `<label><input type="radio" name="pmSrc" value="${s}" ${p.source === s ? "checked" : ""}> ${t("pagex.src." + s)}</label>`).join("")}
            </div>
            <div data-src="text">
              <textarea id="pmText" rows="2">${escapeHtml(p.text)}</textarea>
              <div class="row pm-inline">
                ${fontSelect("pmFont", p.font)}
                <input type="number" id="pmSize" value="${p.fontSize}" min="4" max="500" class="pm-num" data-i18n-title="pagex.fontSize">
                ${colorBtn("pmColor")}
                <label class="pm-chk"><input type="checkbox" id="pmBold" ${p.bold ? "checked" : ""}> <b>B</b></label>
                <label class="pm-chk"><input type="checkbox" id="pmItalic" ${p.italic ? "checked" : ""}> <i>I</i></label>
              </div>
            </div>
            <div data-src="color">
              <div class="row pm-inline"><label>${t("pagex.bgColor")}</label>${colorBtn("pmBgColor")}</div>
            </div>
            <div data-src="file">
              <div class="row pm-inline">
                <button type="button" id="pmPick"><i data-icon="folder-open"></i>${t("pagex.pickFile")}</button>
                <span id="pmFileName" class="muted pm-fname">${file ? escapeHtml(shortName(file)) : t("pagex.noFile")}</span>
              </div>
              <div class="row pm-inline">
                <label id="pmFilePageL">${t("pagex.filePage")}</label><input type="number" id="pmFilePage" value="${p.filePage}" min="1" class="pm-num">
                <label>${t("pagex.absScale")}</label><input type="number" id="pmAbs" value="${p.absScale}" min="1" max="1000" class="pm-num"> %
              </div>
            </div>
          </fieldset>

          <fieldset class="pm-set"><legend>${t("pagex.appearance")}</legend>
            <div class="radiorow">
              <span class="pm-lbl">${t("pagex.rotation")}</span>
              ${["0", "45", "-45", "custom"].map((r) => `<label><input type="radio" name="pmRot" value="${r}" ${p.rotMode === r ? "checked" : ""}> ${r === "custom" ? t("pagex.custom") : (r === "0" ? t("pagex.none") : r + "°")}</label>`).join("")}
              <input type="number" id="pmRotVal" value="${p.rotation}" class="pm-num" min="-360" max="360">
            </div>
            <div class="row pm-inline">
              <span class="pm-lbl">${t("pagex.opacity")}</span>
              <input type="range" id="pmOpacity" min="0" max="100" value="${p.opacity}"><span id="pmOpacityV" class="pm-val">${p.opacity}%</span>
            </div>
            <div class="row pm-inline">
              <label class="pm-chk"><input type="checkbox" id="pmRelOn" ${p.relOn ? "checked" : ""}> ${t("pagex.relScale")}</label>
              <input type="number" id="pmRel" value="${p.rel}" min="1" max="100" class="pm-num"> %
            </div>
            ${isWm ? `<div class="radiorow">
              <span class="pm-lbl">${t("pagex.location")}</span>
              <label><input type="radio" name="pmBehind" value="0" ${p.behind ? "" : "checked"}> ${t("pagex.onTop")}</label>
              <label><input type="radio" name="pmBehind" value="1" ${p.behind ? "checked" : ""}> ${t("pagex.behind")}</label>
            </div>` : ""}
          </fieldset>

          <fieldset class="pm-set"><legend>${t("common.position")}</legend>
            <div class="pm-grid2">
              <label>${t("pagex.vAlign")}</label>
              <select id="pmV">${["top", "center", "bottom"].map((v) => `<option value="${v}" ${p.v === v ? "selected" : ""}>${t("pagex.align." + v)}</option>`).join("")}</select>
              <input type="number" id="pmVOff" value="${p.vOff}" step="0.5" class="pm-num" data-i18n-title="pagex.offset">
              <label>${t("pagex.hAlign")}</label>
              <select id="pmH">${["left", "center", "right"].map((v) => `<option value="${v}" ${p.h === v ? "selected" : ""}>${t("pagex.align." + v)}</option>`).join("")}</select>
              <input type="number" id="pmHOff" value="${p.hOff}" step="0.5" class="pm-num" data-i18n-title="pagex.offset">
              <label>${t("pagex.unit")}</label>${unitSelect(p.unit)}<span></span>
            </div>
          </fieldset>

          ${pagesSection(p)}
          <label class="pm-chk pm-replace"><input type="checkbox" id="pmReplace" ${update ? "checked" : ""}> ${t(isWm ? "pagex.replaceWm" : "pagex.replaceBg")}</label>
          <div class="err" id="pmErr"></div>
        </div>
        ${previewPane()}
      </div>
      <div class="foot"><button id="pmCancel">${t("common.cancel")}</button><button id="pmOk" class="primary">${t("common.apply")}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    bindUnit(box);
    bindColor(box, "pmColor", () => color, (c) => { color = c; });
    bindColor(box, "pmBgColor", () => color, (c) => { color = c; });
    function syncVisibility() {
      const src = q("input[name=pmSrc]:checked").value;
      box.querySelectorAll("[data-src]").forEach((el) => { el.style.display = el.dataset.src === src ? "" : "none"; });
      const isPdf = /\.pdf$/i.test(file);
      q("#pmFilePage").style.display = isPdf ? "" : "none";
      q("#pmFilePageL").style.display = isPdf ? "" : "none";
      q("#pmRotVal").disabled = q("input[name=pmRot]:checked").value !== "custom";
      q("#pmRel").disabled = !q("#pmRelOn").checked;
      q("#pmAbs").disabled = q("#pmRelOn").checked;
      q("#pmOpacityV").textContent = q("#pmOpacity").value + "%";
      // Nền màu phủ kín trang — không cần xoay/tỉ lệ/vị trí.
      const solid = src === "color";
      box.querySelectorAll("#pmRotVal, input[name=pmRot], #pmRelOn, #pmV, #pmH, #pmVOff, #pmHOff").forEach((el) => {
        if (solid) el.disabled = true;
        else if (el.id !== "pmRotVal" && el.id !== "pmRel") el.disabled = false;
      });
    }
    box.addEventListener("change", syncVisibility);
    box.addEventListener("input", syncVisibility);
    q("#pmPick").addEventListener("click", async () => {
      const f = await invoke("pagex_pick_stamp_file");
      if (!f) return;
      file = f;
      q("#pmFileName").textContent = shortName(f);
      syncVisibility();
      box.dispatchEvent(new Event("pm-change"));
    });
    syncVisibility();

    function collect() {
      const src = q("input[name=pmSrc]:checked").value;
      const unit = q("#pmUnit").value;
      const k = UNITS[unit];
      const rotMode = q("input[name=pmRot]:checked").value;
      const rotation = rotMode === "custom" ? num(box, "pmRotVal", 0) : Number(rotMode);
      const pg = readPages(box, base.pageCount);
      const v = q("#pmV").value, h = q("#pmH").value;
      const anchor = v === "center" && h === "center" ? "center"
        : (v === "center" ? "middle" : v) + "-" + h;
      const prefs = {
        source: src, text: q("#pmText").value, font: q("#pmFont").value, fontSize: num(box, "pmSize", 48), color,
        bold: q("#pmBold").checked, italic: q("#pmItalic").checked, file, filePage: Math.max(1, num(box, "pmFilePage", 1)),
        absScale: num(box, "pmAbs", 100), rotMode, rotation, opacity: num(box, "pmOpacity", 100),
        relOn: q("#pmRelOn").checked, rel: num(box, "pmRel", 100), behind: isWm ? q("input[name=pmBehind]:checked").value === "1" : true,
        v, vOff: num(box, "pmVOff", 0), h, hOff: num(box, "pmHOff", 0), unit, range: pg.range || "", subset: pg.subset || "all",
      };
      const spec = {
        source: src, text: prefs.text, font: prefs.font, fontSize: prefs.fontSize, color, bold: prefs.bold, italic: prefs.italic,
        file, filePage: prefs.filePage - 1, rotationDeg: rotation, opacity: prefs.opacity / 100,
        scale: prefs.absScale / 100, relativeScale: prefs.relOn ? prefs.rel / 100 : null,
        anchor: ANCHORS.includes(anchor) ? anchor : "center",
        // Căn trên/phải: khoảng cách tính từ mép vào trong; căn giữa: dương = sang phải/lên trên.
        offsetX: prefs.hOff * k, offsetY: prefs.vOff * k,
        behind: prefs.behind, replaceExisting: q("#pmReplace").checked,
        pages: pg.pages || [], subset: pg.subset || "all",
      };
      return { spec, prefs, error: pg.error || (src === "file" && !file ? t("pagex.needFile") : (src === "text" && !prefs.text.trim() ? t("pagex.needText") : "")) };
    }
    const startPage = state.organizeMode ? (state.orgSelected.size ? Math.min(...state.orgSelected) : 0) : state.current;
    livePreview(box, base.pageCount, startPage, async (page) => {
      const c = collect();
      if (c.error) throw c.error;
      return invoke("pagex_stamp_preview", { input: base.path, kind, page, spec: c.spec, width: PREVIEW_W, password: null });
    });
    q("#pmCancel").addEventListener("click", closeModal);
    q("#pmOk").addEventListener("click", () => {
      const c = collect();
      if (c.error) { q("#pmErr").textContent = c.error; return; }
      savePrefs(kind, c.prefs);
      applyAndOpen(box, (out) => invoke("pagex_stamp_add", { input: base.path, kind, spec: c.spec, output: out, password: null }),
        (out) => t(isWm ? "pagex.wm.done" : "pagex.bg.done", { name: shortName(out) }));
    });
  }

  // ---------- Đầu & chân trang ----------
  const HF_DEF = {
    tl: "", tc: "", tr: "", bl: "", bc: "{page}", br: "", font: "sans", fontSize: 10, color: [0, 0, 0], bold: false, italic: false,
    mt: 12.7, mb: 12.7, ml: 25.4, mr: 25.4, unit: "mm", start: 1, pageFmt: "{page}", dateFmt: "dd/mm/yyyy", range: "", subset: "all",
  };
  const SLOTS = [["tl", "topLeft"], ["tc", "topCenter"], ["tr", "topRight"], ["bl", "bottomLeft"], ["bc", "bottomCenter"], ["br", "bottomRight"]];

  // Khối font + màu + lề (dùng cho Đầu/chân trang và Bates).
  function textAndMarginSection(p) {
    return `
      <fieldset class="pm-set"><legend>${t("pagex.font")}</legend>
        <div class="row pm-inline">
          ${fontSelect("pmFont", p.font)}
          <input type="number" id="pmSize" value="${p.fontSize}" min="4" max="200" class="pm-num" data-i18n-title="pagex.fontSize">
          ${colorBtn("pmColor")}
          <label class="pm-chk"><input type="checkbox" id="pmBold" ${p.bold ? "checked" : ""}> <b>B</b></label>
          <label class="pm-chk"><input type="checkbox" id="pmItalic" ${p.italic ? "checked" : ""}> <i>I</i></label>
        </div>
      </fieldset>
      <fieldset class="pm-set"><legend>${t("pagex.margins")}</legend>
        <div class="pm-grid4">
          <label>${t("pagex.mTop")}</label><input type="number" id="pmMT" value="${p.mt}" step="0.5" min="0" class="pm-num">
          <label>${t("pagex.mBottom")}</label><input type="number" id="pmMB" value="${p.mb}" step="0.5" min="0" class="pm-num">
          <label>${t("pagex.mLeft")}</label><input type="number" id="pmML" value="${p.ml}" step="0.5" min="0" class="pm-num">
          <label>${t("pagex.mRight")}</label><input type="number" id="pmMR" value="${p.mr}" step="0.5" min="0" class="pm-num">
        </div>
        <div class="row pm-inline"><label>${t("pagex.unit")}</label>${unitSelect(p.unit)}</div>
      </fieldset>`;
  }
  function readTextAndMargins(box, color) {
    const k = UNITS[box.querySelector("#pmUnit").value];
    return {
      font: box.querySelector("#pmFont").value,
      fontSize: num(box, "pmSize", 10),
      color: [color[0], color[1], color[2], 255],
      bold: box.querySelector("#pmBold").checked,
      italic: box.querySelector("#pmItalic").checked,
      marginTop: num(box, "pmMT", 0) * k,
      marginBottom: num(box, "pmMB", 0) * k,
      marginLeft: num(box, "pmML", 0) * k,
      marginRight: num(box, "pmMR", 0) * k,
    };
  }
  function textAndMarginPrefs(box, color) {
    return {
      font: box.querySelector("#pmFont").value, fontSize: num(box, "pmSize", 10), color, bold: box.querySelector("#pmBold").checked,
      italic: box.querySelector("#pmItalic").checked, mt: num(box, "pmMT", 0), mb: num(box, "pmMB", 0), ml: num(box, "pmML", 0),
      mr: num(box, "pmMR", 0), unit: box.querySelector("#pmUnit").value,
    };
  }

  async function openHfDialog(update) {
    const base = await baseInput();
    if (!base) return;
    const p = loadPrefs("hf", HF_DEF);
    let color = p.color.slice();
    const box = openModal(t(update ? "pagex.hf.updateTitle" : "pagex.hf.title"), `
      <div class="pm-dialog">
        <div class="pm-form">
          ${base.isTemp ? `<p class="status">${t("org.materializedNote")}</p>` : ""}
          <fieldset class="pm-set"><legend>${t("pagex.hf.text")}</legend>
            <div class="pm-slots">
              <span></span><span class="pm-colh">${t("pagex.align.left")}</span><span class="pm-colh">${t("pagex.align.center")}</span><span class="pm-colh">${t("pagex.align.right")}</span>
              <span class="pm-rowh">${t("pagex.hf.header")}</span>
              ${SLOTS.slice(0, 3).map(([k]) => `<input type="text" id="pmS_${k}" value="${escapeHtml(p[k])}">`).join("")}
              <span class="pm-rowh">${t("pagex.hf.footer")}</span>
              ${SLOTS.slice(3).map(([k]) => `<input type="text" id="pmS_${k}" value="${escapeHtml(p[k])}">`).join("")}
            </div>
            <div class="row pm-inline">
              <select id="pmPageFmt">${PAGE_FORMATS.map((f) => `<option value="${escapeHtml(f)}" ${f === p.pageFmt ? "selected" : ""}>${escapeHtml(f.replace("{page}", "1").replace("{total}", "n"))}</option>`).join("")}</select>
              <button type="button" id="pmInsPage"><i data-icon="plus"></i>${t("pagex.hf.insPage")}</button>
            </div>
            <div class="row pm-inline">
              <select id="pmDateFmt">${DATE_FORMATS.map((f) => `<option value="${f}" ${f === p.dateFmt ? "selected" : ""}>${f} — ${fmtDate(f, new Date())}</option>`).join("")}</select>
              <button type="button" id="pmInsDate"><i data-icon="plus"></i>${t("pagex.hf.insDate")}</button>
            </div>
            <div class="row pm-inline"><label>${t("pagex.hf.start")}</label><input type="number" id="pmStart" value="${p.start}" min="0" class="pm-num"></div>
            <p class="status">${t("pagex.hf.tokens")}</p>
          </fieldset>
          ${textAndMarginSection(p)}
          ${pagesSection(p)}
          <label class="pm-chk pm-replace"><input type="checkbox" id="pmReplace" ${update ? "checked" : ""}> ${t("pagex.replaceHf")}</label>
          <div class="err" id="pmErr"></div>
        </div>
        ${previewPane()}
      </div>
      <div class="foot"><button id="pmCancel">${t("common.cancel")}</button><button id="pmOk" class="primary">${t("common.apply")}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    bindUnit(box);
    bindColor(box, "pmColor", () => color, (c) => { color = c; });
    let lastSlot = q("#pmS_bc");
    box.querySelectorAll(".pm-slots input").forEach((inp) => inp.addEventListener("focus", () => { lastSlot = inp; }));
    const insert = (tok) => {
      const s = lastSlot.selectionStart ?? lastSlot.value.length;
      const e2 = lastSlot.selectionEnd ?? s;
      lastSlot.value = lastSlot.value.slice(0, s) + tok + lastSlot.value.slice(e2);
      lastSlot.focus();
      lastSlot.setSelectionRange(s + tok.length, s + tok.length);
      box.dispatchEvent(new Event("pm-change"));
    };
    q("#pmInsPage").addEventListener("click", () => insert(q("#pmPageFmt").value));
    q("#pmInsDate").addEventListener("click", () => insert("{date}"));

    function collect() {
      const pg = readPages(box, base.pageCount);
      const slots = {};
      for (const [k] of SLOTS) slots[k] = q("#pmS_" + k).value;
      const spec = Object.assign({}, readTextAndMargins(box, color), {
        topLeft: slots.tl, topCenter: slots.tc, topRight: slots.tr, bottomLeft: slots.bl, bottomCenter: slots.bc, bottomRight: slots.br,
        date: fmtDate(q("#pmDateFmt").value, new Date()), startNumber: Math.max(0, num(box, "pmStart", 1)),
        replaceExisting: q("#pmReplace").checked, pages: pg.pages || [], subset: pg.subset || "all",
      });
      const prefs = Object.assign({}, slots, textAndMarginPrefs(box, color), {
        start: spec.startNumber, pageFmt: q("#pmPageFmt").value, dateFmt: q("#pmDateFmt").value, range: pg.range || "", subset: pg.subset || "all",
      });
      const empty = Object.values(slots).every((s) => !s.trim());
      return { spec, prefs, error: pg.error || (empty ? t("pagex.hf.needText") : "") };
    }
    const startPage = state.organizeMode ? (state.orgSelected.size ? Math.min(...state.orgSelected) : 0) : state.current;
    livePreview(box, base.pageCount, startPage, async (page) => {
      const c = collect();
      if (c.error) throw c.error;
      return invoke("pagex_hf_preview", { input: base.path, page, spec: c.spec, width: PREVIEW_W, password: null });
    });
    q("#pmCancel").addEventListener("click", closeModal);
    q("#pmOk").addEventListener("click", () => {
      const c = collect();
      if (c.error) { q("#pmErr").textContent = c.error; return; }
      savePrefs("hf", c.prefs);
      applyAndOpen(box, (out) => invoke("pagex_hf_add", { input: base.path, spec: c.spec, output: out, password: null }),
        (out) => t("pagex.hf.done", { name: shortName(out) }));
    });
  }

  // ---------- Đánh số Bates ----------
  const BATES_DEF = {
    prefix: "", suffix: "", start: 1, digits: 6, slot: "br", font: "sans", fontSize: 10, color: [0, 0, 0], bold: false, italic: false,
    mt: 12.7, mb: 12.7, ml: 25.4, mr: 25.4, unit: "mm", nameSuffix: "_bates",
  };
  async function openBatesDialog(update) {
    const p = loadPrefs("bates", BATES_DEF);
    let color = p.color.slice();
    // Danh sách tệp: tài liệu đang mở (theo trạng thái đang xem) + các tệp thêm.
    const files = [];
    if (state.path) files.push({ current: true, input: null, name: state.origPath || state.path });
    const box = openModal(t(update ? "pagex.bates.updateTitle" : "pagex.bates.title"), `
      <div class="pm-dialog">
        <div class="pm-form">
          <fieldset class="pm-set"><legend>${t("pagex.bates.files")}</legend>
            <div id="pmFiles" class="pm-files"></div>
            <button type="button" id="pmAddFiles"><i data-icon="plus"></i>${t("pagex.bates.addFiles")}</button>
          </fieldset>
          <fieldset class="pm-set"><legend>${t("pagex.bates.format")}</legend>
            <div class="pm-grid4">
              <label>${t("pagex.bates.prefix")}</label><input type="text" id="pmPrefix" value="${escapeHtml(p.prefix)}">
              <label>${t("pagex.bates.suffix")}</label><input type="text" id="pmSuffix" value="${escapeHtml(p.suffix)}">
              <label>${t("pagex.bates.start")}</label><input type="number" id="pmBStart" value="${p.start}" min="0" class="pm-num">
              <label>${t("pagex.bates.digits")}</label><input type="number" id="pmDigits" value="${p.digits}" min="1" max="15" class="pm-num">
            </div>
            <div class="row pm-inline"><label>${t("common.position")}</label>
              <select id="pmSlot">${SLOTS.map(([k]) => `<option value="${k}" ${p.slot === k ? "selected" : ""}>${t("pagex.slot." + k)}</option>`).join("")}</select>
            </div>
            <p class="status">${t("pagex.bates.sample")} <b id="pmSample"></b></p>
          </fieldset>
          ${textAndMarginSection(p)}
          <fieldset class="pm-set" id="pmOutSet"><legend>${t("pagex.bates.output")}</legend>
            <div class="row pm-inline"><label>${t("pagex.bates.nameSuffix")}</label><input type="text" id="pmNameSuffix" value="${escapeHtml(p.nameSuffix)}"></div>
            <p class="status">${t("pagex.bates.outputNote")}</p>
          </fieldset>
          <label class="pm-chk pm-replace"><input type="checkbox" id="pmReplace" ${update ? "checked" : ""}> ${t("pagex.replaceBates")}</label>
          <div class="err" id="pmErr"></div>
        </div>
        ${previewPane()}
      </div>
      <div class="foot"><button id="pmCancel">${t("common.cancel")}</button><button id="pmOk" class="primary">${t("common.apply")}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    bindUnit(box);
    bindColor(box, "pmColor", () => color, (c) => { color = c; });
    function renderFiles() {
      q("#pmFiles").innerHTML = files.length ? files.map((f, i) =>
        `<div class="pm-file"><span class="pm-fi">${i + 1}.</span><span class="pm-fn" title="${escapeHtml(f.name)}">${escapeHtml(shortName(f.name))}${f.current ? ` <em class="muted">(${t("pagex.bates.current")})</em>` : ""}</span>` +
        `<button type="button" data-up="${i}" ${i === 0 ? "disabled" : ""} data-i18n-title="orgx.moveUp"><i data-icon="chevron-up"></i></button>` +
        `<button type="button" data-down="${i}" ${i === files.length - 1 ? "disabled" : ""} data-i18n-title="orgx.moveDown"><i data-icon="chevron-down"></i></button>` +
        `<button type="button" data-rm="${i}" data-i18n-title="orgx.remove"><i data-icon="win-close"></i></button></div>`).join("")
        : `<p class="muted">${t("pagex.bates.noFiles")}</p>`;
      applyIcons(q("#pmFiles"));
      applyI18n(q("#pmFiles"));
      q("#pmOutSet").style.display = files.length > 1 || (files.length === 1 && !files[0].current) ? "" : "none";
    }
    q("#pmFiles").addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      if (b.dataset.up != null) { const i = +b.dataset.up; [files[i - 1], files[i]] = [files[i], files[i - 1]]; }
      else if (b.dataset.down != null) { const i = +b.dataset.down; [files[i + 1], files[i]] = [files[i], files[i + 1]]; }
      else if (b.dataset.rm != null) files.splice(+b.dataset.rm, 1);
      renderFiles();
      box.dispatchEvent(new Event("pm-change"));
    });
    q("#pmAddFiles").addEventListener("click", async () => {
      const picked = await invoke("pagex_pick_pdfs");
      for (const f of picked || []) if (!files.some((x) => !x.current && x.name === f)) files.push({ current: false, input: f, name: f });
      renderFiles();
      box.dispatchEvent(new Event("pm-change"));
    });
    renderFiles();

    function bates() {
      return {
        prefix: q("#pmPrefix").value, suffix: q("#pmSuffix").value,
        start: Math.max(0, Math.floor(num(box, "pmBStart", 1))), digits: Math.max(1, Math.min(15, Math.floor(num(box, "pmDigits", 6)))),
      };
    }
    function collect() {
      const b = bates();
      q("#pmSample").textContent = b.prefix + String(b.start).padStart(b.digits, "0") + b.suffix;
      const slotName = SLOTS.find(([k]) => k === q("#pmSlot").value)[1];
      const spec = Object.assign({}, readTextAndMargins(box, color), {
        [slotName]: "{bates}", bates: b, replaceExisting: q("#pmReplace").checked,
      });
      const prefs = Object.assign({}, textAndMarginPrefs(box, color), b, { slot: q("#pmSlot").value, nameSuffix: q("#pmNameSuffix").value });
      return { spec, prefs, error: files.length ? "" : t("pagex.bates.noFiles") };
    }
    // Xem trước: tệp đầu danh sách (tài liệu đang mở dùng đúng trạng thái đang xem).
    let prevBase = null;
    async function previewInput() {
      const f = files[0];
      if (!f) return null;
      if (f.current) { prevBase = prevBase || await baseInput(); return prevBase; }
      const meta = await invoke("open_document", { path: f.input });
      return { path: f.input, pageCount: meta.pageCount };
    }
    const first = await previewInput();
    livePreview(box, first ? first.pageCount : 1, 0, async (page) => {
      const c = collect();
      if (c.error) throw c.error;
      const inp = await previewInput();
      return invoke("pagex_hf_preview", { input: inp.path, page: Math.min(page, inp.pageCount - 1), spec: c.spec, width: PREVIEW_W, password: null });
    });
    q("#pmCancel").addEventListener("click", closeModal);
    q("#pmOk").addEventListener("click", async () => {
      const c = collect();
      if (c.error) { q("#pmErr").textContent = c.error; return; }
      savePrefs("bates", c.prefs);
      const onlyCurrent = files.length === 1 && files[0].current;
      let output = null, outDir = null;
      if (onlyCurrent) {
        output = await invoke("pick_save_pdf");
        if (!output) return;
      } else {
        outDir = await invoke("pick_dir");
        if (!outDir) return;
      }
      const ok = q("#pmOk");
      ok.disabled = true;
      status(t("common.loading"));
      try {
        const jobs = [];
        for (const f of files) {
          if (f.current) { const b = await baseInput(); jobs.push({ input: b.path, name: f.name }); }
          else jobs.push({ input: f.input, name: f.name });
        }
        const ranges = await invoke("pagex_bates_add", { jobs, spec: c.spec, output, outDir, nameSuffix: c.prefs.nameSuffix || "" });
        closeModal();
        const last = ranges[ranges.length - 1];
        const summary = t("pagex.bates.done", { n: ranges.length, first: ranges[0].first, last: last.last });
        if (onlyCurrent) {
          await loadDocument(output);
        }
        status(summary);
      } catch (e) {
        ok.disabled = false;
        q("#pmErr").textContent = t("pagex.err", { e });
        status("");
      }
    });
  }

  // ---------- Gỡ ----------
  const COUNT_FIELD = { watermark: "watermark", background: "background", headerFooter: "headerFooter", bates: "bates" };
  async function removeMarks(kind) {
    const base = await baseInput();
    if (!base) return;
    status(t("common.loading"));
    let counts;
    try {
      counts = await invoke("pagex_marks_scan", { input: base.path, password: null });
    } catch (e) {
      status(t("pagex.err", { e }));
      return;
    }
    const n = counts[COUNT_FIELD[kind]] || 0;
    status("");
    if (!n) { hint(t("pagex.none." + kind)); status(t("pagex.none." + kind)); return; }
    if (!(await confirmUnsavedAnnots())) return;
    const ok = await confirmModal(t("pagex.remove.title." + kind), t("pagex.remove.confirm", { n }), t("pagex.menu.remove"));
    if (!ok) return;
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    status(t("common.loading"));
    try {
      await invoke("pagex_marks_remove", { input: base.path, kind, output: out, password: null });
      await loadDocument(out);
      status(t("pagex.remove.done." + kind, { name: shortName(out) }));
    } catch (e) {
      status(t("pagex.err", { e }));
    }
  }

  // ---------- Nhân bản / đảo thứ tự trang (lưới Trang) ----------
  function orgDuplicate() {
    if (!state.orgSelected.size) { hint(t("pagex.noSelDup")); return; }
    pushUndo();
    const sel = Array.from(state.orgSelected).sort((a, b) => a - b);
    const next = [];
    const copies = new Set();
    state.pagePlan.forEach((e, i) => {
      next.push(e);
      if (state.orgSelected.has(i)) {
        copies.add(next.length);
        next.push(JSON.parse(JSON.stringify(e)));
      }
    });
    state.pagePlan = next;
    state.orgSelected = copies;
    state.orgAnchor = null;
    buildOrganizeGrid();
    hint(t("pagex.dupDone", { n: sel.length }));
  }
  function orgReverse() {
    if (state.pagePlan.length < 2) return;
    pushUndo();
    if (state.orgSelected.size >= 2) {
      // Chỉ đảo thứ tự giữa các trang đang chọn (giữ nguyên vị trí các khe).
      const idx = Array.from(state.orgSelected).sort((a, b) => a - b);
      const vals = idx.map((i) => state.pagePlan[i]).reverse();
      idx.forEach((i, k) => { state.pagePlan[i] = vals[k]; });
      hint(t("pagex.revSelDone", { n: idx.length }));
    } else {
      state.pagePlan.reverse();
      state.orgSelected = new Set();
      hint(t("pagex.revAllDone"));
    }
    buildOrganizeGrid();
  }
  $("orgDuplicate").addEventListener("click", orgDuplicate);
  $("orgReverse").addEventListener("click", orgReverse);
})();
