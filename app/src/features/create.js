// Tab Chuyển đổi kiểu Foxit: nhóm "Tạo PDF" (từ ảnh / Office / văn bản / trang
// web / máy quét / clipboard / trang trắng / gộp tệp), nhóm "OCR" (OCR tài
// liệu có tuỳ chọn, OCR thẳng ảnh) và nhóm "Xuất" (Word / Excel / PowerPoint /
// ảnh PNG-JPEG-TIFF / HTML / văn bản / RTF) — mọi hộp thoại xuất dựng từ MỘT
// bộ dựng chung (EXPORTS + openExportDialog). Lệnh Tauri: cmd_create.rs.
(function () {
  let tools = null;      // { browser, soffice, tesseract, scanner, clipboard }
  let busy = false;

  async function getTools() {
    if (!tools) {
      try { tools = await invoke("create_tools_status"); } catch (_) { tools = {}; }
    }
    return tools;
  }

  // ---------- Tiện ích chung ----------
  function say(msg) {
    $("status").textContent = msg;
    const h = $("convHint");
    if (h) h.textContent = state.convMode ? msg : "";
  }
  function clearHint() {
    const h = $("convHint");
    if (h) h.textContent = "";
  }
  function baseName(p) {
    return shortName(p || "").replace(/\.[^.]+$/, "") || "document";
  }
  function hasDoc() {
    if (state.path && state.pages && state.pages.length) return true;
    say(t("create.needDoc"));
    return false;
  }
  // Chạy tác vụ nặng: chặn bấm đúp, con trỏ bận, thông báo đang làm.
  async function run(msg, fn) {
    if (busy) { say(t("create.busy")); return undefined; }
    busy = true;
    document.body.classList.add("cr-busy");
    say(msg);
    try {
      return await fn();
    } finally {
      busy = false;
      document.body.classList.remove("cr-busy");
    }
  }
  // Mở kết quả vừa tạo (hỏi trước nếu tài liệu đang mở còn thay đổi chưa lưu).
  async function openResult(path, msg) {
    say(msg);
    if (window.Shell && Shell.confirmDiscardChanges && !Shell.confirmDiscardChanges()) return;
    await loadDocument(path);
    $("status").textContent = msg;
    clearHint();
  }
  async function pickSavePdf(name) {
    return invoke("pick_save_as", { ext: "pdf", name: name.replace(/\.pdf$/i, "") + ".pdf" });
  }
  function errText(e) {
    return String(e && e.message ? e.message : e);
  }
  function modal(titleKey, html) {
    return openModal(escapeHtml(t(titleKey)), html);
  }
  // Lớp .cr-modal (độ rộng hộp thoại) chỉ sống cùng hộp thoại của tệp này:
  // đóng modal / modal khác mở đè → gỡ để không ảnh hưởng tính năng khác.
  new MutationObserver(() => {
    const box = $("modalBox");
    if ($("modalOverlay").classList.contains("hidden") || !box.querySelector("#crCancel")) box.classList.remove("cr-modal");
  }).observe($("modalOverlay"), { attributes: true, attributeFilter: ["class"], subtree: true, childList: true });
  function opt(value, key, sel) {
    return `<option value="${value}"${value === sel ? " selected" : ""}>${escapeHtml(t(key))}</option>`;
  }
  function langOptions(sel) {
    return [["vie+eng", "ribbon.convert.langViEn"], ["vie", "ribbon.convert.langVi"], ["eng", "ribbon.convert.langEn"]]
      .map(([v, k]) => opt(v, k, sel)).join("");
  }
  const SIZES = [
    ["a4", "create.size.a4"], ["letter", "create.size.letter"], ["a3", "create.size.a3"],
    ["a5", "create.size.a5"], ["legal", "create.size.legal"],
  ];
  function sizeOptions(sel, extra) {
    const list = (extra && extra.fit ? [["fit", "create.size.fit"]] : []).concat(SIZES)
      .concat(extra && extra.custom ? [["custom", "create.size.custom"]] : []);
    return list.map(([v, k]) => opt(v, k, sel)).join("");
  }

  // ---------- Popover dropdown trên ribbon ----------
  function bindDropdown(btn) {
    const menu = $(btn.dataset.menu);
    let wasOpen = false;
    btn.addEventListener("mousedown", () => { wasOpen = !menu.classList.contains("hidden"); });
    btn.addEventListener("click", () => {
      if (wasOpen) { menu.classList.add("hidden"); wasOpen = false; return; }
      document.querySelectorAll(".cr-menu").forEach((m) => m.classList.add("hidden"));
      menu.classList.remove("hidden");
      const r = btn.getBoundingClientRect();
      const w = menu.offsetWidth || 240;
      menu.style.right = "";
      menu.style.left = Math.max(8, Math.min(r.left, window.innerWidth - w - 8)) + "px";
      menu.style.top = r.bottom + 4 + "px";
    });
    menu.addEventListener("click", (e) => {
      if (e.target.closest(".mitem")) menu.classList.add("hidden");
    });
  }
  document.querySelectorAll(".cr-drop[data-menu]").forEach(bindDropdown);

  // ---------- Trường "Phạm vi trang" dùng chung ----------
  function rangeHtml() {
    return `<label>${escapeHtml(t("create.range"))}</label>
      <div class="radiorow cr-range">
        <label><input type="radio" name="crRange" value="all" checked>${escapeHtml(t("create.rangeAll"))}</label>
        <label><input type="radio" name="crRange" value="cur">${escapeHtml(t("create.rangeCurrent", { n: currentPageIndex() + 1 }))}</label>
        <label><input type="radio" name="crRange" value="custom">${escapeHtml(t("create.rangeCustom"))}</label>
        <input type="text" id="crRangeTxt" class="cr-range-input" placeholder="${escapeHtml(t("create.rangePh"))}">
      </div>`;
  }
  function wireRange(box) {
    const txt = box.querySelector("#crRangeTxt");
    if (!txt) return;
    txt.addEventListener("focus", () => { box.querySelector('input[name=crRange][value=custom]').checked = true; });
  }
  // → { pages } (mảng rỗng = mọi trang) hoặc { error }.
  function readRange(box) {
    const v = (box.querySelector("input[name=crRange]:checked") || {}).value || "all";
    if (v === "all") return { pages: [] };
    if (v === "cur") return { pages: [currentPageIndex()] };
    const pages = parsePageRange(box.querySelector("#crRangeTxt").value, state.pages.length);
    return pages.length ? { pages } : { error: t("create.rangeInvalid", { n: state.pages.length }) };
  }

  // ---------- Xuất: bảng mô tả + bộ dựng hộp thoại chung ----------
  const EXPORTS = {
    docx: { title: "create.exp.titleWord", ext: "docx", fields: ["range", "office"] },
    xlsx: { title: "create.exp.titleExcel", ext: "xlsx", fields: ["range"], note: "create.exp.excelNote" },
    pptx: { title: "create.exp.titlePpt", ext: "pptx", fields: ["range", "pptMode", "dpi", "office"], dpi: 150 },
    png: { title: "create.exp.titlePng", dir: true, fields: ["range", "dpi"], dpi: 150 },
    jpeg: { title: "create.exp.titleJpeg", dir: true, fields: ["range", "dpi", "quality"], dpi: 150 },
    tiff: { title: "create.exp.titleTiff", ext: "tif", fields: ["range", "dpi"], dpi: 200 },
    html: { title: "create.exp.titleHtml", ext: "html", fields: ["range", "htmlMode"] },
    txt: { title: "create.exp.titleTxt", ext: "txt", fields: ["range"] },
    rtf: { title: "create.exp.titleRtf", ext: "rtf", fields: ["range"] },
  };
  const FIELD_HTML = {
    range: () => rangeHtml(),
    dpi: (s) => `<label>${escapeHtml(t("create.exp.dpi"))}</label>
      <select id="crDpi">${[72, 96, 150, 200, 300, 600].map((d) => `<option value="${d}"${d === s.dpi ? " selected" : ""}>${d} DPI</option>`).join("")}</select>`,
    quality: () => `<label>${escapeHtml(t("create.exp.quality"))} <span id="crQualVal" class="muted">85</span></label>
      <input type="range" id="crQuality" min="10" max="100" step="5" value="85">`,
    pptMode: () => `<label>${escapeHtml(t("create.exp.pptMode"))}</label>
      <div class="radiorow"><label><input type="radio" name="crPpt" value="editable" checked>${escapeHtml(t("create.exp.pptEditable"))}</label>
      <label><input type="radio" name="crPpt" value="picture">${escapeHtml(t("create.exp.pptPicture"))}</label></div>`,
    htmlMode: () => `<label>${escapeHtml(t("create.exp.htmlMode"))}</label>
      <div class="radiorow"><label><input type="radio" name="crHtml" value="positioned" checked>${escapeHtml(t("create.exp.htmlPositioned"))}</label>
      <label><input type="radio" name="crHtml" value="flowing">${escapeHtml(t("create.exp.htmlFlowing"))}</label></div>`,
    office: (s, tl) => tl.soffice
      ? `<label><input type="checkbox" id="crOffice"${s.ext === "docx" ? " checked" : ""}>${escapeHtml(t("create.exp.useOffice"))}</label>`
      : `<p class="muted">${escapeHtml(t("create.exp.noOffice"))}</p>`,
  };

  async function openExportDialog(fmt) {
    if (!hasDoc()) return;
    const spec = EXPORTS[fmt];
    const tl = await getTools();
    const body = spec.fields.map((f) => FIELD_HTML[f](spec, tl)).join("")
      + (spec.note ? `<p class="muted">${escapeHtml(t(spec.note))}</p>` : "")
      + `<div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("create.exp.run"))}</button></div>`;
    const box = modal(spec.title, body);
    box.classList.add("cr-modal");
    wireRange(box);
    const q = box.querySelector("#crQuality");
    if (q) q.addEventListener("input", () => { box.querySelector("#crQualVal").textContent = q.value; });
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    box.querySelector("#crOk").addEventListener("click", async () => {
      const r = readRange(box);
      if (r.error) { box.querySelector("#crErr").textContent = r.error; return; }
      const val = (sel) => { const el = box.querySelector(sel); return el ? el.value : undefined; };
      const opts = {
        pages: r.pages,
        dpi: val("#crDpi") ? Number(val("#crDpi")) : undefined,
        quality: q ? Number(q.value) : undefined,
        useOffice: !!(box.querySelector("#crOffice") && box.querySelector("#crOffice").checked),
        pptMode: val("input[name=crPpt]:checked"),
        htmlMode: val("input[name=crHtml]:checked"),
        sheetPrefix: t("create.exp.sheetPrefix"),
      };
      closeModal();
      const target = spec.dir
        ? await invoke("pick_dir")
        : await invoke("pick_save_as", { ext: spec.ext, name: baseName(state.path) + "." + spec.ext });
      if (!target) return;
      const label = t(spec.title);
      try {
        const res = await run(t("create.exp.running", { what: label }), () =>
          invoke("export_run", { input: state.path, format: fmt, opts, target }));
        if (!res) return;
        const engine = res.engine === "libreoffice" ? " — " + t("conv.engineLibreOffice") : "";
        say(spec.dir
          ? t("create.exp.doneImages", { n: res.files.length, dir: target })
          : t("create.exp.done", { what: label, file: shortName(target) }) + engine);
      } catch (e) {
        say(t("create.exp.error", { what: label, e: errText(e) }));
      }
    });
  }

  const EXPORT_BUTTONS = {
    cvDocx: "docx", cvXlsx: "xlsx", cvPptx: "pptx", cvPng: "png", cvJpeg: "jpeg",
    cvTiff: "tiff", cvHtml: "html", cvTxt: "txt", cvRtf: "rtf",
  };
  for (const [id, fmt] of Object.entries(EXPORT_BUTTONS)) {
    $(id).addEventListener("click", () => openExportDialog(fmt));
  }

  // ---------- Danh sách tệp (ảnh / gộp tệp) dùng chung ----------
  // items: [{ path, kind, pages }]
  function listHtml(items, showKind) {
    if (!items.length) return `<div class="cr-empty muted">${escapeHtml(t("create.listEmpty"))}</div>`;
    return items.map((it, i) => {
      const meta = [];
      if (showKind) meta.push(t("create.kind." + (it.kind || "unknown")));
      if (it.pages) meta.push(t("create.pagesN", { n: it.pages }));
      return `<div class="cr-item${it.kind === "" ? " bad" : ""}" data-i="${i}">
        <span class="cr-idx">${i + 1}</span>
        <span class="cr-name" title="${escapeHtml(it.path)}">${escapeHtml(shortName(it.path))}</span>
        <span class="cr-meta muted">${escapeHtml(meta.join(" · "))}</span>
        <button class="cr-ib" data-act="up" title="${escapeHtml(t("create.moveUp"))}"${i === 0 ? " disabled" : ""}><i data-icon="arrow-up"></i></button>
        <button class="cr-ib" data-act="down" title="${escapeHtml(t("create.moveDown"))}"${i === items.length - 1 ? " disabled" : ""}><i data-icon="arrow-down"></i></button>
        <button class="cr-ib" data-act="del" title="${escapeHtml(t("create.remove"))}"><i data-icon="trash"></i></button>
      </div>`;
    }).join("");
  }
  // Gắn danh sách 1 LẦN; trả hàm render() để vẽ lại sau khi items thay đổi.
  function wireList(box, items, showKind, onChange) {
    const el = box.querySelector("#crList");
    const render = () => {
      el.innerHTML = listHtml(items, showKind);
      applyIcons(el);
      if (onChange) onChange();
    };
    el.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-act]");
      if (!b) return;
      const i = Number(b.closest(".cr-item").dataset.i);
      if (b.dataset.act === "up" && i > 0) [items[i - 1], items[i]] = [items[i], items[i - 1]];
      else if (b.dataset.act === "down" && i < items.length - 1) [items[i + 1], items[i]] = [items[i], items[i + 1]];
      else if (b.dataset.act === "del") items.splice(i, 1);
      render();
    });
    render();
    return render;
  }
  async function infoFor(paths) {
    try { return await invoke("create_source_info", { files: paths }); } catch (_) {
      return paths.map((p) => ({ path: p, kind: "image", pages: null }));
    }
  }

  // ---------- Tạo từ ảnh (cũng dùng cho máy quét / clipboard / OCR ảnh) ----------
  // mode: "images" | "scan" | "clipboard" | "ocr"
  async function openImagesDialog(paths, mode) {
    const tl = await getTools();
    const items = await infoFor(paths);
    const temps = new Set(mode === "scan" || mode === "clipboard" ? paths : []);
    const ocrOn = mode === "ocr";
    const title = { images: "create.titleImages", scan: "create.titleScan", clipboard: "create.titleClipboard", ocr: "create.titleOcrImage" }[mode];
    const box = modal(title, `
      <div class="cr-list" id="crList"></div>
      <div class="cr-listbar">
        <button id="crAdd"><i data-icon="image-add"></i><span>${escapeHtml(t(mode === "scan" ? "create.addImagesToo" : "create.addImages"))}</span></button>
        ${mode === "scan" ? `<button id="crScanMore"><i data-icon="scanner"></i><span>${escapeHtml(t("create.scanMore"))}</span></button>` : ""}
      </div>
      <div class="row">
        <div><label>${escapeHtml(t("create.pageSize"))}</label><select id="crSize">${sizeOptions("fit", { fit: true })}</select></div>
        <div><label>${escapeHtml(t("create.orientation"))}</label><select id="crOrient">
          ${opt("auto", "create.orient.auto", "auto")}${opt("portrait", "create.orient.portrait")}${opt("landscape", "create.orient.landscape")}</select></div>
        <div><label>${escapeHtml(t("create.marginMm"))}</label><input type="number" id="crMargin" min="0" max="50" step="1" value="0"></div>
      </div>
      <label><input type="checkbox" id="crJpeg" checked>${escapeHtml(t("create.jpegPassthrough"))}</label>
      <label><input type="checkbox" id="crOcr"${ocrOn ? " checked" : ""}${tl.tesseract ? "" : " disabled"}>${escapeHtml(t(tl.tesseract ? "create.ocrAfter" : "create.ocrUnavailable"))}</label>
      <div class="row cr-ocr-opts">
        <div><label>${escapeHtml(t("ribbon.convert.ocrLang"))}</label><select id="crLang">${langOptions($("cvLang").value)}</select></div>
        <div><label class="cr-inline"><input type="checkbox" id="crPre" checked>${escapeHtml(t("create.preprocess"))}</label></div>
      </div>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("create.createBtn"))}</button></div>`);
    box.classList.add("cr-modal");
    const okBtn = box.querySelector("#crOk");
    const rerender = wireList(box, items, false, () => { okBtn.disabled = items.length === 0; });
    const sync = () => {
      const fixed = box.querySelector("#crSize").value !== "fit";
      box.querySelector("#crOrient").disabled = !fixed;
      box.querySelector(".cr-ocr-opts").classList.toggle("hidden", !box.querySelector("#crOcr").checked);
    };
    box.querySelector("#crSize").addEventListener("change", sync);
    box.querySelector("#crOcr").addEventListener("change", sync);
    sync();
    const cleanup = () => { if (temps.size) invoke("create_cleanup", { paths: [...temps] }).catch(() => {}); };
    box.querySelector("#crAdd").addEventListener("click", async () => {
      const more = await invoke("pick_images", { title: t("create.imagesFilter") });
      if (more && more.length) { items.push(...(await infoFor(more))); rerender(); }
    });
    const scanMore = box.querySelector("#crScanMore");
    if (scanMore) scanMore.addEventListener("click", async () => {
      const p = await scanOnce(box.querySelector("#crErr"));
      if (p) { temps.add(p); items.push(...(await infoFor([p]))); rerender(); }
    });
    box.querySelector("#crCancel").addEventListener("click", () => { cleanup(); closeModal(); });
    okBtn.addEventListener("click", async () => {
      if (!items.length) return;
      const setup = {
        size: box.querySelector("#crSize").value,
        orientation: box.querySelector("#crOrient").value,
        marginMm: Math.max(0, Number(box.querySelector("#crMargin").value) || 0),
        jpegPassthrough: box.querySelector("#crJpeg").checked,
      };
      const ocr = box.querySelector("#crOcr").checked
        ? { lang: box.querySelector("#crLang").value, preprocess: box.querySelector("#crPre").checked }
        : null;
      const images = items.map((it) => it.path);
      closeModal();
      const out = await pickSavePdf(mode === "scan" ? "scan" : baseName(images[0]));
      if (!out) { cleanup(); return; }
      try {
        const res = await run(t(ocr ? "create.runningOcr" : "create.running"), () =>
          invoke("create_from_images", { images, setup, output: out, ocr }));
        if (!res) return;
        cleanup();
        const msg = res.words != null
          ? t("create.doneOcr", { n: res.pages, w: res.words, file: shortName(out) })
          : t("create.done", { n: res.pages, file: shortName(out) });
        await openResult(out, msg);
      } catch (e) {
        cleanup();
        say(t("create.error", { e: errText(e) }));
      }
    });
  }

  async function fromImages() {
    const files = await invoke("pick_images", { title: t("create.imagesFilter") });
    if (files && files.length) openImagesDialog(files, "images");
  }

  // ---------- Máy quét (WIA) ----------
  // Quét 1 trang → đường dẫn ảnh tạm; null = huỷ/không có máy quét (đã báo).
  // errEl: đang ở trong hộp thoại → báo lỗi tại chỗ (không thay hộp thoại).
  async function scanOnce(errEl) {
    let p = null;
    if (errEl) errEl.textContent = "";
    try {
      p = await run(t("create.scanWaiting"), () => invoke("create_scan_page"));
    } catch (e) {
      const s = errText(e);
      if (errEl) {
        errEl.textContent = s === "NO_SCANNER" ? t("create.noScanner") : t("create.scanError", { e: s });
      } else if (s === "NO_SCANNER") {
        await confirmModal(t("create.noScannerTitle"), t("create.noScanner"), t("common.ok"));
      } else {
        say(t("create.scanError", { e: s }));
      }
      return null;
    }
    if (!p) { say(t("create.scanCancelled")); return null; }
    clearHint();
    return p;
  }
  async function fromScanner() {
    const p = await scanOnce();
    if (p) openImagesDialog([p], "scan");
  }

  async function fromClipboard() {
    try {
      const p = await run(t("common.loading"), () => invoke("create_clipboard_image"));
      if (p) { clearHint(); openImagesDialog([p], "clipboard"); }
    } catch (e) {
      const s = errText(e);
      say(s === "NO_CLIPBOARD_IMAGE" ? t("create.noClipboardImage") : t("create.error", { e: s }));
    }
  }

  // ---------- Trang trắng ----------
  function fromBlank() {
    const box = modal("create.titleBlank", `
      <div class="row">
        <div><label>${escapeHtml(t("create.pageSize"))}</label><select id="crSize">${sizeOptions("a4", { custom: true })}</select></div>
        <div><label>${escapeHtml(t("create.pageCount"))}</label><input type="number" id="crCount" min="1" max="2000" value="1"></div>
      </div>
      <div class="row cr-custom hidden">
        <div><label>${escapeHtml(t("create.widthMm"))}</label><input type="number" id="crW" min="20" max="5000" value="210"></div>
        <div><label>${escapeHtml(t("create.heightMm"))}</label><input type="number" id="crH" min="20" max="5000" value="297"></div>
      </div>
      <label>${escapeHtml(t("create.orientation"))}</label>
      <div class="radiorow" id="crOrientRow">
        <label><input type="radio" name="crOr" value="portrait" checked>${escapeHtml(t("create.orient.portrait"))}</label>
        <label><input type="radio" name="crOr" value="landscape">${escapeHtml(t("create.orient.landscape"))}</label>
      </div>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("create.createBtn"))}</button></div>`);
    box.classList.add("cr-modal");
    const size = box.querySelector("#crSize");
    size.addEventListener("change", () => {
      const custom = size.value === "custom";
      box.querySelector(".cr-custom").classList.toggle("hidden", !custom);
      box.querySelector("#crOrientRow").classList.toggle("cr-dim", custom);
    });
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    box.querySelector("#crOk").addEventListener("click", async () => {
      const count = Math.round(Number(box.querySelector("#crCount").value));
      if (!(count >= 1 && count <= 2000)) { box.querySelector("#crErr").textContent = t("create.countInvalid"); return; }
      const args = {
        size: size.value,
        orientation: size.value === "custom" ? "keep" : box.querySelector("input[name=crOr]:checked").value,
        widthMm: Number(box.querySelector("#crW").value),
        heightMm: Number(box.querySelector("#crH").value),
        count,
      };
      closeModal();
      const out = await pickSavePdf(t("create.blankName"));
      if (!out) return;
      try {
        const n = await run(t("create.running"), () => invoke("create_blank", { ...args, output: out }));
        if (n) await openResult(out, t("create.done", { n, file: shortName(out) }));
      } catch (e) {
        say(t("create.error", { e: errText(e) }));
      }
    });
  }

  // ---------- Tệp văn bản ----------
  async function fromText() {
    let src = await invoke("pick_text_file", { title: t("create.textFilter") });
    if (!src) return;
    const box = modal("create.titleText", `
      <div class="cr-file"><span class="cr-name" id="crSrcName"></span><button id="crBrowse">${escapeHtml(t("common.browse"))}</button></div>
      <div class="row">
        <div><label>${escapeHtml(t("create.pageSize"))}</label><select id="crSize">${sizeOptions("a4")}</select></div>
        <div><label>${escapeHtml(t("create.orientation"))}</label><select id="crOrient">${opt("portrait", "create.orient.portrait", "portrait")}${opt("landscape", "create.orient.landscape")}</select></div>
      </div>
      <div class="row">
        <div><label>${escapeHtml(t("create.fontSize"))}</label><input type="number" id="crFont" min="6" max="36" step="0.5" value="11"></div>
        <div><label>${escapeHtml(t("create.marginMm"))}</label><input type="number" id="crMargin" min="0" max="60" value="20"></div>
      </div>
      <p class="muted">${escapeHtml(t("create.textNote"))}</p>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("create.createBtn"))}</button></div>`);
    box.classList.add("cr-modal");
    const showName = () => { box.querySelector("#crSrcName").textContent = shortName(src); box.querySelector("#crSrcName").title = src; };
    showName();
    box.querySelector("#crBrowse").addEventListener("click", async () => {
      const p = await invoke("pick_text_file", { title: t("create.textFilter") });
      if (p) { src = p; showName(); }
    });
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    box.querySelector("#crOk").addEventListener("click", async () => {
      const setup = {
        size: box.querySelector("#crSize").value,
        orientation: box.querySelector("#crOrient").value,
        fontSize: Math.min(36, Math.max(6, Number(box.querySelector("#crFont").value) || 11)),
        marginMm: Math.max(0, Number(box.querySelector("#crMargin").value) || 0),
      };
      closeModal();
      const out = await pickSavePdf(baseName(src));
      if (!out) return;
      try {
        const n = await run(t("create.running"), () => invoke("create_from_text", { input: src, setup, output: out }));
        if (n) await openResult(out, t("create.done", { n, file: shortName(out) }));
      } catch (e) {
        say(t("create.error", { e: errText(e) }));
      }
    });
  }

  // ---------- Trang web / HTML ----------
  async function fromWeb() {
    const tl = await getTools();
    const box = modal("create.titleWeb", `
      <label>${escapeHtml(t("create.webUrl"))}</label>
      <div class="cr-file"><input type="text" id="crUrl" placeholder="https://"><button id="crBrowse">${escapeHtml(t("create.webBrowse"))}</button></div>
      <div class="row">
        <div><label>${escapeHtml(t("create.webWait"))}</label><input type="number" id="crWait" min="0" max="30" value="3"></div>
        <div></div>
      </div>
      <p class="muted">${escapeHtml(t(tl.browser ? "create.webNote" : "create.noBrowser"))}</p>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary"${tl.browser ? "" : " disabled"}>${escapeHtml(t("create.createBtn"))}</button></div>`);
    box.classList.add("cr-modal");
    const url = box.querySelector("#crUrl");
    box.querySelector("#crBrowse").addEventListener("click", async () => {
      const p = await invoke("pick_html_file", { title: t("create.webBrowse") });
      if (p) url.value = p;
    });
    url.addEventListener("keydown", (e) => { if (e.key === "Enter") box.querySelector("#crOk").click(); });
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    box.querySelector("#crOk").addEventListener("click", async () => {
      const source = url.value.trim();
      if (!source) { box.querySelector("#crErr").textContent = t("create.webEmpty"); return; }
      const waitMs = Math.min(30, Math.max(0, Number(box.querySelector("#crWait").value) || 0)) * 1000;
      closeModal();
      let name = source;
      try { name = /^https?:/i.test(source) ? new URL(source).hostname : baseName(source); } catch (_) { name = "web"; }
      const out = await pickSavePdf(name);
      if (!out) return;
      try {
        const n = await run(t("create.webRunning"), () => invoke("create_from_web", { source, output: out, waitMs }));
        if (n) await openResult(out, t("create.done", { n, file: shortName(out) }));
      } catch (e) {
        say(t("create.error", { e: errText(e) }));
      }
    });
  }

  // ---------- Office ----------
  async function fromOffice() {
    const tl = await getTools();
    if (!tl.soffice) { await confirmModal(t("create.titleOffice"), t("conv.officeMissingTip"), t("common.ok")); return; }
    const src = await invoke("pick_office_file");
    if (!src) return;
    const out = await pickSavePdf(baseName(src));
    if (!out) return;
    try {
      const n = await run(t("conv.officeRunning"), () => invoke("create_from_office", { input: src, output: out }));
      if (n) await openResult(out, t("create.done", { n, file: shortName(out) }));
    } catch (e) {
      say(t("conv.errOffice", { e: errText(e) }));
    }
  }

  // ---------- Gộp nhiều tệp ----------
  async function fromCombine() {
    const tl = await getTools();
    const items = [];
    const box = modal("create.titleCombine", `
      <p class="muted">${escapeHtml(t(tl.soffice ? "create.combineNote" : "create.combineNoteNoOffice"))}</p>
      <div class="cr-list" id="crList"></div>
      <div class="cr-listbar"><button id="crAdd"><i data-icon="page-insert"></i><span>${escapeHtml(t("create.addFiles"))}</span></button></div>
      <div class="row">
        <div><label>${escapeHtml(t("create.imagePageSize"))}</label><select id="crSize">${sizeOptions("fit", { fit: true })}</select></div>
        <div></div>
      </div>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("create.combineBtn"))}</button></div>`);
    box.classList.add("cr-modal");
    const okBtn = box.querySelector("#crOk");
    const refresh = () => {
      const bad = items.filter((it) => !it.kind || (it.kind === "office" && !tl.soffice) || (it.kind === "html" && !tl.browser));
      okBtn.disabled = items.length === 0 || bad.length > 0;
      box.querySelector("#crErr").textContent = bad.length ? t("create.combineBad", { list: bad.map((b) => shortName(b.path)).join(", ") }) : "";
    };
    const render = wireList(box, items, true, refresh);
    const add = async () => {
      const files = await invoke("pick_combine_files", { title: t("create.combineFilter") });
      if (files && files.length) { items.push(...(await infoFor(files))); render(); }
    };
    box.querySelector("#crAdd").addEventListener("click", add);
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    okBtn.addEventListener("click", async () => {
      const files = items.map((it) => it.path);
      const setup = { size: box.querySelector("#crSize").value, orientation: "auto", marginMm: 0, jpegPassthrough: true };
      closeModal();
      const out = await pickSavePdf(t("create.combineName"));
      if (!out) return;
      try {
        const n = await run(t("create.combineRunning", { n: files.length }), () =>
          invoke("create_combine", { files, setup, output: out }));
        if (n) await openResult(out, t("create.combineDone", { n, k: files.length, file: shortName(out) }));
      } catch (e) {
        say(t("create.error", { e: errText(e) }));
      }
    });
    add();
  }

  // ---------- OCR ----------
  async function openOcrDialog() {
    if (!hasDoc()) return;
    const box = modal("create.titleOcr", `
      <div class="row">
        <div><label>${escapeHtml(t("ribbon.convert.ocrLang"))}</label><select id="crLang">${langOptions($("cvLang").value)}</select></div>
        <div></div>
      </div>
      ${rangeHtml()}
      <label><input type="checkbox" id="crSkip" checked>${escapeHtml(t("create.ocrSkipText"))}</label>
      <label><input type="checkbox" id="crPre" checked>${escapeHtml(t("create.preprocess"))}</label>
      <p class="muted">${escapeHtml(t("create.ocrNote"))}</p>
      <div class="err" id="crErr"></div>
      <div class="foot"><button id="crCancel">${escapeHtml(t("common.cancel"))}</button><button id="crOk" class="primary">${escapeHtml(t("ribbon.convert.ocr"))}</button></div>`);
    box.classList.add("cr-modal");
    wireRange(box);
    box.querySelector("#crCancel").addEventListener("click", closeModal);
    box.querySelector("#crOk").addEventListener("click", async () => {
      const r = readRange(box);
      if (r.error) { box.querySelector("#crErr").textContent = r.error; return; }
      const opts = {
        lang: box.querySelector("#crLang").value,
        pages: r.pages,
        skipText: box.querySelector("#crSkip").checked,
        preprocess: box.querySelector("#crPre").checked,
      };
      $("cvLang").value = opts.lang;
      closeModal();
      const out = await pickSavePdf(baseName(state.path) + "_ocr");
      if (!out) return;
      try {
        const rep = await run(t("conv.ocrRunning"), () => invoke("ocr_run_ex", { input: state.path, opts, output: out }));
        if (!rep) return;
        await openResult(out, t("create.ocrDone", { w: rep.words, n: rep.pagesOcred, s: rep.pagesSkipped, file: shortName(out) }));
      } catch (e) {
        say(t("conv.errOcr", { e: errText(e) }));
      }
    });
  }

  async function ocrImage() {
    const files = await invoke("pick_images", { title: t("create.imagesFilter") });
    if (files && files.length) openImagesDialog(files, "ocr");
  }

  // cvOcrImage bật/tắt theo cvOcr (main.js tắt cvOcr khi chưa cài Tesseract).
  new MutationObserver(() => {
    $("cvOcrImage").disabled = $("cvOcr").disabled;
    $("cvOcrImage").title = $("cvOcr").disabled ? $("cvOcr").title : t("create.ocrImageTip");
  }).observe($("cvOcr"), { attributes: true, attributeFilter: ["disabled"] });

  // ---------- Gắn nút ----------
  const ACTIONS = {
    cvNewImages: fromImages,
    cvOffice: fromOffice,
    cvNewText: fromText,
    cvNewWeb: fromWeb,
    cvNewScan: fromScanner,
    cvScanBtn: fromScanner,
    cvNewClipboard: fromClipboard,
    cvNewBlank: fromBlank,
    cvNewCombine: fromCombine,
    cvCombineBtn: fromCombine,
    cvOcr: openOcrDialog,
    cvOcrImage: ocrImage,
  };
  for (const [id, fn] of Object.entries(ACTIONS)) $(id).addEventListener("click", fn);

  // Máy quét / clipboard chỉ có trên Windows.
  getTools().then((tl) => {
    if (!tl.scanner) { $("cvScanBtn").disabled = true; $("cvNewScan").disabled = true; }
    if (!tl.clipboard) $("cvNewClipboard").disabled = true;
  });
})();
