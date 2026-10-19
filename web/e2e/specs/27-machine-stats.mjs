// 머신 CPU %·MEM %(사용자 10-19 「웹 UI에 머신에 cpu %, mem % 를 보여줄수 있어? 간략하게」, plan §8.245): 머신 패널 머리줄 오른쪽
// agent·CPU·MEM 아이콘 + 숫자(CPU·MEM은 2px 막대, 사용자 결정 10-19 「1안」, plan §8.254),
// 90%부터 빨강과 접힌 활동 막대의 머신 배지, 옛 agent의 「—」, 끊긴 머신은 그리지 않음.
// 첫 단계는 실제 agent가 기본 주기(점검 60초 사이 15초 표본)로 보낸 값을 기다린다(이 파일이 그 기본값 길이다). 높은 값·옛 agent는
// 실제 agent가 낼 수 없어 상태 사건 WebSocket의 state에 값을 끼운 화면으로 본다.
import { aaa, step, browser, pairPage, showPanel, stop, until, shot } from "../lib/harness.mjs";

/** labA의 agent·CPU·MEM 칸: 값·높음·빈 값·막대·아이콘·title */
const meters = (p) =>
  p.evaluate(() => {
    const box = document.querySelector('[data-testid="machine-stats-labA"]');
    if (!box) return null;
    const one = (k) => {
      const m = box.querySelector(`[data-stat="${k}"]`);
      const bar = m.querySelector(".machine-meter-bar");
      return { value: m.querySelector(".machine-meter-value").textContent, high: m.classList.contains("is-high"), empty: m.classList.contains("is-empty"), fill: bar ? bar.style.getPropertyValue("--fill") : null, icon: m.querySelector(":scope > svg") !== null, title: m.getAttribute("title") };
    };
    return { a: one("agents"), c: one("cpu"), m: one("mem"), title: box.getAttribute("title") };
  });
const badge = (p) => p.evaluate(() => document.querySelector('[data-testid="activity-badge-machines"]')?.textContent ?? null);

