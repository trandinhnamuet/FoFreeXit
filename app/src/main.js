const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

const PT_PER_PX = 96 / 72; // 1pt = 1.333px ở zoom 100%
const DPR = window.devicePixelRatio || 1;

const state = {
  path: null,
  pages: [],          // [{index,widthPt,heightPt}]
  zoom: 1.0,          // 1.0 = 100%
  slots: [],          // phần tử DOM theo trang
  visible: new Set(), // trang đang trong vùng nhìn
  current: 0,         // trang hiện tại (trên cùng vùng nhìn)
  hits: [],           // kết quả tìm kiếm
  hitIdx: -1,
  textLayers: {},     // cache hộp ký tự theo trang
  tool: null,         // công cụ chú thích đang chọn
  color: [255, 230, 0], // màu chú thích (RGB)
  annotSpecs: [],     // chú thích chưa lưu
  selectedId: null,   // annotation đang chọn
  recentColors: [],   // màu dùng gần đây
  undoStack: [],      // snapshot annotSpecs trước mỗi hành động (Ctrl+Z)
  redoStack: [],
};
const UNDO_LIMIT = 50;
let annotIdSeq = 1;
const PRESET_COLORS = [
  [255, 230, 0], [255, 145, 0], [255, 0, 0], [233, 30, 99],
  [156, 39, 176], [63, 81, 181], [33, 150, 243], [0, 188, 212],
  [0, 150, 136], [76, 175, 80], [0, 0, 0], [120, 120, 120],
];

function displaySize(p) {
  return {
    w: Math.round(p.widthPt * PT_PER_PX * state.zoom),
    h: Math.round(p.heightPt * PT_PER_PX * state.zoom),
  };
}

// ---------- Khởi tạo ----------

function shortName(path) {
  return path.split(/[\\/]/).pop();
}

async function loadDocument(path) {
  try {
    state.path = path;
    state.textLayers = {};
    state.hits = [];
    state.hitIdx = -1;
    state.current = 0;
    state.annotSpecs = [];
    state.undoStack = [];
    state.redoStack = [];
    updateUndoRedoButtons();
    setTool(null);
    updateAnnotCount();
    buildComments();
    $("searchBox").value = "";
    $("searchCount").textContent = "—";
    const meta = await invoke("open_document", { path });
    state.pages = meta.pages;
    buildPages();
    buildThumbnails();
    buildOutline(meta.outline);
    $("status").textContent = `${meta.pageCount} trang · ${shortName(path)}`;
    updatePageTotal();
    updateCurrentPage();
    updateZoomLabel();
  } catch (e) {
    $("status").textContent = "Lỗi mở tài liệu: " + e;
  }
}

async function boot() {
  // Nếu app được mở qua "Open with FoFreeXit" / double-click file PDF trong
  // Explorer, Windows truyền đường dẫn file qua command-line — ưu tiên mở nó.
  const fromExplorer = await invoke("initial_file");
  loadDocument(fromExplorer || (await invoke("default_pdf")));
}

async function openFile() {
  try {
    const path = await invoke("pick_pdf");
    if (path) loadDocument(path);
  } catch (e) {
    $("status").textContent = "Lỗi chọn file: " + e;
  }
}

let pageObserver;

function buildPages() {
  const container = $("pages");
  container.innerHTML = "";
  state.slots = [];
  state.visible.clear();

  for (const p of state.pages) {
    const slot = document.createElement("div");
    slot.className = "page-slot";
    slot.dataset.index = p.index;
    const { w, h } = displaySize(p);
    slot.style.width = w + "px";
    slot.style.height = h + "px";

    const ph = document.createElement("div");
    ph.className = "ph";
    ph.textContent = `Trang ${p.index + 1}`;
    slot.appendChild(ph);

    const overlay = document.createElement("div");
    overlay.className = "overlay";
    slot.appendChild(overlay);

    const annotlayer = document.createElement("div");
    annotlayer.className = "annotlayer";
    slot.appendChild(annotlayer);

    container.appendChild(slot);
    state.slots[p.index] = slot;
  }

  if (pageObserver) pageObserver.disconnect();
  pageObserver = new IntersectionObserver(onIntersect, {
    root: $("viewport"),
    rootMargin: "500px 0px",
    threshold: 0.01,
  });
  for (const slot of state.slots) pageObserver.observe(slot);
  $("viewport").scrollTop = 0;
}

function onIntersect(entries) {
  for (const e of entries) {
    const idx = Number(e.target.dataset.index);
    if (e.isIntersecting) {
      state.visible.add(idx);
      renderSlot(idx);
    } else {
      state.visible.delete(idx);
    }
  }
  updateCurrentPage();
}

function zoomKey() {
  return state.zoom.toFixed(3);
}

async function renderSlot(idx) {
  const slot = state.slots[idx];
  if (!slot) return;
  if (slot.dataset.renderedZoom === zoomKey()) return; // đã render ở zoom này
  slot.dataset.renderedZoom = zoomKey();

  const p = state.pages[idx];
  const renderWidth = Math.round(p.widthPt * PT_PER_PX * state.zoom * DPR);
  try {
    const dataUrl = await invoke("render_page", {
      path: state.path,
      page: idx,
      width: renderWidth,
    });
    let img = slot.querySelector("img");
    if (!img) {
      img = document.createElement("img");
      slot.insertBefore(img, slot.firstChild);
    }
    img.src = dataUrl;
    const ph = slot.querySelector(".ph");
    if (ph) ph.remove();
    buildTextLayer(idx);
    drawAnnotsForPage(idx);
  } catch (e) {
    slot.dataset.renderedZoom = ""; // cho phép thử lại
    $("status").textContent = "Lỗi render trang " + (idx + 1) + ": " + e;
  }
}

