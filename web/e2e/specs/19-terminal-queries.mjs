// 웹 E2E 터미널 질의 답: 프로그램이 터미널에 묻는 것에 붙은 화면 수와 상관없이 답이 한 번 간다(session host가 터미널이다, plan §8.185).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 19-terminal-queries(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import { root, aaa, waitOutput, until, screenText, pairPage, step, browser, inList, sentFrames, sentCount, sentText } from "../lib/harness.mjs";

// 질의 프로그램: "#x"를 받으면 질의 x를 쓰고 표지 Q<n>.을 찍는다. "#z"면 그 질의 뒤 받은 것(답)을 SUM:<n>:JSON으로 보인다
const PROGRAM = `process.stdin.setRawMode(true);
const Q = { a: "\\x1b[c", b: "\\x1b[>c", p: "\\x1b[6n", k: "\\x1b[?u" };
let pending = "";
let got = "";
let n = 0;
process.stdout.write("queries> ");
process.stdin.on("data", (d) => {
  pending += d.toString("latin1");
  for (let m; (m = /#([a-z])/.exec(pending)); ) {
    got += pending.slice(0, m.index);
    pending = pending.slice(m.index + 2);
    if (m[1] === "z") {
      process.stdout.write("\\r\\nSUM:" + n + ":" + JSON.stringify(got) + "\\r\\n");
    } else {
      process.stdout.write(Q[m[1]] + "Q" + ++n + ".\\r\\n");
    }
    got = "";
  }
});
`;

// 질의마다 그 답이 맞는가(한 번). kitty 키보드 질의에는 답하지 않는다: aaa 화면은 그 키 방식을 보내지 않는다(xterm.js 5.5도 답하지 않는다)
const WANT = {
  a: ["DA1", (s) => s === "\x1b[?62;22c"],
  b: ["DA2", (s) => s === "\x1b[>1;0;0c"],
  p: ["DSR 6", (s) => /^\x1b\[\d+;\d+R$/.test(s)],
  k: ["kitty CSI ? u", (s) => s === ""],
};

export default async function ({ page }) {
  await step("터미널 질의 답(DA1·DA2·DSR 6·kitty CSI ? u): 붙은 화면 0·1·2개에서 session host가 한 번 답하고 화면(xterm)은 답하지 않는다", async () => {
    fs.writeFileSync(`${root}/queries.js`, PROGRAM);
    aaa("tab", "create", "--machine", "labA", "--label", "queries", "--", process.execPath, `${root}/queries.js`);
    await waitOutput("queries", "queries>");
    const views = [];
    const bad = [];
    let n = 0;
    const round = async (label) => {
      for (const [k, [name, ok]] of Object.entries(WANT)) {
        const marks = await Promise.all(views.map((v) => sentCount(v)));
        aaa("pane", "send-text", "queries", `#${k}`);
        n += 1;
        // 화면마다 질의 뒤의 표지까지 읽었다: 질의가 화면에 왔다면 xterm은 그 읽기 안에서 답을 보냈다(sentFrames)
        await waitOutput("queries", `Q${n}.`);
        for (const v of views) await until(`${label} 화면이 Q${n}.까지 읽음`, async () => (await screenText(v)).includes(`Q${n}.`));
        const answered = (await Promise.all(views.map((v, i) => sentFrames(v, marks[i])))).map(sentText).filter(Boolean);
        if (answered.length) bad.push(`${label} ${name}: 화면이 답했다 ${JSON.stringify(answered)}`);
        // session host의 답은 질의를 화면에 보내기 전에 pane 입력으로 갔다: 그 뒤에 보낸 #z보다 먼저 닿는다
        aaa("pane", "send-text", "queries", "#z");
        await waitOutput("queries", `SUM:${n}:`);
        const line = aaa("pane", "read", "queries", "--source", "visible").text.split("\n").find((l) => l.startsWith(`SUM:${n}:`));
        const got = JSON.parse(line.slice(`SUM:${n}:`.length));
        if (!ok(got)) bad.push(`${label} ${name}: pane이 받은 답 ${JSON.stringify(got)}`);
      }
    };
    await round("화면 0개");
    await inList(page, "pane-queries");
    await until("첫 화면 질의 프로그램", async () => (await screenText(page)).includes("queries>"));
    views.push(page);
    await round("화면 1개");
    const other = (await pairPage(browser, aaa("web", "pair").code, { width: 1000, height: 640 })).page;
    await inList(other, "pane-queries");
    await until("둘째 화면 질의 프로그램", async () => (await screenText(other)).includes("queries>"));
    views.push(other);
    await round("화면 2개");
    await other.context().close();
    if (bad.length) throw new Error(bad.join("\n"));
  });
}
