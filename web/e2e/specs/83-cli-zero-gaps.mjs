// 「CLI 0회」 인수의 작은 빈칸(관리 GUI W8, plan §8.417, 답 1A): 인벤토리에서 웹 화면이 모자랐던 CLI 다섯을 웹으로 한다.
// pane 「세션 끝내기」(`pane kill`: 자리는 남고 「다시 열기」), coordinator 「하나 더 띄우기」(`project open --new`), 「이 aaa」의
// 「낸 링크」 목록과 폐기(`library list`·`revoke`), 새 링크 수명(`library link --days`).
// 결과는 host 기록(CLI 보기)으로 확인한다.
import fs from "node:fs";
import { root, aaa, step, until, domIs, hashIs, openSettings, screenText, inTime } from "../lib/harness.mjs";

const DAY_MS = 86_400_000;
const pane = (name) => aaa("pane", "list").panes.find((p) => p.name === name);
const coordinators = () => aaa("pane", "list").panes.filter((p) => p.role === "coordinator" && p.state === "running");
/** host가 기록한 그 thread의 링크들(새것부터) */
const links = (p, id) =>
  p.evaluate(async (t) => {
    const r = await (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "library_links", project: "demo" }) })).json();
    return r.ok.links.filter((l) => l.thread === t).sort((a, b) => b.created_at_ms - a.created_at_ms);
  }, id);

export default async function ({ page }) {
  // host의 running 응답과 웹의 pane 교체는 다른 완료 사건이다. 실제 replacement state만 보류한다.
  const old = pane("t1").id;
  const held = [];
  let holding = false;
  let releaseState;
  let replacementArrived;
  const replacement = new Promise((resolve) => { replacementArrived = resolve; });
  await page.context().routeWebSocket(/\/api\/v1\/events$/, (socket) => {
    const server = socket.connectToServer();
    socket.onMessage((raw) => server.send(raw));
    server.onMessage((raw) => {
      const event = JSON.parse(String(raw));
      if (holding && event.ev === "state" && event.panes.some((p) => p.replaced?.includes(old))) {
        held.push(raw);
        replacementArrived();
      } else socket.send(raw);
    });
    releaseState = () => { holding = false; for (const raw of held.splice(0)) socket.send(raw); };
  });
  await page.reload();
  await domIs(page, "교체 전 터미널", (id) => document.querySelector('.term:not([hidden])')?.dataset.pane === id, old);
  await step("pane 「세션 끝내기」: 확인 창 → 세션만 끝나고 자리는 남아 「다시 열기」로 다시 뜬다", async () => {
    await page.getByTestId("pane-t1").click({ button: "right" });
    await page.getByRole("menuitem", { name: "세션 끝내기" }).click();
    const ask = page.getByRole("alertdialog", { name: "t1 세션을 끝낼까요?" });
    await ask.getByRole("button", { name: "세션 끝내기" }).click();
    await ask.waitFor({ state: "detached" });
    await until("t1 세션 끝남", () => pane("t1")?.state !== "running");
    await page.getByTestId("pane-t1").click({ button: "right" });
    holding = true;
    await page.getByRole("menuitem", { name: "다시 열기" }).click();
    const reopened = await until("t1 다시 뜸", () => {
      const p = pane("t1");
      return p?.state === "running" && p.id !== old ? p : null;
    });
    await inTime(replacement, 20_000, "새 pane의 실제 state 수신");
    // 이전 종료 조건에서는 이 시점에 다음 단계가 옛 header의 메뉴를 열었다. 실제 state 적용이 그 메뉴를 없앴다.
    const stillOld = await page.locator(".term:not([hidden])").getAttribute("data-pane");
    if (stillOld !== old) throw new Error(`state 보류 전 pane이 바뀜: ${stillOld}`);
    const visible = domIs(page, "웹의 새 pane 교체 완료", (id) =>
      location.hash === `#pane=${id}` && document.querySelector('.term:not([hidden])')?.dataset.pane === id,
      reopened.id);
    // 완료 대기는 시작과 함께 handler를 붙인다. 주소는 replaceState이므로 hashchange만 기다리지 않는다.
    await Promise.all([visible, Promise.resolve().then(() => releaseState())]);
  });

  await step("coordinator 「하나 더 띄우기」: 돌고 있어도 새 coordinator가 뜬다", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "coordinator_profile", "fake");
    await page.getByTestId("more").click();
    // 둘 중 먼저 실패한 대기를 즉시 처리해 단계 진단·실험실 정리를 건너뛰지 않는다.
    await Promise.all([hashIs(page, "#threads"), page.getByTestId("threads-item").click()]);
    const button = page.getByTestId("project-coordinator");
    await button.click();
    await page.getByRole("menuitem", { name: /^fake/ }).click();
    await until("coordinator 하나", () => coordinators().length === 1);
    await until("coordinator 터미널", async () => (await screenText(page)).includes("fake>"));
    await page.goBack();
    await hashIs(page, "#threads");
    await button.click();
    const more = page.getByRole("menuitem", { name: /하나 더 띄우기/ });
    if (!(await more.innerText()).includes("fake")) throw new Error(`하나 더 띄우기 profile: ${await more.innerText()}`);
    await more.click();
    await until("coordinator 둘", () => coordinators().length === 2);
  });

  let id;
  await step("「낸 링크」: 이 aaa에서 만료·마지막 열람을 보고 폐기하면 열려 있던 thread 화면도 폐기를 보인다. 새 링크는 thread 화면에서 고른 수명(7일)으로", async () => {
    fs.writeFileSync(`${root}/gap-task.md`, "링크 시험\n");
    id = aaa("thread", "start", "demo", "--title", "링크 시험", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/gap-task.md`).id;
    // 보고가 오면 host가 30일 링크를 낸다
    const [first] = await until("자동 발급 링크", async () => (await links(page, id)).length === 1 && (await links(page, id)));
    // thread 화면을 연 채 설정에서 폐기한다: 뒤 화면의 링크 줄도 host 사건으로 다시 읽는다(죽은 링크를 복사하지 않게)
    await page.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
    const linkRow = page.getByTestId("library-link");
    await domIs(page, "자동 발급 링크 줄", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "active");
    const dialog = await openSettings(page, "이 aaa");
    const list = dialog.getByTestId("link-list");
    await list.getByTestId("link-list-load").click();
    const row = list.locator(`[data-link="${first.id}"][data-state="active"]`);
    // 앱 안에서 연 보고도 링크의 열람으로 남는다(host `library_opened_at`)
    await row.filter({ hasText: /마지막 열람 \d/ }).waitFor();
    await row.getByRole("button", { name: /링크 폐기$/ }).click();
    await list.locator(`[data-link="${first.id}"][data-state="revoked"]`).waitFor();
    if (!(await links(page, id))[0].revoked) throw new Error("host 기록이 폐기되지 않았다");
    await dialog.getByRole("button", { name: "설정 닫기" }).click();
    await domIs(page, "열려 있던 화면의 폐기된 링크", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "revoked");
    await linkRow.getByTestId("library-link-days").selectOption("7");
    await linkRow.getByRole("button", { name: "새 링크 만들기" }).click();
    await domIs(page, "새 링크", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "active");
    const [made] = await links(page, id);
    if (made.id === first.id || made.expires_at_ms - made.created_at_ms !== 7 * DAY_MS) throw new Error(`새 링크 수명 ${JSON.stringify(made)}`);
  });
}
