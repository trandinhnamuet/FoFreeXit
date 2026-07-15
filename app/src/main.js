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
  undoStack: [],      // snapshot {annotSpecs,pagePlan} trước mỗi hành động (Ctrl+Z)
  redoStack: [],
  pagePlan: [],       // [{kind:'existing'|'blank', source, srcIndex, widthPt, heightPt, rotationDelta, crop}]
  organizeMode: false,
  orgSelected: new Set(), // index trong pagePlan đang chọn (chế độ Tổ chức trang)
  orgAnchor: null,        // index neo cho Shift-click chọn dải
  orgThumbs: new Map(),   // cache dataURL thumbnail theo "source#srcIndex" (tránh render lại khi chọn/xoay)
  // ----- Phase 4: Sửa nội dung -----
  editMode: false,
  editPage: 0,            // trang đang sửa
  editBase: null,         // file làm việc hiện tại (mỗi op materialize ra file tạm mới)
  editUndo: [],           // stack đường dẫn editBase trước mỗi op
  editRedo: [],
  editObjects: [],        // ObjectInfo của editPage (đọc lại từ editBase sau mỗi op)
  editSel: null,          // index object đang chọn
  editScale: 1,           // px/pt của ảnh stage
  editArm: null,          // 'text' | 'image' khi đang chờ click đặt; null = không
  editPendingImage: null, // đường dẫn ảnh chờ đặt (Thêm ảnh)
  editColor: [0, 0, 0],   // màu chữ áp khi sửa/thêm text
  editTemps: [],          // mọi file tạm đã materialize trong phiên sửa (để dọn)
  secMode: false,         // thanh Bảo mật (Phase 5) đang mở
  redactMarks: [],        // [{page, rect{left,bottom,right,top}}] chờ áp dụng
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
    state.redactMarks = [];
    updateRedactButtons();
    updateUndoRedoButtons();
    setTool(null);
    updateAnnotCount();
    buildComments();
    $("searchBox").value = "";
    $("searchCount").textContent = "—";

    let meta;
    try {
      meta = await invoke("open_document", { path });
    } catch (e) {
      // File hỏng (xref/trailer sai) — tự sửa qua QPDF rồi mở lại (Phase 3).
      const usable = await invoke("ensure_openable", { path, password: null });
      if (usable === path) throw e;
      state.path = usable;
      meta = await invoke("open_document", { path: usable });
    }
    state.pages = meta.pages;
    buildPages();
    buildThumbnails();
    buildOutline(meta.outline);
    $("status").textContent = `${meta.pageCount} trang · ${shortName(path)}`;
    updatePageTotal();
    updateCurrentPage();
    updateZoomLabel();

    state.pagePlan = await invoke("organize_identity_plan", { path: state.path, password: null }).catch(() => []);
    state.orgSelected = new Set();
    state.orgAnchor = null;
    state.orgThumbs = new Map();
    if (state.organizeMode) buildOrganizeGrid();
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
      : state.tool === "redact"
        ? "Kéo chuột quét vùng cần bôi đen — nội dung sẽ bị XOÁ THẬT khi Áp dụng"
        : isTextMarkupTool(state.tool)
          ? "Kéo chọn văn bản trên trang để tô (bấm lại nút để tắt)"
          : "Kéo chuột trên trang để vẽ"
    : "";
}

function updateAnnotCount() {
  $("annotCount").textContent = state.annotSpecs.length;
  $("saveAnnots").disabled = state.annotSpecs.length === 0;
}

