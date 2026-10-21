// 왼쪽 활동 막대(사용자 요청 10-19 「vs code 처럼 … 같은 버튼 한번 더 누르면 숨겨지고」, plan §8.240): 패널 바꾸기·다시 눌러 접기·
// 기기별 기억·키보드·Mod+Shift+B·단축키·팔레트·알림이 연 줄의 패널·보고서 본문 화면·설정 톱니·휴대폰 drawer·막대와 바닥 줄 배치(§8.248),
// 사이드바 폭 손잡이(사용자 요청 10-19 「왼쪽 창 … 좌우 resize」, plan §8.257).
import { root, aaa, step, browser, pairPage, claudeSh, claudeAsk, claudeIdle, waitOutput, domIs, hashIs, focusIs, inList, until, drawerSettled, shot, openSettings } from "../lib/harness.mjs";
import fs from "node:fs";

/** 패널·접힘·고른 단추를 한 번에 읽는다 */
const sidebar = (p) =>
  p.evaluate(() => ({
    panel: [...document.querySelectorAll("#sidebar-column [data-panel]")].filter((e) => !e.hidden).map((e) => e.dataset.panel).join(","),
    remembered: document.querySelector(".app")?.dataset.sidebar,
    collapsed: document.querySelector(".app")?.classList.contains("sidebar-collapsed"),
    column: getComputedStyle(document.getElementById("sidebar-column")).display,
    selected: [...document.querySelectorAll('.activity-bar [role="tab"][aria-selected="true"]')].map((e) => e.dataset.testid).join(","),
    title: document.querySelector('[data-testid="sidebar-title"]')?.textContent,
  }));
const same = (got, want, what) => {
  for (const [k, v] of Object.entries(want)) if (got[k] !== v) throw new Error(`${what}: ${k} ${JSON.stringify(got[k])} ≠ ${JSON.stringify(v)} (${JSON.stringify(got)})`);
};
const sidebarIs = (p, want, what) =>
  domIs(
    p,
    what,
    (want) => {
      const panel = [...document.querySelectorAll("#sidebar-column [data-panel]")].filter((e) => !e.hidden).map((e) => e.dataset.panel).join(",");
      const app = document.querySelector(".app");
      return panel === want.panel && app?.classList.contains("sidebar-collapsed") === want.collapsed;
    },
    want,
  );

