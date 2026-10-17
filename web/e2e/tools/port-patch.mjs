// 나누기 전 한 파일(web/e2e/web.e2e.mjs)에 한 변경을 나눈 파일(lib/harness.mjs·specs/*.mjs)로 옮긴다(2026-10 나누기, docs/testing.md
// 「웹 E2E」). 단계 본문은 글자 그대로 옮겼으므로 hunk마다 고치기 전 글이 그대로 있는 파일을 찾아 고친 글로 바꾼다.
// 둘 이상에 있거나 없으면 옮기지 않고 찍는다(새 단계는 어느 spec에 둘지 사람이 정한다). 나눌 때 바꾼 줄 둘은 같이 바꿔 찾는다.
//
//   git diff <갈라진 곳> <내 branch> -- web/e2e/web.e2e.mjs > /tmp/mine.patch
//   node web/e2e/tools/port-patch.mjs /tmp/mine.patch [--dry-run]
//
// 모든 thread가 나눈 뒤의 main을 받으면 이 파일을 지운다.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stripJs } from "../../test/fixed-sleeps.mjs";

const e2e = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [patchFile, flag] = process.argv.slice(2);
if (!patchFile) throw new Error("쓰는 법: node web/e2e/tools/port-patch.mjs <patch> [--dry-run]");
const dry = flag === "--dry-run";

// 나눌 때 바꾼 줄: t1로 돌아온 표지, push 키 대입(spec은 import한 이름에 쓸 수 없다)
const T1 = 'until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));';
const moved = (l) => {
  if (l.includes(T1)) return l.replace('"한글-8"', '"t1-42"');
  const pk = /^(\s*)pushKey = (.*);$/.exec(l);
  return pk ? `${pk[1]}setPushKey(${pk[2]});` : l;
};

const hunks = [];
let cur = null;
for (const l of fs.readFileSync(patchFile, "utf8").split("\n")) {
  if (l.startsWith("diff --git")) cur = null;
  else if (l.startsWith("@@")) hunks.push((cur = { head: l, pre: [], post: [] }));
  else if (!cur || l.startsWith("\\")) continue;
  else if (l.startsWith(" ") || l === "") {
    cur.pre.push(moved(l.slice(1)));
    cur.post.push(moved(l.slice(1)));
  } else if (l.startsWith("-")) cur.pre.push(moved(l.slice(1)));
  else if (l.startsWith("+")) cur.post.push(moved(l.slice(1)));
}
// 끝 빈 줄(patch 끝)은 문맥이 아니다
for (const h of hunks) while (h.pre.at(-1) === "" && h.post.at(-1) === "") h.pre.pop(), h.post.pop();

const files = [path.join(e2e, "lib/harness.mjs"), ...fs.readdirSync(path.join(e2e, "specs")).sort().map((f) => path.join(e2e, "specs", f))];
const text = new Map(files.map((f) => [f, fs.readFileSync(f, "utf8").split("\n")]));

/** lines 안에서 seq가 시작하는 자리들 */
const find = (lines, seq) => {
  const at = [];
  for (let i = 0; i + seq.length <= lines.length; i++) if (seq.every((l, k) => lines[i + k] === l)) at.push(i);
  return at;
};

