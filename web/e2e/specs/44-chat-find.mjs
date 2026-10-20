// 웹 E2E 대화 안 찾기(plan §8.308, t3code ThreadFindBar·useThreadFind·threadFindHighlights·useThreadFindNavigation).
// 실제 lab Claude 기록(사람 말·답·도구 줄·긴 code·긴 사람 말·표, 400줄) → agent → host → 웹. 앞 쪽을 처음까지 읽어 300줄을 넘겨
// 화면 밖 줄의 본문이 없는(가상화) 상태에서 찾는다. 데스크톱 Chromium(⌘F·Enter·Shift+Enter·Esc)과 iPhone WebKit(머리 돋보기)을 본다.
// 기다림은 제품 표지(`data-find-count`·`data-find-active`·`data-find-revealed`, 줄의 `data-find-count`·`data-find-dom`)다.
// 「앞 기록도 찾기」(plan §8.309)는 최신 쪽만 받은 상태에서 처음까지 쪽을 이어 받으며 센다(`data-find-earlier`·`data-find-pages`). Claude·Codex.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 44-chat-find
import fs from "node:fs";
import { aaa, AA, root, step, webkit, browser, inList, domIs, until, shot, pairPage } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export const labEnv = { CODEX_HOME: `${root}/codex` };

const NEEDLE = "ZQ-needle";
const LINES = 400;
const L = (o) => JSON.stringify(o) + "\n";
/** n번째 기록 줄: 짝수는 사람 말, 홀수는 답. 찾을 말은 일곱 곳(도구 줄의 것은 세지 않는다) */
function line(n) {
  // 이어진 도구 호출 둘은 한 묶음(한 화면 줄)이다: 그 뒤 말의 차례와 화면 줄 차례가 어긋난다(plan §8.304)
  if (n === 5) return L({ type: "assistant", message: { content: [{ type: "tool_use", id: "t5", name: "Bash", input: { command: `echo ${NEEDLE}` } }, { type: "tool_use", id: "t5b", name: "Bash", input: { command: "ls" } }] } });
  if (n % 2 === 0) {
    if (n === 12) return L({ type: "user", message: { role: "user", content: [...Array.from({ length: 60 }, (_, i) => `긴 사람 말 ${i}`), `끝에 ${NEEDLE.toLowerCase()}`].join("\n") } });
    return L({ type: "user", message: { role: "user", content: `질문 ${n}: 빌드 오류를 확인해 줘` } });
  }
  const text = n === 3 ? `처음 ${NEEDLE} 하나`
    : n === 9 ? ["긴 code:", "```text", ...Array.from({ length: 40 }, (_, i) => (i === 25 ? `줄 ${i} zq-NEEDLE in tail` : `줄 ${i}`)), "```"].join("\n")
      : n === 21 ? `| 열 | 값 |\n|---|---|\n| 가 | **ZQ**-needle |`
        : n === 201 ? `가운데 Zq-Needle 그리고 *zq*-needle`
          : n === LINES - 1 ? `끝 ${NEEDLE}`
            : `답 ${n}. **빌드 오류**는 \`cargo test\`에서 났다.\n\n- 원인: 경로\n- 다음: 다시 빌드`;
  return L({ type: "assistant", message: { content: [{ type: "text", text }] } });
}
const TOTAL = 7;

/** 찾기 칸 상태 */
const bar = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-testid="chat-find"]');
  return el && { count: el.dataset.findCount ?? null, index: el.dataset.findIndex ?? null, active: el.dataset.findActive ?? null, revealed: el.dataset.findRevealed === "true",
    earlier: el.dataset.findEarlier, pages: el.dataset.findPages, text: el.querySelector('[data-testid="chat-find-count"]').textContent };
});

