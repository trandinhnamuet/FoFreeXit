// So sánh tài liệu (Compare Files) — như Foxit: chọn tệp Cũ/Mới + tuỳ chọn →
// màn hình kết quả riêng: 2 cột trang đồng bộ (căn theo trang khớp, trang
// chèn/xoá có ô trống), tô màu thay đổi (chèn xanh lá, xoá đỏ, thay cam, đồ hoạ
// xanh dương), bảng khác biệt bên phải (lọc theo loại, bấm để nhảy cả 2 cột),
// nút trước/sau, xuất báo cáo PDF có chú thích. Diff chạy nền trong Rust.
(function () {
  const KINDS = ["replaced", "inserted", "deleted", "pageInserted", "pageDeleted", "graphics"];
  const PREF_KEY = "ff.compare.opts";
  const cmp = {
    res: null,        // CompareResultDto
    oldPath: null, newPath: null,
    zoom: 1, scale: 1,
    sel: -1,          // id thay đổi đang chọn
    filter: new Set(KINDS),
    byPage: new Map(),  // "o:3" / "n:5" → [{c, r}]
    gen: 0,           // thế hệ bố cục (đổi zoom/kích thước → render lại)
    obs: null, queue: [], active: 0,
    running: false,
    lastOpts: null,
  };
  const stage = $("compareStage");

  function loadPrefs() {
    try { return JSON.parse(localStorage.getItem(PREF_KEY) || "null") || {}; } catch (_) { return {}; }
  }
  function savePrefs(p) {
    try { localStorage.setItem(PREF_KEY, JSON.stringify(p)); } catch (_) {}
  }
  const kindLabel = (k) => t("cmp.kind." + k);
  const clip = (s, n) => (s.length > n ? s.slice(0, n) + "…" : s);

  // ---------------- Hộp thoại chọn tệp + tuỳ chọn ----------------

  async function pageCountOf(path) {
    if (path && path === state.path) return state.pages.length;
    const meta = await invoke("open_document", { path });
    return meta.pageCount;
  }

  function openDialog(prefill) {
    if (cmp.running) return;
    const prefs = Object.assign({ mode: "both", ignoreCase: false, ignoreWhitespace: true, ignorePunctuation: false, dpi: 72 }, loadPrefs());
    const files = {
      old: { path: (prefill && prefill.oldPath) || state.path || null, count: null },
      new: { path: (prefill && prefill.newPath) || null, count: null },
    };
    const box = openModal(t("cmp.dlgTitle"), `
      <div class="cmp-dlg">
        <label>${t("cmp.oldFile")}</label>
        <div class="cmp-file"><span id="cmpOldName" class="cmp-fname"></span><button id="cmpOldPick" type="button"><i data-icon="folder-open"></i>${t("cmp.browse")}</button></div>
        <label>${t("cmp.newFile")}</label>
        <div class="cmp-file"><span id="cmpNewName" class="cmp-fname"></span><button id="cmpNewPick" type="button"><i data-icon="folder-open"></i>${t("cmp.browse")}</button></div>
        <div class="cmp-swaprow"><button id="cmpSwap" type="button"><i data-icon="swap"></i>${t("cmp.swap")}</button></div>
        <label>${t("cmp.mode")}</label>
        <div class="cmp-modes">
          <label><input type="radio" name="cmpMode" value="text"> ${t("cmp.modeText")}</label>
          <label><input type="radio" name="cmpMode" value="both"> ${t("cmp.modeBoth")}</label>
          <label><input type="radio" name="cmpMode" value="visual"> ${t("cmp.modeVisual")}</label>
        </div>
        <div class="row">
          <div><label>${t("cmp.oldRange")}</label><input id="cmpOldRange" type="text" placeholder="${t("cmp.rangeAll")}"></div>
          <div><label>${t("cmp.newRange")}</label><input id="cmpNewRange" type="text" placeholder="${t("cmp.rangeAll")}"></div>
        </div>
        <label>${t("cmp.options")}</label>
        <div class="cmp-checks">
          <label><input type="checkbox" id="cmpIgnCase"> ${t("cmp.ignoreCase")}</label>
          <label><input type="checkbox" id="cmpIgnWs"> ${t("cmp.ignoreWhitespace")}</label>
          <label><input type="checkbox" id="cmpIgnPunct"> ${t("cmp.ignorePunct")}</label>
        </div>
        <label>${t("cmp.sensitivity")}</label>
        <select id="cmpDpi">
          <option value="50">${t("cmp.sensLow")}</option>
          <option value="72">${t("cmp.sensNormal")}</option>
          <option value="110">${t("cmp.sensHigh")}</option>
        </select>
        <div class="err" id="cmpErr"></div>
        <div class="foot"><button id="cmpCancel">${t("common.cancel")}</button><button id="cmpGo" class="primary"><i data-icon="compare"></i>${t("cmp.go")}</button></div>
      </div>
    `);
    const q = (s) => box.querySelector(s);
    const err = q("#cmpErr");
    q(`input[name=cmpMode][value=${prefs.mode}]`).checked = true;
    q("#cmpIgnCase").checked = !!prefs.ignoreCase;
    q("#cmpIgnWs").checked = prefs.ignoreWhitespace !== false;
    q("#cmpIgnPunct").checked = !!prefs.ignorePunctuation;
    q("#cmpDpi").value = String(prefs.dpi || 72);
    if (!q("#cmpDpi").value) q("#cmpDpi").value = "72";

    const syncMode = () => {
      const mode = q("input[name=cmpMode]:checked").value;
      q("#cmpDpi").disabled = mode === "text";
      for (const id of ["#cmpIgnCase", "#cmpIgnWs", "#cmpIgnPunct"]) q(id).disabled = mode === "visual";
    };
    box.querySelectorAll("input[name=cmpMode]").forEach((r) => r.addEventListener("change", syncMode));
    syncMode();

    async function refreshFile(side) {
      const f = files[side];
      const el = q(side === "old" ? "#cmpOldName" : "#cmpNewName");
      if (!f.path) {
        el.textContent = t("cmp.noFile");
        el.classList.add("empty");
        el.title = "";
        return;
      }
      el.classList.remove("empty");
      el.textContent = shortName(f.path);
      el.title = f.path;
      try {
        f.count = await pageCountOf(f.path);
        el.textContent = `${shortName(f.path)} — ${t("cmp.pagesCount", { n: f.count })}`;
      } catch (e) {
        f.count = null;
        err.textContent = t("cmp.errOpen", { name: shortName(f.path), e });
      }
    }
    const pick = async (side) => {
      const p = await invoke("pick_pdf");
      if (!p) return;
      err.textContent = "";
      files[side].path = p;
      await refreshFile(side);
    };
    q("#cmpOldPick").addEventListener("click", () => pick("old"));
    q("#cmpNewPick").addEventListener("click", () => pick("new"));
    q("#cmpSwap").addEventListener("click", () => {
      [files.old, files.new] = [files.new, files.old];
      const [a, b] = [q("#cmpOldRange").value, q("#cmpNewRange").value];
      q("#cmpOldRange").value = b;
      q("#cmpNewRange").value = a;
      refreshFile("old");
      refreshFile("new");
    });
    q("#cmpCancel").addEventListener("click", closeModal);
    q("#cmpGo").addEventListener("click", () => {
      err.textContent = "";
      if (!files.old.path || !files.new.path) { err.textContent = t("cmp.errNeedFiles"); return; }
      if (files.old.path === files.new.path) { err.textContent = t("cmp.errSame"); return; }
      if (files.old.count == null || files.new.count == null) { err.textContent = t("cmp.errNotReady"); return; }
      const range = (inp, count) => {
        const s = inp.value.trim();
        if (!s) return { ok: true, pages: null };
        const pages = parsePageRange(s, count);
        return { ok: pages.length > 0, pages };
      };
      const ro = range(q("#cmpOldRange"), files.old.count);
      const rn = range(q("#cmpNewRange"), files.new.count);
      if (!ro.ok || !rn.ok) { err.textContent = t("cmp.errRange"); return; }
      const mode = q("input[name=cmpMode]:checked").value;
      const opts = {
        mode,
        oldPages: ro.pages,
        newPages: rn.pages,
        ignoreCase: q("#cmpIgnCase").checked,
        ignoreWhitespace: q("#cmpIgnWs").checked,
        ignorePunctuation: q("#cmpIgnPunct").checked,
        visualDpi: Number(q("#cmpDpi").value) || 72,
      };
      savePrefs({ mode, ignoreCase: opts.ignoreCase, ignoreWhitespace: opts.ignoreWhitespace, ignorePunctuation: opts.ignorePunctuation, dpi: opts.visualDpi });
      closeModal();
      runCompare(files.old.path, files.new.path, opts);
    });
    refreshFile("old");
    refreshFile("new");
  }

  // ---------------- Vào / rời màn hình so sánh ----------------

  function showStage() {
    // Rời chế độ khác (Sửa/Tổ chức trang/Form…) về Trang chủ trước.
    if (window.Shell && Shell.currentTab !== "home" && Shell.currentTab !== "tools") Shell.setTab("home");
    if (state.editMode || state.organizeMode) return false; // người dùng không đồng ý rời chế độ Sửa
    if (state.tool) setTool(null);
    for (const id of ["viewport", "organizeGrid", "editStage", "sidebar"]) $(id).classList.add("hidden");
    stage.classList.remove("hidden");
    document.body.classList.add("comparing");
    return true;
  }

  function closeStage() {
    if (stage.classList.contains("hidden")) return;
    if (cmp.running) invoke("compare_cancel").catch(() => {});
    if (cmp.obs) { cmp.obs.disconnect(); cmp.obs = null; }
    cmp.gen++;
    cmp.queue = [];
    $("cmpRows").innerHTML = "";
    $("cmpList").innerHTML = "";
    stage.classList.add("hidden");
    document.body.classList.remove("comparing");
    $("viewport").classList.remove("hidden");
    $("sidebar").classList.remove("hidden");
  }

  // ---------------- Chạy so sánh (nền) ----------------

  function setBusy(on, text, frac) {
    $("cmpBusy").classList.toggle("hidden", !on);
    if (text != null) $("cmpBusyText").textContent = text;
    if (frac != null) $("cmpBusyFill").style.width = `${Math.round(Math.max(0, Math.min(1, frac)) * 100)}%`;
  }

  async function runCompare(oldPath, newPath, opts) {
    if (!showStage()) return;
    cmp.running = true;
    cmp.lastOpts = opts;
    $("cmpOldHead").textContent = shortName(oldPath);
    $("cmpOldHead").title = oldPath;
    $("cmpNewHead").textContent = shortName(newPath);
    $("cmpNewHead").title = newPath;
    $("cmpRows").innerHTML = "";
    $("cmpList").innerHTML = "";
    $("cmpChips").innerHTML = "";
    $("cmpTotal").textContent = "";
    setBusy(true, t("cmp.starting"), 0);
    $("status").textContent = t("cmp.running");
    // Trọng số từng giai đoạn trên thanh tiến độ tổng.
    const W = opts.mode === "text" ? { text: [0, 0.8], diff: [0.8, 0.2], visual: [1, 0] }
      : { text: [0, 0.35], diff: [0.35, 0.1], visual: [0.45, 0.55] };
    let unlisten = null;
    try {
      const ev = window.__TAURI__ && window.__TAURI__.event;
      if (ev) {
        unlisten = await ev.listen("compare-progress", (e) => {
          const p = e.payload || {};
          const w = W[p.stage] || [0, 1];
          const f = p.total ? p.done / p.total : 0;
          setBusy(true, t("cmp.stage." + p.stage, { done: p.done, total: p.total }), w[0] + w[1] * f);
        });
      }
    } catch (_) {}
    try {
      const res = await invoke("compare_run", { oldPath, newPath, options: opts });
      cmp.res = res;
      cmp.oldPath = oldPath;
      cmp.newPath = newPath;
      cmp.sel = -1;
      cmp.zoom = 1;
      cmp.filter = new Set(KINDS);
      setBusy(false);
      buildAll();
      $("status").textContent = res.summary.total
        ? t("cmp.doneStatus", { n: res.summary.total, s: (res.elapsedMs / 1000).toFixed(1) })
        : t("cmp.noDiffStatus");
      if (res.changes.length) selectChange(visibleChanges()[0].id, true);
    } catch (e) {
      setBusy(false);
      const msg = String(e);
      if (msg.includes("compare-cancelled")) {
        $("status").textContent = t("cmp.cancelled");
      } else {
        $("status").textContent = t("cmp.errRun", { e: msg });
      }
      cmp.res = null;
      closeStage();
    } finally {
      cmp.running = false;
      if (unlisten) unlisten();
    }
  }

  // ---------------- Dựng 2 cột trang ----------------

  function pageSize(side, idx) {
    const arr = side === "o" ? cmp.res.oldPages : cmp.res.newPages;
    return arr[idx] || { widthPt: 612, heightPt: 792 };
  }

  function computeScale() {
    const sc = $("cmpScroll");
    const colW = Math.max(120, (sc.clientWidth - 24 - 12) / 2);
    let maxW = 1;
    for (const p of cmp.res.pageMap) {
      if (p.old != null) maxW = Math.max(maxW, pageSize("o", p.old).widthPt);
      if (p.new != null) maxW = Math.max(maxW, pageSize("n", p.new).widthPt);
    }
    return Math.max(0.05, (colW - 8) / maxW) * cmp.zoom;
  }

  function indexChanges() {
    cmp.byPage = new Map();
    const add = (key, c, r) => {
      if (!cmp.byPage.has(key)) cmp.byPage.set(key, []);
      cmp.byPage.get(key).push({ c, r });
    };
    for (const c of cmp.res.changes) {
      if (c.oldPage != null) for (const r of c.oldRects) add("o:" + c.oldPage, c, r);
      if (c.newPage != null) for (const r of c.newRects) add("n:" + c.newPage, c, r);
    }
  }

  function marksHtml(side, idx, hPt) {
    const s = cmp.scale;
    const list = cmp.byPage.get(side + ":" + idx) || [];
    return list.map(({ c, r }) => {
      const full = c.kind === "pageInserted" || c.kind === "pageDeleted";
      const caret = !full && r.right - r.left < 3;
      const cls = `cmp-mk k-${c.kind}${full ? " full" : ""}${caret ? " caret" : ""}${cmp.filter.has(c.kind) ? "" : " off"}${c.id === cmp.sel ? " sel" : ""}`;
      const pad = caret ? 0 : 1;
      const left = r.left * s - pad, top = (hPt - r.top) * s - pad;
      const w = Math.max(caret ? 3 : 2, (r.right - r.left) * s + 2 * pad), h = Math.max(4, (r.top - r.bottom) * s + 2 * pad);
      return `<div class="${cls}" data-id="${c.id}" style="left:${left.toFixed(1)}px;top:${top.toFixed(1)}px;width:${w.toFixed(1)}px;height:${h.toFixed(1)}px"></div>`;
    }).join("");
  }

  function slotHtml(side, idx) {
    const { widthPt, heightPt } = pageSize(side, idx);
    const s = cmp.scale;
    return `<div class="cmp-page" data-side="${side}" data-page="${idx}" style="width:${(widthPt * s).toFixed(0)}px;height:${(heightPt * s).toFixed(0)}px">` +
      `<img alt="" draggable="false"><div class="cmp-ov">${marksHtml(side, idx, heightPt)}</div>` +
      `<span class="cmp-pn">${idx + 1}</span></div>`;
  }

  function missingHtml(otherSide, otherIdx) {
    const { widthPt, heightPt } = pageSize(otherSide, otherIdx);
    const s = cmp.scale;
    const msg = otherSide === "n" ? t("cmp.onlyInNew", { n: otherIdx + 1 }) : t("cmp.onlyInOld", { n: otherIdx + 1 });
    return `<div class="cmp-missing" style="width:${(widthPt * s).toFixed(0)}px;height:${(heightPt * s).toFixed(0)}px"><span>${escapeHtml(msg)}</span></div>`;
  }

  function buildRows(keepRatio) {
    const sc = $("cmpScroll");
    const ratio = keepRatio && sc.scrollHeight > 0 ? sc.scrollTop / sc.scrollHeight : 0;
    cmp.gen++;
    cmp.queue = [];
    cmp.scale = computeScale();
    const html = cmp.res.pageMap.map((p, i) =>
      `<div class="cmp-row" data-row="${i}">` +
      `<div class="cmp-cell">${p.old != null ? slotHtml("o", p.old) : missingHtml("n", p.new)}</div>` +
      `<div class="cmp-cell">${p.new != null ? slotHtml("n", p.new) : missingHtml("o", p.old)}</div>` +
      `</div>`).join("");
    $("cmpRows").innerHTML = html;
    $("cmpZoomLbl").textContent = `${Math.round(cmp.zoom * 100)}%`;
    if (keepRatio) sc.scrollTop = ratio * sc.scrollHeight;
    if (cmp.obs) cmp.obs.disconnect();
    cmp.obs = new IntersectionObserver(onRowsVisible, { root: sc, rootMargin: "700px 0px" });
    $("cmpRows").querySelectorAll(".cmp-row").forEach((r) => cmp.obs.observe(r));
  }

  function onRowsVisible(entries) {
    for (const en of entries) {
      if (!en.isIntersecting) continue;
      en.target.querySelectorAll(".cmp-page").forEach((pg) => {
        if (pg.dataset.gen !== String(cmp.gen) && !pg.dataset.queued) {
          pg.dataset.queued = "1";
          cmp.queue.push(pg);
        }
      });
    }
    pump();
  }

  // Render lười, tối đa 2 trang cùng lúc (không nghẽn tài liệu 100+ trang).
  function pump() {
    while (cmp.active < 2 && cmp.queue.length) {
      const pg = cmp.queue.shift();
      if (!pg.isConnected) continue;
      const gen = cmp.gen;
      const path = pg.dataset.side === "o" ? cmp.oldPath : cmp.newPath;
      const width = Math.min(2600, Math.round(pg.clientWidth * (window.devicePixelRatio || 1)));
      cmp.active++;
      invoke("render_page", { path, page: Number(pg.dataset.page), width })
        .then((url) => {
          if (gen !== cmp.gen || !pg.isConnected) return;
          pg.querySelector("img").src = url;
          pg.dataset.gen = String(gen);
        })
        .catch(() => {})
        .finally(() => {
          delete pg.dataset.queued;
          cmp.active--;
          pump();
        });
    }
  }

  // ---------------- Bảng khác biệt ----------------

  function visibleChanges() {
    return cmp.res ? cmp.res.changes.filter((c) => cmp.filter.has(c.kind)) : [];
  }

  function buildSummary() {
    const sm = cmp.res.summary;
    $("cmpTotal").textContent = sm.total ? t("cmp.total", { n: sm.total }) : t("cmp.noDiff");
    const count = { replaced: sm.replaced, inserted: sm.inserted, deleted: sm.deleted, pageInserted: sm.pageInserted, pageDeleted: sm.pageDeleted, graphics: sm.graphics };
    $("cmpChips").innerHTML = KINDS.filter((k) => count[k] > 0 || ["replaced", "inserted", "deleted"].includes(k)).map((k) =>
      `<button class="cmp-chip k-${k}${cmp.filter.has(k) ? " on" : ""}" data-kind="${k}" title="${escapeHtml(t("cmp.filterTip"))}">` +
      `<span class="cmp-dot"></span>${escapeHtml(kindLabel(k))}<b>${count[k]}</b></button>`).join("");
  }

  function pageLabel(v) { return v == null ? "—" : String(v + 1); }

  function itemHtml(c) {
    let snip = "";
    const o = escapeHtml(clip(c.oldText || "", 160)), n = escapeHtml(clip(c.newText || "", 160));
    if (c.kind === "replaced") snip = `<del>${o}</del><span class="cmp-arrow">→</span><ins>${n}</ins>`;
    else if (c.kind === "inserted") snip = `<ins>${n}</ins>`;
    else if (c.kind === "deleted") snip = `<del>${o}</del>`;
    else if (c.kind === "pageInserted") snip = `${escapeHtml(t("cmp.pageInsertedDesc", { n: pageLabel(c.newPage) }))}${n ? ` <ins>${n}</ins>` : ""}`;
    else if (c.kind === "pageDeleted") snip = `${escapeHtml(t("cmp.pageDeletedDesc", { n: pageLabel(c.oldPage) }))}${o ? ` <del>${o}</del>` : ""}`;
    else snip = escapeHtml(t("cmp.graphicsDesc"));
    const oldPg = c.kind === "pageInserted" ? "—" : pageLabel(c.oldPage);
    const newPg = c.kind === "pageDeleted" ? "—" : pageLabel(c.newPage);
    return `<button class="cmp-item${c.id === cmp.sel ? " sel" : ""}" data-id="${c.id}">` +
      `<div class="cmp-item-top"><span class="cmp-badge k-${c.kind}">${escapeHtml(kindLabel(c.kind))}</span>` +
      `<span class="cmp-pg">${escapeHtml(t("cmp.pagesLabel", { o: oldPg, n: newPg }))}</span></div>` +
      `<div class="cmp-snip">${snip}</div></button>`;
  }

  function buildList() {
    const list = visibleChanges();
    $("cmpList").innerHTML = list.length
      ? list.map(itemHtml).join("")
      : `<div class="cmp-empty">${escapeHtml(cmp.res.summary.total ? t("cmp.filteredEmpty") : t("cmp.noDiffLong"))}</div>`;
    updatePos();
  }

  function buildAll() {
    indexChanges();
    buildRows(false);
    buildSummary();
    buildList();
  }

  function updatePos() {
    const list = visibleChanges();
    const i = list.findIndex((c) => c.id === cmp.sel);
    $("cmpPos").textContent = `${i >= 0 ? i + 1 : 0}/${list.length}`;
    $("cmpPrev").disabled = i <= 0;
    $("cmpNext").disabled = list.length === 0 || i >= list.length - 1;
  }

  function selectChange(id, scroll) {
    cmp.sel = id;
    stage.querySelectorAll(".cmp-mk.sel, .cmp-item.sel").forEach((e) => e.classList.remove("sel"));
    const marks = stage.querySelectorAll(`.cmp-mk[data-id="${id}"]`);
    marks.forEach((m) => m.classList.add("sel"));
    const item = $("cmpList").querySelector(`.cmp-item[data-id="${id}"]`);
    if (item) { item.classList.add("sel"); item.scrollIntoView({ block: "nearest" }); }
    updatePos();
    if (!scroll) return;
    const c = cmp.res.changes[id];
    // Ưu tiên dấu bên Mới (trang chèn: cả trang; trang xoá: bên Cũ).
    const pick = (side) => stage.querySelector(`.cmp-page[data-side="${side}"] .cmp-mk[data-id="${id}"]`);
    const target = c.kind === "pageDeleted" ? pick("o") : pick("n") || pick("o");
    if (target) {
      const full = target.classList.contains("full");
      target.scrollIntoView({ block: full ? "start" : "center", inline: "nearest", behavior: "smooth" });
    } else {
      const rowIdx = cmp.res.pageMap.findIndex((p) => (c.newPage != null && p.new === c.newPage) || (c.oldPage != null && p.old === c.oldPage));
      const row = $("cmpRows").querySelector(`.cmp-row[data-row="${rowIdx}"]`);
      if (row) row.scrollIntoView({ block: "start", behavior: "smooth" });
    }
  }

  function step(delta) {
    const list = visibleChanges();
    if (!list.length) return;
    let i = list.findIndex((c) => c.id === cmp.sel);
    i = i < 0 ? 0 : Math.max(0, Math.min(list.length - 1, i + delta));
    selectChange(list[i].id, true);
  }

  function setZoom(z) {
    cmp.zoom = Math.max(0.5, Math.min(4, Math.round(z * 100) / 100));
    buildRows(true);
    if (cmp.sel >= 0) selectChange(cmp.sel, false);
  }

  // ---------------- Xuất báo cáo ----------------

  async function exportReport() {
    if (!cmp.res) return;
    const base = shortName(cmp.newPath).replace(/\.pdf$/i, "");
    const out = await invoke("pick_save_as", { ext: "pdf", name: `${base}-${t("cmp.reportSuffix")}.pdf` });
    if (!out) return;
    const L = (k) => t("cmp.rep." + k);
    const labels = {
      title: L("title"), oldFile: L("oldFile"), newFile: L("newFile"),
      dateLine: t("cmp.rep.dateLine", { d: new Date().toLocaleString(I18N.lang === "vi" ? "vi-VN" : "en-US") }),
      summary: L("summary"), total: L("total"), details: L("details"),
      colNo: L("colNo"), colType: L("colType"), colOldPage: L("colOldPage"), colNewPage: L("colNewPage"), colContent: L("colContent"),
      inserted: kindLabel("inserted"), deleted: kindLabel("deleted"), replaced: kindLabel("replaced"),
      pageInserted: kindLabel("pageInserted"), pageDeleted: kindLabel("pageDeleted"), graphics: kindLabel("graphics"),
      more: L("more"), noDiff: L("noDiff"), author: L("author"),
    };
    $("cmpExport").disabled = true;
    $("status").textContent = t("cmp.exporting");
    try {
      const st = await invoke("compare_export_report", { output: out, oldName: shortName(cmp.oldPath), newName: shortName(cmp.newPath), labels });
      $("status").textContent = t("cmp.exported", { name: shortName(out), n: st.annotations });
      const open = await confirmModal(t("cmp.exportedTitle"), t("cmp.exportedAsk", { name: shortName(out) }), t("cmp.openReport"));
      if (open) {
        if (window.Shell && Shell.confirmDiscardChanges && !Shell.confirmDiscardChanges()) return;
        closeStage();
        await loadDocument(out);
      }
    } catch (e) {
      $("status").textContent = t("cmp.errExport", { e });
    } finally {
      $("cmpExport").disabled = false;
    }
  }

  // ---------------- Sự kiện ----------------

  $("cmpOpen").addEventListener("click", () => openDialog(null));
  $("cmpClose").addEventListener("click", closeStage);
  $("cmpRerun").addEventListener("click", () => openDialog({ oldPath: cmp.oldPath, newPath: cmp.newPath }));
  $("cmpExport").addEventListener("click", exportReport);
  $("cmpPrev").addEventListener("click", () => step(-1));
  $("cmpNext").addEventListener("click", () => step(1));
  $("cmpZoomIn").addEventListener("click", () => setZoom(cmp.zoom * 1.25));
  $("cmpZoomOut").addEventListener("click", () => setZoom(cmp.zoom / 1.25));
  $("cmpBusyCancel").addEventListener("click", () => {
    $("cmpBusyText").textContent = t("cmp.cancelling");
    invoke("compare_cancel").catch(() => {});
  });

  $("cmpRows").addEventListener("click", (e) => {
    const m = e.target.closest(".cmp-mk");
    if (m) selectChange(Number(m.dataset.id), false);
  });
  $("cmpList").addEventListener("click", (e) => {
    const it = e.target.closest(".cmp-item");
    if (it) selectChange(Number(it.dataset.id), true);
  });
  $("cmpList").addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") { e.preventDefault(); step(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); step(-1); }
  });
  $("cmpChips").addEventListener("click", (e) => {
    const ch = e.target.closest(".cmp-chip");
    if (!ch) return;
    const k = ch.dataset.kind;
    if (cmp.filter.has(k)) cmp.filter.delete(k); else cmp.filter.add(k);
    ch.classList.toggle("on", cmp.filter.has(k));
    stage.querySelectorAll(`.cmp-mk.k-${k}`).forEach((m) => m.classList.toggle("off", !cmp.filter.has(k)));
    buildList();
  });
  // Ctrl + lăn chuột = zoom (như viewer).
  $("cmpScroll").addEventListener("wheel", (e) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    setZoom(cmp.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
  }, { passive: false });

  window.addEventListener("keydown", (e) => {
    if (stage.classList.contains("hidden") || !cmp.res) return;
    if (!$("modalOverlay").classList.contains("hidden")) return;
    if (e.key === "F3") { e.preventDefault(); step(e.shiftKey ? -1 : 1); }
  });

  // Đổi bề rộng cửa sổ → dựng lại khung trang cho vừa 2 cột.
  let resizeTimer = null;
  let lastW = 0;
  new ResizeObserver(() => {
    if (stage.classList.contains("hidden") || !cmp.res) return;
    const w = $("cmpScroll").clientWidth;
    if (Math.abs(w - lastW) < 8) return;
    lastW = w;
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => { buildRows(true); if (cmp.sel >= 0) selectChange(cmp.sel, false); }, 150);
  }).observe($("cmpScroll"));

  // Chuyển tab ribbon / mở tệp khác → thoát màn hình so sánh về viewer.
  document.addEventListener("click", (e) => {
    if (stage.classList.contains("hidden")) return;
    const pill = e.target.closest && e.target.closest(".tab-pill");
    if (pill && pill.dataset.tab !== "tools" && !cmp.running) closeStage();
  }, true);
  document.addEventListener("docloaded", () => { if (!cmp.running) closeStage(); });
  document.addEventListener("langchange", () => {
    if (stage.classList.contains("hidden") || !cmp.res) return;
    buildRows(true);
    buildSummary();
    buildList();
  });
})();
