// 대화 입력창을 Enter로 보낼 때 입력창 높이(사용자 10-20 00:5x 「enter를 누르면 입력창이 두줄로 늘었다가 줄어들어」, t-0449).
// 원인은 보내는 동안 상태 문장(「전달됨 · 대화에 오르면 풀립니다 · 취소할 수 없음」)을 단추 줄에 세우려고 늘 두 줄 배치로 바꾼 것이었고,
// 사용자 결정(10-20 「입력창에 다른 메세지 다 빼」)대로 그 글을 그리지 않아 배치가 그대로다(plan §8.292).
// 보내는 중·전달됨·대화에 올라 비워짐의 모든 그리기에서 입력 상자 높이를 ResizeObserver로 잰다:
// ResizeObserver는 배치가 바뀐 그리기마다 그 크기를 알리므로 한 frame만 늘어도 남는다(rAF 표본은 놓칠 수 있다).
// thread(일반 pane)와 coordinator pane(보라 테두리) 입력창, Chromium·WebKit 데스크톱·iPhone(단추로 보냄), 한글 IME 조합 확정 뒤 Enter,
// 여러 줄을 보고, 보내는 동안 입력창에 그려진 글(글 칸·단추 밖)이 없는지 본다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 35-composer-send-height(새 실험실). 준비는 lib/harness.mjs runSpec.
import fs from "node:fs";
import { aaa, step, browser, webkit, pairPage, paneHash, domIs, root, AA, waitOutput, shots, procs } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

/** 받은 글(bracketed paste 밖의 CR이 제출)을 hold 파일이 없으면 바로 Claude 대화 기록에 적는 가짜 agent(23 composer.py의 줄인 꼴) */
const AGENT = `import os, sys, json, subprocess
aa, transcript, hold = sys.argv[1:4]
buf=b''; text=b''; depth=0; count=0
print('\\033[?2004hsend-ready', flush=True)
while True:
    chunk=os.read(0, 4096)
    if not chunk: break
    buf+=chunk
    while buf:
        if buf.startswith(b'\\x1b'):
            if len(buf)<6: break
            if buf[:6] in (b'\\x1b[200~',b'\\x1b[201~'):
                depth += 1 if buf[:6]==b'\\x1b[200~' else -1
                buf=buf[6:]; continue
        ch=buf[:1]; buf=buf[1:]
        if ch==b'\\r' and depth==0:
            message=text.decode(); text=b''; count+=1
            if not os.path.exists(hold):
                with open(transcript,'a') as f: f.write(json.dumps({'type':'user','message':{'content':message}},ensure_ascii=False)+'\\n')
                subprocess.run([aa,'hook','--agent','claude',json.dumps({'hook_event_name':'UserPromptSubmit','session_id':'send-height'})],check=True,stdout=subprocess.DEVNULL)
            print('\\r\\nSEND_RECEIVED_'+str(count),flush=True)
        else: text+=b'\\n' if ch==b'\\r' else ch
`;

/** 웹 ChatComposer `STUCK_MS`: 전달 확인이 없어 ×(탈출구)를 보이는 때. Playwright 시계로 넘긴다(23과 같다) */
const STUCK_MS = 3000;

const userLine = (text) => JSON.stringify({ type: "user", message: { content: text } }) + "\n";