// Dựng lớp text trong suốt cho phép chọn & copy text trên ảnh trang.
async function buildTextLayer(idx) {
  const slot = state.slots[idx];
  if (!slot) return;
  let layer = slot.querySelector(".textlayer");
  if (!layer) {
    layer = document.createElement("div");
    layer.className = "textlayer";
    slot.appendChild(layer);
  }
  let boxes = state.textLayers[idx];
  if (!boxes) {
    try {
      boxes = await invoke("page_text_layer", { path: state.path, page: idx });
      state.textLayers[idx] = boxes;
    } catch {
      return;
    }
  }
  const p = state.pages[idx];
  const scale = PT_PER_PX * state.zoom;
  layer.innerHTML = "";
  const frag = document.createDocumentFragment();

  // Gom ký tự thành "từ" (mỗi từ = 1 span) để double-click chọn cả từ,
  // kéo/Shift+Click chọn dải, và copy giữ khoảng trắng (chèn text node " ").
  let word = null;
  const flush = () => {
    if (word && word.text) {
      const w = (word.right - word.left) * scale;
      const h = (word.top - word.bottom) * scale;
      const span = document.createElement("span");
      span.textContent = word.text;
      span.style.left = word.left * scale + "px";
      span.style.top = (p.heightPt - word.top) * scale + "px";
      span.style.width = Math.max(w, 1) + "px";
      span.style.height = h + "px";
      span.style.fontSize = h * 0.82 + "px";
      span.style.lineHeight = h + "px";
      frag.appendChild(span);
    }
    word = null;
  };

  for (const b of boxes) {
    const isWs = !b.ch || /\s/.test(b.ch);
    if (isWs) {
      flush();
      frag.appendChild(document.createTextNode(b.ch && /\n/.test(b.ch) ? "\n" : " "));
      continue;
    }
    const h = b.top - b.bottom;
    if (h <= 0) continue;
    if (word) {
      const sameLine = Math.abs(b.bottom - word.bottom) <= 0.6 * (word.top - word.bottom);
      const gap = b.left - word.right;
      if (!sameLine || gap > 0.3 * (word.top - word.bottom)) {
        flush();
        frag.appendChild(document.createTextNode(" "));
      }
    }
    if (!word) {
      word = { text: b.ch, left: b.left, right: b.right, top: b.top, bottom: b.bottom };
    } else {
      word.text += b.ch;
      word.left = Math.min(word.left, b.left);
      word.right = Math.max(word.right, b.right);
      word.top = Math.max(word.top, b.top);
      word.bottom = Math.min(word.bottom, b.bottom);
    }
  }
  flush();
  layer.appendChild(frag);
}

function updateCurrentPage() {
  const vp = $("viewport");
  const probe = vp.scrollTop + vp.clientHeight * 0.35;
  let cur = 0;
  for (const slot of state.slots) {
    if (!slot) continue;
    if (slot.offsetTop <= probe) cur = Number(slot.dataset.index);
    else break;
  }
  if (cur !== state.current) {
    state.current = cur;
    if (document.activeElement !== $("pageInput")) $("pageInput").value = cur + 1;
    document.querySelectorAll(".thumb").forEach((t) => {
      t.classList.toggle("active", Number(t.dataset.index) === cur);
    });
  }
}

function updatePageTotal() {
  $("pageTotal").textContent = state.pages.length || "—";
  $("pageInput").value = state.pages.length ? state.current + 1 : "";
}

function goToPageInput() {
  const n = parseInt($("pageInput").value, 10);
  if (!Number.isFinite(n)) { $("pageInput").value = state.current + 1; return; }
  const idx = Math.max(0, Math.min(state.pages.length - 1, n - 1));
  goToPage(idx);
  $("pageInput").value = idx + 1;
}

function gotoPrevPage() { goToPage(Math.max(0, state.current - 1)); }
function gotoNextPage() { goToPage(Math.min(state.pages.length - 1, state.current + 1)); }

// ---------- Thumbnails ----------

let thumbObserver;
const THUMB_W = 130;

function buildThumbnails() {
  const box = $("thumbs");
  box.innerHTML = "";
  if (thumbObserver) thumbObserver.disconnect();
  // Lazy: chỉ render thumbnail khi cuộn tới (quan trọng với file 1000+ trang).
  thumbObserver = new IntersectionObserver(onThumbIntersect, {
    root: box,
    rootMargin: "400px 0px",
    threshold: 0.01,
  });
  for (const p of state.pages) {
    const el = document.createElement("div");
    el.className = "thumb";
    el.dataset.index = p.index;
    // Giữ chỗ đúng tỉ lệ để placeholder có chiều cao (observer hoạt động đúng).
    const thumbH = Math.round(THUMB_W * (p.heightPt / p.widthPt));
    el.style.minHeight = thumbH + 18 + "px";
    el.innerHTML = `<img alt="t${p.index}" style="height:${thumbH}px"/><div class="n">${p.index + 1}</div>`;
    el.addEventListener("click", () => goToPage(p.index));
    box.appendChild(el);
    thumbObserver.observe(el);
  }
}

function onThumbIntersect(entries) {
  for (const e of entries) {
    if (!e.isIntersecting) continue;
    const el = e.target;
    if (el.dataset.loaded) continue;
    el.dataset.loaded = "1";
    const idx = Number(el.dataset.index);
    invoke("render_page", { path: state.path, page: idx, width: 150 })
      .then((url) => {
        const img = el.querySelector("img");
        img.src = url;
        img.style.height = ""; // trả lại auto theo tỉ lệ
        el.style.minHeight = "";
      })
      .catch(() => { el.dataset.loaded = ""; });
  }
}

// ---------- Outline ----------

function buildOutline(items) {
  const box = $("outline");
  box.innerHTML = "";
  if (!items.length) {
    box.innerHTML = `<div class="oitem" style="color:#777">Không có outline</div>`;
    return;
  }
  for (const it of items) {
    const el = document.createElement("div");
    el.className = "oitem";
    el.style.paddingLeft = 6 + it.level * 14 + "px";
    el.textContent = it.title || "(không tiêu đề)";
    if (it.pageIndex != null) {
      el.addEventListener("click", () => goToPage(it.pageIndex));
    } else {
      el.style.color = "#777";
    }
    box.appendChild(el);
  }
}

// ---------- Điều hướng ----------

function goToPage(idx) {
  const slot = state.slots[idx];
  if (slot) slot.scrollIntoView({ behavior: "smooth", block: "start" });
}

// ---------- Zoom ----------

function setZoom(z) {
  finishEditing();
  closeNotePopup();
  closeColorPopover();
  state.zoom = Math.max(0.25, Math.min(5, z));
  // Phản hồi tức thì & mượt: chỉ đổi kích thước slot → ảnh sẵn có co giãn theo.
  for (const p of state.pages) {
    const slot = state.slots[p.index];
    if (!slot) continue;
    const { w, h } = displaySize(p);
    slot.style.width = w + "px";
    slot.style.height = h + "px";
  }
  // Overlay/annotation rẻ → vẽ lại ngay để bám theo.
  drawHighlightsForVisible();
  for (const idx of state.visible) drawAnnotsForPage(idx);
  updateZoomLabel();
  // Render NÉT bằng PDFium chỉ sau khi ngừng zoom (tránh flood gây lag/crash).
  scheduleSharpRerender();
}