// ===== Undo/Redo TOÀN CỤC (Ctrl+Z / Ctrl+Y) =====
// Snapshot {annotSpecs, pagePlan} trước mỗi hành động thay đổi (tạo/xoá/sửa
// chú thích, chèn/xoá/xoay/đảo/crop trang...) — đơn giản & an toàn cho quy mô
// dữ liệu của 1 tài liệu, không cần theo dõi diff từng trường. 1 stack DUY
// NHẤT cho cả Annotate và Organize → Ctrl+Z hoạt động xuyên cả 2 chế độ.
function snapshot() {
  return JSON.parse(JSON.stringify({ annotSpecs: state.annotSpecs, pagePlan: state.pagePlan }));
}
function pushUndo() {
  state.undoStack.push(snapshot());
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
function applySnapshot(snap) {
  finishEditing();
  closeNotePopup();
  state.annotSpecs = snap.annotSpecs;
  state.pagePlan = snap.pagePlan;
  state.selectedId = null;
  state.orgSelected = new Set();
  redrawAllAnnotPages();
  updateAnnotCount();
  updateUndoRedoButtons();
  buildComments();
  if (state.organizeMode) buildOrganizeGrid();
}
function undo() {
  if (!state.undoStack.length) return;
  const prev = state.undoStack.pop();
  state.redoStack.push(snapshot());
  applySnapshot(prev);
}
function redo() {
  if (!state.redoStack.length) return;
  const next = state.redoStack.pop();
  state.undoStack.push(snapshot());
  applySnapshot(next);
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

  // Đánh dấu redact (Phase 5) — vẽ cùng layer để sống sót qua mọi lần redraw.
  for (const m of state.redactMarks) {
    if (m.page !== idx) continue;
    const el = document.createElement("div");
    el.className = "redact-mark";
    Object.assign(el.style, {
      left: m.rect.left * scale + "px",
      top: (p.heightPt - m.rect.top) * scale + "px",
      width: (m.rect.right - m.rect.left) * scale + "px",
      height: (m.rect.top - m.rect.bottom) * scale + "px",
    });
    el.title = "Bấm để bỏ đánh dấu redact";
    el.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const i = state.redactMarks.indexOf(m);
      if (i >= 0) state.redactMarks.splice(i, 1);
      drawAnnotsForPage(idx);
      updateRedactButtons();
    });
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

  if (tool === "crop") {
    if (tiny) return;
    openCropDialog(d.idx, rect);
    return;
  }
  if (tool === "redact") {
    if (tiny) return;
    state.redactMarks.push({ page: d.idx, rect });
    drawAnnotsForPage(d.idx);
    updateRedactButtons();
    return; // giữ tool để quét tiếp nhiều vùng (như Foxit Mark for Redaction)
  }
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

// ---------- Phase 3: Tổ chức trang ----------

function openModal(title, bodyHtml) {
  $("modalBox").innerHTML = `<h3>${title}</h3>${bodyHtml}`;
  $("modalOverlay").classList.remove("hidden");
  return $("modalBox");
}
function closeModal() {
  $("modalOverlay").classList.add("hidden");
  $("modalBox").innerHTML = "";
}

/// Parse "1-3,5" (1-based, kiểu người dùng nhập) → mảng index 0-based hợp lệ.
function parsePageRange(str, count) {
  const out = [];
  for (const part of str.split(",")) {
    const t = part.trim();
    if (!t) continue;
    const m = t.match(/^(\d+)(?:-(\d+))?$/);
    if (!m) continue;
    const a = Number(m[1]);
    const b = m[2] ? Number(m[2]) : a;
    for (let n = Math.min(a, b); n <= Math.max(a, b); n++) {
      if (n >= 1 && n <= count) out.push(n - 1);
    }
  }
  return out;
}

function enterOrganizeMode() {
  state.organizeMode = true;
  $("annobar").classList.add("hidden");
  $("organizeBar").classList.remove("hidden");
  $("viewport").classList.add("hidden");
  $("organizeGrid").classList.remove("hidden");
  $("sidebar").classList.add("hidden");
  $("organizeModeBtn").classList.add("active");
  setTool(null);
  buildOrganizeGrid();
}
function exitOrganizeMode() {
  state.organizeMode = false;
  $("annobar").classList.remove("hidden");
  $("organizeBar").classList.add("hidden");
  $("viewport").classList.remove("hidden");
  $("organizeGrid").classList.add("hidden");
  $("sidebar").classList.remove("hidden");
  $("organizeModeBtn").classList.remove("active");
}
function toggleOrganizeMode() {
  if (state.organizeMode) exitOrganizeMode();
  else enterOrganizeMode();
}

function orgSelectCard(i, e) {
  if (e.shiftKey && state.orgAnchor != null) {
    const a = Math.min(state.orgAnchor, i);
    const b = Math.max(state.orgAnchor, i);
    state.orgSelected = new Set();
    for (let k = a; k <= b; k++) state.orgSelected.add(k);
  } else if (e.ctrlKey || e.metaKey) {
    if (state.orgSelected.has(i)) state.orgSelected.delete(i);
    else state.orgSelected.add(i);
    state.orgAnchor = i;
  } else {
    state.orgSelected = new Set([i]);
    state.orgAnchor = i;
  }
  // Chỉ đổi class — KHÔNG dựng lại lưới (tránh render lại toàn bộ thumbnail).
  refreshOrgSelection();
}

// Chỉ cập nhật trạng thái chọn (viền xanh) trên các card hiện có — không đụng
// tới thumbnail. Tách khỏi buildOrganizeGrid để click chọn không gây render lại.
function refreshOrgSelection() {
  const cards = $("organizeGrid").children;
  for (let i = 0; i < cards.length; i++) {
    cards[i].classList.toggle("selected", state.orgSelected.has(i));
  }
}

// pagePlan có khác trạng thái gốc của file đang mở không (đã chèn/xoá/đảo/xoay/
// crop)? Watermark/Header-Footer cần biết để áp đúng lên trạng thái đang xem,
// không phải file gốc còn trên đĩa (xem materializeBaseInput).
function planIsDirty() {
  if (state.pagePlan.length !== state.pages.length) return true;
  return state.pagePlan.some((e, i) =>
    e.kind !== "existing" || e.source || e.srcIndex !== i ||
    (e.rotationDelta || 0) !== 0 || e.crop);
}

// Trả về đường dẫn file nên dùng làm "input" cho Watermark/Header-Footer:
// nếu pagePlan đang giữ nguyên trạng thái gốc → dùng thẳng state.path; nếu
// đang có thay đổi tổ chức trang chưa lưu (chèn/xoá/đảo/xoay/crop) → dựng tạm
// 1 file theo ĐÚNG trạng thái đang xem trên lưới (qua organize_materialize,
// dùng lại build_document đã có) rồi dùng file đó — để watermark/đánh số thấy
// đúng những gì người dùng đang thấy, không cần ép họ lưu rồi mở lại.
async function materializeBaseInput() {
  if (!planIsDirty()) return { path: state.path, isTemp: false, pageCount: state.pagePlan.length };
  const path = await invoke("organize_materialize", {
    mainInput: state.path, plan: state.pagePlan, password: null,
  });
  return { path, isTemp: true, pageCount: state.pagePlan.length };
}
const MATERIALIZED_NOTE =
  `<p class="status">Đang dùng bản xem trước đã gồm các thay đổi tổ chức trang ` +
  `chưa lưu (chèn/xoá/đảo/xoay/crop) — chưa ghi đè file gốc.</p>`;

function orgMovePage(from, to) {
  if (from === to) return;
  pushUndo();
  const [moved] = state.pagePlan.splice(from, 1);
  state.pagePlan.splice(to, 0, moved);
  state.orgSelected = new Set();
  state.orgAnchor = null;
  buildOrganizeGrid();
}

// Khoá cache thumbnail: 1 ảnh render chỉ phụ thuộc nguồn + trang gốc (rotation
// áp bằng CSS transform, crop chỉ là dấu chấm — đều không cần render lại).
function orgThumbKey(entry) {
  return `${entry.source || ""}#${entry.srcIndex}`;
}

// Dựng lại toàn bộ lưới (chỉ gọi khi CẤU TRÚC plan đổi: chèn/xoá/đảo). Thumbnail
// lấy từ cache nếu có; chỉ render trang chưa từng render → click chọn/xoay không
// còn kéo theo render lại toàn bộ.
function buildOrganizeGrid() {
  const box = $("organizeGrid");
  box.innerHTML = "";
  state.pagePlan.forEach((entry, i) => {
    const dims = (!entry.source && state.pages[entry.srcIndex])
      ? state.pages[entry.srcIndex]
      : { widthPt: entry.widthPt || 612, heightPt: entry.heightPt || 792 };
    const card = document.createElement("div");
    card.className = "org-card" + (state.orgSelected.has(i) ? " selected" : "");
    card.draggable = true;
    card.dataset.index = String(i);
    const wrapH = Math.round(160 * (dims.heightPt / dims.widthPt));
    card.innerHTML =
      `<div class="org-thumb-wrap" style="height:${wrapH}px">` +
      (entry.kind === "blank"
        ? `<div style="width:100%;height:100%;background:#fff"></div>`
        : `<img alt="p${i}"/>`) +
      (entry.crop ? `<div class="org-crop-mark"></div>` : "") +
      `</div><div class="org-n">${i + 1}</div>`;
    const img = card.querySelector("img");
    if (img) {
      const rot = entry.rotationDelta || 0;
      if (rot) img.style.transform = `rotate(${rot}deg)`;
      const key = orgThumbKey(entry);
      const cached = state.orgThumbs.get(key);
      if (cached) {
        img.src = cached;
      } else {
        const path = entry.source || state.path;
        invoke("render_page", { path, page: entry.srcIndex, width: 160 })
          .then((url) => { state.orgThumbs.set(key, url); img.src = url; })
          .catch(() => {});
      }
    }
    card.addEventListener("click", (e) => orgSelectCard(i, e));
    card.addEventListener("dragstart", (e) => e.dataTransfer.setData("text/plain", String(i)));
    card.addEventListener("dragover", (e) => { e.preventDefault(); card.classList.add("drag-over"); });
    card.addEventListener("dragleave", () => card.classList.remove("drag-over"));
    card.addEventListener("drop", (e) => {
      e.preventDefault();
      card.classList.remove("drag-over");
      const from = Number(e.dataTransfer.getData("text/plain"));
      orgMovePage(from, i);
    });
    box.appendChild(card);
  });
  $("orgSave").disabled = state.pagePlan.length === 0;
}

function orgDeleteSelected() {
  if (!state.orgSelected.size) { $("organizeHint").textContent = "Chưa chọn trang nào để xoá"; return; }
  if (state.orgSelected.size >= state.pagePlan.length) { $("organizeHint").textContent = "Không thể xoá hết toàn bộ trang"; return; }
  pushUndo();
  state.pagePlan = state.pagePlan.filter((_, i) => !state.orgSelected.has(i));
  state.orgSelected = new Set();
  $("organizeHint").textContent = "";
  buildOrganizeGrid();
}

function orgRotateSelected(delta) {
  if (!state.pagePlan.length) return;
  pushUndo();
  const targets = state.orgSelected.size ? state.orgSelected : new Set(state.pagePlan.map((_, i) => i));
  const cards = $("organizeGrid").children;
  for (const i of targets) {
    const entry = state.pagePlan[i];
    entry.rotationDelta = ((entry.rotationDelta || 0) + delta + 360) % 360;
    // Chỉ xoay ảnh bằng CSS — KHÔNG render lại trang.
    const img = cards[i] && cards[i].querySelector("img");
    if (img) img.style.transform = entry.rotationDelta ? `rotate(${entry.rotationDelta}deg)` : "";
  }
}

async function orgSaveChanges() {
  const out = await invoke("pick_save_pdf");
  if (!out) return;
  try {
    await invoke("organize_apply", { mainInput: state.path, plan: state.pagePlan, output: out, password: null });
    $("status").textContent = `Đã lưu thay đổi tổ chức trang → ${shortName(out)}`;
    exitOrganizeMode();
    loadDocument(out);
  } catch (e) {
    $("organizeHint").textContent = "Lỗi lưu: " + e;
  }
}

function openInsertDialog() {
  const box = openModal("Chèn trang", `
    <div class="radiorow">
      <label><input type="radio" name="insKind" value="blank" checked> Trang trắng</label>
      <label><input type="radio" name="insKind" value="file"> Từ file…</label>
    </div>
    <div id="insBlankOpts">
      <label>Cỡ giấy</label>
      <select id="insPaper">
        <option value="612x792">Letter</option>
        <option value="595x842">A4</option>
        <option value="custom">Tuỳ chọn…</option>
      </select>
      <div class="row" id="insCustomSize" style="display:none">
        <div><label>Rộng (pt)</label><input type="number" id="insW" value="612"></div>
        <div><label>Cao (pt)</label><input type="number" id="insH" value="792"></div>
      </div>
    </div>
    <div id="insFileOpts" style="display:none">
      <label>File nguồn</label>
      <button id="insPickFile" type="button">📂 Chọn file…</button>
      <span id="insFileName" class="status"></span>
      <label>Trang (vd 1-3,5 — rỗng = tất cả)</label>
      <input type="text" id="insRange" placeholder="tất cả">
    </div>
    <label>Vị trí</label>
    <div class="radiorow">
      <label><input type="radio" name="insPos" value="before" ${state.orgSelected.size ? "" : "disabled"}> Trước trang đang chọn</label>
      <label><input type="radio" name="insPos" value="after" ${state.orgSelected.size ? "checked" : "disabled"}> Sau trang đang chọn</label>
      <label><input type="radio" name="insPos" value="end" ${state.orgSelected.size ? "" : "checked"}> Cuối tài liệu</label>
    </div>
    <div class="err" id="insErr"></div>
    <div class="foot"><button id="insCancel">Huỷ</button><button id="insOk" class="primary">Chèn</button></div>
  `);
  let insertFile = null;
  box.querySelectorAll('input[name=insKind]').forEach((r) => r.addEventListener("change", () => {
    const isFile = box.querySelector('input[name=insKind]:checked').value === "file";
    box.querySelector("#insBlankOpts").style.display = isFile ? "none" : "";
    box.querySelector("#insFileOpts").style.display = isFile ? "" : "none";
  }));
  box.querySelector("#insPaper").addEventListener("change", (e) => {
    box.querySelector("#insCustomSize").style.display = e.target.value === "custom" ? "flex" : "none";
  });
  box.querySelector("#insPickFile").addEventListener("click", async () => {
    const p = await invoke("pick_pdf");
    if (p) { insertFile = p; box.querySelector("#insFileName").textContent = shortName(p); }
  });
  box.querySelector("#insCancel").addEventListener("click", closeModal);
  box.querySelector("#insOk").addEventListener("click", async () => {
    const kind = box.querySelector('input[name=insKind]:checked').value;
    const pos = box.querySelector('input[name=insPos]:checked').value;
    let newEntries = [];
    if (kind === "blank") {
      let w = 612, h = 792;
      const paper = box.querySelector("#insPaper").value;
      if (paper === "custom") {
        w = Number(box.querySelector("#insW").value) || 612;
        h = Number(box.querySelector("#insH").value) || 792;
      } else {
        [w, h] = paper.split("x").map(Number);
      }
      newEntries = [{ kind: "blank", widthPt: w, heightPt: h, rotationDelta: 0, crop: null }];
    } else {
      if (!insertFile) { box.querySelector("#insErr").textContent = "Hãy chọn file nguồn."; return; }
      let count;
      try {
        const meta = await invoke("open_document", { path: insertFile });
        count = meta.pageCount;
      } catch (e) {
        box.querySelector("#insErr").textContent = "Không mở được file: " + e;
        return;
      }
      const rangeStr = box.querySelector("#insRange").value.trim();
      const indices = rangeStr ? parsePageRange(rangeStr, count) : Array.from({ length: count }, (_, i) => i);
      if (!indices.length) { box.querySelector("#insErr").textContent = "Phạm vi trang không hợp lệ."; return; }
      newEntries = indices.map((idx) => ({ kind: "existing", source: insertFile, srcIndex: idx, rotationDelta: 0, crop: null }));
    }
    let at = state.pagePlan.length;
    if (pos === "before") at = Math.min(...state.orgSelected);
    else if (pos === "after") at = Math.max(...state.orgSelected) + 1;
    pushUndo();
    state.pagePlan.splice(at, 0, ...newEntries);
    state.orgSelected = new Set();
    buildOrganizeGrid();
    closeModal();
  });
}

function openExtractDialog() {
  if (!state.orgSelected.size) { $("organizeHint").textContent = "Chưa chọn trang nào để trích"; return; }
  const box = openModal("Trích trang", `
    <p>Trích ${state.orgSelected.size} trang đã chọn ra file PDF mới.</p>
    <label><input type="checkbox" id="extDeleteAfter"> Xoá các trang này khỏi tài liệu sau khi trích</label>
    <div class="err" id="extErr"></div>
    <div class="foot"><button id="extCancel">Huỷ</button><button id="extOk" class="primary">Trích…</button></div>
  `);
  box.querySelector("#extCancel").addEventListener("click", closeModal);
  box.querySelector("#extOk").addEventListener("click", async () => {
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    const indices = Array.from(state.orgSelected).sort((a, b) => a - b);
    const plan = indices.map((i) => state.pagePlan[i]);
    try {
      await invoke("organize_apply", { mainInput: state.path, plan, output: out, password: null });
      if (box.querySelector("#extDeleteAfter").checked) {
        pushUndo();
        state.pagePlan = state.pagePlan.filter((_, i) => !state.orgSelected.has(i));
        state.orgSelected = new Set();
        buildOrganizeGrid();
      }
      $("organizeHint").textContent = `Đã trích ra ${shortName(out)}`;
      closeModal();
    } catch (e) {
      box.querySelector("#extErr").textContent = "Lỗi: " + e;
    }
  });
}

function openReplaceDialog() {
  if (!state.orgSelected.size) { $("organizeHint").textContent = "Chưa chọn trang nào để thay"; return; }
  const box = openModal("Thay trang", `
    <p>Thay nội dung ${state.orgSelected.size} trang đã chọn bằng trang từ file khác.</p>
    <label>File nguồn</label>
    <button id="repPickFile" type="button">📂 Chọn file…</button>
    <span id="repFileName" class="status"></span>
    <label>Trang nguồn (vd 1-3,5 — rỗng = tất cả)</label>
    <input type="text" id="repRange" placeholder="tất cả">
    <div class="err" id="repErr"></div>
    <div class="foot"><button id="repCancel">Huỷ</button><button id="repOk" class="primary">Thay</button></div>
  `);
  let file = null;
  box.querySelector("#repPickFile").addEventListener("click", async () => {
    const p = await invoke("pick_pdf");
    if (p) { file = p; box.querySelector("#repFileName").textContent = shortName(p); }
  });
  box.querySelector("#repCancel").addEventListener("click", closeModal);
  box.querySelector("#repOk").addEventListener("click", async () => {
    if (!file) { box.querySelector("#repErr").textContent = "Hãy chọn file nguồn."; return; }
    let count;
    try {
      const meta = await invoke("open_document", { path: file });
      count = meta.pageCount;
    } catch (e) {
      box.querySelector("#repErr").textContent = "Không mở được file: " + e;
      return;
    }
    const rangeStr = box.querySelector("#repRange").value.trim();
    const indices = rangeStr ? parsePageRange(rangeStr, count) : Array.from({ length: count }, (_, i) => i);
    if (!indices.length) { box.querySelector("#repErr").textContent = "Phạm vi trang không hợp lệ."; return; }
    const newEntries = indices.map((idx) => ({ kind: "existing", source: file, srcIndex: idx, rotationDelta: 0, crop: null }));
    const sorted = Array.from(state.orgSelected).sort((a, b) => a - b);
    const at = sorted[0];
    pushUndo();
    state.pagePlan = state.pagePlan.filter((_, i) => !state.orgSelected.has(i));
    state.pagePlan.splice(Math.min(at, state.pagePlan.length), 0, ...newEntries);
    state.orgSelected = new Set();
    buildOrganizeGrid();
    closeModal();
  });
}

function openMergeDialog() {
  let files = [];
  const box = openModal("Trộn file PDF", `
    <button id="mrgAdd" type="button">➕ Thêm file…</button>
    <div id="mrgList" style="margin-top:8px;"></div>
    <div class="err" id="mrgErr"></div>
    <div class="foot"><button id="mrgCancel">Huỷ</button><button id="mrgOk" class="primary">Trộn…</button></div>
  `);
  function renderList() {
    box.querySelector("#mrgList").innerHTML = files.map((f, i) =>
      `<div class="row" style="margin-bottom:4px;align-items:center;">` +
      `<span style="flex:3">${i + 1}. ${shortName(f)}</span>` +
      `<button data-up="${i}" type="button" ${i === 0 ? "disabled" : ""}>↑</button>` +
      `<button data-down="${i}" type="button" ${i === files.length - 1 ? "disabled" : ""}>↓</button>` +
      `<button data-rm="${i}" type="button">✕</button></div>`
    ).join("");
  }
  box.querySelector("#mrgAdd").addEventListener("click", async () => {
    const p = await invoke("pick_pdf");
    if (p) { files.push(p); renderList(); }
  });
  box.querySelector("#mrgList").addEventListener("click", (e) => {
    const t = e.target;
    if (t.dataset.up != null) {
      const i = Number(t.dataset.up);
      [files[i - 1], files[i]] = [files[i], files[i - 1]];
      renderList();
    } else if (t.dataset.down != null) {
      const i = Number(t.dataset.down);
      [files[i + 1], files[i]] = [files[i], files[i + 1]];
      renderList();
    } else if (t.dataset.rm != null) {
      files.splice(Number(t.dataset.rm), 1);
      renderList();
    }
  });
  box.querySelector("#mrgCancel").addEventListener("click", closeModal);
  box.querySelector("#mrgOk").addEventListener("click", async () => {
    if (files.length < 1) { box.querySelector("#mrgErr").textContent = "Hãy thêm ít nhất 1 file."; return; }
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    try {
      await invoke("organize_merge", { files, output: out });
      $("status").textContent = `Đã trộn ${files.length} file → ${shortName(out)}`;
      closeModal();
    } catch (e) {
      box.querySelector("#mrgErr").textContent = "Lỗi: " + e;
    }
  });
  renderList();
}

function openSplitDialog() {
  const box = openModal("Tách file PDF", `
    <label>Mỗi file tối đa (số trang)</label>
    <input type="number" id="splitN" value="1" min="1">
    <div class="err" id="splitErr"></div>
    <div class="foot"><button id="splitCancel">Huỷ</button><button id="splitOk" class="primary">Tách…</button></div>
  `);
  box.querySelector("#splitCancel").addEventListener("click", closeModal);
  box.querySelector("#splitOk").addEventListener("click", async () => {
    const n = Math.max(1, Number(box.querySelector("#splitN").value) || 1);
    const outDir = await invoke("pick_dir");
    if (!outDir) return;
    try {
      const base = shortName(state.path).replace(/\.pdf$/i, "");
      const outs = await invoke("organize_split", {
        input: state.path, pagesPerFile: n, outDir, baseName: base, password: null,
      });
      $("status").textContent = `Đã tách thành ${outs.length} file vào ${outDir}`;
      closeModal();
    } catch (e) {
      box.querySelector("#splitErr").textContent = "Lỗi: " + e;
    }
  });
}

async function openWatermarkDialog() {
  const base = await materializeBaseInput();
  const previewPage = base.isTemp ? (state.orgSelected.size ? Math.min(...state.orgSelected) : 0) : state.current;
  const box = openModal("Watermark", `
    ${base.isTemp ? MATERIALIZED_NOTE : ""}
    <label>Nội dung</label>
    <input type="text" id="wmText" value="CONFIDENTIAL">
    <div class="row">
      <div><label>Cỡ chữ</label><input type="number" id="wmSize" value="36"></div>
      <div><label>Màu (r,g,b)</label><input type="text" id="wmColor" value="200,0,0"></div>
      <div><label>Độ mờ (0-255)</label><input type="number" id="wmAlpha" value="120" min="0" max="255"></div>
    </div>
    <div class="row">
      <label><input type="checkbox" id="wmBold"> Đậm</label>
      <label><input type="checkbox" id="wmItalic"> Nghiêng</label>
    </div>
    <label>Góc xoay (độ)</label>
    <input type="number" id="wmRotate" value="45">
    <label>Vị trí</label>
    <div class="anchor9" id="wmAnchor">
      ${["top-left", "top-center", "top-right", "middle-left", "center", "middle-right", "bottom-left", "bottom-center", "bottom-right"]
        .map((a) => `<button type="button" data-a="${a}" class="${a === "center" ? "cur" : ""}">●</button>`).join("")}
    </div>
    <label>Trang áp dụng (rỗng = tất cả, vd 1-3,5)</label>
    <input type="text" id="wmPages" placeholder="tất cả">
    <div class="foot">
      <button id="wmPreview" type="button">👁 Xem trước</button>
      <button id="wmCancel">Huỷ</button><button id="wmOk" class="primary">Áp dụng</button>
    </div>
    <div class="err" id="wmErr"></div>
  `);
  let anchor = "center";
  box.querySelector("#wmAnchor").addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-a]");
    if (!btn) return;
    anchor = btn.dataset.a;
    box.querySelectorAll("#wmAnchor button").forEach((b) => b.classList.toggle("cur", b === btn));
  });
  function buildSpec() {
    const [r, g, b] = box.querySelector("#wmColor").value.split(",").map((x) => Number(x.trim()) || 0);
    return {
      text: box.querySelector("#wmText").value || "",
      fontSize: Number(box.querySelector("#wmSize").value) || 36,
      color: [r, g, b, Number(box.querySelector("#wmAlpha").value) || 120],
      bold: box.querySelector("#wmBold").checked,
      italic: box.querySelector("#wmItalic").checked,
      rotationDeg: Number(box.querySelector("#wmRotate").value) || 0,
      anchor,
      pages: box.querySelector("#wmPages").value.trim()
        ? parsePageRange(box.querySelector("#wmPages").value.trim(), base.pageCount)
        : [],
    };
  }
  box.querySelector("#wmPreview").addEventListener("click", async () => {
    try {
      const url = await invoke("preview_watermark", { input: base.path, page: previewPage, spec: buildSpec(), width: 500 });
      let img = box.querySelector("#wmPreviewImg");
      if (!img) {
        img = document.createElement("img");
        img.id = "wmPreviewImg";
        img.style.maxWidth = "100%";
        img.style.marginTop = "8px";
        box.insertBefore(img, box.querySelector(".foot"));
      }
      img.src = url;
    } catch (e) {
      box.querySelector("#wmErr").textContent = "Lỗi xem trước: " + e;
    }
  });
  box.querySelector("#wmCancel").addEventListener("click", closeModal);
  box.querySelector("#wmOk").addEventListener("click", async () => {
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    try {
      await invoke("watermark_add", { input: base.path, spec: buildSpec(), output: out, password: null });
      $("status").textContent = `Đã thêm watermark → ${shortName(out)}`;
      closeModal();
      loadDocument(out);
    } catch (e) {
      box.querySelector("#wmErr").textContent = "Lỗi: " + e;
    }
  });
}