/** 앞 기록을 처음까지 받아 다 셌다: 개수가 n이고 「+」와 앞 기록 줄이 없고 기록 머리가 「처음이다」 */
async function earlierDone(page, n, what) {
  try {
    await domIs(page, what, (n) => {
      const el = document.querySelector('[data-testid="chat-find"]');
      return el?.dataset.findCount === String(n) && el.dataset.findEarlier === "idle" && !el.querySelector('[data-testid="chat-find-count"]').textContent.endsWith("+")
        && !el.querySelector('[data-testid="chat-find-earlier"]') && document.querySelector('[data-testid="chat-history-start"]')?.textContent === "처음이다";
    }, n);
  } catch (error) {
    throw new Error(`${error.message}: ${JSON.stringify(await bar(page))}`);
  }
}

/** 칠한 활성 찾음이 대화 칸 안에 보이고(위 찾기 칸에 가리지 않고), 그 글이 찾는 말이다 */
async function activeVisible(page, what) {
  const r = await page.evaluate(() => {
    const ranges = [...(CSS.highlights.get("aaa-chat-find-active") ?? [])];
    const chat = document.querySelector('[data-testid="chat"]').getBoundingClientRect();
    const findBar = document.querySelector('[data-testid="chat-find"]').getBoundingClientRect();
    return ranges.map((range) => { const b = range.getBoundingClientRect(); return { text: range.toString(), top: b.top, bottom: b.bottom, height: b.height, chatTop: chat.top, chatBottom: chat.bottom, barBottom: findBar.bottom }; });
  });
  if (r.length !== 1) throw new Error(`${what}: 활성 찾음 ${r.length}개`);
  const [a] = r;
  if (a.text.toLowerCase() !== NEEDLE.toLowerCase()) throw new Error(`${what}: 활성 글 ${JSON.stringify(a.text)}`);
  if (a.height === 0 || a.top < a.barBottom || a.bottom > a.chatBottom) throw new Error(`${what}: 활성 찾음이 보이지 않음 ${JSON.stringify(a)}`);
}

/** 그린 줄마다 데이터로 센 수와 그린 글에서 찾은 수가 같다 */
async function countsAgree(page, what) {
  const rows = await page.evaluate(() => [...document.querySelectorAll(".chat-row[data-find-dom]")].map((r) => ({ id: r.dataset.chatId, count: r.dataset.findCount, dom: r.dataset.findDom })));
  const bad = rows.filter((r) => r.count !== r.dom);
  if (rows.length === 0 || bad.length) throw new Error(`${what}: 줄 개수 어긋남 ${JSON.stringify(bad)} / ${rows.length}`);
}

/** 이동 하나: 표지가 그 차례·보임으로 바뀔 때까지 */
async function moved(page, index, what) {
  try {
    await domIs(page, `${what} ${index + 1}/${TOTAL}`, (i) => {
      const el = document.querySelector('[data-testid="chat-find"]');
      return el?.dataset.findIndex === String(i) && el.dataset.findRevealed === "true";
    }, index);
  } catch (error) {
    throw new Error(`${error.message}: ${JSON.stringify(await bar(page))}`);
  }
  await activeVisible(page, what);
}