let sharpTimer = null;
function scheduleSharpRerender() {
  clearTimeout(sharpTimer);
  sharpTimer = setTimeout(() => {
    for (const idx of state.visible) {
      const slot = state.slots[idx];
      if (slot) {
        slot.dataset.renderedZoom = ""; // buộc render lại ở zoom mới
        clearOverlay(slot);
      }
      renderSlot(idx);
    }
    drawHighlightsForVisible();
    for (const idx of state.visible) drawAnnotsForPage(idx);
  }, 180);
}

const ZOOM_PRESETS = [50, 75, 100, 125, 150, 200, 300, 400];
function buildZoomSelect() {
  const sel = $("zoomSelect");
  sel.innerHTML = "";
  for (const pct of ZOOM_PRESETS) {
    const o = document.createElement("option");
    o.value = pct; o.textContent = pct + "%";
    sel.appendChild(o);
  }
}
function updateZoomLabel() {
  const sel = $("zoomSelect");
  const pct = Math.round(state.zoom * 100);
  let custom = sel.querySelector('option[data-custom="1"]');
  const matchesPreset = ZOOM_PRESETS.includes(pct);
  if (matchesPreset) {
    if (custom) custom.remove();
    sel.value = String(pct);
  } else {
    if (!custom) {
      custom = document.createElement("option");
      custom.dataset.custom = "1";
      sel.appendChild(custom);
    }
    custom.value = String(pct);
    custom.textContent = pct + "%";
    sel.value = String(pct);
  }
}

// Zoom giữ nguyên điểm dưới con trỏ chuột (Ctrl + lăn chuột).
function zoomAtPoint(newZoom, clientX, clientY) {
  const vp = $("viewport");
  const rect = vp.getBoundingClientRect();
  const px = clientX - rect.left;
  const py = clientY - rect.top;
  const cx = vp.scrollLeft + px;
  const cy = vp.scrollTop + py;
  const old = state.zoom;
  setZoom(newZoom);
  const f = state.zoom / old;
  vp.scrollLeft = cx * f - px;
  vp.scrollTop = cy * f - py;
}

function fitWidth() {
  if (!state.pages.length) return;
  const avail = $("viewport").clientWidth - 48;
  const p = state.pages[state.current] || state.pages[0];
  setZoom(avail / (p.widthPt * PT_PER_PX));
}

function fitPage() {
  if (!state.pages.length) return;
  const vp = $("viewport");
  const availW = vp.clientWidth - 48;
  const availH = vp.clientHeight - 48;
  const p = state.pages[state.current] || state.pages[0];
  const zw = availW / (p.widthPt * PT_PER_PX);
  const zh = availH / (p.heightPt * PT_PER_PX);
  setZoom(Math.min(zw, zh));
}

// ---------- Tìm kiếm ----------

let searchTimer;
async function runSearch() {
  const q = $("searchBox").value.trim();
  clearAllHighlights();
  state.hits = [];
  state.hitIdx = -1;
  if (!q) { $("searchCount").textContent = "—"; return; }
  try {
    const hits = await invoke("search_document", {
      path: state.path,
      query: q,
      caseSensitive: $("searchCase").checked,
    });
    state.hits = hits;
    $("searchCount").textContent = hits.length ? `0/${hits.length}` : "0";
    if (hits.length) gotoHit(0);
    drawHighlightsForVisible();
  } catch (e) {
    $("searchCount").textContent = "lỗi";
    $("status").textContent = "Lỗi tìm kiếm: " + e;
  }
}

function gotoHit(i) {
  if (!state.hits.length) return;
  state.hitIdx = (i + state.hits.length) % state.hits.length;
  const hit = state.hits[state.hitIdx];
  $("searchCount").textContent = `${state.hitIdx + 1}/${state.hits.length}`;
  goToPage(hit.pageIndex);
  // chờ cuộn rồi vẽ highlight
  setTimeout(() => { drawHighlightsForVisible(); }, 120);
}

function clearOverlay(slot) {
  const ov = slot.querySelector(".overlay");
  if (ov) ov.innerHTML = "";
}
function clearAllHighlights() {
  for (const slot of state.slots) if (slot) clearOverlay(slot);
}

function drawHighlightsForVisible() {
  clearAllHighlights();
  for (const idx of state.visible) drawHighlightsForPage(idx);
  // luôn vẽ trang chứa hit hiện tại
  if (state.hitIdx >= 0) drawHighlightsForPage(state.hits[state.hitIdx].pageIndex);
}

function drawHighlightsForPage(pageIdx) {
  const slot = state.slots[pageIdx];
  if (!slot) return;
  const p = state.pages[pageIdx];
  const ov = slot.querySelector(".overlay");
  if (!ov) return;
  const scale = PT_PER_PX * state.zoom;
  state.hits.forEach((h, i) => {
    if (h.pageIndex !== pageIdx || !h.rect) return;
    const r = h.rect;
    const div = document.createElement("div");
    div.className = "hl" + (i === state.hitIdx ? " current" : "");
    div.style.left = r.left * scale + "px";
    div.style.top = (p.heightPt - r.top) * scale + "px";
    div.style.width = (r.right - r.left) * scale + "px";
    div.style.height = (r.top - r.bottom) * scale + "px";
    ov.appendChild(div);
  });
}

// ---------- Chú thích (annotations) ----------

function hexToRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function rgbCss(c) { return `rgb(${c[0]},${c[1]},${c[2]})`; }
function escapeHtml(s) {
  return s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
}

// Highlight/Underline/Strikeout bám theo TEXT đã chọn (như Foxit), không phải
// 1 rectangle tự do — khác với Square/FreeText/Note (vẽ/đặt tự do trên trang).
function isTextMarkupTool(tool) {
  return tool === "highlight" || tool === "underline" || tool === "strikeout";
}

function setTool(tool) {
  state.tool = state.tool === tool ? null : tool;
  document.querySelectorAll(".atool").forEach((b) =>
    b.classList.toggle("active", b.dataset.tool === state.tool)
  );
  document.body.classList.toggle("tool-active", !!state.tool);
  document.body.classList.toggle("tool-mark", isTextMarkupTool(state.tool));
  $("annotHint").textContent = state.tool
    ? state.tool === "note"
      ? "Bấm lên trang để đặt ghi chú"
      : isTextMarkupTool(state.tool)
        ? "Kéo chọn văn bản trên trang để tô (bấm lại nút để tắt)"
        : "Kéo chuột trên trang để vẽ"
    : "";
}

function updateAnnotCount() {
  $("annotCount").textContent = state.annotSpecs.length;
  $("saveAnnots").disabled = state.annotSpecs.length === 0;
}

