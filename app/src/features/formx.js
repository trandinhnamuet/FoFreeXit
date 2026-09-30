// Form mức Foxit (features/formx.js):
// - ĐIỀN TRỰC TIẾP TRÊN TRANG: mỗi widget có 1 ô HTML phủ đúng khung (co giãn
//   theo zoom, dựng lười theo trang đang hiện), "Tô sáng trường", Tab/Shift+Tab
//   theo thứ tự tab, giá trị chờ trong `pending` → Lưu = formx_apply ra tệp mới.
// - TẠO TRƯỜNG: chọn công cụ trên ribbon rồi kéo khung trên trang → hộp Thuộc tính.
// - SỬA TRƯỜNG (chế độ chọn): chọn / kéo di chuyển / kéo góc đổi cỡ / xoá /
//   đúp chuột = Thuộc tính.
// - NHẬN DIỆN TRƯỜNG tự động (đề xuất để duyệt), Đặt lại / Xoá trắng,
//   Nhập / Xuất FDF · XFDF · XML · CSV · TXT.
// Hook vào main.js KHÔNG sửa hàm gốc: bọc loadDocument / drawAnnotsForPage và
// dùng listener capture-phase riêng trên #pages (chỉ chặn khi công cụ form đang bật).
(function () {
  "use strict";

  const FX = {
    path: null,
    widgets: [],          // WidgetDto (thứ tự tab)
    gen: 1,               // đổi → dựng lại lớp phủ
    pending: new Map(),   // tên field → { value, values }
    hl: true,             // tô sáng trường
    design: false,        // chế độ sửa trường
    arm: null,            // loại trường đang chờ vẽ
    sel: null,            // key đang chọn (id widget | "new:n")
    creates: [],          // [{ key, spec, proposed }]
    edits: new Map(),     // id widget → { rect?, props?, delete? }
    seq: 1,
    loadSeq: 0,
    els: new Map(),       // tên field → Set<wrap> (đồng bộ bản sao cùng tên)
  };
  window.FormX = FX;

  const KINDS = ["text", "checkbox", "radio", "combo", "list", "button", "date", "signature"];
  const NAME_BASE = { text: "Text", checkbox: "CheckBox", radio: "RadioGroup", combo: "ComboBox", list: "ListBox", button: "Button", date: "Date", signature: "Signature" };
  // Cỡ mặc định (pt) khi chỉ bấm, không kéo.
  const DEF_SIZE = { text: [150, 22], checkbox: [14, 14], radio: [14, 14], combo: [150, 22], list: [150, 70], button: [80, 24], date: [100, 22], signature: [160, 50] };
  const DATE_FORMATS = ["dd/mm/yyyy", "mm/dd/yyyy", "yyyy-mm-dd", "dd-mm-yyyy", "d/m/yyyy", "dd/mm/yy", "dd.mm.yyyy", "dd-mmm-yyyy"];
  const MARKS = { check: "✓", cross: "✕", circle: "●", square: "■", diamond: "◆", star: "★" };

  try { FX.hl = localStorage.getItem("ff.fxHighlight") !== "0"; } catch (_) {}
  document.body.classList.toggle("fx-hl", FX.hl);

  const sc = () => PT_PER_PX * state.zoom;
  const typingTarget = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  const modalOpen = () => !$("modalOverlay").classList.contains("hidden");
  const kindLabel = (k) => t("fx.kind." + (KINDS.includes(k) ? k : "text"));
  const rgbHex = (c) => "#" + (c || [0, 0, 0]).map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("");
  const hexRgb = (h) => { const n = parseInt(h.slice(1), 16); return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]; };
  const cssRgb = (c) => `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;

  // ---------- Nạp widget theo tài liệu ----------

  function clearAll() {
    FX.widgets = [];
    FX.pending.clear();
    FX.creates = [];
    FX.edits.clear();
    FX.sel = null;
    FX.els.clear();
    FX.gen++;
    document.querySelectorAll(".fx-layer").forEach((l) => l.remove());
    hideRecogBar();
    updateDirty();
  }

  async function loadWidgets() {
    const seq = ++FX.loadSeq;
    const path = state.path;
    if (!path) return;
    try {
      const list = await invoke("formx_widgets", { path });
      if (seq !== FX.loadSeq || path !== state.path) return;
      FX.widgets = list.filter((w) => !w.hidden);
    } catch (e) {
      if (seq !== FX.loadSeq) return;
      FX.widgets = [];
      console.error(e);
    }
    FX.gen++;
    rebuildAll();
    updateDirty();
  }

  function onDocChanged() {
    FX.path = state.path;
    clearAll();
    loadWidgets();
  }

  // Bọc hàm toàn cục của main.js (không sửa bản gốc).
  const origLoad = window.loadDocument;
  window.loadDocument = async function () {
    const r = await origLoad.apply(this, arguments);
    if (r === "ok") onDocChanged();
    return r;
  };
  const origDraw = window.drawAnnotsForPage;
  window.drawAnnotsForPage = function (idx) {
    origDraw.apply(this, arguments);
    // Tài liệu đổi qua đường khác loadDocument (tham chiếu giữ từ trước) → nạp lại.
    if (state.path && state.path !== FX.path) { onDocChanged(); return; }
    try { layoutPage(idx); } catch (e) { console.error(e); }
  };

  // ---------- Giá trị chờ lưu ----------

  function original(name) {
    return FX.widgets.find((w) => w.name === name);
  }
  function curVal(w) {
    const p = FX.pending.get(w.name);
    return p ? p.value : w.value;
  }
  function curVals(w) {
    const p = FX.pending.get(w.name);
    if (!p) return w.values || [];
    return p.values && p.values.length ? p.values : p.value ? [p.value] : [];
  }
  function setPending(name, value, values) {
    const o = original(name);
    if (!o) return;
    values = values || [];
    const same = o.kind === "list"
      ? JSON.stringify(values.length ? values : value ? [value] : []) === JSON.stringify(o.values || [])
      : (value || "") === (o.value || "") || ((o.kind === "checkbox" || o.kind === "radio") && isOff(value) && isOff(o.value));
    if (same) FX.pending.delete(name);
    else FX.pending.set(name, { value: value || "", values });
    syncName(name);
    updateDirty();
  }
  const isOff = (v) => !v || v === "Off";

  function updateDirty() {
    const n = FX.pending.size + FX.creates.length + FX.edits.size;
    const c = $("fxDirtyN");
    if (c) c.textContent = n;
    const b = $("fxSave");
    if (b) b.disabled = !n || !state.path;
  }
  FX.dirtyCount = () => FX.pending.size + FX.creates.length + FX.edits.size;

  // ---------- Lớp phủ theo trang ----------

  function layerOf(idx, create) {
    const slot = state.slots[idx];
    if (!slot) return null;
    let l = slot.querySelector(":scope > .fx-layer");
    if (!l && create) {
      l = document.createElement("div");
      l.className = "fx-layer";
      slot.appendChild(l);
    }
    return l;
  }

  function editedRect(w) {
    const e = FX.edits.get(w.id);
    return (e && e.rect) || w.rect;
  }

  // Mục hiển thị ở chế độ sửa: widget có sẵn (chưa xoá) + trường mới chờ tạo.
  function designItems(idx) {
    const out = [];
    for (const w of FX.widgets) {
      if (w.pageIndex !== idx) continue;
      const e = FX.edits.get(w.id);
      if (e && e.delete) continue;
      const props = (e && e.props) || null;
      out.push({ key: w.id, kind: props ? props.kind : w.kind, name: props ? props.name : w.name, rect: editedRect(w), existing: w });
    }
    for (const c of FX.creates) {
      if (c.spec.pageIndex !== idx) continue;
      out.push({ key: c.key, kind: c.spec.kind, name: c.spec.name, rect: c.spec.rect, create: c });
    }
    return out;
  }

  function layoutPage(idx) {
    const slot = state.slots[idx];
    if (!slot || !state.pages[idx]) return;
    const fill = FX.design ? [] : FX.widgets.filter((w) => w.pageIndex === idx);
    const design = FX.design ? designItems(idx) : FX.creates.filter((c) => c.spec.pageIndex === idx).map((c) => ({ key: c.key, kind: c.spec.kind, name: c.spec.name, rect: c.spec.rect, create: c, ghost: true }));
    if (!fill.length && !design.length) {
      const l = layerOf(idx, false);
      if (l && !l.querySelector(".fx-draw")) l.remove();
      return;
    }
    const l = layerOf(idx, true);
    const stamp = FX.gen + ":" + (FX.design ? "d" : "f") + ":" + (FX.sel || "");
    if (l.dataset.stamp !== stamp) {
      // Giữ ô đang gõ qua lần dựng lại (đổi chọn trong chế độ sửa không đụng ô điền).
      l.querySelectorAll(":scope > .fx-w, :scope > .fx-d").forEach((n) => { unregister(n); n.remove(); });
      for (const w of fill) l.appendChild(makeFillEl(w));
      for (const it of design) l.appendChild(makeDesignEl(it));
      l.dataset.stamp = stamp;
      applyIcons(l);
    }
    positionLayer(l, idx);
  }

  function rectCss(idx, r) {
    const s = sc();
    const H = state.pages[idx].heightPt;
    return { left: r[0] * s, top: (H - r[3]) * s, width: (r[2] - r[0]) * s, height: (r[3] - r[1]) * s };
  }

  function positionLayer(l, idx) {
    const s = sc();
    for (const el of l.children) {
      const key = el.dataset.key;
      if (!key) continue;
      let r;
      if (el._w) r = el._w.rect;
      else if (el._it) r = el._it.create ? el._it.create.spec.rect : editedRect(el._it.existing);
      if (!r) continue;
      const c = rectCss(idx, r);
      Object.assign(el.style, { left: c.left + "px", top: c.top + "px", width: Math.max(c.width, 4) + "px", height: Math.max(c.height, 4) + "px" });
      if (el._w) styleFill(el, el._w, s, c);
    }
  }

  function rebuildAll() {
    FX.gen++;
    state.slots.forEach((slot, idx) => {
      if (!slot) return;
      if (state.visible.has(idx) || slot.querySelector(":scope > .fx-layer")) layoutPage(idx);
    });
  }
  FX.rebuild = rebuildAll;

  // ---------- Ô điền ----------

  function register(name, wrap) {
    if (!FX.els.has(name)) FX.els.set(name, new Set());
    FX.els.get(name).add(wrap);
  }
  function unregister(wrap) {
    if (wrap._w && FX.els.has(wrap._w.name)) FX.els.get(wrap._w.name).delete(wrap);
  }
  function syncName(name) {
    const set = FX.els.get(name);
    if (set) for (const wrap of set) refreshState(wrap);
  }
  function refreshAllStates() {
    for (const set of FX.els.values()) for (const wrap of set) refreshState(wrap);
  }

  function autoFont(w, hPt) {
    if (w.fontSize > 0) return w.fontSize;
    if (w.kind === "list" || w.multiline) return 12;
    return Math.max(6, Math.min(12 * Math.max(1, hPt / 20), (hPt - 4) / 1.15));
  }

  function styleFill(wrap, w, s, c) {
    const ctl = wrap._ctl;
    if (!ctl) return;
    const hPt = w.rect[3] - w.rect[1];
    const fs = autoFont(w, hPt) * s;
    const bw = (w.borderColor ? Math.max(w.borderWidth, 0) : 0) * s;
    ctl.style.fontSize = fs + "px";
    ctl.style.borderWidth = bw + "px";
    ctl.style.borderColor = w.borderColor ? cssRgb(w.borderColor) : "transparent";
    ctl.style.borderRadius = w.kind === "radio" ? "50%" : "0";
    ctl.style.background = w.fillColor ? cssRgb(w.fillColor) : "#fff";
    ctl.style.color = cssRgb(w.textColor || [0, 0, 0]);
    if (w.kind === "text" || w.kind === "date" || w.kind === "combo") {
      ctl.style.textAlign = ["left", "center", "right"][w.align] || "left";
      ctl.style.padding = w.multiline ? `${2 * s}px ${2 * s}px` : `0 ${2 * s}px`;
      if (w.comb && w.maxLen > 0) {
        const cell = c.width / w.maxLen;
        ctl.style.letterSpacing = Math.max(0, cell - fs * 0.6) + "px";
        ctl.style.paddingLeft = Math.max(0, (cell - fs * 0.6) / 2) + "px";
        ctl.style.fontFamily = "Consolas, 'Courier New', monospace";
      }
    }
    if (w.kind === "checkbox" || w.kind === "radio") {
      ctl.style.fontSize = Math.min(c.width, c.height) * 0.8 + "px";
      ctl.style.lineHeight = c.height - 2 * bw + "px";
    }
    if (w.kind === "button") ctl.style.fontSize = fs + "px";
  }

  function makeFillEl(w) {
    const wrap = document.createElement("div");
    wrap.className = "fx-w fx-k-" + w.kind;
    wrap.dataset.key = w.id;
    wrap.dataset.wid = w.id;
    wrap._w = w;
    if (w.required) wrap.classList.add("fx-req");
    if (w.readOnly) wrap.classList.add("fx-ro");
    if (w.tooltip) wrap.title = w.tooltip;
    let ctl;
    switch (w.kind) {
      case "text":
      case "date": {
        ctl = document.createElement(w.multiline ? "textarea" : "input");
        if (!w.multiline) ctl.type = w.password ? "password" : "text";
        if (w.maxLen > 0) ctl.maxLength = w.maxLen;
        ctl.spellcheck = false;
        ctl.autocomplete = "off";
        ctl.value = curVal(w);
        ctl.addEventListener("input", () => setPending(w.name, ctl.value));
        if (w.kind === "date") {
          ctl.placeholder = w.dateFormat;
          ctl.addEventListener("change", () => {
            const v = formatDateInput(ctl.value, w.dateFormat);
            if (v !== ctl.value) { ctl.value = v; setPending(w.name, v); }
          });
          if (!w.readOnly) wrap.appendChild(datePickerBtn(w, ctl));
        }
        break;
      }
      case "checkbox":
      case "radio": {
        ctl = document.createElement("div");
        ctl.tabIndex = w.readOnly ? -1 : 0;
        ctl.setAttribute("role", w.kind);
        ctl.dataset.mark = MARKS[w.checkStyle] || (w.kind === "radio" ? MARKS.circle : MARKS.check);
        const toggle = () => {
          if (w.readOnly) return;
          const on = curVal(w) === w.exportValue;
          // Radio không bỏ chọn được bằng cách bấm lại (NoToggleToOff, như Foxit).
          if (w.kind === "radio") { if (!on) setPending(w.name, w.exportValue); }
          else setPending(w.name, on ? "Off" : w.exportValue);
        };
        ctl.addEventListener("click", toggle);
        ctl.addEventListener("keydown", (e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); toggle(); } });
        break;
      }
      case "combo": {
        if (w.editable) {
          ctl = document.createElement("input");
          ctl.type = "text";
          const dl = document.createElement("datalist");
          dl.id = "fxdl-" + w.id.replace(/\s+/g, "-");
          w.options.forEach((o) => { const op = document.createElement("option"); op.value = o; dl.appendChild(op); });
          wrap.appendChild(dl);
          ctl.setAttribute("list", dl.id);
          ctl.value = displayOf(w, curVal(w));
          ctl.addEventListener("input", () => setPending(w.name, exportOf(w, ctl.value)));
        } else {
          ctl = document.createElement("select");
          ctl.appendChild(new Option("", ""));
          w.options.forEach((o, i) => ctl.appendChild(new Option(o, w.optionExports[i] || o)));
          ctl.value = curVal(w);
          ctl.addEventListener("change", () => setPending(w.name, ctl.value));
        }
        break;
      }
      case "list": {
        ctl = document.createElement("select");
        ctl.multiple = !!w.multiSelect;
        ctl.size = Math.max(2, w.options.length);
        w.options.forEach((o, i) => ctl.appendChild(new Option(o, w.optionExports[i] || o)));
        ctl.addEventListener("change", () => {
          const vals = [...ctl.selectedOptions].map((o) => o.value);
          setPending(w.name, vals[0] || "", vals);
        });
        break;
      }
      case "button": {
        ctl = document.createElement("div");
        ctl.tabIndex = 0;
        ctl.setAttribute("role", "button");
        ctl.addEventListener("click", () => runAction(w));
        ctl.addEventListener("keydown", (e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); runAction(w); } });
        break;
      }
      default: {
        ctl = document.createElement("div");
        ctl.tabIndex = 0;
        ctl.setAttribute("role", "button");
        ctl.title = t("fx.sigClick");
        ctl.addEventListener("click", () => signField(w));
        break;
      }
    }
    ctl.classList.add("fx-ctl");
    if (w.readOnly && "disabled" in ctl) ctl.disabled = true;
    ctl.addEventListener("keydown", (e) => {
      if (e.key === "Tab") { e.preventDefault(); focusNext(w, e.shiftKey ? -1 : 1); }
      else if (e.key === "Escape") { e.stopPropagation(); ctl.blur(); }
    });
    ctl.addEventListener("focus", () => wrap.classList.add("fx-focus"));
    ctl.addEventListener("blur", () => { wrap.classList.remove("fx-focus"); refreshState(wrap); });
    // Chuột trong ô: không để main.js coi là bấm vùng trống / bắt đầu chọn chữ.
    ctl.addEventListener("mousedown", (e) => e.stopPropagation());
    wrap.prepend(ctl);
    wrap._ctl = ctl;
    register(w.name, wrap);
    refreshState(wrap);
    return wrap;
  }

  function displayOf(w, v) {
    const i = w.optionExports.indexOf(v);
    return i >= 0 ? w.options[i] : v;
  }
  function exportOf(w, d) {
    const i = w.options.indexOf(d);
    return i >= 0 ? w.optionExports[i] || d : d;
  }

  function refreshState(wrap) {
    const w = wrap._w;
    const ctl = wrap._ctl;
    if (!w || !ctl) return;
    wrap.classList.toggle("fx-dirty", FX.pending.has(w.name));
    const focused = document.activeElement === ctl;
    switch (w.kind) {
      case "text":
      case "date":
        if (!focused && ctl.value !== curVal(w)) ctl.value = curVal(w);
        break;
      case "checkbox":
      case "radio": {
        const on = curVal(w) === w.exportValue;
        ctl.classList.toggle("on", on);
        ctl.setAttribute("aria-checked", on ? "true" : "false");
        break;
      }
      case "combo":
        if (!focused) ctl.value = w.editable ? displayOf(w, curVal(w)) : curVal(w);
        break;
      case "list": {
        const vals = curVals(w);
        for (const o of ctl.options) o.selected = vals.includes(o.value);
        break;
      }
      case "button":
        ctl.textContent = w.caption || "";
        break;
    }
    const missing = wrap.classList.contains("fx-missing");
    if (missing && !isEmptyVal(w)) wrap.classList.remove("fx-missing");
  }

  function isEmptyVal(w) {
    if (w.kind === "checkbox" || w.kind === "radio") {
      return !FX.widgets.some((x) => x.name === w.name && curVal(x) === x.exportValue && !isOff(curVal(x)));
    }
    if (w.kind === "list") return !curVals(w).length;
    return !String(curVal(w) || "").trim();
  }

  // Tab / Shift+Tab: theo thứ tự tab của tài liệu, qua cả trang chưa dựng.
  function focusNext(w, dir) {
    const order = FX.widgets.filter((x) => !x.readOnly && x.kind !== "signature");
    if (!order.length) return;
    let i = order.findIndex((x) => x.id === w.id);
    i = (i + dir + order.length) % order.length;
    focusWidget(order[i]);
  }
  function focusWidget(w) {
    layoutPage(w.pageIndex);
    const l = layerOf(w.pageIndex, false);
    const wrap = l && [...l.children].find((n) => n._w && n._w.id === w.id);
    if (!wrap) return;
    wrap.scrollIntoView({ block: "nearest", inline: "nearest" });
    wrap._ctl.focus({ preventScroll: true });
    if (wrap._ctl.select && (w.kind === "text" || w.kind === "date")) wrap._ctl.select();
  }

  // ---------- Ngày ----------

  const MON = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  function fmtDate(d, fmt) {
    const pad = (n) => String(n).padStart(2, "0");
    return fmt.replace(/yyyy|yy|mmmm|mmm|mm|m|dd|d/g, (tok) => ({
      yyyy: String(d.getFullYear()), yy: pad(d.getFullYear() % 100),
      mmmm: MON[d.getMonth()], mmm: MON[d.getMonth()], mm: pad(d.getMonth() + 1), m: String(d.getMonth() + 1),
      dd: pad(d.getDate()), d: String(d.getDate()),
    })[tok]);
  }
  // Chuỗi người dùng gõ → ngày theo đúng định dạng (thứ tự d/m/y lấy từ định dạng).
  function parseDate(v, fmt) {
    const nums = (v.match(/\d+/g) || []).map(Number);
    const monName = MON.findIndex((m) => new RegExp(m, "i").test(v));
    const order = (fmt.match(/y+|m+|d+/g) || []).map((x) => x[0]);
    const vals = {};
    let k = 0;
    for (const o of order) {
      if (o === "m" && monName >= 0 && /mmm/.test(fmt)) { vals.m = monName + 1; continue; }
      vals[o] = nums[k++];
    }
    if (!vals.d || !vals.m || vals.y == null) return null;
    let y = vals.y;
    if (y < 100) y += y < 50 ? 2000 : 1900;
    const d = new Date(y, vals.m - 1, vals.d);
    return d.getDate() === vals.d && d.getMonth() === vals.m - 1 ? d : null;
  }
  function formatDateInput(v, fmt) {
    if (!v.trim()) return v;
    const d = parseDate(v, fmt || "dd/mm/yyyy");
    return d ? fmtDate(d, fmt || "dd/mm/yyyy") : v;
  }
  function datePickerBtn(w, ctl) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "fx-cal";
    b.tabIndex = -1;
    b.title = t("fx.pickDate");
    b.innerHTML = '<i data-icon="field-date"></i>';
    const pick = document.createElement("input");
    pick.type = "date";
    pick.className = "fx-datepick";
    pick.tabIndex = -1;
    b.appendChild(pick);
    b.addEventListener("mousedown", (e) => e.stopPropagation());
    b.addEventListener("click", (e) => {
      e.preventDefault();
      const d = parseDate(ctl.value, w.dateFormat);
      if (d) pick.value = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      try { pick.showPicker(); } catch (_) { pick.focus(); pick.click(); }
    });
    pick.addEventListener("change", () => {
      if (!pick.value) return;
      const [y, m, d] = pick.value.split("-").map(Number);
      const v = fmtDate(new Date(y, m - 1, d), w.dateFormat || "dd/mm/yyyy");
      ctl.value = v;
      setPending(w.name, v);
    });
    return b;
  }

  // ---------- Nút bấm / chữ ký ----------

  async function runAction(w) {
    const a = w.action || { kind: "none" };
    if (a.kind === "reset") { resetValues("defaults"); return; }
    if (a.kind === "url" && a.target) {
      try { await invoke("formx_open_url", { url: a.target }); }
      catch (e) { $("status").textContent = t("fx.err", { e }); }
      return;
    }
    if (a.kind === "print") { $("status").textContent = t("fx.printNA"); return; }
    if (a.kind === "submit") { $("status").textContent = t("fx.submitNA", { url: a.target || "" }); return; }
    if (a.kind === "js") { $("status").textContent = t("fx.jsNA"); return; }
    $("status").textContent = t("fx.noAction");
  }
  function signField() {
    if (FX.dirtyCount()) { $("status").textContent = t("fx.saveBeforeSign"); return; }
    if (typeof openSignDialog === "function") openSignDialog();
  }

  // ---------- Chế độ sửa trường ----------

  function itemRect(key) {
    const c = FX.creates.find((x) => x.key === key);
    if (c) return c.spec.rect;
    const w = FX.widgets.find((x) => x.id === key);
    return w ? editedRect(w) : null;
  }
  function itemPage(key) {
    const c = FX.creates.find((x) => x.key === key);
    if (c) return c.spec.pageIndex;
    const w = FX.widgets.find((x) => x.id === key);
    return w ? w.pageIndex : null;
  }
  function setItemRect(key, r) {
    const c = FX.creates.find((x) => x.key === key);
    if (c) { c.spec.rect = r; return; }
    const e = FX.edits.get(key) || {};
    e.rect = r;
    FX.edits.set(key, e);
    updateDirty();
  }

  function makeDesignEl(it) {
    const el = document.createElement("div");
    el.className = "fx-d fx-k-" + it.kind + (it.create ? " fx-new" : "") + (it.create && it.create.proposed ? " fx-prop" : "") + (it.ghost ? " fx-ghost" : "");
    el.dataset.key = it.key;
    el._it = it;
    const tag = document.createElement("span");
    tag.className = "fx-dn";
    tag.textContent = it.name;
    el.title = `${it.name} — ${kindLabel(it.kind)}`;
    el.appendChild(tag);
    if (it.ghost) return el;
    if (it.create && it.create.proposed) {
      const x = document.createElement("button");
      x.className = "fx-px";
      x.type = "button";
      x.title = t("fx.dropProposal");
      x.textContent = "×";
      x.addEventListener("mousedown", (e) => e.stopPropagation());
      x.addEventListener("click", (e) => { e.stopPropagation(); deleteItem(it.key); });
      el.appendChild(x);
    }
    if (FX.sel === it.key) {
      el.classList.add("fx-sel");
      for (const h of ["nw", "ne", "sw", "se"]) {
        const hd = document.createElement("span");
        hd.className = "fx-h fx-h-" + h;
        hd.dataset.h = h;
        el.appendChild(hd);
      }
      const bar = document.createElement("div");
      bar.className = "fx-dbar";
      bar.innerHTML = `<button type="button" data-a="props">${t("fx.properties")}</button><button type="button" data-a="del">${t("fx.delete")}</button>`;
      bar.addEventListener("mousedown", (e) => e.stopPropagation());
      bar.addEventListener("click", (e) => {
        const a = e.target.closest("[data-a]");
        if (!a) return;
        if (a.dataset.a === "props") openProps(it.key);
        else deleteItem(it.key);
      });
      el.appendChild(bar);
    }
    el.addEventListener("mousedown", (e) => startMove(e, it));
    el.addEventListener("dblclick", (e) => { e.stopPropagation(); openProps(it.key); });
    return el;
  }

  function select(key) {
    FX.sel = key;
    const p = key != null ? itemPage(key) : null;
    // Chỉ dựng lại trang liên quan (stamp gồm sel → mọi trang có lớp phủ tự cập nhật).
    state.slots.forEach((slot, idx) => { if (slot && slot.querySelector(":scope > .fx-layer")) layoutPage(idx); });
    if (p != null) layoutPage(p);
  }

  function startMove(e, it) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    if (FX.sel !== it.key) select(it.key);
    const handle = e.target.dataset && e.target.dataset.h;
    const r0 = itemRect(it.key).slice();
    const idx = itemPage(it.key);
    const P = state.pages[idx];
    const s = sc();
    const x0 = e.clientX, y0 = e.clientY;
    let moved = false;
    let cur = r0;
    const onMove = (ev) => {
      const dx = (ev.clientX - x0) / s;
      const dy = -(ev.clientY - y0) / s;
      if (!moved && Math.abs(ev.clientX - x0) + Math.abs(ev.clientY - y0) < 3) return;
      moved = true;
      let [l, b, r, tp] = r0;
      if (!handle) { l += dx; r += dx; b += dy; tp += dy; }
      else {
        if (handle.includes("w")) l = Math.min(l + dx, r - 6);
        if (handle.includes("e")) r = Math.max(r + dx, l + 6);
        if (handle.includes("s")) b = Math.min(b + dy, tp - 6);
        if (handle.includes("n")) tp = Math.max(tp + dy, b + 6);
      }
      // Giữ trong trang.
      const w = r - l, h = tp - b;
      if (!handle) {
        if (l < 0) { l = 0; r = w; }
        if (r > P.widthPt) { r = P.widthPt; l = r - w; }
        if (b < 0) { b = 0; tp = h; }
        if (tp > P.heightPt) { tp = P.heightPt; b = tp - h; }
      }
      cur = [l, b, r, tp];
      const el = layerOf(idx, false) && [...layerOf(idx, false).children].find((n) => n.dataset.key === it.key);
      if (el) {
        const c = rectCss(idx, cur);
        Object.assign(el.style, { left: c.left + "px", top: c.top + "px", width: c.width + "px", height: c.height + "px" });
      }
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove, true);
      window.removeEventListener("mouseup", onUp, true);
      if (moved) {
        setItemRect(it.key, cur.map((v) => Math.round(v * 100) / 100));
        layoutPage(idx);
      }
    };
    window.addEventListener("mousemove", onMove, true);
    window.addEventListener("mouseup", onUp, true);
  }

  function deleteItem(key) {
    const ci = FX.creates.findIndex((x) => x.key === key);
    if (ci >= 0) FX.creates.splice(ci, 1);
    else if (FX.widgets.some((w) => w.id === key)) FX.edits.set(key, { delete: true });
    if (FX.sel === key) FX.sel = null;
    FX.gen++;
    rebuildAll();
    updateDirty();
    if (!FX.creates.some((c) => c.proposed)) hideRecogBar();
  }

  function setDesign(on) {
    FX.design = !!on;
    if (!on) { FX.sel = null; setArm(null, true); }
    else if (state.tool) setTool(state.tool); // tắt công cụ chú thích
    document.body.classList.toggle("fx-design", FX.design);
    const b = $("fxDesign");
    if (b) b.classList.toggle("on", FX.design);
    if (document.activeElement && document.activeElement.classList.contains("fx-ctl")) document.activeElement.blur();
    FX.gen++;
    rebuildAll();
    $("formHint").textContent = FX.design ? t("fx.designHint") : "";
  }

  function setArm(kind, silent) {
    FX.arm = kind && FX.arm !== kind ? kind : null;
    document.querySelectorAll("[data-fx-tool]").forEach((b) => b.classList.toggle("armed", b.dataset.fxTool === FX.arm));
    document.body.classList.toggle("fx-arming", !!FX.arm);
    if (FX.arm) {
      if (!FX.design) setDesign(true);
      $("formHint").textContent = t("fx.armHint", { kind: kindLabel(FX.arm) });
    } else if (!silent) {
      $("formHint").textContent = FX.design ? t("fx.designHint") : "";
    }
  }

  function allNames() {
    const s = new Set();
    for (const w of FX.widgets) {
      const e = FX.edits.get(w.id);
      if (e && e.delete) continue;
      s.add(e && e.props ? e.props.name : w.name);
    }
    for (const c of FX.creates) s.add(c.spec.name);
    return s;
  }
  function uniqueName(base) {
    const names = allNames();
    if (!names.has(base) && /\d$/.test(base)) return base;
    for (let n = 1; ; n++) if (!names.has(base + n)) return base + n;
  }
  function uniqueLabel(label) {
    const names = allNames();
    if (!names.has(label)) return label;
    for (let n = 2; ; n++) if (!names.has(`${label} ${n}`)) return `${label} ${n}`;
  }

  function defaultSpec(kind, pageIndex, rect, name) {
    const s = {
      name: name || uniqueName(NAME_BASE[kind] || "Field"),
      kind, pageIndex, rect,
      tooltip: "", required: false, readOnly: false,
      value: "", values: [], fontSize: 0, align: 0,
      textColor: [0, 0, 0], borderColor: [0.55, 0.55, 0.55], fillColor: null, borderWidth: 1, borderStyle: "solid",
      options: [], optionExports: [], exportValue: "", checked: false, checkStyle: "",
      multiline: false, maxLen: 0, comb: false, password: false, doNotScroll: false,
      multiSelect: false, editable: false, caption: "", action: { kind: "none", target: "" }, dateFormat: "",
    };
    if (kind === "checkbox") { s.borderColor = [0, 0, 0]; s.exportValue = "Yes"; s.checkStyle = "check"; }
    if (kind === "radio") {
      s.borderColor = [0, 0, 0];
      s.checkStyle = "circle";
      // Nút radio vẽ liền nhau cùng trang → cùng nhóm (Foxit gợi ý nhóm gần nhất).
      const last = [...FX.creates].reverse().find((c) => c.spec.kind === "radio" && c.spec.pageIndex === pageIndex);
      if (!name && last) s.name = last.spec.name;
      const count = FX.creates.filter((c) => c.spec.kind === "radio" && c.spec.name === s.name).length +
        FX.widgets.filter((w) => w.kind === "radio" && w.name === s.name).length;
      s.exportValue = "Choice" + (count + 1);
    }
    if (kind === "combo" || kind === "list") s.options = [];
    if (kind === "button") { s.fillColor = [0.85, 0.85, 0.85]; s.borderColor = [0.45, 0.45, 0.45]; s.borderStyle = "beveled"; s.caption = t("fx.defaultCaption"); s.align = 1; }
    if (kind === "date") s.dateFormat = "dd/mm/yyyy";
    if (kind === "signature") s.borderColor = [0.45, 0.45, 0.45];
    return s;
  }

  // Vẽ khung trường mới trên trang (capture-phase: chỉ khi công cụ form đang bật).
  let draw = null;
  $("pages").addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    const slot = e.target.closest && e.target.closest(".page-slot");
    if (!FX.arm) {
      // Chế độ sửa: bấm vùng trống của trang = bỏ chọn.
      if (FX.design && slot && !e.target.closest(".fx-d") && FX.sel) select(null);
      return;
    }
    if (!slot) return;
    e.preventDefault();
    e.stopPropagation();
    const idx = Number(slot.dataset.index);
    const r = slot.getBoundingClientRect();
    const l = layerOf(idx, true);
    const box = document.createElement("div");
    box.className = "fx-draw";
    l.appendChild(box);
    draw = { idx, slot, box, x0: e.clientX - r.left, y0: e.clientY - r.top, x1: e.clientX - r.left, y1: e.clientY - r.top };
    const onMove = (ev) => {
      const rr = draw.slot.getBoundingClientRect();
      draw.x1 = Math.max(0, Math.min(rr.width, ev.clientX - rr.left));
      draw.y1 = Math.max(0, Math.min(rr.height, ev.clientY - rr.top));
      Object.assign(box.style, {
        left: Math.min(draw.x0, draw.x1) + "px", top: Math.min(draw.y0, draw.y1) + "px",
        width: Math.abs(draw.x1 - draw.x0) + "px", height: Math.abs(draw.y1 - draw.y0) + "px",
      });
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove, true);
      window.removeEventListener("mouseup", onUp, true);
      const d = draw;
      draw = null;
      box.remove();
      finishDraw(d);
    };
    window.addEventListener("mousemove", onMove, true);
    window.addEventListener("mouseup", onUp, true);
  }, true);

  function finishDraw(d) {
    const kind = FX.arm;
    if (!kind) return;
    const s = sc();
    const P = state.pages[d.idx];
    let l = Math.min(d.x0, d.x1) / s, r = Math.max(d.x0, d.x1) / s;
    let top = P.heightPt - Math.min(d.y0, d.y1) / s, bottom = P.heightPt - Math.max(d.y0, d.y1) / s;
    if (r - l < 4 || top - bottom < 4) {
      const [w, h] = DEF_SIZE[kind];
      l = d.x0 / s; top = P.heightPt - d.y0 / s;
      r = Math.min(P.widthPt, l + w); bottom = Math.max(0, top - h);
    }
    const rect = [l, bottom, r, top].map((v) => Math.round(v * 100) / 100);
    const key = "new:" + FX.seq++;
    FX.creates.push({ key, spec: defaultSpec(kind, d.idx, rect), proposed: false });
    setArm(null);
    FX.sel = key;
    FX.gen++;
    rebuildAll();
    updateDirty();
    openProps(key, true);
  }

  // ---------- Hộp Thuộc tính ----------

  function specFromWidget(w) {
    return {
      name: w.name, kind: w.kind, pageIndex: w.pageIndex, rect: editedRect(w),
      tooltip: w.tooltip, required: w.required, readOnly: w.readOnly,
      value: w.kind === "checkbox" || w.kind === "radio" ? "" : w.defaultValue, values: [],
      fontSize: w.fontSize, align: w.align, textColor: w.textColor,
      borderColor: w.borderWidth > 0 ? w.borderColor : null, fillColor: w.fillColor,
      borderWidth: w.borderWidth > 0 ? w.borderWidth : 1, borderStyle: w.borderStyle || "solid",
      options: w.options.slice(), optionExports: w.optionExports.slice(),
      exportValue: w.exportValue, checked: (w.kind === "checkbox" || w.kind === "radio") && w.defaultValue === w.exportValue,
      checkStyle: w.checkStyle, multiline: w.multiline, maxLen: w.maxLen, comb: w.comb, password: w.password,
      doNotScroll: w.doNotScroll, multiSelect: w.multiSelect, editable: w.editable, caption: w.caption,
      action: { kind: (w.action && w.action.kind) || "none", target: (w.action && w.action.target) || "" },
      dateFormat: w.dateFormat,
    };
  }

  function specOf(key) {
    const c = FX.creates.find((x) => x.key === key);
    if (c) return c.spec;
    const w = FX.widgets.find((x) => x.id === key);
    if (!w) return null;
    const e = FX.edits.get(key);
    return (e && e.props) ? e.props : specFromWidget(w);
  }

  function openProps(key, fresh) {
    const sp0 = specOf(key);
    if (!sp0) return;
    const sp = JSON.parse(JSON.stringify(sp0));
    const k = sp.kind;
    const isText = k === "text" || k === "date";
    const hasFont = isText || k === "combo" || k === "list" || k === "button";
    const opt = (v, label, cur) => `<option value="${escapeHtml(String(v))}" ${String(v) === String(cur) ? "selected" : ""}>${escapeHtml(label)}</option>`;
    const sizes = [0, 6, 8, 9, 10, 11, 12, 14, 16, 18, 20, 24];
    const colorRow = (id, label, c, allowNone) => `
      <div><label>${label}</label><div class="fx-color">
        <input type="color" id="${id}" value="${rgbHex(c || [0, 0, 0])}" ${c ? "" : "disabled"}>
        ${allowNone ? `<label class="fx-inl"><input type="checkbox" id="${id}None" ${c ? "" : "checked"}> ${t("fx.p.none")}</label>` : ""}
      </div></div>`;

    let options = "";
    if (isText) {
      options += `<label>${t("fx.p.defaultValue")}</label>${sp.multiline ? `<textarea id="fxpVal" rows="3">${escapeHtml(sp.value)}</textarea>` : `<input type="text" id="fxpVal" value="${escapeHtml(sp.value)}">`}`;
      if (k === "date") {
        options += `<label>${t("fx.p.dateFormat")}</label><select id="fxpDate">${[...new Set([sp.dateFormat || "dd/mm/yyyy", ...DATE_FORMATS])].map((f) => opt(f, f, sp.dateFormat)).join("")}</select>`;
      } else {
        options += `<div class="fx-checks">
          <label class="fx-inl"><input type="checkbox" id="fxpMulti" ${sp.multiline ? "checked" : ""}> ${t("fx.p.multiline")}</label>
          <label class="fx-inl"><input type="checkbox" id="fxpPwd" ${sp.password ? "checked" : ""}> ${t("fx.p.password")}</label>
          <label class="fx-inl"><input type="checkbox" id="fxpComb" ${sp.comb ? "checked" : ""}> ${t("fx.p.comb")}</label>
        </div>`;
      }
      options += `<label>${t("fx.p.maxLen")}</label><input type="number" id="fxpMax" min="0" value="${sp.maxLen || 0}">`;
    } else if (k === "checkbox" || k === "radio") {
      options += `<label>${t("fx.p.exportValue")}</label><input type="text" id="fxpExport" value="${escapeHtml(sp.exportValue)}">
        <label>${t("fx.p.checkStyle")}</label><select id="fxpStyle">${Object.keys(MARKS).map((s) => opt(s, `${MARKS[s]}  ${t("fx.style." + s)}`, sp.checkStyle || (k === "radio" ? "circle" : "check"))).join("")}</select>
        <div class="fx-checks"><label class="fx-inl"><input type="checkbox" id="fxpChecked" ${sp.checked ? "checked" : ""}> ${t("fx.p.checkedDefault")}</label></div>`;
      if (k === "radio") options += `<p class="muted fx-note">${t("fx.p.radioNote")}</p>`;
    } else if (k === "combo" || k === "list") {
      options += `<label>${t("fx.p.items")}</label>
        <div class="row fx-optadd"><input type="text" id="fxpItem" placeholder="${t("fx.p.itemPh")}"><input type="text" id="fxpItemEx" placeholder="${t("fx.p.exportPh")}"><button type="button" id="fxpAdd">${t("fx.p.add")}</button></div>
        <div class="row fx-optlist"><select id="fxpList" size="6"></select>
          <div class="fx-optbtns"><button type="button" id="fxpUp">${t("fx.p.up")}</button><button type="button" id="fxpDown">${t("fx.p.down")}</button><button type="button" id="fxpDefault">${t("fx.p.setDefault")}</button><button type="button" id="fxpDel">${t("fx.p.remove")}</button></div></div>
        <p class="muted fx-note" id="fxpDefNote"></p>
        <div class="fx-checks">${k === "combo"
          ? `<label class="fx-inl"><input type="checkbox" id="fxpEdit" ${sp.editable ? "checked" : ""}> ${t("fx.p.editable")}</label>`
          : `<label class="fx-inl"><input type="checkbox" id="fxpMultiSel" ${sp.multiSelect ? "checked" : ""}> ${t("fx.p.multiSelect")}</label>`}</div>`;
    } else if (k === "button") {
      options += `<label>${t("fx.p.caption")}</label><input type="text" id="fxpCap" value="${escapeHtml(sp.caption)}">
        <label>${t("fx.p.action")}</label><select id="fxpAct">${["none", "reset", "print", "url", "submit"].map((a) => opt(a, t("fx.act." + a), sp.action.kind)).join("")}</select>
        <div id="fxpUrlRow"><label>${t("fx.p.url")}</label><input type="text" id="fxpUrl" value="${escapeHtml(sp.action.target || "")}" placeholder="https://"></div>`;
    } else {
      options += `<p class="muted fx-note">${t("fx.p.sigNote")}</p>`;
    }

    const box = openModal(escapeHtml(t("fx.p.title", { kind: kindLabel(k) })), `
      <div class="fx-tabs"><button type="button" class="on" data-tab="g">${t("fx.p.tabGeneral")}</button><button type="button" data-tab="a">${t("fx.p.tabAppearance")}</button><button type="button" data-tab="o">${t("fx.p.tabOptions")}</button></div>
      <div class="fx-tab" data-tab="g">
        <label>${t("fx.p.name")}</label><input type="text" id="fxpName" value="${escapeHtml(sp.name)}">
        <label>${t("fx.p.tooltip")}</label><input type="text" id="fxpTip" value="${escapeHtml(sp.tooltip)}">
        <div class="fx-checks">
          <label class="fx-inl"><input type="checkbox" id="fxpReq" ${sp.required ? "checked" : ""}> ${t("fx.p.required")}</label>
          <label class="fx-inl"><input type="checkbox" id="fxpRo" ${sp.readOnly ? "checked" : ""}> ${t("fx.p.readOnly")}</label>
        </div>
      </div>
      <div class="fx-tab hidden" data-tab="a">
        <div class="row">${colorRow("fxpBc", t("fx.p.borderColor"), sp.borderColor, true)}${colorRow("fxpBg", t("fx.p.fillColor"), sp.fillColor, true)}</div>
        <div class="row">
          <div><label>${t("fx.p.borderWidth")}</label><select id="fxpBw">${[0.5, 1, 2, 3].map((v) => opt(v, t("fx.bw." + String(v).replace(".", "_")), sp.borderWidth)).join("")}</select></div>
          <div><label>${t("fx.p.borderStyle")}</label><select id="fxpBs">${["solid", "dashed", "beveled", "inset", "underline"].map((v) => opt(v, t("fx.bs." + v), sp.borderStyle)).join("")}</select></div>
        </div>
        <div class="row">
          ${colorRow("fxpTc", t("fx.p.textColor"), sp.textColor || [0, 0, 0], false)}
          ${hasFont || k === "checkbox" || k === "radio" ? `<div><label>${t("fx.p.fontSize")}</label><select id="fxpFs">${sizes.map((v) => opt(v, v ? String(v) : t("fx.p.auto"), sp.fontSize)).join("")}</select></div>` : "<div></div>"}
        </div>
        ${hasFont ? `<label>${t("fx.p.align")}</label><select id="fxpAlign">${[0, 1, 2].map((v) => opt(v, t("fx.align." + v), sp.align)).join("")}</select>` : ""}
      </div>
      <div class="fx-tab hidden" data-tab="o">${options}</div>
      <div class="err" id="fxpErr"></div>
      <div class="foot fx-foot"><button type="button" id="fxpRemove" class="fx-left">${t("fx.delete")}</button><button type="button" id="fxpCancel">${t("common.cancel")}</button><button type="button" id="fxpOk" class="primary">${t("common.ok")}</button></div>
    `);
    box.classList.add("fx-props");
    const q = (id) => box.querySelector("#" + id);
    box.querySelectorAll(".fx-tabs [data-tab]").forEach((b) => b.addEventListener("click", () => {
      box.querySelectorAll(".fx-tabs [data-tab]").forEach((x) => x.classList.toggle("on", x === b));
      box.querySelectorAll(".fx-tab").forEach((p) => p.classList.toggle("hidden", p.dataset.tab !== b.dataset.tab));
    }));
    for (const id of ["fxpBc", "fxpBg"]) {
      const none = q(id + "None");
      if (none) none.addEventListener("change", () => { q(id).disabled = none.checked; });
    }
    // Danh sách lựa chọn (combo/list).
    const items = sp.options.map((d, i) => ({ d, e: sp.optionExports[i] || "" }));
    let defaults = new Set(k === "list" && sp.values.length ? sp.values : sp.value ? [sp.value] : []);
    const renderList = () => {
      const sel = q("fxpList");
      if (!sel) return;
      const cur = sel.selectedIndex;
      sel.innerHTML = "";
      items.forEach((it) => {
        const ex = it.e || it.d;
        const o = new Option(`${defaults.has(ex) ? "★ " : ""}${it.d}${it.e && it.e !== it.d ? `  [${it.e}]` : ""}`, ex);
        sel.appendChild(o);
      });
      if (cur >= 0 && cur < items.length) sel.selectedIndex = cur;
      q("fxpDefNote").textContent = defaults.size ? t("fx.p.defaultIs", { v: [...defaults].map((v) => (items.find((i) => (i.e || i.d) === v) || { d: v }).d).join(", ") }) : "";
    };
    if (q("fxpList")) {
      renderList();
      const add = () => {
        const d = q("fxpItem").value.trim();
        if (!d) return;
        if (items.some((i) => i.d === d)) { q("fxpErr").textContent = t("fx.p.itemDup"); return; }
        items.push({ d, e: q("fxpItemEx").value.trim() });
        q("fxpItem").value = ""; q("fxpItemEx").value = "";
        q("fxpErr").textContent = "";
        renderList();
        q("fxpItem").focus();
      };
      q("fxpAdd").addEventListener("click", add);
      q("fxpItem").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); add(); } });
      q("fxpItemEx").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); add(); } });
      const move = (d) => {
        const i = q("fxpList").selectedIndex;
        const j = i + d;
        if (i < 0 || j < 0 || j >= items.length) return;
        [items[i], items[j]] = [items[j], items[i]];
        renderList();
        q("fxpList").selectedIndex = j;
      };
      q("fxpUp").addEventListener("click", () => move(-1));
      q("fxpDown").addEventListener("click", () => move(1));
      q("fxpDel").addEventListener("click", () => {
        const i = q("fxpList").selectedIndex;
        if (i < 0) return;
        defaults.delete(items[i].e || items[i].d);
        items.splice(i, 1);
        renderList();
      });
      q("fxpDefault").addEventListener("click", () => {
        const i = q("fxpList").selectedIndex;
        if (i < 0) return;
        const ex = items[i].e || items[i].d;
        const multi = k === "list" && q("fxpMultiSel") && q("fxpMultiSel").checked;
        if (defaults.has(ex)) defaults.delete(ex);
        else { if (!multi) defaults = new Set(); defaults.add(ex); }
        renderList();
      });
    }
    const act = q("fxpAct");
    if (act) {
      const sync = () => q("fxpUrlRow").classList.toggle("hidden", !(act.value === "url" || act.value === "submit"));
      act.addEventListener("change", sync);
      sync();
    }
    const comb = q("fxpComb");
    if (comb) comb.addEventListener("change", () => { if (comb.checked && !(Number(q("fxpMax").value) > 0)) q("fxpMax").value = 10; });

    q("fxpCancel").addEventListener("click", closeModal);
    q("fxpRemove").addEventListener("click", () => { closeModal(); deleteItem(key); });
    q("fxpOk").addEventListener("click", () => {
      const err = q("fxpErr");
      const name = q("fxpName").value.trim();
      if (!name) { err.textContent = t("fx.p.nameRequired"); return; }
      // Tên trùng: radio cùng tên = cùng nhóm; loại khác → lỗi.
      const clash = [...FX.widgets.filter((w) => w.id !== key && !(FX.edits.get(w.id) || {}).delete).map((w) => {
        const e = FX.edits.get(w.id);
        return { name: e && e.props ? e.props.name : w.name, kind: w.kind, group: w.name === sp0.name };
      }), ...FX.creates.filter((c) => c.key !== key).map((c) => ({ name: c.spec.name, kind: c.spec.kind }))]
        .find((x) => x.name === name && !(k === "radio" && x.kind === "radio") && !(x.group && name === sp0.name));
      if (clash) { err.textContent = t("fx.p.nameTaken", { name }); return; }
      sp.name = name;
      sp.tooltip = q("fxpTip").value;
      sp.required = q("fxpReq").checked;
      sp.readOnly = q("fxpRo").checked;
      sp.borderColor = q("fxpBcNone").checked ? null : hexRgb(q("fxpBc").value);
      sp.fillColor = q("fxpBgNone").checked ? null : hexRgb(q("fxpBg").value);
      sp.borderWidth = sp.borderColor ? Number(q("fxpBw").value) : 0;
      sp.borderStyle = q("fxpBs").value;
      sp.textColor = hexRgb(q("fxpTc").value);
      if (q("fxpFs")) sp.fontSize = Number(q("fxpFs").value);
      if (q("fxpAlign")) sp.align = Number(q("fxpAlign").value);
      if (isText) {
        sp.value = q("fxpVal").value;
        sp.maxLen = Math.max(0, Math.floor(Number(q("fxpMax").value) || 0));
        if (k === "date") sp.dateFormat = q("fxpDate").value;
        else {
          sp.multiline = q("fxpMulti").checked;
          sp.password = q("fxpPwd").checked;
          sp.comb = q("fxpComb").checked && sp.maxLen > 0;
          if (sp.comb) sp.multiline = false;
        }
        if (sp.maxLen > 0 && sp.value.length > sp.maxLen) sp.value = sp.value.slice(0, sp.maxLen);
      } else if (k === "checkbox" || k === "radio") {
        const ex = q("fxpExport").value.trim();
        if (!ex || /^off$/i.test(ex)) { err.textContent = t("fx.p.exportInvalid"); return; }
        sp.exportValue = ex;
        sp.checkStyle = q("fxpStyle").value;
        sp.checked = q("fxpChecked").checked;
      } else if (k === "combo" || k === "list") {
        if (!items.length) { err.textContent = t("fx.p.needItems"); return; }
        sp.options = items.map((i) => i.d);
        sp.optionExports = items.map((i) => i.e);
        const vals = [...defaults];
        sp.value = vals[0] || "";
        sp.values = k === "list" ? vals : [];
        if (k === "combo") sp.editable = q("fxpEdit").checked;
        else sp.multiSelect = q("fxpMultiSel").checked;
      } else if (k === "button") {
        sp.caption = q("fxpCap").value;
        sp.action = { kind: act.value, target: q("fxpUrl").value.trim() };
        if ((act.value === "url" || act.value === "submit") && !/^(https?:|mailto:)/i.test(sp.action.target)) {
          err.textContent = t("fx.p.urlInvalid"); return;
        }
      }
      const c = FX.creates.find((x) => x.key === key);
      if (c) { c.spec = sp; c.proposed = false; }
      else {
        const e = FX.edits.get(key) || {};
        e.props = sp;
        FX.edits.set(key, e);
        // Đổi tên field có sẵn → giá trị chờ đi theo tên mới.
        if (sp.name !== sp0.name && FX.pending.has(sp0.name)) {
          FX.pending.set(sp.name, FX.pending.get(sp0.name));
          FX.pending.delete(sp0.name);
        }
      }
      closeModal();
      FX.gen++;
      rebuildAll();
      updateDirty();
      if (!FX.creates.some((x) => x.proposed)) hideRecogBar();
    });
    if (fresh) setTimeout(() => { const n = q("fxpName"); if (n) { n.focus(); n.select(); } }, 0);
  }

  // ---------- Nhận diện trường ----------

  let recogBar = null;
  function showRecogBar(n) {
    hideRecogBar();
    recogBar = document.createElement("div");
    recogBar.className = "fx-recog-bar";
    recogBar.innerHTML = `<span>${escapeHtml(t("fx.recogFound", { n }))}</span>
      <button type="button" data-a="keep" class="primary">${t("fx.recogKeep")}</button>
      <button type="button" data-a="drop">${t("fx.recogDrop")}</button>`;
    recogBar.addEventListener("click", (e) => {
      const a = e.target.closest("[data-a]");
      if (!a) return;
      if (a.dataset.a === "keep") FX.creates.forEach((c) => { c.proposed = false; });
      else FX.creates = FX.creates.filter((c) => !c.proposed);
      FX.sel = null;
      FX.gen++;
      rebuildAll();
      updateDirty();
      hideRecogBar();
    });
    $("viewport").parentElement.appendChild(recogBar);
  }
  function hideRecogBar() {
    if (recogBar) { recogBar.remove(); recogBar = null; }
  }

  async function recognize() {
    if (!state.path) return;
    const btn = $("fxRecognize");
    btn.disabled = true;
    actionMsg("formHint", t("fx.recognizing"));
    try {
      const props = await invoke("formx_recognize", { path: state.path, pages: null });
      if (!props.length) { actionMsg("formHint", t("fx.recogNone")); return; }
      FX.creates = FX.creates.filter((c) => !c.proposed);
      for (const p of props) {
        const kind = p.kind === "checkbox" ? "checkbox" : "text";
        const name = uniqueLabel(p.name || uniqueName(NAME_BASE[kind]));
        const spec = defaultSpec(kind, p.pageIndex, p.rect.map((v) => Math.round(v * 100) / 100), name);
        spec.tooltip = p.tooltip || "";
        FX.creates.push({ key: "new:" + FX.seq++, spec, proposed: true });
      }
      if (!FX.design) setDesign(true);
      FX.gen++;
      rebuildAll();
      updateDirty();
      showRecogBar(props.length);
      actionMsg("formHint", t("fx.recogFound", { n: props.length }));
      const first = props[0];
      if (first && !state.visible.has(first.pageIndex)) goToPage(first.pageIndex);
    } catch (e) {
      actionMsg("formHint", t("fx.err", { e }));
    } finally {
      btn.disabled = false;
    }
  }

  // ---------- Đặt lại / nhập / xuất / lưu ----------

  async function resetValues(mode) {
    const fillable = FX.widgets.filter((w) => !w.readOnly && !["button", "signature"].includes(w.kind));
    if (!fillable.length) { $("status").textContent = t("fx.noFields"); return; }
    const ok = await confirmModal(t(mode === "clear" ? "fx.clearTitle" : "fx.resetTitle"), t(mode === "clear" ? "fx.clearMsg" : "fx.resetMsg"), t(mode === "clear" ? "fx.clearAll" : "fx.resetDefaults"));
    if (!ok) return;
    const done = new Set();
    for (const w of fillable) {
      if (done.has(w.name)) continue;
      done.add(w.name);
      if (mode === "clear") setPending(w.name, w.kind === "checkbox" || w.kind === "radio" ? "Off" : "", []);
      else setPending(w.name, w.defaultValue || (w.kind === "checkbox" || w.kind === "radio" ? "Off" : ""), w.kind === "list" && w.defaultValue ? [w.defaultValue] : []);
    }
    refreshAllStates();
    $("status").textContent = t(mode === "clear" ? "fx.clearedNote" : "fx.resetNote");
  }

  function fillsArr() {
    return [...FX.pending.entries()].map(([name, p]) => ({ name, value: p.value, values: p.values || [] }));
  }

  async function importData(kind) {
    if (!state.path) return;
    const f = await invoke("formx_pick_import", { kind });
    if (!f) return;
    try {
      const rows = await invoke("formx_read_data", { data: f });
      let n = 0;
      for (const [name, value, values] of rows) {
        const w = FX.widgets.find((x) => x.name === name);
        if (!w || w.readOnly) continue;
        let v = value;
        if (w.kind === "checkbox" && v && !isOff(v) && !FX.widgets.some((x) => x.name === name && x.exportValue === v)) {
          v = /^(on|true|yes|1|checked)$/i.test(v) ? w.exportValue : "Off";
        }
        setPending(name, v, values || []);
        n++;
      }
      refreshAllStates();
      actionMsg("formHint", t("fx.imported", { n, file: shortName(f) }));
    } catch (e) {
      actionMsg("formHint", t("fx.err", { e }));
    }
  }

  async function exportData(kind) {
    if (!state.path) return;
    const base = shortName(FX.path || state.path).replace(/\.pdf$/i, "");
    const out = await invoke("formx_pick_export", { kind, base });
    if (!out) return;
    try {
      let src = state.path;
      // Giá trị đang điền dở cũng được xuất (ghi tạm ra file rồi xuất từ đó).
      if (FX.pending.size) {
        src = await tempFilePath(`ff_fx_export_${Date.now()}.pdf`);
        await invoke("formx_apply", { input: state.path, creates: [], edits: [], fills: fillsArr(), output: src });
      }
      await invoke("formx_export", { input: src, output: out });
      actionMsg("formHint", t("fx.exported", { file: shortName(out) }));
    } catch (e) {
      actionMsg("formHint", t("fx.err", { e }));
    }
  }

  async function save() {
    if (!state.path || !FX.dirtyCount()) { $("status").textContent = t("shell.nothingToSave"); return; }
    if (document.activeElement && document.activeElement.classList.contains("fx-ctl")) document.activeElement.blur();
    // Trường bắt buộc còn trống → viền đỏ + hỏi trước khi lưu (như Foxit).
    const missing = [];
    const seen = new Set();
    for (const w of FX.widgets) {
      if (!w.required || seen.has(w.name) || (FX.edits.get(w.id) || {}).delete) continue;
      seen.add(w.name);
      if (isEmptyVal(w)) missing.push(w);
    }
    document.querySelectorAll(".fx-w").forEach((el) => el.classList.toggle("fx-missing", !!el._w && missing.some((m) => m.name === el._w.name)));
    if (missing.length) {
      const list = missing.slice(0, 8).map((w) => w.tooltip || w.name).join(", ") + (missing.length > 8 ? "…" : "");
      const ok = await confirmModal(t("fx.reqTitle"), t("fx.reqMsg", { n: missing.length, list }), t("fx.saveAnyway"));
      if (!ok) { focusWidget(missing[0]); return; }
    }
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    const edits = [...FX.edits.entries()].map(([widgetId, e]) => ({ widgetId, delete: !!e.delete, rect: e.rect || null, props: e.props || null }));
    actionMsg("formHint", t("fx.saving"));
    try {
      const n = await invoke("formx_apply", { input: state.path, creates: FX.creates.map((c) => c.spec), edits, fills: fillsArr(), output: out });
      const msg = t("fx.saved", { file: shortName(out), n });
      await loadDocument(out);
      actionMsg("formHint", msg);
      if (state.formMode) refreshFormCount();
    } catch (e) {
      actionMsg("formHint", t("fx.errSave", { e }));
    }
  }
  FX.save = save;

  // ---------- Ribbon & phím tắt ----------

  document.querySelectorAll("[data-fx-tool]").forEach((b) => b.addEventListener("click", () => {
    if (!state.path) return;
    setArm(b.dataset.fxTool);
  }));
  $("fxDesign").addEventListener("click", () => { if (state.path) setDesign(!FX.design); });
  $("fxHighlight").addEventListener("click", () => {
    FX.hl = !FX.hl;
    document.body.classList.toggle("fx-hl", FX.hl);
    $("fxHighlight").classList.toggle("on", FX.hl);
    try { localStorage.setItem("ff.fxHighlight", FX.hl ? "1" : "0"); } catch (_) {}
  });
  $("fxHighlight").classList.toggle("on", FX.hl);
  $("fxRecognize").addEventListener("click", recognize);
  $("fxSave").addEventListener("click", save);
  const menuFor = (btnId, menuId) => $(btnId).addEventListener("click", () => Shell.openMenuAt($(menuId), $(btnId), false));
  menuFor("fxImport", "fxImportMenu");
  menuFor("fxExport", "fxExportMenu");
  menuFor("fxReset", "fxResetMenu");
  $("fxImportMenu").addEventListener("click", (e) => { const it = e.target.closest("[data-fx-import]"); if (!it) return; Shell.closeMenus(); importData(it.dataset.fxImport); });
  $("fxExportMenu").addEventListener("click", (e) => { const it = e.target.closest("[data-fx-export]"); if (!it) return; Shell.closeMenus(); exportData(it.dataset.fxExport); });
  $("fxResetMenu").addEventListener("click", (e) => { const it = e.target.closest("[data-fx-reset]"); if (!it) return; Shell.closeMenus(); resetValues(it.dataset.fxReset); });
  // Menu mở từ nút riêng: shell chỉ miễn đóng cho nút của nó → tự miễn cho nút form.
  document.addEventListener("mousedown", (e) => {
    if (e.target.closest && e.target.closest("#fxImport,#fxExport,#fxReset")) e.stopPropagation();
  }, true);

  // Rời tab Biểu mẫu: tắt công cụ vẽ + chế độ sửa (thay đổi chờ lưu vẫn giữ).
  new MutationObserver(() => {
    if (!state.formMode) { if (FX.arm) setArm(null, true); if (FX.design) setDesign(false); }
  }).observe($("formModeBtn"), { attributes: true, attributeFilter: ["class"] });

  // Ctrl+S / nút Lưu nhanh: có thay đổi biểu mẫu chờ lưu → lưu biểu mẫu.
  const formSaveApplies = () => FX.dirtyCount() > 0 && !state.editMode && !state.organizeMode && !modalOpen();
  window.addEventListener("keydown", (e) => {
    if (e.ctrlKey && !e.altKey && !e.shiftKey && (e.key === "s" || e.key === "S") && formSaveApplies()) {
      e.preventDefault();
      e.stopImmediatePropagation();
      save();
      return;
    }
    if (modalOpen()) return;
    if (e.key === "Escape" && (FX.arm || (FX.design && FX.sel))) {
      e.stopImmediatePropagation();
      if (FX.arm) setArm(null); else select(null);
      return;
    }
    if (!FX.design || !FX.sel || typingTarget(e.target)) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      e.stopImmediatePropagation();
      deleteItem(FX.sel);
    } else if (e.key.startsWith("Arrow")) {
      e.preventDefault();
      e.stopImmediatePropagation();
      const d = e.shiftKey ? 10 : 1;
      const r = itemRect(FX.sel).slice();
      const dx = e.key === "ArrowLeft" ? -d : e.key === "ArrowRight" ? d : 0;
      const dy = e.key === "ArrowUp" ? d : e.key === "ArrowDown" ? -d : 0;
      setItemRect(FX.sel, [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]);
      layoutPage(itemPage(FX.sel));
    } else if (e.key === "Enter") {
      e.preventDefault();
      openProps(FX.sel);
    }
  }, true);
  $("quickSave").addEventListener("click", (e) => {
    if (!formSaveApplies()) return;
    e.stopImmediatePropagation();
    save();
  }, true);

  if (window.Shell && Shell.onLangChange) Shell.onLangChange(() => {
    FX.gen++;
    rebuildAll();
    if (FX.arm) $("formHint").textContent = t("fx.armHint", { kind: kindLabel(FX.arm) });
    else if (FX.design) $("formHint").textContent = t("fx.designHint");
  });

  updateDirty();
  if (state.path) onDocChanged();
})();
