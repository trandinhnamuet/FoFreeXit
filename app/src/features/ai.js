// AI Assistant (Claude API) + Đọc to (Web Speech API).
// Backend: app/src-tauri/src/cmd_ai.rs — key chỉ nằm ở Rust; ở đây chỉ biết
// "đã cấu hình" + dạng che. Mọi chuỗi hiển thị qua t("ai.*"); chuỗi tiếng Anh
// trong tệp này là lời nhắc gửi model (không hiển thị cho người dùng).
(function () {
  const TEV = window.__TAURI__ && window.__TAURI__.event;
  const MAX_TOOL_STEPS = 8;
  const MODEL_LABEL = {
    "claude-opus-5-5": "Claude Opus 5.5",
    "claude-sonnet-5-5": "Claude Sonnet 5.5",
    "claude-haiku-4-5": "Claude Haiku 4.5",
  };
  const PII_CATS = ["person_name", "address", "email", "phone", "id_number", "bank_account", "date_of_birth"];
  const TRANSLATE_LANGS = ["vi", "en", "zh", "ja", "ko", "fr", "de", "es", "ru", "th"];
  const LANG_NAME_EN = {
    vi: "Vietnamese", en: "English", zh: "Simplified Chinese", ja: "Japanese", ko: "Korean",
    fr: "French", de: "German", es: "Spanish", ru: "Russian", th: "Thai",
  };

  const AI = {
    settings: null,       // {configured, maskedKey, model, language, models}
    messages: [],         // hội thoại dạng Messages API (append-only)
    docLabel: null,       // tên tài liệu mà hội thoại đang nói về
    attachments: [],      // [{path, label}] tài liệu đính kèm để phân tích cùng
    currentId: null,      // requestId đang chạy (để Dừng)
    busy: false,
    lastSel: null,        // {text, page} vùng chữ chọn gần nhất trên trang
    keepThread: false,    // tool của AI mở tệp kết quả → không reset hội thoại
  };
  const pending = new Map(); // requestId → {onDelta, onStatus, resolve, reject}

  // ---------------------------------------------------------------- tiện ích
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const newId = () => "ai" + Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
  const panel = () => $("aiPanel");
  const docName = () => AI.docLabel || (state.path ? shortName(state.path) : "");

  function parseErr(e) {
    if (e && typeof e === "object" && e.kind) return e;
    const s = String(e);
    try { const j = JSON.parse(s); if (j && j.kind) return j; } catch (_) {}
    return { kind: "other", message: s };
  }
  function errorText(err) {
    const e = parseErr(err);
    switch (e.kind) {
      case "not_configured": return t("ai.err.notConfigured");
      case "auth": return t("ai.err.auth");
      case "permission": return t("ai.err.permission");
      case "not_found": return t("ai.err.notFound");
      case "rate_limit": return e.retryAfter ? t("ai.err.rateLimitWait", { s: e.retryAfter }) : t("ai.err.rateLimit");
      case "overloaded": return t("ai.err.overloaded");
      case "server": return t("ai.err.server");
      case "too_large": return t("ai.err.tooLarge");
      case "network": return t("ai.err.network", { e: e.message || "" });
      case "invalid_request": return t("ai.err.invalid", { e: e.message || "" });
      case "invalid_key_format": return t("ai.err.keyFormat");
      case "cancelled": return t("ai.stopped");
      default: return t("ai.err.generic", { e: e.message || String(err) });
    }
  }
  function answerLangName() {
    const l = AI.settings && AI.settings.language;
    return LANG_NAME_EN[l === "vi" || l === "en" ? l : I18N.lang] || "Vietnamese";
  }

  // ---------------------------------------------------------------- sự kiện backend
  if (TEV) {
    TEV.listen("ai://delta", (e) => { const p = pending.get(e.payload.requestId); if (p && p.onDelta) p.onDelta(e.payload.text); });
    TEV.listen("ai://status", (e) => { const p = pending.get(e.payload.requestId); if (p && p.onStatus) p.onStatus(e.payload); });
    TEV.listen("ai://done", (e) => { const p = pending.get(e.payload.requestId); if (p) p.resolve(e.payload); });
    TEV.listen("ai://error", (e) => { const p = pending.get(e.payload.requestId); if (p) p.reject(e.payload); });
  }

  /// Một request stream tới Claude. Promise → payload `ai://done`.
  function aiRequest(body, handlers) {
    const requestId = newId();
    AI.currentId = requestId;
    return new Promise((resolve, reject) => {
      pending.set(requestId, Object.assign({}, handlers || {}, { resolve, reject }));
      invoke("ai_chat", { req: Object.assign({ requestId, uiLang: I18N.lang }, body) })
        .catch((e) => reject(parseErr(e)));
    }).finally(() => {
      pending.delete(requestId);
      if (AI.currentId === requestId) AI.currentId = null;
    });
  }

  function docsForRequest() {
    const docs = [];
    if (state.path) docs.push({ path: state.path, label: docName() });
    for (const a of AI.attachments) docs.push({ path: a.path, label: a.label });
    return docs;
  }

  // ---------------------------------------------------------------- Markdown an toàn
  // Escape HTML TRƯỚC, rồi mới dựng thẻ của riêng mình → không thể chèn HTML/JS.
  const CITE_RE = /\[(Trang|trang|Tr\.|tr\.|p\.|pp\.|[Pp]age|[Pp]ages)\s*(\d+(?:\s*[-\u2013]\s*\d+)?(?:\s*(?:,|;|v\u00e0|and)\s*\d+(?:\s*[-\u2013]\s*\d+)?)*)(?:\s*[,;]\s*([^\]\n]{1,80}))?\]/g;
  function citeHtml(m, word, nums, file) {
    const docAttr = file ? ` data-doc="${esc(file.trim())}"` : "";
    const links = nums.replace(/\d+(?:\s*[-–]\s*\d+)?/g, (r) => {
      const first = r.match(/\d+/)[0];
      return `<a class="ai-cite" href="#" data-page="${first}"${docAttr}>${r}</a>`;
    });
    return `<span class="ai-cite-grp">[${word} ${links}${file ? ", " + file : ""}]</span>`;
  }
  function mdInline(s) {
    return s.split(/(`[^`\n]+`)/g).map((part) => {
      if (/^`[^`\n]+`$/.test(part)) return "<code>" + part.slice(1, -1) + "</code>";
      return part
        .replace(/\*\*(?=\S)([^*]+?)\*\*/g, "<strong>$1</strong>")
        .replace(/__(?=\S)([^_]+?)__/g, "<strong>$1</strong>")
        .replace(/(^|[^*\w])\*(?=\S)([^*\n]+?)\*(?!\*)/g, "$1<em>$2</em>")
        .replace(/(^|[\s(])_(?=\S)([^_\n]+?)_(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>")
        .replace(/~~(?=\S)([^~]+?)~~/g, "<del>$1</del>")
        .replace(CITE_RE, citeHtml);
    }).join("");
  }
  function splitRow(line) {
    let l = line.trim();
    if (l.startsWith("|")) l = l.slice(1);
    if (l.endsWith("|")) l = l.slice(0, -1);
    return l.split("|").map((c) => c.trim());
  }
  function renderMarkdown(src) {
    const lines = escapeHtml(String(src || "")).replace(/\r\n?/g, "\n").split("\n");
    const out = [];
    let para = [];
    const flushPara = () => { if (para.length) { out.push("<p>" + para.map(mdInline).join("<br>") + "</p>"); para = []; } };
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      const fence = line.match(/^\s*```/);
      if (fence) {
        flushPara();
        const code = [];
        i++;
        while (i < lines.length && !/^\s*```/.test(lines[i])) code.push(lines[i++]);
        i++;
        out.push("<pre><code>" + code.join("\n") + "</code></pre>");
        continue;
      }
      if (!line.trim()) { flushPara(); i++; continue; }
      const h = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
      if (h) {
        flushPara();
        const lvl = Math.min(5, h[1].length + 2);
        out.push(`<h${lvl}>${mdInline(h[2])}</h${lvl}>`);
        i++; continue;
      }
      if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) { flushPara(); out.push("<hr>"); i++; continue; }
      if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(lines[i + 1])) {
        flushPara();
        const head = splitRow(line);
        i += 2;
        const rows = [];
        while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(splitRow(lines[i++]));
        out.push('<div class="ai-tablewrap"><table><thead><tr>' + head.map((c) => `<th>${mdInline(c)}</th>`).join("") +
          "</tr></thead><tbody>" + rows.map((r) => "<tr>" + head.map((_, k) => `<td>${mdInline(r[k] || "")}</td>`).join("") + "</tr>").join("") +
          "</tbody></table></div>");
        continue;
      }
      if (/^\s*&gt;\s?/.test(line)) {
        flushPara();
        const q = [];
        while (i < lines.length && /^\s*&gt;\s?/.test(lines[i])) q.push(lines[i++].replace(/^\s*&gt;\s?/, ""));
        out.push("<blockquote>" + q.map(mdInline).join("<br>") + "</blockquote>");
        continue;
      }
      const li = line.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
      if (li) {
        flushPara();
        const ordered = /\d/.test(li[2]);
        const items = [];
        while (i < lines.length) {
          const m = lines[i].match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
          if (m) { items.push({ nest: m[1].length >= 2, text: m[3] }); i++; continue; }
          // Dòng tiếp nối (thụt lề) của mục trước.
          if (items.length && /^\s{2,}\S/.test(lines[i])) { items[items.length - 1].text += "<br>" + lines[i].trim(); i++; continue; }
          break;
        }
        const tag = ordered ? "ol" : "ul";
        out.push(`<${tag}>` + items.map((it) => `<li${it.nest ? ' class="nest"' : ""}>${mdInline(it.text)}</li>`).join("") + `</${tag}>`);
        continue;
      }
      para.push(line.trim());
      i++;
    }
    flushPara();
    return out.join("");
  }

  // ---------------------------------------------------------------- khung chat
  function scrollToEnd(force) {
    const sc = $("aiScroll");
    if (force || sc.scrollHeight - sc.scrollTop - sc.clientHeight < 120) sc.scrollTop = sc.scrollHeight;
  }
  function refreshWelcome() {
    $("aiWelcome").hidden = $("aiThread").children.length > 0;
  }
  function addUserBubble(text) {
    const el = document.createElement("div");
    el.className = "ai-msg user";
    el.innerHTML = `<div class="ai-bubble">${esc(text).replace(/\n/g, "<br>")}</div>`;
    $("aiThread").appendChild(el);
    refreshWelcome();
    scrollToEnd(true);
    return el;
  }
  function addNote(text) {
    const el = document.createElement("div");
    el.className = "ai-note";
    el.textContent = text;
    $("aiThread").appendChild(el);
    refreshWelcome();
    scrollToEnd(true);
  }

  /// Bong bóng trả lời: nhận chữ stream, hiện trạng thái, dòng hành động, nút.
  function addAssistantBubble() {
    const el = document.createElement("div");
    el.className = "ai-msg assistant";
    el.innerHTML = `<div class="ai-avatar"><i data-icon="sparkle"></i></div>
      <div class="ai-bubble"><div class="ai-md"></div><div class="ai-status"><span class="ai-dots"><i></i><i></i><i></i></span><span class="ai-status-t"></span></div><div class="ai-extra"></div><div class="ai-actions"></div></div>`;
    applyIcons(el);
    $("aiThread").appendChild(el);
    refreshWelcome();
    scrollToEnd(true);
    const md = el.querySelector(".ai-md");
    const statusEl = el.querySelector(".ai-status");
    const statusT = el.querySelector(".ai-status-t");
    let raw = "";
    let segStart = 0;
    let raf = 0;
    const render = () => { raf = 0; md.innerHTML = renderMarkdown(raw); scrollToEnd(false); };
    const b = {
      el,
      get text() { return raw; },
      /// Chữ của đoạn trả lời cuối cùng (sau lượt tool gần nhất).
      get lastSegment() { return raw.slice(segStart).trim(); },
      append(s) { raw += s; if (!raf) raf = requestAnimationFrame(render); },
      setText(s) { raw = s; render(); },
      newSegment() { if (raw && !raw.endsWith("\n\n")) raw += "\n\n"; segStart = raw.length; },
      status(s) { statusEl.hidden = !s && s !== ""; statusT.textContent = s || ""; },
      done() { statusEl.hidden = true; if (raf) { cancelAnimationFrame(raf); render(); } },
      extra(html) {
        const x = document.createElement("div");
        x.innerHTML = html;
        applyIcons(x);
        el.querySelector(".ai-extra").appendChild(x);
        scrollToEnd(false);
        return x;
      },
      error(msg) { b.done(); b.extra(`<div class="ai-err">${esc(msg)}</div>`); },
      action(icon, label, fn) {
        const btn = document.createElement("button");
        btn.className = "ai-act";
        btn.innerHTML = `<i data-icon="${icon}"></i><span>${esc(label)}</span>`;
        applyIcons(btn);
        btn.addEventListener("click", fn);
        el.querySelector(".ai-actions").appendChild(btn);
        return btn;
      },
    };
    b.status(t("ai.status.thinking"));
    return b;
  }

  function statusLabel(p) {
    switch (p.phase) {
      case "reading": return t("ai.status.reading");
      case "condensing": return t("ai.status.condensing", { done: Math.min(p.done + 1, p.total), total: p.total });
      case "scanning": return t("ai.status.scanning", { done: Math.min(p.done + 1, p.total), total: p.total });
      case "tool": return t("ai.status.tool", { name: toolLabel(p.name) });
      default: return t("ai.status.thinking");
    }
  }

  async function copyText(text, btn) {
    try {
      await navigator.clipboard.writeText(text);
    } catch (_) {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    if (btn) {
      const s = btn.querySelector("span");
      const old = s.textContent;
      s.textContent = t("ai.copied");
      setTimeout(() => { s.textContent = old; }, 1400);
    }
  }

  function addStandardActions(b, opts) {
    b.action("copy", t("ai.copy"), (e) => copyText(opts && opts.copyLast ? b.lastSegment : b.text, e.currentTarget));
    if (opts && opts.exportable) {
      b.action("file-text", t("ai.exportTxt"), () => exportResult(b.text, "txt"));
      b.action("file-word", t("ai.exportDocx"), () => exportResult(b.text, "docx"));
    }
  }

  async function exportResult(text, ext) {
    const base = (docName() || "ai").replace(/\.pdf$/i, "");
    const out = await invoke("pick_save_as", { ext, name: `${base}-ai.${ext}` });
    if (!out) return;
    try {
      await invoke("ai_export_text", { output: out, text });
      $("status").textContent = t("ai.exported", { file: shortName(out) });
    } catch (e) {
      $("status").textContent = t("ai.err.generic", { e });
    }
  }

  function setBusy(on) {
    AI.busy = on;
    $("aiSend").hidden = on;
    $("aiStop").hidden = !on;
    document.querySelectorAll("#aiChips .ai-chip, #aiWelcome .ai-chip").forEach((c) => { c.disabled = on; });
  }

  // ---------------------------------------------------------------- cài đặt
  async function loadSettings() {
    try { AI.settings = await invoke("ai_get_settings"); } catch (_) { AI.settings = { configured: false, models: Object.keys(MODEL_LABEL), model: "claude-opus-5-5", language: "auto" }; }
    refreshConfigured();
    return AI.settings;
  }
  function refreshConfigured() {
    const ok = !!(AI.settings && AI.settings.configured);
    $("aiOnboard").hidden = ok;
    $("aiInput").disabled = !ok;
    $("aiSend").disabled = !ok;
    document.querySelectorAll("#aiChips .ai-chip").forEach((c) => { c.classList.toggle("off", !ok); });
    $("aiModelBadge").textContent = ok ? (MODEL_LABEL[AI.settings.model] || AI.settings.model) : "";
  }
  function ensureReady() {
    if (AI.settings && AI.settings.configured) return true;
    openPanel(true);
    openSettings();
    return false;
  }

  async function openSettings() {
    const s = AI.settings || (await loadSettings());
    const models = (s.models && s.models.length ? s.models : Object.keys(MODEL_LABEL));
    const box = openModal(t("ai.set.title"), `
      <div class="ai-settings"></div>
      <label>${t("ai.set.key")}</label>
      <div class="ai-keyrow">
        <input type="password" id="aiKeyInput" autocomplete="off" spellcheck="false" placeholder="${esc(s.configured ? s.maskedKey : "sk-ant-…")}">
        <button type="button" id="aiKeyShow">${t("ai.set.show")}</button>
      </div>
      <p class="muted">${s.configured ? t("ai.set.keySaved", { key: esc(s.maskedKey) }) : t("ai.set.keyHelp")}</p>
      <div class="row">
        <div><label>${t("ai.set.model")}</label><select id="aiModelSel">${models.map((m) => `<option value="${esc(m)}"${m === s.model ? " selected" : ""}>${esc(MODEL_LABEL[m] || m)}</option>`).join("")}</select></div>
        <div><label>${t("ai.set.lang")}</label><select id="aiLangSel">
          <option value="auto"${s.language === "auto" ? " selected" : ""}>${t("ai.set.langAuto")}</option>
          <option value="vi"${s.language === "vi" ? " selected" : ""}>${t("ai.set.langVi")}</option>
          <option value="en"${s.language === "en" ? " selected" : ""}>${t("ai.set.langEn")}</option>
        </select></div>
      </div>
      <p class="muted">${t("ai.set.modelHelp")}</p>
      <div class="ai-privacy"><b><i data-icon="shield-check"></i>${t("ai.set.privacyTitle")}</b><p>${t("ai.set.privacy")}</p></div>
      <div class="status" id="aiTestMsg"></div>
      <div class="err" id="aiSetErr"></div>
      <div class="foot">
        <button type="button" id="aiTestBtn"><i data-icon="shield-check"></i>${t("ai.set.test")}</button>
        ${s.configured ? `<button type="button" id="aiRemoveKey">${t("ai.set.remove")}</button>` : ""}
        <button type="button" id="aiSetCancel">${t("common.cancel")}</button>
        <button type="button" id="aiSetSave" class="primary">${t("common.save")}</button>
      </div>`);
    const keyIn = box.querySelector("#aiKeyInput");
    const msg = box.querySelector("#aiTestMsg");
    const err = box.querySelector("#aiSetErr");
    box.querySelector("#aiKeyShow").addEventListener("click", () => {
      keyIn.type = keyIn.type === "password" ? "text" : "password";
      box.querySelector("#aiKeyShow").textContent = keyIn.type === "password" ? t("ai.set.show") : t("ai.set.hide");
    });
    box.querySelector("#aiTestBtn").addEventListener("click", async () => {
      err.textContent = "";
      msg.textContent = t("ai.set.testing");
      try {
        const name = await invoke("ai_test_connection", { apiKey: keyIn.value.trim() || null, model: box.querySelector("#aiModelSel").value });
        msg.textContent = t("ai.set.testOk", { model: name });
      } catch (e) {
        msg.textContent = "";
        err.textContent = errorText(e);
      }
    });
    const save = async (clearKey) => {
      err.textContent = "";
      try {
        AI.settings = await invoke("ai_save_settings", {
          apiKey: clearKey ? null : (keyIn.value.trim() || null),
          model: box.querySelector("#aiModelSel").value,
          language: box.querySelector("#aiLangSel").value,
          clearKey: !!clearKey,
        });
        keyIn.value = "";
        refreshConfigured();
        closeModal();
        $("status").textContent = clearKey ? t("ai.set.removed") : t("ai.set.saved");
      } catch (e) {
        err.textContent = errorText(e);
      }
    };
    box.querySelector("#aiSetCancel").addEventListener("click", closeModal);
    box.querySelector("#aiSetSave").addEventListener("click", () => save(false));
    const rm = box.querySelector("#aiRemoveKey");
    if (rm) rm.addEventListener("click", async () => {
      closeModal();
      if (await confirmModal(t("ai.set.removeTitle"), t("ai.set.removeMsg"), t("ai.set.remove"))) {
        try { AI.settings = await invoke("ai_save_settings", { apiKey: null, model: s.model, language: s.language, clearKey: true }); } catch (_) {}
        refreshConfigured();
        $("status").textContent = t("ai.set.removed");
      }
    });
    keyIn.addEventListener("keydown", (e) => { if (e.key === "Enter") save(false); });
  }

  // ---------------------------------------------------------------- panel
  function openPanel(on) {
    const p = panel();
    const show = on === undefined ? p.hidden : on;
    p.hidden = !show;
    $("aiQuickBtn").classList.toggle("active", show);
    $("aiRbPanel").classList.toggle("on", show);
    try { localStorage.setItem("ff.aiPanel", show ? "1" : "0"); } catch (_) {}
    if (show) {
      if (!AI.settings) loadSettings();
      renderDocChips();
      setTimeout(() => { if (!$("aiInput").disabled) $("aiInput").focus(); }, 0);
    }
    return show;
  }

  function renderDocChips() {
    const wrap = $("aiDocs");
    const chips = [];
    if (state.path) chips.push(`<span class="ai-doc cur" title="${esc(docName())}"><i data-icon="file-pdf"></i><span>${esc(docName())}</span></span>`);
    AI.attachments.forEach((a, i) => chips.push(
      `<span class="ai-doc" title="${esc(a.path)}"><i data-icon="file-pdf"></i><span>${esc(a.label)}</span><button class="ai-doc-x" data-i="${i}" title="${esc(t("ai.detach"))}"><i data-icon="win-close"></i></button></span>`));
    chips.push(`<button class="ai-doc add" id="aiAttach" title="${esc(t("ai.attachTip"))}"><i data-icon="paperclip"></i><span>${esc(t("ai.attach"))}</span></button>`);
    wrap.innerHTML = chips.join("");
    applyIcons(wrap);
    $("aiChipCompare").hidden = AI.attachments.length === 0;
  }

  async function attachDocs() {
    const files = await invoke("ai_pick_pdfs");
    if (!files || !files.length) return;
    for (const f of files) {
      if (AI.attachments.some((a) => a.path === f) || f === state.path) continue;
      AI.attachments.push({ path: f, label: shortName(f) });
    }
    renderDocChips();
    addNote(t("ai.attached", { n: AI.attachments.length }));
  }

  function resetThread(note) {
    if (AI.currentId) invoke("ai_cancel", { requestId: AI.currentId }).catch(() => {});
    AI.messages = [];
    $("aiThread").innerHTML = "";
    if (note) addNote(note);
    refreshWelcome();
  }

  // ---------------------------------------------------------------- hội thoại + tool loop
  /// Gửi một lượt: `prompt` (gửi model), `display` (hiện cho người dùng).
  /// opts: strategy "auto"|"summary"|"none", toolChoice "auto"|"none", query,
  ///       noDocs, exportable, copyLast.
  async function send(prompt, display, opts) {
    opts = opts || {};
    if (!AI.settings) await loadSettings();
    if (!ensureReady() || AI.busy) return null;
    openPanel(true);
    const snapshot = AI.messages.length;
    AI.messages.push({ role: "user", content: prompt });
    addUserBubble(display || prompt);
    const b = addAssistantBubble();
    setBusy(true);
    let ok = false;
    try {
      for (let step = 0; step < MAX_TOOL_STEPS; step++) {
        const res = await aiRequest({
          messages: AI.messages,
          docs: opts.noDocs ? [] : docsForRequest(),
          docStrategy: opts.noDocs ? "none" : (opts.strategy || "auto"),
          query: opts.query || "",
          toolChoice: opts.toolChoice || "auto",
        }, {
          onDelta: (s) => { b.status(null); b.append(s); },
          onStatus: (p) => b.status(statusLabel(p)),
        });
        if (step === 0) coverageNote(b, res.docs);
        if (res.stopReason === "refusal") { b.error(t("ai.refused")); break; }
        if (!res.content || !res.content.length) { b.error(t("ai.empty")); break; }
        if (res.stopReason === "max_tokens" && res.content.some((c) => c.type === "tool_use")) { b.error(t("ai.truncated")); break; }
        AI.messages.push({ role: "assistant", content: res.content });
        if (res.stopReason === "tool_use") {
          const uses = res.content.filter((c) => c.type === "tool_use");
          const results = [];
          for (const u of uses) results.push(await runTool(u, res.invalidToolInputs || {}, b));
          AI.messages.push({ role: "user", content: results });
          b.newSegment();
          b.status(t("ai.status.thinking"));
          if (step === MAX_TOOL_STEPS - 1) b.error(t("ai.tooManySteps"));
          continue;
        }
        if (res.stopReason === "max_tokens") b.extra(`<div class="ai-warn">${esc(t("ai.truncatedAnswer"))}</div>`);
        ok = true;
        break;
      }
    } catch (e) {
      const er = parseErr(e);
      if (er.kind === "cancelled") { b.done(); b.extra(`<div class="ai-warn">${esc(t("ai.stopped"))}</div>`); }
      else {
        b.error(errorText(er));
        if (er.kind === "auth" || er.kind === "not_configured") b.action("gear", t("ai.settings"), openSettings);
      }
    } finally {
      b.done();
      setBusy(false);
    }
    // Lượt dở (lỗi/huỷ/từ chối) → trả hội thoại về trạng thái nhất quán gần nhất.
    const last = AI.messages[AI.messages.length - 1];
    if (!ok && !(last && last.role === "assistant" && !last.content.some((c) => c.type === "tool_use"))) AI.messages.length = snapshot;
    if (b.text.trim()) addStandardActions(b, opts);
    return ok ? b : null;
  }

  function coverageNote(b, docs) {
    const parts = (docs || []).filter((d) => d.coverage && d.coverage !== "full").map((d) =>
      d.coverage === "condensed" ? t("ai.coverage.condensed", { name: d.label }) : t("ai.coverage.pages", { name: d.label, pages: d.covered }));
    if (parts.length) b.extra(`<div class="ai-cov"><i data-icon="info"></i><span>${esc(parts.join(" "))}</span></div>`);
  }

  // ---------------------------------------------------------------- tools (thao tác thật của app)
  function toolLabel(name) {
    const k = {
      go_to_page: "ai.tool.goToPage", search_text: "ai.tool.search", rotate_pages: "ai.tool.rotate",
      delete_pages: "ai.tool.delete", extract_pages: "ai.tool.extract", add_watermark: "ai.tool.watermark",
      add_header_footer: "ai.tool.headerFooter", export_document: "ai.tool.export", run_ocr: "ai.tool.ocr",
      mark_redactions: "ai.tool.redact",
    }[name];
    return k ? t(k) : name;
  }
  // Kiểm tra input theo schema (eager input streaming: server không kiểm hộ).
  const TOOL_SPEC = {
    go_to_page: { page: ["integer", true] },
    search_text: { query: ["string", true], match_case: ["boolean"] },
    rotate_pages: { pages: ["string", true], angle: ["integer", true] },
    delete_pages: { pages: ["string", true] },
    extract_pages: { pages: ["string", true] },
    add_watermark: { text: ["string", true], pages: ["string"], font_size: ["number"], rotation: ["number"], opacity: ["number"], color: ["string"] },
    add_header_footer: { header: ["string"], footer: ["string"], page_numbers: ["boolean"], pages: ["string"], font_size: ["number"] },
    export_document: { format: ["string", true] },
    run_ocr: { language: ["string"] },
    mark_redactions: { texts: ["array", true] },
  };
  function validateInput(name, input) {
    const spec = TOOL_SPEC[name];
    if (!spec) return `Unknown tool: ${name}`;
    if (!input || typeof input !== "object" || Array.isArray(input)) return "Input must be an object";
    for (const [k, [type, req]] of Object.entries(spec)) {
      const v = input[k];
      if (v === undefined || v === null) { if (req) return `Missing required field: ${k}`; continue; }
      const okType = type === "integer" ? Number.isInteger(v)
        : type === "number" ? typeof v === "number" && isFinite(v)
        : type === "array" ? Array.isArray(v) && v.every((x) => typeof x === "string")
        : typeof v === type;
      if (!okType) return `Field ${k} must be ${type}`;
    }
    for (const k of Object.keys(input)) if (!(k in spec)) return `Unknown field: ${k}`;
    return null;
  }

  async function runTool(use, invalid, b) {
    const result = (content, isError) => ({ type: "tool_result", tool_use_id: use.id, content: String(content), ...(isError ? { is_error: true } : {}) });
    const line = b.extra(`<div class="ai-toolline"><i data-icon="sparkle"></i><span>${esc(toolLabel(use.name))}</span><em>${esc(t("ai.toolRunning"))}</em></div>`);
    const setLine = (cls, txt) => { line.firstChild.classList.add(cls); line.querySelector("em").textContent = txt; };
    if (Object.prototype.hasOwnProperty.call(invalid, use.id)) {
      setLine("err", t("ai.toolInvalid"));
      return result(JSON.stringify({ INVALID_JSON: invalid[use.id] }), true);
    }
    const bad = validateInput(use.name, use.input);
    if (bad) { setLine("err", t("ai.toolInvalid")); return result(bad, true); }
    try {
      const out = await TOOLS[use.name](use.input);
      if (out && out.cancelled) { setLine("cancel", t("ai.toolCancelled")); return result("The user cancelled this action. Nothing was changed."); }
      setLine("ok", out.label || t("ai.toolDone"));
      return result(out.text);
    } catch (e) {
      const msg = e && e.userMsg ? e.userMsg : String(e);
      setLine("err", msg);
      return result("Error: " + msg, true);
    }
  }

  const fail = (userMsg) => { const e = new Error(userMsg); e.userMsg = userMsg; return e; };
  function needDoc() { if (!state.path || !state.pages.length) throw fail(t("x.noDoc")); }
  function pagesOf(spec) {
    const n = state.pages.length;
    const s = String(spec || "").trim().toLowerCase();
    if (!s || s === "all") return [...Array(n).keys()];
    if (s === "current") return [state.current];
    const idx = [...new Set(parsePageRange(s, n))].sort((a, b) => a - b);
    if (!idx.length) throw fail(t("ai.tool.badPages", { pages: spec, n }));
    return idx;
  }
  const ranges = (idx) => {
    const out = [];
    for (let i = 0; i < idx.length; i++) {
      let j = i;
      while (j + 1 < idx.length && idx[j + 1] === idx[j] + 1) j++;
      out.push(i === j ? `${idx[i] + 1}` : `${idx[i] + 1}-${idx[j] + 1}`);
      i = j;
    }
    return out.join(", ");
  };
  function hexColor(s, fallback) {
    const m = /^#?([0-9a-f]{6})$/i.exec(String(s || "").trim());
    if (!m) return fallback;
    const v = parseInt(m[1], 16);
    return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
  }
  /// Xác nhận (nói rõ sẽ làm gì) → nếu sẽ mở tệp kết quả: hỏi bỏ thay đổi chưa
  /// lưu → chọn nơi lưu. `pick`: "pdf" | "dir" | {ext, name}. null = đã huỷ.
  async function confirmAndPick(title, message, okLabel, pick, loads) {
    if (!(await confirmModal(title, message, okLabel))) return null;
    if (loads && window.Shell && !Shell.confirmDiscardChanges()) return null;
    const out = pick && pick.ext
      ? await invoke("pick_save_as", { ext: pick.ext, name: pick.name })
      : pick === "dir" ? await invoke("pick_dir") : await invoke("pick_save_pdf");
    return out || null;
  }
  async function openResult(out) {
    AI.keepThread = true;
    try { await loadDocument(out); } finally { AI.keepThread = false; }
  }
  async function applyPlan(mutate, title, message, okLabel) {
    const out = await confirmAndPick(title, message, okLabel, "pdf", true);
    if (!out) return null;
    let plan = await invoke("organize_identity_plan", { path: state.path, password: null });
    plan = mutate(plan);
    await invoke("organize_apply", { mainInput: state.path, plan, output: out, password: null });
    await openResult(out);
    return out;
  }

  const TOOLS = {
    async go_to_page({ page }) {
      needDoc();
      if (page < 1 || page > state.pages.length) throw fail(t("ai.tool.badPages", { pages: page, n: state.pages.length }));
      goToPage(page - 1);
      return { text: `Now showing page ${page} of ${state.pages.length}.`, label: t("ai.tool.wentTo", { n: page }) };
    },
    async search_text({ query, match_case }) {
      needDoc();
      $("searchBox").value = query;
      $("searchCase").checked = !!match_case;
      await runSearch();
      const pages = [...new Set(state.hits.map((h) => h.pageIndex + 1))];
      return {
        text: JSON.stringify({ matches: state.hits.length, pages: pages.slice(0, 100) }),
        label: t("ai.tool.found", { n: state.hits.length }),
      };
    },
    async rotate_pages({ pages, angle }) {
      needDoc();
      if (![90, 180, 270].includes(angle)) throw fail(t("ai.tool.badAngle"));
      const idx = pagesOf(pages);
      const set = new Set(idx);
      const out = await applyPlan((plan) => plan.map((e, i) => set.has(i) ? Object.assign({}, e, { rotationDelta: ((e.rotationDelta || 0) + angle) % 360 }) : e),
        t("ai.cfm.rotateTitle"), t("ai.cfm.rotate", { pages: ranges(idx), angle, name: docName() }), t("ai.cfm.saveAs"));
      if (!out) return { cancelled: true };
      return { text: `Rotated pages ${ranges(idx)} by ${angle}° and saved as ${shortName(out)}; the app now shows the new file.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async delete_pages({ pages }) {
      needDoc();
      const idx = pagesOf(pages);
      if (idx.length >= state.pages.length) throw fail(t("ai.tool.cantDeleteAll"));
      const set = new Set(idx);
      const out = await applyPlan((plan) => plan.filter((_, i) => !set.has(i)),
        t("ai.cfm.deleteTitle"), t("ai.cfm.delete", { pages: ranges(idx), n: idx.length, name: docName() }), t("ai.cfm.saveAs"));
      if (!out) return { cancelled: true };
      return { text: `Deleted pages ${ranges(idx)} and saved as ${shortName(out)} (${state.pages.length} pages); the app now shows the new file.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async extract_pages({ pages }) {
      needDoc();
      const idx = pagesOf(pages);
      const out = await confirmAndPick(t("ai.cfm.extractTitle"), t("ai.cfm.extract", { pages: ranges(idx), name: docName() }), t("ai.cfm.saveAs"), "pdf", false);
      if (!out) return { cancelled: true };
      await invoke("organize_extract", { input: state.path, pages: idx, output: out, password: null });
      return { text: `Extracted pages ${ranges(idx)} into ${shortName(out)}. The open document is unchanged.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async add_watermark({ text, pages, font_size, rotation, opacity, color }) {
      needDoc();
      if (!text.trim()) throw fail(t("ai.tool.emptyText"));
      const idx = pages ? pagesOf(pages) : [];
      const alpha = Math.round(255 * Math.min(1, Math.max(0.05, opacity == null ? 0.35 : opacity)));
      const [r, g, bl] = hexColor(color, [200, 0, 0]);
      const spec = {
        text, fontSize: font_size || 48, color: [r, g, bl, alpha], bold: true, italic: false,
        rotationDeg: rotation == null ? 45 : rotation, anchor: "center", pages: idx,
      };
      const out = await confirmAndPick(t("ai.cfm.wmTitle"),
        t("ai.cfm.wm", { text, pages: idx.length ? ranges(idx) : t("ai.allPages"), name: docName() }), t("ai.cfm.saveAs"), "pdf", true);
      if (!out) return { cancelled: true };
      await invoke("watermark_add", { input: state.path, spec, output: out, password: null });
      await openResult(out);
      return { text: `Added watermark "${text}" and saved as ${shortName(out)}; the app now shows the new file.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async add_header_footer({ header, footer, page_numbers, pages, font_size }) {
      needDoc();
      if (!header && !footer && !page_numbers) throw fail(t("ai.tool.emptyText"));
      const idx = pages ? pagesOf(pages) : [];
      const spec = {
        topCenter: header || "", bottomCenter: footer || "", bottomRight: page_numbers ? "{page}/{total}" : "",
        fontSize: font_size || 10, color: [0, 0, 0, 255], marginPt: 20,
        date: new Date().toLocaleDateString(I18N.lang === "vi" ? "vi-VN" : "en-US"), pages: idx,
      };
      const what = [header ? t("ai.cfm.hfHeader", { v: header }) : "", footer ? t("ai.cfm.hfFooter", { v: footer }) : "", page_numbers ? t("ai.cfm.hfNumbers") : ""].filter(Boolean).join("; ");
      const out = await confirmAndPick(t("ai.cfm.hfTitle"),
        t("ai.cfm.hf", { what, pages: idx.length ? ranges(idx) : t("ai.allPages"), name: docName() }), t("ai.cfm.saveAs"), "pdf", true);
      if (!out) return { cancelled: true };
      await invoke("header_footer_add", { input: state.path, spec, output: out, password: null });
      await openResult(out);
      return { text: `Added header/footer and saved as ${shortName(out)}; the app now shows the new file.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async export_document({ format }) {
      needDoc();
      if (!["docx", "txt", "png"].includes(format)) throw fail(t("ai.tool.badFormat"));
      const base = docName().replace(/\.pdf$/i, "");
      const pick = format === "png" ? "dir" : { ext: format, name: `${base}.${format}` };
      const out = await confirmAndPick(t("ai.cfm.exportTitle"), t("ai.cfm.export", { fmt: format.toUpperCase(), name: docName() }), t("ai.cfm.choose"), pick, false);
      if (!out) return { cancelled: true };
      if (format === "docx") await invoke("convert_docx", { input: state.path, output: out });
      else if (format === "txt") await invoke("convert_txt", { input: state.path, output: out });
      else {
        const files = await invoke("convert_images", { input: state.path, outDir: out, dpi: 150 });
        return { text: `Exported ${files.length} PNG images to ${out}.`, label: t("ai.tool.savedTo", { name: out }) };
      }
      return { text: `Exported to ${shortName(out)}.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async run_ocr({ language }) {
      needDoc();
      const st = await invoke("convert_tools_status").catch(() => ({ tesseract: true }));
      if (!st.tesseract) throw fail(t("ai.tool.noTesseract"));
      const lang = ["vie+eng", "vie", "eng"].includes(language) ? language : "vie+eng";
      const out = await confirmAndPick(t("ai.cfm.ocrTitle"), t("ai.cfm.ocr", { lang, name: docName(), n: state.pages.length }), t("ai.cfm.saveAs"), "pdf", true);
      if (!out) return { cancelled: true };
      const words = await invoke("ocr_run", { input: state.path, lang, output: out });
      await openResult(out);
      return { text: `OCR recognized ${words} words; saved as ${shortName(out)} and opened it.`, label: t("ai.tool.savedTo", { name: shortName(out) }) };
    },
    async mark_redactions({ texts }) {
      needDoc();
      const r = await markTextsForRedaction(texts);
      return {
        text: JSON.stringify({ marked_areas: r.added, occurrences_per_text: r.perText, note: "Marks are pending review; nothing was removed. The user applies them in the Protect tab." }),
        label: t("ai.redact.marked", { n: r.added }),
      };
    },
  };

  // ---------------------------------------------------------------- đánh dấu redact theo chuỗi
  async function charBoxes(page) {
    if (!state.textLayers[page]) state.textLayers[page] = await invoke("page_text_layer", { path: state.path, page });
    return state.textLayers[page];
  }
  /// Khung (theo từng dòng) của một kết quả search_document.
  async function hitRects(h) {
    let boxes = [];
    try { boxes = await charBoxes(h.pageIndex); } catch (_) {}
    const sub = boxes.slice(h.charStart, h.charStart + h.charLen)
      .filter((b) => b.ch && b.ch.trim() && b.right > b.left && b.top > b.bottom);
    if (!sub.length) return h.rect ? [h.rect] : [];
    const lines = [];
    for (const b of sub) {
      const cur = lines[lines.length - 1];
      const hgt = b.top - b.bottom;
      if (cur && Math.abs(b.bottom - cur.bottom) <= 0.6 * hgt && b.left >= cur.left - hgt) {
        cur.left = Math.min(cur.left, b.left); cur.right = Math.max(cur.right, b.right);
        cur.top = Math.max(cur.top, b.top); cur.bottom = Math.min(cur.bottom, b.bottom);
      } else {
        lines.push({ left: b.left, right: b.right, top: b.top, bottom: b.bottom });
      }
    }
    return lines.map((r) => ({ left: r.left - 0.6, bottom: r.bottom - 0.6, right: r.right + 0.6, top: r.top + 0.6 }));
  }
  const sameRect = (a, b) => Math.abs(a.left - b.left) < 1 && Math.abs(a.right - b.right) < 1 && Math.abs(a.top - b.top) < 1 && Math.abs(a.bottom - b.bottom) < 1;
  async function markTextsForRedaction(texts) {
    const perText = {};
    const touched = new Set();
    let added = 0;
    for (const raw of texts) {
      const q = String(raw || "").trim();
      if (q.length < 2 || q in perText) continue;
      let hits = await invoke("search_document", { path: state.path, query: q, caseSensitive: true });
      if (!hits.length) hits = await invoke("search_document", { path: state.path, query: q, caseSensitive: false });
      perText[q] = hits.length;
      for (const h of hits) {
        for (const rect of await hitRects(h)) {
          if (state.redactMarks.some((m) => m.page === h.pageIndex && sameRect(m.rect, rect))) continue;
          state.redactMarks.push({ page: h.pageIndex, rect });
          touched.add(h.pageIndex);
          added++;
        }
      }
    }
    touched.forEach((p) => drawAnnotsForPage(p));
    updateRedactButtons();
    return { added, perText };
  }

  // ---------------------------------------------------------------- hành động một chạm
  const inLang = () => ` Write the answer in ${answerLangName()}.`;
  const ACTIONS = {
    summaryShort: {
      label: "ai.act.summaryShort",
      prompt: () => "Summarize the document in 3-5 sentences, then give a one-line key takeaway. Cite pages." + inLang(),
      strategy: "summary",
    },
    summaryLong: {
      label: "ai.act.summaryLong",
      prompt: () => "Write a detailed, well-structured summary of the document: a short overview, then a section per main part with bullet points of the key facts, figures and conclusions. Cite pages." + inLang(),
      strategy: "summary",
      exportable: true,
    },
    keyPoints: {
      label: "ai.act.keyPoints",
      prompt: () => "List the key points of the document as a bullet list (at most about 10), most important first, each with a page citation." + inLang(),
      strategy: "summary",
    },
    extractInfo: {
      label: "ai.act.extractInfo",
      prompt: () => "Extract the important information from the document into Markdown tables:\n1. Dates and deadlines (Date | What | Page)\n2. People and organizations (Name | Role / relation | Page)\n3. Amounts and figures (Amount | Meaning | Page)\n4. Requirements and obligations (Requirement | Who | Deadline | Page)\nUse a heading for each table, skip a table that would be empty and say so in one line. Keep values exactly as written in the document. Cite pages in the Page column as [Trang N] or [p. N]." + inLang(),
      strategy: "summary",
      exportable: true,
    },
    compare: {
      label: "ai.act.compare",
      prompt: () => "Compare all provided documents: first a short description of each, then the main similarities, then the differences as a Markdown table (one column per document), then contradictions or conflicts, and finally a synthesis. Cite pages with the file name, e.g. [p. 3, name.pdf]." + inLang(),
      strategy: "summary",
      exportable: true,
    },
  };
  function runAction(key) {
    const a = ACTIONS[key];
    if (key !== "compare" && !state.path) { $("status").textContent = t("x.noDoc"); return; }
    if (key === "compare" && !AI.attachments.length) { openPanel(true); attachDocs(); return; }
    send(a.prompt(), t(a.label), { strategy: a.strategy, toolChoice: "none", exportable: a.exportable });
  }

  // Theo dõi vùng chữ chọn trên trang (click vào panel có thể làm mất selection).
  function currentSelection() {
    const sel = window.getSelection();
    const txt = sel ? sel.toString().trim() : "";
    if (txt && sel.anchorNode && $("pages").contains(sel.anchorNode)) {
      const slot = sel.anchorNode.parentElement && sel.anchorNode.parentElement.closest(".page-slot");
      return { text: txt, page: slot ? Number(slot.dataset.index) : state.current };
    }
    return null;
  }
  document.addEventListener("selectionchange", () => {
    const s = currentSelection();
    if (s) { AI.lastSel = s; refreshSelChip(); }
  });
  function refreshSelChip() {
    const s = AI.lastSel;
    $("aiSelChip").hidden = !s;
    if (s) {
      $("aiSelText").textContent = s.text.length > 120 ? s.text.slice(0, 120) + "…" : s.text;
      $("aiSelChip").title = t("ai.selFrom", { n: s.page + 1 });
    }
  }
  function needSelection() {
    const s = currentSelection() || AI.lastSel;
    if (!s) {
      openPanel(true);
      addNote(t("ai.needSelection"));
      return null;
    }
    return s;
  }
  const SEL_ACTIONS = {
    explain: {
      label: "ai.act.explain",
      prompt: (s) => `Explain the following passage from page ${s.page + 1} of the document in simple terms: what it means, key terms, and why it matters in the context of the document. Cite pages when you use other parts of the document.${inLang()}\n\n<selection>\n${s.text}\n</selection>`,
      docs: true,
    },
    rewrite: {
      label: "ai.act.rewrite",
      prompt: (s) => `Rewrite the following text to be clearer and more concise while keeping its meaning, tone and language. Return only the rewritten text, without comments or quotes.\n\n<text>\n${s.text}\n</text>`,
    },
    improve: {
      label: "ai.act.improve",
      prompt: (s) => `Improve the writing of the following text (clarity, flow, word choice, professional tone) while keeping its meaning and language. Return only the improved text, then a line "---" and at most 3 short bullet notes about the main changes, written in ${answerLangName()}.\n\n<text>\n${s.text}\n</text>`,
    },
    grammar: {
      label: "ai.act.grammar",
      prompt: (s) => `Fix spelling, grammar and punctuation in the following text. Keep the wording and language otherwise unchanged (for Vietnamese, also fix missing or wrong diacritics). Return only the corrected text.\n\n<text>\n${s.text}\n</text>`,
    },
  };
  function runSelAction(key) {
    const s = needSelection();
    if (!s) return;
    const a = SEL_ACTIONS[key];
    const preview = s.text.length > 160 ? s.text.slice(0, 160) + "…" : s.text;
    send(a.prompt(s), `${t(a.label)}: “${preview}”`, {
      noDocs: !a.docs, strategy: "auto", query: s.text, toolChoice: "none", copyLast: !a.docs,
    });
  }

  // ---------------------------------------------------------------- dịch
  async function openTranslate() {
    if (!AI.settings) await loadSettings();
    if (!ensureReady()) return;
    const sel = currentSelection() || AI.lastSel;
    const def = (AI.settings.language === "en" || I18N.lang === "en") ? "vi" : "en";
    const box = openModal(t("ai.tr.title"), `
      <label>${t("ai.tr.target")}</label>
      <select id="aiTrLang">${TRANSLATE_LANGS.map((l) => `<option value="${l}"${l === def ? " selected" : ""}>${esc(t("ai.lang." + l))}</option>`).join("")}</select>
      <label>${t("ai.tr.scope")}</label>
      <div class="radiorow ai-radiocol">
        <label><input type="radio" name="aiTrScope" value="sel" ${sel ? "checked" : "disabled"}> ${t("ai.tr.scopeSel")}</label>
        <label><input type="radio" name="aiTrScope" value="page" ${sel ? "" : "checked"} ${state.path ? "" : "disabled"}> ${t("ai.tr.scopePage", { n: state.current + 1 })}</label>
        <label><input type="radio" name="aiTrScope" value="doc" ${state.path ? "" : "disabled"}> ${t("ai.tr.scopeDoc", { n: state.pages.length })}</label>
      </div>
      <p class="muted">${t("ai.tr.docNote")}</p>
      <div class="foot"><button id="aiTrCancel">${t("common.cancel")}</button><button id="aiTrOk" class="primary">${t("ai.tr.go")}</button></div>`);
    box.querySelector("#aiTrCancel").addEventListener("click", closeModal);
    box.querySelector("#aiTrOk").addEventListener("click", async () => {
      const lang = box.querySelector("#aiTrLang").value;
      const scope = (box.querySelector("input[name=aiTrScope]:checked") || {}).value;
      closeModal();
      const target = LANG_NAME_EN[lang];
      const langLabel = t("ai.lang." + lang);
      if (scope === "sel" && sel) {
        send(`Translate the following text into ${target}. Preserve meaning, tone, numbers and formatting. Return only the translation.\n\n<text>\n${sel.text}\n</text>`,
          t("ai.tr.userSel", { lang: langLabel }), { noDocs: true, toolChoice: "none", copyLast: true });
      } else if (scope === "page") {
        try {
          const txt = await invoke("page_text", { path: state.path, page: state.current });
          if (!txt.trim()) { addNote(t("ai.tr.emptyPage")); openPanel(true); return; }
          send(`Translate page ${state.current + 1} of the document into ${target}. Preserve meaning, numbers, headings and list structure (use Markdown). Return only the translation.\n\n<page>\n${txt}\n</page>`,
            t("ai.tr.userPage", { lang: langLabel, n: state.current + 1 }), { noDocs: true, toolChoice: "none", exportable: true, copyLast: true });
        } catch (e) { $("status").textContent = t("ai.err.generic", { e }); }
      } else if (scope === "doc") {
        translateDocument(target, langLabel);
      }
    });
  }

  /// Dịch cả tài liệu: chia theo nhóm trang (~8000 ký tự), mỗi nhóm một request
  /// độc lập (không nhồi vào hội thoại), stream vào cùng một bong bóng.
  async function translateDocument(target, langLabel) {
    if (!ensureReady() || AI.busy) return;
    openPanel(true);
    addUserBubble(t("ai.tr.userDoc", { lang: langLabel, name: docName() }));
    const b = addAssistantBubble();
    setBusy(true);
    b.status(t("ai.status.reading"));
    try {
      const pages = await invoke("ai_doc_pages", { path: state.path });
      const groups = [];
      let cur = [], size = 0;
      pages.forEach((p, i) => {
        if (cur.length && size + p.length > 8000) { groups.push(cur); cur = []; size = 0; }
        cur.push(i); size += p.length;
      });
      if (cur.length) groups.push(cur);
      if (!pages.some((p) => p.trim())) { b.error(t("ai.tr.noText")); return; }
      for (let g = 0; g < groups.length; g++) {
        const idx = groups[g];
        const body = idx.map((i) => `[Trang ${i + 1}]\n${pages[i] || ""}`).join("\n\n");
        b.status(t("ai.tr.progress", { done: g + 1, total: groups.length }));
        const res = await aiRequest({
          messages: [{ role: "user", content: `Translate these pages of a document into ${target}. Keep every page marker like [Trang 3] unchanged on its own line, preserve numbers, names, headings and list structure (use Markdown), and do not add commentary. Pages with no text: keep only the marker.\n\n<pages>\n${body}\n</pages>` }],
          docs: [], docStrategy: "none", toolChoice: "none",
        }, { onDelta: (s) => { b.status(null); b.append(s); }, onStatus: () => {} });
        if (res.stopReason === "refusal") b.append(`\n\n[Trang ${idx[0] + 1}] (…)\n`);
        b.newSegment();
      }
      b.done();
      addStandardActions(b, { exportable: true });
    } catch (e) {
      const er = parseErr(e);
      b.error(er.kind === "cancelled" ? t("ai.stopped") : errorText(er));
      if (b.text.trim()) addStandardActions(b, { exportable: true });
    } finally {
      b.done();
      setBusy(false);
    }
  }

  // ---------------------------------------------------------------- AI Smart Redact
  async function openSmartRedact() {
    if (!state.path) { $("status").textContent = t("x.noDoc"); return; }
    if (!AI.settings) await loadSettings();
    if (!ensureReady()) return;
    const box = openModal(t("ai.redact.title"), `
      <p>${t("ai.redact.intro")}</p>
      <div class="ai-catgrid">${PII_CATS.map((c) => `<label><input type="checkbox" value="${c}" checked> ${esc(t("ai.pii." + c))}</label>`).join("")}</div>
      <p class="muted">${t("ai.redact.note")}</p>
      <div class="foot"><button id="aiRdCancel">${t("common.cancel")}</button><button id="aiRdGo" class="primary">${t("ai.redact.go")}</button></div>`);
    box.querySelector("#aiRdCancel").addEventListener("click", closeModal);
    box.querySelector("#aiRdGo").addEventListener("click", () => {
      const cats = [...box.querySelectorAll(".ai-catgrid input:checked")].map((i) => i.value);
      closeModal();
      if (cats.length) smartRedact(cats);
    });
  }

  async function smartRedact(categories) {
    if (AI.busy) return;
    openPanel(true);
    addUserBubble(t("ai.redact.user", { list: categories.map((c) => t("ai.pii." + c)).join(", ") }));
    const b = addAssistantBubble();
    setBusy(true);
    const requestId = newId();
    AI.currentId = requestId;
    pending.set(requestId, { onStatus: (p) => b.status(statusLabel(p)), resolve() {}, reject() {} });
    try {
      const items = await invoke("ai_find_pii", { requestId, path: state.path, categories });
      if (!items.length) { b.done(); b.setText(t("ai.redact.none")); return; }
      b.status(t("ai.redact.locating"));
      const r = await markTextsForRedaction(items.map((i) => i.text));
      b.done();
      const rows = items.map((i) => `<tr><td>${esc(t("ai.pii." + i.category))}</td><td>${esc(i.text)}</td><td>${r.perText[i.text] || 0}</td></tr>`).join("");
      b.setText(t("ai.redact.result", { n: items.length, marks: r.added }));
      b.extra(`<div class="ai-tablewrap"><table><thead><tr><th>${esc(t("ai.redact.colCat"))}</th><th>${esc(t("ai.redact.colText"))}</th><th>${esc(t("ai.redact.colHits"))}</th></tr></thead><tbody>${rows}</tbody></table></div>`);
      b.action("redact", t("ai.redact.review"), () => { if (window.Shell) Shell.setTab("protect"); });
    } catch (e) {
      const er = parseErr(e);
      b.error(er.kind === "cancelled" ? t("ai.stopped") : errorText(er));
    } finally {
      pending.delete(requestId);
      if (AI.currentId === requestId) AI.currentId = null;
      b.done();
      setBusy(false);
    }
  }

  // ---------------------------------------------------------------- Đọc to (không cần AI)
  const TTS = { gen: 0, queue: [], i: 0, mode: null, page: 0, speaking: false, paused: false };
  const synth = window.speechSynthesis;
  // Chữ đặc trưng tiếng Việt (ă â đ ê ô ơ ư + khối Latin mở rộng U+1EA0–U+1EF9).
  const VI_RE = /[\u0103\u00e2\u0111\u00ea\u00f4\u01a1\u01b0\u1ea0-\u1ef9]/i;
  function ttsVoices() { return synth ? synth.getVoices() : []; }
  function fillVoices() {
    const sel = $("aiTtsVoice");
    const cur = sel.value || (() => { try { return localStorage.getItem("ff.ttsVoice") || ""; } catch (_) { return ""; } })();
    const vs = ttsVoices();
    sel.innerHTML = `<option value="">${esc(t("ai.tts.voiceAuto"))}</option>` +
      vs.map((v) => `<option value="${esc(v.voiceURI)}">${esc(v.name)} (${esc(v.lang)})</option>`).join("");
    sel.value = vs.some((v) => v.voiceURI === cur) ? cur : "";
  }
  function pickVoice(lang) {
    const vs = ttsVoices();
    const chosen = $("aiTtsVoice").value;
    if (chosen) { const v = vs.find((x) => x.voiceURI === chosen); if (v) return v; }
    const pref = vs.filter((v) => v.lang && v.lang.toLowerCase().startsWith(lang));
    return pref.find((v) => /natural|online/i.test(v.name)) || pref.find((v) => v.localService) || pref[0] || null;
  }
  function splitSpeech(text) {
    const out = [];
    const clean = text.replace(/\s+/g, " ").trim();
    const sentences = clean.match(/[^.!?;:…]+[.!?;:…]*\s*/g) || [clean];
    let cur = "";
    for (const s of sentences) {
      if ((cur + s).length > 220 && cur) { out.push(cur.trim()); cur = ""; }
      if (s.length > 220) {
        for (let k = 0; k < s.length; k += 200) out.push(s.slice(k, k + 200));
      } else cur += s;
    }
    if (cur.trim()) out.push(cur.trim());
    return out.filter(Boolean);
  }
  function ttsUi() {
    $("aiTtsPlay").querySelector("i").dataset.icon = TTS.speaking && !TTS.paused ? "pause" : "play";
    $("aiTtsPlay").title = t(TTS.speaking && !TTS.paused ? "ai.tts.pause" : "ai.tts.play");
    applyIcons($("aiTtsPlay"));
    $("aiTtsStop").disabled = !TTS.speaking;
    $("aiTtsState").textContent = !TTS.speaking ? "" : TTS.paused ? t("ai.tts.paused")
      : TTS.mode === "selection" ? t("ai.tts.readingSel") : t("ai.tts.readingPage", { n: TTS.page + 1 });
    const rb = $("aiRbRead");
    if (rb) rb.classList.toggle("on", TTS.speaking);
  }
  function ttsStop() {
    TTS.gen++;
    TTS.speaking = false;
    TTS.paused = false;
    if (synth) synth.cancel();
    ttsUi();
  }
  function speakQueue(gen, onFinish) {
    if (gen !== TTS.gen) return;
    if (TTS.i >= TTS.queue.length) { onFinish(); return; }
    const text = TTS.queue[TTS.i];
    const lang = VI_RE.test(TTS.queue.join(" ").slice(0, 2000)) ? "vi" : "en";
    const u = new SpeechSynthesisUtterance(text);
    const v = pickVoice(lang);
    if (v) { u.voice = v; u.lang = v.lang; } else u.lang = lang === "vi" ? "vi-VN" : "en-US";
    u.rate = Number($("aiTtsRate").value) || 1;
    u.onend = () => { if (gen === TTS.gen) { TTS.i++; speakQueue(gen, onFinish); } };
    u.onerror = (ev) => {
      if (gen !== TTS.gen || ev.error === "interrupted" || ev.error === "canceled") return;
      $("aiTtsState").textContent = t("ai.tts.error", { e: ev.error || "" });
      TTS.i++;
      speakQueue(gen, onFinish);
    };
    synth.speak(u);
  }
  async function ttsReadPage(gen, page, continueAfter) {
    if (gen !== TTS.gen) return;
    TTS.page = page;
    ttsUi();
    let txt = "";
    try { txt = await invoke("page_text", { path: state.path, page }); } catch (_) {}
    if (gen !== TTS.gen) return;
    const next = () => {
      if (continueAfter && page + 1 < state.pages.length) { goToPage(page + 1); ttsReadPage(gen, page + 1, true); }
      else ttsStop();
    };
    if (!txt.trim()) {
      $("status").textContent = t("ai.tts.emptyPage", { n: page + 1 });
      if (continueAfter) next(); else ttsStop();
      return;
    }
    TTS.queue = splitSpeech(txt);
    TTS.i = 0;
    speakQueue(gen, next);
  }
  function ttsStart(mode) {
    if (!synth) { $("status").textContent = t("ai.tts.unsupported"); return; }
    ttsStop();
    const gen = TTS.gen;
    TTS.mode = mode;
    TTS.speaking = true;
    if (mode === "selection") {
      const s = currentSelection() || AI.lastSel;
      if (!s) { ttsStop(); addNote(t("ai.needSelection")); return; }
      TTS.queue = splitSpeech(s.text);
      TTS.i = 0;
      ttsUi();
      speakQueue(gen, ttsStop);
      return;
    }
    if (!state.path) { ttsStop(); $("status").textContent = t("x.noDoc"); return; }
    ttsReadPage(gen, state.current, mode === "from");
  }
  function ttsToggle() {
    if (!synth) { $("status").textContent = t("ai.tts.unsupported"); return; }
    if (!TTS.speaking) { ttsStart($("aiTtsMode").value); return; }
    if (TTS.paused) { synth.resume(); TTS.paused = false; } else { synth.pause(); TTS.paused = true; }
    ttsUi();
  }
  if (synth) {
    fillVoices();
    synth.addEventListener && synth.addEventListener("voiceschanged", fillVoices);
  }

  // ---------------------------------------------------------------- gắn sự kiện
  function submitInput() {
    const v = $("aiInput").value.trim();
    if (!v || AI.busy) return;
    if (!ensureReady()) return;
    $("aiInput").value = "";
    autoGrow();
    const ctx = state.path ? `\n\n[App context: the user is viewing page ${state.current + 1} of ${state.pages.length} of "${docName()}".]` : "";
    send(v + ctx, v, { strategy: "auto", query: v, toolChoice: "auto" });
  }
  function autoGrow() {
    const ta = $("aiInput");
    ta.style.height = "auto";
    ta.style.height = Math.min(160, ta.scrollHeight) + "px";
  }

  $("aiQuickBtn").addEventListener("click", () => openPanel());
  $("aiRbPanel").addEventListener("click", () => openPanel());
  // Vào tab Trợ lý AI → mở luôn bảng (như Foxit).
  const aiPill = document.querySelector('.tab-pill[data-tab="ai"]');
  if (aiPill) aiPill.addEventListener("click", () => openPanel(true));
  $("aiClose").addEventListener("click", () => openPanel(false));
  $("aiSettingsBtn").addEventListener("click", openSettings);
  $("aiRbSettings").addEventListener("click", openSettings);
  $("aiOnboardBtn").addEventListener("click", openSettings);
  $("aiNewChat").addEventListener("click", () => { if (!AI.busy) resetThread(); });
  $("aiSend").addEventListener("click", submitInput);
  $("aiStop").addEventListener("click", () => {
    if (AI.currentId) invoke("ai_cancel", { requestId: AI.currentId }).catch(() => {});
    ttsStop();
  });
  $("aiInput").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); submitInput(); }
  });
  $("aiInput").addEventListener("input", autoGrow);
  $("aiSelClear").addEventListener("click", () => { AI.lastSel = null; refreshSelChip(); });
  $("aiDocs").addEventListener("click", (e) => {
    const x = e.target.closest(".ai-doc-x");
    if (x) { AI.attachments.splice(Number(x.dataset.i), 1); renderDocChips(); return; }
    if (e.target.closest("#aiAttach")) attachDocs();
  });
  // Trích dẫn trang → nhảy tới trang (chỉ với tài liệu đang mở).
  $("aiThread").addEventListener("click", (e) => {
    const a = e.target.closest(".ai-cite");
    if (!a) return;
    e.preventDefault();
    const doc = a.dataset.doc;
    if (doc && docName() && doc.toLowerCase() !== docName().toLowerCase()) {
      $("status").textContent = t("ai.citeOtherDoc", { name: doc, n: a.dataset.page });
      return;
    }
    const p = Number(a.dataset.page) - 1;
    if (p >= 0 && p < state.pages.length) goToPage(p);
  });
  // Chip hành động (panel + ribbon).
  document.addEventListener("click", (e) => {
    const c = e.target.closest("[data-ai-act]");
    if (!c || c.disabled) return;
    const k = c.dataset.aiAct;
    if (ACTIONS[k]) runAction(k);
    else if (SEL_ACTIONS[k]) runSelAction(k);
    else if (k === "translate") openTranslate();
    else if (k === "redact") openSmartRedact();
    else if (k === "read") { openPanel(true); ttsToggle(); }
  });
  $("aiTtsPlay").addEventListener("click", ttsToggle);
  $("aiTtsStop").addEventListener("click", ttsStop);
  $("aiTtsMode").addEventListener("change", () => { if (TTS.speaking) ttsStart($("aiTtsMode").value); });
  $("aiTtsVoice").addEventListener("change", () => { try { localStorage.setItem("ff.ttsVoice", $("aiTtsVoice").value); } catch (_) {} });
  $("aiTtsRate").addEventListener("change", () => { try { localStorage.setItem("ff.ttsRate", $("aiTtsRate").value); } catch (_) {} });

  // Kéo mép trái để đổi độ rộng panel.
  $("aiResizer").addEventListener("mousedown", (e) => {
    e.preventDefault();
    const p = panel();
    const right = p.getBoundingClientRect().right;
    document.body.classList.add("ai-resizing");
    const move = (ev) => { p.style.width = Math.max(300, Math.min(760, right - ev.clientX)) + "px"; };
    const up = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      document.body.classList.remove("ai-resizing");
      try { localStorage.setItem("ff.aiPanelW", String(parseInt(p.style.width, 10))); } catch (_) {}
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
  });

  // Tài liệu mới → hội thoại mới (trừ khi chính AI vừa mở tệp kết quả của tool).
  document.addEventListener("docloaded", (e) => {
    const name = e.detail && e.detail.path ? shortName(e.detail.path) : null;
    const prev = AI.docLabel;
    AI.docLabel = name;
    AI.lastSel = null;
    refreshSelChip();
    if (!AI.keepThread && name !== prev) {
      if (prev && $("aiThread").children.length) resetThread(t("ai.newDocThread", { name }));
      else AI.messages = [];
    }
    if (!panel().hidden) renderDocChips();
  });

  if (window.Shell && Shell.onLangChange) Shell.onLangChange(() => { refreshConfigured(); renderDocChips(); fillVoices(); ttsUi(); });

  // ---------------------------------------------------------------- khởi tạo
  try {
    const w = Number(localStorage.getItem("ff.aiPanelW"));
    if (w >= 300) panel().style.width = Math.min(760, w) + "px";
    const r = localStorage.getItem("ff.ttsRate");
    if (r) $("aiTtsRate").value = r;
  } catch (_) {}
  loadSettings().then(() => {
    let open = false;
    try { open = localStorage.getItem("ff.aiPanel") === "1"; } catch (_) {}
    if (open) openPanel(true);
  });
  ttsUi();
  refreshWelcome();

  window.AIAssistant = { open: openPanel, send, openSettings, renderMarkdown };
})();
