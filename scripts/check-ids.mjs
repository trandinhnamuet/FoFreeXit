// Kiểm tra ID: mọi $("id") / getElementById("id") trong app/src/*.js phải có id="..." trong
// index.html hoặc được tạo động trong JS (id="..." trong template, .id = "...").
// Chạy: node scripts/check-ids.mjs   (exit 1 nếu có lỗi)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SRC = path.join(ROOT, "app", "src");
const read = (f) => fs.readFileSync(f, "utf8");
const lineOf = (text, idx) => text.slice(0, idx).split("\n").length;

const known = new Set();
const html = read(path.join(SRC, "index.html"));
for (const m of html.matchAll(/\bid="([^"]+)"/g)) known.add(m[1]);

const FEAT = path.join(SRC, "features");
const jsFiles = [
  ...fs.readdirSync(SRC).filter((f) => f.endsWith(".js")),
  ...(fs.existsSync(FEAT) ? fs.readdirSync(FEAT).filter((f) => f.endsWith(".js")).map((f) => "features/" + f) : []),
];
const sources = jsFiles.map((f) => [f, read(path.join(SRC, f))]);
for (const [, src] of sources) {
  for (const m of src.matchAll(/\bid=\\?["']([\w-]+)\\?["']/g)) known.add(m[1]);           // id="x" trong template HTML
  for (const m of src.matchAll(/\.id\s*=\s*["'`]([\w-]+)["'`]/g)) known.add(m[1]);          // el.id = "x"
  for (const m of src.matchAll(/setAttribute\(\s*["']id["']\s*,\s*["'`]([\w-]+)["'`]/g)) known.add(m[1]);
}

const missing = [];
let refs = 0;
for (const [f, src] of sources) {
  for (const m of src.matchAll(/(?:\$|getElementById)\(\s*(["'`])([\w-]+)\1\s*\)/g)) {
    refs++;
    if (!known.has(m[2])) missing.push(`app/src/${f}:${lineOf(src, m.index)}  ${m[2]}`);
  }
}

console.log(`ids: ${known.size} id đã biết, ${refs} lượt tham chiếu trong ${jsFiles.length} tệp JS`);
if (missing.length) {
  console.error(`FAIL — ${missing.length} id không tồn tại:`);
  for (const e of missing) console.error("  " + e);
  process.exit(1);
}
console.log("PASS");