// ===== Undo/Redo (Ctrl+Z / Ctrl+Y) =====
// Snapshot toàn bộ annotSpecs trước mỗi hành động thay đổi (tạo/xoá/sửa nội
// dung-định dạng/đổi màu) — đơn giản & an toàn cho quy mô số lượng chú thích
// của 1 tài liệu, không cần theo dõi diff từng trường.
function snapshotAnnots() {
  return JSON.parse(JSON.stringify(state.annotSpecs));
}
function pushUndo() {
  state.undoStack.push(snapshotAnnots());
  if (state.undoStack.length > UNDO_LIMIT) state.undoStack.shift();
  state.redoStack = [];
  updateUndoRedoButtons();
}
function updateUndoRedoButtons() {
  $("undoBtn").disabled = state.undoStack.length === 0;
  $("redoBtn").disabled = state.redoStack.length === 0;
}
function redrawAllAnnotPages() {
  for (const slot of state.slots) {
    if (slot) drawAnnotsForPage(Number(slot.dataset.index));
  }
}
function applyAnnotSnapshot(snapshot) {
  finishEditing();
  closeNotePopup();
  state.annotSpecs = snapshot;
  state.selectedId = null;
  redrawAllAnnotPages();
  updateAnnotCount();
  updateUndoRedoButtons();
  buildComments();
}
function undo() {
  if (!state.undoStack.length) return;
  const prev = state.undoStack.pop();
  state.redoStack.push(snapshotAnnots());
  applyAnnotSnapshot(prev);
}
function redo() {
  if (!state.redoStack.length) return;
  const next = state.redoStack.pop();
  state.undoStack.push(snapshotAnnots());
  applyAnnotSnapshot(next);
}

// Vẽ preview toàn bộ chú thích chưa lưu của 1 trang.
function drawAnnotsForPage(idx) {
  const slot = state.slots[idx];
  if (!slot) return;
  const layer = slot.querySelector(".annotlayer");
  if (!layer) return;
  layer.innerHTML = "";
  const p = state.pages[idx];
  const scale = PT_PER_PX * state.zoom;
  const isMarkup = (k) => k === "highlight" || k === "underline" || k === "strikeout";

  for (const s of state.annotSpecs) {
    if (s.pageIndex !== idx) continue;
    if (editing && editing.id === s.id) continue; // đang sửa thì bỏ qua preview
    const col = rgbCss(s.color);

    if (isMarkup(s.kind)) {
      // Mỗi quad (= 1 dòng text đã chọn) vẽ riêng — không phải 1 khối phủ cả
      // khoảng trắng giữa các dòng (đúng như Foxit).
      const quads = s.quads && s.quads.length ? s.quads : [{ left: s.left, bottom: s.bottom, right: s.right, top: s.top }];
      quads.forEach((q, qi) => {
        const left = q.left * scale;
        const top = (p.heightPt - q.top) * scale;
        const w = (q.right - q.left) * scale;
        const h = (q.top - q.bottom) * scale;
        const el = document.createElement("div");
        el.dataset.id = s.id;
        if (s.kind === "highlight") {
          el.className = "a-hl a-sel";
          el.style.background = `rgba(${s.color[0]},${s.color[1]},${s.color[2]},.4)`;
          Object.assign(el.style, { left: left + "px", top: top + "px", width: w + "px", height: h + "px" });
        } else if (s.kind === "underline") {
          el.className = "a-line a-sel";
          el.style.borderTopColor = col;
          Object.assign(el.style, { left: left + "px", top: top + h - 2 + "px", width: w + "px", height: "4px" });
        } else {
          el.className = "a-line a-sel";
          el.style.borderTopColor = col;
          Object.assign(el.style, { left: left + "px", top: top + h / 2 + "px", width: w + "px", height: "4px" });
        }
        el.addEventListener("click", (ev) => { ev.stopPropagation(); selectAnnot(s.id); });
        if (s.id === state.selectedId) {
          el.classList.add("selected");
          if (qi === quads.length - 1) {
            const del = document.createElement("div");
            del.className = "a-del";
            del.textContent = "✕";
            del.title = "Xoá (Delete)";
            del.addEventListener("click", (ev) => { ev.stopPropagation(); deleteSpec(s.id); });
            el.appendChild(del);
          }
        }
        layer.appendChild(el);
      });
      continue;
    }

    const left = s.left * scale;
    const top = (p.heightPt - s.top) * scale;
    const w = (s.right - s.left) * scale;
    const h = (s.top - s.bottom) * scale;
    const el = document.createElement("div");
    el.dataset.id = s.id;
    if (s.kind === "square") {
      el.className = "a-box a-sel";
      el.style.borderColor = col;
      Object.assign(el.style, { left: left + "px", top: top + "px", width: w + "px", height: h + "px" });
    } else if (s.kind === "freetext") {
      el.className = "a-text a-sel";
      el.style.borderColor = col;
      el.style.color = col;
      el.style.fontSize = (s.fontSize || 14) * scale + "px";
      el.style.fontWeight = s.bold ? "bold" : "normal";
      el.style.fontStyle = s.italic ? "italic" : "normal";
      el.style.textDecoration = s.underline ? "underline" : "none";
      el.textContent = s.contents || "Văn bản…";
      Object.assign(el.style, { left: left + "px", top: top + "px", width: Math.max(w, 40) + "px", minHeight: h + "px" });
      el.addEventListener("dblclick", (ev) => { ev.stopPropagation(); editTextBox(s); });
    } else if (s.kind === "note") {
      el.className = "a-note a-sel";
      el.textContent = "📝";
      el.style.background = `rgb(${s.color[0]},${s.color[1]},${s.color[2]})`;
      Object.assign(el.style, { left: left + "px", top: top + "px" });
      el.addEventListener("click", (ev) => { ev.stopPropagation(); openNotePopup(s); });
    }
    if (s.kind !== "note") {
      el.addEventListener("click", (ev) => { ev.stopPropagation(); selectAnnot(s.id); });
    }
    if (s.id === state.selectedId) {
      el.classList.add("selected");
      const del = document.createElement("div");
      del.className = "a-del";
      del.textContent = "✕";
      del.title = "Xoá (Delete)";
      del.addEventListener("click", (ev) => { ev.stopPropagation(); deleteSpec(s.id); });
      el.appendChild(del);
    }
    layer.appendChild(el);
  }
}

function selectAnnot(id) {
  state.selectedId = id;
  finishEditing();
  redrawVisibleAnnots();
}
function deselectAnnot() {
  if (state.selectedId == null) return;
  state.selectedId = null;
  redrawVisibleAnnots();
}
function redrawVisibleAnnots() {
  for (const idx of state.visible) drawAnnotsForPage(idx);
}
function deleteSpec(id) {
  const i = state.annotSpecs.findIndex((s) => s.id === id);
  if (i < 0) return;
  pushUndo();
  const pg = state.annotSpecs[i].pageIndex;
  state.annotSpecs.splice(i, 1);
  if (state.selectedId === id) state.selectedId = null;
  drawAnnotsForPage(pg);
  updateAnnotCount();
  buildComments();
}

