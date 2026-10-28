// 실제 앱·PTY에서 알림 교체 중 바로 입력과 대화 초안을 검사한다. 알림만 events WS 경계에 주입한다.
// 성능 수치는 진단 자료다. 회귀 판정은 화면 폭으로 커지지 않는 필터 면적·포커스·정확한 입력/출력이다.
import fs from "node:fs";
import path from "node:path";
import { phase } from "../lib/phases.mjs";
import { specCleanup, aaa, root, AA, step, browser, webkit, pairPage, paneHash, domIs, sentCount, sentFrames, sentText, shot } from "../lib/harness.mjs";

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
    const notice = document.querySelector(".droplet");
    if (notice?.getAnimations({ subtree: true }).some(a => a.playState === "running"))
      sample.moving = notice.querySelector(".droplet-title")?.textContent;
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
      moving: [...new Set(state.keys.map(k => k.moving).filter(Boolean))],
      focus: state.focus, focused: document.activeElement === input, draft: input.value };
  };
}

async function check(engine, name, width, terminalMode, transcript) {
  phase("prepare", "pane-context-and-ready-output");
  const pane = aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c",
    '"$0" hook --agent claude "$1"; stty -echo -icanon; printf "NOTICE_READY\\n"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: name, source: "startup", transcript_path: transcript }));
  let events;
  const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: width === 3840 ? 2160 : 844 }, {
    // drawer 전환은 E39가 맡는다. 같은 실제 pane을 처음부터 연다.
    hash: paneHash(name),
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
    if (await p.getByTestId("chat").count()) await p.getByTestId("chat-toggle").click();
    await domIs(p, "알림 시험 PTY 준비", () => document.querySelector(".xterm-rows")?.textContent.includes("NOTICE_READY"));
    phase("contract", "notice-animation-and-continuous-input");
    const results = [];
    for (const terminal of [terminalMode]) {
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
        phase("contract", `notice-round-${round}`);
        const title = `연속 알림 ${terminal ? "터미널" : "대화"} ${round}`;
        // 같은 대상/종류는 400ms 중복 방지가 있다. 서로 다른 두 실제 종류를
        // 번갈아 보내 등장 중 교체를 검사하며 그 정책이나 시계를 바꾸지 않는다.
        events.send(JSON.stringify({ ev: "notice", kind: round === 1 ? "post_muted" : "automation", title }));
        // 배지 갱신과 질문/알림함 재조회도 입력 중 같은 연결로 받는다.
        events.send(JSON.stringify({ ev: "reports", unseen: round + 1 }));
        events.send(JSON.stringify({ ev: "asks", requests: 0 }));
        events.send(JSON.stringify({ ev: "inbox" }));
        let ended = false;
        const typed = Promise.withResolvers();
        const animation = (async () => {
          await domIs(p, title, (title) => document.querySelector(".droplet-title")?.textContent === title, title);
          const bounds = await p.locator(".droplet").evaluate((el) => {
            const goo = el.querySelector(".droplet-goo").getBoundingClientRect();
            const card = el.querySelector(".droplet-card").getBoundingClientRect();
            return { goo: goo.width, card: card.width, live: el.getAttribute("aria-live"), role: el.getAttribute("role") };
          });
          if (bounds.goo > bounds.card + 1) throw new Error(`${name}: 화면 폭의 알림 필터 ${JSON.stringify(bounds)}`);
          if (bounds.live !== "polite" || bounds.role !== "status") throw new Error(`알림 접근성 ${JSON.stringify(bounds)}`);
          if (round < 2) {
            // 앞 두 알림은 등장 중 실제 키 세 개를 받은 뒤 교체한다. 다음
            // 제목이 올 때까지 이전 알림의 접힘에도 계속 친다. 마지막 등장은
            // 끝까지 보므로 같은 전체 등장 대기를 세 번 반복하지 않는다.
            await typed.promise;
          } else await p.locator(".droplet").evaluate(async (el) => {
            // 연속 입력과 실제 애니메이션이 겹치는 원래 속도를 유지한다.
            await Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => {})));
          });
        })().finally(() => ended = true);
        // 새 제목을 기다리는 동안에도 친다: 앞 알림의 접힘과 다음 알림의 등장 모두 검사한다.
        await Promise.all([animation, (async () => {
          let currentKeys = 0;
          do {
            await p.keyboard.press("x");
            keys++;
            const current = await p.evaluate(() => document.querySelector(".droplet-title")?.textContent);
            if (current === title && ++currentKeys >= 3) typed.resolve();
            // 6 frame은 다음 키까지의 간격이다. 마지막 키 뒤에는 실제
            // 애니메이션/입력 완료로 끝내며 남은 간격을 마저 기다리지 않는다.
            await Promise.race([animation, p.evaluate(async () => {
              for (let frame = 0; frame < 6; frame++) await new Promise(requestAnimationFrame);
            })]);
          } while (!ended);
        })()]);
        if ((await p.getByTestId("activity-badge-reports").textContent()) !== String(round + 1)) throw new Error("입력 중 보고서 배지가 갱신되지 않음");
      }
      phase("contract", "input-display-and-close");
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
      if (result.moving.length !== 3) throw new Error(`${name}: 세 알림의 움직임 중 입력을 받지 못함 ${JSON.stringify(result)}`);
      await shot(p, `notice-input-${name}-${terminal ? "terminal" : "chat"}`);
      // 자동 사라짐의 4.16초는 입력 회귀마다 반복하지 않는다.
      // 실제 닫기와 교체의 fold 경로를 쓰며, 자동 만료는 알림 owner가 맡는다.
      await p.locator(".droplet-close").click();
      await domIs(p, "알림 닫힘", () => !document.querySelector(".droplet"));
    }
    console.log(`알림 입력 ${name}: ${JSON.stringify(results)}`);
    if (dev.errors.length) throw new Error(dev.errors.join("; "));
  } finally {
    phase("cleanup", "context-and-pane");
    await p.context().close();
    aaa("pane", "kill", pane.pane?.id ?? name);
  }
}

export default async function () {
  const transcript = `${root}/claude/projects/-notice/s1.jsonl`;
  fs.mkdirSync(path.dirname(transcript), { recursive: true });
  fs.writeFileSync(transcript, JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "알림 입력 시험" }] } }) + "\n");
  let safari;
  const safariBrowser = async () => (safari ??= specCleanup.own(() => webkit.launch({ headless: true }), value => value.close())).value;
  try {
  await step("연속 알림 중 입력: chromium-4k 터미널", () => check(browser, "chromium-4k-true", 3840, true, transcript));
  await step("연속 알림 중 입력: chromium-4k 대화", () => check(browser, "chromium-4k-false", 3840, false, transcript));
  await step("연속 알림 중 입력: chromium-phone 터미널", () => check(browser, "chromium-phone-true", 390, true, transcript));
  await step("연속 알림 중 입력: chromium-phone 대화", () => check(browser, "chromium-phone-false", 390, false, transcript));
  await step("연속 알림 중 입력: webkit-desktop 터미널", async () => check(await safariBrowser(), "webkit-desktop-true", 1280, true, transcript));
  await step("연속 알림 중 입력: webkit-desktop 대화", async () => check(await safariBrowser(), "webkit-desktop-false", 1280, false, transcript));
  await step("연속 알림 중 입력: webkit-phone 터미널", async () => check(await safariBrowser(), "webkit-phone-true", 390, true, transcript));
  await step("연속 알림 중 입력: webkit-phone 대화", async () => check(await safariBrowser(), "webkit-phone-false", 390, false, transcript));
  } finally { await safari?.close(); }
}