async function openHeaderFooterDialog() {
  const base = await materializeBaseInput();
  const previewPage = base.isTemp ? (state.orgSelected.size ? Math.min(...state.orgSelected) : 0) : state.current;
  const box = openModal("Header / Footer", `
    ${base.isTemp ? MATERIALIZED_NOTE : ""}
    <div class="row">
      <div><label>Trên-trái</label><input type="text" id="hfTL"></div>
      <div><label>Trên-giữa</label><input type="text" id="hfTC"></div>
      <div><label>Trên-phải</label><input type="text" id="hfTR"></div>
    </div>
    <div class="row">
      <div><label>Dưới-trái</label><input type="text" id="hfBL"></div>
      <div><label>Dưới-giữa</label><input type="text" id="hfBC" value="Trang {page}/{total}"></div>
      <div><label>Dưới-phải</label><input type="text" id="hfBR"></div>
    </div>
    <p class="status">Chèn vào ô đang focus:
      <button type="button" data-tok="{page}">{page}</button>
      <button type="button" data-tok="{total}">{total}</button>
      <button type="button" data-tok="{date}">{date}</button>
    </p>
    <div class="row">
      <div><label>Cỡ chữ</label><input type="number" id="hfSize" value="10"></div>
      <div><label>Lề (pt)</label><input type="number" id="hfMargin" value="20"></div>
      <div><label>Màu (r,g,b)</label><input type="text" id="hfColor" value="0,0,0"></div>
    </div>
    <label>Trang áp dụng (rỗng = tất cả)</label>
    <input type="text" id="hfPages" placeholder="tất cả">
    <div class="foot">
      <button id="hfPreview" type="button">👁 Xem trước</button>
      <button id="hfCancel">Huỷ</button><button id="hfOk" class="primary">Áp dụng</button>
    </div>
    <div class="err" id="hfErr"></div>
  `);
  let lastFocused = box.querySelector("#hfBC");
  box.querySelectorAll("input[type=text]").forEach((inp) => inp.addEventListener("focus", () => { lastFocused = inp; }));
  box.querySelectorAll("button[data-tok]").forEach((btn) => btn.addEventListener("click", () => {
    if (lastFocused) { lastFocused.value += btn.dataset.tok; lastFocused.focus(); }
  }));
  function buildSpec() {
    const [r, g, b] = box.querySelector("#hfColor").value.split(",").map((x) => Number(x.trim()) || 0);
    return {
      topLeft: box.querySelector("#hfTL").value,
      topCenter: box.querySelector("#hfTC").value,
      topRight: box.querySelector("#hfTR").value,
      bottomLeft: box.querySelector("#hfBL").value,
      bottomCenter: box.querySelector("#hfBC").value,
      bottomRight: box.querySelector("#hfBR").value,
      fontSize: Number(box.querySelector("#hfSize").value) || 10,
      color: [r, g, b, 255],
      marginPt: Number(box.querySelector("#hfMargin").value) || 20,
      date: new Date().toLocaleDateString("vi-VN"),
      pages: box.querySelector("#hfPages").value.trim()
        ? parsePageRange(box.querySelector("#hfPages").value.trim(), base.pageCount)
        : [],
    };
  }
  box.querySelector("#hfPreview").addEventListener("click", async () => {
    try {
      const url = await invoke("preview_header_footer", { input: base.path, page: previewPage, spec: buildSpec(), width: 500 });
      let img = box.querySelector("#hfPreviewImg");
      if (!img) {
        img = document.createElement("img");
        img.id = "hfPreviewImg";
        img.style.maxWidth = "100%";
        img.style.marginTop = "8px";
        box.insertBefore(img, box.querySelector(".foot"));
      }
      img.src = url;
    } catch (e) {
      box.querySelector("#hfErr").textContent = "Lỗi xem trước: " + e;
    }
  });
  box.querySelector("#hfCancel").addEventListener("click", closeModal);
  box.querySelector("#hfOk").addEventListener("click", async () => {
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    try {
      await invoke("header_footer_add", { input: base.path, spec: buildSpec(), output: out, password: null });
      $("status").textContent = `Đã thêm header/footer → ${shortName(out)}`;
      closeModal();
      loadDocument(out);
    } catch (e) {
      box.querySelector("#hfErr").textContent = "Lỗi: " + e;
    }
  });
}

