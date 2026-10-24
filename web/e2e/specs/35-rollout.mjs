// 카드 소비 경계: 실제 Reader 세션/WS에 rollout 사건만 주입한다. 배포 실행기는 L3 m13이 소유한다.
import { aaa, step, browser, webkit, pairPage, domIs, inList, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";
const id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const error = "새 실행 판을 확인하지 못해 실패 머신만 복구했습니다. 앞에서 완료한 머신은 유지합니다. ".repeat(4);
const fixture = () => ({
  id, state: "running", version: "0.1.2+0123456789ab", sha256: "a".repeat(64), commit: "0123456789abcdef0123456789abcdef01234567", cursor: 1,
  error: null, recovery_phase: null, recovery_sha256: null, created_ms: 1000, updated_ms: 42000,
  steps: [
    { machine: "mac-a", phase: "host_restart", completed: true, started_ms: 1000, transferred_bytes: 0, total_bytes: 0 },
    { machine: "mac-b", phase: "transfer", completed: false, started_ms: 2000, transferred_bytes: 524288, total_bytes: 1048576 },
    { machine: "mac-b", phase: "agent_restart", completed: false, started_ms: 0, transferred_bytes: 0, total_bytes: 0 },
  ],
});
export default async function () {
  await step("배포 카드: Reader도 전송·복구·실패를 읽고 재연결 뒤 같은 ID를 보며 조작·잘림·가로 넘침이 없다", async () => {
    let latest = fixture();
    let peer;
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, {
      touch: true,
      wsRoute: [/\/api\/v1\/events$/, (socket) => {
        peer = socket;
        const server = socket.connectToServer();
        let initial = false;
        server.onMessage((raw) => {
          if (typeof raw === "string" && JSON.parse(raw).ev === "rollout") return;
          socket.send(raw);
          if (!initial && typeof raw === "string" && JSON.parse(raw).ev === "state") {
            initial = true;
            socket.send(JSON.stringify({ ev: "rollout", result: { ok: latest } }));
          }
        });
        socket.onMessage((raw) => server.send(raw));
      }],
    });
    const p = dev.page;
    const send = () => peer.send(JSON.stringify({ ev: "rollout", result: { ok: latest } }));
    try {
      await inList(p, "activity-machines");
      await p.getByRole("button", { name: "최근 배포", exact: true }).click();
      const card = p.getByTestId("rollout-card");
      await domIs(p, "배포 전송 진행", () => document.querySelector('[data-testid="rollout-card"]')?.textContent.includes("50%"));
      if (!(await card.textContent()).includes("mac-b")) throw new Error("실패/진행 머신 이름이 없다");
      latest = { ...latest, cursor: 2, steps: latest.steps.map((s, i) => i === 1 ? { ...s, completed: true, transferred_bytes: s.total_bytes } : s), state: "recovering", recovery_phase: "awaiting_old", recovery_sha256: "b".repeat(64), error };
      send();
      await domIs(p, "독립 복구 기다림", () => document.querySelector('[data-testid="rollout-card"]')?.textContent.includes("이전 실행 확인 중"));
      await p.reload();
      const toggle = p.locator(".view [data-testid='drawer-toggle']");
      await toggle.waitFor();
      if (await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
      await p.getByRole("button", { name: "최근 배포", exact: true }).click();
      await card.waitFor({ state: "visible" });
      await domIs(p, "재연결 뒤 같은 배포", () => document.querySelector('[data-testid="rollout-card"]')?.textContent.includes("이전 실행 확인 중"));
      latest = { ...latest, state: "stopped_rolled_back", recovery_phase: "restored" };
      send();
      await domIs(p, "복구 완료와 배포 실패 구분", () => document.querySelector('[data-testid="rollout-card"]')?.textContent.includes("배포 실패 · 복구 완료"));
      if (await card.locator('[aria-current="step"] .rollout-step-state').innerText() !== "복구 완료") throw new Error("실패한 단계의 복구 완료가 없다");
      if (!(await card.textContent()).includes(error)) throw new Error("긴 실패 원인이 잘렸다");
      await card.locator("summary").click();
      for (const value of [id, "a".repeat(64), "b".repeat(64)])
        if (!(await card.textContent()).includes(value)) throw new Error(`배포/검증 신원 누락: ${value}`);
      if (await card.getByRole("button", { name: /배포 시작|중단|되돌리기/ }).count()) throw new Error("Reader 조작 버튼 노출");
      const denied = await p.evaluate(async (id) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "rollout_start", id, spec: { from: "/candidate/aaa", sha256: "a".repeat(64), commit: null, machines: [] } }) })).json(), id);
      if (denied.err?.code !== "forbidden") throw new Error(`Reader 시작 권한: ${JSON.stringify(denied)}`);
      const layout = await card.evaluate((node) => ({ overflow: node.scrollWidth > node.clientWidth + 1, error: getComputedStyle(node.querySelector('[role="alert"]')).webkitLineClamp }));
      if (layout.overflow || !["none", "", "0"].includes(layout.error)) throw new Error(`카드 잘림 ${JSON.stringify(layout)}`);
      await p.getByRole("button", { name: "배포 상세 닫기" }).click();
      await inList(p, "activity-work");
      const oldHash = await p.evaluate(() => location.hash);
      peer.send(JSON.stringify({ ev: "notice", kind: "rollout_failed", title: "배포 실패 · 복구 완료", pane: null, machine: "", project: null, thread: null, agent: null, turn_ms: null, long: true, at_ms: Date.now() }));
      await p.getByRole("button", { name: /배포 실패 · 복구 완료,.*패널 열기/ }).click();
      await p.getByRole("button", { name: "최근 배포", exact: true }).click();
      await card.waitFor({ state: "visible" });
      if (await p.evaluate(() => location.hash) !== oldHash) throw new Error("배포 알림이 보던 pane 주소를 바꿨다");
      await shot(p, "rollout-phone-reader");
      await p.setViewportSize({ width: 1280, height: 800 });
      await shot(p, "rollout-desktop-reader");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await p.context().close(); }
  });
  await step("A안: 두 엔진·두 화면의 상태·실행 판·팝업 초점·줄바꿈·움직임 줄이기", async () => {
    const wk = await webkit.launch();
    try {
      for (const engine of [browser, wk]) for (const width of [390, 1280]) {
        let latest = fixture();
        let peer;
        const dev = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, {
          wsRoute: [/\/api\/v1\/events$/, (socket) => {
            peer = socket;
            const server = socket.connectToServer();
            server.onMessage((raw) => {
              if (typeof raw !== "string") return socket.send(raw);
              const msg = JSON.parse(raw);
              if (msg.ev === "rollout") return socket.send(JSON.stringify({ ev: "rollout", result: { ok: latest } }));
              if (msg.ev === "state") {
                const base = msg.machines[0];
                msg.machines = [
                  ...msg.machines,
                  { ...base, id: "demo-a", name: "mac-a", software: "0.1.5+confirmed", software_differs: false },
                  { ...base, id: "demo-b", name: "mac-b", software: "0.1.4+old", software_differs: true },
                  { ...base, id: "demo-c", name: "mac-c", software: "", connection: "disconnected" },
                  { ...base, id: "demo-d", name: "mac-d", software: "0.1.3+last", connection: "disconnected" },
                  { ...base, id: "demo-e", name: "mac-e", software: "aaa 0.1.6+0123456789ab", software_differs: false },
                ];
                return socket.send(JSON.stringify(msg));
              }
              socket.send(raw);
            });
            socket.onMessage((raw) => server.send(raw));
          }],
        });
        const p = dev.page;
        const recent = p.getByRole("button", { name: "최근 배포", exact: true });
        const dialog = p.getByRole("dialog", { name: "최근 배포" });
        const close = p.getByRole("button", { name: "배포 상세 닫기" });
        const card = p.getByTestId("rollout-card");
        const send = (result) => peer.send(JSON.stringify({ ev: "rollout", result }));
        try {
          await inList(p, "activity-machines");
          await recent.waitFor();
          if (await card.count()) throw new Error("큰 배포 카드가 목록에 남았다");
          await domIs(p, "머신별 실행 판", () => document.querySelector('[data-testid="machine-mac-b"] .machine-software')?.textContent.includes("aaa 0.1.4+old · host와 다름"));
          await domIs(p, "같은 커밋의 짧은 실행 판", () => document.querySelector('[data-testid="machine-mac-e"] .machine-software')?.textContent.trim() === "aaa 0.1.6+0123456789ab");
          await shot(p, `35-software-${engine.browserType().name()}-${width}`);
          for (const [name, text] of [["mac-c", "aaa 판 알 수 없음"], ["mac-d", "마지막 확인"]]) {
            if (!(await p.getByTestId(`machine-${name}`).textContent()).includes(text)) throw new Error(text);
          }
          await recent.click();
          await close.waitFor();
          await domIs(p, "닫기에 초점", () => document.activeElement?.getAttribute("aria-label") === "배포 상세 닫기");
          await p.keyboard.press("Shift+Tab");
          if (await p.evaluate(() => document.activeElement?.tagName) !== "SUMMARY") throw new Error("역방향 Tab 가두기 실패");
          await p.keyboard.press("Tab");
          if (!await close.evaluate((e) => e === document.activeElement)) throw new Error("Tab 가두기 실패");
          await p.keyboard.press("Escape");
          await dialog.waitFor({ state: "detached" });
          if (!await recent.evaluate((e) => e === document.activeElement)) throw new Error("초점 복귀 실패");
          await recent.click();
          for (const state of ["running", "recovering", "stopped", "stopped_rolled_back", "recovery_failed", "done"]) {
            latest = { ...fixture(), state, updated_ms: latest.updated_ms + 1 };
            if (state === "done") latest.steps = latest.steps.map((s) => ({ ...s, completed: true }));
            send({ ok: latest });
            await domIs(p, "배포 상태 반영", (state) => document.querySelector('[data-testid="rollout-card"]')?.dataset.state === state, state);
            if (await card.getByRole("button").count()) throw new Error("읽기 전용 상세에 조작 단추가 있다");
            if (!(await p.getByTestId("machine-mac-b").textContent()).includes("0.1.4+old")) throw new Error("대상 판으로 실행 판을 덮었다");
            const spins = await p.locator(".machine-panel-heading .rollout-spinner").count();
            if (spins !== (["running", "recovering"].includes(state) ? 1 : 0)) throw new Error(`잘못된 회전: ${state}`);
          }
          send({ ok: null });
          await domIs(p, "배포 없음", () => document.querySelector('[data-testid="rollout-card"]')?.textContent.includes("배포 기록이 없습니다"));
          send({ err: { code: "unavailable", message: "상태 조회 · 응답 없음" } });
          await domIs(p, "조회 실패", () => document.querySelector(".machine-panel-heading")?.textContent.includes("조회 실패"));
          if (!(await card.textContent()).includes("상태 조회 · 응답 없음")) throw new Error("조회 오류 유실");
          send({ ok: fixture() });
          await domIs(p, "진행 상태", () => document.querySelector('[data-testid="rollout-card"]')?.dataset.state === "running");
          await p.emulateMedia({ reducedMotion: "reduce" });
          if (await p.locator(".rollout-spinner").first().evaluate((e) => getComputedStyle(e).animationName) !== "none") throw new Error("움직임 줄이기 실패");
          await card.locator("summary").click();
          const box = await dialog.boundingBox();
          if (!box || box.x < 0 || box.x + box.width > width + 1 || await dialog.evaluate((e) => e.scrollWidth > e.clientWidth + 1)) throw new Error("팝업 가로 넘침");
          if (width === 390 && Math.abs(box.y + box.height - 844) > 2) throw new Error("휴대폰 아래 시트가 아니다");
          await shot(p, `35-a-${engine.browserType().name()}-${width}`);
          await p.mouse.click(width - 2, 2);
          await dialog.waitFor({ state: "detached" });
          await recent.click();
          await p.context().setOffline(true);
          peer.close();
          await domIs(p, "끊김 표시", () => document.querySelector(".machine-panel-heading")?.textContent.includes("상태 확인 불가"));
          if (await p.locator(".rollout-spinner").count()) throw new Error("끊긴 상태가 회전한다");
          if (!(await card.textContent()).includes("마지막 수신 상태")) throw new Error("오래된 상태 경고 없음");
          if (dev.errors.length) throw new Error(dev.errors.join("\n"));
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });
}
