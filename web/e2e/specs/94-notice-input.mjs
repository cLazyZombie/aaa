// 실제 앱·PTY에서 알림 교체 중 바로 입력과 대화 초안을 검사한다. 알림만 events WS 경계에 주입한다.
// 성능 수치는 진단 자료다. 회귀 판정은 화면 폭으로 커지지 않는 필터 면적·포커스·정확한 입력/출력이다.
import fs from "node:fs";
import path from "node:path";
import { aaa, root, AA, step, browser, webkit, pairPage, domIs, inList, sentCount, sentFrames, sentText, shot } from "../lib/harness.mjs";

function monitor({ selector, terminal }) {
  const input = document.querySelector(selector);
  const output = document.querySelector(".xterm-rows");
  const state = { keys: [], frames: [], focus: [], tasks: [], stopped: false };
  let previous;
  const frame = (at) => {
    if (state.stopped) return;
    if (previous !== undefined) state.frames.push(at - previous);
    previous = at;
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  const focus = (e) => state.focus.push(e.target.className);
  const key = (e) => {
    if (e.key !== "x") return;
    const sample = { queued: performance.now() - e.timeStamp, at: performance.now() };
    state.keys.push(sample);
    // DOM 반영 뒤 첫 frame. PTY 왕복·xterm의 출력 묶음도 포함한다.
    if (!terminal) requestAnimationFrame(() => sample.display = performance.now() - sample.at);
  };
  const pending = [];
  const rendered = new MutationObserver(() => {
    const count = (output.textContent.match(/x/g) ?? []).length;
    for (const sample of state.keys.slice(0, count)) {
      if (sample.scheduled) continue;
      sample.scheduled = true;
      pending.push(sample);
    }
    if (pending.length) {
      const batch = pending.splice(0);
      requestAnimationFrame(() => {
        for (const sample of batch) sample.display = performance.now() - sample.at;
      });
    }
  });
  if (terminal) rendered.observe(output, { subtree: true, childList: true, characterData: true });
  const tasks = PerformanceObserver.supportedEntryTypes.includes("longtask") ? new PerformanceObserver((list) => {
    state.tasks.push(...list.getEntries().map((e) => e.duration));
  }) : null;
  tasks?.observe({ type: "longtask" });
  document.addEventListener("focusin", focus);
  document.addEventListener("keydown", key, true); // xterm은 bubbling을 멈춘다.
  window.__noticeMeasure = () => {
    state.stopped = true;
    rendered.disconnect();
    tasks?.disconnect();
    document.removeEventListener("focusin", focus);
    document.removeEventListener("keydown", key, true);
    const values = state.keys.map((k) => k.display).filter(Number.isFinite).sort((a, b) => a - b);
    const percentile = (p) => values.length ? values[Math.floor((values.length - 1) * p)] : null;
    return { keys: state.keys.length, displayed: values.length, displayP50: percentile(.5), displayP95: percentile(.95),
      displayMax: percentile(1), frameMax: Math.max(0, ...state.frames), longTasks: state.tasks,
      focus: state.focus, focused: document.activeElement === input, draft: input.value };
  };
}

export default async function () {
  await step("연속 알림 중 입력: 4K·데스크톱·휴대폰의 터미널 전송/표시·대화 초안·포커스와 알림 필터 범위", async () => {
    const transcript = `${root}/claude/projects/-notice/s1.jsonl`;
    fs.mkdirSync(path.dirname(transcript), { recursive: true });
    fs.writeFileSync(transcript, JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "알림 입력 시험" }] } }) + "\n");
    const wk = await webkit.launch();
    try {
      for (const [name, engine, width] of [["chromium-4k", browser, 3840], ["chromium-phone", browser, 390], ["webkit-desktop", wk, 1280], ["webkit-phone", wk, 390]]) {
        const pane = aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c",
          '"$0" hook --agent claude "$1"; stty -echo -icanon; printf "NOTICE_READY\\n"; exec cat', AA,
          JSON.stringify({ hook_event_name: "SessionStart", session_id: name, source: "startup", transcript_path: transcript }));
        let events;
        const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: width === 3840 ? 2160 : 844 }, {
          deviceScaleFactor: width === 3840 ? 2 : 1, serviceWorkers: "block",
          wsRoute: [/\/api\/v1\/events(?:\?|$)/, (socket) => {
            events = socket;
            const server = socket.connectToServer();
            server.onMessage((raw) => socket.send(raw));
            socket.onMessage((raw) => server.send(raw));
          }],
        });
        const p = dev.page;
        try {
          await inList(p, `pane-${name}`);
          if (await p.getByTestId("chat").count()) await p.getByTestId("chat-toggle").click();
          await domIs(p, "알림 시험 PTY 준비", () => document.querySelector(".xterm-rows")?.textContent.includes("NOTICE_READY"));
          const results = [];
          for (const terminal of [true, false]) {
            if (!terminal) {
              await p.getByTestId("chat-toggle").click();
              await p.getByTestId("chat-composer").waitFor();
            }
            const selector = terminal ? ".xterm-helper-textarea" : '[data-testid="chat-composer"] textarea';
            await p.locator(selector).focus();
            await p.evaluate(monitor, { selector, terminal });
            const mark = await sentCount(p);
            let keys = 0;
            for (let round = 0; round < 3; round++) {
              const title = `연속 알림 ${terminal ? "터미널" : "대화"} ${round}`;
              events.send(JSON.stringify({ ev: "notice", kind: "automation", title }));
              // 배지 갱신과 질문/알림함 재조회도 입력 중 같은 연결로 받는다.
              events.send(JSON.stringify({ ev: "reports", unseen: round + 1 }));
              events.send(JSON.stringify({ ev: "asks", requests: 0 }));
              events.send(JSON.stringify({ ev: "inbox" }));
              let ended = false;
              const animation = (async () => {
                await domIs(p, title, (title) => document.querySelector(".droplet-title")?.textContent === title, title);
                const bounds = await p.locator(".droplet").evaluate((el) => {
                  const goo = el.querySelector(".droplet-goo").getBoundingClientRect();
                  const card = el.querySelector(".droplet-card").getBoundingClientRect();
                  return { goo: goo.width, card: card.width, live: el.getAttribute("aria-live"), role: el.getAttribute("role") };
                });
                if (bounds.goo > bounds.card + 1) throw new Error(`${name}: 화면 폭의 알림 필터 ${JSON.stringify(bounds)}`);
                if (bounds.live !== "polite" || bounds.role !== "status") throw new Error(`알림 접근성 ${JSON.stringify(bounds)}`);
                await p.locator(".droplet").evaluate(async (el) => {
                  await Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => {})));
                });
              })().finally(() => ended = true);
              // 새 제목을 기다리는 동안에도 친다: 앞 알림의 접힘과 다음 알림의 등장 모두 검사한다.
              await Promise.all([animation, (async () => {
                do {
                  await p.keyboard.press("x");
                  keys++;
                  // 약 10타/초의 연속 입력. PTY에 매 frame 키를 쌓는 부하 시험과 구분한다.
                  await p.evaluate(async () => {
                    for (let frame = 0; frame < 6; frame++) await new Promise(requestAnimationFrame);
                  });
                } while (!ended);
              })()]);
              if ((await p.getByTestId("activity-badge-reports").textContent()) !== String(round + 1)) throw new Error("입력 중 보고서 배지가 갱신되지 않음");
            }
            if (terminal) {
              const sent = sentText(await sentFrames(p, mark));
              if (sent !== "x".repeat(keys)) throw new Error(`${name}: 터미널 입력 순서/누락 ${sent.length}/${keys}`);
              await domIs(p, "모든 입력이 실제 PTY에서 표시됨", (n) => (document.querySelector(".xterm-rows").textContent.match(/x/g) ?? []).length === n, keys);
            }
            await p.evaluate(() => new Promise(requestAnimationFrame));
            const result = await p.evaluate(() => window.__noticeMeasure());
            results.push({ terminal, ...result });
            if (!result.focused || result.focus.length) throw new Error(`${name}: 알림이 포커스를 바꿈 ${JSON.stringify(result)}`);
            if (result.keys !== keys || (!terminal && result.draft !== "x".repeat(keys))) throw new Error(`${name}: 대화 초안/키 누락 ${JSON.stringify(result)}`);
            await shot(p, `notice-input-${name}-${terminal ? "terminal" : "chat"}`);
            await p.locator(".droplet").waitFor({ state: "detached" });
          }
          console.log(`알림 입력 ${name}: ${JSON.stringify(results)}`);
          if (dev.errors.length) throw new Error(dev.errors.join("; "));
        } finally {
          await p.context().close();
          aaa("pane", "kill", pane.pane?.id ?? name);
        }
      }
    } finally { await wk.close(); }
  });
}