export default async function () {
  const label = "find";
  const dir = `${root}/claude/projects/-lab-${label}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s.jsonl`;
  // 기록 줄 수: 「앞 기록도 찾기」 단계가 울타리 말 하나를 더한다
  let rowsTotal = LINES;
  fs.writeFileSync(tr, Array.from({ length: LINES }, (_, n) => line(n)).join(""));
  aaa("tab", "create", "--machine", "labA", "--label", label, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a5b4c3d-2e1f-4a0b-9c8d-7e6f5a4b3c40", source: "startup", transcript_path: tr }));
  await until("harness 기록 연결", () => aaa("pane", "list").panes.find((p) => p.name === label)?.agent?.harness === "claude");

  /** 대화 보기를 열고 앞 쪽을 처음까지 읽은 뒤 끝으로 돌아온다(300줄이 넘어 화면 밖 줄은 본문이 없다) */
  const openChat = async (page, latestOnly = async () => {}) => {
    await inList(page, `pane-${label}`);
    const toggle = page.getByTestId("chat-toggle");
    await toggle.waitFor();
    if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    const chat = page.getByTestId("chat");
    await chat.getByText(`끝 ${NEEDLE}`).waitFor();
    await latestOnly(chat);
    while (!(await chat.getByText("처음이다", { exact: true }).count())) {
      await chat.evaluate((el) => { el.scrollTop = 0; el.dispatchEvent(new Event("scroll", { bubbles: true })); });
      await chat.and(page.locator('[aria-busy="false"]')).waitFor();
    }
    await until("기록 전부", async () => (await chat.locator(".chat-row").count()) === rowsTotal);
    await chat.evaluate((el) => { el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll", { bubbles: true })); });
    await chat.getByText(`끝 ${NEEDLE}`).waitFor();
    const mounted = await chat.locator(".markdown").count();
    if (mounted >= LINES / 2) throw new Error(`가상화가 아니다: 본문 ${mounted}`);
    return chat;
  };

  await step("대화 안 찾기: ⌘F·개수·화면 밖 줄로 이동·접힌 곳 펴기·Esc", async () => {
    const { page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    // 1단계는 받아 둔 범위만 찾는다: 최신 쪽(150줄)에는 하나뿐이고 앞 기록이 남았다는 「+」가 붙는다
    const chat = await openChat(page, async (chat) => {
      await chat.focus();
      await page.keyboard.press("ControlOrMeta+f");
      await page.getByRole("searchbox", { name: "대화에서 찾기" }).fill(NEEDLE);
      await domIs(page, "받은 범위 1/1+", () => document.querySelector('[data-testid="chat-find-count"]')?.textContent === "1/1+");
      await shot(page, "find-desktop-latest");
      await page.keyboard.press("Escape");
      await domIs(page, "앞 기록 전 닫힘", () => document.querySelector('[data-testid="chat-find"]') === null);
    });
    // 대화를 볼 때 ⌘F(Ctrl+F)는 브라우저 찾기가 아니라 이 칸이다. 칸이 focus를 갖는다
    await chat.focus();
    await page.keyboard.press("ControlOrMeta+f");
    const input = page.getByRole("searchbox", { name: "대화에서 찾기" });
    await input.waitFor();
    if (!(await input.evaluate((el) => el === document.activeElement))) throw new Error("찾기 칸에 focus가 없다");
    await input.fill("빌드 오류");
    await domIs(page, "한글 찾기 개수", () => Number(document.querySelector('[data-testid="chat-find"]')?.dataset.findCount) > 100);
    await countsAgree(page, "한글");
    // 대소문자를 가리지 않고, 굵은 글자 사이도 이어 찾고, 도구 줄은 세지 않는다
    await input.fill(NEEDLE.toLowerCase());
    await domIs(page, "찾을 말 개수", (n) => document.querySelector('[data-testid="chat-find"]')?.dataset.findCount === String(n), TOTAL);
    // 끝을 보던 자리에서 시작한다: 보이는 첫 찾음(맨 끝 말)
    await moved(page, TOTAL - 1, "시작");
    await shot(page, "find-desktop-start");
    await countsAgree(page, "시작");
    // 다음은 처음으로 돌아 화면 밖(본문 없는) 맨 앞 줄로 간다
    await page.keyboard.press("Enter");
    await moved(page, 0, "Enter 처음으로");
    await countsAgree(page, "처음");
    // 긴 code의 가린 꼬리: 그 자리로 가면 code가 펴진다
    await page.keyboard.press("Enter");
    await moved(page, 1, "code 꼬리");
    if (!(await chat.getByRole("button", { name: "접기" }).count())) throw new Error("code 꼬리가 펴지지 않았다");
    // 긴 사람 말의 잘린 아래: 그 말이 펴진다
    await page.keyboard.press("Enter");
    await moved(page, 2, "긴 사람 말");
    await chat.locator(".chat-bubble-fold", { hasText: "접기" }).waitFor();
    for (let i = 3; i < TOTAL; i++) {
      await page.keyboard.press("Enter");
      await moved(page, i, "Enter");
      // 한 말에 둘: 활성과 나머지 칠함이 함께 보인다
      if (i === 4) await shot(page, "find-desktop-two");
    }
    await shot(page, "find-desktop-middle");
    await page.keyboard.press("Shift+Enter");
    await moved(page, TOTAL - 2, "Shift+Enter");
    await page.getByRole("button", { name: "다음 찾음" }).click();
    await moved(page, TOTAL - 1, "다음 단추");
    await countsAgree(page, "끝");
    const mounted = await chat.locator(".markdown").count();
    if (mounted >= LINES / 2) throw new Error(`찾기가 본문을 모두 그렸다: ${mounted}`);
    // 없는 말은 0/0, 이동 단추는 꺼진다
    await input.fill("없는말");
    await domIs(page, "0/0", () => document.querySelector('[data-testid="chat-find-count"]')?.textContent === "0/0");
    if (!(await page.getByRole("button", { name: "다음 찾음" }).isDisabled())) throw new Error("0개에서 이동 단추가 켜짐");
    // Esc로 닫으면 칠한 것도 없어지고 focus는 연 자리(대화)로 돌아간다
    await page.keyboard.press("Escape");
    await domIs(page, "찾기 닫힘", () => document.querySelector('[data-testid="chat-find"]') === null && !CSS.highlights.has("aaa-chat-find"));
    if (!(await chat.evaluate((el) => el === document.activeElement))) throw new Error("닫은 뒤 focus가 대화로 돌아가지 않았다");
    // 팔레트 「대화에서 찾기」도 연다
    await page.keyboard.press("ControlOrMeta+Shift+k");
    await page.getByRole("option", { name: /대화에서 찾기/ }).click();
    await input.waitFor();
    await page.keyboard.press("Escape");
    await page.context().close();
  });

  await step("대화 안 찾기: 앞 기록도 찾기·멈춤·이어 찾기", async () => {
    // 앞 쪽 요청(pane_chat before)을 센다. gate가 있으면 첫 요청 하나를 붙잡는다
    const befores = [];
    let gate = null;
    const { page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      route: ["**/api/v1/call", async (route) => {
        const request = route.request().postDataJSON();
        if (request?.op === "pane_chat" && request.before) {
          befores.push(request.before);
          if (gate) {
            const held = gate;
            gate = null;
            held.arrive();
            await held.released;
          }
        }
        await route.continue();
      }],
    });
    await inList(page, `pane-${label}`);
    const toggle = page.getByTestId("chat-toggle");
    await toggle.waitFor();
    if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    const chat = page.getByTestId("chat");
    await chat.getByText(`끝 ${NEEDLE}`).waitFor();
    await chat.focus();
    await page.keyboard.press("ControlOrMeta+f");
    await page.getByRole("searchbox", { name: "대화에서 찾기" }).fill(NEEDLE);
    await domIs(page, "받은 범위 1/1+", () => document.querySelector('[data-testid="chat-find-count"]')?.textContent === "1/1+");
    await moved(page, 0, "받은 범위 시작");
    const latest = await chat.locator(".chat-row").count();
    if (befores.length !== 0 || latest >= LINES) throw new Error(`최신 쪽만이 아니다: 앞 쪽 ${befores.length}, 줄 ${latest}`);
    const earlierButton = page.getByRole("button", { name: "앞 기록도 찾기" });
    await earlierButton.waitFor();
    await shot(page, "find-desktop-earlier");

    // 멈춤: 첫 앞 쪽을 붙잡은 동안 멈추면, 받던 쪽은 붙이고(그 쪽도 센다) 다음 쪽은 부르지 않는다
    let release;
    const arrived = new Promise((arrive) => { gate = { arrive, released: new Promise((r) => { release = r; }) }; });
    await earlierButton.click();
    await arrived;
    await domIs(page, "앞 기록 받는 중", () => document.querySelector('[data-testid="chat-find"]')?.dataset.findEarlier === "loading");
    await shot(page, "find-desktop-earlier-loading");
    await page.getByRole("button", { name: "멈춤" }).click();
    await domIs(page, "멈춤", () => document.querySelector('[data-testid="chat-find"]')?.dataset.findEarlier === "idle");
    release();
    await until("멈춘 뒤 받던 쪽 붙음", async () => (await chat.locator(".chat-row").count()) > latest);
    // 울타리: 새 말을 기록에 더하면 최신 쪽 읽기가 앞 쪽과 같은 한 줄 차례(ConversationRefresh)로 돈다. 그 글이 그려졌으면 멈춤 뒤에
    // 앞 쪽을 또 불렀다면 그 요청이 이미 왔다
    fs.appendFileSync(tr, L({ type: "user", message: { role: "user", content: "울타리 말" } }));
    rowsTotal++;
    await chat.getByText("울타리 말").waitFor();
    if (befores.length !== 1) throw new Error(`멈춘 뒤에도 앞 쪽을 불렀다: ${befores.length}`);
    const stopped = await bar(page);
    if (!stopped.text.endsWith("+") || stopped.pages !== "0") throw new Error(`멈춘 뒤 상태 ${JSON.stringify(stopped)}`);
    await countsAgree(page, "멈춘 뒤");

    // 이어 찾기: 처음까지 받으면 7개, 「+」와 앞 기록 줄이 사라진다. 보던 찾음(끝 말)과 끝 따라가기는 그대로다
    await page.getByRole("button", { name: /앞 기록도 찾기|이어 찾기/ }).click();
    await earlierDone(page, TOTAL, "처음까지 7개");
    const done = await bar(page);
    if (done.index !== String(TOTAL - 1) || done.active !== stopped.active) throw new Error(`받는 동안 보던 찾음이 바뀌었다 ${JSON.stringify({ stopped, done })}`);
    await activeVisible(page, "앞 기록을 다 받은 뒤");
    if (!(await chat.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop <= 1))) throw new Error("앞 기록을 받는 동안 끝 따라가기를 놓았다");
    await countsAgree(page, "처음까지");
    if ((await chat.locator(".chat-row").count()) !== rowsTotal) throw new Error("기록 줄 수가 다르다");
    const mounted = await chat.locator(".markdown").count();
    if (mounted >= LINES / 2) throw new Error(`앞 기록 찾기가 본문을 모두 그렸다: ${mounted}`);
    await shot(page, "find-desktop-earlier-done");
    // 받은 앞 기록의 찾음으로 간다(본문 없는 맨 앞 줄)
    await page.keyboard.press("Enter");
    await moved(page, 0, "앞 기록의 처음");
    await page.context().close();
  });

  await step("대화 안 찾기: iPad Codex 기록 앞 기록도 찾기", async () => {
    const codexLabel = "find-codex";
    const id = "01900000-0000-7000-8000-00000000b486";
    const dir = `${root}/codex/sessions/2026/10/20`;
    fs.mkdirSync(dir, { recursive: true });
    const text = (n) => (n === 1 ? `처음 ${NEEDLE}` : n === LINES - 1 ? `끝 ${NEEDLE}` : `기록 ${n}: 빌드 오류`);
    fs.writeFileSync(`${dir}/rollout-2026-10-20T00-00-00-${id}.jsonl`, Array.from({ length: LINES }, (_, n) => L({ type: "response_item",
      payload: { type: "message", role: n % 2 ? "assistant" : "user", content: [{ type: n % 2 ? "output_text" : "input_text", text: text(n) }] } })).join(""));
    aaa("tab", "create", "--machine", "labA", "--label", codexLabel, "--", "/bin/sh", "-c", '"$0" hook --agent codex "$1"; exec cat', AA,
      JSON.stringify({ type: "agent-turn-complete", "thread-id": id }));
    await until("Codex 기록 연결", () => aaa("pane", "list").panes.find((p) => p.name === codexLabel)?.agent?.harness === "codex");
    // 패드(iPad WebKit 820): 머리 돋보기로 연다
    const wk = await webkit.launch({ headless: true });
    try {
      const { page } = await pairDevice(wk, aaa("web", "pair").code, "ipad");
      await inList(page, `pane-${codexLabel}`);
      const toggle = page.getByTestId("chat-toggle");
      await toggle.waitFor();
      if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
      const chat = page.getByTestId("chat");
      await chat.getByText(`끝 ${NEEDLE}`).waitFor();
      await page.getByTestId("chat-find-open").click();
      await page.getByRole("searchbox", { name: "대화에서 찾기" }).fill(NEEDLE);
      await domIs(page, "Codex 받은 범위 1/1+", () => document.querySelector('[data-testid="chat-find-count"]')?.textContent === "1/1+");
      await shot(page, "find-ipad-codex-latest");
      await page.getByRole("button", { name: "앞 기록도 찾기" }).click();
      await earlierDone(page, 2, "Codex 처음까지 2개");
      await page.getByRole("button", { name: "다음 찾음" }).click();
      await domIs(page, "Codex 처음 찾음", () => {
        const el = document.querySelector('[data-testid="chat-find"]');
        return el?.dataset.findIndex === "0" && el.dataset.findRevealed === "true";
      });
      await activeVisible(page, "Codex 처음 찾음");
      await shot(page, "find-ipad-codex-first");
    } finally {
      await wk.close();
    }
  });

  await step("대화 안 찾기: iPhone 머리 돋보기", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      const { page } = await pairDevice(wk, aaa("web", "pair").code, "iphone");
      await openChat(page, async () => {
        await page.getByTestId("chat-find-open").click();
        await page.getByRole("searchbox", { name: "대화에서 찾기" }).fill(NEEDLE);
        await domIs(page, "iPhone 받은 범위 1/1+", () => document.querySelector('[data-testid="chat-find-count"]')?.textContent === "1/1+");
        await shot(page, "find-iphone-latest");
        // 휴대폰도 앞 기록 줄이 칸 아래 화면 안에 들고, 처음까지 받으면 사라진다
        const earlier = await page.getByTestId("chat-find-earlier").boundingBox();
        if (!earlier || earlier.x < 0 || earlier.x + earlier.width > page.viewportSize().width) throw new Error(`앞 기록 줄이 화면 밖 ${JSON.stringify(earlier)}`);
        await page.getByRole("button", { name: "앞 기록도 찾기" }).click();
        await earlierDone(page, TOTAL, "iPhone 처음까지 7개");
        await shot(page, "find-iphone-earlier-done");
        await page.getByRole("button", { name: "찾기 닫기" }).click();
      });
      await page.getByTestId("chat-find-open").click();
      const input = page.getByRole("searchbox", { name: "대화에서 찾기" });
      await input.waitFor();
      await input.fill(NEEDLE);
      await domIs(page, "찾을 말 개수", (n) => document.querySelector('[data-testid="chat-find"]')?.dataset.findCount === String(n), TOTAL);
      await moved(page, TOTAL - 1, "iPhone 시작");
      await page.getByRole("button", { name: "다음 찾음" }).click();
      await moved(page, 0, "iPhone 처음으로");
      await page.getByRole("button", { name: "다음 찾음" }).click();
      await moved(page, 1, "iPhone code 꼬리");
      await shot(page, "find-iphone");
      // 칸이 머리 아래 화면 안에 들고 단추가 손가락 크기다
      const box = await page.getByTestId("chat-find").boundingBox();
      const view = page.viewportSize();
      if (!box || box.x < 0 || box.x + box.width > view.width) throw new Error(`찾기 칸이 화면 밖 ${JSON.stringify(box)}`);
      await page.getByRole("button", { name: "찾기 닫기" }).click();
      await domIs(page, "iPhone 닫힘", () => document.querySelector('[data-testid="chat-find"]') === null);
    } finally {
      await wk.close();
    }
  });
}
