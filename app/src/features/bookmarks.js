// Bookmark kiểu Foxit (panel Mục lục sửa được: thêm/thêm con/đổi tên/xoá/
// nâng-hạ cấp/kéo-thả/thu gọn/menu chuột phải/tự tạo từ tiêu đề), Liên kết
// (vùng bấm trên trang + công cụ tạo liên kết) và hộp thoại Tách tệp nhiều chế độ.
//
// Thay đổi bookmark + liên kết là "đang chờ" (chưa ghi tệp) — lưu một lần bằng
// nút Lưu của panel (hoặc Ctrl+S), ghi qua bm_save rồi mở lại tệp kết quả.
// Hoàn tác dùng CHUNG stack Ctrl+Z của main.js (snapshot được mở rộng thêm
// phần bookmark/liên kết).
//
// Công cụ Liên kết: nút ở ribbon Sửa (và Chú thích). Liên kết là annotation,
// không phải nội dung trang → công cụ làm việc trên viewer thường: bấm từ tab
// Sửa sẽ rời chế độ sửa nội dung (hỏi nếu còn thay đổi) và chuyển sang tab Chú
// thích với công cụ đang bật; kéo khung trên trang rồi chọn đích (trang / URL).
(function () {
  "use strict";

  const bm = {
    path: null,        // tài liệu mà cây/ liên kết dưới đây thuộc về
    gen: 0,            // tăng mỗi lần nạp cây mới (snapshot khác thế hệ bị bỏ qua)
    docTok: 0,         // định danh lần mở tài liệu (bỏ kết quả async của tệp cũ)
    tree: [],          // [{_id,title,pageIndex,uri,left,top,zoom,fit,bold,italic,color,open,children}]
    savedKey: "[]",
    loaded: false,
    selId: null,
    seq: 1,
    links: [],         // liên kết đọc từ tệp
    linkPath: null,
    linkAdd: [],       // liên kết mới chờ lưu {tmpId,pageIndex,rect,destPage,destTop,uri,border}
    linkRemove: new Set(),
    linkSel: null,     // id/tmpId liên kết đang chọn
    armed: false,      // công cụ Liên kết đang bật
    lastSel: "",       // chữ đang bôi đen trên trang (làm tiêu đề bookmark mới)
  };

  // ---------------- Mô hình cây ----------------
  const FIELDS = ["title", "pageIndex", "uri", "left", "top", "zoom", "fit", "bold", "italic", "color", "open"];
  function withIds(list) {
    return (list || []).map((n) => {
      const o = { _id: bm.seq++ };
      for (const f of FIELDS) o[f] = n[f] === undefined ? null : n[f];
      o.fit = !!o.fit; o.bold = !!o.bold; o.italic = !!o.italic; o.open = !!o.open;
      o.title = o.title || "";
      o.children = withIds(n.children);
      return o;
    });
  }
  function strip(list, keepOpen = true) {
    return list.map((n) => {
      const o = {};
      for (const f of FIELDS) if (keepOpen || f !== "open") o[f] = n[f];
      o.children = strip(n.children, keepOpen);
      return o;
    });
  }
  // Mở/đóng nút không tính là thay đổi tài liệu.
  const treeKey = () => JSON.stringify(strip(bm.tree, false));
  const treeDirty = () => bm.loaded && treeKey() !== bm.savedKey;
  const isDirty = () => !!state.path && bm.path === state.path && (treeDirty() || bm.linkAdd.length > 0 || bm.linkRemove.size > 0);

  function find(id, list = bm.tree, parent = null) {
    for (let i = 0; i < list.length; i++) {
      const n = list[i];
      if (n._id === id) return { node: n, list, index: i, parent };
      const r = find(id, n.children, n);
      if (r) return r;
    }
    return null;
  }
  function isAncestor(a, b) { // a là tổ tiên (hoặc chính) b?
    const fa = find(a);
    return !!fa && (a === b || !!find(b, fa.node.children));
  }
  function forEachNode(fn, list = bm.tree) {
    for (const n of list) { fn(n); forEachNode(fn, n.children); }
  }

  // ---------------- Hoàn tác (mở rộng snapshot toàn cục) ----------------
  function mySnap() {
    return {
      gen: bm.gen, path: bm.path,
      tree: JSON.stringify(bm.tree),
      linkAdd: JSON.stringify(bm.linkAdd),
      linkRemove: [...bm.linkRemove],
    };
  }
  function myRestore(s) {
    if (!s || s.gen !== bm.gen || s.path !== bm.path) return;
    bm.tree = JSON.parse(s.tree);
    bm.linkAdd = JSON.parse(s.linkAdd);
    bm.linkRemove = new Set(s.linkRemove);
    if (bm.selId != null && !find(bm.selId)) bm.selId = null;
    bm.linkSel = null;
    renderTree();
    redrawAllLinks();
  }
  const origSnapshot = window.snapshot;
  const origApplySnapshot = window.applySnapshot;
  if (typeof origSnapshot === "function" && typeof origApplySnapshot === "function") {
    window.snapshot = function () { const s = origSnapshot(); s.bm = mySnap(); return s; };
    window.applySnapshot = function (snap) { origApplySnapshot(snap); if (snap) myRestore(snap.bm); };
  }
  function change(fn) {
    if (typeof pushUndo === "function") pushUndo();
    fn();
    renderTree();
  }

  // ---------------- Nạp theo tài liệu ----------------
  // main.js gọi buildOutline(items) khi mở tệp (items = outline phẳng của
  // PDFium) và khi đổi ngôn ngữ → cùng tệp thì chỉ vẽ lại.
  window.buildOutline = function () {
    ensurePanel();
    if (state.path && state.path === bm.path) { renderTree(); return; }
    resetForDoc(state.path);
  };

  function resetForDoc(path) {
    const c = takeCarry();
    bm.path = path;
    const tok = ++bm.docTok;
    bm.gen++;
    bm.tree = [];
    bm.savedKey = "[]";
    bm.loaded = false;
    bm.selId = null;
    bm.links = [];
    bm.linkPath = null;
    bm.linkAdd = [];
    bm.linkRemove = new Set();
    bm.linkSel = null;
    renderTree();
    if (!path) return;
    // Tệp mới là bản vừa lưu chú thích/bôi đen của CHÍNH tài liệu này (cùng số
    // trang, cùng bookmark đã lưu) → mang các thay đổi bookmark/liên kết đang
    // chờ sang, không để mất âm thầm.
    const carryOk = () => c && state.pages.length === c.pages && bm.savedKey === c.savedKey;
    let treeDone = false;
    let linksDone = false;
    const applyCarry = () => {
      if (!treeDone || !linksDone || !carryOk()) return;
      bm.gen++; // snapshot hoàn tác cũ không còn khớp
      if (c.treeDirty) bm.tree = withIds(JSON.parse(c.tree));
      bm.linkAdd = JSON.parse(c.linkAdd);
      for (const r of c.removed) {
        const m = bm.links.find((l) => l.pageIndex === r.pageIndex &&
          ["left", "bottom", "right", "top"].every((k) => Math.abs(l.rect[k] - r.rect[k]) < 0.5));
        if (m) bm.linkRemove.add(m.id);
      }
      renderTree();
      redrawAllLinks();
    };
    invoke("bm_get_tree", { path })
      .then((tree) => {
        if (bm.docTok !== tok) return;
        bm.gen++; // cây thật đã về: snapshot chụp lúc cây còn rỗng không áp lại được
        bm.tree = withIds(tree);
        bm.savedKey = treeKey();
        bm.loaded = true;
        renderTree();
      })
      .catch((e) => {
        if (bm.docTok !== tok) return;
        bm.loaded = true;
        bm.savedKey = treeKey();
        renderTree();
        $("status").textContent = t("bm.loadErr", { e });
      })
      .finally(() => { if (bm.docTok === tok) { treeDone = true; applyCarry(); } });
    invoke("link_list", { path })
      .then((links) => {
        if (bm.docTok !== tok) return;
        bm.links = links;
        bm.linkPath = path;
        redrawAllLinks();
      })
      .catch(() => { if (bm.docTok === tok) { bm.links = []; bm.linkPath = path; } })
      .finally(() => { if (bm.docTok === tok) { linksDone = true; applyCarry(); } });
  }

  // Chụp thay đổi đang chờ ngay trước khi lưu chú thích / áp dụng bôi đen
  // (các thao tác đó mở lại tệp kết quả). Hết hạn sau 2 phút / khi mở tệp khác.
  let carry = null;
  function armCarry() {
    if (!isDirty()) { carry = null; return; }
    carry = {
      until: Date.now() + 120000,
      pages: state.pages.length,
      savedKey: bm.savedKey,
      treeDirty: treeDirty(),
      tree: JSON.stringify(bm.tree),
      linkAdd: JSON.stringify(bm.linkAdd),
      removed: bm.links.filter((l) => bm.linkRemove.has(l.id)).map((l) => ({ pageIndex: l.pageIndex, rect: l.rect })),
    };
  }
  function takeCarry() {
    const c = carry;
    carry = null;
    return c && Date.now() < c.until ? c : null;
  }

  // ---------------- Panel ----------------
  function ensurePanel() {
    const box = $("outline");
    if (box.querySelector("#bmTree")) return box;
    box.classList.add("bm-panel");
    box.innerHTML = `
      <div class="bm-toolbar" role="toolbar">
        <button class="bm-tb" id="bmAdd" data-i18n-title="bm.addTip"><i data-icon="bookmark-add"></i></button>
        <button class="bm-tb" id="bmAddChild" data-i18n-title="bm.addChildTip"><i data-icon="bookmark-child"></i></button>
        <button class="bm-tb" id="bmRename" data-i18n-title="bm.renameTip"><i data-icon="rename"></i></button>
        <button class="bm-tb" id="bmDelete" data-i18n-title="bm.deleteTip"><i data-icon="trash"></i></button>
        <button class="bm-tb" id="bmPromote" data-i18n-title="bm.promoteTip"><i data-icon="outdent"></i></button>
        <button class="bm-tb" id="bmDemote" data-i18n-title="bm.demoteTip"><i data-icon="indent"></i></button>
        <button class="bm-tb" id="bmMore" data-i18n-title="bm.moreTip"><i data-icon="more"></i></button>
      </div>
      <div class="bm-dirty hidden" id="bmDirtyBar">
        <span class="bm-dot"></span><span class="bm-dirty-txt" id="bmDirtyTxt"></span>
        <button class="bm-dbtn" id="bmDiscard" data-i18n-title="bm.discardTip"><i data-icon="discard"></i></button>
        <button class="bm-dbtn primary" id="bmSave" data-i18n-title="bm.saveTip"><i data-icon="save"></i><span data-i18n="common.save"></span></button>
      </div>
      <div class="bm-tree" id="bmTree" role="tree" tabindex="0" data-i18n-aria="side.outline"></div>`;
    applyI18n(box);
    applyIcons(box);
    const snapSel = () => { bm.lastSel = pageSelectionText(); };
    box.querySelector(".bm-toolbar").addEventListener("pointerdown", snapSel, true);
    $("bmAdd").addEventListener("click", () => addBookmark(false));
    $("bmAddChild").addEventListener("click", () => addBookmark(true));
    $("bmRename").addEventListener("click", () => { if (bm.selId != null) startRename(bm.selId); });
    $("bmDelete").addEventListener("click", () => deleteSelected());
    $("bmPromote").addEventListener("click", () => promote(bm.selId));
    $("bmDemote").addEventListener("click", () => demote(bm.selId));
    $("bmMore").addEventListener("click", (e) => {
      const r = e.currentTarget.getBoundingClientRect();
      showNodeMenu(r.left, r.bottom + 4, bm.selId != null ? find(bm.selId) : null);
    });
    $("bmSave").addEventListener("click", save);
    $("bmDiscard").addEventListener("click", discard);
    bindTree($("bmTree"));
    return box;
  }

  function pageSelectionText() {
    const s = window.getSelection && window.getSelection();
    if (!s || s.isCollapsed || !s.anchorNode) return "";
    const el = s.anchorNode.nodeType === 1 ? s.anchorNode : s.anchorNode.parentElement;
    if (!el || !el.closest(".textlayer")) return "";
    return s.toString().replace(/\s+/g, " ").trim().slice(0, 200);
  }

  function nodeLabel(n) { return n.title || t("bm.untitled"); }
  function nodeTip(n) {
    const dest = n.uri ? n.uri : n.pageIndex != null ? t("bm.pageN", { n: n.pageIndex + 1 }) : t("bm.noDest");
    return `${nodeLabel(n)} — ${dest}`;
  }

  function renderTree() {
    const tree = $("bmTree");
    if (!tree) return;
    const keepFocus = document.activeElement === tree;
    tree.innerHTML = "";
    if (!state.path) {
      tree.innerHTML = `<div class="bm-empty muted">${escapeHtml(t("viewer.noOutline"))}</div>`;
    } else if (!bm.loaded) {
      tree.innerHTML = `<div class="bm-empty muted">${escapeHtml(t("common.loading"))}</div>`;
    } else if (!bm.tree.length) {
      tree.innerHTML = `<div class="bm-empty muted">${escapeHtml(t("viewer.noOutline"))}</div>
        <div class="bm-empty muted">${escapeHtml(t("bm.emptyHint"))}</div>`;
    } else {
      tree.appendChild(renderLevel(bm.tree, 0));
      applyIcons(tree);
    }
    if (keepFocus) tree.focus();
    updateToolbar();
    updateDirty();
  }

  function renderLevel(list, depth) {
    const frag = document.createDocumentFragment();
    for (const n of list) {
      const row = document.createElement("div");
      row.className = "bm-row";
      row.dataset.id = n._id;
      row.setAttribute("role", "treeitem");
      row.setAttribute("aria-level", String(depth + 1));
      row.style.paddingLeft = 2 + depth * 14 + "px";
      const has = n.children.length > 0;
      if (has) row.setAttribute("aria-expanded", n.open ? "true" : "false");
      if (n._id === bm.selId) { row.classList.add("sel"); row.setAttribute("aria-selected", "true"); }
      if (n.pageIndex == null && !n.uri) row.classList.add("nodest");
      row.title = nodeTip(n);
      row.innerHTML = `<span class="bm-tw${has ? " has" : ""}${n.open ? " open" : ""}">${has ? '<i data-icon="chevron-right"></i>' : ""}</span>` +
        `<i data-icon="${n.uri ? "link" : "bookmark"}" class="bm-ic"></i><span class="bm-title"></span>`;
      const ti = row.querySelector(".bm-title");
      ti.textContent = nodeLabel(n);
      if (n.bold) ti.style.fontWeight = "700";
      if (n.italic) ti.style.fontStyle = "italic";
      if (n.color) ti.style.color = rgbCss(n.color);
      frag.appendChild(row);
      if (has && n.open) frag.appendChild(renderLevel(n.children, depth + 1));
    }
    return frag;
  }

  function updateToolbar() {
    if (!$("bmAdd")) return;
    const f = bm.selId != null ? find(bm.selId) : null;
    const ready = !!state.path && bm.loaded;
    $("bmAdd").disabled = !ready;
    $("bmAddChild").disabled = !f;
    $("bmRename").disabled = !f;
    $("bmDelete").disabled = !f;
    $("bmPromote").disabled = !f || !f.parent;
    $("bmDemote").disabled = !f || f.index === 0;
    $("bmMore").disabled = !ready;
  }

  function updateDirty() {
    const bar = $("bmDirtyBar");
    if (!bar) return;
    const d = isDirty();
    bar.classList.toggle("hidden", !d);
    $("tabOutline").classList.toggle("bm-has-dirty", d);
    if (d) {
      const nl = bm.linkAdd.length + bm.linkRemove.size;
      $("bmDirtyTxt").textContent = treeDirty() && nl ? t("bm.unsavedBoth", { n: nl })
        : nl ? t("bm.unsavedLinks", { n: nl }) : t("bm.unsaved");
      $("bmDirtyTxt").title = $("bmDirtyTxt").textContent;
    }
  }

  const rowEl = (id) => $("bmTree") && $("bmTree").querySelector(`.bm-row[data-id="${id}"]`);

  function select(id, scroll) {
    bm.selId = id;
    const tree = $("bmTree");
    tree.querySelectorAll(".bm-row.sel").forEach((r) => { r.classList.remove("sel"); r.removeAttribute("aria-selected"); });
    const r = id != null ? rowEl(id) : null;
    if (r) {
      r.classList.add("sel");
      r.setAttribute("aria-selected", "true");
      if (scroll) r.scrollIntoView({ block: "nearest" });
    }
    updateToolbar();
  }

  function activate(n) {
    if (n.uri) { openExternal(n.uri); return; }
    if (n.pageIndex == null) { $("status").textContent = t("bm.noDest"); return; }
    navigateTo(n.pageIndex, n.fit ? null : n.top);
  }

  // Cuộn tới trang + vị trí top (điểm PDF, gốc dưới-trái) nếu biết.
  function navigateTo(page, top) {
    if (page == null || !state.pages[page]) return;
    const slot = state.slots[page];
    const p = state.pages[page];
    const vp = $("viewport");
    if (state.editMode || state.organizeMode || !slot || top == null || !(top >= 0 && top <= p.heightPt) || vp.offsetParent === null) {
      goToPage(page);
      return;
    }
    const scale = PT_PER_PX * state.zoom;
    const y = vp.scrollTop + slot.getBoundingClientRect().top - vp.getBoundingClientRect().top + (p.heightPt - top) * scale - 12;
    vp.scrollTo({ top: Math.max(0, y), behavior: "smooth" });
  }

  // Trang + top của khung nhìn hiện tại (đích cho bookmark mới / "đặt đích").
  function currentView() {
    const idx = currentPageIndex();
    const slot = state.slots[idx];
    const p = state.pages[idx];
    const vp = $("viewport");
    if (state.editMode || state.organizeMode || !slot || !p || vp.offsetParent === null) return { pageIndex: idx, top: null };
    const scale = PT_PER_PX * state.zoom;
    const off = (vp.getBoundingClientRect().top - slot.getBoundingClientRect().top) / scale;
    if (off <= 1) return { pageIndex: idx, top: null };
    return { pageIndex: idx, top: Math.round(Math.max(0, Math.min(p.heightPt, p.heightPt - off)) * 10) / 10 };
  }

  // ---------------- Thao tác ----------------
  function newNode(title) {
    const v = currentView();
    return {
      _id: bm.seq++, title, pageIndex: v.pageIndex, uri: null, left: null, top: v.top, zoom: null,
      fit: false, bold: false, italic: false, color: null, open: false, children: [],
    };
  }

  function addBookmark(asChild) {
    if (!state.path || !bm.loaded || !state.pages.length) return;
    const title = bm.lastSel || pageSelectionText() || t("bm.untitled");
    bm.lastSel = "";
    const n = newNode(title);
    const f = bm.selId != null ? find(bm.selId) : null;
    change(() => {
      if (asChild && f) { f.node.children.push(n); f.node.open = true; }
      else if (f) f.list.splice(f.index + 1, 0, n);
      else bm.tree.push(n);
      bm.selId = n._id;
    });
    if ($("outline").classList.contains("hidden")) $("tabOutline").click();
    startRename(n._id, true);
  }

  function deleteSelected() {
    const f = bm.selId != null ? find(bm.selId) : null;
    if (!f) return;
    change(() => {
      f.list.splice(f.index, 1);
      const next = f.list[f.index] || f.list[f.index - 1] || f.parent;
      bm.selId = next ? next._id : null;
    });
    focusTree();
  }

  function promote(id) {
    const f = id != null ? find(id) : null;
    if (!f || !f.parent) return;
    const pf = find(f.parent._id);
    change(() => {
      f.list.splice(f.index, 1);
      pf.list.splice(pf.index + 1, 0, f.node);
    });
    focusTree();
  }

  function demote(id) {
    const f = id != null ? find(id) : null;
    if (!f || f.index === 0) return;
    const prev = f.list[f.index - 1];
    change(() => {
      f.list.splice(f.index, 1);
      prev.children.push(f.node);
      prev.open = true;
    });
    focusTree();
  }

  // Kéo-thả: where = "before" | "after" | "inside".
  function moveNode(id, targetId, where) {
    if (id === targetId || isAncestor(id, targetId)) return;
    const f = find(id);
    if (!f) return;
    change(() => {
      f.list.splice(f.index, 1);
      const tf = find(targetId);
      if (where === "inside") { tf.node.children.push(f.node); tf.node.open = true; }
      else tf.list.splice(tf.index + (where === "after" ? 1 : 0), 0, f.node);
      bm.selId = id;
    });
  }

  function toggleOpen(n, open) {
    if (!n.children.length) return;
    n.open = open === undefined ? !n.open : open;
    renderTree();
  }
  function setAllOpen(open) {
    forEachNode((n) => { if (n.children.length) n.open = open; });
    renderTree();
  }

  function setDestHere(id) {
    const f = id != null ? find(id) : null;
    if (!f) return;
    const v = currentView();
    change(() => { Object.assign(f.node, { pageIndex: v.pageIndex, top: v.top, left: null, zoom: null, fit: false, uri: null }); });
    $("status").textContent = t("bm.destSet", { n: v.pageIndex + 1 });
  }

  function focusTree() { const tr = $("bmTree"); if (tr) tr.focus(); }

  function startRename(id, isNew) {
    const row = rowEl(id);
    const f = find(id);
    if (!row || !f) return;
    row.scrollIntoView({ block: "nearest" });
    const ti = row.querySelector(".bm-title");
    const inp = document.createElement("input");
    inp.type = "text";
    inp.className = "bm-edit";
    inp.value = f.node.title;
    ti.replaceWith(inp);
    inp.focus();
    inp.select();
    let done = false;
    const finish = (commit) => {
      if (done) return;
      done = true;
      const v = inp.value.replace(/\s+/g, " ").trim();
      if (commit && v && v !== f.node.title) {
        // Bookmark vừa thêm: đổi tên nằm chung bước hoàn tác với lệnh thêm.
        if (!isNew && typeof pushUndo === "function") pushUndo();
        f.node.title = v;
      }
      renderTree();
      focusTree();
    };
    inp.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { e.preventDefault(); finish(true); }
      else if (e.key === "Escape") { e.preventDefault(); finish(false); }
    });
    inp.addEventListener("blur", () => finish(true));
    for (const ev of ["pointerdown", "click", "dblclick"]) inp.addEventListener(ev, (e) => e.stopPropagation());
  }

  // ---------------- Cây: chuột / bàn phím / kéo-thả ----------------
  function bindTree(tree) {
    tree.addEventListener("click", (e) => {
      const row = e.target.closest(".bm-row");
      if (!row || drag.justDropped) return;
      const f = find(Number(row.dataset.id));
      if (!f) return;
      if (e.target.closest(".bm-tw")) { toggleOpen(f.node); return; }
      select(f.node._id);
      activate(f.node);
    });
    tree.addEventListener("dblclick", (e) => {
      const row = e.target.closest(".bm-row");
      if (row && !e.target.closest(".bm-tw")) startRename(Number(row.dataset.id));
    });
    tree.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      bm.lastSel = pageSelectionText();
      const row = e.target.closest(".bm-row");
      if (row) select(Number(row.dataset.id));
      showNodeMenu(e.clientX, e.clientY, row ? find(Number(row.dataset.id)) : null);
    });
    tree.addEventListener("keydown", onTreeKey);
    tree.addEventListener("pointerdown", onDragDown);
  }

  function visibleIds() {
    return [...$("bmTree").querySelectorAll(".bm-row")].map((r) => Number(r.dataset.id));
  }

  function onTreeKey(e) {
    if (e.target !== $("bmTree")) return;
    const ids = visibleIds();
    const f = bm.selId != null ? find(bm.selId) : null;
    const pos = f ? ids.indexOf(bm.selId) : -1;
    const k = e.key;
    let handled = true;
    if (k === "ArrowDown") select(ids[Math.min(ids.length - 1, pos + 1)] ?? null, true);
    else if (k === "ArrowUp") select(ids[Math.max(0, pos - 1)] ?? null, true);
    else if (k === "Home") select(ids[0] ?? null, true);
    else if (k === "End") select(ids[ids.length - 1] ?? null, true);
    else if (k === "ArrowRight" && f) {
      if (f.node.children.length && !f.node.open) toggleOpen(f.node, true);
      else if (f.node.children.length) select(f.node.children[0]._id, true);
    } else if (k === "ArrowLeft" && f) {
      if (f.node.children.length && f.node.open) toggleOpen(f.node, false);
      else if (f.parent) select(f.parent._id, true);
    } else if (k === "Enter" && f) activate(f.node);
    else if (k === "F2" && f) startRename(f.node._id);
    else if ((k === "Delete" || k === "Backspace") && f) deleteSelected();
    else if (k === "ContextMenu" || (k === "F10" && e.shiftKey)) {
      const r = f ? rowEl(f.node._id) : $("bmTree");
      const b = r.getBoundingClientRect();
      showNodeMenu(b.left + 24, b.bottom, f);
    } else handled = false;
    if (handled) { e.preventDefault(); e.stopPropagation(); }
  }

  // Kéo-thả bằng pointer (HTML5 DnD bị WebView/Tauri chặn trên Windows).
  const drag = { id: null, x: 0, y: 0, active: false, ghost: null, target: null, where: null, justDropped: false };
  function onDragDown(e) {
    if (e.button !== 0) return;
    const row = e.target.closest(".bm-row");
    if (!row || e.target.closest(".bm-tw") || e.target.closest("input")) return;
    drag.id = Number(row.dataset.id);
    drag.x = e.clientX;
    drag.y = e.clientY;
    drag.active = false;
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragUp, { once: true });
  }
  function clearDropMarks() {
    document.querySelectorAll(".bm-row.drop-before,.bm-row.drop-after,.bm-row.drop-inside")
      .forEach((r) => r.classList.remove("drop-before", "drop-after", "drop-inside"));
  }
  function onDragMove(e) {
    if (!drag.active) {
      if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
      drag.active = true;
      const f = find(drag.id);
      drag.ghost = document.createElement("div");
      drag.ghost.className = "bm-ghost";
      drag.ghost.textContent = f ? nodeLabel(f.node) : "";
      document.body.appendChild(drag.ghost);
      document.body.classList.add("bm-dragging");
    }
    drag.ghost.style.left = e.clientX + 12 + "px";
    drag.ghost.style.top = e.clientY + 8 + "px";
    clearDropMarks();
    drag.target = null;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const row = el && el.closest && el.closest("#bmTree .bm-row");
    if (row) {
      const tid = Number(row.dataset.id);
      if (tid === drag.id || isAncestor(drag.id, tid)) return;
      const r = row.getBoundingClientRect();
      const y = (e.clientY - r.top) / r.height;
      drag.where = y < 0.28 ? "before" : y > 0.72 ? "after" : "inside";
      drag.target = tid;
      row.classList.add("drop-" + drag.where);
    } else if (el && el.closest && el.closest("#bmTree")) {
      // Thả vào vùng trống cuối cây → thành bookmark cấp 1 cuối cùng.
      const last = bm.tree[bm.tree.length - 1];
      if (last && last._id !== drag.id) { drag.target = last._id; drag.where = "after"; }
    }
  }
  function onDragUp() {
    window.removeEventListener("pointermove", onDragMove);
    const wasActive = drag.active;
    if (drag.ghost) drag.ghost.remove();
    drag.ghost = null;
    document.body.classList.remove("bm-dragging");
    clearDropMarks();
    if (wasActive) {
      drag.justDropped = true; // chặn click sinh ra sau khi thả
      setTimeout(() => { drag.justDropped = false; }, 0);
      if (drag.target != null) moveNode(drag.id, drag.target, drag.where);
    }
    drag.active = false;
    drag.id = null;
  }

  // ---------------- Menu chuột phải ----------------
  function menuEl() {
    let m = $("bmMenu");
    if (!m) {
      m = document.createElement("div");
      m.id = "bmMenu";
      m.className = "menu mini-menu bm-menu hidden";
      m.setAttribute("role", "menu");
      document.body.appendChild(m);
      m.addEventListener("click", (e) => {
        const it = e.target.closest(".mitem");
        if (!it || it.classList.contains("disabled")) return;
        m.classList.add("hidden");
        const fn = menuActions[it.dataset.act];
        if (fn) fn();
      });
    }
    return m;
  }
  let menuActions = {};
  function showMenu(x, y, items) {
    const m = menuEl();
    if (window.Shell) Shell.closeMenus();
    menuActions = {};
    m.innerHTML = items.map((it, i) => {
      if (it === "-") return '<div class="msep"></div>';
      menuActions[i] = it.run;
      return `<button class="mitem${it.disabled ? " disabled" : ""}" data-act="${i}" role="menuitem">` +
        `<i data-icon="${it.icon || "bookmark"}"></i><span>${escapeHtml(it.label)}</span>${it.kbd ? `<kbd>${it.kbd}</kbd>` : ""}</button>`;
    }).join("");
    applyIcons(m);
    m.classList.remove("hidden");
    const r = m.getBoundingClientRect();
    m.style.left = Math.max(4, Math.min(x, window.innerWidth - r.width - 6)) + "px";
    m.style.top = Math.max(4, Math.min(y, window.innerHeight - r.height - 6)) + "px";
    m.style.right = "";
    const first = m.querySelector(".mitem:not(.disabled)");
    if (first) first.focus();
  }

  function showNodeMenu(x, y, f) {
    const has = !!f;
    const ready = !!state.path && bm.loaded;
    showMenu(x, y, [
      { label: t("bm.add"), icon: "bookmark-add", kbd: "Ctrl+Shift+B", disabled: !ready, run: () => addBookmark(false) },
      { label: t("bm.addChild"), icon: "bookmark-child", disabled: !has, run: () => addBookmark(true) },
      { label: t("bm.rename"), icon: "rename", kbd: "F2", disabled: !has, run: () => startRename(f.node._id) },
      { label: t("bm.delete"), icon: "trash", kbd: "Del", disabled: !has, run: deleteSelected },
      "-",
      { label: t("bm.promote"), icon: "outdent", disabled: !has || !f.parent, run: () => promote(f.node._id) },
      { label: t("bm.demote"), icon: "indent", disabled: !has || f.index === 0, run: () => demote(f.node._id) },
      "-",
      { label: t("bm.setDest"), icon: "cursor", disabled: !has, run: () => setDestHere(f.node._id) },
      { label: t("bm.props"), icon: "edit-text", disabled: !has, run: () => openProps(f.node._id) },
      "-",
      { label: t("bm.expandAll"), icon: "expand-all", disabled: !bm.tree.length, run: () => setAllOpen(true) },
      { label: t("bm.collapseAll"), icon: "collapse-all", disabled: !bm.tree.length, run: () => setAllOpen(false) },
      { label: t("bm.auto"), icon: "bookmark-auto", disabled: !ready, run: openAutoDialog },
    ]);
  }

  // ---------------- Thuộc tính bookmark ----------------
  function hex(c) { return "#" + (c || [0, 0, 0]).map((v) => v.toString(16).padStart(2, "0")).join(""); }
  function unhex(h) { const m = /^#?([0-9a-f]{6})$/i.exec(h || ""); if (!m) return null; const v = parseInt(m[1], 16); return [(v >> 16) & 255, (v >> 8) & 255, v & 255]; }

  function openProps(id) {
    const f = find(id);
    if (!f) return;
    const n = f.node;
    const N = state.pages.length;
    const box = openModal(escapeHtml(t("bm.propsTitle")), `
      <label>${escapeHtml(t("bm.fTitle"))}</label>
      <input type="text" id="bmpTitle">
      <label>${escapeHtml(t("bm.fAction"))}</label>
      <div class="radiorow">
        <label><input type="radio" name="bmpKind" value="page"${n.uri ? "" : " checked"}>${escapeHtml(t("bm.kindPage"))}</label>
        <label><input type="radio" name="bmpKind" value="url"${n.uri ? " checked" : ""}>${escapeHtml(t("bm.kindUrl"))}</label>
      </div>
      <div id="bmpPageRow" class="row">
        <div><label>${escapeHtml(t("common.page"))} (1–${N})</label><input type="number" id="bmpPage" min="1" max="${N}"></div>
        <div><label>${escapeHtml(t("bm.fView"))}</label>
          <select id="bmpView"><option value="xyz">${escapeHtml(t("bm.viewKeep"))}</option><option value="fit">${escapeHtml(t("bm.viewFit"))}</option></select></div>
      </div>
      <div id="bmpUrlRow"><label>${escapeHtml(t("lnk.fUrl"))}</label><input type="text" id="bmpUrl" placeholder="https://"></div>
      <label>${escapeHtml(t("bm.fStyle"))}</label>
      <div class="radiorow">
        <label><input type="checkbox" id="bmpBold"><b>${escapeHtml(t("bm.bold"))}</b></label>
        <label><input type="checkbox" id="bmpItalic"><i>${escapeHtml(t("bm.italic"))}</i></label>
        <label>${escapeHtml(t("common.color"))} <input type="color" id="bmpColor"></label>
      </div>
      <div class="err" id="bmpErr"></div>
      <div class="foot"><button id="bmpCancel">${escapeHtml(t("common.cancel"))}</button><button id="bmpOk" class="primary">${escapeHtml(t("common.ok"))}</button></div>`);
    const q = (s) => box.querySelector(s);
    q("#bmpTitle").value = n.title;
    q("#bmpPage").value = (n.pageIndex ?? currentPageIndex()) + 1;
    q("#bmpView").value = n.fit ? "fit" : "xyz";
    q("#bmpUrl").value = n.uri || "";
    q("#bmpBold").checked = n.bold;
    q("#bmpItalic").checked = n.italic;
    q("#bmpColor").value = hex(n.color);
    const sync = () => {
      const url = q('input[name="bmpKind"]:checked').value === "url";
      q("#bmpPageRow").classList.toggle("hidden", url);
      q("#bmpUrlRow").classList.toggle("hidden", !url);
    };
    box.querySelectorAll('input[name="bmpKind"]').forEach((r) => r.addEventListener("change", sync));
    sync();
    q("#bmpCancel").addEventListener("click", closeModal);
    q("#bmpOk").addEventListener("click", () => {
      const url = q('input[name="bmpKind"]:checked').value === "url";
      const title = q("#bmpTitle").value.replace(/\s+/g, " ").trim() || t("bm.untitled");
      let upd;
      if (url) {
        const u = normalizeUrl(q("#bmpUrl").value);
        if (!u) { q("#bmpErr").textContent = t("lnk.errUrl"); return; }
        upd = { uri: u, pageIndex: null, top: null, left: null, zoom: null, fit: false };
      } else {
        const p = parseInt(q("#bmpPage").value, 10);
        if (!(p >= 1 && p <= N)) { q("#bmpErr").textContent = t("lnk.errPage", { n: N }); return; }
        const fit = q("#bmpView").value === "fit";
        const samePage = n.pageIndex === p - 1;
        upd = { uri: null, pageIndex: p - 1, fit, top: fit || !samePage ? null : n.top, left: samePage ? n.left : null, zoom: samePage ? n.zoom : null };
      }
      const col = unhex(q("#bmpColor").value);
      closeModal();
      change(() => Object.assign(n, upd, {
        title, bold: q("#bmpBold").checked, italic: q("#bmpItalic").checked,
        color: col && (col[0] || col[1] || col[2]) ? col : null,
      }));
    });
  }

  // ---------------- Tự tạo từ tiêu đề ----------------
  function openAutoDialog() {
    if (!state.path) return;
    const box = openModal(escapeHtml(t("bm.autoTitle")), `
      <p>${escapeHtml(t("bm.autoIntro"))}</p>
      <label>${escapeHtml(t("bm.autoLevels"))}</label>
      <select id="bmaLevels"><option>1</option><option>2</option><option selected>3</option><option>4</option></select>
      <div class="radiorow bm-radio-gap">
        <label><input type="radio" name="bmaMode" value="replace" checked>${escapeHtml(t("bm.autoReplace"))}</label>
        <label><input type="radio" name="bmaMode" value="append">${escapeHtml(t("bm.autoAppend"))}</label>
      </div>
      <div class="err" id="bmaErr"></div>
      <div class="foot"><button id="bmaCancel">${escapeHtml(t("common.cancel"))}</button><button id="bmaOk" class="primary">${escapeHtml(t("bm.autoOk"))}</button></div>`);
    box.querySelector("#bmaCancel").addEventListener("click", closeModal);
    box.querySelector("#bmaOk").addEventListener("click", async () => {
      const levels = Number(box.querySelector("#bmaLevels").value) || 3;
      const append = box.querySelector('input[name="bmaMode"]:checked').value === "append";
      const ok = box.querySelector("#bmaOk");
      ok.disabled = true;
      ok.textContent = t("common.loading");
      const tok = bm.docTok;
      try {
        const tree = await invoke("bm_auto_generate", { path: state.path, maxLevels: levels });
        if (tok !== bm.docTok) { closeModal(); return; }
        if (!tree.length) { box.querySelector("#bmaErr").textContent = t("bm.autoNone"); ok.disabled = false; ok.textContent = t("bm.autoOk"); return; }
        closeModal();
        const nodes = withIds(tree);
        change(() => { bm.tree = append ? bm.tree.concat(nodes) : nodes; bm.selId = null; });
        let count = 0;
        forEachNode(() => count++, nodes);
        $("status").textContent = t("bm.autoDone", { n: count });
        if ($("outline").classList.contains("hidden")) $("tabOutline").click();
      } catch (e) {
        box.querySelector("#bmaErr").textContent = t("bm.err", { e });
        ok.disabled = false;
        ok.textContent = t("bm.autoOk");
      }
    });
  }

  // ---------------- Lưu / bỏ ----------------
  async function save() {
    if (!isDirty()) { $("status").textContent = t("shell.nothingToSave"); return; }
    // Lưu = ghi tệp mới rồi mở lại → chú thích chưa lưu sẽ mất: hỏi trước.
    if (state.annotSpecs.length && !$("saveAnnots").disabled) {
      const go = await confirmModal(t("bm.saveTitle"), t("bm.warnAnnots", { n: state.annotSpecs.length }), t("bm.saveAnyway"));
      if (!go) return;
    }
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    const nb = treeDirty();
    const nl = bm.linkAdd.length + bm.linkRemove.size;
    $("status").textContent = t("common.loading");
    try {
      await invoke("bm_save", {
        input: state.path,
        output: out,
        tree: nb ? strip(bm.tree) : null,
        removeLinks: [...bm.linkRemove],
        addLinks: bm.linkAdd.map((l) => ({
          pageIndex: l.pageIndex, rect: l.rect, destPage: l.destPage, destTop: l.destTop, uri: l.uri, border: l.border,
        })),
      });
      bm.path = null; // buộc nạp lại cả khi lưu đè đúng tệp đang mở
      await loadDocument(out);
      $("status").textContent = t("bm.saved", { file: shortName(out) });
    } catch (e) {
      $("status").textContent = t("bm.err", { e });
    }
  }

  async function discard() {
    if (!isDirty()) return;
    if (!(await confirmModal(t("bm.discardTitle"), t("bm.discardMsg"), t("bm.discardOk")))) return;
    change(() => {
      bm.tree = withIds(JSON.parse(bm.savedKey));
      bm.linkAdd = [];
      bm.linkRemove = new Set();
      bm.selId = null;
    });
    // savedKey bỏ "open" → giữ nguyên trạng thái mở theo mặc định (đóng).
    bm.savedKey = treeKey();
    renderTree();
    redrawAllLinks();
  }

  // ================= LIÊN KẾT =================
  const linkKey = (l) => l.tmpId || l.id;
  function pageLinks(idx) {
    if (bm.linkPath !== state.path) return bm.linkAdd.filter((l) => l.pageIndex === idx);
    return bm.links.filter((l) => l.pageIndex === idx && !bm.linkRemove.has(l.id))
      .concat(bm.linkAdd.filter((l) => l.pageIndex === idx));
  }
  function linkTip(l) {
    if (l.uri) return t("lnk.tipUrl", { url: l.uri });
    if (l.destPage != null) return t("lnk.tipPage", { n: l.destPage + 1 });
    return t("lnk.tipOther", { what: l.other || "?" });
  }

  function drawLinksForPage(idx) {
    const slot = state.slots[idx];
    if (!slot) return;
    let layer = slot.querySelector(".linklayer");
    const items = pageLinks(idx);
    if (!layer && !items.length) return;
    if (!layer) {
      layer = document.createElement("div");
      layer.className = "linklayer";
      slot.appendChild(layer);
    }
    layer.innerHTML = "";
    const p = state.pages[idx];
    if (!p) return;
    const scale = PT_PER_PX * state.zoom;
    for (const l of items) {
      const k = linkKey(l);
      const a = document.createElement("div");
      a.className = "lnk" + (l.tmpId ? " pending" : "") + (bm.linkSel === k ? " sel" : "");
      a.dataset.key = k;
      Object.assign(a.style, {
        left: l.rect.left * scale + "px",
        top: (p.heightPt - l.rect.top) * scale + "px",
        width: Math.max(2, (l.rect.right - l.rect.left) * scale) + "px",
        height: Math.max(2, (l.rect.top - l.rect.bottom) * scale) + "px",
      });
      if (l.border) a.style.borderColor = rgbCss(l.border);
      a.title = linkTip(l);
      a.addEventListener("click", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        if (bm.armed) { selectLink(k); return; }
        followLink(l);
      });
      a.addEventListener("dblclick", (ev) => { ev.stopPropagation(); if (bm.armed) editLink(l); });
      a.addEventListener("contextmenu", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        selectLink(k);
        showMenu(ev.clientX, ev.clientY, [
          { label: l.uri ? t("lnk.open") : t("lnk.goto"), icon: "link", disabled: !l.uri && l.destPage == null, run: () => followLink(l) },
          { label: t("lnk.edit"), icon: "edit-text", run: () => editLink(l) },
          { label: t("lnk.delete"), icon: "trash", kbd: "Del", run: () => deleteLink(k) },
        ]);
      });
      if (bm.linkSel === k) {
        const del = document.createElement("div");
        del.className = "lnk-del";
        del.textContent = "✕";
        del.title = t("lnk.delete");
        del.addEventListener("click", (ev) => { ev.stopPropagation(); deleteLink(k); });
        a.appendChild(del);
      }
      layer.appendChild(a);
    }
  }
  function redrawAllLinks() {
    for (const slot of state.slots) if (slot) drawLinksForPage(Number(slot.dataset.index));
    updateDirty();
  }
  // Vẽ lại liên kết mỗi khi main.js vẽ lại chú thích của trang (render/zoom/…).
  const origDrawAnnots = window.drawAnnotsForPage;
  if (typeof origDrawAnnots === "function") {
    window.drawAnnotsForPage = function (idx) {
      origDrawAnnots(idx);
      try { drawLinksForPage(idx); } catch (err) { console.error(err); }
    };
  }

  function selectLink(k) {
    const prev = bm.linkSel;
    bm.linkSel = k;
    const pages = new Set();
    for (const l of bm.links.concat(bm.linkAdd)) if (linkKey(l) === k || linkKey(l) === prev) pages.add(l.pageIndex);
    pages.forEach(drawLinksForPage);
  }

  async function followLink(l) {
    if (l.uri) { openExternal(l.uri); return; }
    if (l.destPage != null) { navigateTo(l.destPage, l.destTop); return; }
    $("status").textContent = t("lnk.unsupported", { what: l.other || "?" });
  }

  async function openExternal(url) {
    const go = await confirmModal(t("lnk.openTitle"), t("lnk.openMsg", { url }), t("lnk.openOk"));
    if (!go) return;
    try {
      await invoke("open_url", { url });
    } catch (e) {
      $("status").textContent = t("lnk.openErr", { e });
    }
  }

  function deleteLink(k) {
    const pending = bm.linkAdd.find((l) => l.tmpId === k);
    const orig = bm.links.find((l) => l.id === k);
    if (!pending && !orig) return;
    if (typeof pushUndo === "function") pushUndo();
    if (pending) bm.linkAdd = bm.linkAdd.filter((l) => l.tmpId !== k);
    else bm.linkRemove.add(k);
    bm.linkSel = null;
    drawLinksForPage((pending || orig).pageIndex);
    updateDirty();
    $("status").textContent = t("lnk.deleted");
  }

  function editLink(l) {
    openLinkDialog(l, (target) => {
      if (typeof pushUndo === "function") pushUndo();
      if (l.tmpId) {
        Object.assign(l, target);
      } else {
        bm.linkRemove.add(l.id);
        bm.linkAdd.push({ tmpId: "n" + bm.seq++, pageIndex: l.pageIndex, rect: { ...l.rect }, ...target });
      }
      bm.linkSel = null;
      drawLinksForPage(l.pageIndex);
      updateDirty();
    });
  }

  function normalizeUrl(s) {
    let u = (s || "").trim();
    if (!u) return null;
    if (/^[^\s@/]+@[^\s@/]+\.[^\s@/]+$/.test(u)) u = "mailto:" + u;
    else if (!/^[a-z][a-z0-9+.-]*:/i.test(u)) u = "https://" + u;
    if (!/^(https?:\/\/[^\s/]+|ftp:\/\/[^\s/]+|mailto:[^\s]+)/i.test(u) || /\s/.test(u)) return null;
    return u;
  }

  // Hộp chọn đích liên kết (tạo mới hoặc sửa). onOk({destPage,destTop,uri,border}).
  function openLinkDialog(l, onOk) {
    const N = state.pages.length;
    const isUrl = !!(l && l.uri);
    const box = openModal(escapeHtml(t(l && (l.id || l.tmpId) ? "lnk.editTitle" : "lnk.newTitle")), `
      <label>${escapeHtml(t("lnk.fAction"))}</label>
      <div class="radiorow">
        <label><input type="radio" name="lnkKind" value="page"${isUrl ? "" : " checked"}>${escapeHtml(t("lnk.kindPage"))}</label>
        <label><input type="radio" name="lnkKind" value="url"${isUrl ? " checked" : ""}>${escapeHtml(t("lnk.kindUrl"))}</label>
      </div>
      <div id="lnkPageRow"><label>${escapeHtml(t("common.page"))} (1–${N})</label><input type="number" id="lnkPage" min="1" max="${N}"></div>
      <div id="lnkUrlRow"><label>${escapeHtml(t("lnk.fUrl"))}</label><input type="text" id="lnkUrl" placeholder="https://"></div>
      <label>${escapeHtml(t("lnk.fStyle"))}</label>
      <div class="radiorow">
        <select id="lnkStyle"><option value="none">${escapeHtml(t("lnk.styleNone"))}</option><option value="box">${escapeHtml(t("lnk.styleBox"))}</option></select>
        <input type="color" id="lnkColor" value="#0000ff">
      </div>
      <div class="err" id="lnkErr"></div>
      <div class="foot"><button id="lnkCancel">${escapeHtml(t("common.cancel"))}</button><button id="lnkOk" class="primary">${escapeHtml(t("common.ok"))}</button></div>`);
    const q = (s) => box.querySelector(s);
    const destPage = l && l.destPage != null ? l.destPage : Math.min(N - 1, currentPageIndex() + 1);
    q("#lnkPage").value = destPage + 1;
    q("#lnkUrl").value = (l && l.uri) || "";
    q("#lnkStyle").value = l && l.border ? "box" : "none";
    if (l && l.border) q("#lnkColor").value = hex(l.border);
    const sync = () => {
      const url = q('input[name="lnkKind"]:checked').value === "url";
      q("#lnkPageRow").classList.toggle("hidden", url);
      q("#lnkUrlRow").classList.toggle("hidden", !url);
      q("#lnkColor").classList.toggle("hidden", q("#lnkStyle").value !== "box");
      setTimeout(() => { const el = url ? q("#lnkUrl") : q("#lnkPage"); if (el) el.focus(); }, 0);
    };
    box.querySelectorAll('input[name="lnkKind"]').forEach((r) => r.addEventListener("change", sync));
    q("#lnkStyle").addEventListener("change", sync);
    sync();
    const submit = () => {
      const url = q('input[name="lnkKind"]:checked').value === "url";
      const border = q("#lnkStyle").value === "box" ? unhex(q("#lnkColor").value) || [0, 0, 255] : null;
      if (url) {
        const u = normalizeUrl(q("#lnkUrl").value);
        if (!u) { q("#lnkErr").textContent = t("lnk.errUrl"); return; }
        closeModal();
        onOk({ uri: u, destPage: null, destTop: null, border });
      } else {
        const p = parseInt(q("#lnkPage").value, 10);
        if (!(p >= 1 && p <= N)) { q("#lnkErr").textContent = t("lnk.errPage", { n: N }); return; }
        closeModal();
        const keepTop = l && l.destPage === p - 1 ? l.destTop : null;
        onOk({ uri: null, destPage: p - 1, destTop: keepTop ?? null, border });
      }
    };
    q("#lnkCancel").addEventListener("click", closeModal);
    q("#lnkOk").addEventListener("click", submit);
    box.querySelectorAll("input[type=text],input[type=number]").forEach((i) =>
      i.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); submit(); } }));
  }

  // ---------- Công cụ Liên kết (kéo khung trên trang) ----------
  function setArmed(on) {
    bm.armed = on;
    document.body.classList.toggle("lnk-armed", on);
    document.querySelectorAll(".lnk-tool").forEach((b) => b.classList.toggle("armed", on));
    if (!on && bm.linkSel) { const k = bm.linkSel; bm.linkSel = null; selectLinkRedraw(k); }
    $("status").textContent = on ? t("lnk.armHint") : "";
  }
  function selectLinkRedraw(k) {
    for (const l of bm.links.concat(bm.linkAdd)) if (linkKey(l) === k) drawLinksForPage(l.pageIndex);
  }

  async function onLinkToolClick() {
    if (!state.path) return;
    if (bm.armed) { setArmed(false); return; }
    if (state.editMode && window.Shell) {
      Shell.setTab("annotate"); // rời chế độ sửa (hỏi nếu còn thay đổi)
      if (state.editMode) return;
    } else if (window.Shell && state.organizeMode) {
      Shell.setTab("annotate");
    }
    if (state.tool) setTool(state.tool); // tắt công cụ chú thích đang bật
    if (window.Shell && Shell.currentTab !== "annotate" && Shell.currentTab !== "home") Shell.setTab("annotate");
    setArmed(true);
  }

  // Bật công cụ chú thích khác → tắt công cụ Liên kết.
  const origSetTool = window.setTool;
  if (typeof origSetTool === "function") {
    window.setTool = function (tl) {
      origSetTool(tl);
      if (state.tool && bm.armed) setArmed(false);
    };
  }

  let band = null;
  function onPagesDown(e) {
    if (!bm.armed || e.button !== 0 || state.tool) return;
    if (e.target.closest(".lnk")) return;
    const slot = e.target.closest(".page-slot");
    if (!slot) return;
    e.preventDefault();
    e.stopPropagation();
    if (bm.linkSel) { const k = bm.linkSel; bm.linkSel = null; selectLinkRedraw(k); }
    const r = slot.getBoundingClientRect();
    const el = document.createElement("div");
    el.className = "lnk-band";
    slot.appendChild(el);
    band = { slot, idx: Number(slot.dataset.index), x0: e.clientX - r.left, y0: e.clientY - r.top, el, x1: 0, y1: 0 };
    band.x1 = band.x0; band.y1 = band.y0;
    window.addEventListener("mousemove", onBandMove);
    window.addEventListener("mouseup", onBandUp, { once: true });
  }
  function onBandMove(e) {
    if (!band) return;
    const r = band.slot.getBoundingClientRect();
    band.x1 = Math.max(0, Math.min(r.width, e.clientX - r.left));
    band.y1 = Math.max(0, Math.min(r.height, e.clientY - r.top));
    Object.assign(band.el.style, {
      left: Math.min(band.x0, band.x1) + "px", top: Math.min(band.y0, band.y1) + "px",
      width: Math.abs(band.x1 - band.x0) + "px", height: Math.abs(band.y1 - band.y0) + "px",
    });
  }
  function onBandUp() {
    window.removeEventListener("mousemove", onBandMove);
    const b = band;
    band = null;
    if (!b) return;
    b.el.remove();
    if (Math.abs(b.x1 - b.x0) < 6 || Math.abs(b.y1 - b.y0) < 6) {
      $("status").textContent = t("lnk.armHint");
      return;
    }
    const p = state.pages[b.idx];
    const scale = PT_PER_PX * state.zoom;
    const rect = {
      left: Math.min(b.x0, b.x1) / scale,
      right: Math.max(b.x0, b.x1) / scale,
      top: p.heightPt - Math.min(b.y0, b.y1) / scale,
      bottom: p.heightPt - Math.max(b.y0, b.y1) / scale,
    };
    openLinkDialog(null, (target) => {
      if (typeof pushUndo === "function") pushUndo();
      const nl = { tmpId: "n" + bm.seq++, pageIndex: b.idx, rect, ...target };
      bm.linkAdd.push(nl);
      bm.linkSel = null;
      drawLinksForPage(b.idx);
      updateDirty();
      $("status").textContent = t("lnk.added");
    });
  }

  // ================= TÁCH TỆP =================
  function dirOf(p) { const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/")); return i > 0 ? p.slice(0, i) : p; }

  // Kiểm tra "1-3, 4-10, 11-" phía UI (thông báo theo ngôn ngữ); engine kiểm lại.
  function checkRanges(spec, n) {
    const parts = spec.split(/[,;]/).map((s) => s.trim()).filter(Boolean);
    if (!parts.length) return t("spl.errRangesEmpty");
    for (const p of parts) {
      const m = /^(\d*)\s*(-)?\s*(\d*)$/.exec(p);
      if (!m || (!m[1] && !m[3]) || (!m[2] && !m[1])) return t("spl.errRange", { r: p });
      const a = m[1] ? Number(m[1]) : 1;
      const b = m[2] ? (m[3] ? Number(m[3]) : n) : a;
      if (a < 1 || b < a) return t("spl.errRange", { r: p });
      if (a > n) return t("spl.errRangeMax", { r: p, n });
    }
    return null;
  }

  window.openSplitDialogEx = function () {
    if (!state.path) return;
    const n = state.pages.length;
    const src = state.origPath || state.path;
    const base = shortName(src).replace(/(\.pdf)+$/i, "");
    const tops = bm.tree.filter((b) => b.pageIndex != null).length;
    const box = openModal(escapeHtml(t("orgx.splitTitle")), `
      <p class="muted">${escapeHtml(t("spl.intro", { n }))}</p>
      <div class="spl-modes">
        <label class="spl-opt"><input type="radio" name="splMode" value="count" checked><span>${escapeHtml(t("orgx.splitPerFile"))}</span><input type="number" id="splCount" value="1" min="1" max="${n}"></label>
        <label class="spl-opt"><input type="radio" name="splMode" value="size"><span>${escapeHtml(t("spl.bySize"))}</span><input type="number" id="splSize" value="5" min="0.1" step="0.5"></label>
        <label class="spl-opt${tops ? "" : " disabled"}"><input type="radio" name="splMode" value="bookmarks"${tops ? "" : " disabled"}><span>${escapeHtml(t("spl.byBookmarks"))}${tops ? "" : " — " + escapeHtml(t("spl.noBookmarks"))}</span></label>
        <label class="spl-opt"><input type="radio" name="splMode" value="ranges"><span>${escapeHtml(t("spl.byRanges"))}</span><input type="text" id="splRanges" placeholder="1-3, 4-10, 11-"></label>
      </div>
      <p class="muted" id="splNote"></p>
      <label>${escapeHtml(t("spl.outDir"))}</label>
      <div class="spl-dir"><input type="text" id="splDir" readonly><button id="splBrowse">${escapeHtml(t("common.browse"))}</button></div>
      <label>${escapeHtml(t("spl.baseName"))}</label>
      <input type="text" id="splBase">
      <div class="err" id="splErr"></div>
      <div class="foot"><button id="splCancel">${escapeHtml(t("common.cancel"))}</button><button id="splOk" class="primary">${escapeHtml(t("orgx.splitOk"))}</button></div>`);
    const q = (s) => box.querySelector(s);
    q("#splDir").value = dirOf(src);
    q("#splBase").value = base;
    const mode = () => q('input[name="splMode"]:checked').value;
    const sync = () => {
      const m = mode();
      const notes = { count: "spl.noteCount", size: "spl.noteSize", bookmarks: "spl.noteBookmarks", ranges: "spl.noteRanges" };
      q("#splNote").textContent = t(notes[m]) + (m === "bookmarks" && treeDirty() ? " " + t("spl.noteUnsavedBm") : "");
      q("#splErr").textContent = "";
    };
    box.querySelectorAll('input[name="splMode"]').forEach((r) => r.addEventListener("change", sync));
    // Gõ vào ô của chế độ nào thì chọn chế độ đó.
    for (const [id, m] of [["#splCount", "count"], ["#splSize", "size"], ["#splRanges", "ranges"]]) {
      q(id).addEventListener("focus", () => { q(`input[name="splMode"][value="${m}"]`).checked = true; sync(); });
    }
    sync();
    q("#splBrowse").addEventListener("click", async () => {
      const d = await invoke("pick_dir");
      if (d) q("#splDir").value = d;
    });
    q("#splCancel").addEventListener("click", closeModal);
    q("#splOk").addEventListener("click", async () => {
      const m = mode();
      const outDir = q("#splDir").value.trim();
      const baseName = (q("#splBase").value.trim() || base).replace(/[<>:"/\\|?*]/g, "_");
      const err = (s) => { q("#splErr").textContent = s; };
      if (!outDir) { err(t("spl.errDir")); return; }
      let cmd, args;
      if (m === "count") {
        const k = Math.floor(Number(q("#splCount").value));
        if (!(k >= 1)) { err(t("spl.errCount")); return; }
        cmd = "organize_split"; args = { input: state.path, pagesPerFile: Math.min(k, 65535), outDir, baseName, password: null };
      } else if (m === "size") {
        const mb = Number(q("#splSize").value);
        if (!(mb > 0)) { err(t("spl.errSize")); return; }
        cmd = "split_by_size"; args = { input: state.path, maxMb: mb, outDir, baseName };
      } else if (m === "bookmarks") {
        cmd = "split_by_bookmarks"; args = { input: state.path, outDir, baseName };
      } else {
        const spec = q("#splRanges").value;
        const bad = checkRanges(spec, n);
        if (bad) { err(bad); return; }
        cmd = "split_by_ranges"; args = { input: state.path, spec, outDir, baseName };
      }
      const ok = q("#splOk");
      ok.disabled = true;
      ok.textContent = t("common.loading");
      $("status").textContent = t("spl.running");
      try {
        const outs = await invoke(cmd, args);
        closeModal();
        $("status").textContent = t("orgx.splitDone", { n: outs.length, dir: outDir });
      } catch (e) {
        err(t("orgx.err", { e }));
        ok.disabled = false;
        ok.textContent = t("orgx.splitOk");
        $("status").textContent = "";
      }
    });
  };

  // ================= Phím tắt & sự kiện chung =================
  const typingEl = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  window.addEventListener("keydown", (e) => {
    if (!$("modalOverlay").classList.contains("hidden")) return;
    const typing = typingEl(e.target);
    // Ctrl+Shift+B: thêm bookmark tại khung nhìn hiện tại (Ctrl+B = chữ đậm khi sửa).
    if (e.ctrlKey && e.shiftKey && !e.altKey && (e.key === "b" || e.key === "B") && !(typing && e.target.isContentEditable)) {
      if (!state.path || state.editMode) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      bm.lastSel = pageSelectionText();
      addBookmark(false);
      return;
    }
    if (typing) return;
    if (e.key === "Escape" && bm.armed && !document.querySelector(".menu:not(.submenu):not(.hidden)")) {
      e.stopImmediatePropagation();
      setArmed(false);
      return;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && bm.linkSel && !state.editMode) {
      e.preventDefault();
      e.stopImmediatePropagation();
      deleteLink(bm.linkSel);
    }
  }, true);

  // Bấm ra ngoài liên kết → bỏ chọn.
  document.addEventListener("mousedown", (e) => {
    if (bm.linkSel && !e.target.closest(".lnk") && !e.target.closest(".menu") && !e.target.closest("#modalBox")) {
      const k = bm.linkSel;
      bm.linkSel = null;
      selectLinkRedraw(k);
    }
  });

  // Chọn chữ trên trang: nhớ để làm tiêu đề bookmark mới (bấm nút có thể bỏ chọn).
  document.addEventListener("selectionchange", () => {
    const s = pageSelectionText();
    if (s) bm.lastSel = s;
  });
  document.addEventListener("mousedown", (e) => {
    if (e.target.closest(".page-slot")) bm.lastSel = "";
  }, true);

  function boot() {
    ensurePanel();
    renderTree();
    const pages = $("pages");
    pages.addEventListener("mousedown", onPagesDown, true);
    // Đúp chuột lúc đang bật công cụ Liên kết: không vào chế độ sửa nội dung.
    pages.addEventListener("dblclick", (e) => { if (bm.armed) e.stopPropagation(); }, true);
    document.querySelectorAll(".lnk-tool").forEach((b) => b.addEventListener("click", onLinkToolClick));
    for (const id of ["saveAnnots", "secRedactApply"]) { const b = $(id); if (b) b.addEventListener("click", armCarry, true); }
    $("openBtn").addEventListener("click", () => { carry = null; }, true);
    document.addEventListener("docloaded", () => { if (bm.armed) setArmed(false); });
  }
  boot();

  window.Bookmarks = {
    isDirty,
    save,
    addBookmark,
    get tree() { return bm.tree; },
  };
})();