// Crop: tái dùng cơ chế kéo-vẽ hình chữ nhật đã có cho tool "square" — xem
// nhánh `tool === "crop"` trong onPagesMouseUp. Tìm theo `srcIndex` (không
// theo vị trí trong pagePlan) vì viewer luôn hiển thị trang theo số trang GỐC
// của file đang mở, bất kể pagePlan đã bị đảo/chèn/xoá hay chưa.
function openCropDialog(pageIdx, rectPdf) {
  const p = state.pages[pageIdx];
  const m = {
    left: rectPdf.left,
    bottom: rectPdf.bottom,
    right: p.widthPt - rectPdf.right,
    top: p.heightPt - rectPdf.top,
  };
  const box = openModal("Crop trang", `
    <div class="row">
      <div><label>Trái (pt)</label><input type="number" id="cropL" value="${m.left.toFixed(1)}"></div>
      <div><label>Phải (pt)</label><input type="number" id="cropR" value="${m.right.toFixed(1)}"></div>
    </div>
    <div class="row">
      <div><label>Trên (pt)</label><input type="number" id="cropT" value="${m.top.toFixed(1)}"></div>
      <div><label>Dưới (pt)</label><input type="number" id="cropB" value="${m.bottom.toFixed(1)}"></div>
    </div>
    <div class="radiorow">
      <label><input type="radio" name="cropScope" value="this" checked> Trang này</label>
      <label><input type="radio" name="cropScope" value="all"> Tất cả trang</label>
    </div>
    <p class="status">Áp dụng ngay vào kế hoạch tổ chức trang — vào "🗂 Tổ chức trang" → "💾 Lưu thay đổi" để ghi ra file thật.</p>
    <div class="foot"><button id="cropCancel">Huỷ</button><button id="cropOk" class="primary">Áp dụng</button></div>
  `);
  box.querySelector("#cropCancel").addEventListener("click", () => {
    closeModal();
    drawAnnotsForPage(pageIdx);
  });
  box.querySelector("#cropOk").addEventListener("click", () => {
    const l = Number(box.querySelector("#cropL").value) || 0;
    const r = Number(box.querySelector("#cropR").value) || 0;
    const t = Number(box.querySelector("#cropT").value) || 0;
    const b = Number(box.querySelector("#cropB").value) || 0;
    const scope = box.querySelector('input[name=cropScope]:checked').value;
    pushUndo();
    for (const entry of state.pagePlan) {
      const isThis = !entry.source && entry.srcIndex === pageIdx;
      if (scope === "all" || isThis) {
        const dims = (!entry.source && state.pages[entry.srcIndex]) ? state.pages[entry.srcIndex] : p;
        entry.crop = { left: l, bottom: b, right: dims.widthPt - r, top: dims.heightPt - t };
      }
    }
    closeModal();
    setTool(null);
    drawAnnotsForPage(pageIdx);
    $("status").textContent = "Đã đặt vùng crop — vào Tổ chức trang > Lưu để ghi ra file thật.";
  });
}