export default async function () {
  await step("활동 막대(VS Code activity bar): 단추가 패널 하나를 보이고 다시 누르면 접히며(본문이 넓어짐) 접힌 동안 고른 단추가 없고, ↑↓·Enter, Mod+Shift+B, 새로 열어도 이 기기의 패널·접힘", async () => {
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = dev.page;
    try {
      await p.getByTestId("activity-bar").waitFor();
      await p.evaluate(() => localStorage.setItem("aaa:sidebar-panel","special"));
      await p.reload();
      await sidebarIs(p,{panel:"work",collapsed:false},"옛 special 기억은 작업 패널");
      if (await p.getByTestId("activity-special").count()) throw new Error("특수 단추가 남았다");
      same(await sidebar(p), { panel: "work", collapsed: false, column: "flex", selected: "activity-work", title: "작업" }, "처음");
      // 예전 프로젝트·에이전트 두 패널은 「작업」 하나다(plan §8.280). 「답할 것」은 패널 끝, 보고서 바로 위다(§8.359).
      const tabs = await p.evaluate(() => [...document.querySelectorAll('.activity-bar [role="tab"]')].map((e) => e.dataset.testid).join(","));
      if (tabs !== "activity-work,activity-machines,activity-automations,activity-inbox,activity-asks") throw new Error(`패널 단추: ${tabs}`);
      await p.getByTestId("pane-t1").waitFor();
      const viewWidth = () => p.evaluate(() => Math.round(document.querySelector(".view").getBoundingClientRect().width));
      const wide = await viewWidth();
      // 다른 패널
      await p.getByTestId("activity-machines").click();
      await sidebarIs(p, { panel: "machines", collapsed: false }, "머신 패널");
      same(await sidebar(p), { selected: "activity-machines", title: "머신" }, "머신 패널");
      await p.getByTestId("machine-labA").waitFor();
      await p.getByTestId("activity-automations").click();
      await sidebarIs(p, { panel: "automations", collapsed: false }, "자동화 패널");
      // 같은 단추를 다시 누르면 접힌다: 막대만 남고 본문이 사이드바 폭만큼 넓어진다
      await p.getByTestId("activity-automations").click();
      await sidebarIs(p, { panel: "automations", collapsed: true }, "접힘");
      same(await sidebar(p), { column: "none", selected: "" }, "접힘");
      if ((await viewWidth()) - wide !== 320) throw new Error(`접어도 본문 폭 ${wide} → ${await viewWidth()}`);
      await shot(p, "activity-collapsed");
      // 접힌 동안 다른 단추는 그 패널로 편다
      await p.getByTestId("activity-work").click();
      await sidebarIs(p, { panel: "work", collapsed: false }, "펼침");
      // 키보드(roving tabindex): ↑↓는 focus만 옮기고 Enter가 고른다, Home·End
      await p.getByTestId("activity-work").focus();
      await p.keyboard.press("ArrowDown");
      await focusIs(p, "↓ 머신 단추", '[data-testid="activity-machines"]');
      same(await sidebar(p), { panel: "work" }, "↓만으로는 바꾸지 않는다");
      await p.keyboard.press("Enter");
      await sidebarIs(p, { panel: "machines", collapsed: false }, "Enter로 머신");
      await p.keyboard.press("End");
      await focusIs(p, "End 답할 것 단추", '[data-testid="activity-asks"]');
      await p.keyboard.press("Home");
      await focusIs(p, "Home 작업 단추", '[data-testid="activity-work"]');
      const tabStops = await p.evaluate(() => [...document.querySelectorAll('.activity-bar [role="tab"]')].map((e) => ({ id: e.dataset.testid, stop: e.tabIndex })));
      if (!tabStops.some((t) => t.id === "activity-machines") || tabStops.some((t) => t.stop !== (t.id === "activity-machines" ? 0 : -1))) throw new Error(`Tab이 머무는 단추가 고른 패널 하나가 아니다: ${JSON.stringify(tabStops)}`);
      // Mod+Shift+B(참고 toggle-sidebar): 터미널에 focus가 있어도 앱이 가져가 접고 편다
      await p.getByTestId("terminal").click();
      await p.keyboard.press("ControlOrMeta+Shift+B");
      await sidebarIs(p, { panel: "machines", collapsed: true }, "Mod+Shift+B 접기");
      // 새로 열어도 이 기기의 패널·접힘 그대로(localStorage)
      await p.reload();
      await p.getByTestId("activity-bar").waitFor();
      await sidebarIs(p, { panel: "machines", collapsed: true }, "새로 연 화면");
      await p.keyboard.press("ControlOrMeta+Shift+B");
      await sidebarIs(p, { panel: "machines", collapsed: false }, "Mod+Shift+B 펴기");
      // 다른 기기(새 context)는 기본값
      const other = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
      try {
        await other.page.getByTestId("activity-bar").waitFor();
        same(await sidebar(other.page), { panel: "work", collapsed: false }, "다른 기기");
      } finally {
        await other.page.context().close();
      }
    } finally {
      await p.context().close();
    }
  });

  await step("열린 줄의 패널(VS Code explorer autoReveal): 단축키·팔레트·알림이 연 pane은 그 줄이 있는 「작업」 패널로 바꾸고, 접어 둔 사이드바는 펴지 않으며, 목록 줄을 누르면 그대로다. 「작업」 배지는 확인 필요·검토 대기 thread와 답을 기다리는 agent 수(예전 두 배지의 합집합)", async () => {
    const ask = aaa("tab", "create", "--machine", "labA", "--label", "asker", "--", claudeSh, "-c", `stty -echo; ${claudeAsk("make test")}; echo ask-ready; exec cat > /dev/null`).tabs.at(-1).root.pane;
    const idle = aaa("tab", "create", "--machine", "labA", "--label", "calm", "--", claudeSh, "-c", `stty -echo; ${claudeIdle}; echo idle-ready; exec cat > /dev/null`).tabs.at(-1).root.pane;
    await waitOutput(ask, "ask-ready");
    await waitOutput(idle, "idle-ready");
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = dev.page;
    try {
      await p.getByTestId("pane-t1").waitFor();
      await domIs(p, "작업 배지 1", () => document.querySelector('[data-testid="activity-badge-work"]')?.textContent === "1");
      const label = await p.getByTestId("activity-work").getAttribute("aria-label");
      if (label !== "작업, 확인 필요·검토 대기·답을 기다림 1") throw new Error(`배지 이름: ${label}`);
      // 머신 패널에서 다음 패널(Mod+Shift+↓) → 그 줄이 있는 작업 패널
      await p.getByTestId("activity-machines").click();
      await sidebarIs(p, { panel: "machines", collapsed: false }, "머신 패널");
      await p.keyboard.press("ControlOrMeta+Shift+ArrowDown");
      await sidebarIs(p, { panel: "work", collapsed: false }, "단축키가 연 줄의 패널");
      // 자동화 패널에서 팔레트로 agent pane·셸을 열면 둘 다 작업 패널로(예전 에이전트 패널의 줄도 이 트리에 있다)
      await p.getByTestId("activity-automations").click();
      await sidebarIs(p, { panel: "automations", collapsed: false }, "자동화 패널");
      const palette = async (query) => {
        await p.keyboard.press("ControlOrMeta+Shift+K");
        const box = p.getByTestId("command-palette");
        const search = box.getByRole("searchbox", { name: "패널과 동작 검색" });
        await search.waitFor();
        await focusIs(p, "팔레트 검색 칸", '[data-testid="command-palette"] input[type="search"]');
        await search.fill(query);
        await until(`${query}가 첫 줄`, async () => (await box.getByRole("option").first().innerText()).includes(query));
        await p.keyboard.press("Enter");
        await box.waitFor({ state: "detached" });
      };
      await palette("calm");
      await hashIs(p, `#pane=${encodeURIComponent(idle)}`);
      await sidebarIs(p, { panel: "work", collapsed: false }, "팔레트가 연 agent");
      await p.getByTestId("activity-machines").click();
      await sidebarIs(p, { panel: "machines", collapsed: false }, "머신 패널");
      await palette("t1");
      await sidebarIs(p, { panel: "work", collapsed: false }, "팔레트가 연 셸");
      // 앱 안 알림(Droplet)은 주소를 바꾼다: 머신 패널을 접어 둔 채면 펴지 않고 패널만 바꾼다(다음에 펴면 그 패널)
      await p.getByTestId("activity-machines").click();
      await p.getByTestId("activity-machines").click();
      await sidebarIs(p, { panel: "machines", collapsed: true }, "머신 패널 접음");
      await p.evaluate((id) => (location.hash = `pane=${encodeURIComponent(id)}`), ask);
      await sidebarIs(p, { panel: "work", collapsed: true }, "알림이 연 줄: 접힌 채 패널만");
      // 목록 줄 누름은 그 패널 그대로(작업 패널 「기타」의 agent 줄)
      await p.keyboard.press("ControlOrMeta+Shift+B");
      await sidebarIs(p, { panel: "work", collapsed: false }, "펼침");
      await p.getByTestId("pane-calm").click();
      await hashIs(p, `#pane=${encodeURIComponent(idle)}`);
      same(await sidebar(p), { panel: "work" }, "목록 줄");
      await shot(p, "activity-work");
    } finally {
      await p.context().close();
      for (const id of [ask, idle]) aaa("pane", "kill", id);
    }
  });

  await step("보고서 본문 화면(사용자 요청 10-19 「보고서 누르면 오른쪽 창이 보고서로」): 막대 「보고서」가 본문을 목록으로 바꾸고 다시 누르면 앞 화면, 연 보고서·iframe 안 굴린 자리·목록 굴린 자리는 다른 화면을 다녀와도 그대로, 설정 톱니는 보고서 쪽 없는 설정 창", async () => {
    aaa("project", "create", "rv");
    const dir = `${root}/rv-report`;
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(`${dir}/index.html`, `<!doctype html><meta charset=utf-8><h1>긴 보고서</h1>${Array.from({ length: 200 }, (_, i) => `<p>줄 ${i}</p>`).join("")}`);
    // 「내 보고서」 탭의 카드(plan §8.256)
    for (let i = 0; i < 14; i++) aaa("library", "publish", dir, "--project", "rv", "--name", `r${String(i).padStart(2, "0")}`, "--to-user");
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = dev.page;
    try {
      await p.getByTestId("pane-t1").click();
      const t1 = await p.evaluate(() => location.hash);
      await p.getByTestId("activity-reports").click();
      await hashIs(p, "#reports");
      await domIs(p, "보고서 목록", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
      if ((await p.getByTestId("activity-reports").getAttribute("aria-pressed")) !== "true") throw new Error("보고서 단추가 눌린 표시가 아니다");
      // 다시 누르면 앞 화면(t1)
      await p.getByTestId("activity-reports").click();
      await hashIs(p, t1);
      await p.getByTestId("terminal").waitFor();
      await p.getByTestId("activity-reports").click();
      await hashIs(p, "#reports");
      // 목록을 굴리고 하나를 연다 → iframe 안을 굴린다
      const list = p.getByTestId("reports-scroll");
      await list.evaluate((el) => (el.scrollTop = 600));
      const listTop = await list.evaluate((el) => el.scrollTop);
      if (listTop < 300) throw new Error(`목록이 굴러가지 않는다(${listTop}): 시험이 안 된다`);
      // 굴린 자리를 바꾸지 않고 누른다(click()은 단추를 화면 안으로 굴린다)
      await p.getByTestId("report-rv-r10").getByRole("button", { name: "열기", exact: true }).dispatchEvent("click");
      await hashIs(p, `#report=${encodeURIComponent("rv/r10")}`);
      const frameEl = p.getByTestId("report-frame");
      await frameEl.waitFor();
      const frame = await (await frameEl.elementHandle()).contentFrame();
      await frame.getByRole("heading", { name: "긴 보고서" }).waitFor();
      await frame.evaluate(() => {
        window.__kept = "same document";
        window.scrollTo(0, 1500);
      });
      const inside = await frame.evaluate(() => Math.round(window.scrollY));
      if (inside < 1000) throw new Error(`보고서 안이 굴러가지 않는다(${inside})`);
      // 다른 화면(프로젝트 패널의 pane)으로 갔다가 막대 「보고서」로 돌아오면 그 보고서·그 자리(같은 문서)
      await p.getByTestId("pane-t1").click();
      await hashIs(p, t1);
      await domIs(p, "보고서 화면 숨음", () => document.querySelector('[data-testid="reports-view"]')?.dataset.shown === "false");
      const hidden = await p.evaluate(() => {
        const v = document.querySelector('[data-testid="reports-view"]');
        return { inert: v.inert, visibility: getComputedStyle(v).visibility };
      });
      if (!hidden.inert || hidden.visibility !== "hidden") throw new Error(`숨은 보고서 화면 ${JSON.stringify(hidden)}`);
      await p.getByTestId("activity-reports").click();
      await hashIs(p, `#report=${encodeURIComponent("rv/r10")}`);
      const back = await frame.evaluate(() => ({ kept: window.__kept, y: Math.round(window.scrollY) }));
      if (back.kept !== "same document" || back.y !== inside) throw new Error(`돌아온 보고서 ${JSON.stringify(back)} (떠날 때 ${inside})`);
      await shot(p, "reports-view-open");
      // 목록으로: 굴린 자리 그대로
      await p.getByTestId("report-back").click();
      await hashIs(p, "#reports");
      await domIs(p, "목록 굴린 자리", (top) => document.querySelector('[data-testid="reports-scroll"]')?.scrollTop === top, listTop);
      // 설정 톱니: 설정 창, 보고서 쪽은 없다
      await p.getByTestId("settings-open").click();
      const dialog = p.getByRole("dialog", { name: "설정" });
      await dialog.waitFor();
      if (await dialog.getByRole("tab", { name: "보고서" }).count()) throw new Error("설정에 보고서 쪽이 남았다");
      await p.keyboard.press("Escape");
      await dialog.waitFor({ state: "detached" });
      await focusIs(p, "닫은 뒤 톱니", '[data-testid="settings-open"]');
    } finally {
      await p.context().close();
    }
  });

  await step("휴대폰 활동 막대: drawer 안 왼쪽 세로 막대, 다른 단추는 패널을 바꾸고 보이는 패널의 단추는 drawer를 닫으며, 「보고서」는 drawer를 닫고 본문을 바꾸고 뒤로 가기는 앞 화면", async () => {
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
    const p = dev.page;
    try {
      await inList(p, "pane-t1");
      await hashIs(p, `#pane=${encodeURIComponent(aaa("pane", "list").panes.find((x) => x.name === "t1").id)}`);
      const t1 = await p.evaluate(() => location.hash);
      const toggle = p.locator(".view [data-testid='drawer-toggle']");
      await toggle.click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      const box = await p.evaluate(() => {
        const bar = document.querySelector(".activity-bar").getBoundingClientRect();
        const drawer = document.getElementById("workspace-drawer").getBoundingClientRect();
        return { bar: Math.round(bar.width), left: Math.round(bar.left), drawer: Math.round(drawer.width) };
      });
      if (box.bar !== 48 || box.left !== 0 || box.drawer > Math.round(390 * 0.88) + 1) throw new Error(`drawer 안 막대 ${JSON.stringify(box)}`);
      await p.getByTestId("activity-machines").click();
      await sidebarIs(p, { panel: "machines", collapsed: false }, "휴대폰 머신 패널");
      await shot(p, "phone-activity-machines");
      // 보이는 패널의 단추: drawer를 닫는다(접힘은 넓은 창 것이라 바뀌지 않는다)
      await p.getByTestId("activity-machines").click();
      await domIs(p, "drawer 닫힘", () => !document.querySelector("#workspace-drawer.is-open"));
      same(await sidebar(p), { panel: "machines", collapsed: false }, "닫은 뒤");
      // 보고서: drawer를 닫고 본문이 보고서 목록
      await inList(p, "activity-reports");
      await hashIs(p, "#reports");
      await domIs(p, "drawer 닫힘", () => !document.querySelector("#workspace-drawer.is-open"));
      await p.getByTestId("reports-panel").waitFor();
      await until("보고서 목록이 보인다", () => p.getByTestId("reports-panel").isVisible());
      await shot(p, "phone-reports");
      // 시스템 뒤로: 앞 화면
      await p.goBack();
      await hashIs(p, t1);
      await p.getByTestId("terminal").waitFor();
    } finally {
      await p.context().close();
    }
  });

  await step("사이드바 폭(plan §8.257): 경계 손잡이를 끌면 활동 막대가 아닌 사이드바 칸만 넓어지고 본문이 그만큼 준다. 놓으면 host 공유 설정이라 다른 기기도 같은 폭이고, focus 뒤 ←→ 16px·Home·End, 두 번 누르면 기본 320px. 접힘·좁은 창(drawer)·터치 기기에는 손잡이가 없다", async () => {
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const other = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = dev.page;
    const widths = (page) =>
      page.evaluate(() => {
        const w = (q) => Math.round(document.querySelector(q)?.getBoundingClientRect().width ?? -1);
        const grip = document.querySelector('[data-testid="sidebar-resizer"]');
        return { column: w("#sidebar-column"), bar: w(".activity-bar"), view: w(".view"), grip: grip ? getComputedStyle(grip).display : "없음", now: Number(grip?.getAttribute("aria-valuenow")) };
      });
    const columnIs = (page, want, what) => domIs(page, what, (want) => Math.round(document.getElementById("sidebar-column").getBoundingClientRect().width) === want, want);
    try {
      for (const page of [p, other.page]) await page.getByTestId("pane-t1").waitFor();
      const start = await widths(p);
      same(start, { column: 320, bar: 48, grip: "block", now: 320 }, "처음");
      // 끌기: 손잡이 가운데를 잡고 오른쪽으로 80px
      const box = await p.getByTestId("sidebar-resizer").boundingBox();
      const x = box.x + box.width / 2;
      const y = box.y + box.height / 2;
      await p.mouse.move(x, y);
      await p.mouse.down();
      await p.mouse.move(x + 80, y, { steps: 4 });
      await p.mouse.up();
      await columnIs(p, 400, "끈 폭 400");
      const dragged = await widths(p);
      same(dragged, { bar: 48, view: start.view - 80 }, "끈 뒤 막대·본문");
      // 놓으면 공유 설정: 다른 기기는 preferences 사건으로 같은 폭
      await columnIs(other.page, 400, "다른 기기 폭 400");
      await shot(p, "sidebar-resized");
      // 키보드: focus 뒤 ←는 16px 좁게, End는 이 창의 최대(1280의 절반이 넘는 520), Home은 최소 240
      await p.getByTestId("sidebar-resizer").focus();
      await p.keyboard.press("ArrowLeft");
      await columnIs(p, 384, "← 384");
      await p.keyboard.press("End");
      await columnIs(p, 520, "End 520");
      await columnIs(other.page, 520, "다른 기기 End 520");
      // 좁은 창(drawer)은 제 폭이고 손잡이가 없다
      await p.setViewportSize({ width: 700, height: 800 });
      const narrow = await widths(p);
      if (narrow.grip !== "none" || narrow.column > 320) throw new Error(`좁은 창 ${JSON.stringify(narrow)}`);
      await p.setViewportSize({ width: 1280, height: 800 });
      await columnIs(p, 520, "넓은 창으로 돌아와 520");
      await p.getByTestId("sidebar-resizer").focus();
      await p.keyboard.press("Home");
      await columnIs(p, 240, "Home 240");
      // 접으면 칸과 함께 손잡이도 없다
      await p.keyboard.press("ControlOrMeta+Shift+B");
      await sidebarIs(p, { panel: "work", collapsed: true }, "접힘");
      same(await widths(p), { grip: "none" }, "접힘의 손잡이");
      await p.keyboard.press("ControlOrMeta+Shift+B");
      await sidebarIs(p, { panel: "work", collapsed: false }, "펼침");
      await columnIs(p, 240, "펴도 240");
      // 두 번 누르면 기본 폭이고 공유 값도 지운다(다음 시험은 기본 폭)
      await p.getByTestId("sidebar-resizer").dblclick();
      await columnIs(p, 320, "두 번 눌러 기본 320");
      await columnIs(other.page, 320, "다른 기기도 기본 320");
      // 손가락이 주 입력인 넓은 창(가로 iPad)에도 손잡이가 없다: 경계 옆 입력창 띠 끌기가 폭 바꾸기가 된다(22 「iPad 입력창 끌기」)
      const touch = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { touch: true })).page;
      try {
        await touch.getByTestId("pane-t1").waitFor();
        same(await widths(touch), { column: 320, grip: "none" }, "터치 넓은 창");
      } finally {
        await touch.context().close();
      }
    } finally {
      await p.context().close();
      await other.page.context().close();
    }
  });

  await step("막대·바닥 줄 배치(사용자 요청 10-19 「앱 설치는 설정 안으로, 보고서는 왼쪽 위로(에이전트 밑으로), 사용량은 오른쪽으로」): 보고서는 위 묶음의 패널 단추(마지막 「답할 것」) 바로 밑(패널 tablist 밖), 아래 묶음은 설정만, 바닥 줄에 설치 단추 없이 사용량 칩이 오른쪽 끝, 데스크톱·좁은 창·휴대폰 세로·가로와 기본·GitHub Dark", async () => {
    const VIEWS = [
      ["desktop", { width: 1280, height: 800 }, false],
      ["narrow", { width: 700, height: 800 }, false],
      ["phone", { width: 390, height: 844 }, true],
      ["phone-landscape", { width: 844, height: 390 }, true],
    ];
    const layout = (p) =>
      p.evaluate(() => {
        const box = (s) => document.querySelector(s).getBoundingClientRect();
        // 보고서는 패널 단추 묶음의 마지막 「답할 것」 바로 밑이다(§8.359).
        const lastPanel = box('[data-testid="activity-asks"]');
        const reports = box('[data-testid="activity-reports"]');
        const foot = document.querySelector(".list-foot");
        const footBox = foot.getBoundingClientRect();
        const strip = box('[data-testid="usage-strip"]');
        return {
          reportsGap: Math.round(reports.top - lastPanel.bottom),
          reportsLeft: Math.round(reports.left - lastPanel.left),
          inTablist: document.querySelector('[role="tablist"] [data-testid="activity-reports"]') !== null,
          bottom: [...document.querySelectorAll(".activity-bottom [data-testid]")].map((e) => e.dataset.testid).join(","),
          footButtons: [...foot.querySelectorAll("button")].map((e) => e.dataset.testid ?? e.textContent.trim()).join(","),
          stripRight: Math.round(footBox.right - parseFloat(getComputedStyle(foot).paddingRight) - strip.right),
          stripLeft: Math.round(strip.left - footBox.left - parseFloat(getComputedStyle(foot).paddingLeft)),
        };
      });
    const bad = [];
    for (const theme of ["default", "github-dark"]) {
      for (const [name, viewport, touch] of VIEWS) {
        const p = (await pairPage(browser, aaa("web", "pair").code, viewport, { touch })).page;
        try {
          await p.getByTestId("usage-strip").waitFor({ state: "attached" });
          if (theme === "github-dark" && name === VIEWS[0][0]) {
            // 팔레트는 host 공유 설정이다(plan §8.79): 한 번 고르면 다음 화면들도 그 팔레트로 연다
            const d = await openSettings(p, "모양");
            const sent = p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set");
            await d.locator("#settings-palette").selectOption("github-dark");
            if (!(await sent).ok()) throw new Error("GitHub Dark 저장 실패");
            await p.keyboard.press("Escape");
            await d.waitFor({ state: "detached" });
          }
          await domIs(p, `${name} 팔레트`, (want) => document.documentElement.dataset.palette === want, theme === "default" ? "amber" : theme);
          // 좁은 창·휴대폰 세로는 drawer를 연다(inList는 항목을 눌러 사용량 창을 펼치므로 쓰지 않는다)
          const toggle = p.locator(".view [data-testid='drawer-toggle']");
          if (await toggle.isVisible()) {
            await toggle.click();
            await p.locator("#workspace-drawer.is-open").waitFor();
          }
          await drawerSettled(p);
          await shot(p, `layout-${name}-${theme}`);
          const at = await layout(p);
          const want = { reportsGap: 0, reportsLeft: 0, inTablist: false, bottom: "settings-open", footButtons: "usage-strip", stripRight: 0 };
          const off = Object.entries(want).filter(([k, v]) => at[k] !== v);
          if (off.length || at.stripLeft < 16) bad.push(`${name} ${theme}: ${JSON.stringify(at)}`);
        } finally {
          await p.context().close();
        }
      }
    }
    if (bad.length) throw new Error(bad.join("; "));
  });
}