export default async function ({ page }) {
  await step("머신 CPU·MEM: 실제 agent가 15초마다 잰 값이 머신 머리줄 오른쪽 아이콘·숫자·막대로 보이고, agent 수는 에이전트 패널 기준으로 세며 title이 풀이한다", async () => {
    await showPanel(page, "machines");
    // CPU는 두 표본 사이 평균이라 첫 점검에는 없고 15초 표본부터 온다. 값만 바뀐 보고도 처음 생긴 값은 host가 화면에 보낸다
    const got = await until("CPU·MEM 값", async () => {
      const v = await meters(page);
      return v && /^\d+$/.test(v.c.value) && /^\d+$/.test(v.m.value) ? v : null;
    }, 40_000);
    for (const k of ["c", "m"]) {
      const n = Number(got[k].value);
      if (n > 100 || got[k].fill !== `${n}%` || got[k].empty || !got[k].icon) throw new Error(`${k} 칸: ${JSON.stringify(got)}`);
    }
    if (got.c.title !== `CPU ${got.c.value}%` || got.m.title !== `메모리 ${got.m.value}%`) throw new Error(`칸 title: ${JSON.stringify(got)}`);
    // agent 칸은 막대가 없고, 에이전트 패널에 보이는 labA agent 수와 같다(이 실험실의 셸 tab은 agent가 아니다)
    await showPanel(page, "agents");
    const listed = await page.evaluate(() => document.querySelectorAll('.agent-list [data-testid^="agent-row-"]').length);
    await showPanel(page, "machines");
    const agents = (await meters(page)).a;
    if (agents.value !== String(listed) || agents.fill !== null || !agents.icon || agents.empty !== (listed === 0)) throw new Error(`agent 칸(패널 ${listed}): ${JSON.stringify(agents)}`);
    if (!/^agent \d+ · 일하는 중 \d+ · 답 기다림 \d+$/.test(agents.title)) throw new Error(`agent title: ${agents.title}`);
    const [agentLine, detail] = got.title.split("\n");
    if (agentLine !== agents.title || !/^CPU \d+% · 메모리 \d+\.\dGB \/ \d+\.\dGB 사용\(\d+%\) · 부하 \d+\.\d\d\/\d+ · \d+초 전$/.test(detail)) throw new Error(`title: ${got.title}`);
    const h = aaa("machine", "list").machines.find((m) => m.name === "labA").health;
    if (typeof h.cpu_pct !== "number" || !(h.mem_used_bytes <= h.mem_total_bytes)) throw new Error(`host 값: ${JSON.stringify(h)}`);
    await shot(page, "desktop-machine-stats");
  });

  // 열린 화면의 state 사건에서 labA 건강을 바꾼다(null이면 그대로)
  let fake = null;
  const route = (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((m) => server.send(m));
    server.onMessage((m) => {
      if (typeof m === "string" && fake) {
        const ev = JSON.parse(m);
        if (ev.ev === "state") {
          for (const mc of ev.machines) if (mc.name === "labA" && mc.health) mc.health = fake(mc.health);
          m = JSON.stringify(ev);
        }
      }
      ws.send(m);
    });
  };
  // 새 탭 하나로 host가 새 목록을 보내게 한다(다음 state에 바꾼 값이 실린다)
  let bumps = 0;
  const bump = () => aaa("tab", "create", "--machine", "labA", "--label", `stats-${++bumps}`, "--", "/bin/sh");
  const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { wsRoute: [/\/api\/v1\/events/, route] });
  const p = dev.page;

  await step("머신 CPU·MEM: 90%부터 그 칸이 빨갛고 접힌 활동 막대의 머신 단추에 배지, 값 칸이 없는 옛 agent는 「—」이고 배지가 없다", async () => {
    fake = (h) => ({ ...h, cpu_pct: 94, mem_used_bytes: 89, mem_total_bytes: 100 });
    bump();
    await showPanel(p, "machines");
    await until("높은 CPU", async () => (await meters(p))?.c.value === "94");
    const v = await meters(p);
    if (!v.c.high || v.m.high || v.m.value !== "89") throw new Error(`90 경계: ${JSON.stringify(v)}`);
    // 머신 단추를 다시 누르면 사이드바가 접히고 머신 줄이 없다: 배지가 남는다
    await p.getByTestId("activity-machines").click();
    await p.locator(".app.sidebar-collapsed").waitFor();
    if ((await badge(p)) !== "1") throw new Error(`배지: ${await badge(p)}`);
    const label = await p.getByTestId("activity-machines").getAttribute("aria-label");
    if (label !== "머신, CPU·MEM 90% 넘음 1") throw new Error(`단추 이름: ${label}`);
    await shot(p, "desktop-machine-stats-collapsed");
    await p.getByTestId("activity-machines").click();
    await p.locator(".app:not(.sidebar-collapsed)").waitFor();

    fake = (h) => {
      const { cpu_pct, mem_used_bytes, mem_total_bytes, ...old } = h;
      return old;
    };
    bump();
    await until("옛 agent", async () => (await meters(p))?.c.value === "—");
    const old = await meters(p);
    if (!(old.c.empty && old.m.empty && old.m.value === "—" && old.title.split("\n")[1].startsWith("CPU 모름 · 메모리 모름") && old.c.title === "CPU 모름(옛 agent)")) throw new Error(`옛 agent: ${JSON.stringify(old)}`);
    if ((await badge(p)) !== null) throw new Error(`값 없는 머신에 배지: ${await badge(p)}`);
  });

  await step("머신 CPU·MEM: 끊긴 머신은 마지막 값이 높아도 막대·배지를 그리지 않는다(오래된 값)", async () => {
    fake = (h) => ({ ...h, cpu_pct: 97, mem_used_bytes: 95, mem_total_bytes: 100 });
    bump();
    await until("높은 값", async () => (await meters(p))?.c.value === "97");
    if ((await badge(p)) !== "1") throw new Error(`끊기기 전 배지: ${await badge(p)}`);
    await stop("agent");
    await p.getByTestId("machine-labA").locator(".machine-state-word", { hasText: "끊김" }).waitFor();
    if ((await meters(p)) !== null) throw new Error(`끊긴 머신에 막대: ${JSON.stringify(await meters(p))}`);
    if ((await badge(p)) !== null) throw new Error(`끊긴 머신에 배지: ${await badge(p)}`);
    await p.context().close();
  });
}