function cssToPdf(idx, cssX, cssY) {
  const p = state.pages[idx];
  const scale = PT_PER_PX * state.zoom;
  return { x: cssX / scale, y: p.heightPt - cssY / scale };
}

// ===== Sửa Text box tại chỗ (in-place) + thanh Format =====
let editing = null; // { id, el, bar }

function editTextBox(spec) {
  finishEditing();
  pushUndo(); // ghi lại trạng thái TRƯỚC khi sửa nội dung/định dạng text box
  state.selectedId = null;
  state.tool = null;
  document.querySelectorAll(".atool").forEach((b) => b.classList.remove("active"));
  document.body.classList.remove("tool-active");

  const slot = state.slots[spec.pageIndex];
  const layer = slot.querySelector(".annotlayer");
  const scale = PT_PER_PX * state.zoom;
  const left = spec.left * scale;
  const top = (state.pages[spec.pageIndex].heightPt - spec.top) * scale;
  const w = Math.max((spec.right - spec.left) * scale, 60);
  const h = Math.max((spec.top - spec.bottom) * scale, 20);

  const ed = document.createElement("div");
  ed.className = "a-editor";
  ed.contentEditable = "true";
  ed.textContent = spec.contents || "";
  Object.assign(ed.style, {
    left: left + "px", top: top + "px", width: w + "px", minHeight: h + "px",
    color: rgbCss(spec.color),
    fontSize: (spec.fontSize || 14) * scale + "px",
    fontWeight: spec.bold ? "bold" : "normal",
    fontStyle: spec.italic ? "italic" : "normal",
    textDecoration: spec.underline ? "underline" : "none",
  });
  layer.appendChild(ed);
  drawAnnotsForPage(spec.pageIndex); // ẩn preview của spec đang sửa

  editing = { id: spec.id, el: ed };
  showFmtBar(spec, ed);
  ed.focus();
  // đặt con trỏ cuối
  const sel = window.getSelection();
  sel.selectAllChildren(ed);
  sel.collapseToEnd();

  ed.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { e.preventDefault(); finishEditing(); }
  });
}

function finishEditing() {
  if (!editing) return;
  const spec = state.annotSpecs.find((s) => s.id === editing.id);
  const ed = editing.el;
  if (spec && ed) {
    spec.contents = ed.innerText.trim();
    // mở rộng rect theo chiều cao thực tế của editor
    const scale = PT_PER_PX * state.zoom;
    const realH = ed.offsetHeight / scale;
    spec.bottom = spec.top - Math.max(realH, 14);
  }
  if (ed && ed.parentNode) ed.remove();
  if (editing.bar && editing.bar.parentNode) editing.bar.remove();
  const pg = spec ? spec.pageIndex : null;
  editing = null;
  // Xoá text box rỗng (không nhập gì).
  if (spec && spec.kind === "freetext" && !spec.contents) {
    const i = state.annotSpecs.indexOf(spec);
    if (i >= 0) state.annotSpecs.splice(i, 1);
  }
  if (pg != null) drawAnnotsForPage(pg);
  updateAnnotCount();
  buildComments();
}

// Thanh định dạng nổi cho text box (size, B, I, U, màu)
function showFmtBar(spec, editorEl) {
  const bar = document.createElement("div");
  bar.className = "fmtbar";
  const r = editorEl.getBoundingClientRect();
  bar.style.left = r.left + "px";
  bar.style.top = r.top - 40 + "px";

  const sizes = [8, 10, 12, 14, 18, 24, 32, 48];
  const sizeSel = document.createElement("select");
  sizeSel.title = "Cỡ chữ";
  for (const s of sizes) {
    const o = document.createElement("option");
    o.value = s; o.textContent = s;
    if (s === (spec.fontSize || 14)) o.selected = true;
    sizeSel.appendChild(o);
  }
  sizeSel.addEventListener("change", () => {
    spec.fontSize = Number(sizeSel.value);
    editorEl.style.fontSize = spec.fontSize * PT_PER_PX * state.zoom + "px";
    editorEl.focus();
  });

  const mkToggle = (label, prop, cssApply) => {
    const b = document.createElement("button");
    b.textContent = label;
    b.className = "fmtbtn" + (spec[prop] ? " on" : "");
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => {
      spec[prop] = !spec[prop];
      b.classList.toggle("on", spec[prop]);
      cssApply(spec[prop]);
      editorEl.focus();
    });
    return b;
  };

  bar.appendChild(sizeSel);
  bar.appendChild(mkToggle("B", "bold", (on) => editorEl.style.fontWeight = on ? "bold" : "normal"));
  const it = mkToggle("I", "italic", (on) => editorEl.style.fontStyle = on ? "italic" : "normal");
  it.style.fontStyle = "italic";
  bar.appendChild(it);
  const un = mkToggle("U", "underline", (on) => editorEl.style.textDecoration = on ? "underline" : "none");
  un.style.textDecoration = "underline";
  bar.appendChild(un);

  const colorBtn = document.createElement("button");
  colorBtn.className = "fmtbtn fmt-color";
  colorBtn.innerHTML = `<span class="sw" style="background:${rgbCss(spec.color)}"></span>A`;
  colorBtn.addEventListener("mousedown", (e) => e.preventDefault());
  colorBtn.addEventListener("click", () => {
    openColorPopover(colorBtn, spec.color, (rgb) => {
      spec.color = rgb;
      editorEl.style.color = rgbCss(rgb);
      colorBtn.querySelector(".sw").style.background = rgbCss(rgb);
      editorEl.focus();
    });
  });
  bar.appendChild(colorBtn);

  document.body.appendChild(bar);
  editing.bar = bar;
}

