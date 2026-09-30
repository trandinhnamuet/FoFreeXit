// Kiểm tra i18n: tập key vi/en bằng nhau, mọi key được dùng đều có, main.js không còn chuỗi tiếng Việt.
// Chạy: node scripts/check-i18n.mjs   (exit 1 nếu có lỗi)
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SRC = path.join(ROOT, "app", "src");
const rel = (f) => path.relative(ROOT, f).replace(/\\/g, "/");
const read = (f) => fs.readFileSync(f, "utf8");
const errors = [];

// ---- 1. Nạp từ điển trong sandbox (stub I18N.add + DOM tối thiểu) ----
const dict = { vi: {}, en: {} };
const defined = { vi: new Map(), en: new Map() }; // key → file đầu tiên định nghĩa
const dictFiles = [
  ...fs.readdirSync(path.join(SRC, "i18n")).filter((f) => f.endsWith(".js")).sort().map((f) => path.join(SRC, "i18n", f)),
  path.join(SRC, "shell-extras.js"),
];
for (const file of dictFiles) {
  const I18N = {
    add(lang, obj) {
      if (!(lang in dict)) { errors.push(`${rel(file)}: I18N.add với ngôn ngữ lạ "${lang}"`); return; }
      for (const [k, v] of Object.entries(obj)) {
        if (typeof v !== "string") errors.push(`${rel(file)}: [${lang}] ${k} không phải chuỗi`);
        if (defined[lang].has(k) && defined[lang].get(k) !== rel(file)) {
          errors.push(`${rel(file)}: [${lang}] key trùng "${k}" (đã có ở ${defined[lang].get(k)})`);
        }
        defined[lang].set(k, rel(file));
        dict[lang][k] = v;
      }
    },
  };
  // Chỉ chạy phần I18N.add: IIFE giao diện phía sau sẽ ném lỗi vì DOM giả — bỏ qua.
  const anyProxy = new Proxy(function () {}, { get: () => anyProxy, apply: () => anyProxy, construct: () => anyProxy });
  const ctx = { I18N, window: anyProxy, document: anyProxy, localStorage: anyProxy, navigator: {}, console };
  ctx.window = new Proxy({ I18N }, { get: (o, k) => (k in o ? o[k] : anyProxy) });
  try { vm.runInNewContext(read(file), ctx, { filename: file }); } catch (_) { /* phần DOM — không liên quan */ }
}

// ---- 2. vi và en cùng tập key ----
const viKeys = new Set(Object.keys(dict.vi));
const enKeys = new Set(Object.keys(dict.en));
for (const k of viKeys) if (!enKeys.has(k)) errors.push(`thiếu bản EN: ${k} (${defined.vi.get(k)})`);
for (const k of enKeys) if (!viKeys.has(k)) errors.push(`thiếu bản VI: ${k} (${defined.en.get(k)})`);
for (const l of ["vi", "en"]) for (const [k, v] of Object.entries(dict[l])) {
  if (!v.trim()) errors.push(`[${l}] ${k} rỗng (${defined[l].get(k)})`);
}
// Biến {x} phải khớp giữa hai bản.
const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
for (const k of viKeys) if (enKeys.has(k) && vars(dict.vi[k]) !== vars(dict.en[k])) {
  errors.push(`biến lệch vi/en ở ${k}: {${vars(dict.vi[k])}} vs {${vars(dict.en[k])}}`);
}

// ---- 3. Mọi key được dùng đều tồn tại ----
const lineOf = (text, idx) => text.slice(0, idx).split("\n").length;
const used = []; // [key, where]
const jsFiles = fs.readdirSync(SRC).filter((f) => f.endsWith(".js")).map((f) => path.join(SRC, f));
for (const file of jsFiles) {
  const src = read(file);
  for (const m of src.matchAll(/\bt\(\s*(["'`])([\w.-]+)\1/g)) used.push([m[2], `${rel(file)}:${lineOf(src, m.index)}`]);
  // dataset.i18n* = "key" hoặc = cond ? "a" : "b"
  for (const m of src.matchAll(/dataset\.i18n\w*\s*=\s*([^;\n]+)/g)) {
    for (const s of m[1].matchAll(/["']([\w-]+\.[\w.-]+)["']/g)) used.push([s[1], `${rel(file)}:${lineOf(src, m.index)}`]);
  }
}
const html = read(path.join(SRC, "index.html"));
for (const m of html.matchAll(/data-i18n(?:-title|-placeholder|-aria)?="([^"]+)"/g)) {
  used.push([m[1], `app/src/index.html:${lineOf(html, m.index)}`]);
}
for (const [k, where] of used) {
  // Tiền tố động (vd. t("form.kind." + kind)): chỉ cần có ít nhất một key bắt đầu bằng nó.
  if (k.endsWith(".")) {
    if (![...viKeys].some((x) => x.startsWith(k))) errors.push(`tiền tố key không có key nào: ${k} — ${where}`);
    continue;
  }
  if (!viKeys.has(k) && !enKeys.has(k)) errors.push(`key không tồn tại: ${k} — ${where}`);
}

// ---- 4. main.js không còn chuỗi tiếng Việt ngoài comment ----
const VI = /[àáảãạăằắẳẵặâầấẩẫậđèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵ]/i;
const mainSrc = read(path.join(SRC, "main.js"));
// Bỏ comment (// và /* */) nhưng giữ nguyên chuỗi và số dòng.
function stripComments(s) {
  let out = "", i = 0, q = null;
  while (i < s.length) {
    const c = s[i], n = s[i + 1];
    if (q) {
      out += c;
      if (c === "\\") { out += n ?? ""; i += 2; continue; }
      if (c === q) q = null;
      i++;
    } else if (c === '"' || c === "'" || c === "`") { q = c; out += c; i++; }
    else if (c === "/" && n === "/") { while (i < s.length && s[i] !== "\n") i++; }
    else if (c === "/" && n === "*") {
      const end = s.indexOf("*/", i + 2); const chunk = s.slice(i, end < 0 ? s.length : end + 2);
      out += chunk.replace(/[^\n]/g, ""); i += chunk.length;
    } else { out += c; i++; }
  }
  return out;
}
const viLines = stripComments(mainSrc).split("\n")
  .map((l, i) => [i + 1, l]).filter(([, l]) => VI.test(l));
for (const [n, l] of viLines) errors.push(`main.js:${n} còn chữ tiếng Việt: ${l.trim().slice(0, 100)}`);

// ---- Kết quả ----
console.log(`i18n: ${viKeys.size} key vi, ${enKeys.size} key en, ${used.length} lượt dùng, ${dictFiles.length} tệp từ điển`);
if (errors.length) {
  console.error(`FAIL — ${errors.length} lỗi:`);
  for (const e of errors) console.error("  " + e);
  process.exit(1);
}
console.log("PASS");
