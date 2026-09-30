// Chú thích mở rộng — tab Comment kiểu Foxit:
//  • Vẽ: Bút chì (Ink, nhiều nét), Đường thẳng, Mũi tên, Chữ nhật, Oval, Đa giác,
//    Đường gấp khúc, Đám mây — xem trước trực tiếp khi kéo/nhấp.
//  • Con dấu: mẫu có sẵn (Đã duyệt, Bản nháp, Mật...), dấu ngày giờ + tác giả, ảnh PNG/JPG.
//  • Thuộc tính: màu nét (nút Màu sẵn có), màu tô, độ dày, độ mờ.
//  • Chú thích CÓ SẴN trong file: liệt kê (loại, trang, tác giả, ngày, nội dung),
//    chọn → nhảy trang, sửa nội dung/màu/tác giả, kéo dời/co giãn, xoá.
//  • Tác giả (/T) lưu trong localStorage (mặc định = tên người dùng hệ điều hành).
// Mọi thay đổi đi qua undo toàn cục (snapshot annotSpecs + annotExisting) và
// được lưu 1 lần qua lệnh `annot_save`.
(function () {
  "use strict";
  const LS_AUTHOR = "ff.annotAuthor";
  const LS_PROPS = "ff.annotProps";
  const DRAW_TOOLS = ["ink", "line", "arrow", "square", "oval", "polygon", "polyline", "cloud", "stamp"];
  const POLY_TOOLS = ["polygon", "polyline", "cloud"];
  const FILLABLE = ["rect", "oval", "polygon", "cloud"];
  const TOOL_KIND = { square: "rect" };
  const KIND_ICON = {
    highlight: "highlight", underline: "underline", strikeout: "strikeout", squiggly: "underline",
    rect: "square", square: "square", oval: "oval", line: "line", arrow: "arrow", polygon: "polygon",
    polyline: "polyline", cloud: "cloud", ink: "pencil", stamp: "stamp", freetext: "textbox", note: "note",
  };

  // Con dấu mẫu (tên /Name theo chuẩn Acrobat/Foxit để app khác nhận diện).
  const STAMPS = [
    { name: "SBApproved", key: "annx.st.approved", color: [22, 128, 60] },
    { name: "SBNotApproved", key: "annx.st.rejected", color: [200, 30, 30] },
    { name: "SBDraft", key: "annx.st.draft", color: [30, 80, 200] },
    { name: "SBConfidential", key: "annx.st.confidential", color: [200, 30, 30] },
    { name: "SBReviewed", key: "annx.st.reviewed", color: [30, 80, 200] },
    { name: "SBCompleted", key: "annx.st.completed", color: [22, 128, 60] },
    { name: "SHSignHere", key: "annx.st.signHere", color: [220, 90, 0] },
  ];
  const DYN_STAMPS = [
    { name: "#DApproved", key: "annx.st.approved", color: [22, 128, 60], dynamic: true },
    { name: "#DReviewed", key: "annx.st.reviewed", color: [30, 80, 200], dynamic: true },
    { name: "#DReceived", key: "annx.st.received", color: [30, 80, 200], dynamic: true },
  ];

  if (!Array.isArray(state.annotExisting)) state.annotExisting = [];

  const X = {
    author: "",
    width: 2,
    opacity: 1,
    fill: null,
    stamp: STAMPS[0],
    orig: new Map(),    // xid → JSON gốc của chú thích có sẵn (để biết đã sửa)
    snaps: new Map(),   // xid → ảnh cắt từ trang (preview khi dời loại không tự vẽ được)
    imgs: new Map(),    // đường dẫn ảnh con dấu → {dataUrl, width, height}
    loadSeq: 0,
    draft: null,        // hình đang vẽ
    ghost: null,        // vị trí xem trước con dấu theo con trỏ
    lastInk: null,      // {id, idx, t} gom các nét liền nhau vào cùng 1 Ink (như Foxit)
    drag: null,         // dời / co giãn
  };

  // ---------- Tiện ích ----------
  const scale = () => PT_PER_PX * state.zoom;
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const isX = (id) => typeof id === "string" && id.startsWith("x:");
  const rgbA = (c, a) => `rgba(${c[0]},${c[1]},${c[2]},${a})`;
  const esc = (s) => escapeHtml(String(s == null ? "" : s)).replace(/"/g, "&quot;");
  const fmtN = (v) => Math.round(v * 100) / 100;
  function lsGet(k) { try { return localStorage.getItem(k); } catch (_) { return null; } }
  function lsSet(k, v) { try { localStorage.setItem(k, v); } catch (_) { /* bỏ qua */ } }

  function locale() { return I18N.lang === "vi" ? "vi-VN" : "en-US"; }
  function pdfDateNow() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, "0");
    const off = -d.getTimezoneOffset();
    const a = Math.abs(off);
    return `D:${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}` +
      `${off >= 0 ? "+" : "-"}${p(Math.floor(a / 60))}'${p(a % 60)}'`;
  }
  function parsePdfDate(s) {
    const m = /D:(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?(\d{2})?([Zz+-])?(\d{2})?'?(\d{2})?/.exec(s || "");
    if (!m) return null;
    const n = (v, d) => (v ? Number(v) : d);
    let ms = Date.UTC(n(m[1]), n(m[2], 1) - 1, n(m[3], 1), n(m[4], 0), n(m[5], 0), n(m[6], 0));
    if (m[7] === "+" || m[7] === "-") {
      const off = (n(m[8], 0) * 60 + n(m[9], 0)) * 60000;
      ms += m[7] === "+" ? -off : off;
    } else if (!m[7]) {
      ms += new Date().getTimezoneOffset() * 60000; // không có múi giờ → coi như giờ địa phương
    }
    return new Date(ms);
  }
  // "30/09/2026 14:05" (vi) / "09/30/2026, 02:05 PM" (en).
  function fmtDateObj(d) {
    if (I18N.lang !== "vi") return d.toLocaleString(locale(), { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
    const p = (n) => String(n).padStart(2, "0");
    return `${p(d.getDate())}/${p(d.getMonth() + 1)}/${d.getFullYear()} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }
  function fmtDate(s) {
    const d = parsePdfDate(s);
    return d ? fmtDateObj(d) : "";
  }

  // mousedown đã preventDefault (không cho trình duyệt bôi chọn) nên focus không
  // tự rời ô nhập (vd ô Tìm kiếm) → phím Delete/Enter sẽ bị coi là đang gõ.
  function blurInputs() {
    const a = document.activeElement;
    if (a && a !== document.body && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName))) a.blur();
  }
  function ptOf(slot, e) {
    const r = slot.getBoundingClientRect();
    return cssToPdf(Number(slot.dataset.index), e.clientX - r.left, e.clientY - r.top);
  }
  function rectOf(a, b) {
    return { left: Math.min(a.x, b.x), right: Math.max(a.x, b.x), bottom: Math.min(a.y, b.y), top: Math.max(a.y, b.y) };
  }

  // ---------- Mô hình ----------
  function allItems() {
    return state.annotSpecs.concat((state.annotExisting || []).filter((r) => !r.deleted));
  }
  function findItem(id) {
    if (id == null) return null;
    if (isX(id)) return (state.annotExisting || []).find((r) => r.xid === id) || null;
    return state.annotSpecs.find((s) => s.id === id) || null;
  }
  // Item do file này tự vẽ/sửa được (hình vẽ mới + mọi chú thích có sẵn).
  const isOurs = (it) => !!it && (it.ext || it.existing);

  function kindOfDetail(d) {
    const st = d.subtype;
    const le = d.lineEndings || [];
    const arrow = le.some((x) => /Arrow/.test(x));
    return ({
      Highlight: "highlight", Underline: "underline", StrikeOut: "strikeout", Squiggly: "squiggly",
      Square: "rect", Circle: "oval", Ink: "ink", Stamp: "stamp", FreeText: "freetext", Text: "note",
      PolyLine: "polyline", Polygon: d.cloudy ? "cloud" : "polygon", Line: arrow ? "arrow" : "line",
    })[st] || "other";
  }
  function recFromDetail(d) {
    let points = [];
    if (d.line) points = [[d.line[0], d.line[1]], [d.line[2], d.line[3]]];
    else if (d.vertices && d.vertices.length) points = d.vertices;
    return {
      xid: `x:${d.pageIndex}:${d.annotIndex}`,
      existing: true,
      pageIndex: d.pageIndex,
      annotIndex: d.annotIndex,
      nm: d.nm || null,
      subtype: d.subtype,
      kind: kindOfDetail(d),
      left: d.rect.left, bottom: d.rect.bottom, right: d.rect.right, top: d.rect.top,
      points,
      strokes: d.ink || [],
      quads: d.quads || [],
      le: d.lineEndings || ["None", "None"],
      color: d.color || [0, 0, 0],
      fill: d.fill || null,
      width: d.width == null ? 1 : d.width,
      opacity: d.opacity == null ? 1 : d.opacity,
      contents: d.contents || "",
      author: d.author || "",
      subject: d.subject || "",
      modified: d.modified || "",
      created: d.created || "",
      fontSize: d.fontSize || 12,
      icon: d.icon || "",
      irt: d.inReplyTo == null ? null : `x:${d.pageIndex}:${d.inReplyTo}`,
      hiddenFlag: !!d.hidden,
      deleted: false,
    };
  }
  const GEO_KEYS = ["left", "bottom", "right", "top"];
  const sameArr = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  function origOf(r) { return X.orig.get(r.xid) || null; }
  // Thay đổi làm đổi HÌNH (phải ẩn bản render gốc & vẽ preview thay).
  function visualDirty(r) {
    if (r.deleted) return true;
    const o = origOf(r);
    if (!o) return false;
    return GEO_KEYS.some((k) => Math.abs(r[k] - o[k]) > 0.01) ||
      !sameArr(r.color, o.color) || !sameArr(r.fill, o.fill) ||
      r.width !== o.width || r.opacity !== o.opacity ||
      (r.kind === "freetext" && r.contents !== o.contents);
  }
  function updateOf(r) {
    const o = origOf(r);
    if (!o || r.deleted) return null;
    const u = { pageIndex: r.pageIndex, annotIndex: r.annotIndex, nm: r.nm };
    let any = false;
    if (GEO_KEYS.some((k) => Math.abs(r[k] - o[k]) > 0.01)) {
      u.rect = { left: r.left, bottom: r.bottom, right: r.right, top: r.top };
      any = true;
    }
    if (!sameArr(r.color, o.color)) { u.color = r.color; any = true; }
    if (!sameArr(r.fill, o.fill)) { u.setFill = true; u.fill = r.fill; any = true; }
    if (r.width !== o.width) { u.width = r.width; any = true; }
    if (r.opacity !== o.opacity) { u.opacity = r.opacity; any = true; }
    if (r.contents !== o.contents) { u.contents = r.contents; any = true; }
    if (r.author !== o.author) { u.author = r.author; any = true; }
    return any ? u : null;
  }
  function pendingCount() {
    let n = state.annotSpecs.length;
    for (const r of state.annotExisting || []) if (r.deleted || updateOf(r)) n++;
    return n;
  }

  // Khung bao cho hình vẽ mới (tính từ điểm/nét + nửa độ dày nét).
  function fitBounds(s) {
    let pts = [];
    if (s.kind === "ink") pts = s.strokes.flat();
    else if (s.points && s.points.length && !["rect", "oval", "stamp"].includes(s.kind)) pts = s.points;
    if (!pts.length) return;
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    const pad = (s.width || 1) / 2 + (s.kind === "arrow" ? 6 + (s.width || 1) * 3 : 0) + (s.kind === "cloud" ? 6 + (s.width || 1) * 3 : 0);
    s.left = Math.min(...xs) - pad; s.right = Math.max(...xs) + pad;
    s.bottom = Math.min(...ys) - pad; s.top = Math.max(...ys) + pad;
  }
  // Ánh xạ hình học từ khung `from` sang khung `to` (dời/co giãn).
  function mapGeom(it, base, to) {
    const fw = base.right - base.left || 1;
    const fh = base.top - base.bottom || 1;
    const sx = (to.right - to.left) / fw;
    const sy = (to.top - to.bottom) / fh;
    const m = (p) => [to.left + (p[0] - base.left) * sx, to.bottom + (p[1] - base.bottom) * sy];
    it.left = to.left; it.right = to.right; it.bottom = to.bottom; it.top = to.top;
    if (base.points) it.points = base.points.map(m);
    if (base.strokes) it.strokes = base.strokes.map((st) => st.map(m));
    if (base.quads) it.quads = base.quads.map((q) => {
      const a = m([q.left, q.bottom]);
      const b = m([q.right, q.top]);
      return { left: a[0], bottom: a[1], right: b[0], top: b[1] };
    });
  }

  // ---------- Vẽ SVG (preview) ----------
  function arrowHead(from, tip, w) {
    const dx = tip[0] - from[0];
    const dy = tip[1] - from[1];
    const len = Math.hypot(dx, dy) || 0.001;
    const ux = dx / len;
    const uy = dy / len;
    const l = 6 + w * 3;
    const c = Math.cos(0.5) * l;
    const s = Math.sin(0.5) * l;
    return [[tip[0] - ux * c + uy * s, tip[1] - uy * c - ux * s], tip, [tip[0] - ux * c - uy * s, tip[1] - uy * c + ux * s]];
  }
  function cloudPath(pts, w, P) {
    const n = pts.length;
    if (n < 3) return "";
    let area = 0;
    for (let i = 0; i < n; i++) { const a = pts[i]; const b = pts[(i + 1) % n]; area += a[0] * b[1] - b[0] * a[1]; }
    const out = area >= 0 ? 1 : -1;
    const rt = Math.max(4 + w * 2, 5);
    let d = `M${P(pts[0]).join(" ")}`;
    for (let i = 0; i < n; i++) {
      const a = pts[i];
      const b = pts[(i + 1) % n];
      const dx = b[0] - a[0];
      const dy = b[1] - a[1];
      const len = Math.hypot(dx, dy);
      if (len < 0.01) continue;
      const k = Math.max(1, Math.round(len / (rt * 1.6)));
      const nx = (dy / len) * out;
      const ny = (-dx / len) * out;
      for (let j = 0; j < k; j++) {
        const p0 = [a[0] + dx * (j / k), a[1] + dy * (j / k)];
        const p1 = [a[0] + dx * ((j + 1) / k), a[1] + dy * ((j + 1) / k)];
        const seg = len / k;
        const bl = seg * 0.55;
        const c0 = [p0[0] + nx * bl + (dx / len) * seg * 0.05, p0[1] + ny * bl + (dy / len) * seg * 0.05];
        const c1 = [p1[0] + nx * bl - (dx / len) * seg * 0.05, p1[1] + ny * bl - (dy / len) * seg * 0.05];
        d += ` C${P(c0).join(" ")} ${P(c1).join(" ")} ${P(p1).join(" ")}`;
      }
    }
    return d + " Z";
  }
  function stampLabel(st) {
    return st && st.label ? st.label : "";
  }
  // SVG (chuỗi) của 1 item trên trang cao `H` pt.
  function svgOf(it, H) {
    const s = scale();
    const P = (p) => [fmtN(p[0] * s), fmtN((H - p[1]) * s)];
    const col = rgbCss(it.color || [0, 0, 0]);
    const w = Math.max(it.width == null ? 1 : it.width, 0);
    const sw = Math.max(w * s, 0.6);
    const op = it.opacity == null ? 1 : it.opacity;
    const fill = it.fill ? rgbCss(it.fill) : "none";
    const g = (inner) => `<g opacity="${op}">${inner}</g>`;
    const base = `stroke="${col}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round"`;
    const L = it.left * s;
    const T = (H - it.top) * s;
    const W = (it.right - it.left) * s;
    const Hh = (it.top - it.bottom) * s;
    switch (it.kind) {
      case "rect":
      case "square": {
        const i = (w * s) / 2;
        return g(`<rect x="${L + i}" y="${T + i}" width="${Math.max(W - 2 * i, 0)}" height="${Math.max(Hh - 2 * i, 0)}" fill="${fill}" ${w > 0 ? base : 'stroke="none"'}/>`);
      }
      case "oval": {
        const i = (w * s) / 2;
        return g(`<ellipse cx="${L + W / 2}" cy="${T + Hh / 2}" rx="${Math.max(W / 2 - i, 0)}" ry="${Math.max(Hh / 2 - i, 0)}" fill="${fill}" ${w > 0 ? base : 'stroke="none"'}/>`);
      }
      case "line":
      case "arrow": {
        if (!it.points || it.points.length < 2) return "";
        const [a, b] = [it.points[0], it.points[it.points.length - 1]];
        let out = `<line x1="${P(a)[0]}" y1="${P(a)[1]}" x2="${P(b)[0]}" y2="${P(b)[1]}" ${base}/>`;
        const le = it.le || (it.kind === "arrow" ? ["None", "OpenArrow"] : ["None", "None"]);
        const heads = [[le[0], b, a], [le[1], a, b]];
        for (const [e, from, tip] of heads) {
          if (!/Arrow/.test(e || "")) continue;
          const pts = arrowHead(from, tip, w).map((p) => P(p).join(",")).join(" ");
          out += e === "ClosedArrow"
            ? `<polygon points="${pts}" fill="${it.fill ? fill : col}" ${base}/>`
            : `<polyline points="${pts}" fill="none" ${base}/>`;
        }
        return g(out);
      }
      case "polygon":
      case "polyline": {
        if (!it.points || it.points.length < 2) return "";
        const pts = it.points.map((p) => P(p).join(",")).join(" ");
        return it.kind === "polygon"
          ? g(`<polygon points="${pts}" fill="${fill}" ${base}/>`)
          : g(`<polyline points="${pts}" fill="none" ${base}/>`);
      }
      case "cloud":
        if (!it.points || it.points.length < 3) return "";
        return g(`<path d="${cloudPath(it.points, w, P)}" fill="${fill}" ${base}/>`);
      case "ink":
        return g((it.strokes || []).map((st) =>
          st.length === 1
            ? `<circle cx="${P(st[0])[0]}" cy="${P(st[0])[1]}" r="${sw / 2}" fill="${col}"/>`
            : `<polyline points="${st.map((p) => P(p).join(",")).join(" ")}" fill="none" ${base}/>`).join(""));
      case "stamp": {
        const st = it.stamp || {};
        if (st.image) {
          const im = X.imgs.get(st.image);
          if (!im) return g(`<rect x="${L}" y="${T}" width="${W}" height="${Hh}" fill="none" stroke="${col}" stroke-dasharray="4 3"/>`);
          return g(`<image href="${im.dataUrl}" x="${L}" y="${T}" width="${W}" height="${Hh}" preserveAspectRatio="none"/>`);
        }
        if (it.existing) return snapSvg(it, L, T, W, Hh, op);
        const bw = Math.min(Math.max((Hh / s) * 0.06, 1), 4) * s;
        const label = stampLabel(st);
        const sub = st.sub || "";
        const padx = bw + Hh * 0.18;
        const avail = Math.max(W - 2 * padx, 4);
        // Cùng bố cục với engine (annot_ext.rs::text_stamp_ap).
        const lf = Math.min(sub ? Hh * 0.38 : Hh * 0.5, avail / Math.max(label.length * 0.66, 1));
        const sf = Math.min(Hh * 0.19, avail / Math.max(sub.length * 0.56, 1));
        const r = Math.min(Hh * 0.18, 10 * s);
        let out = `<rect x="${L + bw / 2}" y="${T + bw / 2}" width="${W - bw}" height="${Hh - bw}" rx="${r}" fill="none" stroke="${col}" stroke-width="${bw}"/>`;
        const font = `font-family="Arial, 'Segoe UI', sans-serif" font-weight="700" fill="${col}" text-anchor="middle"`;
        if (sub) {
          const gap = Hh * 0.1;
          const total = lf * 0.72 + gap + sf * 0.72;
          const y0 = T + (Hh - total) / 2;
          out += `<text x="${L + W / 2}" y="${y0 + lf * 0.72}" font-size="${lf}" ${font}>${esc(label)}</text>`;
          out += `<text x="${L + W / 2}" y="${y0 + total}" font-size="${sf}" ${font}>${esc(sub)}</text>`;
        } else {
          out += `<text x="${L + W / 2}" y="${T + (Hh + lf * 0.72) / 2}" font-size="${lf}" ${font}>${esc(label)}</text>`;
        }
        return g(out);
      }
      case "highlight":
      case "underline":
      case "strikeout":
      case "squiggly": {
        const qs = it.quads && it.quads.length ? it.quads : [{ left: it.left, bottom: it.bottom, right: it.right, top: it.top }];
        return g(qs.map((q) => {
          const x = q.left * s;
          const y = (H - q.top) * s;
          const qw = (q.right - q.left) * s;
          const qh = (q.top - q.bottom) * s;
          if (it.kind === "highlight") return `<rect x="${x}" y="${y}" width="${qw}" height="${qh}" fill="${col}" style="mix-blend-mode:multiply" opacity=".55"/>`;
          const ly = it.kind === "strikeout" ? y + qh / 2 : y + qh * 0.92;
          return `<line x1="${x}" y1="${ly}" x2="${x + qw}" y2="${ly}" stroke="${col}" stroke-width="${Math.max(qh / 14, 1)}"${it.kind === "squiggly" ? ' stroke-dasharray="3 2"' : ""}/>`;
        }).join(""));
      }
      default:
        return snapSvg(it, L, T, W, Hh, op);
    }
  }
  function snapSvg(it, L, T, W, H, op) {
    const url = X.snaps.get(it.xid);
    if (url) return `<g opacity="${op}"><image href="${url}" x="${L}" y="${T}" width="${W}" height="${H}" preserveAspectRatio="none"/></g>`;
    return `<rect x="${L}" y="${T}" width="${W}" height="${H}" fill="none" stroke="${rgbCss(it.color || [0, 0, 0])}" stroke-dasharray="4 3"/>`;
  }
  // HTML cho loại vẽ bằng DOM (text box / ghi chú có sẵn đã bị sửa).
  function htmlOf(it, H) {
    const s = scale();
    const L = it.left * s;
    const T = (H - it.top) * s;
    if (it.kind === "freetext") {
      return `<div class="ax-ftext" style="left:${L}px;top:${T}px;width:${(it.right - it.left) * s}px;height:${(it.top - it.bottom) * s}px;` +
        `color:${rgbCss(it.color)};font-size:${(it.fontSize || 12) * s}px">${esc(it.contents)}</div>`;
    }
    if (it.kind === "note") {
      return `<div class="ax-noteic" style="left:${L}px;top:${T}px;background:${rgbCss(it.color)}"><i data-icon="note"></i></div>`;
    }
    return "";
  }
  // Vùng bắt chuột: nét thật (hình không tô) hoặc khung bao.
  function hitOf(it, H) {
    const s = scale();
    const P = (p) => [fmtN(p[0] * s), fmtN((H - p[1]) * s)];
    const id = esc(it.xid || it.id);
    const sw = Math.max((it.width || 1) * s, 10);
    const stroke = `data-id="${id}" class="ax-hs" fill="none" stroke="transparent" stroke-width="${sw}" pointer-events="stroke"`;
    const L = it.left * s;
    const T = (H - it.top) * s;
    const W = Math.max((it.right - it.left) * s, 6);
    const Hh = Math.max((it.top - it.bottom) * s, 6);
    const box = `<rect data-id="${id}" class="ax-hs" x="${L}" y="${T}" width="${W}" height="${Hh}" fill="transparent" pointer-events="all"/>`;
    if (it.fill && FILLABLE.includes(it.kind)) return box;
    switch (it.kind) {
      case "rect": return `<rect x="${L}" y="${T}" width="${W}" height="${Hh}" ${stroke}/>`;
      case "oval": return `<ellipse cx="${L + W / 2}" cy="${T + Hh / 2}" rx="${W / 2}" ry="${Hh / 2}" ${stroke}/>`;
      case "line": case "arrow": case "polyline": case "polygon": case "cloud":
        if (!it.points || it.points.length < 2) return box;
        return `<poly${it.kind === "polygon" || it.kind === "cloud" ? "gon" : "line"} points="${it.points.map((p) => P(p).join(",")).join(" ")}" ${stroke}/>`;
      case "ink":
        return (it.strokes || []).map((st) => `<polyline points="${st.map((p) => P(p).join(",")).join(" ")}" ${stroke}/>`).join("");
      default:
        return box;
    }
  }

  // ---------- Vẽ lớp chú thích của trang (bọc drawAnnotsForPage của main.js) ----------
  function drawPage(idx) {
    const slot = state.slots[idx];
    if (!slot) return;
    const layer = slot.querySelector(".annotlayer");
    const p = state.pages[idx];
    if (!layer || !p) return;
    const s = scale();
    const W = p.widthPt * s;
    const H = p.heightPt * s;
    let vis = "";
    let html = "";
    let hits = "";
    const items = [];
    for (const r of state.annotExisting || []) {
      if (r.pageIndex !== idx || r.deleted || r.hiddenFlag) continue;
      items.push(r);
      if (visualDirty(r)) {
        if (r.kind === "freetext" || r.kind === "note") html += htmlOf(r, p.heightPt);
        else vis += svgOf(r, p.heightPt);
      }
    }
    for (const sp of state.annotSpecs) {
      if (sp.pageIndex !== idx || !sp.ext || sp.kind === "reply") continue;
      items.push(sp);
      vis += svgOf(sp, p.heightPt);
    }
    for (const it of items) hits += hitOf(it, p.heightPt);
    if (!items.length && !(X.draft && X.draft.idx === idx) && !(X.ghost && X.ghost.idx === idx)) {
      syncHidden(idx);
      return;
    }
    const wrap = document.createElement("div");
    wrap.className = "ax-wrap";
    wrap.innerHTML =
      `<svg class="ax-svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">${vis}</svg>${html}` +
      `<svg class="ax-hitsvg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">${hits}</svg>` +
      `<svg class="ax-draft" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"></svg>`;
    applyIcons(wrap);
    wrap.querySelectorAll(".ax-hs").forEach((el) => {
      const id = el.dataset.id;
      const realId = isX(id) ? id : Number(id);
      el.addEventListener("mousedown", (ev) => onItemDown(ev, realId, "move"));
      el.addEventListener("dblclick", (ev) => { ev.stopPropagation(); ev.preventDefault(); openEditDialog(realId); });
    });
    // Khung chọn + tay nắm co giãn + nút xoá.
    const sel = findItem(state.selectedId);
    if (sel && sel.pageIndex === idx && isOurs(sel)) {
      const box = document.createElement("div");
      box.className = "ax-selbox";
      Object.assign(box.style, {
        left: sel.left * s - 2 + "px", top: (p.heightPt - sel.top) * s - 2 + "px",
        width: (sel.right - sel.left) * s + 4 + "px", height: (sel.top - sel.bottom) * s + 4 + "px",
      });
      box.addEventListener("mousedown", (ev) => onItemDown(ev, state.selectedId, "move"));
      box.addEventListener("dblclick", (ev) => { ev.stopPropagation(); ev.preventDefault(); openEditDialog(state.selectedId); });
      if (resizable(sel)) {
        for (const h of ["nw", "ne", "sw", "se"]) {
          const hd = document.createElement("div");
          hd.className = "ax-h";
          hd.dataset.h = h;
          hd.addEventListener("mousedown", (ev) => onItemDown(ev, state.selectedId, h));
          box.appendChild(hd);
        }
      }
      const del = document.createElement("div");
      del.className = "a-del";
      del.textContent = "✕";
      del.title = t("viewer.deleteTip");
      del.addEventListener("mousedown", (ev) => ev.stopPropagation());
      del.addEventListener("click", (ev) => { ev.stopPropagation(); deleteSpec(state.selectedId); });
      box.appendChild(del);
      wrap.appendChild(box);
    }
    layer.appendChild(wrap);
    if (X.draft && X.draft.idx === idx) renderDraft();
    else if (X.ghost && X.ghost.idx === idx) renderGhost();
    syncHidden(idx);
  }
  const resizable = (it) => it.kind !== "note";

  // Ẩn bản render gốc của chú thích có sẵn đã sửa/xoá → render lại trang khi cần.
  function hiddenIndices(idx) {
    return (state.annotExisting || []).filter((r) => r.pageIndex === idx && visualDirty(r)).map((r) => r.annotIndex);
  }
  function syncHidden(idx) {
    const slot = state.slots[idx];
    if (!slot || !slot.dataset.renderedZoom) return;
    const key = hiddenIndices(idx).join(",");
    if ((slot.dataset.hideKey || "") !== key) {
      slot.dataset.renderedZoom = "";
      renderSlot(idx);
    }
  }

  // Ảnh cắt từ trang đã render (preview khi dời loại không tự vẽ lại được).
  function captureSnap(r) {
    if (X.snaps.has(r.xid)) return;
    const slot = state.slots[r.pageIndex];
    const img = slot && slot.querySelector("img");
    if (!img || !img.naturalWidth) return;
    const p = state.pages[r.pageIndex];
    const k = img.naturalWidth / p.widthPt;
    const sx = Math.max(0, r.left * k);
    const sy = Math.max(0, (p.heightPt - r.top) * k);
    const sw = Math.max(1, (r.right - r.left) * k);
    const sh = Math.max(1, (r.top - r.bottom) * k);
    try {
      const c = document.createElement("canvas");
      c.width = Math.round(sw);
      c.height = Math.round(sh);
      c.getContext("2d").drawImage(img, sx, sy, sw, sh, 0, 0, c.width, c.height);
      X.snaps.set(r.xid, c.toDataURL("image/png"));
    } catch (_) { /* không cắt được → preview khung nét đứt */ }
  }

  // ---------- Vẽ mới ----------
  function draftSvg() {
    const slot = state.slots[(X.draft || X.ghost).idx];
    if (!slot) return null;
    let svg = slot.querySelector(".ax-draft");
    if (!svg) {
      const layer = slot.querySelector(".annotlayer");
      const p = state.pages[(X.draft || X.ghost).idx];
      const s = scale();
      const wrap = document.createElement("div");
      wrap.className = "ax-wrap";
      wrap.innerHTML = `<svg class="ax-draft" width="${p.widthPt * s}" height="${p.heightPt * s}" viewBox="0 0 ${p.widthPt * s} ${p.heightPt * s}"></svg>`;
      layer.appendChild(wrap);
      svg = wrap.firstChild;
    }
    return svg;
  }
  function draftItem() {
    const d = X.draft;
    const base = { kind: d.kind, color: d.color, fill: d.fill, width: d.width, opacity: d.opacity, stamp: d.stamp };
    if (d.kind === "ink") return Object.assign(base, { strokes: [d.stroke] });
    if (d.kind === "line" || d.kind === "arrow") return Object.assign(base, { points: [[d.a.x, d.a.y], [d.b.x, d.b.y]] });
    if (POLY_TOOLS.includes(d.kind)) {
      const pts = d.pts.concat(d.hover ? [[d.hover.x, d.hover.y]] : []);
      return Object.assign(base, { kind: d.kind === "polyline" ? "polyline" : d.kind === "cloud" && pts.length < 3 ? "polyline" : d.kind, points: pts });
    }
    return Object.assign(base, rectOf(d.a, d.b));
  }
  function renderDraft() {
    const svg = draftSvg();
    if (!svg) return;
    const H = state.pages[X.draft.idx].heightPt;
    let out = svgOf(draftItem(), H);
    if (POLY_TOOLS.includes(X.draft.kind)) {
      const s = scale();
      out += X.draft.pts.map((p) => `<circle cx="${p[0] * s}" cy="${(H - p[1]) * s}" r="3" class="ax-vtx"/>`).join("");
    }
    svg.innerHTML = out;
  }
  function renderGhost() {
    const svg = draftSvg();
    if (!svg || !X.ghost) return;
    const H = state.pages[X.ghost.idx].heightPt;
    svg.innerHTML = `<g opacity=".6">${svgOf(X.ghost.item, H)}</g>`;
  }
  function clearDraftSvg(idx) {
    const slot = state.slots[idx];
    const svg = slot && slot.querySelector(".ax-draft");
    if (svg) svg.innerHTML = "";
  }
  function cancelDraft() {
    if (X.draft) clearDraftSvg(X.draft.idx);
    if (X.ghost) clearDraftSvg(X.ghost.idx);
    X.draft = null;
    X.ghost = null;
  }

  function newSpec(kind, idx, extra) {
    return Object.assign({
      id: annotIdSeq++,
      ext: true,
      kind,
      pageIndex: idx,
      left: 0, bottom: 0, right: 0, top: 0,
      points: [], strokes: [],
      color: [...state.color],
      fill: FILLABLE.includes(kind) ? (X.fill ? [...X.fill] : null) : null,
      width: X.width,
      opacity: X.opacity,
      contents: "",
    }, extra || {});
  }
  function commitSpec(spec) {
    pushUndo();
    state.annotSpecs.push(spec);
    pushRecentColor(state.color);
    afterChange(spec.pageIndex);
  }
  function afterChange(idx) {
    if (idx != null) drawAnnotsForPage(idx);
    updateAnnotCount();
    buildComments();
  }

  function stampSpecFor(preset) {
    if (preset.image) return { name: "FFImage", label: "", sub: null, image: preset.image };
    const label = t(preset.key).toLocaleUpperCase(locale());
    let sub = null;
    if (preset.dynamic) {
      const d = fmtDateObj(new Date());
      sub = X.author ? `${d} · ${X.author}` : d;
    }
    return { name: preset.name, label, sub, image: null };
  }
  function stampDefaultSize(preset) {
    if (preset.image) {
      const im = X.imgs.get(preset.image);
      const ar = im && im.width ? im.height / im.width : 0.5;
      const w = ar > 1 ? 120 / ar : 160;
      return [w, w * ar];
    }
    return preset.dynamic ? [170, 54] : [160, 44];
  }
  function stampItemAt(pt, idx) {
    const [w, h] = stampDefaultSize(X.stamp);
    return {
      kind: "stamp", pageIndex: idx,
      left: pt.x - w / 2, right: pt.x + w / 2, bottom: pt.y - h / 2, top: pt.y + h / 2,
      color: X.stamp.color || [0, 0, 0], opacity: X.opacity, width: 0,
      stamp: stampSpecFor(X.stamp),
    };
  }

  function constrain(a, b, kind, shift) {
    if (!shift) return b;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    if (kind === "line" || kind === "arrow") {
      const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
      const len = Math.hypot(dx, dy);
      return { x: a.x + Math.cos(ang) * len, y: a.y + Math.sin(ang) * len };
    }
    const m = Math.max(Math.abs(dx), Math.abs(dy));
    return { x: a.x + Math.sign(dx || 1) * m, y: a.y + Math.sign(dy || 1) * m };
  }

  function onDown(e) {
    if (e.button !== 0 || !DRAW_TOOLS.includes(state.tool)) return;
    const slot = e.target.closest && e.target.closest(".page-slot");
    if (!slot) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    blurInputs();
    finishEditing();
    closeNotePopup();
    closeColorPopover();
    const idx = Number(slot.dataset.index);
    const pt = ptOf(slot, e);
    const tool = state.tool;
    const kind = TOOL_KIND[tool] || tool;
    if (POLY_TOOLS.includes(tool)) {
      if (X.draft && X.draft.idx !== idx) return; // đa giác không vắt qua 2 trang
      if (!X.draft) {
        X.draft = { kind, idx, pts: [], hover: null, color: [...state.color], fill: X.fill, width: X.width, opacity: X.opacity };
      }
      const pts = X.draft.pts;
      const s = scale();
      // Bấm lại vào đỉnh đầu → khép hình.
      if (pts.length >= 3 && Math.hypot((pts[0][0] - pt.x) * s, (pts[0][1] - pt.y) * s) < 7) { finishPoly(); return; }
      const last = pts[pts.length - 1];
      if (!last || Math.hypot((last[0] - pt.x) * s, (last[1] - pt.y) * s) > 2) pts.push([pt.x, pt.y]);
      renderDraft();
      return;
    }
    X.ghost = null;
    X.draft = {
      kind, idx, a: pt, b: pt, stroke: [[pt.x, pt.y]], moved: false,
      color: kind === "stamp" ? X.stamp.color || [0, 0, 0] : [...state.color],
      fill: FILLABLE.includes(kind) ? X.fill : null,
      width: kind === "stamp" ? 0 : X.width, opacity: X.opacity,
      stamp: kind === "stamp" ? stampSpecFor(X.stamp) : null,
    };
    renderDraft();
  }
  function onMove(e) {
    if (X.drag) { onDragMove(e); return; }
    const tool = state.tool;
    if (!DRAW_TOOLS.includes(tool)) return;
    const slot = X.draft ? state.slots[X.draft.idx] : e.target.closest && e.target.closest(".page-slot");
    if (!slot) {
      if (X.ghost) { clearDraftSvg(X.ghost.idx); X.ghost = null; }
      return;
    }
    const pt = ptOf(slot, e);
    const d = X.draft;
    if (!d) {
      if (tool === "stamp") {
        const idx = Number(slot.dataset.index);
        if (X.ghost && X.ghost.idx !== idx) clearDraftSvg(X.ghost.idx);
        X.ghost = { idx, item: stampItemAt(pt, idx) };
        renderGhost();
      }
      return;
    }
    if (POLY_TOOLS.includes(d.kind)) { d.hover = pt; renderDraft(); return; }
    if (!(e.buttons & 1)) return;
    if (d.kind === "ink") {
      const last = d.stroke[d.stroke.length - 1];
      const s = scale();
      if (Math.hypot((last[0] - pt.x) * s, (last[1] - pt.y) * s) < 1.5) return;
      d.stroke.push([pt.x, pt.y]);
    } else {
      d.b = constrain(d.a, pt, d.kind, e.shiftKey);
    }
    d.moved = true;
    renderDraft();
  }
  function onUp() {
    if (X.drag) { endDrag(); return; }
    const d = X.draft;
    if (!d || POLY_TOOLS.includes(d.kind)) return;
    X.draft = null;
    clearDraftSvg(d.idx);
    const s = scale();
    const tiny = Math.abs(d.b.x - d.a.x) * s < 4 && Math.abs(d.b.y - d.a.y) * s < 4;
    if (d.kind === "ink") {
      const now = Date.now();
      const prev = X.lastInk && findItem(X.lastInk.id);
      // Nét vẽ liền nhau (≤2s, cùng trang, cùng thuộc tính) gom vào 1 Ink như Foxit.
      if (prev && X.lastInk.idx === d.idx && now - X.lastInk.t < 2000 && sameArr(prev.color, d.color) && prev.width === d.width && prev.opacity === d.opacity) {
        pushUndo();
        prev.strokes.push(d.stroke);
        fitBounds(prev);
        X.lastInk.t = now;
        afterChange(d.idx);
        return;
      }
      const sp = newSpec("ink", d.idx, { strokes: [d.stroke], color: d.color, width: d.width, opacity: d.opacity });
      fitBounds(sp);
      commitSpec(sp);
      X.lastInk = { id: sp.id, idx: d.idx, t: now };
      return;
    }
    if (d.kind === "stamp") {
      const it = tiny ? stampItemAt(d.a, d.idx) : Object.assign(stampItemAt(d.a, d.idx), rectOf(d.a, d.b));
      commitSpec(newSpec("stamp", d.idx, {
        left: it.left, right: it.right, bottom: it.bottom, top: it.top,
        color: it.color, width: 0, fill: null, stamp: it.stamp,
      }));
      return;
    }
    if (tiny) return;
    if (d.kind === "line" || d.kind === "arrow") {
      const sp = newSpec(d.kind, d.idx, { points: [[d.a.x, d.a.y], [d.b.x, d.b.y]], le: d.kind === "arrow" ? ["None", "OpenArrow"] : ["None", "None"] });
      fitBounds(sp);
      commitSpec(sp);
      return;
    }
    commitSpec(newSpec(d.kind, d.idx, rectOf(d.a, d.b)));
  }
  function finishPoly() {
    const d = X.draft;
    if (!d || !POLY_TOOLS.includes(d.kind)) return;
    // Bỏ đỉnh trùng do double-click (2 lần mousedown cùng chỗ).
    const s = scale();
    const pts = [];
    for (const p of d.pts) {
      const last = pts[pts.length - 1];
      if (!last || Math.hypot((last[0] - p[0]) * s, (last[1] - p[1]) * s) > 3) pts.push(p);
    }
    X.draft = null;
    clearDraftSvg(d.idx);
    const need = d.kind === "polyline" ? 2 : 3;
    if (pts.length < need) { $("status").textContent = t("annx.polyNeed", { n: need }); return; }
    const sp = newSpec(d.kind, d.idx, { points: pts, color: d.color, width: d.width, opacity: d.opacity, fill: d.kind === "polyline" ? null : d.fill });
    fitBounds(sp);
    commitSpec(sp);
  }

  // ---------- Chọn, dời, co giãn ----------
  function onItemDown(ev, id, mode) {
    if (ev.button !== 0 || state.tool) return;
    ev.stopPropagation();
    ev.preventDefault();
    blurInputs();
    const it = findItem(id);
    if (!it) return;
    if (state.selectedId !== id) selectAnnot(id);
    X.drag = {
      id, mode, sx: ev.clientX, sy: ev.clientY, started: false,
      base: clone({ left: it.left, bottom: it.bottom, right: it.right, top: it.top, points: it.points || null, strokes: it.strokes || null, quads: it.quads || null }),
    };
  }
  function onDragMove(e) {
    const d = X.drag;
    const it = findItem(d.id);
    if (!it) { X.drag = null; return; }
    const s = scale();
    const dxp = e.clientX - d.sx;
    const dyp = e.clientY - d.sy;
    if (!d.started) {
      if (Math.hypot(dxp, dyp) < 3) return;
      d.started = true;
      pushUndo();
      if (it.existing) captureSnap(it);
    }
    const dx = dxp / s;
    const dy = -dyp / s;
    const b = d.base;
    const r = { left: b.left, bottom: b.bottom, right: b.right, top: b.top };
    const MIN = 4;
    if (d.mode === "move") { r.left += dx; r.right += dx; r.bottom += dy; r.top += dy; }
    if (d.mode.includes("w")) r.left = Math.min(b.left + dx, b.right - MIN);
    if (d.mode.includes("e")) r.right = Math.max(b.right + dx, b.left + MIN);
    if (d.mode.includes("n")) r.top = Math.max(b.top + dy, b.bottom + MIN);
    if (d.mode.includes("s")) r.bottom = Math.min(b.bottom + dy, b.top - MIN);
    mapGeom(it, b, r);
    drawAnnotsForPage(it.pageIndex);
  }
  function endDrag() {
    const d = X.drag;
    X.drag = null;
    if (d && d.started) {
      updateAnnotCount();
      buildComments();
    }
  }

  // ---------- Thuộc tính (màu tô / độ dày / độ mờ) ----------
  function loadProps() {
    try {
      const p = JSON.parse(lsGet(LS_PROPS) || "{}");
      if (typeof p.width === "number") X.width = p.width;
      if (typeof p.opacity === "number") X.opacity = p.opacity;
      if (Array.isArray(p.fill) || p.fill === null) X.fill = p.fill;
    } catch (_) { /* mặc định */ }
  }
  function saveProps() { lsSet(LS_PROPS, JSON.stringify({ width: X.width, opacity: X.opacity, fill: X.fill })); }
  function setSelectValue(sel, v) {
    const val = String(v);
    if (![...sel.options].some((o) => o.value === val)) {
      const o = document.createElement("option");
      o.value = val;
      o.textContent = sel.id === "axOpacity" ? Math.round(v * 100) + "%" : fmtN(v) + " pt";
      sel.appendChild(o);
    }
    sel.value = val;
  }
  function refreshProps() {
    const it = findItem(state.selectedId);
    const src = isOurs(it) ? it : null;
    setSelectValue($("axWidth"), src && src.width != null ? src.width : X.width);
    setSelectValue($("axOpacity"), src && src.opacity != null ? src.opacity : X.opacity);
    const fill = src ? src.fill : X.fill;
    const sw = $("axFillSw");
    sw.classList.toggle("none", !fill);
    sw.style.background = fill ? rgbCss(fill) : "";
  }
  // Đổi thuộc tính: áp cho chú thích đang chọn (nếu có) + nhớ làm mặc định.
  function setSelectedProp(props) {
    const it = findItem(state.selectedId);
    if (!isOurs(it)) return false;
    pushUndo();
    if (props.color && it.existing) captureSnap(it);
    for (const [k, v] of Object.entries(props)) {
      if (k === "fill" && !FILLABLE.includes(it.kind)) continue;
      if (k === "width" && (it.kind === "stamp" || it.kind === "note" || it.kind === "freetext")) continue;
      it[k] = Array.isArray(v) ? v.slice() : v;
    }
    if (it.ext && (props.width != null)) fitBounds(it);
    afterChange(it.pageIndex);
    refreshProps();
    return true;
  }
  function onWidth() {
    const v = Number($("axWidth").value);
    X.width = v;
    saveProps();
    setSelectedProp({ width: v });
  }
  function onOpacity() {
    const v = Number($("axOpacity").value);
    X.opacity = v;
    saveProps();
    setSelectedProp({ opacity: v });
  }
  function onFillBtn() {
    const cur = (() => { const it = findItem(state.selectedId); return isOurs(it) ? it.fill : X.fill; })();
    const apply = (rgb) => {
      X.fill = rgb;
      saveProps();
      if (!setSelectedProp({ fill: rgb })) refreshProps();
    };
    openColorPopover($("axFillBtn"), cur || [255, 255, 255], (rgb) => apply(rgb));
    // Thêm lựa chọn "Không tô" vào popover màu dùng chung.
    const pop = document.querySelector(".colorpop");
    if (pop) {
      const none = document.createElement("button");
      none.className = "ax-nofill";
      none.innerHTML = `<span class="ax-fillsw none"></span>${esc(t("annx.noFill"))}`;
      none.addEventListener("click", () => { apply(null); closeColorPopover(); });
      pop.insertBefore(none, pop.firstChild);
    }
  }

  // ---------- Con dấu ----------
  let stampMenu = null;
  function closeStampMenu() {
    if (stampMenu) { stampMenu.remove(); stampMenu = null; }
  }
  function chip(p) {
    if (p.image) {
      const im = X.imgs.get(p.image);
      return im ? `<img class="ax-chipimg" src="${im.dataUrl}" alt="">` : "";
    }
    return `<span class="ax-chip" style="color:${rgbCss(p.color)};border-color:${rgbCss(p.color)}">${esc(t(p.key).toLocaleUpperCase(locale()))}</span>`;
  }
  function openStampMenu() {
    if (stampMenu) { closeStampMenu(); return; }
    const r = $("axStampBtn").getBoundingClientRect();
    const m = document.createElement("div");
    m.className = "menu mini-menu ax-stampmenu";
    m.setAttribute("role", "menu");
    const item = (p, i, group) => `<button class="mitem${X.stamp === p ? " checked" : ""}" data-g="${group}" data-i="${i}"><span class="mcheck"></span>${chip(p)}${p.dynamic ? `<span class="ax-dyn">${esc(t("annx.st.withDate"))}</span>` : ""}</button>`;
    const customs = [...X.imgs.keys()].map((path) => ({ image: path, color: [0, 0, 0] }));
    m.innerHTML =
      `<div class="ax-mhead">${esc(t("annx.st.standard"))}</div>` +
      STAMPS.map((p, i) => item(p, i, "s")).join("") +
      `<div class="msep"></div><div class="ax-mhead">${esc(t("annx.st.dynamic"))}</div>` +
      DYN_STAMPS.map((p, i) => item(p, i, "d")).join("") +
      (customs.length ? `<div class="msep"></div><div class="ax-mhead">${esc(t("annx.st.custom"))}</div>` + customs.map((p, i) => item(p, i, "c")).join("") : "") +
      `<div class="msep"></div><button class="mitem" data-g="pick"><span class="mcheck"></span><i data-icon="file-image"></i><span>${esc(t("annx.st.pickImage"))}</span></button>`;
    applyIcons(m);
    document.body.appendChild(m);
    m.style.left = Math.min(r.left, window.innerWidth - m.offsetWidth - 8) + "px";
    m.style.top = r.bottom + 4 + "px";
    stampMenu = m;
    m.addEventListener("click", async (ev) => {
      const b = ev.target.closest(".mitem");
      if (!b) return;
      const g = b.dataset.g;
      closeStampMenu();
      if (g === "pick") { await pickStampImage(); return; }
      const list = g === "s" ? STAMPS : g === "d" ? DYN_STAMPS : customs;
      const p = list[Number(b.dataset.i)];
      if (!p) return;
      X.stamp = p;
      armStamp();
    });
  }
  async function pickStampImage() {
    try {
      const path = await invoke("annot_pick_stamp_image");
      if (!path) return;
      $("status").textContent = t("common.loading");
      const pv = await invoke("annot_image_preview", { path });
      X.imgs.set(path, pv);
      X.stamp = { image: path, color: [0, 0, 0] };
      $("status").textContent = "";
      armStamp();
    } catch (e) {
      $("status").textContent = t("annx.imageErr", { e });
    }
  }
  function armStamp() {
    if (state.tool !== "stamp") setTool("stamp");
    else updateHint();
  }

  // ---------- Bọc các hàm của main.js ----------
  const origSetTool = setTool;
  setTool = function (tool) {
    const prev = state.tool;
    origSetTool(tool);
    if (state.tool !== prev) cancelDraft();
    $("axStampBtn").classList.toggle("active", state.tool === "stamp");
    updateHint();
  };
  function updateHint() {
    const tool = state.tool;
    if (!DRAW_TOOLS.includes(tool)) return;
    const key = POLY_TOOLS.includes(tool) ? "annx.hint.poly" : tool === "ink" ? "annx.hint.ink"
      : tool === "stamp" ? "annx.hint.stamp" : tool === "line" || tool === "arrow" ? "annx.hint.line" : "annx.hint.shape";
    $("annotHint").textContent = t(key);
  }

  const origDraw = drawAnnotsForPage;
  drawAnnotsForPage = function (idx) {
    origDraw(idx);
    drawPage(idx);
  };

  const origDelete = deleteSpec;
  deleteSpec = function (id) {
    if (!isX(id)) {
      if (X.lastInk && X.lastInk.id === id) X.lastInk = null;
      return origDelete(id);
    }
    const r = findItem(id);
    if (!r || r.deleted) return;
    pushUndo();
    r.deleted = true;
    if (state.selectedId === id) state.selectedId = null;
    afterChange(r.pageIndex);
  };

  const origSelect = selectAnnot;
  selectAnnot = function (id) {
    origSelect(id);
    refreshProps();
    markSelectedInList();
  };
  const origDeselect = deselectAnnot;
  deselectAnnot = function () {
    origDeselect();
    refreshProps();
    markSelectedInList();
  };

  updateAnnotCount = function () {
    const n = pendingCount();
    $("annotCount").textContent = n;
    $("saveAnnots").disabled = n === 0;
  };

  // ---------- Danh sách Comments (sidebar) ----------
  function kindLabel(it) {
    const k = it.kind === "square" ? "rect" : it.kind;
    const key = "annx.kind." + k;
    const s = t(key);
    return s === key ? (it.subtype || k) : s;
  }
  function itemId(it) { return it.xid || it.id; }
  function markSelectedInList() {
    document.querySelectorAll("#comments .ax-citem").forEach((el) => {
      el.classList.toggle("selected", String(state.selectedId) === el.dataset.id);
    });
  }
  buildComments = function () {
    const box = $("comments");
    if (!box) return;
    const items = allItems().filter((it) => it.kind !== "reply" && !(it.existing && it.irt && findItem(it.irt)));
    // Trả lời (có sẵn + chưa lưu) gom dưới chú thích cha.
    const replies = new Map();
    for (const r of (state.annotExisting || []).concat(state.annotSpecs.filter((s) => s.kind === "reply"))) {
      if (r.deleted || !r.irt) continue;
      if (!replies.has(r.irt)) replies.set(r.irt, []);
      replies.get(r.irt).push(r);
    }
    items.sort((a, b) => a.pageIndex - b.pageIndex || (b.top - a.top) || (a.left - b.left));
    const head =
      `<div class="ax-chead"><span class="ax-ctitle">${esc(t("annx.listTitle", { n: items.length }))}</span>` +
      `<button class="ax-author" id="axAuthorBtn" title="${esc(t("annx.authorTip"))}"><i data-icon="id-card"></i><span>${esc(X.author || t("annx.noAuthor"))}</span></button></div>`;
    if (!items.length) {
      box.innerHTML = head + `<div class="empty">${esc(t("annx.emptyList"))}</div>`;
    } else {
      let html = head;
      let page = -1;
      for (const it of items) {
        if (it.pageIndex !== page) {
          page = it.pageIndex;
          html += `<div class="ax-cpage">${esc(t("annx.pageN", { n: page + 1 }))}</div>`;
        }
        const id = itemId(it);
        const badge = !it.existing ? `<span class="ax-badge new">${esc(t("annx.badgeNew"))}</span>`
          : updateOf(it) ? `<span class="ax-badge mod">${esc(t("annx.badgeMod"))}</span>` : "";
        const author = it.existing ? it.author : X.author;
        const date = it.existing ? fmtDate(it.modified || it.created) : "";
        const meta = [author, date].filter(Boolean).map(esc).join(" · ");
        const text = it.contents || (it.kind === "stamp" && it.stamp ? stampLabel(it.stamp) : it.subject || "");
        const reps = (replies.get(it.xid) || []).map((r) =>
          `<div class="ax-reply"><i data-icon="reply"></i><b>${esc((r.existing ? r.author : X.author) || t("annx.noAuthor"))}</b>` +
          (r.existing ? (r.modified ? ` · ${esc(fmtDate(r.modified))}` : "") : ` <span class="ax-badge new">${esc(t("annx.badgeNew"))}</span>`) +
          `<button class="ax-cbtn danger ax-rdel" data-act="delreply" data-rid="${esc(itemId(r))}" title="${esc(t("common.delete"))}"><i data-icon="trash"></i></button>` +
          `<div>${esc(r.contents)}</div></div>`).join("");
        const replyBtn = it.existing ? `<button class="ax-cbtn" data-act="reply" title="${esc(t("annx.reply"))}"><i data-icon="reply"></i></button>` : "";
        html +=
          `<div class="citem ax-citem${String(state.selectedId) === String(id) ? " selected" : ""}" data-id="${esc(id)}" tabindex="0">` +
          `<div class="ax-crow"><span class="csw" style="background:${rgbCss(it.color || [0, 0, 0])}"></span>` +
          `<i data-icon="${KIND_ICON[it.kind] || "comments"}"></i><span class="ckind">${esc(kindLabel(it))}</span>${badge}` +
          `<span class="ax-cact">${replyBtn}<button class="ax-cbtn" data-act="edit" title="${esc(t("annx.edit"))}"><i data-icon="pencil-edit"></i></button>` +
          `<button class="ax-cbtn danger" data-act="del" title="${esc(t("common.delete"))}"><i data-icon="trash"></i></button></span></div>` +
          (meta ? `<div class="ax-cmeta">${meta}</div>` : "") +
          (text ? `<div class="ctxt">${esc(text)}</div>` : "") + reps + `</div>`;
      }
      box.innerHTML = html;
    }
    applyIcons(box);
    const ab = box.querySelector("#axAuthorBtn");
    if (ab) ab.addEventListener("click", openAuthorDialog);
    box.querySelectorAll(".ax-citem").forEach((el) => {
      const raw = el.dataset.id;
      const id = isX(raw) ? raw : Number(raw);
      el.addEventListener("click", (ev) => {
        const act = ev.target.closest("[data-act]");
        if (act && act.dataset.act === "del") { deleteSpec(id); return; }
        if (act && act.dataset.act === "edit") { openEditDialog(id); return; }
        if (act && act.dataset.act === "reply") { openReplyDialog(id); return; }
        if (act && act.dataset.act === "delreply") {
          const rid = act.dataset.rid;
          deleteSpec(isX(rid) ? rid : Number(rid));
          return;
        }
        focusItem(id);
      });
      el.addEventListener("dblclick", (ev) => { if (!ev.target.closest("[data-act]")) openEditDialog(id); });
      el.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter") { ev.preventDefault(); openEditDialog(id); }
      });
    });
  };
  function focusItem(id) {
    const it = findItem(id);
    if (!it) return;
    if (state.tool) setTool(state.tool); // về chế độ chọn
    // Cuộn tới đúng vị trí chú thích trên trang (không chỉ đầu trang).
    const slot = state.slots[it.pageIndex];
    const vp = $("viewport");
    if (slot && vp && !state.editMode) {
      const y = vp.scrollTop + slot.getBoundingClientRect().top - vp.getBoundingClientRect().top +
        (state.pages[it.pageIndex].heightPt - it.top) * scale() - 80;
      vp.scrollTo({ top: Math.max(0, y), behavior: "smooth" });
    } else {
      goToPage(it.pageIndex);
    }
    selectAnnot(id);
    if (!isOurs(it) && it.kind === "note") setTimeout(() => openNotePopup(it), 350);
  }

  // ---------- Hộp thoại sửa thuộc tính ----------
  function openEditDialog(id) {
    const it = findItem(id);
    if (!it) return;
    if (!isOurs(it) && it.kind === "freetext") { editTextBox(it); return; }
    const shape = isOurs(it) && !["stamp", "note", "freetext", "highlight", "underline", "strikeout", "squiggly", "other"].includes(it.kind);
    const fillable = isOurs(it) && FILLABLE.includes(it.kind);
    let color = (it.color || [0, 0, 0]).slice();
    let fill = it.fill ? it.fill.slice() : null;
    const opt = (vals, cur, fmt) => {
      const list = vals.includes(cur) ? vals : vals.concat([cur]).sort((a, b) => a - b);
      return list.map((v) => `<option value="${v}"${v === cur ? " selected" : ""}>${fmt(v)}</option>`).join("");
    };
    const box = openModal(esc(t("annx.editTitle", { kind: kindLabel(it) })), `
      <label for="axEdText">${esc(t("annx.contents"))}</label>
      <textarea id="axEdText" rows="4">${esc(it.contents || "")}</textarea>
      ${it.existing ? `<label for="axEdAuthor">${esc(t("annx.author"))}</label><input type="text" id="axEdAuthor" value="${esc(it.author || "")}">` : ""}
      <div class="row ax-edrow">
        <button id="axEdColor" class="ax-edsw"><span class="sw" id="axEdColorSw"></span>${esc(t("common.color"))}</button>
        ${fillable ? `<button id="axEdFill" class="ax-edsw"><span class="sw ax-fillsw" id="axEdFillSw"></span>${esc(t("annx.fill"))}</button>` : ""}
        ${shape ? `<label>${esc(t("annx.width"))} <select id="axEdWidth">${opt([0.5, 1, 2, 3, 4, 6, 8, 12], it.width, (v) => v + " pt")}</select></label>` : ""}
        ${isOurs(it) ? `<label>${esc(t("annx.opacity"))} <select id="axEdOpacity">${opt([0.25, 0.5, 0.75, 1], it.opacity == null ? 1 : it.opacity, (v) => Math.round(v * 100) + "%")}</select></label>` : ""}
      </div>
      ${it.existing ? `<p class="muted">${esc([t("annx.pageN", { n: it.pageIndex + 1 }), it.created ? t("annx.created", { d: fmtDate(it.created) }) : "", it.modified ? t("annx.modified", { d: fmtDate(it.modified) }) : ""].filter(Boolean).join(" · "))}</p>` : ""}
      <div class="foot"><button id="axEdCancel">${esc(t("common.cancel"))}</button><button id="axEdOk" class="primary">${esc(t("common.ok"))}</button></div>
    `);
    // Popover màu nằm trên hộp thoại (z-index cao hơn modal).
    const topPop = () => { const pop = document.querySelector(".colorpop"); if (pop) pop.classList.add("ax-top"); return pop; };
    const paint = () => {
      box.querySelector("#axEdColorSw").style.background = rgbCss(color);
      const fs = box.querySelector("#axEdFillSw");
      if (fs) { fs.classList.toggle("none", !fill); fs.style.background = fill ? rgbCss(fill) : ""; }
    };
    paint();
    box.querySelector("#axEdColor").addEventListener("click", (e) => {
      openColorPopover(e.currentTarget, color, (rgb) => { color = rgb; paint(); });
      topPop();
    });
    const fb = box.querySelector("#axEdFill");
    if (fb) {
      fb.addEventListener("click", (e) => {
        openColorPopover(e.currentTarget, fill || [255, 255, 255], (rgb) => { fill = rgb; paint(); });
        const pop = topPop();
        if (pop) {
          const none = document.createElement("button");
          none.className = "ax-nofill";
          none.innerHTML = `<span class="ax-fillsw none"></span>${esc(t("annx.noFill"))}`;
          none.addEventListener("click", () => { fill = null; paint(); closeColorPopover(); });
          pop.insertBefore(none, pop.firstChild);
        }
      });
    }
    box.querySelector("#axEdCancel").addEventListener("click", () => { closeColorPopover(); closeModal(); });
    box.querySelector("#axEdOk").addEventListener("click", () => {
      const next = { contents: box.querySelector("#axEdText").value, color };
      const au = box.querySelector("#axEdAuthor");
      if (au) next.author = au.value.trim();
      if (fillable) next.fill = fill;
      const w = box.querySelector("#axEdWidth");
      if (w) next.width = Number(w.value);
      const o = box.querySelector("#axEdOpacity");
      if (o) next.opacity = Number(o.value);
      closeColorPopover();
      closeModal();
      const changed = Object.keys(next).some((k) => JSON.stringify(next[k]) !== JSON.stringify(it[k] == null ? (k === "contents" ? "" : it[k]) : it[k]));
      if (!changed) return;
      pushUndo();
      if (it.existing && (next.color || next.opacity != null)) captureSnap(it);
      Object.assign(it, next);
      if (it.ext && next.width != null) fitBounds(it);
      afterChange(it.pageIndex);
      refreshProps();
    });
  }

  // ---------- Trả lời ----------
  function openReplyDialog(id) {
    const it = findItem(id);
    if (!it || !it.existing) return;
    const box = openModal(esc(t("annx.replyTitle")), `
      ${it.contents ? `<p class="muted">${esc((it.author ? it.author + ": " : "") + it.contents)}</p>` : ""}
      <label for="axReplyText">${esc(t("annx.replyAs", { name: X.author || t("annx.noAuthor") }))}</label>
      <textarea id="axReplyText" rows="4"></textarea>
      <div class="foot"><button id="axRpCancel">${esc(t("common.cancel"))}</button><button id="axRpOk" class="primary">${esc(t("annx.reply"))}</button></div>
    `);
    const ta = box.querySelector("#axReplyText");
    const ok = () => {
      const text = ta.value.trim();
      if (!text) { ta.focus(); return; }
      closeModal();
      pushUndo();
      state.annotSpecs.push({
        id: annotIdSeq++, ext: true, kind: "reply", pageIndex: it.pageIndex, irt: it.xid,
        left: it.left, bottom: it.bottom, right: it.right, top: it.top, color: it.color, contents: text,
      });
      afterChange(null);
    };
    box.querySelector("#axRpOk").addEventListener("click", ok);
    box.querySelector("#axRpCancel").addEventListener("click", closeModal);
    ta.addEventListener("keydown", (e) => { if (e.key === "Enter" && e.ctrlKey) { e.preventDefault(); ok(); } });
  }

  // ---------- Tác giả ----------
  function openAuthorDialog() {
    const box = openModal(esc(t("annx.authorTitle")), `
      <p class="muted">${esc(t("annx.authorIntro"))}</p>
      <label for="axAuthorIn">${esc(t("annx.author"))}</label>
      <input type="text" id="axAuthorIn" value="${esc(X.author)}" autocomplete="name">
      <div class="foot"><button id="axAuCancel">${esc(t("common.cancel"))}</button><button id="axAuOk" class="primary">${esc(t("common.ok"))}</button></div>
    `);
    const inp = box.querySelector("#axAuthorIn");
    const ok = () => {
      X.author = inp.value.trim();
      lsSet(LS_AUTHOR, X.author);
      closeModal();
      buildComments();
    };
    box.querySelector("#axAuOk").addEventListener("click", ok);
    box.querySelector("#axAuCancel").addEventListener("click", closeModal);
    inp.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); ok(); } });
  }
  async function initAuthor() {
    const saved = lsGet(LS_AUTHOR);
    if (saved != null) { X.author = saved; return; }
    try {
      X.author = (await invoke("annot_default_author")) || "";
      lsSet(LS_AUTHOR, X.author);
    } catch (_) { /* không có tên → để trống */ }
    buildComments();
  }

  // ---------- Nạp chú thích có sẵn / Lưu ----------
  function onDocLoaded() {
    state.annotExisting = [];
    X.orig.clear();
    X.snaps.clear();
    X.draft = null;
    X.ghost = null;
    X.lastInk = null;
    const seq = ++X.loadSeq;
    const path = state.path;
    invoke("annot_list_detailed", { path })
      .then((list) => {
        if (seq !== X.loadSeq || state.path !== path) return;
        state.annotExisting = list.map(recFromDetail);
        for (const r of state.annotExisting) X.orig.set(r.xid, clone(r));
        redrawAllAnnotPages();
        updateAnnotCount();
        buildComments();
      })
      .catch((e) => { $("status").textContent = t("annx.listErr", { e }); });
  }

  function markupDto(s) {
    return {
      kind: s.kind, pageIndex: s.pageIndex,
      left: s.left, bottom: s.bottom, right: s.right, top: s.top,
      quads: s.quads || [],
      color: [s.color[0], s.color[1], s.color[2], 255],
      contents: s.contents || null,
      fontSize: s.fontSize || 14, bold: !!s.bold, italic: !!s.italic, underline: !!s.underline,
    };
  }
  function shapeDto(s) {
    return {
      kind: s.kind, pageIndex: s.pageIndex,
      left: s.left, bottom: s.bottom, right: s.right, top: s.top,
      points: s.points || [], strokes: s.strokes || [],
      color: s.color.slice(0, 3), fill: s.fill || null,
      width: s.width == null ? 1 : s.width, opacity: s.opacity == null ? 1 : s.opacity,
      contents: s.contents || null,
      stamp: s.stamp ? { name: s.stamp.name || "", label: s.stamp.label || "", sub: s.stamp.sub || null, image: s.stamp.image || null } : null,
    };
  }
  async function save() {
    finishEditing();
    cancelDraft();
    const specs = state.annotSpecs.filter((s) => !s.ext).map(markupDto);
    const shapes = state.annotSpecs.filter((s) => s.ext && s.kind !== "reply").map(shapeDto);
    const replies = [];
    for (const s of state.annotSpecs) {
      if (s.kind !== "reply") continue;
      const p = findItem(s.irt);
      if (p && !p.deleted) replies.push({ pageIndex: p.pageIndex, annotIndex: p.annotIndex, nm: p.nm, contents: s.contents });
    }
    const updates = [];
    const deletes = [];
    for (const r of state.annotExisting || []) {
      if (r.deleted) deletes.push({ pageIndex: r.pageIndex, annotIndex: r.annotIndex, nm: r.nm });
      else { const u = updateOf(r); if (u) updates.push(u); }
    }
    const n = specs.length + shapes.length + updates.length + deletes.length + replies.length;
    if (!n) return;
    const out = await invoke("pick_save_pdf");
    if (!out) return;
    $("status").textContent = t("annx.saving");
    try {
      await invoke("annot_save", {
        input: state.path, output: out, specs, shapes, updates, deletes, replies,
        author: X.author || null, date: pdfDateNow(),
      });
      state.annotSpecs = [];
      state.annotExisting = [];
      X.orig.clear();
      updateAnnotCount();
      $("status").textContent = t("annot.saved", { n, file: shortName(out) });
      loadDocument(out);
    } catch (e) {
      $("status").textContent = t("annot.saveErr", { e });
    }
  }

  // ---------- Sự kiện ----------
  const pagesEl = $("pages");
  pagesEl.addEventListener("mousedown", onDown, true);
  pagesEl.addEventListener("dblclick", (e) => {
    if (X.draft && POLY_TOOLS.includes(X.draft.kind)) {
      e.preventDefault();
      e.stopImmediatePropagation();
      finishPoly();
    }
  }, true);
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
  window.addEventListener("keydown", (e) => {
    if (!X.draft || !POLY_TOOLS.includes(X.draft.kind)) return;
    if (e.key === "Enter") { e.preventDefault(); e.stopImmediatePropagation(); finishPoly(); }
    else if (e.key === "Backspace" || e.key === "Delete") {
      e.preventDefault();
      e.stopImmediatePropagation();
      X.draft.pts.pop();
      if (!X.draft.pts.length) cancelDraft(); else renderDraft();
    }
  }, true);
  document.addEventListener("mousedown", (e) => {
    if (stampMenu && !stampMenu.contains(e.target) && !e.target.closest("#axStampBtn")) closeStampMenu();
  });
  window.addEventListener("keydown", (e) => { if (e.key === "Escape") closeStampMenu(); });
  $("axStampBtn").addEventListener("click", openStampMenu);
  $("axWidth").addEventListener("change", onWidth);
  $("axOpacity").addEventListener("change", onOpacity);
  $("axFillBtn").addEventListener("click", onFillBtn);
  document.addEventListener("langchange", () => { buildComments(); updateHint(); });

  loadProps();
  refreshProps();
  initAuthor();

  window.AnnotExt = { save, onDocLoaded, hiddenIndices, setSelectedProp };
  // Tài liệu đầu tiên có thể đã mở trước khi tệp này nạp.
  if (state.path) onDocLoaded();
  updateAnnotCount();
  buildComments();
})();
