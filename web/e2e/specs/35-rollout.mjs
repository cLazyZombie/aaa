// 카드 소비 경계: 실제 Reader 세션/WS에 rollout 사건만 주입한다. 배포 실행기는 L3 m13이 소유한다.
import { aaa, step, browser, pairPage, domIs, inList, shot } from "../lib/harness.mjs";
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
      await inList(p, "activity-work");
      const oldHash = await p.evaluate(() => location.hash);
      peer.send(JSON.stringify({ ev: "notice", kind: "rollout_failed", title: "배포 실패 · 복구 완료", pane: null, machine: "", project: null, thread: null, agent: null, turn_ms: null, long: true, at_ms: Date.now() }));
      await p.getByRole("button", { name: /배포 실패 · 복구 완료,.*패널 열기/ }).click();
      await card.waitFor({ state: "visible" });
      if (await p.evaluate(() => location.hash) !== oldHash) throw new Error("배포 알림이 보던 pane 주소를 바꿨다");
      await shot(p, "rollout-phone-reader");
      await p.setViewportSize({ width: 1280, height: 800 });
      await shot(p, "rollout-desktop-reader");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await p.context().close(); }
  });
}