// ===== Note popup (xem/sửa/đổi màu/xoá) =====
let notePopupEl = null;
function openNotePopup(spec) {
  closeNotePopup();
  pushUndo(); // ghi lại trạng thái TRƯỚC khi sửa nội dung/màu ghi chú
  const slot = state.slots[spec.pageIndex];
  const layer = slot.querySelector(".annotlayer");
  const icon = layer.querySelector(`.a-note[data-id="${spec.id}"]`);
  const r = (icon || slot).getBoundingClientRect();

  const pop = document.createElement("div");
  pop.className = "notepop";
  pop.style.left = r.right + 6 + "px";
  pop.style.top = r.top + "px";
  pop.innerHTML = `
    <div class="np-head"><span>Ghi chú</span><span class="np-close">✕</span></div>
    <textarea class="np-text" placeholder="Nhập nội dung ghi chú…">${escapeHtml(spec.contents || "")}</textarea>
    <div class="np-foot">
      <button class="np-color"><span class="sw" style="background:${rgbCss(spec.color)}"></span>Màu</button>
      <button class="np-del">🗑 Xoá</button>
    </div>`;
  document.body.appendChild(pop);
  notePopupEl = pop;

  const ta = pop.querySelector(".np-text");
  ta.addEventListener("input", () => {
    spec.contents = ta.value;
    buildComments();
  });
  pop.querySelector(".np-close").addEventListener("click", closeNotePopup);
  pop.querySelector(".np-del").addEventListener("click", () => {
    closeNotePopup();
    deleteSpec(spec.id);
  });
  pop.querySelector(".np-color").addEventListener("click", (e) => {
    openColorPopover(e.currentTarget, spec.color, (rgb) => {
      spec.color = rgb;
      pop.querySelector(".np-color .sw").style.background = rgbCss(rgb);
      drawAnnotsForPage(spec.pageIndex);
    });
  });
  ta.focus();
}
function closeNotePopup() {
  if (notePopupEl) { notePopupEl.remove(); notePopupEl = null; }
}

// ===== Color popover (preset + gần đây + tuỳ chọn) =====
let colorPopEl = null;
function openColorPopover(anchor, currentRgb, onPick) {
  closeColorPopover();
  const r = anchor.getBoundingClientRect();
  const pop = document.createElement("div");
  pop.className = "colorpop";
  pop.style.left = r.left + "px";
  pop.style.top = r.bottom + 4 + "px";

  const addSwatch = (rgb, container) => {
    const s = document.createElement("div");
    s.className = "cs";
    s.style.background = rgbCss(rgb);
    if (rgb[0] === currentRgb[0] && rgb[1] === currentRgb[1] && rgb[2] === currentRgb[2]) s.classList.add("cur");
    s.addEventListener("click", () => {
      pushRecentColor(rgb);
      onPick(rgb.slice());
      closeColorPopover();
    });
    container.appendChild(s);
  };

  const grid = document.createElement("div");
  grid.className = "cgrid";
  for (const c of PRESET_COLORS) addSwatch(c, grid);
  pop.appendChild(grid);

  if (state.recentColors.length) {
    const lbl = document.createElement("div");
    lbl.className = "clbl"; lbl.textContent = "Gần đây";
    pop.appendChild(lbl);
    const rg = document.createElement("div");
    rg.className = "cgrid";
    for (const c of state.recentColors) addSwatch(c, rg);
    pop.appendChild(rg);
  }

  const custom = document.createElement("label");
  custom.className = "ccustom";
  custom.innerHTML = `Tuỳ chọn… <input type="color" value="${rgbToHex(currentRgb)}">`;
  custom.querySelector("input").addEventListener("input", (e) => {
    const rgb = hexToRgb(e.target.value);
    pushRecentColor(rgb);
    onPick(rgb);
  });
  custom.querySelector("input").addEventListener("change", closeColorPopover);
  pop.appendChild(custom);

  document.body.appendChild(pop);
  colorPopEl = pop;
}
function closeColorPopover() {
  if (colorPopEl) { colorPopEl.remove(); colorPopEl = null; }
}
function pushRecentColor(rgb) {
  state.recentColors = state.recentColors.filter((c) => !(c[0] === rgb[0] && c[1] === rgb[1] && c[2] === rgb[2]));
  state.recentColors.unshift(rgb.slice());
  state.recentColors = state.recentColors.slice(0, 8);
}
function rgbToHex(c) {
  return "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");
}

let drag = null;
function onPagesMouseDown(e) {
  if (!state.tool) {
    // click vùng trống: bỏ chọn + đóng popup + kết thúc sửa
    if (!e.target.closest(".a-editor")) {
      finishEditing();
      closeNotePopup();
      closeColorPopover();
      if (!e.target.dataset || e.target.dataset.id == null) deselectAnnot();
    }
    return;
  }
  // Highlight/Underline/Strikeout: để trình duyệt tự xử lý chọn text (textlayer
  // pointer-events đã mở qua .tool-mark) — tạo annotation ở mouseup từ Selection.
  if (isTextMarkupTool(state.tool)) return;
  const slot = e.target.closest && e.target.closest(".page-slot");
  if (!slot) return;
  e.preventDefault();
  const r = slot.getBoundingClientRect();
  drag = {
    idx: Number(slot.dataset.index),
    x0: e.clientX - r.left,
    y0: e.clientY - r.top,
    x1: e.clientX - r.left,
    y1: e.clientY - r.top,
  };
}
function onPagesMouseMove(e) {
  if (!drag) return;
  const slot = state.slots[drag.idx];
  const r = slot.getBoundingClientRect();
  drag.x1 = e.clientX - r.left;
  drag.y1 = e.clientY - r.top;
  const layer = slot.querySelector(".annotlayer");
  let prev = layer.querySelector(".a-preview");
  if (!prev) {
    prev = document.createElement("div");
    prev.className = "a-hl a-preview";
    layer.appendChild(prev);
  }
  const l = Math.min(drag.x0, drag.x1);
  const t = Math.min(drag.y0, drag.y1);
  prev.style.cssText =
    `left:${l}px;top:${t}px;width:${Math.abs(drag.x1 - drag.x0)}px;height:${Math.abs(drag.y1 - drag.y0)}px;` +
    `background:rgba(${state.color[0]},${state.color[1]},${state.color[2]},.3)`;
}

// Mỗi "từ" trong text layer là 1 <span> định vị tuyệt đối riêng (để bám đúng
// pixel PDFium) — vì vậy Range.getClientRects() trả về 1 rect/TỪ, không phải
// 1 rect/DÒNG như với text chảy bình thường. Gộp các rect chồng lấp theo
// chiều dọc (cùng 1 dòng) thành 1 rect duy nhất, để highlight 1 cụm từ liền
// mạch chứ không bị đứt từng từ với khoảng trắng hở giữa.
function mergeRectsIntoLines(rects) {
  const sorted = rects.slice().sort((a, b) => a.top - b.top);
  const lines = [];
  for (const r of sorted) {
    const line = lines.find((l) => {
      const overlap = Math.min(l.bottom, r.bottom) - Math.max(l.top, r.top);
      return overlap > 0.5 * Math.min(l.bottom - l.top, r.bottom - r.top);
    });
    if (line) {
      line.left = Math.min(line.left, r.left);
      line.right = Math.max(line.right, r.right);
      line.top = Math.min(line.top, r.top);
      line.bottom = Math.max(line.bottom, r.bottom);
    } else {
      lines.push({ left: r.left, right: r.right, top: r.top, bottom: r.bottom });
    }
  }
  return lines;
}