// ---------- Phase 4: Sửa nội dung (Edit) ----------
// Mô hình "materialize tức thì": mỗi thao tác (sửa text / xoá / thêm / di
// chuyển) được áp NGAY vào 1 file tạm mới (editBase) qua edit_apply, rồi đọc
// lại object + render ảnh từ file đó. Nhờ vậy: index object luôn khớp ảnh đang
// hiện (WYSIWYG thật), không phải tự suy đoán; undo = quay lại file tạm trước.

const EDIT_STAGE_W = 820; // px bề rộng ảnh trang khi sửa

function enterEditMode() {
  if (!state.path) { $("status").textContent = "Hãy mở file trước khi sửa nội dung"; return; }
  if (state.organizeMode) exitOrganizeMode();
  state.editMode = true;
  state.editPage = state.current;
  state.editBase = state.path;
  state.editUndo = [];
  state.editRedo = [];
  state.editTemps = [];
  state.editSel = null;
  state.editArm = null;
  state.editPendingImage = null;
  $("annobar").classList.add("hidden");
  $("editBar").classList.remove("hidden");
  $("viewport").classList.add("hidden");
  $("editStage").classList.remove("hidden");
  $("sidebar").classList.add("hidden");
  $("editModeBtn").classList.add("active");
  setTool(null);
  loadEditPage();
}
// Dọn mọi file làm việc tạm của phiên sửa (backend chỉ xoá đúng ff_edit_*.pdf
// trong thư mục temp nên gọi thoải mái).
function cleanupEditTemps() {
  if (state.editTemps.length) {
    invoke("edit_cleanup", { paths: state.editTemps.slice() }).catch(() => {});
    state.editTemps = [];
  }
}

function exitEditMode() {
  cleanupEditTemps();
  state.editMode = false;
  $("annobar").classList.remove("hidden");
  $("editBar").classList.add("hidden");
  $("viewport").classList.remove("hidden");
  $("editStage").classList.add("hidden");
  $("sidebar").classList.remove("hidden");
  $("editModeBtn").classList.remove("active");
  $("editOverlay").innerHTML = "";
}
function toggleEditMode() {
  if (state.editMode) exitEditMode();
  else enterEditMode();
}

// Đọc lại object + render ảnh trang hiện tại từ editBase, dựng overlay box.
async function loadEditPage() {
  const p = state.pages[state.editPage];
  state.editScale = EDIT_STAGE_W / p.widthPt;
  try {
    const [url, objs] = await Promise.all([
      invoke("render_page", { path: state.editBase, page: state.editPage, width: EDIT_STAGE_W }),
      invoke("edit_list_objects", { path: state.editBase, page: state.editPage, password: null }),
    ]);
    $("editImg").src = url;
    state.editObjects = objs;
    buildEditOverlay();
    $("editHint").textContent = `Trang ${state.editPage + 1} · ${objs.length} đối tượng`;
  } catch (e) {
    $("editHint").textContent = "Lỗi nạp trang sửa: " + e;
  }
  $("edSave").disabled = state.editBase === state.path; // chưa có thay đổi nào
  updateEditUndoButtons();
}

function editBoxStyle(rect) {
  const s = state.editScale;
  const p = state.pages[state.editPage];
  return {
    left: rect.left * s + "px",
    top: (p.heightPt - rect.top) * s + "px",
    width: Math.max(2, (rect.right - rect.left) * s) + "px",
    height: Math.max(2, (rect.top - rect.bottom) * s) + "px",
  };
}

function buildEditOverlay() {
  const ov = $("editOverlay");
  ov.innerHTML = "";
  ov.classList.toggle("armed", !!state.editArm);
  const img = $("editImg");
  // Khớp kích thước overlay với ảnh thực tế hiển thị.
  state.editObjects.forEach((o) => {
    if (o.kind === "path" || o.kind === "shading") return; // path/shading: chưa cho chọn ở v1
    const box = document.createElement("div");
    box.className = "edit-box kind-" + o.kind + (state.editSel === o.index ? " selected" : "");
    box.dataset.index = String(o.index);
    Object.assign(box.style, editBoxStyle(o.rect));
    box.title = o.kind === "text" ? (o.text || "") : o.kind;
    box.addEventListener("mousedown", (e) => onEditBoxMouseDown(e, o));
    box.addEventListener("click", (e) => {
      if (state.editArm) return; // đang chờ đặt chữ/ảnh → để click rơi xuống overlay
      e.stopPropagation();
      selectEditObject(o.index);
    });
    if (o.kind === "text") {
      box.addEventListener("dblclick", (e) => { e.stopPropagation(); startTextEdit(o); });
    }
    ov.appendChild(box);
  });
  refreshEditSelection();
  void img;
}

// Cập nhật trạng thái chọn TẠI CHỖ (không dựng lại overlay) — quan trọng để
// double-click không bị mất element giữa 2 lần click.
function refreshEditSelection() {
  const ov = $("editOverlay");
  ov.querySelectorAll(".edit-box").forEach((box) => {
    const isSel = Number(box.dataset.index) === state.editSel;
    box.classList.toggle("selected", isSel);
    const existing = box.querySelector(".ed-handle");
    if (isSel && !existing) {
      const o = state.editObjects.find((x) => x.index === state.editSel);
      const h = document.createElement("div");
      h.className = "ed-handle";
      h.addEventListener("mousedown", (e) => onEditResizeMouseDown(e, o));
      box.appendChild(h);
    } else if (!isSel && existing) {
      existing.remove();
    }
  });
}

function selectEditObject(index) {
  state.editSel = index != null && index >= 0 ? index : null;
  const o = state.editObjects.find((x) => x.index === state.editSel);
  const isText = o && o.kind === "text";
  const isImage = o && o.kind === "image";
  $("edDelete").disabled = !o;
  $("edReplaceImage").disabled = !isImage;
  $("edFontSize").disabled = !isText;
  $("edColorBtn").disabled = !isText;
  $("edFontFamily").disabled = !isText;
  $("edBold").disabled = !isText;
  $("edItalic").disabled = !isText;
  if (isText) {
    $("edFontSize").value = Math.round(o.font_size || 12);
    if (o.color) { state.editColor = o.color.slice(0, 3); $("edSw").style.background = rgbCss(state.editColor); }
    // Ô font: mặc định "(giữ nguyên: <family gốc>)" — chỉ đổi khi người dùng chọn khác.
    $("edFontFamily").options[0].textContent = o.font_family
      ? `(giữ nguyên: ${o.font_family})`
      : "(giữ nguyên)";
    $("edFontFamily").value = "";
    $("edBold").classList.toggle("on", !!o.font_bold);
    $("edItalic").classList.toggle("on", !!o.font_italic);
    const emb = o.font_embedded == null ? "" : o.font_embedded ? " · font nhúng" : " · font hệ thống";
    $("editHint").textContent =
      `${o.font_family || o.font_name || "?"} · ${Math.round(o.font_size || 0)}pt${emb}`;
  } else {
    $("edFontFamily").options[0].textContent = "(giữ nguyên)";
    $("edBold").classList.remove("on");
    $("edItalic").classList.remove("on");
  }
  refreshEditSelection();
}

function updateEditUndoButtons() {
  // Dùng chung 2 nút Hoàn tác/Làm lại toàn cục khi đang ở chế độ sửa.
  if (!state.editMode) return;
  $("undoBtn").disabled = state.editUndo.length === 0;
  $("redoBtn").disabled = state.editRedo.length === 0;
}

// Áp 1 nhóm op (1 thao tác người dùng): lưu editBase cũ vào stack undo,
// materialize ra file tạm mới, ghi nhận file tạm để dọn khi thoát.
async function stageEditOps(ops) {
  try {
    const out = await invoke("edit_apply_to_temp", {
      input: state.editBase, page: state.editPage, ops, password: null,
    });
    state.editTemps.push(out);
    state.editUndo.push(state.editBase);
    state.editRedo = [];
    state.editBase = out;
    state.editSel = null;
    selectEditObject(-1);
    await loadEditPage();
  } catch (e) {
    $("editHint").textContent = "Lỗi: " + e;
  }
}

function stageEditOp(op) {
  return stageEditOps([op]);
}

function editUndo() {
  if (!state.editUndo.length) return;
  state.editRedo.push(state.editBase);
  state.editBase = state.editUndo.pop();
  state.editSel = null;
  loadEditPage();
}
function editRedo() {
  if (!state.editRedo.length) return;
  state.editUndo.push(state.editBase);
  state.editBase = state.editRedo.pop();
  state.editSel = null;
  loadEditPage();
}

// CSS font-family xấp xỉ cho family PDF (để ô sửa hiển thị đúng dáng chữ).
function cssFontStack(family) {
  if (!family) return "sans-serif";
  const k = family.toLowerCase();
  const generic = /times|georgia|garamond|palatino|cambria|book|serif/.test(k)
    ? "serif"
    : /courier|consolas|mono/.test(k)
      ? "monospace"
      : "sans-serif";
  return `"${family}", ${generic}`;
}

