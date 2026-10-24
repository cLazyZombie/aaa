// 자동화 만들기·고치기·지우기·되살리기·지금 실행·명령 승인(관리 GUI W2, plan §8.342). 실제 host가 폼 값을 원문 TOML에 합치고
// 본 판(sha256)일 때만 쓴다. 웹은 사람의 고침과 결정만 보낸다. 시간은 기다리지 않는다: 파일 글·routine 기록·inbox·화면 표지를 본다.
// 「저장만으로는 돌지 않음」은 저절로 돌지 않는 thread 사건 규칙을 저장한 뒤 ticker 울타리(`ticker tick`) 뒤에 기록 0건으로 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, step, browser, pairPage, until, domIs, inTime, drawerSettled, shot, webkit } from "../lib/harness.mjs";

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
    await tomlHas(page, sheet, "[trigger]");
    const text = await sheet.getByTestId("automation-toml").inputValue();
    for (const want of ["#:schema ./automation.schema.json", 'title = "실패 확인"', 'events = ["thread_failed"]', "schema = 2", 'source = "thread"', "[action]", 'kind = "notice"', "usage_session_below = 5", "usage_week_below = 10"]) if (!text.includes(want)) throw new Error(`새 원문 ${want}:\n${text}`);
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

  await step("일정의 열린 일반 thread 조건: 폼·TOML·검토·저장·다시 열기·CLI", async () => {
    await page.getByTestId("automation-new-demo").click();
    const sheet = page.getByTestId("automation-editor");
    await sheet.waitFor();
    await sheet.getByTestId("automation-form-name").fill("active-check");
    await sheet.getByTestId("automation-form-body").fill("열린 작업을 점검한다");
    await sheet.getByTestId("automation-form-min-open-threads").fill("1");
    await tomlHas(page, sheet, "min_open_threads = 1");
    await sheet.getByTestId("automation-tab-review").click();
    await until("검토에 열린 thread 조건", async () => (await sheet.getByTestId("automation-review").textContent()).includes("열린 일반 thread 1개 이상(자동화·완료·보류 제외)"));
    await sheet.getByTestId("automation-save").click();
    await sheet.waitFor({ state: "detached" });
    const detail = page.getByTestId("automation-detail");
    await detail.waitFor();
    const doc = aaa("automation", "show", "active-check");
    if (!doc.doc.text.includes("min_open_threads = 1")) throw new Error(`CLI show 조건 누락: ${JSON.stringify(doc)}`);
    if (!rule("active-check").trigger.includes("열린 일반 thread 1개 이상")) throw new Error("CLI 목록 조건 누락");
    await detail.getByRole("button", { name: "고치기", exact: true }).click();
    await sheet.waitFor();
    if (await sheet.getByTestId("automation-form-min-open-threads").inputValue() !== "1") throw new Error("다시 연 폼 조건 누락");
    await sheet.getByTestId("automation-form-min-open-threads").fill("");
    await tomlHas(page, sheet, "[trigger]");
    if ((await sheet.getByTestId("automation-toml").inputValue()).includes("min_open_threads")) throw new Error("빈 조건 제거 실패");
    await sheet.getByTestId("automation-save").click();
    await sheet.waitFor({ state: "detached" });
    await detail.waitFor();
    await closeDetail(page);
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

  await step("시스템 idle-audit: 웹은 상태·원문·기록만 읽고 직접 변경 API도 거부하며 CLI는 유지한다", async () => {
    const text = 'title = "운영 점검"\n[on.schedule]\nevery = "10m"\n[do.inbox]\ntext = "운영 상태를 점검한다"\n';
    fs.writeFileSync(file("idle-audit"), text);
    const check = async (p, label) => {
      await p.getByTestId("automations").getByRole("button", { name: "다시 읽기", exact: true }).click();
      const card = p.getByTestId("automation-demo-idle-audit");
      await card.getByText("시스템 · 읽기 전용", { exact: true }).waitFor();
      const d = await openDetail(p, "idle-audit");
      await d.getByTestId("automation-read-only").waitFor();
      if (!(await d.getByTestId("automation-read-only").textContent()).includes("CLI나 파일 편집")) throw new Error("읽기 전용 이유 없음");
      if (await d.getByTestId("automation-detail-actions").count()) throw new Error("시스템 자동화 쓰기 단추");
      if (!(await d.getByRole("switch").isDisabled())) throw new Error("시스템 켜기 스위치 활성");
      await until("시스템 원문 표시", async () => (await d.getByTestId("automation-source").textContent()).includes("운영 상태를 점검한다"));
      await d.getByTestId("automation-runs").waitFor();
      const fit = await fits(p, '[data-testid="automation-detail"]');
      if (fit !== true) throw new Error(`시스템 상세 폭 ${fit}`);
      await shot(p, `automation-system-${label}`);
      for (const req of [
        { op: "automation_save", text: text.replace("운영 점검", "웹 변경"), expected_sha256: sha(text) },
        { op: "routine_toggle", on: false },
        { op: "automation_delete", expected_sha256: sha(text) },
        { op: "automation_run", key: `web-${label}` },
        { op: "routine_approve", command_sha256: "unseen" },
        { op: "automation_approve", approval_sha256: "unseen" },
        { op: "automation_unblock", batch: "unseen", discard: true },
      ]) {
        const reply = await call(p, { project: "demo", name: "idle-audit", ...req });
        if (reply.err?.code !== "forbidden" || !reply.err.message.includes("시스템 자동화")) throw new Error(`시스템 ${req.op}: ${JSON.stringify(reply)}`);
        if (fs.readFileSync(file("idle-audit"), "utf8") !== text) throw new Error("거부한 요청이 원문 변경");
      }
      await closeDetail(p);
    };
    await check(page, "desktop");
    const wk = await webkit.launch({ headless: true });
    try {
      const phone = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      try {
        const p = phone.page;
        await p.locator(".view [data-testid='drawer-toggle']").click();
        await drawerSettled(p);
        await openPanel(p);
        await check(p, "phone");
        if (phone.errors.length) throw new Error(phone.errors.join("\n"));
      } finally { await phone.page.context().close(); }
    } finally { await wk.close(); }
    if (runs("idle-audit").length) throw new Error("웹 수동 실행이 기록 생성");
    // Local CLI 경로는 꺼짐·켜짐·삭제·복원까지 실제로 동작한다.
    aaa("routine", "toggle", "idle-audit", "--off");
    if (rule("idle-audit").enabled) throw new Error("CLI 끄기 거부");
    aaa("routine", "toggle", "idle-audit", "--on");
    aaa("automation", "delete", "idle-audit");
    const trash = aaa("routine", "list").trash.find((t) => t.name === "idle-audit");
    if (!trash?.web_read_only_reason) throw new Error("시스템 휴지통 표지 없음");
    const rejected = await call(page, { op: "automation_restore", project: "demo", id: trash.id });
    if (rejected.err?.code !== "forbidden") throw new Error(`시스템 웹 복원: ${JSON.stringify(rejected)}`);
    aaa("automation", "restore", trash.id);
    if (rule("idle-audit").enabled) throw new Error("CLI 꺼진 채 복원 실패");
  });

  await step("초기 검사: 이름 칸은 첫 검사 뒤 열리고 두 번째 자동화도 입력·저장 중 같은 칸을 유지한다", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      for (const engine of [browser, wk]) for (const width of [1280, 390]) {
        let release;
        let reached;
        let completed;
        let routeError;
        let held = false;
        const gate = new Promise((done) => { release = done; });
        const checking = new Promise((done) => { reached = done; });
        const handled = new Promise((done) => { completed = done; });
        const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
          serviceWorkers: "block",
          init: () => {
            // load가 setLoading을 호출한 바로 뒤, React가 그 변경을 그리기 전의 실제 DOM이다.
            // 관측만 한다. 입력·effect·타이머·host 응답 내용을 바꾸지 않는다.
            window.__automationInitialInputs = [];
            const fetch_ = window.fetch;
            window.fetch = function (url, options) {
              if (typeof options?.body === "string" && JSON.parse(options.body).op === "automation_validate") {
                window.__automationInitialInputs.push(!!document.querySelector('[data-testid="automation-form-name"]'));
              }
              return fetch_.call(this, url, options);
            };
          },
          route: ["**/api/v1/call", async (route) => {
            if (held || route.request().postDataJSON()?.op !== "automation_validate") return route.fallback();
            held = true;
            try {
              const response = await route.fetch();
              reached();
              await gate;
              await route.fulfill({ response });
            } catch (e) { routeError = e; reached(); }
            finally { completed(); }
          }],
        });
        const p = dev.page;
        const label = `${engine.browserType().name()}-${width}`;
        try {
          await p.evaluate(() => { location.hash = "#automation=demo%2F"; });
          await inTime(checking, 20_000, `${label} 초기 검사 응답`);
          if (routeError) throw routeError;
          const editor = p.getByTestId("automation-editor");
          await editor.getByRole("status").getByText("읽는 중…").waitFor();
          const firstExposed = await p.evaluate(() => window.__automationInitialInputs[0]);
          if (firstExposed !== false) throw new Error(`${label}: 초기 검사 전에 이름 칸 노출(${firstExposed})`);
          if (await editor.getByTestId("automation-form-name").count() || !(await editor.getByTestId("automation-save").isDisabled())) throw new Error(`${label}: 초기 검사 중 편집 가능`);
          release();
          await inTime(handled, 20_000, `${label} 초기 검사 전달`);
          if (routeError) throw routeError;
          // 같은 화면에서 두 번 연다(E2E82 자동화① → 다른 화면 → 자동화②).
          for (let n = 1; n <= 2; n++) {
            if (n === 2) await p.evaluate(() => { location.hash = "#automation=demo%2F"; });
            const name = `name-${label}-${n}`;
            const input = editor.getByTestId("automation-form-name");
            await input.fill(name);
            const original = await input.elementHandle();
            try {
              await editor.getByTestId("automation-form-title").fill(`확인 글 ${n}`);
              await editor.getByTestId("automation-form-condition").selectOption("thread");
              await editor.getByTestId("automation-form-body").fill(`입력 보존 ${n}`);
              await tomlHas(p, editor, `text = "입력 보존 ${n}"`);
              if (!(await original.evaluate((el) => el.isConnected && el === document.querySelector('[data-testid="automation-form-name"]')))) throw new Error(`${label}: 입력 중 이름 칸 교체`);
              if ((await input.inputValue()) !== name) throw new Error(`${label}: 이름 입력 유실`);
              await editor.getByTestId("automation-save").click();
              await editor.waitFor({ state: "detached" });
              await p.getByTestId("automation-detail").waitFor();
              const text = fs.readFileSync(`${root}/projects/demo/automations/${name}.toml`, "utf8");
              if (!text.includes(`title = "확인 글 ${n}"`) || !text.includes(`text = "입력 보존 ${n}"`)) throw new Error(`${label}: 저장 내용 유실`);
              await p.evaluate(() => { location.hash = "#threads"; });
              await p.locator('[data-testid="automation-view"][data-shown="false"]').waitFor({ state: "attached" });
            } finally { await original.dispose(); }
          }
          if (dev.errors.length) throw new Error(dev.errors.join("\n"));
        } finally {
          release();
          if (held) await inTime(handled, 20_000, `${label} 초기 검사 정리`);
          await p.context().close();
        }
      }
    } finally { await wk.close(); }
  });

}
