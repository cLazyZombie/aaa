// 자동화 만들기·고치기·지우기·되살리기·지금 실행·명령 승인(관리 GUI W2, plan §8.342). 실제 host가 폼 값을 원문 TOML에 합치고
// 본 판(sha256)일 때만 쓴다. 웹은 사람의 고침과 결정만 보낸다. 시간은 기다리지 않는다: 파일 글·routine 기록·inbox·화면 표지를 본다.
// 「저장만으로는 돌지 않음」은 저절로 돌지 않는 thread 사건 규칙을 저장한 뒤 ticker 울타리(`ticker tick`) 뒤에 기록 0건으로 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, step, browser, pairPage, until, domIs, drawerSettled, shot } from "../lib/harness.mjs";

const dir = () => `${root}/projects/demo`;
const file = (name) => `${dir()}/automations/${name}.toml`;
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
const call = (p, req) => p.evaluate(async (body) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json(), req);
const runs = (name) => aaa("routine", "runs", "demo", name).runs;
const rule = (name) => aaa("routine", "list").routines.find((r) => r.name === name);
/** 본문 편집 화면이 휴대폰 폭 안에 있고 옆으로 넘치지 않는다(떠 있는 창이 아니다, t-0628) */
const fits = (p, selector) => p.evaluate((sel) => {
  const sheet = document.querySelector(sel);
  const body = sheet?.querySelector(".automation-view-body");
  const box = sheet?.getBoundingClientRect();
  return sheet && body && box.left >= 0 && box.right <= innerWidth + 0.5 && body.scrollWidth <= body.clientWidth + 1 ? true : JSON.stringify({ box, sw: body?.scrollWidth, cw: body?.clientWidth, w: innerWidth });
}, selector);
/** 편집 창의 host 검사가 이 글을 원문으로 돌려줬다(폼을 고친 뒤 합친 원문) */
const tomlHas = async (_p, sheet, want) => {
  await sheet.getByTestId("automation-tab-toml").click();
  await until(`TOML에 ${want}`, async () => (await sheet.getByTestId("automation-toml").inputValue()).includes(want));
};
async function openPanel(p) {
  await p.getByTestId("activity-automations").click();
  await p.getByTestId("automations").waitFor();
}
async function openDetail(p, name) {
  await p.getByTestId(`automation-demo-${name}`).click();
  const sheet = p.getByTestId("automation-detail");
  await sheet.waitFor();
  return sheet;
}
/** 본문 설정 화면을 닫는다(앞 화면으로) */
async function closeDetail(p) {
  const sheet = p.getByTestId("automation-detail");
  await sheet.getByTestId("automation-close").click();
  await sheet.waitFor({ state: "hidden" });
}