// Gom các text run CÙNG DÒNG với `o` (baseline xấp xỉ, liền kề theo chiều
// ngang) — PDF hay cắt 1 dòng nhìn thấy thành nhiều run; Foxit cho sửa cả
// dòng nên ta cũng vậy. Trả về mảng run đã xếp trái→phải (luôn chứa `o`).
function sameLineRuns(o) {
  const fs = o.font_size || 12;
  const tol = Math.max(1.5, fs * 0.25);
  const cands = state.editObjects
    .filter(
      (x) =>
        x.kind === "text" &&
        Math.abs(x.rect.bottom - o.rect.bottom) <= tol &&
        (x.font_size || 12) <= fs * 2.2 &&
        (x.font_size || 12) >= fs / 2.2
    )
    .sort((a, b) => a.rect.left - b.rect.left);
  const i0 = cands.findIndex((x) => x.index === o.index);
  if (i0 < 0) return [o];
  const gapMax = Math.max(fs * 1.2, 6); // hở quá 1.2em coi như cột khác
  const runs = [cands[i0]];
  for (let i = i0 - 1; i >= 0; i--) {
    if (runs[0].rect.left - cands[i].rect.right <= gapMax) runs.unshift(cands[i]);
    else break;
  }
  for (let i = i0 + 1; i < cands.length; i++) {
    if (cands[i].rect.left - runs[runs.length - 1].rect.right <= gapMax) runs.push(cands[i]);
    else break;
  }
  return runs;
}

// Ghép nội dung 1 dòng từ các run: chèn khoảng trắng khi 2 run hở nhau rõ.
function composeLineText(runs) {
  let s = "";
  for (let i = 0; i < runs.length; i++) {
    const t = runs[i].text || "";
    if (i > 0) {
      const gap = runs[i].rect.left - runs[i - 1].rect.right;
      const em = (runs[i].font_size || 12) * 0.28;
      if (gap > em && !s.endsWith(" ") && !t.startsWith(" ")) s += " ";
    }
    s += t;
  }
  return s;
}

// Gom ĐOẠN VĂN nhiều dòng quanh run `o` (iteration 3): các dòng baseline cách
// đều (±25%), cỡ chữ tương đồng, giao nhau theo chiều ngang. Trả về mảng dòng
// trên→dưới, mỗi dòng = mảng run trái→phải.
function paragraphLines(o) {
  const fs = o.font_size || 12;
  const boundsOf = (runs) => ({
    left: Math.min(...runs.map((r) => r.rect.left)),
    right: Math.max(...runs.map((r) => r.rect.right)),
    bottom: runs[0].rect.bottom,
  });
  const overlaps = (a, b) =>
    Math.min(a.right, b.right) - Math.max(a.left, b.left) >
    0.3 * Math.min(a.right - a.left, b.right - b.left);

  const base = sameLineRuns(o);
  const used = new Set(base.map((r) => r.index));
  const lines = [base];
  const cands = state.editObjects.filter(
    (x) =>
      x.kind === "text" &&
      !used.has(x.index) &&
      (x.font_size || 12) <= fs * 1.6 &&
      (x.font_size || 12) >= fs / 1.6
  );
  let advance = null; // khoảng baseline đã chốt của đoạn

  const extend = (dir) => {
    // dir = -1: mở rộng XUỐNG (bottom giảm); +1: mở rộng LÊN.
    for (;;) {
      const edge = dir < 0 ? lines[lines.length - 1] : lines[0];
      const eb = boundsOf(edge);
      let best = null;
      let bestGap = Infinity;
      for (const r of cands) {
        if (used.has(r.index)) continue;
        const gap = dir < 0 ? eb.bottom - r.rect.bottom : r.rect.bottom - eb.bottom;
        if (gap < fs * 0.6 || gap > fs * 2.6) continue; // quá sát/quá xa = không cùng đoạn
        if (advance && (gap < advance * 0.75 || gap > advance * 1.25)) continue;
        const line = sameLineRuns(r);
        if (!overlaps(eb, boundsOf(line))) continue;
        if (gap < bestGap) {
          bestGap = gap;
          best = line;
        }
      }
      if (!best) break;
      best.forEach((r) => used.add(r.index));
      if (dir < 0) lines.push(best);
      else lines.unshift(best);
      if (advance == null) advance = bestGap;
    }
  };
  extend(-1);
  extend(+1);
  return lines;
}

// Double-click: đoạn ≥2 dòng → sửa CẢ ĐOẠN với reflow; 1 dòng → sửa dòng.
function startTextEdit(o) {
  const lines = paragraphLines(o);
  if (lines.length >= 2) startBlockTextEdit(o, lines);
  else startInlineTextEdit(o);
}

// Sửa cả đoạn "như Word" (Foxit Edit Text): textarea phủ khối, gõ tự chảy
// dòng; commit → engine reflow (bẻ dòng lại theo bề rộng khối, giữ font).
function startBlockTextEdit(o, lines) {
  const ov = $("editOverlay");
  const existing = ov.querySelector(".edit-inline");
  if (existing) existing.remove();

  const allRuns = lines.flat();
  const union = {
    left: Math.min(...allRuns.map((r) => r.rect.left)),
    right: Math.max(...allRuns.map((r) => r.rect.right)),
    bottom: Math.min(...allRuns.map((r) => r.rect.bottom)),
    top: Math.max(...allRuns.map((r) => r.rect.top)),
  };
  // Đoạn = 1 dòng chảy; Enter của người dùng = ngắt cứng.
  const original = lines.map(composeLineText).join(" ");

  const ta = document.createElement("textarea");
  ta.className = "edit-inline edit-inline-block";
  ta.value = original;
  Object.assign(ta.style, editBoxStyle(union));
  const advPt =
    lines.length > 1
      ? lines[0][0].rect.bottom - lines[1][0].rect.bottom
      : (o.font_size || 12) * 1.25;
  const advPx = Math.max(8, advPt * state.editScale);
  // Chừa 2 dòng trống để gõ thêm.
  ta.style.height = parseFloat(ta.style.height) + 2 * advPx + "px";
  ta.style.lineHeight = advPx + "px";
  ta.style.fontSize = Math.max(8, (o.font_size || 12) * state.editScale) + "px";
  ta.style.fontFamily = cssFontStack(o.font_family);
  ta.style.fontWeight = o.font_bold ? "bold" : "normal";
  ta.style.fontStyle = o.font_italic ? "italic" : "normal";
  if (o.color) ta.style.color = rgbCss(o.color);
  ov.appendChild(ta);
  ta.focus();
  ta.setSelectionRange(0, 0);
  $("editHint").textContent =
    "Sửa cả đoạn — Enter: xuống dòng cứng · Ctrl+Enter hoặc bấm ra ngoài: áp dụng · Esc: huỷ";

  let done = false;
  const commit = (save) => {
    if (done) return;
    done = true;
    const text = ta.value;
    ta.remove();
    if (save && text.trim() && text !== original) {
      stageEditOp({
        op: "reflowText",
        indices: allRuns.map((r) => r.index),
        text,
      });
    } else {
      $("editHint").textContent = "";
    }
  };
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { e.preventDefault(); commit(false); }
    else if (e.key === "Enter" && e.ctrlKey) { e.preventDefault(); commit(true); }
    e.stopPropagation();
  });
  ta.addEventListener("blur", () => commit(true));
}

// Sửa text tại chỗ kiểu Foxit: gộp cả dòng, ô sửa hiển thị đúng font/cỡ/màu,
// Enter/blur commit — engine GIỮ FONT GỐC (chỉ thay khi thiếu glyph).
function startInlineTextEdit(o) {
  const ov = $("editOverlay");
  const existing = ov.querySelector(".edit-inline");
  if (existing) existing.remove();

  const runs = sameLineRuns(o);
  const original = composeLineText(runs);
  const union = {
    left: Math.min(...runs.map((r) => r.rect.left)),
    right: Math.max(...runs.map((r) => r.rect.right)),
    bottom: Math.min(...runs.map((r) => r.rect.bottom)),
    top: Math.max(...runs.map((r) => r.rect.top)),
  };

  const inp = document.createElement("input");
  inp.className = "edit-inline";
  inp.value = original;
  const st = editBoxStyle(union);
  Object.assign(inp.style, st);
  // Chừa chỗ gõ thêm về bên phải (không tràn khung trang).
  const p = state.pages[state.editPage];
  const maxW = (p.widthPt - union.left) * state.editScale - 4;
  inp.style.width = Math.min(maxW, parseFloat(st.width) + 220) + "px";
  // WYSIWYG khi gõ: đúng font/cỡ/màu/kiểu của run đầu.
  inp.style.fontSize = Math.max(8, (o.font_size || 12) * state.editScale) + "px";
  inp.style.fontFamily = cssFontStack(o.font_family);
  inp.style.fontWeight = o.font_bold ? "bold" : "normal";
  inp.style.fontStyle = o.font_italic ? "italic" : "normal";
  if (o.color) inp.style.color = rgbCss(o.color);
  ov.appendChild(inp);
  inp.focus();
  inp.select();

  let done = false;
  const commit = (save) => {
    if (done) return;
    done = true;
    const text = inp.value;
    inp.remove();
    if (save && text !== original) {
      // Run đầu nhận toàn bộ text mới (giữ font/cỡ/màu gốc — mọi field null),
      // các run còn lại của dòng bị xoá.
      const ops = [
        {
          op: "setText",
          index: runs[0].index,
          text,
          fontSize: null,
          color: null,
          fontFamily: null,
          bold: null,
          italic: null,
        },
        ...runs.slice(1).map((r) => ({ op: "delete", index: r.index })),
      ];
      stageEditOps(ops);
    }
  };
  inp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); commit(true); }
    else if (e.key === "Escape") { e.preventDefault(); commit(false); }
    e.stopPropagation();
  });
  inp.addEventListener("blur", () => commit(true));
}

