import { taskContent } from "../lib/task-content.mjs";
// task 첨부(plan §8.327, 사용자 10-20 「할일에 이미지 첨부할수있게 해」·「붙여넣기도 지원하게」·「폰에서 캡쳐 후 복사해서 바로 붙이게」·
// 「md·html 문서도 task에 첨부(링크)」). 그림·문서는 프로젝트 uploads/에 쓰고(이름은 내용 hash) 메모에 첨부 줄을 더한다:
// `![그림](uploads/<이름>)`·`[제목](uploads/<이름>.md)`·`[제목](library/<보고서>/<경로>)`·`[제목](https://…)`. 손대지 않은 폼에 붙이면
// 바로 저장하고, 폼을 고치는 중이면 초안에 넣어 「저장」 때 쓴다. 「바로 시작」은 메모가 가리키는 host 파일을 thread 작업 폴더
// `attachments/`에 복사하고 할 일 글의 경로를 바꾼다. 시간은 기다리지 않는다: 화면 표지(`data-tasks-sha`·`data-count`)와 파일 글을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, until, step, browser, domIs, shot, base, openSettings } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const dir = () => `${root}/projects/demo`;
const file = () => `${dir()}/TASKS.md`;
const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
const read = () => fs.readFileSync(file(), "utf8");
const write = (text) => {
  fs.writeFileSync(file(), text);
  // 화면 관측 전에 식별 주석 이주를 마친 fixture를 준비한다.
  return aaa("task", "list").sha256;
};
const START = `# Tasks

## Backlog
- [ ] 화면 고치기 (fake@labA)
  버튼이 잘린다
- [ ] 계획 반영
- [ ] 폰에서 붙이기
- [ ] 보고서 읽기
`;
const shaIs = (p, want) => domIs(p, `할 일 hash ${want.slice(0, 8)}`, (w) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha === w, want);
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
const countIs = (p, n) => domIs(p, `첨부 ${n}개`, (n) => document.querySelector('[data-testid="task-attach"]')?.dataset.count === String(n), n);
/** 그 task 블록의 메모 줄들(파일에서) */
const notesOf = (title) => {
  const lines = read().split("\n");
  const at = lines.findIndex((l) => l.startsWith(`- [ ] ${title}`));
  const out = [];
  for (let i = at + 1; i < lines.length && lines[i].startsWith("  "); i++) out.push(lines[i].trim());
  return out;
};
/** 페이지의 한 요소에 File을 실은 붙여 넣기(사람의 ⌘V와 같은 ClipboardEvent). `selector`가 없으면 문서 본문 */
async function pasteOn(p, selector, files, text) {
  await p.evaluate(
    ({ selector, files, text }) => {
      const dt = new DataTransfer();
      for (const f of files) dt.items.add(new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: f.type }));
      if (text) dt.setData("text/plain", text);
      const target = selector ? document.querySelector(selector) : document.body;
      target.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    },
    { selector, files: files.map((f) => ({ ...f, b64: Buffer.from(f.bytes).toString("base64") })), text },
  );
}
const png = (bytes, name = "image.png") => ({ bytes, name, type: "image/png" });
/** 공유 설정의 테마(모든 기기가 따른다): 밝음·어두움 캡처 */
async function setTheme(page, scheme) {
  const d = await openSettings(page, "모양");
  await d.getByRole("group", { name: "테마" }).getByRole("button", { name: scheme === "light" ? "밝게" : "어둡게" }).click();
  await page.keyboard.press("Escape");
  await domIs(page, `테마 ${scheme}`, (s) => document.documentElement.dataset.theme === s, scheme);
}
/** 그림이 실제로 그려졌다(host가 준 바이트를 브라우저가 풀었다) */
const thumbsDrawn = (p, n) =>
  domIs(p, `썸네일 ${n}개 그려짐`, (n) => [...document.querySelectorAll('[data-testid="task-attach-item"] img')].filter((i) => i.complete && i.naturalWidth > 0).length === n, n);