export default async function ({ page }) {
  await step("새 자동화: 폼으로 조건·동작·본문을 고르면 host가 원문을 만들고, 검토는 저장하면 무엇이 일어나는지 보이며, 저장만으로는 돌지 않는다", async () => {
    aaa("project", "create", "demo");
    await openPanel(page);
    await page.getByTestId("automation-new-demo").click();
    const sheet = page.getByTestId("automation-editor");
    await sheet.waitFor();
    await sheet.getByTestId("automation-form-name").fill("watch");
    await sheet.getByTestId("automation-form-title").fill("실패 확인");
    await sheet.getByTestId("automation-form-condition").selectOption("thread");
    await sheet.getByTestId("automation-form-events").fill("thread_failed");
    await sheet.getByTestId("automation-form-body").fill("**실패**한 thread를 본다");
    // 사용량 문턱 기본값(5시간 5%·주간 10%)을 채웠다
    if ((await sheet.getByTestId("automation-form-usage-session").inputValue()) !== "5" || (await sheet.getByTestId("automation-form-usage-week").inputValue()) !== "10") throw new Error("사용량 문턱 기본값");
    await sheet.getByRole("button", { name: "미리보기", exact: true }).click();
    await until("Markdown 미리보기", async () => (await sheet.getByTestId("automation-body-preview").locator("strong").textContent()) === "실패");
    await shot(page, "automation-edit-new-form");
    await tomlHas(page, sheet, "[on.thread]");
    const text = await sheet.getByTestId("automation-toml").inputValue();
    for (const want of ["#:schema ./automation.schema.json", 'title = "실패 확인"', 'events = ["thread_failed"]', "[do.inbox]", "usage_session_below = 5", "usage_week_below = 10"]) if (!text.includes(want)) throw new Error(`새 원문 ${want}:\n${text}`);
    if (/catch_up|notify|enabled/.test(text)) throw new Error(`기본값 칸을 썼다:\n${text}`);
    if (fs.existsSync(file("watch"))) throw new Error("검사가 파일을 썼다");
    await sheet.getByTestId("automation-tab-review").click();
    const review = sheet.getByTestId("automation-review");
    await review.waitFor();
    if (!(await review.textContent()).includes("저장만으로는 돌지 않고")) throw new Error(`검토 ${await review.textContent()}`);
    await shot(page, "automation-edit-new-review");
    await sheet.getByTestId("automation-save").click();
    await sheet.waitFor({ state: "detached" });
    // 저장 뒤 같은 본문 자리에 그 규칙 설정이 열린다(주소도 그 규칙). 파일은 검사가 보인 원문 그대로다
    const detail = page.getByTestId("automation-detail");
    await detail.waitFor();
    if ((await page.evaluate(() => location.hash)) !== `#automation=${encodeURIComponent("demo/watch")}`) throw new Error(`저장 뒤 주소 ${await page.evaluate(() => location.hash)}`);
    if (await page.locator(".modal-scrim").count()) throw new Error("편집이 떠 있는 창으로 열렸다");
    if (fs.readFileSync(file("watch"), "utf8") !== text) throw new Error("저장한 파일이 검사 원문과 다르다");
    await closeDetail(page);
    aaa("ticker", "tick");
    aaa("ticker", "tick");
    if (runs("watch").length !== 0) throw new Error(`저장만으로 돌았다: ${JSON.stringify(runs("watch"))}`);
    if (aaa("inbox", "list", "demo", "--peek").items.some((i) => i.subject === "watch" && i.item_kind === "routine")) throw new Error("저장만으로 안내가 생겼다");
  });

  await step("충돌: 두 브라우저·파일 직접 편집 중 본 판이 아니면 아무것도 쓰지 않고 차이를 보이며, 사람이 고른다", async () => {
    // a(page)가 편집기를 연 채, 파일을 직접 고친다
    const sheetA = page.getByTestId("automation-editor");
    const d = await openDetail(page, "watch");
    await d.getByRole("button", { name: "고치기", exact: true }).click();
    await sheetA.waitFor();
    await sheetA.getByTestId("automation-form-title").fill("a 화면 제목");
    const direct = fs.readFileSync(file("watch"), "utf8").replace("**실패**한 thread를 본다", "직접 고친 본문");
    fs.writeFileSync(file("watch"), direct);
    await tomlHas(page, sheetA, 'title = "a 화면 제목"');
    await sheetA.getByTestId("automation-save").click();
    const conflict = sheetA.getByTestId("automation-conflict");
    await conflict.waitFor();
    if (fs.readFileSync(file("watch"), "utf8") !== direct) throw new Error("충돌 저장이 파일을 바꿨다");
    const diff = await conflict.getByTestId("automation-conflict-diff").textContent();
    if (!diff.includes("+ text = \"직접 고친 본문\"") || !diff.includes("− title = \"a 화면 제목\"")) throw new Error(`차이 ${diff}`);
    await shot(page, "automation-edit-conflict");
    // 지금 파일로 다시 열면 내 초안을 버리고 지금 파일을 본다
    await conflict.getByRole("button", { name: /지금 파일로 다시 열기/ }).click();
    await conflict.waitFor({ state: "detached" });
    await tomlHas(page, sheetA, "직접 고친 본문");
    // 다른 브라우저(b)가 먼저 저장하면 a의 저장은 다시 거부된다. a가 차이를 보고 고르면 a의 초안을 쓴다
    const b = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      await openPanel(b.page);
      const db = await openDetail(b.page, "watch");
      await db.getByRole("button", { name: "고치기", exact: true }).click();
      const sheetB = b.page.getByTestId("automation-editor");
      await sheetB.getByTestId("automation-form-title").fill("b 제목");
      await tomlHas(b.page, sheetB, 'title = "b 제목"');
      await sheetA.getByTestId("automation-tab-form").click();
      await sheetA.getByTestId("automation-form-title").fill("a 둘째 제목");
      await tomlHas(page, sheetA, 'title = "a 둘째 제목"');
      await sheetB.getByTestId("automation-save").click();
      await sheetB.waitFor({ state: "detached" });
      const afterB = fs.readFileSync(file("watch"), "utf8");
      if (!afterB.includes('title = "b 제목"') || !afterB.includes("직접 고친 본문")) throw new Error(`b 저장 ${afterB}`);
      await sheetA.getByTestId("automation-save").click();
      await conflict.waitFor();
      if (fs.readFileSync(file("watch"), "utf8") !== afterB) throw new Error("a의 낡은 판 저장이 b를 덮었다");
      await conflict.getByRole("button", { name: /내 초안으로 저장/ }).click();
      await sheetA.waitFor({ state: "detached" });
      if (!fs.readFileSync(file("watch"), "utf8").includes('title = "a 둘째 제목"')) throw new Error("고른 뒤 a 초안 저장");
      // 저장하면 편집 화면이 그 규칙 설정으로 돌아온다
      await page.getByTestId("automation-detail").waitFor();
      await closeDetail(page);
      if (b.errors.length) throw new Error(b.errors.join("\n"));
    } finally { await b.page.context().close(); }
  });

  await step("지우기·되살리기: 지운 규칙은 낡은 편집기의 저장으로 되살아나지 않고, 「지운 자동화」에서 꺼진 채로 돌아온다", async () => {
    const sheetA = page.getByTestId("automation-editor");
    const d = await openDetail(page, "watch");
    await d.getByRole("button", { name: "고치기", exact: true }).click();
    await sheetA.getByTestId("automation-form-title").fill("지운 뒤 저장");
    await tomlHas(page, sheetA, 'title = "지운 뒤 저장"');
    const b = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      await openPanel(b.page);
      const db = await openDetail(b.page, "watch");
      await db.getByTestId("automation-source").locator("pre").first().waitFor();
      await db.getByRole("button", { name: "지우기", exact: true }).click();
      const confirm = b.page.getByRole("alertdialog");
      await confirm.getByRole("button", { name: "지우기", exact: true }).click();
      // 지우면 설정 화면을 닫고 앞 화면으로 간다
      await db.waitFor({ state: "hidden" });
      if (fs.existsSync(file("watch"))) throw new Error("지운 파일이 남았다");
      const trash = fs.readdirSync(`${dir()}/automations/.trash`);
      if (trash.length !== 1 || !fs.readFileSync(`${dir()}/automations/.trash/${trash[0]}`, "utf8").includes("enabled = false")) throw new Error(`.trash ${trash}`);
      await b.page.getByTestId("automation-trash-demo").waitFor();
      if (b.errors.length) throw new Error(b.errors.join("\n"));
    } finally { await b.page.context().close(); }
    await sheetA.getByTestId("automation-save").click();
    const conflict = sheetA.getByTestId("automation-conflict");
    await conflict.waitFor();
    if (!(await conflict.textContent()).includes("지워졌다")) throw new Error(`지운 뒤 저장 ${await conflict.textContent()}`);
    if (fs.existsSync(file("watch"))) throw new Error("지운 규칙이 저장으로 되살아났다");
    await sheetA.getByRole("button", { name: "닫기", exact: true }).last().click();
    await sheetA.waitFor({ state: "detached" });
    // 편집을 닫으면 지워진 규칙이라 「지워졌다」 안내다
    await page.getByTestId("automation-missing").waitFor();
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    const trash = page.getByTestId("automation-trash-demo");
    await trash.locator("summary").click();
    await shot(page, "automation-edit-trash");
    await trash.getByRole("button", { name: "꺼진 채로 되살리기", exact: true }).click();
    await page.getByTestId("automation-demo-watch").waitFor();
    if (rule("watch").enabled) throw new Error("되살린 규칙이 켜졌다");
    await domIs(page, "지운 목록 비움", () => !document.querySelector('[data-testid="automation-trash-demo"]'));
  });

  await step("한 초안: TOML을 고친 뒤 기다리지 않고 폼 탭에서 고쳐 저장해도 두 쪽 고침이 모두 남는다", async () => {
    fs.writeFileSync(file("both"), '#:schema ./automation.schema.json\n# 머리 주석\ntitle = "둘"\n[on.schedule]\nevery = "1d"\n[do.inbox]\ntext = "본다"\n');
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    const d = await openDetail(page, "both");
    await d.getByRole("button", { name: "고치기", exact: true }).click();
    const sheet = page.getByTestId("automation-editor");
    await sheet.getByTestId("automation-tab-toml").click();
    const toml = sheet.getByTestId("automation-toml");
    await toml.fill((await toml.inputValue()).replace('every = "1d"', 'every = "2d"'));
    // 검사 debounce를 기다리지 않고 곧바로 폼으로 가서 다른 칸을 고친다
    await sheet.getByTestId("automation-tab-form").click();
    await sheet.getByTestId("automation-form-title").fill("둘 다 고침");
    if ((await sheet.getByTestId("automation-form-every").inputValue()) !== "2d") throw new Error("폼이 TOML 고침을 읽지 않았다");
    await sheet.getByTestId("automation-save").click();
    await sheet.waitFor({ state: "detached" });
    const saved = fs.readFileSync(file("both"), "utf8");
    if (!saved.includes('every = "2d"') || !saved.includes('title = "둘 다 고침"') || !saved.startsWith("#:schema ./automation.schema.json\n# 머리 주석\n")) throw new Error(`두 쪽 고침 ${saved}`);
    await closeDetail(page);
  });

  await step("지금 실행: 같은 key는 한 번만 받고, 승인되지 않은 시작 범위·꺼진 규칙은 거부한다", async () => {
    fs.writeFileSync(file("note"), '[on.schedule]\nevery = "1d"\n[do.inbox]\ntext = "지금 실행 안내"\n');
    fs.writeFileSync(file("go"), '[on.schedule]\nevery = "1d"\n[do.start]\ntitle = "점검"\nprofiles = ["fake"]\nmachines = ["labA"]\ntext = "본다"\n');
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    const d = await openDetail(page, "note");
    await d.getByRole("button", { name: "지금 실행", exact: true }).click();
    const confirm = page.getByRole("alertdialog");
    await confirm.getByRole("button", { name: "지금 실행", exact: true }).click();
    await confirm.waitFor({ state: "detached" });
    await d.getByTestId("automation-notice").waitFor();
    const item = await until("지금 실행 안내", () => aaa("inbox", "list", "demo", "--peek").items.find((i) => i.subject === "note" && i.body.includes("지금 실행 안내")));
    if (!runs("note").some((r) => r.id.startsWith("manual:") && r.status === "done")) throw new Error(`실행 기록 ${JSON.stringify(runs("note"))}`);
    // 같은 key 두 번 → 하나
    const key = "e2e-same-key";
    const first = await call(page, { op: "automation_run", project: "demo", name: "note", key });
    const second = await call(page, { op: "automation_run", project: "demo", name: "note", key });
    if (first.ok?.accepted !== true || second.ok?.accepted !== false) throw new Error(`같은 key ${JSON.stringify([first, second])}`);
    await until("둘째 안내", () => aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.subject === "note").length === 2);
    aaa("ticker", "tick");
    aaa("ticker", "tick");
    if (aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.subject === "note").length !== 2) throw new Error("같은 key가 두 번 돌았다");
    if (!item) throw new Error("안내");
    await closeDetail(page);
    // 시작 범위를 승인하지 않은 규칙은 큐에 넣지 않고 거부한다
    const refused = await call(page, { op: "automation_run", project: "demo", name: "go", key: "e2e-go" });
    if (refused.err?.code !== "forbidden" || !refused.err.message.includes("승인")) throw new Error(`미승인 실행 ${JSON.stringify(refused)}`);
    if (rule("go").queue.pending !== 0) throw new Error("미승인 실행이 큐에 들어갔다");
    const off = await call(page, { op: "automation_run", project: "demo", name: "watch", key: "e2e-off" });
    if (off.err?.code !== "bad_request") throw new Error(`꺼진 규칙 실행 ${JSON.stringify(off)}`);
  });

  await step("명령 승인: 웹 확인 창이 명령·머신·hash·routine_commands를 보이고, 승인하면 그 hash가 승인된다. Reader는 편집·실행 단추가 없다", async () => {
    fs.writeFileSync(file("cmd"), '[on.command]\nevery = "1d"\ncommand = "echo e2e"\n[do.inbox]\ntext = "출력 바뀜"\n');
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    const d = await openDetail(page, "cmd");
    await d.getByTestId("automation-command-approval").getByRole("button", { name: "명령 승인 검토", exact: true }).click();
    const confirm = page.getByRole("alertdialog");
    const body = await confirm.textContent();
    const hash = rule("cmd").command_sha256;
    for (const want of ["echo e2e", "host 머신", hash, "routine_commands"]) if (!body.includes(want)) throw new Error(`승인 창 ${want}: ${body}`);
    await shot(page, "automation-edit-command-approve");
    await confirm.getByRole("button", { name: "승인", exact: true }).click();
    await confirm.waitFor({ state: "detached" });
    await until("명령 승인", () => rule("cmd").approved === true);
    await d.getByTestId("automation-command-approval").waitFor({ state: "detached" });
    await closeDetail(page);

    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { touch: true });
    try {
      const p = dev.page;
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await openPanel(p);
      if (await p.getByTestId("automation-new-demo").count()) throw new Error("Reader 새 자동화 단추");
      const rd = await openDetail(p, "note");
      if (await rd.getByTestId("automation-detail-actions").count()) throw new Error("Reader 편집·실행 단추");
      const forbidden = await call(p, { op: "automation_save", project: "demo", name: "note", text: "x", expected_sha256: sha(fs.readFileSync(file("note"), "utf8")) });
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 저장 ${JSON.stringify(forbidden)}`);
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await dev.page.context().close(); }

    // 휴대폰 조작 기기: 편집 창이 폭 안에 있다
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
    try {
      const p = phone.page;
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await openPanel(p);
      await p.getByTestId("automation-new-demo").click();
      const sheet = p.getByTestId("automation-editor");
      await sheet.getByTestId("automation-form-name").waitFor();
      await sheet.getByTestId("automation-form-condition").selectOption("git");
      await sheet.getByTestId("automation-form-repo").waitFor();
      const fit = await fits(p, '[data-testid="automation-editor"]');
      if (fit !== true) throw new Error(`휴대폰 편집 창 폭 ${fit}`);
      await shot(p, "automation-edit-phone");
      if (phone.errors.length) throw new Error(phone.errors.join("\n"));
    } finally { await phone.page.context().close(); }
  });
}