// Đổi thuộc tính chữ cho run đang chọn. `part` chỉ chứa field muốn đổi —
// mọi field khác gửi null = GIỮ NGUYÊN (engine không đụng font khi không cần).
function applyTextPropToSelected(part) {
  const o = state.editObjects.find((x) => x.index === state.editSel);
  if (!o || o.kind !== "text") return;
  stageEditOp({
    op: "setText",
    index: o.index,
    text: o.text || "",
    fontSize: null,
    color: null,
    fontFamily: null,
    bold: null,
    italic: null,
    ...part,
  });
}

function deleteSelectedEditObject() {
  if (state.editSel == null) return;
  stageEditOp({ op: "delete", index: state.editSel });
}

// Click lên trang khi đang "armed" để đặt chữ/ảnh mới.
function onEditStageClick(e) {
  if (!state.editArm) { selectEditObject(-1); state.editSel = null; buildEditOverlay(); return; }
  const img = $("editImg");
  const r = img.getBoundingClientRect();
  const cssX = e.clientX - r.left;
  const cssY = e.clientY - r.top;
  const p = state.pages[state.editPage];
  const pdfX = cssX / state.editScale;
  const pdfY = p.heightPt - cssY / state.editScale;
  if (state.editArm === "text") {
    state.editArm = null;
    $("edAddText").classList.remove("armed");
    promptAddText(pdfX, pdfY);
  } else if (state.editArm === "image") {
    const path = state.editPendingImage;
    state.editArm = null;
    state.editPendingImage = null;
    $("edAddImage").classList.remove("armed");
    if (path) {
      // Khung mặc định 150×112pt (4:3) — người dùng kéo handle để chỉnh lại.
      stageEditOp({ op: "addImage", x: pdfX, y: pdfY - 112, widthPt: 150, heightPt: 112, imagePath: path });
    }
  }
  $("editOverlay").classList.remove("armed");
}

function promptAddText(pdfX, pdfY) {
  const ov = $("editOverlay");
  const inp = document.createElement("input");
  inp.className = "edit-inline";
  inp.placeholder = "Nhập chữ…";
  const s = state.editScale;
  const p = state.pages[state.editPage];
  const family = $("edFontFamily").value || null; // null = font mặc định
  const size = Number($("edFontSize").value) || 16;
  inp.style.left = pdfX * s + "px";
  inp.style.top = (p.heightPt - pdfY) * s + "px";
  inp.style.minWidth = "120px";
  inp.style.fontSize = Math.max(10, size * s) + "px";
  inp.style.fontFamily = cssFontStack(family);
  inp.style.color = rgbCss(state.editColor);
  ov.appendChild(inp);
  inp.focus();
  let done = false;
  const commit = (save) => {
    if (done) return; done = true;
    const text = inp.value;
    inp.remove();
    if (save && text.trim()) {
      stageEditOp({
        op: "addText", x: pdfX, y: pdfY, text,
        fontSize: size, color: [state.editColor[0], state.editColor[1], state.editColor[2], 255],
        fontFamily: family,
        bold: null, italic: null,
      });
    }
  };
  inp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); commit(true); }
    else if (e.key === "Escape") { e.preventDefault(); commit(false); }
    e.stopPropagation();
  });
  inp.addEventListener("blur", () => commit(true));
}

// Kéo để di chuyển object đang chọn — box đi theo con trỏ NGAY (live feedback
// như Foxit), thả chuột mới commit vào engine.
function onEditBoxMouseDown(e, o) {
  if (state.editArm) return;
  if (e.target.classList.contains("ed-handle")) return;
  selectEditObject(o.index);
  const box = e.currentTarget;
  const startX = e.clientX, startY = e.clientY;
  const left0 = parseFloat(box.style.left), top0 = parseFloat(box.style.top);
  let moved = false;
  const onMove = (ev) => {
    const dx = ev.clientX - startX, dy = ev.clientY - startY;
    if (!moved && (Math.abs(dx) > 2 || Math.abs(dy) > 2)) {
      moved = true;
      box.classList.add("dragging");
    }
    if (moved) {
      box.style.left = left0 + dx + "px";
      box.style.top = top0 + dy + "px";
    }
  };
  const onUp = (ev) => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    box.classList.remove("dragging");
    if (!moved) return;
    const dx = (ev.clientX - startX) / state.editScale;
    const dy = -(ev.clientY - startY) / state.editScale; // CSS y xuống = PDF y giảm
    stageEditOp({ op: "transform", index: o.index, dx, dy, sx: 1, sy: 1 });
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
  e.preventDefault();
}

