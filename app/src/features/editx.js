// Edit Object chuẩn Foxit (tab Sửa): thêm hình (chữ nhật / bo góc / elip /
// đường thẳng / mũi tên), xoay – lật – cắt – trích ảnh, định dạng đối tượng
// (viền, nền, kiểu nét, độ dày, độ đục, góc xoay), căn lề đoạn văn, sắp lớp,
// căn chỉnh / phân bố nhiều đối tượng, chọn nhiều (Shift+bấm, kéo quét), phím
// tắt (mũi tên dịch 1pt / Shift 10pt, Ctrl+C/X/V/D/A).
//
// Mọi thao tác đi qua stageEditOps của main.js (materialize ra file tạm →
// undo/redo, lưu như cũ). Engine: EditOp Rotate/Flip/CropImage/SetOpacity/
// SetPathStyle/AddShape/Duplicate/Arrange (crates/ff-engine/src/editobj.rs).
// Móc vào main.js qua window.editx: decorateOverlay / isSelected /
// beforeSelect / afterSelect / dragGroup / stageKeep / addTextStyle.
(() => {
  const X = {
    sel: new Map(), // index đại diện của khung → các run (chọn nhiều)
    internal: false, // đang tự gọi selectEditObject (bỏ qua beforeSelect)
    lastToggle: null,
    addTextStyle: { bold: false, italic: false },
    shapeKind: "rect",
    shapeStyle: { stroke: [0, 0, 0], fill: null, width: 1, opacity: 1, dash: "solid" },
    cropTarget: null,
    clip: null,
    nudgeState: null,
    queue: Promise.resolve(),
    suppressClick: false,
    menuWasOpen: false,
  };
  window.editx = X;

  const SHAPES = [
    ["rect", "shape-rect", "editx.shapeRect"],
    ["roundRect", "shape-roundrect", "editx.shapeRoundRect"],
    ["ellipse", "shape-ellipse", "editx.shapeEllipse"],
    ["line", "shape-line", "editx.shapeLine"],
    ["arrow", "shape-arrow", "editx.shapeArrow"],
  ];

  // ---------- Tiện ích ----------
  const objOf = (i) => state.editObjects.find((o) => o.index === i);
  const ov = () => $("editOverlay");
  const hint = (msg) => { $("editHint").textContent = msg; };
  const say = (msg) => { hint(msg); $("status").textContent = msg; };
  const shiftRect = (r, dx, dy) => ({ left: r.left + dx, right: r.right + dx, bottom: r.bottom + dy, top: r.top + dy });
  const center = (r) => [(r.left + r.right) / 2, (r.bottom + r.top) / 2];
  const normAngle = (a) => {
    let v = ((a % 360) + 540) % 360 - 180;
    if (Math.abs(v + 180) < 0.01) v = 180;
    return v;
  };
  function unionRect(objs) {
    const r = { left: Infinity, bottom: Infinity, right: -Infinity, top: -Infinity };
    for (const o of objs) {
      r.left = Math.min(r.left, o.rect.left); r.bottom = Math.min(r.bottom, o.rect.bottom);
      r.right = Math.max(r.right, o.rect.right); r.top = Math.max(r.top, o.rect.top);
    }
    return r;
  }
  const boxKind = (b) => (b.className.match(/kind-(\w+)/) || [])[1] || "?";
  const boxRuns = (b) => {
    try { if (b.dataset.runs) return JSON.parse(b.dataset.runs); } catch (_) {}
    return [Number(b.dataset.index)];
  };
  const boxFor = (rep) => ov().querySelector(`.edit-box[data-index="${rep}"]`);

  // Các mục đang chọn: { rep, runs, o (object đại diện), kind, rect, nested }.
  function items() {
    let list;
    if (X.sel.size) list = [...X.sel.entries()].map(([rep, runs]) => ({ rep, runs }));
    else if (state.editSel != null) {
      list = [{ rep: state.editSel, runs: state.editSelRuns && state.editSelRuns.length ? state.editSelRuns : [state.editSel] }];
    } else return [];
    return list
      .map((it) => {
        const o = objOf(it.rep);
        const objs = it.runs.map(objOf).filter(Boolean);
        return { ...it, o, kind: o ? o.kind : "?", rect: unionRect(objs.length ? objs : o ? [o] : []), nested: objs.some((x) => x.nested) };
      })
      .filter((it) => it.o);
  }
  const usable = () => items().filter((i) => !i.nested);

  // Pdf point tại sự kiện chuột (so với ảnh trang đang sửa).
  function pdfAt(e) {
    const r = $("editImg").getBoundingClientRect();
    const p = state.pages[state.editPage];
    return {
      cx: e.clientX - r.left, cy: e.clientY - r.top,
      x: (e.clientX - r.left) / state.editScale,
      y: p.heightPt - (e.clientY - r.top) / state.editScale,
    };
  }

  // ---------- Áp op (tuần tự) + chọn lại theo hình học ----------
  // Mỗi lô op = 1 bước undo; xong thì chọn lại đúng các đối tượng vừa thao tác
  // (khớp theo loại + khung dự kiến) — như Foxit, thao tác xong vẫn giữ chọn.
  function stage(ops, expected) {
    if (!ops.length) return X.queue;
    X.queue = X.queue.then(async () => {
      await stageEditOps(ops);
      if (expected && expected.length) reselect(expected);
    }).catch(() => {});
    return X.queue;
  }
  function reselect(expected) {
    const boxes = [...ov().querySelectorAll(".edit-box")];
    const found = new Map();
    for (const ex of expected) {
      let best = null, bestD = Infinity;
      for (const b of boxes) {
        if (boxKind(b) !== ex.kind || found.has(Number(b.dataset.index))) continue;
        const objs = boxRuns(b).map(objOf).filter(Boolean);
        if (!objs.length) continue;
        const r = unionRect(objs);
        const d = Math.abs(r.left - ex.rect.left) + Math.abs(r.right - ex.rect.right) +
          Math.abs(r.top - ex.rect.top) + Math.abs(r.bottom - ex.rect.bottom);
        if (d < bestD) { bestD = d; best = b; }
      }
      if (best && bestD < (ex.tol || 6)) found.set(Number(best.dataset.index), boxRuns(best));
    }
    if (!found.size) return;
    X.sel = found;
    applySel();
  }
  const keepSame = (its) => its.map((it) => ({ kind: it.kind, rect: it.rect }));

  // Đồng bộ lựa chọn nhiều → state của main.js (editSel = mục chính, editSelRuns = hợp mọi run).
  function applySel() {
    const entries = [...X.sel.entries()];
    X.internal = true;
    try {
      if (!entries.length) selectEditObject(-1);
      else selectEditObject(entries[entries.length - 1][0], [...new Set(entries.flatMap(([, r]) => r))]);
    } finally {
      X.internal = false;
    }
    if (entries.length > 1) hint(t("editx.multi", { n: entries.length }));
  }

  // ---------- Móc cho main.js ----------
  X.isSelected = (i) => X.sel.size > 1 && X.sel.has(i);

  X.beforeSelect = (index, runs) => {
    if (X.internal) return false;
    if (index == null || index < 0) { X.sel.clear(); return false; }
    const ev = window.event;
    const type = ev && ev.type;
    runs = runs && runs.length ? runs.slice() : [index];
    if (ev && ev.shiftKey && (type === "mousedown" || type === "click")) {
      // mousedown + click của CÙNG 1 lần bấm chỉ đảo trạng thái 1 lần.
      const now = Date.now();
      if (X.lastToggle && X.lastToggle.index === index && now - X.lastToggle.t < 700) return true;
      X.lastToggle = { index, t: now };
      if (!X.sel.size && state.editSel != null) {
        X.sel.set(state.editSel, state.editSelRuns && state.editSelRuns.length ? state.editSelRuns.slice() : [state.editSel]);
      }
      if (X.sel.has(index)) X.sel.delete(index); else X.sel.set(index, runs);
      applySel();
      return true;
    }
    // Bấm (mousedown) vào 1 khung TRONG nhóm đang chọn: giữ nhóm để kéo cả nhóm.
    if (X.sel.size > 1 && X.sel.has(index) && type === "mousedown") return true;
    X.sel.clear();
    X.sel.set(index, runs);
    return false;
  };

  X.afterSelect = () => X.refreshRibbon();

  X.dragGroup = (box) => {
    const rep = Number(box.dataset.index);
    if (!(X.sel.size > 1 && X.sel.has(rep))) return null;
    const its = items();
    if (its.some((i) => i.nested)) return null;
    return {
      boxes: its.map((i) => boxFor(i.rep)).filter(Boolean),
      runs: [...new Set(its.flatMap((i) => i.runs))],
    };
  };

  X.stageKeep = (ops, dx, dy) => {
    stage(ops, items().map((it) => ({ kind: it.kind, rect: shiftRect(it.rect, dx, dy) })));
  };

  // Khung chọn cho path (hình vẽ / đường kẻ) — chèn ĐẦU overlay để nằm dưới
  // khung chữ/ảnh (không che thao tác chữ). Nền phủ gần cả trang bị bỏ qua.
  X.decorateOverlay = (el) => {
    if (state.editSel == null) X.sel.clear();
    const p = state.pages[state.editPage];
    const pageArea = p.widthPt * p.heightPt;
    const frag = document.createDocumentFragment();
    let n = 0;
    for (const o of state.editObjects) {
      if (o.kind !== "path" || o.nested) continue;
      const w = o.rect.right - o.rect.left, h = o.rect.top - o.rect.bottom;
      if (w < 0.3 && h < 0.3) continue;
      if (w * h > pageArea * 0.6) continue;
      if (++n > 3000) break;
      const box = document.createElement("div");
      box.className = "edit-box kind-path";
      box.dataset.index = String(o.index);
      box.dataset.runs = JSON.stringify([o.index]);
      const st = editBoxStyle(o.rect);
      // Đường mảnh (kẻ bảng, gạch chân): nới khung tối thiểu 7px để bấm trúng.
      let L = parseFloat(st.left), T = parseFloat(st.top), W = parseFloat(st.width), H = parseFloat(st.height);
      if (W < 7) { L -= (7 - W) / 2; W = 7; }
      if (H < 7) { T -= (7 - H) / 2; H = 7; }
      Object.assign(box.style, { left: L + "px", top: T + "px", width: W + "px", height: H + "px" });
      box.title = t("editx.kindPath");
      box.addEventListener("mousedown", (e) => onEditBoxMouseDown(e, o, [o.index]));
      box.addEventListener("click", (e) => {
        if (state.editArm) return;
        e.stopPropagation();
        selectEditObject(o.index, [o.index]);
      });
      frag.appendChild(box);
    }
    el.prepend(frag);
    X.refreshRibbon(); // overlay dựng lại (undo/redo/zoom) → nút theo lựa chọn hiện tại
  };

  // ---------- Ribbon: bật/tắt + giá trị theo lựa chọn ----------
  const dis = (id, off) => { const b = $(id); if (b) b.disabled = !!off; };
  function swatch(id, rgb) {
    const el = $(id);
    el.classList.toggle("ex-none", !rgb);
    el.style.background = rgb ? rgbCss(rgb) : "";
  }
  const round2 = (v) => Math.round(v * 100) / 100;

  X.refreshRibbon = () => {
    if (!state.editMode) return;
    const its = items();
    const ok = its.filter((i) => !i.nested);
    const one = its.length === 1 ? its[0] : null;
    const shapeArmed = state.editArm === "shape";
    const transformable = ok.length > 0 && ok.length === its.length &&
      ok.every((i) => ["image", "path", "text", "form"].includes(i.kind));
    for (const id of ["edRotL", "edRotR", "edFlipH", "edFlipV"]) dis(id, !transformable);
    dis("edCrop", !(one && one.kind === "image" && !one.nested));
    dis("edExtractImage", !(one && one.kind === "image"));
    const paths = ok.filter((i) => i.kind === "path");
    const styleOn = paths.length > 0 || (shapeArmed && !its.length);
    for (const id of ["edStroke", "edFill", "edDash", "edWidth"]) dis(id, !styleOn);
    dis("edOpacity", !(ok.length || shapeArmed));
    dis("edAngle", !(one && transformable));
    for (const id of ["edAlignL", "edAlignC", "edAlignR"]) dis(id, !(one && one.kind === "text" && !one.nested));
    dis("edArrange", !ok.length);
    dis("edAlignObj", ok.length < 2);
    $("edAddShape").classList.toggle("armed", shapeArmed);
    $("edCrop").classList.toggle("armed", state.editArm === "crop");

    // Giá trị hiện tại (path đầu tiên đang chọn, hoặc kiểu cho hình sắp vẽ).
    const src = paths[0] && paths[0].o;
    const st = src
      ? {
        stroke: src.strokeColor ? src.strokeColor.slice(0, 3) : null,
        fill: src.fillColor ? src.fillColor.slice(0, 3) : null,
        width: src.strokeWidth != null ? src.strokeWidth : 0,
        dash: "solid",
      }
      : X.shapeStyle;
    swatch("edStrokeSw", styleOn ? st.stroke : null);
    swatch("edFillSw", styleOn ? st.fill : null);
    $("edWidth").value = styleOn ? round2(st.width) : "";
    if (!src) $("edDash").value = X.shapeStyle.dash;
    const opSrc = one ? one.o : ok[0] ? ok[0].o : null;
    // Ảnh trên trang quá nhiều ảnh: engine không đo độ đục (null) → để trống.
    $("edOpacity").value = opSrc
      ? (opSrc.opacity != null ? Math.round(opSrc.opacity * 100) : "")
      : shapeArmed ? Math.round(X.shapeStyle.opacity * 100) : "";
    $("edAngle").value = one && transformable ? Math.round(normAngle(one.o.rotation || 0)) : "";

    // Thêm chữ đang "armed" + chưa chọn gì: cho chọn phông/cỡ/màu/B/I TRƯỚC khi đặt.
    if (state.editArm === "text" && !its.length) {
      for (const id of ["edFontFamily", "edFontSize", "edBold", "edItalic", "edColorBtn"]) dis(id, false);
      if (!$("edFontSize").value) $("edFontSize").value = 14;
      $("edBold").classList.toggle("on", X.addTextStyle.bold);
      $("edItalic").classList.toggle("on", X.addTextStyle.italic);
    }
  };

  // ---------- Menu thả xuống (Thêm hình / Sắp xếp / Căn chỉnh) ----------
  function menuEl() {
    let m = document.getElementById("editxMenu");
    if (!m) {
      m = document.createElement("div");
      m.id = "editxMenu";
      m.className = "menu mini-menu hidden";
      m.setAttribute("role", "menu");
      document.body.appendChild(m);
    }
    return m;
  }
  function bindMenu(btnId, build) {
    const btn = $(btnId);
    // Bấm lại nút khi menu của chính nó đang mở = đóng (shell đóng ở mousedown).
    btn.addEventListener("mousedown", () => {
      const m = menuEl();
      X.menuWasOpen = !m.classList.contains("hidden") && m.dataset.anchor === btnId;
    });
    btn.addEventListener("click", () => {
      const m = menuEl();
      if (X.menuWasOpen) { X.menuWasOpen = false; m.classList.add("hidden"); return; }
      m.innerHTML = "";
      for (const en of build()) {
        if (en === "-") { const sp = document.createElement("div"); sp.className = "msep"; m.appendChild(sp); continue; }
        const b = document.createElement("button");
        b.className = "mitem" + (en.disabled ? " disabled" : "");
        b.innerHTML = `<i data-icon="${en.icon}"></i><span></span>`;
        b.querySelector("span").textContent = en.label;
        b.addEventListener("click", () => { m.classList.add("hidden"); en.run(); });
        m.appendChild(b);
      }
      applyIcons(m);
      const r = btn.getBoundingClientRect();
      m.style.right = "";
      m.style.left = Math.min(r.left, window.innerWidth - 240) + "px";
      m.style.top = r.bottom + 4 + "px";
      m.dataset.anchor = btnId;
      m.classList.remove("hidden");
    });
  }

  // ---------- Thêm hình (kéo để vẽ) ----------
  function armShape(kind) {
    finishInline();
    X.sel.clear();
    selectEditObject(-1);
    state.editArm = "shape";
    X.shapeKind = kind;
    $("edAddText").classList.remove("armed");
    $("edAddImage").classList.remove("armed");
    ov().classList.add("armed");
    const label = t(SHAPES.find((s) => s[0] === kind)[2]);
    hint(t("editx.shapeHint", { shape: label.toLowerCase() }));
    X.refreshRibbon();
  }
  function armCrop() {
    const one = items()[0];
    if (!one || one.kind !== "image") return;
    if (one.nested) { hint(t("editx.nestedSkip")); return; }
    state.editArm = "crop";
    X.cropTarget = { index: one.rep, rect: one.rect };
    ov().classList.add("armed");
    hint(t("editx.cropHint"));
    X.refreshRibbon();
  }
  function disarm() {
    state.editArm = null;
    X.cropTarget = null;
    ov().classList.remove("armed");
    hint("");
    X.refreshRibbon();
  }
  function finishInline() {
    const ce = ov().querySelector(".edit-inline");
    if (ce) ce.blur();
  }
  function dashArray(kind, w) {
    const k = Math.max(0.5, w || 1);
    if (kind === "dash") return [Math.max(3, k * 3), Math.max(2, k * 2)];
    if (kind === "dot") return [Math.max(1, k), Math.max(2, k * 2)];
    return [];
  }

  const SVGNS = "http://www.w3.org/2000/svg";
  function startDraw(e) {
    const mode = state.editArm; // "shape" | "crop"
    const kind = mode === "crop" ? "rect" : X.shapeKind;
    const a = pdfAt(e);
    const svg = document.createElementNS(SVGNS, "svg");
    svg.setAttribute("class", "ex-draw");
    const img = $("editImg");
    svg.setAttribute("width", img.clientWidth);
    svg.setAttribute("height", img.clientHeight);
    const isLine = kind === "line" || kind === "arrow";
    const el = document.createElementNS(SVGNS, isLine ? "line" : kind === "ellipse" ? "ellipse" : "rect");
    const st = X.shapeStyle;
    const s = state.editScale;
    const strokeCss = mode === "crop" ? "var(--accent)" : st.stroke ? rgbCss(st.stroke) : isLine ? "#000" : "none";
    el.setAttribute("stroke", strokeCss);
    el.setAttribute("stroke-width", mode === "crop" ? 1.5 : Math.max(1, (st.width || 1) * s));
    el.setAttribute("fill", mode === "crop" ? "rgba(0,0,0,.08)" : !isLine && st.fill ? rgbCss(st.fill) : "none");
    el.setAttribute("opacity", mode === "crop" ? 1 : st.opacity);
    if (mode === "crop" || (st.dash && st.dash !== "solid")) {
      el.setAttribute("stroke-dasharray", mode === "crop" ? "5 3" : dashArray(st.dash, st.width).map((v) => v * s).join(" "));
    }
    svg.appendChild(el);
    ov().appendChild(svg);
    let b = a;
    const geom = (ev) => {
      let bx = ev.cx, by = ev.cy;
      if (window.event && window.event.shiftKey) {
        const dx = bx - a.cx, dy = by - a.cy;
        if (isLine) {
          const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
          const len = Math.hypot(dx, dy);
          bx = a.cx + Math.cos(ang) * len; by = a.cy + Math.sin(ang) * len;
        } else {
          const m = Math.max(Math.abs(dx), Math.abs(dy));
          bx = a.cx + Math.sign(dx || 1) * m; by = a.cy + Math.sign(dy || 1) * m;
        }
      }
      return { bx, by };
    };
    const draw = (bx, by) => {
      if (isLine) {
        el.setAttribute("x1", a.cx); el.setAttribute("y1", a.cy); el.setAttribute("x2", bx); el.setAttribute("y2", by);
      } else if (kind === "ellipse") {
        el.setAttribute("cx", (a.cx + bx) / 2); el.setAttribute("cy", (a.cy + by) / 2);
        el.setAttribute("rx", Math.abs(bx - a.cx) / 2); el.setAttribute("ry", Math.abs(by - a.cy) / 2);
      } else {
        el.setAttribute("x", Math.min(a.cx, bx)); el.setAttribute("y", Math.min(a.cy, by));
        el.setAttribute("width", Math.abs(bx - a.cx)); el.setAttribute("height", Math.abs(by - a.cy));
        if (kind === "roundRect") {
          const rr = Math.min(Math.abs(bx - a.cx), Math.abs(by - a.cy)) * 0.18;
          el.setAttribute("rx", rr); el.setAttribute("ry", rr);
        }
      }
    };
    const onMove = (ev) => {
      const p = pdfAt(ev);
      const g = geom(p);
      b = { cx: g.bx, cy: g.by };
      draw(g.bx, g.by);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      svg.remove();
      X.suppressClick = true;
      setTimeout(() => { X.suppressClick = false; }, 0);
      const ph = state.pages[state.editPage].heightPt;
      const toPdf = (cx, cy) => [cx / s, ph - cy / s];
      let [x0, y0] = toPdf(a.cx, a.cy);
      let [x1, y1] = toPdf(b.cx, b.cy);
      const tiny = Math.hypot(b.cx - a.cx, b.cy - a.cy) < 4;
      if (mode === "crop") {
        const tgt = X.cropTarget;
        disarm();
        if (tiny || !tgt) return;
        const keep = {
          left: Math.max(Math.min(x0, x1), tgt.rect.left), right: Math.min(Math.max(x0, x1), tgt.rect.right),
          bottom: Math.max(Math.min(y0, y1), tgt.rect.bottom), top: Math.min(Math.max(y0, y1), tgt.rect.top),
        };
        if (keep.right - keep.left < 1 || keep.top - keep.bottom < 1) { hint(t("editx.cropOutside")); return; }
        stage([{ op: "cropImage", index: tgt.index, keep: [keep.left, keep.bottom, keep.right, keep.top] }], [{ kind: "image", rect: keep, tol: 8 }]);
        return;
      }
      // Bấm không kéo: hình cỡ mặc định (như Foxit) tại điểm bấm.
      if (tiny) {
        if (isLine) { x1 = x0 + 120; y1 = y0; } else { x1 = x0 + 100; y1 = y0 - 60; }
      }
      disarm();
      const w = st.width || 0;
      const op = {
        op: "addShape", shape: kind, x0, y0, x1, y1,
        stroke: st.stroke, noStroke: !st.stroke, fill: st.fill, noFill: !st.fill,
        width: isLine && !w ? 1 : w, dash: dashArray(st.dash, st.width), opacity: st.opacity,
      };
      const r = { left: Math.min(x0, x1), right: Math.max(x0, x1), bottom: Math.min(y0, y1), top: Math.max(y0, y1) };
      stage([op], [{ kind: "path", rect: r, tol: 4 * Math.max(2, w) + 12 }]);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // ---------- Quét chọn (kéo trên vùng trống) ----------
  function startMarquee(e) {
    const a = pdfAt(e);
    const additive = e.shiftKey;
    let div = null;
    let b = a;
    const onMove = (ev) => {
      b = pdfAt(ev);
      if (!div && Math.hypot(b.cx - a.cx, b.cy - a.cy) < 5) return;
      if (!div) { div = document.createElement("div"); div.className = "ex-marquee"; ov().appendChild(div); }
      Object.assign(div.style, {
        left: Math.min(a.cx, b.cx) + "px", top: Math.min(a.cy, b.cy) + "px",
        width: Math.abs(b.cx - a.cx) + "px", height: Math.abs(b.cy - a.cy) + "px",
      });
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (!div) return; // chỉ là bấm → main.js bỏ chọn như cũ
      div.remove();
      X.suppressClick = true;
      setTimeout(() => { X.suppressClick = false; }, 0);
      const L = Math.min(a.cx, b.cx), R = Math.max(a.cx, b.cx), T = Math.min(a.cy, b.cy), B = Math.max(a.cy, b.cy);
      const found = additive ? new Map(items().map((i) => [i.rep, i.runs])) : new Map();
      for (const box of ov().querySelectorAll(".edit-box")) {
        const x = parseFloat(box.style.left) + parseFloat(box.style.width) / 2;
        const y = parseFloat(box.style.top) + parseFloat(box.style.height) / 2;
        if (x >= L && x <= R && y >= T && y <= B) found.set(Number(box.dataset.index), boxRuns(box));
      }
      X.sel = found;
      applySel();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // Capture ở #editStage: chạy TRƯỚC các handler của overlay/khung trong main.js.
  $("editStage").addEventListener("mousedown", (e) => {
    if (!state.editMode || e.button !== 0) return;
    const o = ov();
    if (!o.contains(e.target)) return;
    if (e.target.closest && e.target.closest(".edit-inline")) return;
    if (state.editArm === "shape" || state.editArm === "crop") {
      e.preventDefault();
      e.stopPropagation();
      startDraw(e);
      return;
    }
    if (!state.editArm && e.target === o && !o.querySelector(".edit-inline")) startMarquee(e);
  }, true);
  $("editStage").addEventListener("click", (e) => {
    if (!X.suppressClick) return;
    X.suppressClick = false;
    e.stopPropagation();
    e.preventDefault();
  }, true);

  // ---------- Xoay / lật / cắt / trích ----------
  function rotRect(r, deg) {
    const [cx, cy] = center(r);
    const w = r.right - r.left, h = r.top - r.bottom;
    const a = (deg * Math.PI) / 180;
    const W = Math.abs(w * Math.cos(a)) + Math.abs(h * Math.sin(a));
    const H = Math.abs(w * Math.sin(a)) + Math.abs(h * Math.cos(a));
    return { left: cx - W / 2, right: cx + W / 2, bottom: cy - H / 2, top: cy + H / 2 };
  }
  function rotateSel(deg, only) {
    const its = only || usable();
    if (!its.length || !deg) return;
    const ops = [];
    for (const it of its) {
      const [cx, cy] = center(it.rect);
      for (const i of it.runs) ops.push({ op: "rotate", index: i, degrees: deg, cx, cy });
    }
    stage(ops, its.map((it) => ({ kind: it.kind, rect: rotRect(it.rect, deg), tol: 14 })));
  }
  function flipSel(horizontal) {
    const its = usable();
    if (!its.length) return;
    const ops = [];
    for (const it of its) {
      const [cx, cy] = center(it.rect);
      for (const i of it.runs) ops.push({ op: "flip", index: i, horizontal, cx, cy });
    }
    stage(ops, keepSame(its).map((e) => ({ ...e, tol: 10 })));
  }
  async function extractImage() {
    const one = items()[0];
    if (!one || one.kind !== "image") return;
    const out = await invoke("pick_save_as", { ext: "png", name: "image.png" });
    if (!out) return;
    try {
      const [w, h] = await invoke("editx_extract_image", {
        path: state.editBase, page: state.editPage, index: one.rep, output: out, password: null,
      });
      say(t("editx.extracted", { w, h, file: shortName(out) }));
    } catch (e) {
      say(t("editx.errExtract", { e }));
    }
  }

  // ---------- Định dạng: viền / nền / nét / độ dày / độ đục / góc ----------
  function applyStyle(part) {
    const paths = usable().filter((i) => i.kind === "path");
    if (!paths.length) {
      if (state.editArm === "shape") {
        if ("stroke" in part) X.shapeStyle.stroke = part.stroke;
        if (part.noStroke) X.shapeStyle.stroke = null;
        if ("fill" in part) X.shapeStyle.fill = part.fill;
        if (part.noFill) X.shapeStyle.fill = null;
        if ("width" in part) X.shapeStyle.width = part.width;
        if ("dashKind" in part) X.shapeStyle.dash = part.dashKind;
        X.refreshRibbon();
      }
      return;
    }
    const ops = paths.flatMap((it) => it.runs.map((i) => {
      const o = objOf(i);
      const op = { op: "setPathStyle", index: i };
      if (part.stroke) op.stroke = part.stroke;
      if (part.noStroke) op.noStroke = true;
      if (part.fill) op.fill = part.fill;
      if (part.noFill) op.noFill = true;
      if ("width" in part) op.width = part.width;
      if ("dashKind" in part) op.dash = dashArray(part.dashKind, (o && o.strokeWidth) || 1);
      return op;
    }));
    stage(ops, keepSame(paths).map((e) => ({ ...e, tol: 12 })));
  }
  function pickColor(btnId, cur, onPick, onNone) {
    openColorPopover($(btnId), cur || [0, 0, 0], onPick);
    const pop = document.querySelector(".colorpop");
    if (!pop) return;
    const none = document.createElement("div");
    none.className = "ex-nocolor";
    none.innerHTML = '<span class="cs ex-none"></span><span></span>';
    none.lastChild.textContent = t("editx.noColor");
    none.addEventListener("click", () => { closeColorPopover(); onNone(); });
    pop.prepend(none);
  }
  const curStyle = () => {
    const p = usable().find((i) => i.kind === "path");
    if (!p) return X.shapeStyle;
    return { stroke: p.o.strokeColor ? p.o.strokeColor.slice(0, 3) : null, fill: p.o.fillColor ? p.o.fillColor.slice(0, 3) : null };
  };

  $("edStroke").addEventListener("click", () =>
    pickColor("edStroke", curStyle().stroke, (rgb) => applyStyle({ stroke: rgb }), () => applyStyle({ noStroke: true })));
  $("edFill").addEventListener("click", () =>
    pickColor("edFill", curStyle().fill, (rgb) => applyStyle({ fill: rgb }), () => applyStyle({ noFill: true })));
  $("edWidth").addEventListener("change", () => {
    const v = Number($("edWidth").value);
    if (v >= 0 && v <= 72) applyStyle({ width: v });
  });
  $("edDash").addEventListener("change", () => applyStyle({ dashKind: $("edDash").value }));
  $("edOpacity").addEventListener("change", () => {
    const v = Math.max(0, Math.min(100, Number($("edOpacity").value)));
    if (!Number.isFinite(v)) return;
    const its = usable();
    if (!its.length) {
      if (state.editArm === "shape") X.shapeStyle.opacity = v / 100;
      return;
    }
    const ops = its.flatMap((it) => it.runs.map((i) => ({ op: "setOpacity", index: i, opacity: v / 100 })));
    stage(ops, keepSame(its).map((e) => ({ ...e, tol: 8 })));
  });
  $("edAngle").addEventListener("change", () => {
    const its = usable();
    if (its.length !== 1) return;
    const target = Number($("edAngle").value);
    if (!Number.isFinite(target)) return;
    const delta = normAngle(target - (its[0].o.rotation || 0));
    if (Math.abs(delta) > 0.05) rotateSel(delta, its);
  });

  // ---------- Căn lề đoạn văn (trái / giữa / phải) ----------
  function alignText(mode) {
    const its = items();
    if (its.length !== 1 || its[0].kind !== "text") return;
    if (its[0].nested) { hint(t("editx.nestedSkip")); return; }
    const lines = paragraphLines(its[0].o);
    const L = Math.min(...lines.map((l) => l.rect.left));
    const R = Math.max(...lines.map((l) => l.rect.right));
    const ops = [];
    let myDx = 0;
    for (const ln of lines) {
      const w = ln.rect.right - ln.rect.left;
      const target = mode === "left" ? L : mode === "center" ? L + (R - L - w) / 2 : R - w;
      const dx = target - ln.rect.left;
      if (ln.runs.some((r) => r.index === its[0].rep)) myDx = dx;
      if (Math.abs(dx) < 0.2) continue;
      for (const r of ln.runs) ops.push({ op: "transform", index: r.index, dx, dy: 0, sx: 1, sy: 1 });
    }
    if (!ops.length) { hint(t("editx.alignNone")); return; }
    stage(ops, [{ kind: "text", rect: shiftRect(its[0].rect, myDx, 0), tol: 8 }]);
  }
  $("edAlignL").addEventListener("click", () => alignText("left"));
  $("edAlignC").addEventListener("click", () => alignText("center"));
  $("edAlignR").addEventListener("click", () => alignText("right"));

  // ---------- Sắp lớp ----------
  function arrange(mode) {
    const its = usable();
    if (!its.length) return;
    stage([{ op: "arrange", indices: [...new Set(its.flatMap((i) => i.runs))], mode }], keepSame(its));
  }

  // ---------- Căn chỉnh / phân bố nhiều đối tượng ----------
  function alignObjs(mode) {
    const its = usable();
    if (its.length < 2) { hint(t("editx.needTwo")); return; }
    if ((mode === "distH" || mode === "distV") && its.length < 3) { hint(t("editx.needThree")); return; }
    const bb = unionRect(its.map((i) => ({ rect: i.rect })));
    const moves = new Map(); // rep → [dx, dy]
    if (mode === "distH" || mode === "distV") {
      const hz = mode === "distH";
      const sorted = its.slice().sort((a, b) => (hz ? center(a.rect)[0] - center(b.rect)[0] : center(b.rect)[1] - center(a.rect)[1]));
      const size = (r) => (hz ? r.right - r.left : r.top - r.bottom);
      const total = sorted.reduce((s, i) => s + size(i.rect), 0);
      const gap = ((hz ? bb.right - bb.left : bb.top - bb.bottom) - total) / (sorted.length - 1);
      let pos = hz ? bb.left : bb.top;
      for (const it of sorted) {
        if (hz) { moves.set(it.rep, [pos - it.rect.left, 0]); pos += size(it.rect) + gap; }
        else { moves.set(it.rep, [0, pos - it.rect.top]); pos -= size(it.rect) + gap; }
      }
    } else {
      const [bcx, bcy] = center(bb);
      for (const it of its) {
        const r = it.rect;
        const [cx, cy] = center(r);
        const d = {
          left: [bb.left - r.left, 0], center: [bcx - cx, 0], right: [bb.right - r.right, 0],
          top: [0, bb.top - r.top], middle: [0, bcy - cy], bottom: [0, bb.bottom - r.bottom],
        }[mode];
        moves.set(it.rep, d);
      }
    }
    const ops = [];
    const expected = [];
    for (const it of its) {
      const [dx, dy] = moves.get(it.rep) || [0, 0];
      expected.push({ kind: it.kind, rect: shiftRect(it.rect, dx, dy) });
      if (Math.abs(dx) < 0.01 && Math.abs(dy) < 0.01) continue;
      for (const i of it.runs) ops.push({ op: "transform", index: i, dx, dy, sx: 1, sy: 1 });
    }
    if (!ops.length) return;
    stage(ops, expected);
  }

  // ---------- Sao chép / dán / nhân bản / chọn tất cả / dịch bằng phím ----------
  const sig = (o) => ({ kind: o.kind, rect: { ...o.rect }, text: o.text || null });
  function matchSig(objs, s) {
    return objs.find((o) =>
      o.kind === s.kind && (o.text || null) === s.text &&
      Math.abs(o.rect.left - s.rect.left) < 0.5 && Math.abs(o.rect.right - s.rect.right) < 0.5 &&
      Math.abs(o.rect.top - s.rect.top) < 0.5 && Math.abs(o.rect.bottom - s.rect.bottom) < 0.5);
  }
  function copySel() {
    const its = usable().filter((i) => i.kind !== "form");
    if (!its.length) return false;
    X.clip = {
      page: state.editPage,
      n: 0,
      items: its.map((it) => ({ kind: it.kind, rect: it.rect, runs: it.runs.map(objOf).filter(Boolean).map(sig) })),
    };
    hint(t("editx.copied", { n: its.length }));
    return true;
  }
  async function paste() {
    const clip = X.clip;
    if (!clip) return;
    const same = clip.page === state.editPage;
    let src = state.editObjects;
    if (!same) {
      try { src = await invoke("edit_list_objects", { path: state.editBase, page: clip.page, password: null }); }
      catch (_) { src = []; }
    }
    clip.n += 1;
    const off = same ? 10 * clip.n : 0; // cùng trang: lệch chéo mỗi lần dán (như Foxit)
    const ops = [];
    const expected = [];
    for (const it of clip.items) {
      const idxs = it.runs.map((r) => matchSig(src, r));
      if (idxs.some((m) => !m)) continue;
      for (const m of idxs) ops.push({ op: "duplicate", index: m.index, srcPage: same ? null : clip.page, ox: off, oy: -off });
      expected.push({ kind: it.kind, rect: shiftRect(it.rect, off, -off) });
    }
    if (!ops.length) { hint(t("editx.pasteNone")); return; }
    stage(ops, expected);
  }
  function duplicateSel() {
    const its = usable().filter((i) => i.kind !== "form");
    if (!its.length) return;
    const ops = its.flatMap((it) => it.runs.map((i) => ({ op: "duplicate", index: i, ox: 10, oy: -10 })));
    stage(ops, its.map((it) => ({ kind: it.kind, rect: shiftRect(it.rect, 10, -10) })));
  }
  function selectAll() {
    const found = new Map();
    for (const box of ov().querySelectorAll(".edit-box")) found.set(Number(box.dataset.index), boxRuns(box));
    if (!found.size) return;
    X.sel = found;
    applySel();
  }
  function nudge(dx, dy) {
    const its = X.nudgeState ? X.nudgeState.items : usable();
    if (!its.length) { if (items().length) hint(t("editx.nestedSkip")); return; }
    const s = state.editScale;
    for (const it of its) {
      const b = boxFor(it.rep);
      if (b) {
        b.style.left = parseFloat(b.style.left) + dx * s + "px";
        b.style.top = parseFloat(b.style.top) - dy * s + "px";
      }
    }
    const n = X.nudgeState || (X.nudgeState = { dx: 0, dy: 0, items: its, timer: 0 });
    n.dx += dx;
    n.dy += dy;
    clearTimeout(n.timer);
    // Gom các lần bấm liên tiếp thành 1 bước (1 lần áp + 1 bước undo).
    n.timer = setTimeout(() => {
      X.nudgeState = null;
      const ops = n.items.flatMap((it) => it.runs.map((i) => ({ op: "transform", index: i, dx: n.dx, dy: n.dy, sx: 1, sy: 1 })));
      stage(ops, n.items.map((it) => ({ kind: it.kind, rect: shiftRect(it.rect, n.dx, n.dy) })));
    }, 400);
  }

  const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
  window.addEventListener("keydown", (e) => {
    if (!state.editMode) return;
    if (!$("modalOverlay").classList.contains("hidden")) return;
    if (e.key === "Escape" && (state.editArm === "shape" || state.editArm === "crop")) {
      e.preventDefault();
      e.stopImmediatePropagation();
      disarm();
      return;
    }
    const typing = e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName);
    if (typing || ov().querySelector(".edit-inline")) return;
    if (ARROWS[e.key] && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (!items().length) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      const k = e.shiftKey ? 10 : 1;
      nudge(ARROWS[e.key][0] * k, ARROWS[e.key][1] * k);
      return;
    }
    if (!e.ctrlKey || e.altKey) return;
    const key = e.key.toLowerCase();
    let handled = false;
    if (key === "c") handled = copySel();
    else if (key === "x") { handled = copySel(); if (handled) deleteSelectedEditObject(); }
    else if (key === "v" && X.clip) { paste(); handled = true; }
    else if (key === "d") { duplicateSel(); handled = true; }
    else if (key === "a") { selectAll(); handled = true; }
    if (handled) { e.preventDefault(); e.stopImmediatePropagation(); }
  }, true);

  // ---------- Gắn nút ribbon ----------
  bindMenu("edAddShape", () => SHAPES.map(([kind, icon, key]) => ({ icon, label: t(key), run: () => armShape(kind) })));
  bindMenu("edArrange", () => [
    { icon: "bring-front", label: t("editx.toFront"), run: () => arrange("front") },
    { icon: "bring-forward", label: t("editx.forward"), run: () => arrange("forward") },
    { icon: "send-backward", label: t("editx.backward"), run: () => arrange("backward") },
    { icon: "send-back", label: t("editx.toBack"), run: () => arrange("back") },
  ]);
  bindMenu("edAlignObj", () => {
    const n = usable().length;
    return [
      { icon: "align-left", label: t("editx.alignLeft"), run: () => alignObjs("left") },
      { icon: "align-center", label: t("editx.alignCenter"), run: () => alignObjs("center") },
      { icon: "align-right", label: t("editx.alignRight"), run: () => alignObjs("right") },
      "-",
      { icon: "align-top", label: t("editx.alignTop"), run: () => alignObjs("top") },
      { icon: "align-middle", label: t("editx.alignMiddle"), run: () => alignObjs("middle") },
      { icon: "align-bottom", label: t("editx.alignBottom"), run: () => alignObjs("bottom") },
      "-",
      { icon: "distribute-h", label: t("editx.distH"), disabled: n < 3, run: () => alignObjs("distH") },
      { icon: "distribute-v", label: t("editx.distV"), disabled: n < 3, run: () => alignObjs("distV") },
    ];
  });
  $("edRotL").addEventListener("click", () => rotateSel(90));
  $("edRotR").addEventListener("click", () => rotateSel(-90));
  $("edFlipH").addEventListener("click", () => flipSel(true));
  $("edFlipV").addEventListener("click", () => flipSel(false));
  $("edCrop").addEventListener("click", () => (state.editArm === "crop" ? disarm() : armCrop()));
  $("edExtractImage").addEventListener("click", extractImage);
  // Thêm chữ: đậm/nghiêng chọn trước khi đặt (main.js đọc editx.addTextStyle).
  for (const [id, key] of [["edBold", "bold"], ["edItalic", "italic"]]) {
    $(id).addEventListener("click", () => {
      if (state.editArm !== "text" || items().length) return;
      X.addTextStyle[key] = !X.addTextStyle[key];
      $(id).classList.toggle("on", X.addTextStyle[key]);
    });
  }
  // Nút "armed" của main.js đổi trạng thái → cập nhật nhóm định dạng.
  for (const id of ["edAddText", "edAddImage"]) $(id).addEventListener("click", () => setTimeout(X.refreshRibbon, 0));
})();