let ok = 0;
const left = [];
for (const h of hunks) {
  // 문맥이 단계 경계(다른 spec)에 걸치면 앞뒤 문맥을 한 줄씩 덜어 다시 찾는다(바꾼 줄은 덜지 않는다)
  let lead = h.pre.findIndex((l, i) => l !== h.post[i]);
  let trail = 0;
  while (trail < h.pre.length && trail < h.post.length && h.pre.at(-1 - trail) === h.post.at(-1 - trail)) trail++;
  if (lead < 0) lead = h.pre.length;
  let placed = null;
  for (let cut = 0; !placed && cut <= Math.max(lead, trail); cut++) {
    const a = Math.min(cut, lead);
    const b = Math.min(cut, trail);
    const pre = h.pre.slice(a, h.pre.length - b);
    const post = h.post.slice(a, h.post.length - b);
    if (pre.length < 2 && pre.join("").trim().length < 12) break; // 너무 짧은 글은 어디에나 있다
    const hits = files.flatMap((f) => find(text.get(f), pre).map((i) => [f, i]));
    if (hits.length === 1) placed = { file: hits[0][0], at: hits[0][1], pre, post };
    else if (hits.length > 1) break;
  }
  const first = h.post.find((l, i) => l !== h.pre[i]) ?? h.pre.find((l, i) => l !== h.post[i]) ?? "";
  if (!placed) {
    left.push(`${h.head} ${first.trim().slice(0, 80)}`);
    continue;
  }
  text.get(placed.file).splice(placed.at, placed.pre.length, ...placed.post);
  ok++;
  console.log(`옮김 ${path.relative(e2e, placed.file)}:${placed.at + 1} ← ${h.head}`);
}
// 새 helper: harness의 export에 더하고, 쓰는 spec의 import에 더한다(나누기와 같은 규칙: 그 이름이 단어로 나오면 import)
const harnessFile = path.join(e2e, "lib/harness.mjs");
const list = (body) => body.split(",").map((n) => n.trim()).filter(Boolean);
const wrap = (head, names, tail) => {
  const one = `${head} ${names.join(", ")} ${tail}`;
  if (one.length <= 120) return one;
  const rows = [];
  for (const n of names) rows.at(-1) && rows.at(-1).length + n.length < 114 ? (rows[rows.length - 1] += ` ${n},`) : rows.push(`  ${n},`);
  return `${head}\n${rows.join("\n")}\n${tail}`;
};
const harnessText = text.get(harnessFile).join("\n");
const declared = [];
for (const l of stripJs(harnessText, { strings: true }).split("\n")) {
  const m = /^(?:async\s+)?function\*?\s+(\w+)|^(?:const|let|var|class)\s+(\w+)/.exec(l);
  if (m) declared.push(m[1] ?? m[2]);
}
const EXPORT = /\nexport \{([^}]*)\};\n*$/;
const exported = list(EXPORT.exec(harnessText)?.[1] ?? "");
const added = declared.filter((n) => !exported.includes(n));
if (added.length) {
  text.set(harnessFile, harnessText.replace(EXPORT, `\n${wrap("export {", [...exported, ...added], "};")}\n`).split("\n"));
  console.log(`harness export에 더함: ${added.join(", ")}`);
}
const IMPORT = /^import \{([^}]*)\} from "\.\.\/lib\/harness\.mjs";$/m;
for (const f of files.slice(1)) {
  const src = text.get(f).join("\n");
  const m = IMPORT.exec(src);
  if (!m) continue;
  const have = list(m[1]);
  // 다른 import(node:fs 등)로 이미 묶인 이름은 더하지 않는다
  const bound = [...src.matchAll(/^import (?:(\w+)|\{([^}]*)\}) from /gm)].flatMap((x) => (x[1] ? [x[1]] : list(x[2])));
  const code = stripJs(src);
  const need = [...exported, ...added].filter((n) => !bound.includes(n) && new RegExp(`(^|[^.\\w$])${n}\\b`).test(code.slice(m.index + m[0].length)));
  if (!need.length) continue;
  text.set(f, src.replace(IMPORT, wrap("import {", [...have, ...need], '} from "../lib/harness.mjs";')).split("\n"));
  console.log(`${path.relative(e2e, f)} import에 더함: ${need.join(", ")}`);
}
if (!dry) for (const [f, lines] of text) fs.writeFileSync(f, lines.join("\n"));
console.log(`hunk ${hunks.length}개 중 ${ok}개 옮김${dry ? "(--dry-run: 쓰지 않음)" : ""}`);
if (left.length) {
  console.log(`손으로 옮길 hunk ${left.length}개(새 단계는 그 기능의 spec 끝에, 새 helper는 harness의 export와 spec import에):`);
  for (const l of left) console.log(`  ${l}`);
  process.exitCode = 1;
}