/** 그 pane에서 가짜 agent를 돌릴 셸 명령: Claude 세션을 알리고(대화 보기) 터미널을 raw로 두고 가짜 agent가 된다 */
function agentScript(name, session) {
  const dir = `${root}/claude/projects/-lab-${name}`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s1.jsonl`;
  fs.writeFileSync(transcript, userLine("앞 질문") + JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "앞 답" }] } }) + "\n");
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: session, source: "startup", transcript_path: transcript });
  const script = `${dir}/run.sh`;
  fs.writeFileSync(script, `'${AA}' hook --agent claude '${hook}'\nstty raw -echo\nexec python3 '${root}/send-height.py' '${AA}' '${transcript}' '${dir}/hold'\n`);
  return { script, transcript, hold: `${dir}/hold` };
}

export default async function () {
  const panes = {};
  const received = { thread: 0, coordinator: 0 };
  await step("입력창 높이 실험실 준비: thread pane과 coordinator pane에 대화 기록을 적는 가짜 Claude", async () => {
    fs.writeFileSync(`${root}/send-height.py`, AGENT);
    const thread = agentScript("send-thread", "5b4a3928-1706-4f5e-8d4c-3b2a19087f61");
    aaa("tab", "create", "--machine", "labA", "--label", "send-thread", "--", "/bin/sh", thread.script);
    await waitOutput("send-thread", "send-ready");
    panes.thread = { ...thread, id: "send-thread", hash: paneHash("send-thread") };
    // coordinator: fake profile pane이 post-probe로 그 셸 명령을 돌려 가짜 agent가 된다(22 「coordinator 입력창」과 같은 길)
    aaa("project", "create", "sendh");
    const coord = aaa("project", "open", "sendh", "--profile", "fake").pane.id;
    await waitOutput(coord, "fake>");
    const coordinator = agentScript("sendh", "5b4a3928-1706-4f5e-8d4c-3b2a19087f62");
    aaa("pane", "run", coord, `post-probe ${coordinator.script}`);
    await waitOutput(coord, "send-ready");
    panes.coordinator = { ...coordinator, id: coord, hash: `#pane=${encodeURIComponent(coord)}` };
  }, { always: true });


  /** 입력 상자(.composer-surface)와 입력창 묶음(.composer: 알림 띠까지) 높이를 그리기마다 적기 시작한다. 단계 표지도 함께 */
  const record = (p) => p.evaluate(() => {
    const group = document.querySelector('[data-testid="chat-composer"]');
    const surface = group.querySelector(".composer-surface");
    window.__sizes?.observer.disconnect();
    const sizes = { list: [], observer: null };
    window.__sizes = sizes;
    sizes.note = (what, h) => sizes.list.push({
      what, h: Math.round(h * 10) / 10,
      lines: surface.dataset.lines,
      state: document.querySelector('[data-testid="chat-send-state"]')?.dataset.state ?? "none",
    });
    sizes.observer = new ResizeObserver((entries) => {
      for (const e of entries) sizes.note(e.target === surface ? "surface" : "group", e.borderBoxSize[0].blockSize);
    });
    sizes.observer.observe(surface);
    sizes.observer.observe(group);
  });
  /** 기록을 닫는다: 마지막 표지 바뀜의 그리기가 ResizeObserver에 닿도록 두 frame을 기다리고, 끝 크기도 한 번 더 적는다 */
  const recorded = (p) => p.evaluate(async () => {
    const sizes = window.__sizes;
    for (let i = 0; i < 2; i++) await new Promise((r) => requestAnimationFrame(r));
    sizes.observer.disconnect();
    const group = document.querySelector('[data-testid="chat-composer"]');
    sizes.note("surface", group.querySelector(".composer-surface").getBoundingClientRect().height);
    sizes.note("group", group.getBoundingClientRect().height);
    return sizes.list;
  });
  const state = (p, want) => domIs(p, `보내기 단계 ${want}`, (want) => (document.querySelector('[data-testid="chat-send-state"]')?.dataset.state ?? "none") === want, want);
  const cleared = (p) => domIs(p, "비워지고 풀림", () => {
    const box = document.querySelector('[data-testid="chat-composer"] [aria-label="메시지"]');
    return box.value === "" && !box.readOnly && !document.querySelector('[data-testid="chat-send-state"]');
  });
  const heights = (p) => p.evaluate(() => {
    const group = document.querySelector('[data-testid="chat-composer"]');
    return { surface: group.querySelector(".composer-surface").getBoundingClientRect().height, group: group.getBoundingClientRect().height };
  });
  /** 입력창 묶음에 그려진 글(글 칸의 글·단추 이름·화면 읽기 전용 글은 빼고): 보내는 동안 비어 있어야 한다(사용자 10-20 「입력창에 다른 메세지 다 빼」) */
  const drawnText = (p) => p.evaluate(() => {
    const group = document.querySelector('[data-testid="chat-composer"]');
    const walker = document.createTreeWalker(group, NodeFilter.SHOW_TEXT);
    const out = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const el = node.parentElement;
      if (!node.textContent.trim() || el.closest("textarea, button, .visually-hidden")) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const r = range.getBoundingClientRect();
      if (r.width > 1 && r.height > 1 && getComputedStyle(el).visibility !== "hidden") out.push(node.textContent.trim());
    }
    return out;
  });

  await step("Enter·보내기 단추로 보낼 때 입력창 높이가 한 frame도 늘지 않고 상태 글이 없다: 보내는 중·전달됨·대화에 올라 비워짐, 한글 조합 확정 뒤 Enter, 여러 줄, thread·coordinator, Chromium·WebKit 데스크톱·iPhone", async () => {
    const bad = [];
    const log = [];
    const wk = await webkit.launch();
    try {
      for (const [device, open] of [
        ["chromium", (hash) => pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash })],
        ["webkit", (hash) => pairPage(wk, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash })],
        ["iphone", (hash) => pairDevice(wk, aaa("web", "pair").code, "iphone", { hash })],
      ]) {
        const touch = device === "iphone";
        for (const kind of ["thread", "coordinator"]) {
          const pane = panes[kind];
          const p = (await open(pane.hash)).page;
          if ((await p.getByTestId("chat-composer").count()) === 0) await p.getByTestId("chat-toggle").click();
          await p.getByTestId("chat-composer").waitFor();
          await domIs(p, "입력 가능", () => !document.querySelector('[aria-label="그림 첨부"]')?.disabled);
          if (kind === "coordinator") await domIs(p, "보라 입력창", () => document.querySelector(".composer-surface.is-coordinator") !== null);
          // 시계는 흐르게 두고 3초 막힘 창만 넘긴다(23 「보내기 상태」와 같다)
          await p.clock.install();
          const box = p.getByRole("textbox", { name: "메시지", exact: true });
          await box.click();
          const name = `${device} ${kind}`;
          const capture = (state) => shots && p.getByTestId("chat-composer").screenshot({ path: `${shots}/send-${device}-${kind}-${state}.png`, animations: "disabled" });
          // 터치 기기의 Enter는 줄바꿈이라 보내기 단추로 보낸다(23 「대화 Enter」)
          const send = () => touch ? p.getByRole("button", { name: "메시지 보내기", exact: true }).click() : box.press("Enter");
          /** 한 번 보내기의 높이 기록을 판정한다: 보내기 전 높이(rest)보다 큰 그리기가 없고 끝에 처음 높이로 돌아온다 */
          const judge = (label, sizes, rest, end, expected = rest) => {
            log.push(`${name} ${label}: ${sizes.map((s) => `${s.what}=${s.h}(${s.lines},${s.state})`).join(" ")}`);
            for (const what of ["surface", "group"]) {
              const grew = sizes.filter((s) => s.what === what && s.h > rest[what] + 0.5);
              if (grew.length) bad.push(`${name} ${label}: ${what} ${Math.round(rest[what])}px → ${grew.map((s) => `${s.h}px(${s.state})`).join(",")}`);
              if (Math.abs(end[what] - expected[what]) > 0.5) bad.push(`${name} ${label}: 끝 ${what} ${end[what]}px ≠ 예상 ${expected[what]}px`);
            }
          };
          const quiet = async (label) => {
            const drawn = await drawnText(p);
            if (drawn.length) bad.push(`${name} ${label}: 입력창에 글이 그려졌다 ${JSON.stringify(drawn)}`);
            if (await p.locator('[data-testid="chat-composer"] .composer-note').count()) bad.push(`${name} ${label}: 알림 띠가 나왔다`);
          };

          // 1) 바로 대화에 오름: 보내는 중 → 비워짐
          await box.fill("짧은 한 줄 메시지");
          let rest = await heights(p);
          await capture("typed");
          await record(p);
          await send();
          await waitOutput(pane.id, `SEND_RECEIVED_${++received[kind]}`);
          await cleared(p);
          judge("바로 반영", await recorded(p), rest, await heights(p));

          // 2) 전달됨에서 기다림(사용자 캡처 enter-composer.png의 순간) → 대화에 오르며 비워짐
          fs.writeFileSync(pane.hold, "");
          await box.fill("전달됨에서 기다리는 메시지");
          rest = await heights(p);
          await record(p);
          await send();
          await waitOutput(pane.id, `SEND_RECEIVED_${++received[kind]}`);
          await cleared(p);
          await quiet("전달됨");
          await capture("sent");
          judge("전달됨으로", await recorded(p), rest, await heights(p));
          await record(p);
          fs.unlinkSync(pane.hold);
          fs.appendFileSync(pane.transcript, userLine("전달됨에서 기다리는 메시지"));
          await cleared(p);
          await capture("cleared");
          judge("대화에 올라 비워짐", await recorded(p), rest, await heights(p));

          // 3) 한글 조합 중 Enter(조합 확정, WebKit은 compositionend 뒤 229로 온다)는 보내지 않고, 다음 Enter가 보낸다
          if (!touch) {
            await box.fill("한글 조합 ");
            rest = await heights(p);
            await record(p);
            await box.evaluate((el) => {
              el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
              el.dispatchEvent(new CompositionEvent("compositionupdate", { data: "끝", bubbles: true }));
              el.setRangeText("끝", el.value.length, el.value.length, "end");
              el.dispatchEvent(new InputEvent("input", { data: "끝", inputType: "insertCompositionText", isComposing: true, bubbles: true }));
              el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", isComposing: true, keyCode: 229, bubbles: true, cancelable: true }));
              el.dispatchEvent(new CompositionEvent("compositionend", { data: "끝", bubbles: true }));
              el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", keyCode: 229, bubbles: true, cancelable: true }));
            });
            if ((await box.inputValue()) !== "한글 조합 끝") bad.push(`${name} 조합 확정 Enter가 글을 바꿨다: ${JSON.stringify(await box.inputValue())}`);
            await box.press("Enter");
            await waitOutput(pane.id, `SEND_RECEIVED_${++received[kind]}`);
            await cleared(p);
            judge("조합 확정 뒤 Enter", await recorded(p), rest, await heights(p));
          }

          // 4) 여러 줄: 전달됨에서도 상자가 늘지 않고, 비워지면 한 줄로 돌아온다
          fs.writeFileSync(pane.hold, "");
          await box.fill("");
          const single = await heights(p);
          await box.fill("첫 줄\n둘째 줄");
          rest = await heights(p);
          await record(p);
          await send();
          await waitOutput(pane.id, `SEND_RECEIVED_${++received[kind]}`);
          await cleared(p);
          await quiet("여러 줄 전달됨");
          await capture("multiline-sent");
          judge("여러 줄 전달됨으로", await recorded(p), rest, await heights(p), single); // ACK 뒤 빈 초안은 한 줄로 줄어든다.
          fs.unlinkSync(pane.hold);
          fs.appendFileSync(pane.transcript, userLine("첫 줄\n둘째 줄"));
          await cleared(p);
          const end = await heights(p);
          if (Math.abs(end.surface - single.surface) > 0.5) bad.push(`${name} 여러 줄 비워짐: ${end.surface}px ≠ 한 줄 ${single.surface}px`);

          // 5) 전달 전 막힘: agent가 멈춰 3초 뒤 도는 단추가 ×로 바뀌어도 상자가 늘지 않는다. ×로 멈춘 뒤의 결과 띠는 상자 위 알림(그대로 둔 것)이다
          await box.fill("막힌 한 줄 메시지");
          rest = await heights(p);
          const agent = procs.get("agent");
          process.kill(agent.pid, "SIGSTOP");
          try {
            await record(p);
            await send();
            await state(p, "sending");
            await p.clock.runFor(STUCK_MS + 1);
            await state(p, "stuck");
            await quiet("막힘");
            await capture("stuck");
            judge("막힘으로", await recorded(p), rest, await heights(p));
            await p.getByRole("button", { name: "멈추고 글 돌려받기", exact: true }).click();
            await state(p, "none");
          } finally {
            process.kill(agent.pid, "SIGCONT");
          }
          // 멈춘 submit의 글이 agent 입력에 남았을 수 있다: 다음 보내기와 붙어 간다. 그 글을 기록에 올려 비우고 다음 기기가 깨끗이 시작한다
          await box.fill("정리");
          await send();
          await waitOutput(pane.id, `SEND_RECEIVED_${++received[kind]}`);
          fs.appendFileSync(pane.transcript, userLine("정리"));
          await cleared(p);
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(`${bad.join("; ")} ‖ 높이 기록: ${log.join(" | ")}`);
  });
}