// Tạo Highlight/Underline/Strikeout từ vùng TEXT người dùng vừa chọn (Selection
// API) — bám đúng từng dòng như Foxit, không phải 1 rectangle tự do.
function createMarkupFromSelection() {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return;
  const range = sel.getRangeAt(0);
  const anchorNode = range.commonAncestorContainer;
  const anchorEl = anchorNode.nodeType === 1 ? anchorNode : anchorNode.parentElement;
  const slot = anchorEl && anchorEl.closest(".page-slot");
  if (!slot) { sel.removeAllRanges(); return; }

  const idx = Number(slot.dataset.index);
  const slotRect = slot.getBoundingClientRect();
  const rawRects = Array.from(range.getClientRects()).filter((r) => r.width > 0.5 && r.height > 0.5);
  sel.removeAllRanges();
  if (!rawRects.length) return;
  const clientRects = mergeRectsIntoLines(rawRects);

  const quads = clientRects.map((r) => {
    const a = cssToPdf(idx, r.left - slotRect.left, r.top - slotRect.top);
    const b = cssToPdf(idx, r.right - slotRect.left, r.bottom - slotRect.top);
    return {
      left: Math.min(a.x, b.x), right: Math.max(a.x, b.x),
      bottom: Math.min(a.y, b.y), top: Math.max(a.y, b.y),
    };
  });
  const bounds = quads.reduce(
    (acc, q) => acc ? {
      left: Math.min(acc.left, q.left), right: Math.max(acc.right, q.right),
      bottom: Math.min(acc.bottom, q.bottom), top: Math.max(acc.top, q.top),
    } : { ...q },
    null
  );

  const spec = {
    id: annotIdSeq++,
    kind: state.tool,
    pageIndex: idx,
    left: bounds.left, bottom: bounds.bottom, right: bounds.right, top: bounds.top,
    quads,
    color: [...state.color],
    contents: "",
    fontSize: 14,
    bold: false,
    italic: false,
    underline: false,
  };
  pushUndo();
  state.annotSpecs.push(spec);
  pushRecentColor(state.color);
  // Giữ tool active để tô tiếp liên tục (như Foxit) — bấm lại nút hoặc Esc để tắt.
  drawAnnotsForPage(idx);
  updateAnnotCount();
  buildComments();
}

function onPagesMouseUp() {
  if (state.tool && isTextMarkupTool(state.tool)) {
    createMarkupFromSelection();
    return;
  }
  if (!drag) return;
  const d = drag;
  drag = null;
  const slot = state.slots[d.idx];
  const prev = slot.querySelector(".a-preview");
  if (prev) prev.remove();

  const a = cssToPdf(d.idx, d.x0, d.y0);
  const b = cssToPdf(d.idx, d.x1, d.y1);
  const rect = {
    left: Math.min(a.x, b.x),
    right: Math.max(a.x, b.x),
    bottom: Math.min(a.y, b.y),
    top: Math.max(a.y, b.y),
  };
  const tool = state.tool;
  const scale = PT_PER_PX * state.zoom;
  const tiny = rect.right - rect.left < 2 || rect.top - rect.bottom < 2;

  if (tool === "note") {
    const sz = 18 / scale;
    rect.right = rect.left + sz;
    rect.bottom = rect.top - sz;
  } else if (tool === "freetext") {
    if (tiny) {
      rect.right = rect.left + 160 / scale;
      rect.top = rect.bottom + 24 / scale;
    }
  } else if (tiny) {
    return;
  }

  const spec = {
    id: annotIdSeq++,
    kind: tool,
    pageIndex: d.idx,
    left: rect.left,
    bottom: rect.bottom,
    right: rect.right,
    top: rect.top,
    color: [...state.color],
    contents: "",
    fontSize: 14,
    bold: false,
    italic: false,
    underline: false,
  };
  pushUndo();
  state.annotSpecs.push(spec);
  pushRecentColor(state.color);

  // FreeText/Note: gõ ngay → về chế độ chọn (như Foxit). Square: giữ tool active
  // để vẽ tiếp liên tục.
  if (tool === "freetext" || tool === "note") setTool(null);
  drawAnnotsForPage(d.idx);
  updateAnnotCount();
  buildComments();

  if (tool === "freetext") editTextBox(spec);
  else if (tool === "note") openNotePopup(spec);
}

async function saveAnnots() {
  if (!state.annotSpecs.length) return;
  finishEditing();
  const out = await invoke("pick_save_pdf");
  if (!out) return;
  try {
    const specs = state.annotSpecs.map((s) => ({
      kind: s.kind,
      pageIndex: s.pageIndex,
      left: s.left, bottom: s.bottom, right: s.right, top: s.top,
      quads: s.quads || [],
      color: [s.color[0], s.color[1], s.color[2], 255],
      contents: s.contents || null,
      fontSize: s.fontSize || 14,
      bold: !!s.bold,
      italic: !!s.italic,
      underline: !!s.underline,
    }));
    await invoke("apply_annotations", { input: state.path, output: out, specs });
    const n = state.annotSpecs.length;
    state.annotSpecs = [];
    updateAnnotCount();
    $("status").textContent = `Đã lưu ${n} chú thích → ${shortName(out)}`;
    loadDocument(out);
  } catch (e) {
    $("status").textContent = "Lỗi lưu chú thích: " + e;
  }
}

function buildComments() {
  const box = $("comments");
  box.innerHTML = "";
  if (!state.annotSpecs.length) {
    box.innerHTML = `<div class="empty">Chưa có chú thích chưa lưu. Chọn công cụ ở thanh trên và vẽ lên trang, rồi bấm "Lưu chú thích".</div>`;
    return;
  }
  const labels = {
    highlight: "Tô sáng", underline: "Gạch chân", strikeout: "Gạch ngang",
    square: "Khung", freetext: "Text box", note: "Ghi chú",
  };
  for (const s of state.annotSpecs) {
    const el = document.createElement("div");
    el.className = "citem";
    el.innerHTML =
      `<span class="cdel">✕</span>` +
      `<span class="csw" style="background:${rgbCss(s.color)}"></span>` +
      `<span class="ckind">${labels[s.kind] || s.kind}</span> · trang ${s.pageIndex + 1}` +
      (s.contents ? `<br><span class="ctxt">${escapeHtml(s.contents)}</span>` : "");
    el.addEventListener("click", (ev) => {
      if (ev.target.classList.contains("cdel")) {
        deleteSpec(s.id);
      } else {
        goToPage(s.pageIndex);
        selectAnnot(s.id);
        if (s.kind === "note") openNotePopup(s);
      }
    });
    box.appendChild(el);
  }
}

