// 가짜 Claude Code 질문 창(plan §8.172, 웹 E2E 11 「화면 선택 창」). 화면은 지어낸 것이다(실제 기록을 옮기지 않음):
// AskUserQuestion 하나(칩·설명·추천·직접 쓰기·「Chat about this」)와 여러 질문(탭 막대·체크 상자·Submit 줄·검토).
// ↑↓로 커서, Enter로 고름(여러 개면 체크), →는 다음 탭, 직접 쓰기 줄에서 친 글은 그 줄에 보인다. 「r」은 선택지 하나의
// 이름을 바꾼다(카드가 본 창과 다른 창). 고른 것은 위에 「⏺ answer: …」로 남고, 다 답하면 입력 상자(쉼)로 간다.
const mode = process.argv[2];
const RULE = "─".repeat(40);
const forms = {
  one: [
    { chip: "저장소", text: "결과를 어디에 둘까요?", options: [["SQLite (Recommended)", "한 파일, 설치 없음"], ["Postgres", "서버가 필요하다"], ["파일 몇 개", null]] },
    { chip: "이름", text: "이 작업을 무엇이라 부를까요?", options: [["기본 이름", null]] },
  ],
  tabs: [
    { chip: "범위", text: "어디서 일할까요?", options: [["웹", null], ["TUI", null]] },
    { chip: "대상", text: "어디를 시험할까요?", multi: true, options: [["웹", null], ["TUI", null], ["CLI", null]] },
  ],
};
const qs = forms[mode];
const log = [];
let at = 0;
let cur = 0;
let draft = "";
let checked = new Set();
const answers = [];
let review = false;

function draw() {
  const out = ["\x1b[2J\x1b[H"];
  for (const l of log) out.push(`⏺ ${l}`);
  if (at >= qs.length && !review) {
    out.push("", "────────────────", "❯ ", "────────────────", "  ? for shortcuts");
    process.stdout.write(out.join("\r\n"));
    return;
  }
  out.push("", RULE);
  if (mode === "tabs") {
    out.push(`←  ${qs.map((q, i) => `${i < answers.length ? "☒" : "☐"} ${q.chip}`).join("  ")}  ✔ Submit  →`, "");
  } else out.push(` ☐ ${qs[at].chip}`, "");
  if (review) {
    out.push(" Review your answers", "");
    qs.forEach((q, i) => out.push(`  ● ${q.text}`, `    → ${answers[i]}`));
    out.push("", " Ready to submit your answers?", "");
    ["Submit answers", "Cancel"].forEach((l, i) => out.push(` ${cur === i ? "❯" : " "} ${i + 1}. ${l}`));
    process.stdout.write(out.join("\r\n"));
    return;
  }
  const q = qs[at];
  out.push(` ${q.text}`, "");
  const mark = (i) => (cur === i ? "❯" : " ");
  q.options.forEach(([label, desc], i) => {
    out.push(` ${mark(i)} ${i + 1}. ${q.multi ? `[${checked.has(i) ? "✔" : " "}] ` : ""}${label}`);
    if (desc) out.push(`      ${desc}`);
  });
  const n = q.options.length;
  out.push(` ${mark(n)} ${n + 1}. ${draft || "Type something."}`);
  if (q.multi) out.push(`   ${cur === n + 1 ? "❯ " : "  "}Submit`);
  out.push(RULE, ` ${mark(q.multi ? n + 2 : n + 1)} ${n + 2}. Chat about this`, "");
  out.push(mode === "tabs" ? " Enter to select · Tab/Arrow keys to navigate · Esc to cancel" : " Enter to select · ↑/↓ to navigate · Esc to cancel");
  process.stdout.write(out.join("\r\n"));
}

function next(answer) {
  answers.push(answer);
  log.push(`answer: ${answer}`);
  at += 1;
  cur = 0;
  draft = "";
  checked = new Set();
  if (mode === "tabs" && at >= qs.length) review = true;
}

function key(k) {
  if (review) {
    if (k === "\x1b[A") cur = 0;
    else if (k === "\x1b[B") cur = 1;
    else if (k === "\r") {
      review = false;
      log.push(cur === 0 ? `answers: ${qs.map((q, i) => `${q.chip}=${answers[i]}`).join("; ")}` : "answers: cancel");
    }
    return;
  }
  if (at >= qs.length) return;
  const q = qs[at];
  const n = q.options.length;
  const last = q.multi ? n + 2 : n + 1;
  if (k === "\x1b[A") cur = Math.max(0, cur - 1);
  else if (k === "\x1b[B") cur = Math.min(last, cur + 1);
  else if (k === "\x1b[C" && q.multi) next([...checked].sort().map((i) => q.options[i][0]).join(", ") || "-");
  else if (k === "\r") {
    if (cur < n && q.multi) checked.has(cur) ? checked.delete(cur) : checked.add(cur);
    else if (cur < n) next(q.options[cur][0]);
    else if (cur === n && draft) next(`custom ${draft}`);
  } else if (cur === n && !k.startsWith("\x1b")) draft += k;
  else if (k === "r") q.options[1][0] = "MySQL";
}

process.stdin.setRawMode(true);
process.stdin.on("data", (b) => {
  // 한 덩이에 여러 키가 올 수 있다: 화살표·Enter는 떼고 나머지는 글
  for (const k of b.toString("utf8").match(/\x1b\[[A-D]|\r|\x1b|[^\x1b\r]+/g) ?? []) key(k);
  draw();
});
draw();