export default async function ({ page }) {
  let md;
  let shotPng;
  let uploadedMd;
  let started;
  await step("붙여 넣기: task를 고른 채 ⌘V(스크린샷)면 사진 고르기 없이 그 task 메모에 첨부 줄이 저장되고 썸네일이 보인다", async () => {
    // 가짜 thread가 곧 턴을 끝내 앱 안 알림이 화면 위를 덮는다: 이 기기는 앱 안 알림을 끈다(43·47과 같다)
    const dialog = await openSettings(page, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    md = write(START);
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await shaIs(page, md);
    await domIs(page, "편집 가능", () => document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "true");
    await pick(page, "화면 고치기");
    await page.getByTestId("task-attach-paste").waitFor();
    await shot(page, "attach-desktop-before");
    // 붙여 넣을 그림은 이 화면의 스크린샷이다(사용자가 찍어 복사하는 것과 같은 PNG)
    shotPng = await page.screenshot();
    await pasteOn(page, null, [png(shotPng)]);
    const name = `${sha(shotPng).slice(0, 16)}.png`;
    await until("첨부 줄", () => notesOf("화면 고치기").includes(`![그림](uploads/${name})`));
    if (!notesOf("화면 고치기").includes("버튼이 잘린다")) throw new Error(`메모가 사라졌다: ${notesOf("화면 고치기")}`);
    if (!fs.readFileSync(`${dir()}/uploads/${name}`).equals(shotPng)) throw new Error("uploads/에 쓴 그림이 붙인 것과 다르다");
    await countIs(page, 1);
    await thumbsDrawn(page, 1);
    await row(page, "화면 고치기").getByTestId("task-attach-mark").waitFor();
    await shot(page, "attach-desktop-pasted");
    // 같은 그림을 다시 붙여도 파일·줄은 하나다(이름이 내용 hash)
    const before = read();
    await pasteOn(page, null, [png(shotPng, "again.png")]);
    await page.getByTestId("task-attach-pending").waitFor({ state: "detached" });
    if (read() !== before) throw new Error(`같은 그림이 두 번 붙었다:\n${read()}`);
    // 크게 보기: 누르면 화면을 덮고 Esc는 보기만 닫는다(상세는 남는다)
    await page.getByTestId("task-attach-item").first().locator("button").first().click();
    await page.getByTestId("lightbox").waitFor();
    await shot(page, "attach-desktop-viewer");
    await page.keyboard.press("Escape");
    await page.getByTestId("lightbox").waitFor({ state: "detached" });
    await page.getByTestId("task-detail").waitFor();
  });

  await step("메모를 고치는 중 붙여 넣기: 입력칸에는 글만 남고 첨부 칸에 「저장 전」으로 붙으며, 저장하면 충돌 없이 함께 쓴다", async () => {
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-notes-toggle").click();
    const box = detail.getByTestId("task-field-notes");
    await box.waitFor();
    // 메모 입력칸은 첨부 줄을 빼고 보인다(첨부는 첨부 칸이 보인다)
    if ((await box.inputValue()) !== "버튼이 잘린다") throw new Error(`입력칸에 첨부 줄이 보인다: ${await box.inputValue()}`);
    // 메모를 고치는 중(손댄 폼)이다
    await box.click();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type("\n여백도 본다");
    const small = await page.locator(".task-attach-grid").screenshot();
    await pasteOn(page, '[data-testid="task-field-notes"]', [png(small, "crop.png")]);
    const name = `${sha(small).slice(0, 16)}.png`;
    await domIs(page, "저장 전 첨부", (n) => document.querySelector(`[data-testid="task-attach-item"][data-path="uploads/${n}"] .task-attach-badge`) !== null, name);
    if ((await box.inputValue()) !== "버튼이 잘린다\n여백도 본다") throw new Error(`입력칸 글이 바뀌었다: ${await box.inputValue()}`);
    if (notesOf("화면 고치기").some((l) => l.includes(name))) throw new Error("고치는 중인데 바로 저장했다");
    await shot(page, "attach-desktop-draft");
    await detail.getByTestId("task-save").click();
    // 글 뒤에 첨부 줄이 차례대로 온다
    await until("저장한 첨부 줄", () => notesOf("화면 고치기").join("|") === `버튼이 잘린다|여백도 본다|![그림](uploads/${sha(shotPng).slice(0, 16)}.png)|![그림](uploads/${name})`);
    if (await page.locator('[role="alertdialog"], [data-testid^="task-conflict"]').count()) throw new Error("충돌 창이 떴다");
    await countIs(page, 2);
  });

  await step("붙여넣기 칸·떼기: 칸에 붙인 그림은 저장되고, 글만 붙이면 까닭을 알린다. ×로 떼면 줄만 지우고 파일은 남는다", async () => {
    await pick(page, "폰에서 붙이기");
    await page.getByTestId("task-attach-paste").waitFor();
    await pasteOn(page, '[data-testid="task-attach-paste"]', [], "그냥 글");
    await page.getByTestId("task-attach-note").waitFor();
    const img = await page.screenshot({ clip: { x: 0, y: 0, width: 240, height: 160 } });
    await pasteOn(page, '[data-testid="task-attach-paste"]', [png(img, "phone.png")]);
    const name = `${sha(img).slice(0, 16)}.png`;
    await until("칸에 붙인 첨부", () => notesOf("폰에서 붙이기").includes(`![그림](uploads/${name})`));
    await countIs(page, 1);
    await page.getByTestId("task-attach-remove").first().click();
    await until("뗀 줄", () => notesOf("폰에서 붙이기").length === 0);
    if (!fs.existsSync(`${dir()}/uploads/${name}`)) throw new Error("떼면서 파일을 지웠다(되살리기가 다시 보이지 못한다)");
    const log = aaa("task", "log", "--limit", "3").entries;
    if (!log.some((e) => e.title === "폰에서 붙이기" && e.action === "update")) throw new Error(`뗀 기록이 없다: ${JSON.stringify(log)}`);
  });

  await step("문서: md를 올리면 문서 줄, 누르면 그려진 Markdown, 보고서 문서는 고르기로 붙이고 html은 보고서 페이지로 연다, 주소는 링크로", async () => {
    // thread가 만든 계획서 보고서(보고서 화면과 같은 library)
    const plan = `${root}/plan-report`;
    fs.mkdirSync(plan, { recursive: true });
    fs.writeFileSync(`${plan}/plan.md`, "# 계획서\n\n- 첫 단계\n- 둘째 단계\n");
    fs.writeFileSync(`${plan}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1 id=page>계획 보고서</h1></html>");
    aaa("library", "publish", plan, "--project", "demo", "--name", "plan-report");
    await pick(page, "계획 반영");
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-attach-paste").waitFor();
    // 올린 md
    const text = "# 올린 계획\n\n고칠 것 셋\n";
    uploadedMd = `${sha(Buffer.from(text)).slice(0, 16)}.md`;
    await detail.getByTestId("task-attach-input").setInputFiles({ name: "draft-plan.md", mimeType: "", buffer: Buffer.from(text) });
    await until("올린 문서 줄", () => notesOf("계획 반영").includes(`[draft-plan.md](uploads/${uploadedMd})`));
    // 보고서에서 고르기
    await detail.getByTestId("task-attach-link").click();
    const form = detail.getByTestId("task-doclink-form");
    await form.getByTestId("task-doclink-report").selectOption("plan-report");
    await domIs(page, "보고서 문서 목록", () => [...document.querySelectorAll('[data-testid="task-doclink-file"] option')].some((o) => o.value === "plan.md"));
    await form.getByTestId("task-doclink-file").selectOption("plan.md");
    await form.getByTestId("task-doclink-label").fill("계획서");
    await shot(page, "attach-desktop-doclink");
    await form.getByTestId("task-doclink-add").click();
    await until("보고서 문서 줄", () => notesOf("계획 반영").includes("[계획서](library/plan-report/plan.md)"));
    await detail.getByTestId("task-attach-link").click();
    await detail.getByTestId("task-doclink-target").fill("library/plan-report/index.html");
    await detail.getByTestId("task-doclink-label").fill("계획 보고서");
    await detail.getByTestId("task-doclink-add").click();
    await until("html 문서 줄", () => notesOf("계획 반영").includes("[계획 보고서](library/plan-report/index.html)"));
    // 주소 글을 붙여넣기 칸에 붙이면 링크다
    await pasteOn(page, '[data-testid="task-attach-paste"]', [], "https://example.com/spec");
    await until("주소 줄", () => notesOf("계획 반영").includes("[example.com/spec](https://example.com/spec)"));
    await countIs(page, 4);
    await shot(page, "attach-desktop-docs");
    // 열기: 올린 md는 그린 Markdown
    await detail.locator(`[data-testid="task-doc"][data-target="uploads/${uploadedMd}"] [data-testid="task-doc-open"]`).click();
    await page.getByTestId("file-viewer-markdown").getByText("올린 계획", { exact: true }).waitFor();
    await page.keyboard.press("Escape");
    await page.getByTestId("file-viewer").waitFor({ state: "detached" });
    // 보고서 md
    await detail.locator('[data-testid="task-doc"][data-target="library/plan-report/plan.md"] [data-testid="task-doc-open"]').click();
    await page.getByTestId("file-viewer-markdown").getByText("계획서", { exact: true }).waitFor();
    await shot(page, "attach-desktop-open-md");
    await page.keyboard.press("Escape");
    await page.getByTestId("file-viewer").waitFor({ state: "detached" });
    // 보고서 html은 sandbox 페이지
    await detail.locator('[data-testid="task-doc"][data-target="library/plan-report/index.html"] [data-testid="task-doc-open"]').click();
    const frame = page.frameLocator('[data-testid="file-viewer"] iframe.file-viewer-page');
    await frame.locator("#page", { hasText: "계획 보고서" }).waitFor();
    await shot(page, "attach-desktop-open-html");
    await page.keyboard.press("Escape");
    await page.getByTestId("file-viewer").waitFor({ state: "detached" });
    // 올린 html은 앱 origin에서 돌지 않는다: 결과물 보기와 같은 sandbox CSP·nosniff로 보낸다. 없는 이름은 404다
    const served = await page.evaluate(async (b) => {
      const up = await fetch(`${b}api/v1/projects/demo/uploads?kind=html`, { method: "POST", body: "<!doctype html><script>parent.document.title='x'</script>" });
      const { ok } = await up.json();
      const r = await fetch(`${b}api/v1/projects/demo/${ok.path}`);
      const missing = await fetch(`${b}api/v1/projects/demo/uploads/0123456789abcdef.png`);
      return { csp: r.headers.get("content-security-policy"), sniff: r.headers.get("x-content-type-options"), type: r.headers.get("content-type"), missing: missing.status };
    }, base);
    if (!/(^|;)\s*sandbox\b/.test(served.csp ?? "") || served.sniff !== "nosniff" || !served.type?.startsWith("text/html") || served.missing !== 404) {
      throw new Error(`올린 html 응답: ${JSON.stringify(served)}`);
    }
  });

  await step("바로 시작: 첨부 그림·올린 문서·보고서 문서를 thread 작업 폴더 attachments/로 복사하고 할 일 글과 brief가 그 경로를 가리킨다", async () => {
    // 첨부 셋을 가진 task를 owner 있는 task로 만들어 바로 시작한다
    await pick(page, "화면 고치기");
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-attach-link").click();
    await detail.getByTestId("task-doclink-target").fill("library/plan-report/plan.md");
    await detail.getByTestId("task-doclink-add").click();
    await until("계획서 줄", () => notesOf("화면 고치기").some((l) => l.includes("library/plan-report/plan.md")));
    await pick(page, "화면 고치기");
    await detail.getByTestId("task-handoff-start").click();
    await domIs(page, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
    await page.getByTestId("task-start-repo").selectOption("작업 폴더");
    await shot(page, "attach-desktop-start");
    await page.getByTestId("task-start-go").click();
    await page.getByTestId("task-started").waitFor();
    const id = await until("이은 thread", () => / · (t-\d{4})$/m.exec(taskContent(read()))?.[1]);
    started = id;
    const t = aaa("thread", "show", "demo", id);
    const task = fs.readFileSync(`${dir()}/threads/${id}.task.md`, "utf8");
    if (task.includes("](uploads/") || task.includes("](library/")) throw new Error(`host 경로가 할 일에 남았다:\n${task}`);
    const names = [...task.matchAll(/\]\(attachments\/([0-9a-f]{16}\.(?:png|md|html))\)/g)].map((m) => m[1]);
    if (names.length !== 3) throw new Error(`넘길 첨부 셋이 아니다: ${names}\n${task}`);
    const work = await until("작업 폴더", () => aaa("thread", "show", "demo", id).work_dir);
    for (const n of names) {
      const got = await until(`attachments/${n}`, () => fs.existsSync(`${work}/attachments/${n}`) && fs.readFileSync(`${work}/attachments/${n}`));
      if (sha(got).slice(0, 16) !== n.split(".")[0]) throw new Error(`복사한 ${n}의 내용이 이름(hash)과 다르다`);
    }
    if (!fs.readFileSync(`${work}/attachments/${names.find((n) => n.endsWith(".md"))}`, "utf8").includes("# 계획서")) throw new Error("보고서 문서를 복사하지 않았다");
    const brief = fs.readFileSync(`${work}/brief.md`, "utf8");
    if (!brief.includes(`- task 첨부 3개(할 일의 \`attachments/…\`): \`${work}/attachments\``)) throw new Error(`brief 경로 절에 첨부 줄이 없다:\n${brief.slice(-600)}`);
    // 가짜 agent가 보고를 쓴 뒤 끝낸다(47과 같다)
    await until(`${id} 보고`, () => aaa("thread", "show", "demo", id).report_hash);
    aaa("thread", "resolve", "demo", t.id);
  });

  await step("할 일에 추가: 보고서 머리·thread 결과물 줄에서 그 문서를 링크한 새 task를 만들거나 있는 task에 링크한다", async () => {
    // 보고서 머리: index.html이 있는 보고서
    await page.evaluate(() => (location.hash = "#report=demo%2Fplan-report"));
    await page.locator('[data-testid="report-detail"][data-status="ready"]').waitFor();
    await shot(page, "attach-desktop-report-add");
    await page.getByTestId("report-task-add").click();
    const add = page.getByTestId("task-docadd");
    await add.waitFor();
    await shot(page, "attach-desktop-docadd");
    await add.getByTestId("task-docadd-new").click();
    const detail = page.getByTestId("task-detail");
    await countIs(page, 1);
    await detail.getByTestId("task-field-title").fill("계획 보고서 검토");
    await detail.getByTestId("task-save").click();
    await until("새 task의 보고서 줄", () => JSON.stringify(notesOf("계획 보고서 검토")) === JSON.stringify(["[plan-report](library/plan-report/index.html)"]));
    // thread 결과물의 md 줄: 있는 task에 링크한다
    await page.evaluate((id) => (location.hash = `#thread=${id}`), started);
    const notes = page.locator('[data-testid="files"] [data-testid="file-task-add"][data-path="docs/notes.md"]');
    await notes.waitFor();
    if (await page.locator('[data-testid="files"] [data-testid="file-task-add"][data-path="big.bin"]').count()) throw new Error("문서가 아닌 결과물에 「할 일에」가 있다");
    await notes.scrollIntoViewIfNeeded();
    await shot(page, "attach-desktop-thread-add");
    await notes.click();
    await add.waitFor();
    await add.getByTestId("task-docadd-search").fill("보고서 읽기");
    await add.locator('[data-testid="task-docadd-pick"][data-title="보고서 읽기"]').click();
    await until("결과물 문서 줄", () => notesOf("보고서 읽기").includes(`[notes.md](library/${started}/docs/notes.md)`));
    await add.waitFor({ state: "detached" });
    await countIs(page, 1);
    // 이미 그 문서를 링크한 task는 다시 고를 수 없다
    await page.evaluate((id) => (location.hash = `#thread=${id}`), started);
    await notes.click();
    await add.getByTestId("task-docadd-search").fill("보고서 읽기");
    await domIs(page, "이미 링크한 task 빠짐", () => document.querySelectorAll('[data-testid="task-docadd-pick"]').length === 0);
    await page.keyboard.press("Escape");
    await add.waitFor({ state: "detached" });
  });

  await step("보고서 주소: 공유 링크·앱 보고서 주소를 붙이면 앱 안 보고서 문서로 알아보고, 풀 수 없는 주소는 주소 그대로 붙인다", async () => {
    await pick(page, "보고서 읽기");
    const detail = page.getByTestId("task-detail");
    await countIs(page, 1);
    const shared = aaa("library", "link", "demo", "plan-report");
    await pasteOn(page, '[data-testid="task-attach-paste"]', [], new URL(`${shared.base}plan.md`, base).href);
    await until("공유 링크 → 보고서 문서 줄", () => notesOf("보고서 읽기").includes("[plan.md](library/plan-report/plan.md)"));
    await pasteOn(page, '[data-testid="task-attach-paste"]', [], `${base}#report=demo%2Fplan-report`);
    await until("앱 보고서 주소 → index.html 줄", () => notesOf("보고서 읽기").includes("[plan-report](library/plan-report/index.html)"));
    const bogus = new URL(`/v/${"0".repeat(48)}/x.md`, base).href;
    await pasteOn(page, '[data-testid="task-attach-paste"]', [], bogus);
    await until("풀 수 없는 주소는 주소 줄", () => notesOf("보고서 읽기").some((l) => l.endsWith(`](${bogus})`)));
    await detail.locator('[role="alert"]', { hasText: "보고서 주소를 앱 문서로 바꾸지 못해" }).waitFor();
    await countIs(page, 4);
    await detail.locator('[data-testid="task-doc"][data-target="library/plan-report/plan.md"]').scrollIntoViewIfNeeded();
    await shot(page, "attach-desktop-report-url");
    await detail.locator('[data-testid="task-doc"][data-target="library/plan-report/plan.md"] [data-testid="task-doc-open"]').click();
    await page.getByTestId("file-viewer-markdown").getByText("계획서", { exact: true }).waitFor();
    await page.keyboard.press("Escape");
    await page.getByTestId("file-viewer").waitFor({ state: "detached" });
  });

  await step("보기 권한: 썸네일·문서는 보이고 붙여넣기 칸·떼기는 없으며, 올리기 요청은 거부된다", async () => {
    const { page: r } = await pairDevice(browser, aaa("web", "pair", "--reader").code, "desktop", { hash: "#tasks" });
    await pick(r, "계획 반영");
    await r.getByTestId("task-attach").waitFor();
    if ((await r.getByTestId("task-attach-paste").count()) || (await r.getByTestId("task-attach-remove").count())) throw new Error("보기 권한에 붙이기·떼기가 있다");
    await r.locator('[data-testid="task-doc"][data-target="library/plan-report/plan.md"] [data-testid="task-doc-open"]').click();
    await r.getByTestId("file-viewer-markdown").getByText("계획서", { exact: true }).waitFor();
    await r.keyboard.press("Escape");
    await r.getByTestId("file-viewer").waitFor({ state: "detached" });
    const status = await r.evaluate(async (b) => (await fetch(`${b}api/v1/projects/demo/uploads`, { method: "POST", body: new Uint8Array([0x89, 0x50, 0x4e, 0x47]) })).status, base);
    if (status !== 403) throw new Error(`보기 권한 올리기가 ${status}로 답했다`);
    await r.evaluate(() => (location.hash = "#report=demo%2Fplan-report"));
    await r.locator('[data-testid="report-detail"][data-status="ready"]').waitFor();
    if (await r.getByTestId("report-task-add").count()) throw new Error("보기 권한에 「할 일에 추가」가 있다");
    await r.close();
  });

  await step("휴대폰·패드(밝음·어두움): 상세 시트의 붙여넣기 칸을 길게 눌러 붙인 그림이 바로 첨부되고, 문서를 눌러 연다", async () => {
    for (const [kind, scheme] of [["iphone", "light"], ["iphone", "dark"], ["ipad", "light"], ["ipad", "dark"]]) {
      await setTheme(page, scheme);
      if (kind === "iphone" && scheme === "light") {
        await pick(page, "계획 반영");
        await page.getByTestId("task-attach").scrollIntoViewIfNeeded();
        await shot(page, "attach-desktop-light-docs");
      }
      const { page: p } = await pairDevice(browser, aaa("web", "pair").code, kind, { hash: "#tasks", colorScheme: scheme });
      await domIs(p, `테마 ${scheme}`, (s) => document.documentElement.dataset.theme === s, scheme);
      await shaIs(p, sha(read()));
      await shot(p, `attach-${kind}-${scheme}-list`);
      await pick(p, "폰에서 붙이기");
      const zone = p.getByTestId("task-attach-paste");
      await zone.waitFor();
      await zone.scrollIntoViewIfNeeded();
      await shot(p, `attach-${kind}-${scheme}-before`);
      // 시스템 「붙여넣기」 메뉴가 고른 칸에 보내는 사건과 같다(메뉴 자체는 headless가 그리지 못한다)
      await zone.focus();
      const img = await p.screenshot();
      await pasteOn(p, '[data-testid="task-attach-paste"]', [png(img, "IMG_0001.png")]);
      const name = `${sha(img).slice(0, 16)}.png`;
      await until(`${kind} 첨부 줄`, () => notesOf("폰에서 붙이기").includes(`![그림](uploads/${name})`));
      await thumbsDrawn(p, 1);
      await p.getByTestId("task-attach").scrollIntoViewIfNeeded();
      await shot(p, `attach-${kind}-${scheme}-pasted`);
      await p.getByTestId("task-attach-item").first().locator("button").first().click();
      await p.getByTestId("lightbox").waitFor();
      await shot(p, `attach-${kind}-${scheme}-viewer`);
      await p.getByTestId("lightbox-close").click();
      await p.getByTestId("task-attach-remove").first().click();
      await until(`${kind} 뗀 줄`, () => notesOf("폰에서 붙이기").length === 0);
      // 문서 열기(계획 반영의 보고서 문서)
      await p.getByTestId("task-detail-close").click();
      await pick(p, "계획 반영");
      const doc = p.locator('[data-testid="task-doc"][data-target="library/plan-report/plan.md"] [data-testid="task-doc-open"]');
      await doc.scrollIntoViewIfNeeded();
      await shot(p, `attach-${kind}-${scheme}-docs`);
      await doc.click();
      await p.getByTestId("file-viewer-markdown").getByText("계획서", { exact: true }).waitFor();
      await shot(p, `attach-${kind}-${scheme}-open-md`);
      // 상세 시트 위 파일 보기를 닫으면 상세로 돌아오고, 다른 화면으로 가도 파일 보기가 되살아나지 않는다(시트 기록이 어긋나지 않는다)
      await p.getByTestId("file-viewer").getByRole("button", { name: "파일 닫기" }).click();
      await p.getByTestId("file-viewer").waitFor({ state: "detached" });
      await p.getByTestId("task-detail").waitFor();
      // 보고서 머리의 「할 일에 추가」와 그 창
      await p.evaluate(() => (location.hash = "#report=demo%2Fplan-report"));
      await p.locator('[data-testid="report-detail"][data-status="ready"]').waitFor();
      await p.getByTestId("report-frame").waitFor();
      if (await p.getByTestId("file-viewer").count()) throw new Error("닫은 파일 보기가 화면 이동 뒤에 다시 떴다");
      await shot(p, `attach-${kind}-${scheme}-report`);
      await p.getByTestId("report-task-add").click();
      await p.getByTestId("task-docadd").waitFor();
      await shot(p, `attach-${kind}-${scheme}-docadd`);
      await p.close();
    }
  });
}