// ---------- Copy ----------

async function copyCurrentPage() {
  try {
    const txt = await invoke("page_text", { path: state.path, page: state.current });
    await navigator.clipboard.writeText(txt);
    $("status").textContent = `Đã copy text trang ${state.current + 1} (${txt.length} ký tự)`;
  } catch (e) {
    $("status").textContent = "Lỗi copy: " + e;
  }
}

// ---------- Sự kiện ----------

$("openBtn").addEventListener("click", openFile);
$("zoomIn").addEventListener("click", () => setZoom(state.zoom * 1.25));
$("zoomOut").addEventListener("click", () => setZoom(state.zoom / 1.25));
$("zoomFit").addEventListener("click", fitWidth);
$("zoomFitPage").addEventListener("click", fitPage);
buildZoomSelect();
$("zoomSelect").addEventListener("change", (e) => setZoom(Number(e.target.value) / 100));
$("copyPage").addEventListener("click", copyCurrentPage);

$("pagePrev").addEventListener("click", gotoPrevPage);
$("pageNext").addEventListener("click", gotoNextPage);
$("pageInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); goToPageInput(); }
});
$("pageInput").addEventListener("blur", goToPageInput);

$("searchBox").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(runSearch, 300);
});
$("searchBox").addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); gotoHit(state.hitIdx + (e.shiftKey ? -1 : 1)); }
});
$("searchCase").addEventListener("change", runSearch);
$("searchNext").addEventListener("click", () => gotoHit(state.hitIdx + 1));
$("searchPrev").addEventListener("click", () => gotoHit(state.hitIdx - 1));
$("viewport").addEventListener("scroll", () => {
  updateCurrentPage();
  if (state.hits.length) drawHighlightsForVisible();
  closeColorPopover();
  closeNotePopup();
  finishEditing();
});
// Ctrl + lăn chuột: zoom tại vị trí con trỏ.
// Gộp nhiều nấc lăn trong 1 khung hình (rAF) để mượt, không dồn render.
let wheelFactor = 1;
let wheelRAF = 0;
let wheelX = 0;
let wheelY = 0;
$("viewport").addEventListener(
  "wheel",
  (e) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    wheelFactor *= e.deltaY < 0 ? 1.1 : 1 / 1.1;
    wheelX = e.clientX;
    wheelY = e.clientY;
    if (!wheelRAF) {
      wheelRAF = requestAnimationFrame(() => {
        wheelRAF = 0;
        const f = wheelFactor;
        wheelFactor = 1;
        zoomAtPoint(state.zoom * f, wheelX, wheelY);
      });
    }
  },
  { passive: false }
);

$("tabThumbs").addEventListener("click", () => switchTab("thumbs"));
$("tabOutline").addEventListener("click", () => switchTab("outline"));
$("tabComments").addEventListener("click", () => switchTab("comments"));
function switchTab(which) {
  for (const [tab, panel] of [["tabThumbs", "thumbs"], ["tabOutline", "outline"], ["tabComments", "comments"]]) {
    const on = panel === which;
    $(tab).classList.toggle("active", on);
    $(panel).classList.toggle("hidden", !on);
  }
}

// Công cụ chú thích
document.querySelectorAll(".atool").forEach((b) =>
  b.addEventListener("click", () => setTool(b.dataset.tool))
);
function refreshColorSwatch() {
  const sw = $("annotSw");
  if (sw) sw.style.background = rgbCss(state.color);
}
refreshColorSwatch();
$("annotColorBtn").addEventListener("click", () => {
  openColorPopover($("annotColorBtn"), state.color, (rgb) => {
    state.color = rgb;
    refreshColorSwatch();
    // áp cho annotation đang chọn (nếu có)
    if (state.selectedId != null) {
      const s = state.annotSpecs.find((x) => x.id === state.selectedId);
      if (s) { pushUndo(); s.color = rgb.slice(); drawAnnotsForPage(s.pageIndex); buildComments(); }
    }
  });
});
$("saveAnnots").addEventListener("click", saveAnnots);
$("undoBtn").addEventListener("click", undo);
$("redoBtn").addEventListener("click", redo);
$("pages").addEventListener("mousedown", onPagesMouseDown);
window.addEventListener("mousemove", onPagesMouseMove);
window.addEventListener("mouseup", onPagesMouseUp);

// Phím tắt chuẩn của trình xem PDF (Foxit/Adobe): điều hướng trang, zoom,
// tìm kiếm, xoá/bỏ chọn annotation.
window.addEventListener("keydown", (e) => {
  const typing = e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName);

  if (e.key === "Escape") {
    finishEditing(); closeNotePopup(); closeColorPopover(); deselectAnnot(); setTool(null);
    return;
  }
  if ((e.key === "Delete" || e.key === "Backspace") && !typing && state.selectedId != null) {
    e.preventDefault();
    deleteSpec(state.selectedId);
    return;
  }
  if (e.ctrlKey && (e.key === "f" || e.key === "F")) {
    e.preventDefault();
    $("searchBox").focus();
    $("searchBox").select();
    return;
  }
  if (e.ctrlKey && (e.key === "=" || e.key === "+")) { e.preventDefault(); setZoom(state.zoom * 1.25); return; }
  if (e.ctrlKey && e.key === "-") { e.preventDefault(); setZoom(state.zoom / 1.25); return; }
  if (e.ctrlKey && e.key === "0") { e.preventDefault(); setZoom(1); return; }
  // Ctrl+Z hoàn tác / Ctrl+Y hoặc Ctrl+Shift+Z làm lại — không chặn khi đang gõ
  // trong ô text (để trình duyệt tự xử lý undo cấp ký tự trong contenteditable).
  if (e.ctrlKey && !e.shiftKey && (e.key === "z" || e.key === "Z") && !typing) {
    e.preventDefault();
    undo();
    return;
  }
  if (e.ctrlKey && (e.key === "y" || e.key === "Y" || (e.shiftKey && (e.key === "z" || e.key === "Z"))) && !typing) {
    e.preventDefault();
    redo();
    return;
  }
  if (typing) return;
  if (e.key === "PageDown") { e.preventDefault(); gotoNextPage(); return; }
  if (e.key === "PageUp") { e.preventDefault(); gotoPrevPage(); return; }
  if (e.key === "Home") { e.preventDefault(); goToPage(0); return; }
  if (e.key === "End") { e.preventDefault(); goToPage(state.pages.length - 1); return; }
});

window.addEventListener("DOMContentLoaded", boot);