// Kéo handle góc dưới-phải để resize — khung đổi kích thước live, thả commit.
function onEditResizeMouseDown(e, o) {
  e.stopPropagation();
  e.preventDefault();
  const box = e.target.parentElement;
  const startX = e.clientX, startY = e.clientY;
  const w0 = (o.rect.right - o.rect.left), h0 = (o.rect.top - o.rect.bottom);
  const cssW0 = parseFloat(box.style.width), cssH0 = parseFloat(box.style.height);
  const onMove = (ev) => {
    box.classList.add("dragging");
    box.style.width = Math.max(4, cssW0 + (ev.clientX - startX)) + "px";
    box.style.height = Math.max(4, cssH0 + (ev.clientY - startY)) + "px";
  };
  const onUp = (ev) => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    box.classList.remove("dragging");
    const dwPt = (ev.clientX - startX) / state.editScale;
    const dhPt = (ev.clientY - startY) / state.editScale; // kéo xuống = cao hơn (y giảm ở đáy)
    const sx = Math.max(0.1, (w0 + dwPt) / Math.max(1, w0));
    const sy = Math.max(0.1, (h0 + dhPt) / Math.max(1, h0));
    if (Math.abs(sx - 1) > 0.01 || Math.abs(sy - 1) > 0.01) {
      // Engine scale quanh góc DƯỚI-trái; bù dy để neo góc TRÊN-trái đứng yên
      // (khớp handle góc dưới-phải + preview khi kéo, như Foxit).
      stageEditOp({ op: "transform", index: o.index, dx: 0, dy: -(h0 * (sy - 1)), sx, sy });
    } else {
      loadEditPage(); // trả khung về đúng kích thước cũ
    }
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

async function armAddImage() {
  const path = await invoke("pick_image");
  if (!path) return;
  state.editPendingImage = path;
  state.editArm = "image";
  $("edAddImage").classList.add("armed");
  $("editOverlay").classList.add("armed");
  $("editHint").textContent = "Bấm lên trang để đặt ảnh";
}

async function replaceSelectedImage() {
  if (state.editSel == null) return;
  const o = state.editObjects.find((x) => x.index === state.editSel);
  if (!o || o.kind !== "image") return;
  const path = await invoke("pick_image");
  if (!path) return;
  stageEditOp({ op: "replaceImage", index: o.index, imagePath: path });
}

async function saveEdits() {
  const out = await invoke("pick_save_pdf");
  if (!out) return;
  try {
    // editBase đã gồm mọi thay đổi của trang đang sửa; ghi ra output (ops rỗng = sao chép/lưu lại).
    await invoke("edit_apply", { input: state.editBase, page: state.editPage, ops: [], output: out, password: null });
    $("status").textContent = `Đã lưu nội dung đã sửa → ${shortName(out)}`;
    exitEditMode();
    loadDocument(out);
  } catch (e) {
    $("editHint").textContent = "Lỗi lưu: " + e;
  }
}

// ---------- Phase 5: Bảo mật (redact / mật khẩu / metadata) ----------

function toggleSecMode() {
  state.secMode = !state.secMode;
  $("secBar").classList.toggle("hidden", !state.secMode);
  $("secModeBtn").classList.toggle("active", state.secMode);
  if (!state.secMode && state.tool === "redact") setTool(null);
  $("secHint").textContent = "";
}

function updateRedactButtons() {
  const n = state.redactMarks.length;
  $("redactCount").textContent = n;
  $("secRedactApply").disabled = n === 0;
  $("secRedactClear").disabled = n === 0;
}

function clearRedactMarks() {
  const pages = [...new Set(state.redactMarks.map((m) => m.page))];
  state.redactMarks = [];
  pages.forEach((p) => drawAnnotsForPage(p));
  updateRedactButtons();
}

// Áp dụng redact: XOÁ THẬT nội dung trong các vùng đã đánh dấu → file mới.
async function applyRedactions() {
  if (!state.redactMarks.length) return;
  const out = await invoke("pick_save_pdf");
  if (!out) return;
  // Gom theo trang: [{page, rects: [[l,b,r,t], ...]}]
  const byPage = new Map();
  for (const m of state.redactMarks) {
    if (!byPage.has(m.page)) byPage.set(m.page, []);
    byPage.get(m.page).push([m.rect.left, m.rect.bottom, m.rect.right, m.rect.top]);
  }
  const areas = [...byPage.entries()].map(([page, rects]) => ({ page, rects }));
  try {
    const n = await invoke("redact_apply", { input: state.path, areas, output: out, password: null });
    clearRedactMarks();
    setTool(null);
    $("status").textContent = `Đã redact (xoá thật) ${n} đối tượng → ${shortName(out)}`;
    loadDocument(out);
  } catch (e) {
    $("secHint").textContent = "Lỗi redact: " + e;
  }
}

function openEncryptDialog() {
  const box = openModal("Đặt mật khẩu (AES-256)", `
    <label>Mật khẩu mở file (user password)</label>
    <input type="password" id="secPw1" autocomplete="new-password">
    <label>Nhập lại mật khẩu</label>
    <input type="password" id="secPw2" autocomplete="new-password">
    <label>Mật khẩu chủ sở hữu (owner — để trống = dùng mật khẩu mở)</label>
    <input type="password" id="secPwOwner" autocomplete="new-password">
    <label>Quyền hạn khi mở bằng mật khẩu user</label>
    <div class="row">
      <label><input type="checkbox" id="secPermPrint" checked> In</label>
      <label><input type="checkbox" id="secPermModify" checked> Sửa nội dung</label>
    </div>
    <div class="row">
      <label><input type="checkbox" id="secPermExtract" checked> Sao chép text/ảnh</label>
      <label><input type="checkbox" id="secPermAnnotate" checked> Chú thích/điền form</label>
    </div>
    <div class="err" id="secEncErr"></div>
    <div class="foot"><button id="secEncCancel">Huỷ</button><button id="secEncOk" class="primary">Mã hoá…</button></div>
  `);
  box.querySelector("#secEncCancel").addEventListener("click", closeModal);
  box.querySelector("#secEncOk").addEventListener("click", async () => {
    const p1 = box.querySelector("#secPw1").value;
    const p2 = box.querySelector("#secPw2").value;
    const err = box.querySelector("#secEncErr");
    if (!p1) { err.textContent = "Mật khẩu không được để trống."; return; }
    if (p1 !== p2) { err.textContent = "Hai lần nhập không khớp."; return; }
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    try {
      await invoke("security_encrypt", {
        input: state.path,
        output: out,
        userPassword: p1,
        ownerPassword: box.querySelector("#secPwOwner").value,
        allowPrint: box.querySelector("#secPermPrint").checked,
        allowModify: box.querySelector("#secPermModify").checked,
        allowExtract: box.querySelector("#secPermExtract").checked,
        allowAnnotate: box.querySelector("#secPermAnnotate").checked,
      });
      closeModal();
      $("status").textContent = `Đã mã hoá AES-256 → ${shortName(out)}`;
      $("secHint").textContent = "File mã hoá đã lưu riêng — file đang mở giữ nguyên.";
    } catch (e) {
      err.textContent = "Lỗi: " + e;
    }
  });
}

function openDecryptDialog() {
  const box = openModal("Gỡ mật khẩu", `
    <p class="muted">Chọn file PDF đang có mật khẩu, nhập mật khẩu hiện tại, lưu ra bản không mã hoá.</p>
    <label>Mật khẩu hiện tại</label>
    <input type="password" id="secDecPw" autocomplete="current-password">
    <div class="err" id="secDecErr"></div>
    <div class="foot"><button id="secDecCancel">Huỷ</button><button id="secDecOk" class="primary">Chọn file &amp; gỡ…</button></div>
  `);
  box.querySelector("#secDecCancel").addEventListener("click", closeModal);
  box.querySelector("#secDecOk").addEventListener("click", async () => {
    const pw = box.querySelector("#secDecPw").value;
    const err = box.querySelector("#secDecErr");
    if (!pw) { err.textContent = "Cần nhập mật khẩu hiện tại."; return; }
    const inp = await invoke("pick_pdf");
    if (!inp) return;
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    try {
      await invoke("security_decrypt", { input: inp, password: pw, output: out });
      closeModal();
      $("status").textContent = `Đã gỡ mật khẩu → ${shortName(out)}`;
      loadDocument(out);
    } catch (e) {
      err.textContent = "Lỗi (mật khẩu sai?): " + e;
    }
  });
}

async function stripMetadataAction() {
  const out = await invoke("pick_save_pdf");
  if (!out) return;
  try {
    await invoke("security_strip_metadata", { input: state.path, output: out });
    $("status").textContent = `Đã xoá metadata (/Info + XMP) → ${shortName(out)}`;
    loadDocument(out);
  } catch (e) {
    $("secHint").textContent = "Lỗi xoá metadata: " + e;
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
$("undoBtn").addEventListener("click", () => (state.editMode ? editUndo() : undo()));
$("redoBtn").addEventListener("click", () => (state.editMode ? editRedo() : redo()));
$("pages").addEventListener("mousedown", onPagesMouseDown);
window.addEventListener("mousemove", onPagesMouseMove);
window.addEventListener("mouseup", onPagesMouseUp);

$("organizeModeBtn").addEventListener("click", toggleOrganizeMode);
$("orgInsert").addEventListener("click", openInsertDialog);
$("orgDelete").addEventListener("click", orgDeleteSelected);
$("orgRotateL").addEventListener("click", () => orgRotateSelected(-90));
$("orgRotateR").addEventListener("click", () => orgRotateSelected(90));
$("orgExtract").addEventListener("click", openExtractDialog);
$("orgReplace").addEventListener("click", openReplaceDialog);
$("orgMerge").addEventListener("click", openMergeDialog);
$("orgSplit").addEventListener("click", openSplitDialog);
$("orgWatermark").addEventListener("click", openWatermarkDialog);
$("orgHeaderFooter").addEventListener("click", openHeaderFooterDialog);
$("orgSave").addEventListener("click", orgSaveChanges);
$("modalOverlay").addEventListener("click", (e) => {
  if (e.target.id === "modalOverlay") closeModal();
});

// Bảo mật (Phase 5)
$("secModeBtn").addEventListener("click", toggleSecMode);
$("secRedactApply").addEventListener("click", applyRedactions);
$("secRedactClear").addEventListener("click", clearRedactMarks);
$("secEncrypt").addEventListener("click", openEncryptDialog);
$("secDecrypt").addEventListener("click", openDecryptDialog);
$("secStripMeta").addEventListener("click", stripMetadataAction);

// Sửa nội dung (Phase 4)
$("editModeBtn").addEventListener("click", toggleEditMode);
$("edAddText").addEventListener("click", () => {
  state.editArm = state.editArm === "text" ? null : "text";
  $("edAddText").classList.toggle("armed", state.editArm === "text");
  $("editOverlay").classList.toggle("armed", !!state.editArm);
  $("editHint").textContent = state.editArm === "text" ? "Bấm lên trang để đặt chữ" : "";
});
$("edAddImage").addEventListener("click", armAddImage);
$("edDelete").addEventListener("click", deleteSelectedEditObject);
$("edReplaceImage").addEventListener("click", replaceSelectedImage);
// Mọi thay đổi thuộc tính chỉ gửi ĐÚNG field đó — engine giữ nguyên font khi
// không cần thay (chuẩn Foxit: đổi cỡ/màu không được đổi font).
$("edFontSize").addEventListener("change", () => {
  const size = Number($("edFontSize").value);
  if (size > 0) applyTextPropToSelected({ fontSize: size });
});
$("edFontFamily").addEventListener("change", () => {
  const fam = $("edFontFamily").value;
  if (fam) applyTextPropToSelected({ fontFamily: fam });
});
$("edBold").addEventListener("click", () => {
  const o = state.editObjects.find((x) => x.index === state.editSel);
  if (o) applyTextPropToSelected({ bold: !o.font_bold });
});
$("edItalic").addEventListener("click", () => {
  const o = state.editObjects.find((x) => x.index === state.editSel);
  if (o) applyTextPropToSelected({ italic: !o.font_italic });
});
$("edColorBtn").addEventListener("click", () => {
  openColorPopover($("edColorBtn"), state.editColor, (rgb) => {
    state.editColor = rgb;
    $("edSw").style.background = rgbCss(rgb);
    applyTextPropToSelected({ color: [rgb[0], rgb[1], rgb[2], 255] });
  });
});
$("edSave").addEventListener("click", saveEdits);
$("editOverlay").addEventListener("click", onEditStageClick);

// Phím tắt chuẩn của trình xem PDF (Foxit/Adobe): điều hướng trang, zoom,
// tìm kiếm, xoá/bỏ chọn annotation.
window.addEventListener("keydown", (e) => {
  const typing = e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName);

  if (e.key === "Escape") {
    if (!$("modalOverlay").classList.contains("hidden")) { closeModal(); return; }
    if (state.editMode && state.editArm) {
      state.editArm = null; state.editPendingImage = null;
      $("edAddText").classList.remove("armed"); $("edAddImage").classList.remove("armed");
      $("editOverlay").classList.remove("armed"); $("editHint").textContent = "";
      return;
    }
    finishEditing(); closeNotePopup(); closeColorPopover(); deselectAnnot(); setTool(null);
    return;
  }
  if ((e.key === "Delete" || e.key === "Backspace") && !typing && state.editMode && state.editSel != null) {
    e.preventDefault();
    deleteSelectedEditObject();
    return;
  }
  if ((e.key === "Delete" || e.key === "Backspace") && !typing && state.organizeMode && state.orgSelected.size) {
    e.preventDefault();
    orgDeleteSelected();
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
    if (state.editMode) editUndo(); else undo();
    return;
  }
  if (e.ctrlKey && (e.key === "y" || e.key === "Y" || (e.shiftKey && (e.key === "z" || e.key === "Z"))) && !typing) {
    e.preventDefault();
    if (state.editMode) editRedo(); else redo();
    return;
  }
  if (typing) return;
  if (e.key === "PageDown") { e.preventDefault(); gotoNextPage(); return; }
  if (e.key === "PageUp") { e.preventDefault(); gotoPrevPage(); return; }
  if (e.key === "Home") { e.preventDefault(); goToPage(0); return; }
  if (e.key === "End") { e.preventDefault(); goToPage(state.pages.length - 1); return; }
});

window.addEventListener("DOMContentLoaded", boot);
