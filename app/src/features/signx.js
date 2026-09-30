// Điền & Ký (Fill & Sign), chữ ký số nâng cao (PFX, chữ ký hiển thị, ô có
// sẵn, bảng xác thực), In (Ctrl+P) và Batch / Action Wizard — theo Foxit.
// Dùng các hàm/biến toàn cục của main.js (invoke, $, state, t, openModal…).
(function () {
  const LS_SIGS = "ff.signx.sigs";      // thư viện chữ ký / chữ viết tắt (PNG data URL)
  const LS_IDS = "ff.signx.ids";        // Digital ID đã dùng gần đây [{path, cn}]
  const LS_SEQ = "ff.signx.sequences";  // chuỗi hành động đã lưu {name: {steps, suffix}}
  const LS_SIGOPT = "ff.signx.sigOpts"; // tuỳ chọn giao diện chữ ký lần trước
  const LS_DATEFMT = "ff.signx.dateFmt";

  const lsGet = (k, d) => { try { const v = localStorage.getItem(k); return v ? JSON.parse(v) : d; } catch (_) { return d; } };
  const lsSet = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch (_) {} };
  const modalOpen = () => !$("modalOverlay").classList.contains("hidden");
  const esc = (s) => escapeHtml(String(s == null ? "" : s));
  const scaleOf = () => PT_PER_PX * state.zoom;
  const typingIn = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  const status = (msg) => { $("status").textContent = msg; };

  // Hộp thoại rộng (Batch / tạo chữ ký): gỡ lớp khi đóng.
  function wideModal(cls) { $("modalBox").classList.add(cls); }
  new MutationObserver(() => {
    if ($("modalOverlay").classList.contains("hidden")) $("modalBox").classList.remove("sx-wide", "sx-mid");
  }).observe($("modalOverlay"), { attributes: true, attributeFilter: ["class"] });

  function needDoc() {
    if (state.path) return true;
    status(t("sx.noDoc"));
    return false;
  }

  // ====================================================================
  // Popover menu nhỏ (thư viện chữ ký, định dạng ngày)
  // ====================================================================
  let popEl = null;
  function closePop() { if (popEl) { popEl.remove(); popEl = null; } }
  function openPop(anchor, html, onClick) {
    const again = popEl && popEl.dataset.anchor === anchor.id;
    closePop();
    if (again) return null;
    popEl = document.createElement("div");
    popEl.className = "menu sx-pop";
    popEl.dataset.anchor = anchor.id;
    popEl.innerHTML = html;
    document.body.appendChild(popEl);
    applyIcons(popEl);
    const r = anchor.getBoundingClientRect();
    popEl.style.top = r.bottom + 4 + "px";
    popEl.style.left = Math.min(r.left, window.innerWidth - popEl.offsetWidth - 8) + "px";
    popEl.addEventListener("click", (e) => { const it = e.target.closest("[data-v]"); if (it) onClick(it.dataset.v, e, it); });
    return popEl;
  }
  document.addEventListener("mousedown", (e) => {
    if (popEl && !popEl.contains(e.target) && !e.target.closest("#" + popEl.dataset.anchor)) closePop();
  });

  // ====================================================================
  // A. ĐIỀN & KÝ
  // ====================================================================
  const fs = {
    items: [],   // {id, page, kind:'text'|'mark'|'image', x, top, w, h, ...} — điểm PDF, gốc trên-trái
    tool: null,  // 'text'|'check'|'cross'|'dot'|'line'|'date'|'image'
    pendingImg: null, // mục thư viện chờ đặt
    dateFmt: lsGet(LS_DATEFMT, "dd/MM/yyyy"),
    sel: null,
    seq: 1,
    undo: [],
  };
  const MARK_SIZE = { check: [14, 14], cross: [14, 14], dot: [6, 6], line: [110, 6] };
  const FS_TOOLS = ["text", "check", "cross", "dot", "line", "date"];

  function fsSnapshot() {
    fs.undo.push(JSON.stringify(fs.items));
    if (fs.undo.length > 50) fs.undo.shift();
  }
  function fsUndo() {
    if (!fs.undo.length) return false;
    fs.items = JSON.parse(fs.undo.pop());
    fs.sel = null;
    fsRenderAll();
    return true;
  }

  function fmtDate(d, f) {
    const p2 = (n) => String(n).padStart(2, "0");
    const loc = I18N.lang === "vi" ? "vi-VN" : "en-US";
    if (f === "long") return d.toLocaleDateString(loc, { day: "numeric", month: "long", year: "numeric" });
    return f.replace("yyyy", d.getFullYear()).replace("MM", p2(d.getMonth() + 1)).replace("dd", p2(d.getDate()));
  }
  const DATE_FMTS = ["dd/MM/yyyy", "MM/dd/yyyy", "yyyy-MM-dd", "dd.MM.yyyy", "long"];

  function fsSetTool(tool, img) {
    closePop();
    if (tool && state.tool) setTool(null); // tắt công cụ chú thích đang bật
    if (sx.drawing) cancelSigDraw();
    fs.tool = tool;
    fs.pendingImg = img || null;
    document.querySelectorAll("#fsBar .fs-tool").forEach((b) => b.classList.toggle("active", !!tool && b.dataset.fs === tool));
    $("fsSig").classList.toggle("active", tool === "image" && img && img.kind === "sig");
    $("fsInit").classList.toggle("active", tool === "image" && img && img.kind === "init");
    document.body.classList.toggle("fs-placing", !!tool);
    $("fsHint").textContent = tool ? t(tool === "image" ? "sx.fsHintImage" : "sx.fsHintPlace") : (fs.items.length ? t("sx.fsHintApply") : "");
  }

  function fsLayer(page) {
    const slot = state.slots[page];
    if (!slot) return null;
    let layer = slot.querySelector(".fs-layer");
    if (!layer) {
      layer = document.createElement("div");
      layer.className = "fs-layer";
      slot.appendChild(layer);
      fsResize.observe(slot);
    }
    return layer;
  }
  const fsResize = new ResizeObserver((entries) => {
    for (const en of entries) {
      const idx = Number(en.target.dataset.index);
      if (fs.items.some((i) => i.page === idx) || sx.rect) fsRenderPage(idx);
    }
  });

  function markSvg(it) {
    const { w, h } = it;
    const sw = it.mark === "line" ? 1.2 : Math.max(0.8, Math.min(w, h) * 0.12);
    const c = rgbCss(it.color || [0, 0, 0]);
    let body = "";
    if (it.mark === "check") body = `<polyline points="${w * 0.08},${h * 0.48} ${w * 0.38},${h * 0.85} ${w * 0.92},${h * 0.12}" />`;
    else if (it.mark === "cross") {
      const i = sw / 2 + Math.min(w, h) * 0.08;
      body = `<path d="M${i} ${i}L${w - i} ${h - i}M${i} ${h - i}L${w - i} ${i}" />`;
    } else if (it.mark === "dot") body = `<ellipse cx="${w / 2}" cy="${h / 2}" rx="${Math.min(w, h) / 2}" ry="${Math.min(w, h) / 2}" fill="${c}" stroke="none"/>`;
    else body = `<path d="M0 ${h / 2}H${w}" />`;
    return `<svg viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" fill="none" stroke="${c}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
  }

  function fsRenderPage(page) {
    const layer = fsLayer(page);
    if (!layer) return;
    const s = scaleOf();
    // Giữ nguyên ô đang gõ (không dựng lại khi resize trong lúc sửa).
    const editing = layer.querySelector(".fs-text[contenteditable='true']");
    layer.querySelectorAll(".fs-item").forEach((el) => { if (el !== editing) el.remove(); });
    for (const it of fs.items.filter((i) => i.page === page)) {
      if (editing && editing.dataset.id === String(it.id)) {
        editing.style.left = it.x * s + "px"; editing.style.top = it.top * s + "px";
        editing.style.fontSize = it.fontSize * s + "px";
        continue;
      }
      const el = document.createElement("div");
      el.className = "fs-item fs-" + it.kind + (fs.sel === it.id ? " sel" : "");
      el.dataset.id = it.id;
      el.style.left = it.x * s + "px";
      el.style.top = it.top * s + "px";
      if (it.kind === "text") {
        el.style.fontSize = it.fontSize * s + "px";
        el.style.color = rgbCss(it.color || [0, 0, 0]);
        el.textContent = it.text;
      } else {
        el.style.width = it.w * s + "px";
        el.style.height = it.h * s + "px";
        if (it.kind === "image") el.innerHTML = `<img src="${it.dataUrl}" alt="" draggable="false">`;
        else el.innerHTML = markSvg(it);
      }
      if (fs.sel === it.id) {
        el.insertAdjacentHTML("beforeend",
          `<span class="fs-handle" data-h="1"></span><span class="fs-bar">` +
          (it.kind === "text" ? `<button data-a="smaller" title="${esc(t("sx.fsSmaller"))}">A−</button><button data-a="bigger" title="${esc(t("sx.fsBigger"))}">A+</button>` : "") +
          (it.kind !== "image" ? `<button data-a="color" title="${esc(t("sx.fsColor"))}"><span class="fs-sw" style="background:${rgbCss(it.color || [0, 0, 0])}"></span></button>` : "") +
          `<button data-a="del" title="${esc(t("sx.fsDelete"))}"><i data-icon="trash"></i></button></span>`);
      }
      layer.appendChild(el);
    }
    applyIcons(layer);
    if (sx.rect && sx.rect.page === page) drawSigRectPreview();
  }
  function fsRenderAll() {
    const pages = new Set(fs.items.map((i) => i.page));
    document.querySelectorAll(".page-slot .fs-layer").forEach((l) => pages.add(Number(l.parentElement.dataset.index)));
    for (const p of pages) fsRenderPage(p);
    fsUpdateButtons();
  }
  function fsUpdateButtons() {
    $("fsCount").textContent = fs.items.length;
    $("fsApply").disabled = !fs.items.length;
    $("fsClear").disabled = !fs.items.length;
    if (!fs.tool) $("fsHint").textContent = fs.items.length ? t("sx.fsHintApply") : "";
  }

  function fsAdd(it) {
    fsSnapshot();
    it.id = fs.seq++;
    fs.items.push(it);
    fs.sel = it.id;
    fsRenderPage(it.page);
    fsUpdateButtons();
    return it;
  }

  // Đặt phần tử tại điểm bấm (pt, gốc trên-trái của trang).
  function fsPlace(page, px, py) {
    const p = state.pages[page];
    const clampX = (x, w) => Math.max(0, Math.min(p.widthPt - w, x));
    const clampY = (y, h) => Math.max(0, Math.min(p.heightPt - h, y));
    const tool = fs.tool;
    if (tool === "text" || tool === "date") {
      const size = 12;
      const it = fsAdd({ page, kind: "text", x: clampX(px, 10), top: clampY(py - size * 0.6, size), fontSize: size, color: [0, 0, 0],
        text: tool === "date" ? fmtDate(new Date(), fs.dateFmt) : "" });
      if (tool === "text") setTimeout(() => fsEditText(it.id), 0);
    } else if (MARK_SIZE[tool]) {
      const [w, h] = MARK_SIZE[tool];
      fsAdd({ page, kind: "mark", mark: tool, x: clampX(px - (tool === "line" ? 0 : w / 2), w), top: clampY(py - h / 2, h), w, h, color: [0, 0, 0] });
    } else if (tool === "image" && fs.pendingImg) {
      const im = fs.pendingImg;
      const w = Math.min(im.kind === "init" ? 60 : 150, p.widthPt * 0.8);
      const h = w * (im.h / im.w);
      fsAdd({ page, kind: "image", sigKind: im.kind, dataUrl: im.dataUrl, x: clampX(px - w / 2, w), top: clampY(py - h / 2, h), w, h });
    }
    // Ngày/dấu: giữ công cụ để đặt tiếp (như Foxit); chữ ký/text: thôi công cụ.
    if (tool === "text" || tool === "image") fsSetTool(null);
  }

  function fsItemById(id) { return fs.items.find((i) => i.id === Number(id)); }

  function fsEditText(id) {
    const it = fsItemById(id);
    if (!it) return;
    const el = document.querySelector(`.fs-item[data-id="${it.id}"]`);
    if (!el) return;
    el.querySelectorAll(".fs-handle,.fs-bar").forEach((x) => x.remove());
    el.textContent = it.text;
    el.contentEditable = "true";
    el.classList.add("editing");
    el.focus();
    const r = document.createRange();
    r.selectNodeContents(el);
    r.collapse(false);
    const sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    const done = () => {
      el.removeEventListener("blur", done);
      el.contentEditable = "false";
      const txt = el.innerText.replace(/ /g, " ").replace(/\n+$/, "");
      if (!txt.trim()) {
        fs.items = fs.items.filter((i) => i !== it);
        if (fs.sel === it.id) fs.sel = null;
      } else if (txt !== it.text) {
        fsSnapshot();
        it.text = txt;
      }
      fsRenderPage(it.page);
      fsUpdateButtons();
    };
    el.addEventListener("blur", done);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); el.blur(); }
    });
  }

  // Kéo / đổi cỡ / thanh công cụ nhỏ của phần tử.
  let fsDrag = null;
  function onFsItemDown(e) {
    const el = e.target.closest(".fs-item");
    if (!el || e.button !== 0) return false;
    if (el.isContentEditable) { e.stopPropagation(); return true; }
    const it = fsItemById(el.dataset.id);
    if (!it) return false;
    e.stopPropagation();
    const btn = e.target.closest(".fs-bar button");
    if (btn) {
      e.preventDefault();
      fsItemAction(it, btn.dataset.a, btn);
      return true;
    }
    e.preventDefault();
    if (fs.sel !== it.id) { fs.sel = it.id; fsRenderAll(); }
    fsDrag = { it, resize: !!e.target.closest(".fs-handle"), x0: e.clientX, y0: e.clientY,
      orig: { x: it.x, top: it.top, w: it.w, h: it.h, fontSize: it.fontSize }, moved: false };
    return true;
  }
  function fsItemAction(it, a, btn) {
    if (a === "del") { fsSnapshot(); fs.items = fs.items.filter((i) => i !== it); fs.sel = null; fsRenderPage(it.page); fsUpdateButtons(); }
    else if (a === "bigger" || a === "smaller") {
      fsSnapshot();
      it.fontSize = Math.max(6, Math.min(72, it.fontSize + (a === "bigger" ? 1 : -1)));
      fsRenderPage(it.page);
    } else if (a === "color") {
      openColorPopover(btn, it.color || [0, 0, 0], (rgb) => { fsSnapshot(); it.color = rgb; fsRenderPage(it.page); });
    }
  }
  window.addEventListener("mousemove", (e) => {
    if (!fsDrag) return;
    const s = scaleOf();
    const dx = (e.clientX - fsDrag.x0) / s;
    const dy = (e.clientY - fsDrag.y0) / s;
    if (!fsDrag.moved && Math.abs(dx) + Math.abs(dy) < 1.5 / s) return;
    if (!fsDrag.moved) { fsSnapshot(); fsDrag.moved = true; }
    const { it, orig } = fsDrag;
    const p = state.pages[it.page];
    if (fsDrag.resize) {
      if (it.kind === "text") {
        it.fontSize = Math.max(6, Math.min(96, orig.fontSize + dy * 0.8));
      } else if (it.kind === "image" || it.mark === "check" || it.mark === "cross" || it.mark === "dot") {
        const k = Math.max(0.1, (orig.w + dx) / orig.w);
        it.w = Math.max(4, Math.min(p.widthPt - it.x, orig.w * k));
        it.h = it.w * (orig.h / orig.w);
      } else {
        it.w = Math.max(8, Math.min(p.widthPt - it.x, orig.w + dx));
      }
    } else {
      const w = it.w || 10, h = it.h || it.fontSize;
      it.x = Math.max(0, Math.min(p.widthPt - w, orig.x + dx));
      it.top = Math.max(0, Math.min(p.heightPt - h, orig.top + dy));
    }
    fsRenderPage(it.page);
  });
  window.addEventListener("mouseup", () => { fsDrag = null; });

  $("pages").addEventListener("mousedown", (e) => {
    if (sx.drawing) { onSigDrawDown(e); return; }
    if (e.target.closest(".fs-item")) { onFsItemDown(e); return; }
    if (fs.sel != null && !fs.tool) { fs.sel = null; fsRenderAll(); }
    if (!fs.tool || e.button !== 0) return;
    const slot = e.target.closest(".page-slot");
    if (!slot) return;
    e.preventDefault();
    e.stopPropagation();
    const r = slot.getBoundingClientRect();
    const s = scaleOf();
    fsPlace(Number(slot.dataset.index), (e.clientX - r.left) / s, (e.clientY - r.top) / s);
  }, true);
  $("pages").addEventListener("dblclick", (e) => {
    const el = e.target.closest(".fs-item.fs-text");
    if (!el) return;
    e.stopPropagation();
    e.preventDefault();
    fsEditText(el.dataset.id);
  }, true);

  // Phím: Esc thôi công cụ/bỏ chọn, Delete xoá, Ctrl+Z hoàn tác — trước main.js.
  window.addEventListener("keydown", (e) => {
    if (modalOpen()) return;
    const typing = typingIn(e.target);
    if (e.key === "Escape") {
      if (sx.drawing) { cancelSigDraw(); e.stopImmediatePropagation(); return; }
      if (fs.tool) { fsSetTool(null); e.stopImmediatePropagation(); return; }
      if (fs.sel != null && !typing) { fs.sel = null; fsRenderAll(); e.stopImmediatePropagation(); }
      return;
    }
    if (typing) return;
    if ((e.key === "Delete" || e.key === "Backspace") && fs.sel != null) {
      const it = fsItemById(fs.sel);
      if (it) { e.preventDefault(); e.stopImmediatePropagation(); fsItemAction(it, "del"); }
      return;
    }
    if (e.ctrlKey && !e.shiftKey && (e.key === "z" || e.key === "Z") && Shell.currentTab === "fillsign" && fs.undo.length) {
      e.preventDefault(); e.stopImmediatePropagation(); fsUndo();
    }
  }, true);

  async function fsApply() {
    if (!fs.items.length || !needDoc()) return;
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    const items = fs.items.map((it) => {
      const p = state.pages[it.page];
      if (it.kind === "text") {
        return { kind: "text", page: it.page, x: it.x, y: p.heightPt - it.top - it.fontSize * 0.93, text: it.text,
          fontSize: it.fontSize, color: [...(it.color || [0, 0, 0]), 255], lineHeight: it.fontSize * 1.2 };
      }
      const y = p.heightPt - it.top - it.h;
      if (it.kind === "image") return { kind: "image", page: it.page, x: it.x, y, width: it.w, height: it.h, imageDataUrl: it.dataUrl };
      return { kind: "mark", page: it.page, mark: it.mark, x: it.x, y, width: it.w, height: it.h,
        color: [...(it.color || [0, 0, 0]), 255], stroke: it.mark === "line" ? 1.2 : null };
    });
    status(t("sx.fsApplying"));
    try {
      const n = await invoke("fillsign_apply", { input: state.path, output: out, items, password: null });
      fs.items = []; fs.sel = null; fs.undo = [];
      await loadDocument(out);
      status(t("sx.fsDone", { n, file: shortName(out) }));
    } catch (e) {
      status(t("sx.fsErr", { e }));
    }
  }

  // Tài liệu mới → bỏ các phần tử chưa áp dụng (thuộc trang của tệp cũ).
  document.addEventListener("docloaded", () => {
    fs.items = []; fs.sel = null; fs.undo = [];
    fsSetTool(null);
    sx.rect = null;
    fsUpdateButtons();
    backgroundVerify();
  });

  // ---------- Thư viện chữ ký ----------
  const sigLib = () => lsGet(LS_SIGS, []);
  function sigMenu(kind, anchor) {
    const lib = sigLib().filter((s) => s.kind === kind);
    const items = lib.map((s) => `<div class="mitem sx-sigitem" data-v="use:${s.id}"><img src="${s.dataUrl}" alt=""><button class="sx-sigdel" data-v="del:${s.id}" title="${esc(t("sx.sigDelete"))}"><i data-icon="win-close"></i></button></div>`).join("");
    const create = kind === "sig" ? t("sx.sigCreate") : t("sx.initCreate");
    openPop(anchor, `${items || `<div class="sx-empty muted">${esc(t("sx.sigEmpty"))}</div>`}<div class="msep"></div><button class="mitem" data-v="new"><i data-icon="plus"></i><span>${esc(create)}</span></button>`,
      (v, e) => {
        if (v === "new") { closePop(); openCreateSig(kind, (entry) => fsSetTool("image", entry)); return; }
        const [op, id] = v.split(":");
        if (op === "del") {
          e.stopPropagation();
          lsSet(LS_SIGS, sigLib().filter((s) => String(s.id) !== id));
          closePop(); sigMenu(kind, anchor);
          return;
        }
        const entry = sigLib().find((s) => String(s.id) === id);
        if (entry) fsSetTool("image", entry);
      });
  }

  // Font viết tay có trên máy (Windows) — dò bằng đo bề rộng so với fallback.
  const HAND_FONTS = ["Segoe Script", "Lucida Handwriting", "Ink Free", "Brush Script MT", "Mistral", "Freestyle Script", "Gabriola", "Segoe Print", "Comic Sans MS"];
  let availFonts = null;
  function handFonts() {
    if (availFonts) return availFonts;
    const c = document.createElement("canvas").getContext("2d");
    const sample = "Signature Aa Wy 123";
    const w = (f) => { c.font = `32px ${f}`; return c.measureText(sample).width; };
    const base = [w("monospace"), w("serif"), w("sans-serif")];
    availFonts = HAND_FONTS.filter((f) => {
      const a = [w(`"${f}", monospace`), w(`"${f}", serif`), w(`"${f}", sans-serif`)];
      return a.some((v, i) => Math.abs(v - base[i]) > 0.5);
    });
    availFonts.push("cursive");
    return availFonts;
  }

  // Cắt sát nét mực (alpha > 8) + đệm, trả {dataUrl, w, h}.
  function trimCanvas(cv) {
    const ctx = cv.getContext("2d");
    const { width: W, height: H } = cv;
    const d = ctx.getImageData(0, 0, W, H).data;
    let x0 = W, y0 = H, x1 = -1, y1 = -1;
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
      if (d[(y * W + x) * 4 + 3] > 8) { if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y; }
    }
    if (x1 < 0) return null;
    const pad = 6;
    x0 = Math.max(0, x0 - pad); y0 = Math.max(0, y0 - pad); x1 = Math.min(W - 1, x1 + pad); y1 = Math.min(H - 1, y1 + pad);
    const out = document.createElement("canvas");
    out.width = x1 - x0 + 1; out.height = y1 - y0 + 1;
    out.getContext("2d").drawImage(cv, x0, y0, out.width, out.height, 0, 0, out.width, out.height);
    return { dataUrl: out.toDataURL("image/png"), w: out.width, h: out.height };
  }

  function openCreateSig(kind, onDone) {
    const INK = [[0, 0, 0], [26, 63, 168], [190, 20, 30]];
    const fonts = handFonts();
    wideModal("sx-mid");
    const box = openModal(esc(kind === "sig" ? t("sx.sigCreate") : t("sx.initCreate")), `
      <div class="sx-tabs" role="tablist">
        <button class="sx-tab on" data-m="type">${esc(t("sx.sigType"))}</button>
        <button class="sx-tab" data-m="draw">${esc(t("sx.sigDraw"))}</button>
        <button class="sx-tab" data-m="image">${esc(t("sx.sigImage"))}</button>
      </div>
      <div class="sx-pane" data-m="type">
        <label>${esc(kind === "sig" ? t("sx.sigName") : t("sx.initName"))}</label>
        <input type="text" id="sxTypeText" maxlength="60">
        <label>${esc(t("sx.sigFont"))}</label>
        <select id="sxTypeFont">${fonts.map((f) => `<option value="${esc(f)}" style="font-family:'${esc(f)}'">${esc(f === "cursive" ? t("sx.sigFontDefault") : f)}</option>`).join("")}</select>
      </div>
      <div class="sx-pane hidden" data-m="draw">
        <div class="row sx-drawopts">
          <label class="sx-inline">${esc(t("sx.sigPen"))} <input type="range" id="sxPen" min="1" max="10" value="3"></label>
          <button id="sxDrawUndo"><i data-icon="undo"></i> ${esc(t("sx.sigUndoStroke"))}</button>
          <button id="sxDrawClear"><i data-icon="eraser"></i> ${esc(t("sx.sigClear"))}</button>
        </div>
      </div>
      <div class="sx-pane hidden" data-m="image">
        <div class="row">
          <button id="sxImgPick"><i data-icon="image-add"></i> ${esc(t("sx.sigImgPick"))}</button>
          <label class="sx-inline"><input type="checkbox" id="sxImgBg" checked> ${esc(t("sx.sigImgRemoveBg"))}</label>
          <label class="sx-inline">${esc(t("sx.sigImgThreshold"))} <input type="range" id="sxImgThr" min="150" max="254" value="220"></label>
        </div>
      </div>
      <div class="sx-inkrow">${esc(t("sx.sigColor"))}
        ${INK.map((c, i) => `<button class="sx-ink${i === 0 ? " on" : ""}" data-i="${i}" style="background:${rgbCss(c)}"></button>`).join("")}
      </div>
      <canvas id="sxSigCanvas" class="sx-sigcanvas"></canvas>
      <label class="sx-inline"><input type="checkbox" id="sxSigSave" checked> ${esc(t("sx.sigSaveLib"))}</label>
      <div class="err" id="sxSigErr"></div>
      <div class="foot"><button id="sxSigCancel">${esc(t("common.cancel"))}</button><button id="sxSigOk" class="primary">${esc(t("sx.sigOk"))}</button></div>
    `);
    const cv = box.querySelector("#sxSigCanvas");
    const DPR2 = 2;
    const CW = 560, CH = kind === "sig" ? 180 : 140;
    cv.width = CW * DPR2; cv.height = CH * DPR2;
    cv.style.width = CW + "px"; cv.style.height = CH + "px";
    const ctx = cv.getContext("2d");
    let mode = "type", ink = INK[0];
    const strokes = []; let cur = null;
    let srcImg = null;

    function redraw() {
      ctx.clearRect(0, 0, cv.width, cv.height);
      const col = rgbCss(ink);
      if (mode === "type") {
        const txt = box.querySelector("#sxTypeText").value.trim();
        if (!txt) return;
        const f = box.querySelector("#sxTypeFont").value;
        let size = CH * 0.55 * DPR2;
        ctx.font = `${size}px ${f === "cursive" ? "cursive" : `"${f}", cursive`}`;
        const w = ctx.measureText(txt).width;
        if (w > cv.width * 0.92) { size *= (cv.width * 0.92) / w; ctx.font = `${size}px ${f === "cursive" ? "cursive" : `"${f}", cursive`}`; }
        ctx.fillStyle = col; ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(txt, cv.width / 2, cv.height / 2);
      } else if (mode === "draw") {
        ctx.strokeStyle = col; ctx.lineCap = "round"; ctx.lineJoin = "round";
        for (const st of strokes) {
          ctx.lineWidth = st.w * DPR2;
          const pts = st.pts;
          ctx.beginPath();
          ctx.moveTo(pts[0][0], pts[0][1]);
          if (pts.length < 3) { ctx.lineTo(pts[pts.length - 1][0] + 0.1, pts[pts.length - 1][1]); }
          // Làm mượt: cong bậc 2 qua trung điểm các đoạn.
          for (let i = 1; i < pts.length - 1; i++) {
            const mx = (pts[i][0] + pts[i + 1][0]) / 2, my = (pts[i][1] + pts[i + 1][1]) / 2;
            ctx.quadraticCurveTo(pts[i][0], pts[i][1], mx, my);
          }
          if (pts.length >= 3) ctx.lineTo(pts[pts.length - 1][0], pts[pts.length - 1][1]);
          ctx.stroke();
        }
      } else if (mode === "image" && srcImg) {
        const s = Math.min(cv.width / srcImg.width, cv.height / srcImg.height, 1 * DPR2 * 2);
        const w = srcImg.width * s, h = srcImg.height * s;
        ctx.drawImage(srcImg, (cv.width - w) / 2, (cv.height - h) / 2, w, h);
        if (box.querySelector("#sxImgBg").checked) {
          const thr = Number(box.querySelector("#sxImgThr").value);
          const id = ctx.getImageData(0, 0, cv.width, cv.height);
          const d = id.data;
          for (let i = 0; i < d.length; i += 4) {
            const lum = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
            if (lum >= thr) d[i + 3] = 0;
            else if (lum > thr - 30) d[i + 3] = Math.round(d[i + 3] * (thr - lum) / 30); // mép mềm
          }
          ctx.putImageData(id, 0, 0);
        }
      }
    }
    box.querySelectorAll(".sx-tab").forEach((b) => b.addEventListener("click", () => {
      mode = b.dataset.m;
      box.querySelectorAll(".sx-tab").forEach((x) => x.classList.toggle("on", x === b));
      box.querySelectorAll(".sx-pane").forEach((p) => p.classList.toggle("hidden", p.dataset.m !== mode));
      cv.classList.toggle("drawable", mode === "draw");
      redraw();
    }));
    box.querySelectorAll(".sx-ink").forEach((b) => b.addEventListener("click", () => {
      ink = INK[Number(b.dataset.i)];
      box.querySelectorAll(".sx-ink").forEach((x) => x.classList.toggle("on", x === b));
      redraw();
    }));
    box.querySelector("#sxTypeText").addEventListener("input", redraw);
    box.querySelector("#sxTypeFont").addEventListener("change", redraw);
    box.querySelector("#sxImgBg").addEventListener("change", redraw);
    box.querySelector("#sxImgThr").addEventListener("input", redraw);
    box.querySelector("#sxDrawClear").addEventListener("click", () => { strokes.length = 0; redraw(); });
    box.querySelector("#sxDrawUndo").addEventListener("click", () => { strokes.pop(); redraw(); });
    const pos = (e) => { const r = cv.getBoundingClientRect(); return [(e.clientX - r.left) * DPR2, (e.clientY - r.top) * DPR2]; };
    cv.addEventListener("pointerdown", (e) => {
      if (mode !== "draw") return;
      cv.setPointerCapture(e.pointerId);
      cur = { w: Number(box.querySelector("#sxPen").value), pts: [pos(e)] };
      strokes.push(cur);
      redraw();
    });
    cv.addEventListener("pointermove", (e) => {
      if (!cur) return;
      const p = pos(e), last = cur.pts[cur.pts.length - 1];
      if (Math.hypot(p[0] - last[0], p[1] - last[1]) < 2) return;
      cur.pts.push(p);
      redraw();
    });
    cv.addEventListener("pointerup", () => { cur = null; });
    box.querySelector("#sxImgPick").addEventListener("click", async () => {
      const p = await invoke("pick_image");
      if (!p) return;
      try {
        const url = await invoke("signx_read_image", { path: p, maxSide: 1600 });
        const img = new Image();
        img.onload = () => { srcImg = img; redraw(); };
        img.src = url;
      } catch (e) { box.querySelector("#sxSigErr").textContent = t("sx.err", { e }); }
    });
    box.querySelector("#sxSigCancel").addEventListener("click", closeModal);
    box.querySelector("#sxSigOk").addEventListener("click", () => {
      redraw();
      const r = trimCanvas(cv);
      if (!r) { box.querySelector("#sxSigErr").textContent = t("sx.sigEmptyErr"); return; }
      const entry = { id: Date.now(), kind, ...r };
      if (box.querySelector("#sxSigSave").checked) {
        const lib = sigLib();
        lib.unshift(entry);
        lsSet(LS_SIGS, lib.slice(0, 20));
      }
      closeModal();
      onDone(entry);
    });
    setTimeout(() => box.querySelector("#sxTypeText").focus(), 0);
  }

  // Ribbon Điền & Ký.
  document.querySelectorAll("#fsBar .fs-tool").forEach((b) => b.addEventListener("click", () => {
    if (!needDoc()) return;
    const tool = b.dataset.fs;
    if (tool === "date") {
      const d = new Date();
      const pop = openPop(b, DATE_FMTS.map((f) => `<button class="mitem${f === fs.dateFmt ? " checked" : ""}" data-v="${f}"><span class="mcheck"></span><span>${esc(fmtDate(d, f))}</span></button>`).join(""),
        (f) => { fs.dateFmt = f; lsSet(LS_DATEFMT, f); fsSetTool("date"); });
      if (!pop) fsSetTool(null);
      return;
    }
    fsSetTool(fs.tool === tool ? null : tool);
  }));
  $("fsSig").addEventListener("click", () => { if (needDoc()) sigMenu("sig", $("fsSig")); });
  $("fsInit").addEventListener("click", () => { if (needDoc()) sigMenu("init", $("fsInit")); });
  $("fsApply").addEventListener("click", fsApply);
  $("fsClear").addEventListener("click", async () => {
    if (!fs.items.length) return;
    if (!(await confirmModal(t("sx.fsClearTitle"), t("sx.fsClearMsg", { n: fs.items.length }), t("sx.fsClear")))) return;
    fsSnapshot();
    fs.items = []; fs.sel = null;
    fsRenderAll();
  });

  // ====================================================================
  // B. CHỮ KÝ SỐ
  // ====================================================================
  const sx = { drawing: false, rect: null, drag: null };

  function startSigDraw() {
    fsSetTool(null);
    if (state.tool) setTool(null);
    sx.drawing = true;
    document.body.classList.add("sx-drawing");
    actionMsg("secHint", t("sx.sigDrawHint"));
  }
  function cancelSigDraw() {
    sx.drawing = false;
    sx.drag = null;
    document.body.classList.remove("sx-drawing");
    const prev = sx.rect ? sx.rect.page : null;
    sx.rect = null;
    if (prev != null) fsRenderPage(prev);
    $("secHint").textContent = "";
  }
  function onSigDrawDown(e) {
    const slot = e.target.closest(".page-slot");
    if (!slot || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const r = slot.getBoundingClientRect();
    const s = scaleOf();
    const page = Number(slot.dataset.index);
    const x = (e.clientX - r.left) / s, y = (e.clientY - r.top) / s;
    sx.drag = { page, x0: x, y0: y };
    sx.rect = { page, x, top: y, w: 0, h: 0 };
    drawSigRectPreview();
  }
  window.addEventListener("mousemove", (e) => {
    if (!sx.drawing || !sx.drag) return;
    const slot = state.slots[sx.drag.page];
    const r = slot.getBoundingClientRect();
    const s = scaleOf();
    const p = state.pages[sx.drag.page];
    const x = Math.max(0, Math.min(p.widthPt, (e.clientX - r.left) / s));
    const y = Math.max(0, Math.min(p.heightPt, (e.clientY - r.top) / s));
    sx.rect = { page: sx.drag.page, x: Math.min(x, sx.drag.x0), top: Math.min(y, sx.drag.y0), w: Math.abs(x - sx.drag.x0), h: Math.abs(y - sx.drag.y0) };
    drawSigRectPreview();
  });
  window.addEventListener("mouseup", () => {
    if (!sx.drawing || !sx.drag) return;
    sx.drag = null;
    const p = state.pages[sx.rect.page];
    if (sx.rect.w < 12 || sx.rect.h < 8) {
      // Bấm (không kéo) → khung mặc định 180×60 quanh điểm bấm.
      const w = 180, h = 60;
      sx.rect = { page: sx.rect.page, x: Math.max(0, Math.min(p.widthPt - w, sx.rect.x - w / 2)), top: Math.max(0, Math.min(p.heightPt - h, sx.rect.top - h / 2)), w, h };
      drawSigRectPreview();
    }
    sx.drawing = false;
    document.body.classList.remove("sx-drawing");
    $("secHint").textContent = "";
    openDigitalSignDialog({ rect: sx.rect });
  });
  function drawSigRectPreview() {
    if (!sx.rect) return;
    const layer = fsLayer(sx.rect.page);
    if (!layer) return;
    let el = layer.querySelector(".sx-sigrect");
    if (!el) { el = document.createElement("div"); el.className = "sx-sigrect"; layer.appendChild(el); }
    const s = scaleOf();
    Object.assign(el.style, { left: sx.rect.x * s + "px", top: sx.rect.top * s + "px", width: sx.rect.w * s + "px", height: sx.rect.h * s + "px" });
  }

  const recentIds = () => lsGet(LS_IDS, []);
  function rememberId(path, cn) {
    const list = recentIds().filter((x) => x.path !== path);
    list.unshift({ path, cn: cn || shortName(path) });
    lsSet(LS_IDS, list.slice(0, 8));
  }

  function tzOffsetMin() { return -new Date().getTimezoneOffset(); }
  function foxitDate(d) {
    const p2 = (n) => String(n).padStart(2, "0");
    const off = tzOffsetMin();
    const sign = off < 0 ? "-" : "+";
    const a = Math.abs(off);
    return `${d.getFullYear()}.${p2(d.getMonth() + 1)}.${p2(d.getDate())} ${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())} ${sign}${p2(Math.floor(a / 60))}'${p2(a % 60)}'`;
  }

  async function openSign() {
    if (!needDoc()) return;
    if (state.editMode) { status(t("sx.sigLeaveEdit")); return; }
    let fields = [];
    try { fields = await invoke("signx_list_fields", { input: state.path }); } catch (_) {}
    const empty = fields.filter((f) => !f.signed);
    if (empty.length) openDigitalSignDialog({ fields: empty });
    else startSigDraw();
  }

  function openDigitalSignDialog(ctx) {
    const opts = Object.assign({ showName: true, showDate: true, showReason: true, showLocation: true, showLabels: true, apMode: "text", sigId: null, reason: "", location: "", contact: "" }, lsGet(LS_SIGOPT, {}));
    const ids = recentIds();
    const lib = sigLib().filter((s) => s.kind === "sig");
    const fields = ctx.fields || [];
    const REASONS = ["sx.reason1", "sx.reason2", "sx.reason3", "sx.reason4"].map((k) => t(k));
    const box = openModal(esc(t("sx.signTitle")), `
      <label>${esc(t("sx.signAs"))}</label>
      <div class="row">
        <select id="sxId">${ids.map((x, i) => `<option value="${i}">${esc(x.cn)} — ${esc(shortName(x.path))}</option>`).join("")}<option value="">${esc(t("sx.idNone"))}</option></select>
        <button id="sxIdBrowse" class="sx-grow0">${esc(t("common.browse"))}</button>
        <button id="sxIdNew" class="sx-grow0">${esc(t("sx.idNew"))}</button>
      </div>
      <label>${esc(t("sx.idPassword"))}</label>
      <input type="password" id="sxIdPw" autocomplete="off">
      <div class="muted sx-idinfo" id="sxIdInfo"></div>
      ${fields.length ? `<label>${esc(t("sx.sigField"))}</label>
        <select id="sxField">${fields.map((f) => `<option value="${esc(f.name)}">${esc(f.name)}${f.pageIndex != null ? " — " + esc(t("sx.pageN", { n: f.pageIndex + 1 })) : ""}</option>`).join("")}<option value="__new">${esc(t("sx.sigFieldNew"))}</option></select>` : ""}
      <label>${esc(t("sx.apType"))}</label>
      <select id="sxAp">
        <option value="text">${esc(t("sx.apText"))}</option>
        <option value="image"${lib.length ? "" : " disabled"}>${esc(t("sx.apImage"))}</option>
        <option value="details">${esc(t("sx.apDetails"))}</option>
        <option value="invisible">${esc(t("sx.apInvisible"))}</option>
      </select>
      <div id="sxApImgRow" class="sx-aplib hidden">${lib.map((s) => `<button class="sx-aplibitem" data-id="${s.id}"><img src="${s.dataUrl}" alt=""></button>`).join("")}</div>
      <div class="sx-checks">
        <label><input type="checkbox" id="sxShowName"> ${esc(t("sx.showName"))}</label>
        <label><input type="checkbox" id="sxShowDate"> ${esc(t("sx.showDate"))}</label>
        <label><input type="checkbox" id="sxShowReason"> ${esc(t("sx.showReason"))}</label>
        <label><input type="checkbox" id="sxShowLocation"> ${esc(t("sx.showLocation"))}</label>
        <label><input type="checkbox" id="sxShowLabels"> ${esc(t("sx.showLabels"))}</label>
      </div>
      <canvas id="sxApPreview" class="sx-appreview"></canvas>
      <div class="row">
        <div><label>${esc(t("sx.reason"))}</label><input type="text" id="sxReason" list="sxReasons"><datalist id="sxReasons">${REASONS.map((r) => `<option value="${esc(r)}">`).join("")}</datalist></div>
        <div><label>${esc(t("sx.location"))}</label><input type="text" id="sxLocation"></div>
      </div>
      <label>${esc(t("sx.contact"))}</label>
      <input type="text" id="sxContact">
      <div class="err" id="sxSignErr"></div>
      <div class="foot"><button id="sxSignCancel">${esc(t("common.cancel"))}</button><button id="sxSignOk" class="primary">${esc(t("sx.signOk"))}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    let idPath = ids.length ? ids[0].path : null;
    let idInfo = null;
    let sigImg = lib.find((s) => s.id === opts.sigId) || lib[0] || null;
    q("#sxAp").value = opts.apMode === "image" && !lib.length ? "text" : opts.apMode;
    q("#sxShowName").checked = opts.showName; q("#sxShowDate").checked = opts.showDate;
    q("#sxShowReason").checked = opts.showReason; q("#sxShowLocation").checked = opts.showLocation;
    q("#sxShowLabels").checked = opts.showLabels;
    q("#sxReason").value = opts.reason || REASONS[0];
    q("#sxLocation").value = opts.location || "";
    q("#sxContact").value = opts.contact || "";
    if (!ids.length) q("#sxId").value = "";

    const curRect = () => {
      const fsel = q("#sxField");
      if (fsel && fsel.value !== "__new") {
        const f = fields.find((x) => x.name === fsel.value);
        if (f && f.rect) return { w: f.rect[2] - f.rect[0], h: f.rect[3] - f.rect[1] };
      }
      return ctx.rect ? { w: ctx.rect.w, h: ctx.rect.h } : { w: 180, h: 60 };
    };
    const signerName = () => (idInfo && idInfo.commonName) || (ids.find((x) => x.path === idPath) || {}).cn || "";
    function apLines() {
      const L = q("#sxShowLabels").checked;
      const name = signerName();
      const lines = [];
      if (q("#sxShowName").checked && name) lines.push(L ? t("sx.apSignedBy", { name }) : name);
      if (q("#sxShowDate").checked) { const d = foxitDate(new Date()); lines.push(L ? t("sx.apDate", { d }) : d); }
      const r = q("#sxReason").value.trim(), l = q("#sxLocation").value.trim();
      if (q("#sxShowReason").checked && r) lines.push(L ? t("sx.apReason", { r }) : r);
      if (q("#sxShowLocation").checked && l) lines.push(L ? t("sx.apLocation", { l }) : l);
      return lines;
    }
    function preview() {
      const mode = q("#sxAp").value;
      q("#sxApImgRow").classList.toggle("hidden", mode !== "image");
      box.querySelectorAll(".sx-aplibitem").forEach((b) => b.classList.toggle("on", sigImg && String(sigImg.id) === b.dataset.id));
      const cv = q("#sxApPreview");
      const { w, h } = curRect();
      const W = 360, H = Math.max(40, Math.min(200, W * (h / Math.max(1, w))));
      cv.width = W * 2; cv.height = H * 2; cv.style.width = W + "px"; cv.style.height = H + "px";
      cv.classList.toggle("hidden", mode === "invisible");
      const c = cv.getContext("2d");
      c.fillStyle = "#fff"; c.fillRect(0, 0, cv.width, cv.height);
      if (mode === "invisible") return;
      const k = cv.width / w; // px/pt
      const pad = Math.max(1, Math.min(6, h * 0.06)) * k;
      const lines = apLines();
      const hasLeft = mode === "image" ? !!sigImg : mode === "text";
      const splitX = hasLeft && lines.length ? cv.width * 0.5 : hasLeft ? cv.width : 0;
      c.fillStyle = "#000"; c.textBaseline = "alphabetic";
      if (mode === "image" && sigImg) {
        const im = new Image();
        im.onload = () => {
          const bw = splitX - 2 * pad, bh = cv.height - 2 * pad;
          const s = Math.min(bw / im.width, bh / im.height);
          c.drawImage(im, pad + (bw - im.width * s) / 2, pad + (bh - im.height * s) / 2, im.width * s, im.height * s);
        };
        im.src = sigImg.dataUrl;
      } else if (mode === "text") {
        const name = signerName() || t("sx.apNamePh");
        const size = Math.max(4 * k, Math.min((splitX - 2 * pad) / (name.length * 0.55), (cv.height - 2 * pad) * 0.6));
        c.font = `${size}px "Noto Sans", "Segoe UI", sans-serif`;
        c.fillText(name, pad, (cv.height + size * 0.72) / 2);
      }
      if (lines.length) {
        const rx = splitX ? splitX + pad : pad;
        const availW = cv.width - rx - pad;
        const maxc = Math.max(...lines.map((l) => l.length), 1);
        const size = Math.max(2 * k, Math.min(availW / (maxc * 0.52), (cv.height - 2 * pad) / (lines.length * 1.22), 14 * k));
        const lead = size * 1.22, top = (cv.height - lead * lines.length) / 2;
        c.font = `${size}px "Noto Sans", "Segoe UI", sans-serif`;
        lines.forEach((l, i) => c.fillText(l, rx, top + lead * (i + 1) - (lead - size) * 0.5 - size * 0.2));
      }
    }
    async function loadIdInfo() {
      idInfo = null;
      q("#sxIdInfo").textContent = "";
      q("#sxIdInfo").classList.remove("warn");
      if (!idPath) { preview(); return; }
      const isPem = /\.pem$/i.test(idPath);
      q("#sxIdPw").disabled = isPem;
      try {
        idInfo = await invoke("signx_id_info", { path: idPath, password: isPem ? null : q("#sxIdPw").value });
        q("#sxIdInfo").textContent = t("sx.idInfo", { cn: idInfo.commonName, issuer: idInfo.selfSigned ? t("sx.idSelfSigned") : idInfo.issuer, until: idInfo.notAfter });
        if (idInfo.expired) { q("#sxIdInfo").textContent += " — " + t("sx.idExpired"); q("#sxIdInfo").classList.add("warn"); }
      } catch (e) {
        if (!isPem && !q("#sxIdPw").value) q("#sxIdInfo").textContent = t("sx.idNeedPw");
        else { q("#sxIdInfo").textContent = t("sx.err", { e }); q("#sxIdInfo").classList.add("warn"); }
      }
      preview();
    }
    q("#sxId").addEventListener("change", () => {
      const i = q("#sxId").value;
      idPath = i === "" ? null : ids[Number(i)].path;
      q("#sxIdPw").value = "";
      loadIdInfo();
    });
    q("#sxIdBrowse").addEventListener("click", async () => {
      const p = await invoke("signx_pick_id");
      if (!p) return;
      idPath = p;
      const opt = document.createElement("option");
      opt.value = "x"; opt.textContent = shortName(p);
      q("#sxId").prepend(opt); q("#sxId").value = "x";
      ids.unshift({ path: p, cn: shortName(p) });
      opt.value = "0";
      // Đánh lại chỉ số các option cũ.
      [...q("#sxId").options].forEach((o, k) => { if (o.value !== "") o.value = String(k); });
      q("#sxIdPw").value = "";
      loadIdInfo();
    });
    q("#sxIdNew").addEventListener("click", () => {
      const keep = { ctx };
      openCreateIdDialog((path) => openDigitalSignDialog(Object.assign({}, keep.ctx, { justCreated: path })));
    });
    q("#sxIdPw").addEventListener("change", loadIdInfo);
    q("#sxIdPw").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); loadIdInfo(); } });
    for (const id of ["#sxAp", "#sxShowName", "#sxShowDate", "#sxShowReason", "#sxShowLocation", "#sxShowLabels", "#sxField"]) {
      const el = q(id);
      if (el) el.addEventListener("change", preview);
    }
    q("#sxReason").addEventListener("input", preview);
    q("#sxLocation").addEventListener("input", preview);
    box.querySelectorAll(".sx-aplibitem").forEach((b) => b.addEventListener("click", () => {
      sigImg = lib.find((s) => String(s.id) === b.dataset.id) || sigImg;
      preview();
    }));
    q("#sxSignCancel").addEventListener("click", () => { closeModal(); cancelSigDraw(); });
    if (ctx.justCreated) {
      const i = ids.findIndex((x) => x.path === ctx.justCreated);
      if (i >= 0) { q("#sxId").value = String(i); idPath = ctx.justCreated; }
    }
    q("#sxSignOk").addEventListener("click", async () => {
      const err = q("#sxSignErr");
      err.textContent = "";
      if (!idPath) { err.textContent = t("sx.errNoId"); return; }
      const isPem = /\.pem$/i.test(idPath);
      try {
        idInfo = await invoke("signx_id_info", { path: idPath, password: isPem ? null : q("#sxIdPw").value });
      } catch (e) { err.textContent = t("sx.err", { e }); return; }
      const mode = q("#sxAp").value;
      const fsel = q("#sxField");
      const fieldName = fsel && fsel.value !== "__new" ? fsel.value : null;
      if (!fieldName && mode !== "invisible" && !ctx.rect) {
        // Chọn "vùng mới" mà chưa vẽ → vẽ rồi quay lại.
        closeModal();
        startSigDraw();
        return;
      }
      let appearance = null;
      if (mode !== "invisible") {
        const r = ctx.rect;
        const p = r ? state.pages[r.page] : null;
        appearance = {
          page: r ? r.page : 0,
          rect: r ? [r.x, p.heightPt - r.top - r.h, r.x + r.w, p.heightPt - r.top] : [0, 0, 0, 0],
          imageDataUrl: mode === "image" && sigImg ? sigImg.dataUrl : null,
          bigText: mode === "text" ? signerName() : "",
          lines: apLines(),
        };
      }
      lsSet(LS_SIGOPT, {
        showName: q("#sxShowName").checked, showDate: q("#sxShowDate").checked, showReason: q("#sxShowReason").checked,
        showLocation: q("#sxShowLocation").checked, showLabels: q("#sxShowLabels").checked, apMode: mode,
        sigId: sigImg ? sigImg.id : null, reason: q("#sxReason").value, location: q("#sxLocation").value, contact: q("#sxContact").value,
      });
      const out = await invoke("pick_save_pdf");
      if (!out) return;
      const req = {
        idPath, password: isPem ? null : q("#sxIdPw").value,
        reason: q("#sxReason").value.trim(), location: q("#sxLocation").value.trim(), contactInfo: q("#sxContact").value.trim(),
        signerName: "", tzOffsetMin: tzOffsetMin(), fieldName, appearance,
      };
      q("#sxSignOk").disabled = true;
      err.textContent = t("sx.signing");
      try {
        await invoke("signx_sign", { input: state.path, output: out, req });
        rememberId(idPath, idInfo.commonName);
        closeModal();
        sx.rect = null;
        await loadDocument(out);
        status(t("sx.signDone", { file: shortName(out) }));
        verifyPanel(out);
      } catch (e) {
        q("#sxSignOk").disabled = false;
        err.textContent = t("sx.errSign", { e });
      }
    });
    loadIdInfo();
  }

  function openCreateIdDialog(after) {
    const box = openModal(esc(t("sx.idTitle")), `
      <p class="muted">${esc(t("sx.idIntro"))}</p>
      <label>${esc(t("sx.idName"))}</label><input type="text" id="sxCn">
      <div class="row">
        <div><label>${esc(t("sx.idOrg"))}</label><input type="text" id="sxOrg"></div>
        <div><label>${esc(t("sx.idEmail"))}</label><input type="text" id="sxEmail"></div>
      </div>
      <label>${esc(t("sx.idFormat"))}</label>
      <div class="radiorow">
        <label><input type="radio" name="sxFmt" value="pfx" checked> ${esc(t("sx.idPfx"))}</label>
        <label><input type="radio" name="sxFmt" value="pem"> ${esc(t("sx.idPem"))}</label>
      </div>
      <div id="sxPwRow" class="row">
        <div><label>${esc(t("sx.idPassword"))}</label><input type="password" id="sxPw1" autocomplete="new-password"></div>
        <div><label>${esc(t("sx.idPassword2"))}</label><input type="password" id="sxPw2" autocomplete="new-password"></div>
      </div>
      <div class="err" id="sxIdErr"></div>
      <div class="foot"><button id="sxIdCancel">${esc(t("common.cancel"))}</button><button id="sxIdOk" class="primary">${esc(t("sx.idCreate"))}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    box.querySelectorAll("input[name=sxFmt]").forEach((r) => r.addEventListener("change", () => {
      q("#sxPwRow").classList.toggle("hidden", box.querySelector("input[name=sxFmt]:checked").value !== "pfx");
    }));
    q("#sxIdCancel").addEventListener("click", closeModal);
    q("#sxIdOk").addEventListener("click", async () => {
      const cn = q("#sxCn").value.trim();
      const fmt = box.querySelector("input[name=sxFmt]:checked").value;
      const err = q("#sxIdErr");
      if (!cn) { err.textContent = t("sx.errIdName"); return; }
      if (fmt === "pfx") {
        if (q("#sxPw1").value.length < 6) { err.textContent = t("sx.errPwShort"); return; }
        if (q("#sxPw1").value !== q("#sxPw2").value) { err.textContent = t("sx.errPwMismatch"); return; }
      }
      const out = fmt === "pfx" ? await invoke("signx_pick_save_pfx", { name: cn }) : await invoke("pick_save_pem");
      if (!out) return;
      err.textContent = t("sx.idCreating");
      try {
        if (fmt === "pfx") await invoke("signx_create_pfx", { commonName: cn, organization: q("#sxOrg").value.trim(), email: q("#sxEmail").value.trim(), password: q("#sxPw1").value, output: out });
        else await invoke("sig_create_id", { commonName: cn, output: out });
        rememberId(out, cn);
        closeModal();
        actionMsg("secHint", t("sx.idDone", { file: shortName(out) }));
        if (after) after(out);
      } catch (e) { err.textContent = t("sx.err", { e }); }
    });
  }

  // Bảng xác thực chữ ký (Signature Panel / Validation Status).
  function sigStatus(c) {
    if (!c.intact) return { cls: "bad", icon: "win-close", text: t("sx.vInvalid") };
    if (c.coversDocument) return { cls: c.certSelfSigned ? "warn" : "ok", icon: "shield-check", text: t("sx.vValid") };
    if (c.laterChangesAreSignatures) return { cls: c.certSelfSigned ? "warn" : "ok", icon: "shield-check", text: t("sx.vValidLaterSigs") };
    return { cls: "warn", icon: "info", text: t("sx.vModified") };
  }
  async function verifyPanel(pathOverride) {
    const target = typeof pathOverride === "string" ? pathOverride : state.path;
    if (!target) { status(t("sx.noDoc")); return; }
    let checks;
    try { checks = await invoke("signx_verify", { input: target }); }
    catch (e) { actionMsg("secHint", t("sx.errVerify", { e })); return; }
    const rows = checks.map((c) => {
      const st = sigStatus(c);
      const kv = (k, v) => (v ? `<tr><td>${esc(t(k))}</td><td>${esc(v)}</td></tr>` : "");
      const notes = [];
      if (!c.cryptoValid) notes.push(t("sx.vNoteCrypto"));
      if (!c.digestMatches) notes.push(t("sx.vNoteDigest"));
      notes.push(c.certSelfSigned ? t("sx.vNoteSelfSigned") : t("sx.vNoteChain"));
      if (!c.certValidAtSigning) notes.push(t("sx.vNoteCertTime"));
      return `<details class="sx-sig ${st.cls}"${checks.length === 1 ? " open" : ""}>
        <summary><i data-icon="${st.icon}"></i><b>${esc(t("sx.vRev", { n: c.revision }))}</b> ${esc(t("sx.vBy", { name: c.signer }))}<span class="sx-sigtime">${esc(c.signTime)}</span></summary>
        <p class="sx-sigstat">${esc(st.text)}</p>
        <ul class="sx-notes">${notes.map((n) => `<li>${esc(n)}</li>`).join("")}</ul>
        <table class="kv">
          ${kv("sx.vSigner", c.name || c.signer)}${kv("sx.vTime", c.signTime)}${kv("sx.vReason", c.reason)}${kv("sx.vLocation", c.location)}${kv("sx.vContact", c.contactInfo)}
          ${kv("sx.vFilter", c.subFilter)}
          ${kv("sx.vSubject", c.certSubject)}${kv("sx.vIssuer", c.certIssuer)}${kv("sx.vSerial", c.certSerial)}
          ${kv("sx.vValidity", c.certNotBefore && `${c.certNotBefore} → ${c.certNotAfter}`)}
          ${kv("sx.vKey", c.certKeyBits ? `RSA ${c.certKeyBits} bit` : "")}${kv("sx.vChain", String(c.chainLen))}
        </table>
      </details>`;
    }).join("");
    const allOk = checks.length && checks.every((c) => c.intact && (c.coversDocument || c.laterChangesAreSignatures));
    const box = openModal(esc(t("sx.verifyTitle")), `
      <p class="sx-vsum ${checks.length ? (allOk ? "ok" : "bad") : ""}">${esc(checks.length ? (allOk ? t("sx.vAllValid", { n: checks.length }) : t("sx.vSomeInvalid")) : t("sx.vNone"))}</p>
      ${rows}
      <div class="foot"><button id="sxVClose" class="primary">${esc(t("common.close"))}</button></div>
    `);
    box.querySelector("#sxVClose").addEventListener("click", closeModal);
  }

  // Mở tài liệu có chữ ký → báo trạng thái ở thanh dưới (như thanh chữ ký Foxit).
  async function backgroundVerify() {
    const p = state.path;
    if (!p) return;
    try {
      const checks = await invoke("signx_verify", { input: p });
      if (!checks.length || state.path !== p) return;
      const ok = checks.every((c) => c.intact && (c.coversDocument || c.laterChangesAreSignatures));
      status(ok ? t("sx.barValid", { n: checks.length }) : t("sx.barInvalid", { n: checks.length }));
    } catch (_) {}
  }

  // ====================================================================
  // C. IN
  // ====================================================================
  let printing = false;
  function openPrint() {
    if (!needDoc() || printing) return;
    const n = state.pages.length;
    const cur = (state.editMode ? state.editPage : state.current) + 1;
    const box = openModal(esc(t("sx.printTitle")), `
      <p class="muted">${esc(t("sx.printIntro"))}</p>
      <label>${esc(t("sx.prPages"))}</label>
      <div class="radiorow">
        <label><input type="radio" name="sxPr" value="all" checked> ${esc(t("sx.prAll", { n }))}</label>
        <label><input type="radio" name="sxPr" value="cur"> ${esc(t("sx.prCurrent", { n: cur }))}</label>
        <label><input type="radio" name="sxPr" value="range"> ${esc(t("sx.prRange"))}</label>
        <input type="text" id="sxPrRange" class="sx-prrange" placeholder="1-3, 5">
      </div>
      <div class="row">
        <div><label>${esc(t("sx.prCopies"))}</label><input type="number" id="sxPrCopies" min="1" max="99" value="1"></div>
        <div><label>${esc(t("sx.prOrient"))}</label><select id="sxPrOrient"><option value="auto">${esc(t("sx.prAuto"))}</option><option value="portrait">${esc(t("sx.prPortrait"))}</option><option value="landscape">${esc(t("sx.prLandscape"))}</option></select></div>
      </div>
      <label class="sx-inline"><input type="checkbox" id="sxPrCollate" checked> ${esc(t("sx.prCollate"))}</label>
      <label>${esc(t("sx.prScale"))}</label>
      <div class="radiorow">
        <label><input type="radio" name="sxSc" value="fit" checked> ${esc(t("sx.prFit"))}</label>
        <label><input type="radio" name="sxSc" value="actual"> ${esc(t("sx.prActual"))}</label>
        <label><input type="radio" name="sxSc" value="custom"> ${esc(t("sx.prCustom"))}</label>
        <input type="number" id="sxPrPct" class="sx-prrange" min="10" max="400" value="100"> %
      </div>
      <label>${esc(t("sx.prContent"))}</label>
      <div class="sx-checks">
        <label><input type="checkbox" id="sxPrAnnots" checked> ${esc(t("sx.prAnnots"))}</label>
        <label><input type="checkbox" id="sxPrForms" checked> ${esc(t("sx.prForms"))}</label>
        <label><input type="checkbox" id="sxPrGray"> ${esc(t("sx.prGray"))}</label>
        <label><input type="checkbox" id="sxPrReverse"> ${esc(t("sx.prReverse"))}</label>
      </div>
      <label>${esc(t("sx.prQuality"))}</label>
      <select id="sxPrDpi"><option value="150">150 dpi</option><option value="200">200 dpi</option><option value="300" selected>300 dpi</option><option value="600">600 dpi</option></select>
      <div class="sx-progress hidden" id="sxPrProg"><div class="sx-bar"><span id="sxPrBar"></span></div><span id="sxPrMsg" class="muted"></span></div>
      <div class="err" id="sxPrErr"></div>
      <div class="foot"><button id="sxPrCancel">${esc(t("common.cancel"))}</button><button id="sxPrOk" class="primary"><i data-icon="printer"></i> ${esc(t("sx.prPrint"))}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    q("#sxPrRange").addEventListener("focus", () => { box.querySelector("input[name=sxPr][value=range]").checked = true; });
    q("#sxPrPct").addEventListener("focus", () => { box.querySelector("input[name=sxSc][value=custom]").checked = true; });
    let cancelled = false;
    q("#sxPrCancel").addEventListener("click", () => { cancelled = true; closeModal(); });
    q("#sxPrOk").addEventListener("click", async () => {
      const which = box.querySelector("input[name=sxPr]:checked").value;
      let pages = which === "all" ? state.pages.map((p) => p.index) : which === "cur" ? [cur - 1] : parsePageRange(q("#sxPrRange").value, n);
      if (!pages.length) { q("#sxPrErr").textContent = t("sx.prErrRange"); return; }
      if (q("#sxPrReverse").checked) pages = pages.slice().reverse();
      const o = {
        pages,
        copies: Math.max(1, Math.min(99, Number(q("#sxPrCopies").value) || 1)),
        collate: q("#sxPrCollate").checked,
        orient: q("#sxPrOrient").value,
        scale: box.querySelector("input[name=sxSc]:checked").value,
        pct: Math.max(10, Math.min(400, Number(q("#sxPrPct").value) || 100)),
        annots: q("#sxPrAnnots").checked,
        forms: q("#sxPrForms").checked,
        gray: q("#sxPrGray").checked,
        dpi: Number(q("#sxPrDpi").value),
      };
      q("#sxPrOk").disabled = true;
      q("#sxPrProg").classList.remove("hidden");
      try {
        await runPrint(o, (i, tot) => {
          q("#sxPrBar").style.width = Math.round((i / tot) * 100) + "%";
          q("#sxPrMsg").textContent = t("sx.prRendering", { i, n: tot });
        }, () => cancelled);
      } catch (e) {
        if (!modalOpen()) status(t("sx.prErr", { e }));
        else { q("#sxPrErr").textContent = t("sx.prErr", { e }); q("#sxPrOk").disabled = false; }
      }
    });
  }

  async function runPrint(o, onProg, isCancelled) {
    printing = true;
    const root = $("printRoot");
    const urls = [];
    let temp = null;
    try {
      let src = state.path;
      // Chú thích chưa lưu cũng được in (như Foxit): ghi tạm ra file.
      if (o.annots && state.annotSpecs.length) {
        temp = await tempFilePath(`ff_print_${Date.now()}.pdf`);
        const specs = state.annotSpecs.map((s) => ({
          kind: s.kind, pageIndex: s.pageIndex, left: s.left, bottom: s.bottom, right: s.right, top: s.top,
          quads: s.quads || [], color: [s.color[0], s.color[1], s.color[2], 255], contents: s.contents || null,
          fontSize: s.fontSize || 14, bold: !!s.bold, italic: !!s.italic, underline: !!s.underline,
        }));
        await invoke("apply_annotations", { input: src, output: temp, specs });
        src = temp;
      }
      root.innerHTML = "";
      const rendered = [];
      // Render TUẦN TỰ từng trang (await) — tài liệu lớn không làm đơ UI.
      for (let i = 0; i < o.pages.length; i++) {
        if (isCancelled()) throw new Error(t("sx.prCancelled"));
        onProg(i, o.pages.length);
        const r = await invoke("print_render_page", { path: src, page: o.pages[i], dpi: o.dpi, annotations: o.annots, forms: o.forms, grayscale: o.gray });
        const blob = await (await fetch(r.dataUrl)).blob();
        const url = URL.createObjectURL(blob);
        urls.push(url);
        rendered.push({ url, wPt: (r.widthPx / o.dpi) * 72, hPt: (r.heightPx / o.dpi) * 72 });
      }
      onProg(o.pages.length, o.pages.length);
      // Bản sao: collate = 1,2,3,1,2,3; không = 1,1,2,2,3,3.
      const seq = [];
      if (o.collate) for (let c = 0; c < o.copies; c++) seq.push(...rendered);
      else for (const r of rendered) for (let c = 0; c < o.copies; c++) seq.push(r);
      // Mỗi hướng/cỡ trang → 1 @page có tên (hướng giấy tự động theo trang).
      const css = [];
      const names = new Map();
      for (const r of seq) {
        const land = o.orient === "auto" ? r.wPt > r.hPt : o.orient === "landscape";
        const key = o.scale === "fit" ? (land ? "L" : "P") : `${land ? "L" : "P"}`;
        if (!names.has(key)) {
          const nm = `sxp${names.size}`;
          names.set(key, nm);
          css.push(`@page ${nm} { size: ${land ? "landscape" : "portrait"}; margin: 0; }`);
        }
        r.pageName = names.get(key);
      }
      const style = document.createElement("style");
      style.textContent = css.join("\n");
      root.appendChild(style);
      for (const r of seq) {
        const d = document.createElement("div");
        d.className = "sx-prpage";
        d.style.page = r.pageName;
        const img = document.createElement("img");
        img.src = r.url;
        if (o.scale === "fit") img.className = "fit";
        else {
          const k = o.scale === "custom" ? o.pct / 100 : 1;
          img.style.width = r.wPt * k + "pt";
          img.style.height = r.hPt * k + "pt";
        }
        d.appendChild(img);
        root.appendChild(d);
      }
      await Promise.all([...root.querySelectorAll("img")].map((im) => (im.decode ? im.decode().catch(() => {}) : null)));
      if (isCancelled()) throw new Error(t("sx.prCancelled"));
      closeModal();
      document.body.classList.add("sx-printing");
      status(t("sx.prOpening"));
      window.print(); // WebView2: hộp thoại in của hệ thống (chọn máy in)
      status(t("sx.prSent", { n: o.pages.length }));
    } finally {
      document.body.classList.remove("sx-printing");
      root.innerHTML = "";
      for (const u of urls) URL.revokeObjectURL(u);
      printing = false;
    }
  }

  window.addEventListener("keydown", (e) => {
    if (e.ctrlKey && !e.altKey && !e.shiftKey && (e.key === "p" || e.key === "P")) {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!modalOpen()) openPrint();
    }
  }, true);

  // ====================================================================
  // D. BATCH / ACTION WIZARD
  // ====================================================================
  // Mô tả bước: thêm bước mới = thêm 1 mục ở đây + biến thể BatchStep (Rust).
  const STEP_DEFS = {
    ocr: { icon: "ocr", fields: [{ k: "lang", type: "select", opts: ["vie+eng", "eng", "vie"], def: "vie+eng" }] },
    watermark: { icon: "watermark", fields: [
      { k: "text", type: "text", def: "CONFIDENTIAL" }, { k: "fontSize", type: "number", def: 48 },
      { k: "opacity", type: "percent", def: 30 }, { k: "rotation", type: "number", def: 45 }] },
    headerFooter: { icon: "header-footer", fields: [
      { k: "topLeft", type: "text", def: "" }, { k: "topCenter", type: "text", def: "" }, { k: "topRight", type: "text", def: "" },
      { k: "bottomLeft", type: "text", def: "" }, { k: "bottomCenter", type: "text", def: "{page}/{total}" }, { k: "bottomRight", type: "text", def: "" },
      { k: "fontSize", type: "number", def: 10 }] },
    optimize: { icon: "compress", fields: [] },
    removeMetadata: { icon: "eraser", fields: [] },
    flattenForm: { icon: "flatten", fields: [] },
    rotate: { icon: "rotate-right", fields: [{ k: "degrees", type: "select", opts: ["90", "180", "270"], def: "90" }] },
    encrypt: { icon: "lock", terminal: true, fields: [{ k: "userPassword", type: "password", def: "" }, { k: "ownerPassword", type: "password", def: "" }] },
    convert: { icon: "file-export", terminal: true, fields: [{ k: "format", type: "select", opts: ["word", "text", "images"], def: "word" }, { k: "dpi", type: "number", def: 150 }] },
  };
  const bw = { files: [], steps: [], outDir: lsGet("ff.signx.outDir", ""), suffix: "_out", running: false, unlisten: null };

  function stepToEngine(s) {
    const o = { kind: s.kind };
    for (const f of STEP_DEFS[s.kind].fields) {
      let v = s.opts[f.k];
      if (f.type === "number") v = Number(v);
      if (f.type === "percent") v = Number(v) / 100;
      if (s.kind === "rotate" && f.k === "degrees") v = Number(v);
      o[f.k] = v;
    }
    if (s.kind === "headerFooter") o.date = fmtDate(new Date(), fs.dateFmt);
    return o;
  }
  function newStep(kind) {
    const opts = {};
    for (const f of STEP_DEFS[kind].fields) opts[f.k] = f.def;
    return { kind, opts };
  }
  // Bước kết thúc (Mã hoá / Chuyển đổi) luôn ở cuối, chỉ 1.
  function normalizeSteps() {
    const normal = bw.steps.filter((s) => !STEP_DEFS[s.kind].terminal);
    const term = bw.steps.filter((s) => STEP_DEFS[s.kind].terminal);
    bw.steps = normal.concat(term.slice(-1));
  }

  function openBatch() {
    wideModal("sx-wide");
    const seqs = lsGet(LS_SEQ, {});
    const box = openModal(esc(t("sx.bwTitle")), `
      <div class="sx-bw">
        <section class="sx-bwcol">
          <div class="sx-bwhead"><b>${esc(t("sx.bwFiles"))}</b> <span class="muted" id="sxBwFileCount"></span></div>
          <div class="sx-bwtools">
            <button id="sxBwAddFiles"><i data-icon="plus"></i> ${esc(t("sx.bwAddFiles"))}</button>
            <button id="sxBwAddFolder"><i data-icon="folder-add"></i> ${esc(t("sx.bwAddFolder"))}</button>
            <button id="sxBwAddOpen"${state.path ? "" : " disabled"}><i data-icon="file-pdf"></i> ${esc(t("sx.bwAddOpen"))}</button>
            <button id="sxBwClearFiles"><i data-icon="trash"></i></button>
          </div>
          <label class="sx-inline"><input type="checkbox" id="sxBwRecursive"> ${esc(t("sx.bwRecursive"))}</label>
          <ul class="sx-bwlist" id="sxBwFiles"></ul>
        </section>
        <section class="sx-bwcol">
          <div class="sx-bwhead"><b>${esc(t("sx.bwSteps"))}</b></div>
          <div class="sx-bwtools">
            <select id="sxBwStepKind">${Object.keys(STEP_DEFS).map((k) => `<option value="${k}">${esc(t("sx.step." + k))}</option>`).join("")}</select>
            <button id="sxBwAddStep"><i data-icon="plus"></i> ${esc(t("sx.bwAddStep"))}</button>
          </div>
          <div class="sx-bwsteps" id="sxBwSteps"></div>
          <div class="sx-bwseq">
            <select id="sxBwSeq"><option value="">${esc(t("sx.bwSeqPick"))}</option>${Object.keys(seqs).map((n) => `<option value="${esc(n)}">${esc(n)}</option>`).join("")}</select>
            <button id="sxBwSeqDel" title="${esc(t("sx.bwSeqDelete"))}"><i data-icon="trash"></i></button>
            <input type="text" id="sxBwSeqName" placeholder="${esc(t("sx.bwSeqNamePh"))}">
            <button id="sxBwSeqSave"><i data-icon="save"></i> ${esc(t("sx.bwSeqSave"))}</button>
          </div>
        </section>
      </div>
      <div class="row sx-bwout">
        <div class="sx-grow"><label>${esc(t("sx.bwOutDir"))}</label><div class="row"><input type="text" id="sxBwOut" readonly placeholder="${esc(t("sx.bwOutPh"))}"><button id="sxBwOutPick" class="sx-grow0">${esc(t("common.browse"))}</button></div></div>
        <div class="sx-suffix"><label>${esc(t("sx.bwSuffix"))}</label><input type="text" id="sxBwSuffix"></div>
      </div>
      <div class="sx-progress hidden" id="sxBwProg"><div class="sx-bar"><span id="sxBwBar"></span></div><span id="sxBwMsg" class="muted"></span></div>
      <div class="err" id="sxBwErr"></div>
      <div class="foot"><button id="sxBwClose">${esc(t("common.close"))}</button><button id="sxBwCancel" class="hidden"><i data-icon="stop"></i> ${esc(t("sx.bwCancel"))}</button><button id="sxBwRun" class="primary"><i data-icon="play"></i> ${esc(t("sx.bwRun"))}</button></div>
    `);
    const q = (s) => box.querySelector(s);
    q("#sxBwOut").value = bw.outDir;
    q("#sxBwSuffix").value = bw.suffix;
    bw.status = {};

    function renderFiles() {
      q("#sxBwFileCount").textContent = bw.files.length ? `(${bw.files.length})` : "";
      q("#sxBwFiles").innerHTML = bw.files.length
        ? bw.files.map((f, i) => {
          const st = bw.status[i] || {};
          const outs = (st.outputs || []).map((o) => `<a href="#" class="sx-out" data-p="${esc(o)}">${esc(shortName(o))}</a>`).join(" ");
          return `<li class="${st.cls || ""}" title="${esc(f)}"><span class="sx-st">${st.icon ? `<i data-icon="${st.icon}"></i>` : ""}</span><span class="sx-fn">${esc(shortName(f))}</span>` +
            `<span class="sx-msg">${esc(st.msg || "")}${outs ? " " + outs : ""}</span>` +
            (bw.running ? "" : `<button class="sx-x" data-i="${i}" title="${esc(t("sx.bwRemove"))}"><i data-icon="win-close"></i></button>`) + `</li>`;
        }).join("")
        : `<li class="muted sx-empty">${esc(t("sx.bwNoFiles"))}</li>`;
      applyIcons(q("#sxBwFiles"));
    }
    function addFiles(list) {
      for (const f of list) if (!bw.files.includes(f)) bw.files.push(f);
      bw.status = {};
      renderFiles();
    }
    function fieldHtml(s, si, f) {
      const v = s.opts[f.k];
      const lab = esc(t(`sx.f.${f.k}`));
      if (f.type === "select") {
        const labels = (o) => (s.kind === "convert" || s.kind === "ocr" ? t(`sx.opt.${o.replace(/\+/g, "_")}`) : o + "°");
        return `<label>${lab}<select data-si="${si}" data-k="${f.k}">${f.opts.map((o) => `<option value="${o}"${String(v) === o ? " selected" : ""}>${esc(labels(o))}</option>`).join("")}</select></label>`;
      }
      const type = f.type === "password" ? "password" : f.type === "text" ? "text" : "number";
      return `<label>${lab}<input type="${type}" data-si="${si}" data-k="${f.k}" value="${esc(v)}"${type === "password" ? ' autocomplete="new-password"' : ""}></label>`;
    }
    function renderSteps() {
      q("#sxBwSteps").innerHTML = bw.steps.length
        ? bw.steps.map((s, si) => {
          const d = STEP_DEFS[s.kind];
          return `<div class="sx-step"><div class="sx-stephead"><i data-icon="${d.icon}"></i><b>${si + 1}. ${esc(t("sx.step." + s.kind))}</b>
            <span class="spacer"></span>
            <button data-mv="-1" data-si="${si}" title="${esc(t("sx.bwUp"))}"${si === 0 || d.terminal ? " disabled" : ""}><i data-icon="arrow-up"></i></button>
            <button data-mv="1" data-si="${si}" title="${esc(t("sx.bwDown"))}"${si === bw.steps.length - 1 || d.terminal || (bw.steps[si + 1] && STEP_DEFS[bw.steps[si + 1].kind].terminal) ? " disabled" : ""}><i data-icon="arrow-down"></i></button>
            <button data-rm="${si}" title="${esc(t("sx.bwRemove"))}"><i data-icon="trash"></i></button></div>
            ${d.fields.length ? `<div class="sx-stepbody">${d.fields.map((f) => fieldHtml(s, si, f)).join("")}</div>` : ""}
            ${s.kind === "headerFooter" ? `<div class="muted sx-tip">${esc(t("sx.bwHfTip"))}</div>` : ""}
          </div>`;
        }).join("")
        : `<div class="muted sx-empty">${esc(t("sx.bwNoSteps"))}</div>`;
      applyIcons(q("#sxBwSteps"));
    }
    q("#sxBwSteps").addEventListener("input", (e) => {
      const el = e.target;
      if (el.dataset.si == null) return;
      bw.steps[Number(el.dataset.si)].opts[el.dataset.k] = el.value;
    });
    q("#sxBwSteps").addEventListener("change", (e) => {
      const el = e.target;
      if (el.dataset.si == null) return;
      bw.steps[Number(el.dataset.si)].opts[el.dataset.k] = el.value;
    });
    q("#sxBwSteps").addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b || bw.running) return;
      if (b.dataset.rm != null) bw.steps.splice(Number(b.dataset.rm), 1);
      else if (b.dataset.mv != null) {
        const i = Number(b.dataset.si), j = i + Number(b.dataset.mv);
        [bw.steps[i], bw.steps[j]] = [bw.steps[j], bw.steps[i]];
      }
      normalizeSteps();
      renderSteps();
    });
    q("#sxBwAddStep").addEventListener("click", () => {
      const kind = q("#sxBwStepKind").value;
      const def = STEP_DEFS[kind];
      if (def.terminal) bw.steps = bw.steps.filter((s) => !STEP_DEFS[s.kind].terminal);
      bw.steps.push(newStep(kind));
      normalizeSteps();
      if (def.terminal) q("#sxBwErr").textContent = t("sx.bwTerminalNote");
      renderSteps();
    });
    q("#sxBwFiles").addEventListener("click", (e) => {
      const x = e.target.closest(".sx-x");
      if (x && !bw.running) { bw.files.splice(Number(x.dataset.i), 1); bw.status = {}; renderFiles(); return; }
      const a = e.target.closest(".sx-out");
      if (a) {
        e.preventDefault();
        if (/\.pdf$/i.test(a.dataset.p)) { closeModal(); loadDocument(a.dataset.p); }
      }
    });
    q("#sxBwAddFiles").addEventListener("click", async () => addFiles(await invoke("batch_pick_files")));
    q("#sxBwAddFolder").addEventListener("click", async () => {
      const d = await invoke("pick_dir");
      if (!d) return;
      const list = await invoke("batch_list_folder", { dir: d, recursive: q("#sxBwRecursive").checked });
      if (!list.length) q("#sxBwErr").textContent = t("sx.bwFolderEmpty");
      addFiles(list);
    });
    q("#sxBwAddOpen").addEventListener("click", () => { if (state.path) addFiles([state.path]); });
    q("#sxBwClearFiles").addEventListener("click", () => { if (!bw.running) { bw.files = []; bw.status = {}; renderFiles(); } });
    q("#sxBwOutPick").addEventListener("click", async () => {
      const d = await invoke("pick_dir");
      if (d) { bw.outDir = d; q("#sxBwOut").value = d; lsSet("ff.signx.outDir", d); }
    });
    q("#sxBwSuffix").addEventListener("input", () => { bw.suffix = q("#sxBwSuffix").value; });
    q("#sxBwSeq").addEventListener("change", () => {
      const s = lsGet(LS_SEQ, {})[q("#sxBwSeq").value];
      if (!s) return;
      bw.steps = JSON.parse(JSON.stringify(s.steps || [])).filter((x) => STEP_DEFS[x.kind]);
      if (s.suffix != null) { bw.suffix = s.suffix; q("#sxBwSuffix").value = s.suffix; }
      q("#sxBwSeqName").value = q("#sxBwSeq").value;
      normalizeSteps();
      renderSteps();
    });
    q("#sxBwSeqSave").addEventListener("click", () => {
      const name = q("#sxBwSeqName").value.trim() || q("#sxBwSeq").value;
      if (!name) { q("#sxBwErr").textContent = t("sx.bwSeqNameErr"); return; }
      if (!bw.steps.length) { q("#sxBwErr").textContent = t("sx.bwNoSteps"); return; }
      const all = lsGet(LS_SEQ, {});
      // Không lưu mật khẩu vào localStorage.
      const steps = bw.steps.map((s) => ({ kind: s.kind, opts: Object.fromEntries(Object.entries(s.opts).map(([k, v]) => [k, /password/i.test(k) ? "" : v])) }));
      all[name] = { steps, suffix: bw.suffix };
      lsSet(LS_SEQ, all);
      if (![...q("#sxBwSeq").options].some((o) => o.value === name)) q("#sxBwSeq").insertAdjacentHTML("beforeend", `<option value="${esc(name)}">${esc(name)}</option>`);
      q("#sxBwSeq").value = name;
      q("#sxBwErr").textContent = "";
      status(t("sx.bwSeqSaved", { name }));
    });
    q("#sxBwSeqDel").addEventListener("click", async () => {
      const name = q("#sxBwSeq").value;
      if (!name) return;
      const all = lsGet(LS_SEQ, {});
      delete all[name];
      lsSet(LS_SEQ, all);
      const opt = [...q("#sxBwSeq").options].find((o) => o.value === name);
      if (opt) opt.remove();
      q("#sxBwSeq").value = "";
    });
    q("#sxBwClose").addEventListener("click", closeModal);
    q("#sxBwCancel").addEventListener("click", () => { invoke("batch_cancel"); q("#sxBwMsg").textContent = t("sx.bwCancelling"); });
    q("#sxBwRun").addEventListener("click", async () => {
      const err = q("#sxBwErr");
      err.textContent = "";
      if (!bw.files.length) { err.textContent = t("sx.bwNoFiles"); return; }
      if (!bw.steps.length) { err.textContent = t("sx.bwNoSteps"); return; }
      if (!bw.outDir) { err.textContent = t("sx.bwNoOut"); return; }
      const steps = bw.steps.map(stepToEngine);
      const enc = steps.find((s) => s.kind === "encrypt");
      if (enc && !enc.userPassword && !enc.ownerPassword) { err.textContent = t("sx.bwNeedPw"); return; }
      const wm = steps.find((s) => s.kind === "watermark");
      if (wm && !String(wm.text).trim()) { err.textContent = t("sx.bwNeedWm"); return; }
      try { await invoke("batch_validate", { steps }); } catch (e) { err.textContent = t("sx.err", { e }); return; }
      bw.running = true;
      bw.status = {};
      q("#sxBwRun").disabled = true;
      q("#sxBwCancel").classList.remove("hidden");
      q("#sxBwProg").classList.remove("hidden");
      renderFiles();
      const total = bw.files.length;
      const onEv = (ev) => {
        const p = ev.payload || {};
        if (p.type === "fileStart") {
          bw.status[p.index] = { cls: "run", icon: "clock", msg: t("sx.bwRunning") };
          q("#sxBwBar").style.width = Math.round((p.index / total) * 100) + "%";
          q("#sxBwMsg").textContent = t("sx.bwProgress", { i: p.index + 1, n: total, name: shortName(p.file) });
        } else if (p.type === "step") {
          bw.status[p.index] = { cls: "run", icon: "clock", msg: t("sx.bwStepN", { i: p.step + 1, n: p.steps, step: t("sx.step." + p.kind) }) };
        } else if (p.type === "fileDone") {
          bw.status[p.index] = p.ok ? { cls: "ok", icon: "check-mark", msg: "", outputs: p.outputs } : { cls: "bad", icon: "win-close", msg: p.error || "" };
          q("#sxBwBar").style.width = Math.round(((p.index + 1) / total) * 100) + "%";
        } else if (p.type === "finished") {
          q("#sxBwMsg").textContent = t(p.cancelled ? "sx.bwDoneCancelled" : "sx.bwDone", { ok: p.ok, failed: p.failed });
          status(q("#sxBwMsg").textContent);
        }
        if (box.isConnected) renderFiles();
      };
      const un = await window.__TAURI__.event.listen("batch://progress", onEv);
      try {
        await invoke("batch_run", { files: bw.files, steps, outDir: bw.outDir, suffix: bw.suffix });
      } catch (e) {
        err.textContent = t("sx.err", { e });
      } finally {
        un();
        bw.running = false;
        if (box.isConnected) {
          q("#sxBwRun").disabled = false;
          q("#sxBwCancel").classList.add("hidden");
          renderFiles();
        }
      }
    });
    normalizeSteps();
    renderFiles();
    renderSteps();
  }

  // ---------- Gắn nút ----------
  $("sxBatch").addEventListener("click", openBatch);
  $("printItem").addEventListener("click", () => { Shell.closeMenus(); openPrint(); });

  window.SignX = {
    sign: openSign,
    verify: verifyPanel,
    createId: () => openCreateIdDialog(null),
    print: openPrint,
    batch: openBatch,
    get fillItems() { return fs.items.length; },
  };
  Shell.onLangChange(() => { fsUpdateButtons(); if (fs.tool) fsSetTool(fs.tool, fs.pendingImg); fsRenderAll(); });
  fsUpdateButtons();
})();
