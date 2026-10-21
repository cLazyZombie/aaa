// 대화 첨부는 저장 → 초안의 @경로 → 보내기 → coordinator의 파일 읽기까지 실제 host·agent로 본다.
// 참고: herdr-web-ui Composer(onPaste/onDrop/uploadImages), compose(imageMention/insertLeadingMention).
import fs from "node:fs";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import { aaa, step, browser, webkit, pairPage, inList, domIs, root, repo, AA, procs, sentFrames, waitOutput, shot, until } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

// CODEX_HOME: agent가 Codex 대화 기록을 읽는 폴더(실험실 안, 실제 ~/.codex는 건드리지 않는다)
export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0", CODEX_HOME: `${root}/codex` };
/** 웹 ChatComposer `STUCK_MS`·`SLOW_MS`: 전달 확인이 없어 탈출구를 보이는 때, 전달 뒤 보내짐 증거를 기다리는 때. Playwright 시계로 넘긴다 */
const STUCK_MS = 3000;
const SLOW_MS = 10_000;

async function openChat(p, label = "composer") {
  await inList(p, `pane-${label}`);
  if (await p.getByTestId("chat-composer").count() === 0) await p.getByTestId("chat-toggle").click();
  await p.getByTestId("chat-composer").waitFor();
  await domIs(p, "입력 가능", () => !document.querySelector('[aria-label="그림 첨부"]')?.disabled);
  return p.getByRole("textbox", { name: "메시지", exact: true });
}
async function fileEvent(box, type, png, text = "") {
  return box.evaluate((el, { type, b64, text }) => {
    const dt = new DataTransfer();
    dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "clipboard.png", { type: "image/png" }));
    if (text) dt.setData("text/plain", text);
    if (type === "drop") el.dispatchEvent(new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true }));
    const event = type === "paste" ? new ClipboardEvent(type, { clipboardData: dt, bubbles: true, cancelable: true }) : new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true });
    el.dispatchEvent(event);
    return event.defaultPrevented;
  }, { type, b64: png.toString("base64"), text });
}

export default async function ({ page }) {
  const project = `${root}/projects/images`;
  const receipts = `${root}/composer-received.json`;
  const transcript = `${root}/claude/projects/-lab-composer/chat.jsonl`;
  const hold = `${root}/hold-chat`;
  const chatLine = (text) => JSON.stringify({ type: "user", message: { content: text } }) + "\n";
  await step("대화 첨부 실험실 준비", async () => {
    aaa("project", "create", "images");
    fs.mkdirSync(`${root}/claude/projects/-lab-composer`, { recursive: true });
    fs.mkdirSync(`${project}/uploads`, { recursive: true });
    const seed = await page.evaluate(() => {
      const canvas = document.createElement("canvas"); canvas.width = 640; canvas.height = 360;
      const c = canvas.getContext("2d"); c.fillStyle = "#19639b"; c.fillRect(0, 0, 640, 360);
      c.fillStyle = "#41b3a3"; c.fillRect(320, 0, 320, 360);
      c.fillStyle = "white"; c.font = "bold 40px sans-serif"; c.fillText("첨부 이미지", 72, 182);
      return canvas.toDataURL("image/png").split(",")[1];
    });
    fs.writeFileSync(`${project}/uploads/seed.png`, Buffer.from(seed, "base64"));
    fs.writeFileSync(transcript, chatLine("같은 글 재전송") + chatLine(`@${project}/uploads/seed.png`));
    // agent 입력 경계: bracketed paste 안의 CR은 줄바꿈, 바깥 CR만 제출이다. 받은 @경로를 실제로 연다.
    fs.writeFileSync(`${root}/composer.py`, `import os, sys, json, re, hashlib, subprocess
buf=b''; text=b''; depth=0; count=0
print('\\033[?2004hcomposer-ready', flush=True)
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
            if re.search(rb'(?:^|\\s)@\\S+$',text):
                print('\\r\\nCMP_MENTION_SELECTED',flush=True); continue
            message=text.decode(); text=b''; count+=1
            paths=re.findall(r'@([^\\s]+)',message)
            images=[{'path':p,'sha256':hashlib.sha256(open(p,'rb').read()).hexdigest()} for p in paths]
            open(sys.argv[2],'w').write(json.dumps({'text':message,'images':images},ensure_ascii=False))
            hook=lambda event: subprocess.run([sys.argv[1],'hook','--agent','claude',json.dumps({'hook_event_name':event,'session_id':'composer'})],check=True,stdout=subprocess.DEVNULL)
            # hold: Claude가 받지 못한 것처럼(기록·hook 없음). -queue: 작업 중이라 대기열에 넣는다(Claude는 넣자마자 적는다)
            if not os.path.exists(sys.argv[4]):
                queue=os.path.exists(sys.argv[4]+'-queue')
                recorded='<pasted_content id="local_408">\\n'+message+'\\n</pasted_content id="local_408">' if '\\n' in message else message
                line={'type':'queue-operation','operation':'enqueue','content':recorded} if queue else {'type':'user','message':{'content':recorded}}
                with open(sys.argv[3],'a') as f: f.write(json.dumps(line,ensure_ascii=False)+'\\n')
                if not queue: hook('UserPromptSubmit')
            print('\\r\\nCMP_RECEIVED_'+str(count),flush=True)
        else: text+=b'\\n' if ch==b'\\r' else ch
`);
    aaa("tab", "create", "--machine", "labA", "--label", "composer", "--cwd", project, "--", "/bin/sh", "-c",
      `"$0" hook --agent claude '{"hook_event_name":"SessionStart","session_id":"composer","source":"startup","transcript_path":"${transcript}"}'; stty raw -echo; exec python3 "$1" "$0" "$2" "$3" "$4"`, AA, `${root}/composer.py`, receipts, transcript, hold);
    await waitOutput("composer", "composer-ready");
  }, { always: true });

  let received = 0;
  const press = (p) => p.getByRole("button", { name: "메시지 보내기", exact: true }).click();
  const submit = async (p) => {
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await domIs(p, "전송 완료", () => document.querySelector('[aria-label="메시지"]').value === "");
    return JSON.parse(fs.readFileSync(receipts, "utf8"));
  };

  await step("재로딩 중 예약된 자동화 조회: 끝나는 문서에서 후속 fetch를 시작하지 않는다(WebKit)", async () => {
    const wk = await webkit.launch();
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    let reached;
    const held = new Promise(resolve => { reached = resolve; });
    let first = true;
    try {
      const client = await pairPage(wk, aaa("web", "pair").code, { width: 1280, height: 800 }, {
        serviceWorkers: "block",
        route: ["**/api/v1/call", async route => {
          if (first && route.request().postDataJSON()?.op === "projects") {
            first = false;
            reached();
            await gate;
          }
          await route.continue().catch(() => {}); // 이전 문서의 요청은 reload가 이미 취소했다.
        }],
      });
      await held;
      // events 연결과 첫 snapshot이 각각 요청한 조회를 하나로 합친 채 첫 HTTP 응답을 보류한다.
      await inList(client.page, "pane-composer");
      await client.page.reload();
      release();
      await openChat(client.page);
      assert.deepEqual(client.errors, [], "문서 종료 뒤 예약된 조회의 페이지 오류");
      const hiddenCalls = await client.page.evaluate(() => {
        const fetch = window.fetch;
        let calls = 0;
        window.fetch = (...args) => { calls++; return fetch(...args); };
        window.dispatchEvent(new PageTransitionEvent("pagehide", { persisted: true }));
        window.dispatchEvent(new Event("aaa:automations-changed"));
        window.fetch = fetch;
        return calls;
      });
      assert.equal(hiddenCalls, 0, "끝난 문서에 온 갱신 사건은 조회를 시작하지 않는다");
      const resumed = client.page.waitForRequest(r => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "projects");
      await client.page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
      await resumed;
      assert.deepEqual(client.errors, [], "pageshow 뒤 조회 재개");
      await client.page.context().close();
    } finally {
      release();
      await wk.close();
    }
  });


  await step("종료 문서 API: WebKit pagehide 뒤 tasks·inbox 조회를 시작하지 않고 pageshow 뒤 다시 읽는다", async () => {
    const wk = await webkit.launch();
    try {
      const client = await pairPage(wk, aaa("web", "pair").code, { width: 1280, height: 800 }, {
        serviceWorkers: "block",
        init: () => {
          window.__apiEventSockets = [];
          const WS = window.WebSocket;
          window.WebSocket = class extends WS {
            constructor(...args) {
              super(...args);
              if (String(args[0]).endsWith("/api/v1/events")) window.__apiEventSockets.push(this);
            }
          };
        },
      });
      const p = client.page;
      const receive = op => p.waitForResponse(r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === op).then(r => r.finished());
      const notify = () => p.evaluate(() => {
        const socket = window.__apiEventSockets.find(s => s.readyState === WebSocket.OPEN);
        if (!socket) throw new Error("실제 앱 events 연결이 없다");
        for (const event of [{ ev: "tasks", sha256: "0".repeat(64) }, { ev: "inbox" }]) {
          socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(event) }));
        }
      });
      await p.waitForFunction(() => window.__apiEventSockets.some(s => s.readyState === WebSocket.OPEN));
      // 먼저 실제 두 조회가 가능한 연결·구독인지 확인한다. listener 부재로 숨김 검사가 통과하면 안 된다.
      await Promise.all([receive("tasks"), receive("inbox"), notify()]);
      const hiddenCalls = await p.evaluate(() => {
        const original = window.fetch;
        const calls = [];
        window.fetch = (input, options) => {
          if (!options?.keepalive && new URL(String(input), location.href).pathname.startsWith("/api/")) {
            calls.push(options?.body ?? String(input));
          }
          return original(input, options);
        };
        try {
          window.dispatchEvent(new PageTransitionEvent("pagehide", { persisted: true }));
          const socket = window.__apiEventSockets.find(s => s.readyState === WebSocket.OPEN);
          for (const event of [{ ev: "tasks", sha256: "0".repeat(64) }, { ev: "inbox" }, { ev: "inbox" }]) {
            socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(event) }));
          }
          return calls;
        } finally { window.fetch = original; }
      });
      assert.deepEqual(hiddenCalls, [], "끝나는 문서에서 일반 앱 API를 시작했다");
      await p.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
      await Promise.all([receive("tasks"), receive("inbox"), notify()]);
      assert.deepEqual(client.errors, [], "문서 복귀 뒤 처리되지 않은 오류");
      await p.context().close();
    } finally { await wk.close(); }
  });

  await step("WebCrypto 없는 연결도 짧은 글은 기존 수신 비교로 보낸다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    const box = await openChat(p);
    await p.evaluate(() => {
      Object.defineProperty(crypto, "subtle", { value: undefined });
      Object.defineProperty(crypto, "randomUUID", { value: undefined });
    });
    await box.fill("짧은 호환 입력");
    await submit(p);
    if (client.errors.length) throw new Error(client.errors.join("; "));
    await p.context().close();
  });

  await step("긴 글 수신: 4천 자 표시 상한 뒤에도 입력 잠금이 풀리고 같은 앞부분의 다른 글은 확인이 아니다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    const box = await openChat(p);
    await domIs(p, "기존 기록 준비", () => document.querySelector('[data-role="user"]')?.textContent.includes("같은 글 재전송"));
    const message = "긴 합성 글 ".repeat(700) + "진짜 끝";
    await box.fill(message);
    fs.writeFileSync(hold, "hold");
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    fs.appendFileSync(transcript, chatLine(message.slice(0, -4) + "다른 끝"));
    await domIs(p, "다른 긴 글은 확인 아님", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.includes("긴 합성 글")));
    if (!await box.evaluate((el) => el.readOnly)) throw new Error("같은 앞부분만으로 잠금을 풀었다");
    fs.appendFileSync(transcript, chatLine(message));
    await domIs(p, "긴 글도 온전한 수신 확인", () => document.querySelector('[aria-label="메시지"]').value === "" && !document.querySelector('[aria-label="메시지"]').readOnly);
    fs.unlinkSync(hold);
    await p.context().close();
  });

  await step("Claude 내부 알림은 사용자 말·수신 확인에서 제외하고 사람이 같은 모양을 보낸 글은 보존한다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const p = client.page;
    const box = await openChat(p);
    await domIs(p, "기존 기록 준비", () => document.querySelector('[data-role="user"]')?.textContent.includes("같은 글 재전송"));
    const before = await p.locator('[data-role="user"]').count();
    const text = "<task-notification><task-id>task-1</task-id><status>completed</status><summary>사람이 인용한 알림 예시</summary></task-notification>";
    fs.writeFileSync(hold, "hold");
    await box.fill(text);
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    fs.appendFileSync(transcript, JSON.stringify({ type: "user", origin: { kind: "task-notification", producer: "session-task" }, message: { role: "user", content: text } }) + "\n"
      + JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "알림 경계 확인" }] } }) + "\n");
    await domIs(p, "내부 알림 뒤 기록 갱신", () => [...document.querySelectorAll('[data-role="assistant"]')].some(el => el.textContent.includes("알림 경계 확인")));
    assert.equal(await p.locator('[data-role="user"]').count(), before, "비사람 알림은 사용자 말풍선에서 제외");
    assert.ok(await box.evaluate(el => el.readOnly), "같은 내용의 내부 알림으로 입력 잠금을 풀지 않는다");
    fs.appendFileSync(transcript, JSON.stringify({ type: "user", origin: { kind: "human" }, message: { role: "user", content: text } }) + "\n");
    await domIs(p, "사람이 보낸 알림 예시 수신", () => document.querySelector('[aria-label="메시지"]').value === "" && !document.querySelector('[aria-label="메시지"]').readOnly);
    assert.equal(await p.locator('[data-role="user"]').count(), before + 1);
    await shot(p, "claude-internal-notification-literal-mobile");
    fs.unlinkSync(hold);
    await p.context().close();
  });

  await step("그림만 보내기: 마지막 @경로의 자동완성을 닫고 한 번 제출한다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const box = await openChat(client.page);
    await box.fill("");
    await fileEvent(box, "paste", fs.readFileSync(`${root}/pic.png`));
    await domIs(client.page, "그림만 첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
    const path = (await box.inputValue()).trim().slice(1);
    const got = await submit(client.page);
    if (got.images.length !== 1 || got.images[0].path !== path || got.text.trim() !== `@${path}`) throw new Error("그림만 보낸 글이 바뀌었다");
    await client.page.context().close();
  });

  await step("첨부 기록: @경로 링크·썸네일은 같은 pane의 허용 파일을 열고 확대한다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const p = client.page;
    await openChat(p);
    const got = { images: [{ path: `${project}/uploads/seed.png` }] };
    const link = p.getByTestId("chat").locator(".markdown-file").filter({ hasText: got.images[0].path });
    const thumb = p.getByRole("button", { name: "첨부 그림 확대", exact: true }).filter({ has: p.getByAltText("seed.png", { exact: true }) });
    await thumb.waitFor();
    await thumb.scrollIntoViewIfNeeded();
    await thumb.locator("img").evaluate((img) => img.decode());
    const clipped = await thumb.evaluate((el) => {
      const b = el.getBoundingClientRect(), i = el.querySelector("img").getBoundingClientRect();
      return i.height < 80 || i.bottom > b.bottom + 1 || i.right > b.right + 1;
    });
    if (clipped) throw new Error("썸네일이 단추 높이에 잘렸다");
    await shot(p, "chat-image-thumbnail-phone");
    await link.click();
    await domIs(p, "첨부 경로 보기", () => document.querySelector('[data-testid="file-viewer"] img')?.naturalWidth > 0);
    await p.keyboard.press("Escape");
    await p.getByTestId("file-viewer").waitFor({ state: "detached" });
    await thumb.click();
    await domIs(p, "첨부 그림 확대", () => document.querySelector('[data-testid="file-viewer"] img')?.naturalWidth > 0);
    await p.getByTestId("file-viewer").evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await shot(p, "chat-image-expanded-phone");
    await p.keyboard.press("Escape");
    await p.getByTestId("file-viewer").waitFor({ state: "detached" });
    if (!await thumb.evaluate((el) => el === document.activeElement)) throw new Error("확대 닫기 뒤 썸네일 focus를 잃었다");
    await p.context().close();
  });

  await step("첨부 권한: 허용 밖·symlink·reader·미인증 읽기를 막는다", async () => {
    fs.writeFileSync(`${root}/outside.png`, fs.readFileSync(`${root}/pic.png`));
    fs.symlinkSync(`${root}/outside.png`, `${project}/uploads/escape.png`);
    fs.appendFileSync(transcript, chatLine(`@${root}/outside.png @${project}/uploads/escape.png`));
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    await openChat(p);
    await domIs(p, "허용 밖 썸네일 거부", () => document.querySelectorAll('.chat-user-image[data-state="error"]').length === 2);
    const image = p.getByAltText("seed.png", { exact: true });
    await image.evaluate((img) => img.decode());
    const url = await image.getAttribute("src");
    const anon = await browser.newContext();
    if ((await anon.request.get(new URL(url, p.url()).href)).status() !== 401) throw new Error("미인증 그림 읽기를 허용했다");
    await anon.close();
    await p.context().close();
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await inList(reader.page, "pane-composer");
    if (await reader.page.getByTestId("chat").count() === 0) await reader.page.getByTestId("chat-toggle").click();
    await domIs(reader.page, "reader 대화 읽기", () => document.querySelectorAll('[data-role="user"]').length >= 3);
    if (await reader.page.locator(".chat-user-image").count()) throw new Error("reader가 파일 썸네일을 읽었다");
    const pane = aaa("pane", "list").panes.find((p) => p.name === "composer").id;
    const response = await reader.page.evaluate(async ({ pane, path }) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "pane_file", pane, path }) })).json(), { pane, path: `${project}/uploads/seed.png` });
    if (response.err?.code !== "forbidden") throw new Error(`reader 파일 보기 ${JSON.stringify(response)}`);
    if (await reader.page.evaluate(async (url) => (await fetch(url)).status, url) !== 403) throw new Error("reader가 읽기 경로로 그림을 읽었다");
    await reader.page.context().close();
  });

  await step("보내기 상태: 보내짐(기록 증거)까지 잠금, 3초 막힘의 탈출구는 Enter를 막는다, 오래 걸림·늦은 확인·잠금 풀기, 작업 중 대기 말", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    const box = await openChat(p);
    await domIs(p, "기존 같은 글 기록", () => document.querySelector('[data-role="user"]')?.textContent.includes("같은 글 재전송"));
    // 시계는 흐르게 두고(기록 다시 읽기의 타이머도 돈다) 3초·10초 창만 넘긴다
    await p.clock.install();
    const phase = (want) => domIs(p, `보내기 단계 ${want}`, (want) => (document.querySelector('[data-testid="chat-send-state"]')?.dataset.state ?? "none") === want, want);
    const readOnly = () => box.evaluate((el) => el.readOnly);
    const submits = async (from) => (await sentFrames(p, from)).filter((f) => f.t === "submit").length;
    const users = (text) => p.evaluate((text) => [...document.querySelectorAll('[data-role="user"]')].filter((el) => el.textContent.trim() === text).length, text);
    const before = (await sentFrames(p)).length;

    // 바로 보낸다. agent가 받은 기록이 올 때까지 글이 잠긴 채 남고 단추는 도는 표시다. 다른 기록·옛 같은 글은 증거가 아니다
    fs.writeFileSync(hold, "hold");
    await box.fill("같은 글 재전송");
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await phase("sent");
    if (await box.inputValue() !== "같은 글 재전송" || !await readOnly()) throw new Error("전달된 글이 보내짐 전에 사라졌거나 풀렸다");
    if (!await p.getByRole("button", { name: "보내는 중 · 취소할 수 없음", exact: true }).isDisabled()) throw new Error("전달 뒤에 단추를 누를 수 있다");
    await shot(p, "chat-send-sent");
    // 앞 글이 보내지기 전에는 다음 글을 보내지 않는다(웹 대기열 없음)
    await box.press("Enter");
    // 가는 글은 초안에 없다: 같은 pane을 새로 열어도 보통 초안으로 돌아오지 않는다
    const again = await p.context().newPage();
    await again.goto(p.url());
    if (await (await openChat(again)).inputValue() !== "") throw new Error("전달 중인 글이 다른 탭 초안으로 돌아왔다");
    await again.close();
    fs.appendFileSync(transcript, chatLine("다른 기기 메시지"));
    await domIs(p, "다른 기기 메시지 기록", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "다른 기기 메시지"));
    await p.clock.runFor(SLOW_MS + 1);
    await phase("slow");
    await box.press("Control+Enter");
    await box.evaluate(el => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, isComposing: true, keyCode: 229, bubbles: true })));
    const blocked = (await sentFrames(p)).filter(f => f.t === "input_trace" && f.event === "ctrl_enter_blocked");
    for (const reason of ["locked", "composing"]) assert.ok(blocked.some(f => f.method === reason), `대화 입력 Ctrl+Enter 차단 이유: ${reason}`);
    await shot(p, "chat-send-slow");
    if (await submits(before) !== 1) throw new Error("잠긴 동안 다시 보냈거나 다음 글을 보냈다");
    // 늦게라도 기록에 오르면 비우고 푼다
    fs.appendFileSync(transcript, chatLine("같은 글 재전송"));
    await phase("none");
    if (await users("같은 글 재전송") !== 2 || await box.inputValue() !== "" || await readOnly()) throw new Error("늦은 반영 뒤 비우고 풀지 않았다");

    // 오래 걸림에서 잠금 풀기: 갔을 수 있는 글을 고칠 수 있게 돌려준다(다시 보내지 않는다)
    await box.fill("먹힌 글");
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await phase("sent");
    await p.clock.runFor(SLOW_MS + 1);
    await phase("slow");
    await p.getByRole("button", { name: "잠금 풀기", exact: true }).click();
    await phase("none");
    if (await box.inputValue() !== "먹힌 글" || await readOnly() || await submits(before) !== 2) throw new Error("잠금 풀기가 글을 돌려주지 않았거나 다시 보냈다");
    fs.unlinkSync(hold);

    // 바로 반영: 기록에 오르면 비우고 푼다. 안내 줄은 없다
    await box.fill("386 추천대로 해");
    await submit(p);
    await phase("none");
    if (await users("386 추천대로 해") !== 1 || await readOnly() || await submits(before) !== 3) throw new Error("즉시 반영이 다르다");
    await shot(p, "chat-send-reflected");

    // 전달 전에 막힘: agent가 멈춰 글 요청이 돌아오지 않는다. 3초 뒤에만 탈출구가 나오고, 누르면 host가 Enter를 보내지 않는다
    const agent = procs.get("agent");
    process.kill(agent.pid, "SIGSTOP");
    try {
      await box.fill("막힌 글");
      await press(p);
      await phase("sending");
      if (await p.getByRole("button", { name: "멈추고 글 돌려받기", exact: true }).count() !== 0) throw new Error("3초 전에 탈출구가 보였다");
      await p.clock.runFor(STUCK_MS + 1);
      await phase("stuck");
      await shot(p, "chat-send-stuck");
      await p.getByRole("button", { name: "멈추고 글 돌려받기", exact: true }).click();
      await phase("none");
    } finally {
      process.kill(agent.pid, "SIGCONT");
    }
    if (!(await p.getByTestId("chat-note").innerText()).includes("Enter를 보내지 않음") || await box.inputValue() !== "막힌 글" || await readOnly()) throw new Error("탈출구가 글을 돌려주지 않았거나 단계를 말하지 않았다");
    await shot(p, "chat-send-stopped");
    // 멈춘 글은 agent 입력창에만 남고 제출되지 않았다: 다음 글이 그 뒤에 붙어 한 번에 간다(Enter가 갔다면 따로 왔다)
    await box.fill("다음 글");
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    const got = JSON.parse(fs.readFileSync(receipts, "utf8"));
    if (got.text !== "막힌 글다음 글") throw new Error(`멈춘 submit의 Enter가 갔다: ${JSON.stringify(got)}`);
    await p.clock.runFor(SLOW_MS + 1);
    await phase("slow");
    await p.getByRole("button", { name: "잠금 풀기", exact: true }).click();
    await box.fill("");

    // Claude 작업 중: 대기열 줄이 보내짐이다. 풀고 대화 끝 점선 대기 말, 흡수되면 보통 말
    fs.writeFileSync(`${hold}-queue`, "");
    await box.fill("작업 중 추가\n두 번째 줄");
    await press(p);
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await domIs(p, "대기 말", () => [...document.querySelectorAll(".chat-msg.user.is-pending")].filter((el) => el.textContent.includes("작업 중 추가")).length === 1
      && document.querySelector('[data-testid="chat-pending"]')?.textContent.includes("대기 중")
      && !document.querySelector('[data-testid="chat-pending"]').textContent.includes("읽습니다")
      && document.querySelector('[aria-label="메시지"]').value === "" && !document.querySelector('[aria-label="메시지"]').readOnly);
    await shot(p, "chat-send-pending");
    fs.appendFileSync(transcript, [
      { type: "queue-operation", operation: "remove", content: '<pasted_content id="local_408">\n작업 중 추가\n두 번째 줄\n</pasted_content id="local_408">', reason: "absorbed_mid_turn" },
      { type: "attachment", attachment: { type: "queued_command", commandMode: "prompt", origin: { kind: "human" }, prompt: '<pasted_content id="local_408">\n작업 중 추가\n두 번째 줄\n</pasted_content id="local_408">' } },
    ].map((l) => JSON.stringify(l) + "\n").join(""));
    await domIs(p, "흡수 뒤 보통 말", () => !document.querySelector('[data-testid="chat-pending"]')
      && [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.includes("작업 중 추가")));
    fs.unlinkSync(`${hold}-queue`);
    const events = aaa("debug", "input", "--pane", "composer", "--since", "1h").events;
    const queuedReceipt = events.find((e) => e.event === "ui" && e.detail.includes("QueueOperation"));
    if (!queuedReceipt || !events.some((e) => e.id === queuedReceipt.id && e.event === "received" && e.detail.includes("multiline=true"))) throw new Error("여러 줄 대기 수신 진단이 같은 ID로 이어지지 않았다");
    for (const stage of ["Slow", "Unlock", "Cancel", "Unlocked"]) if (!events.some((e) => e.event === "ui" && e.detail.includes(`event=${stage}`))) throw new Error(`입력 진단 사건 누락: ${stage}`);
    if (JSON.stringify(events).includes("작업 중 추가") || JSON.stringify(events).includes("먹힌 글")) throw new Error("진단에 사용자 글이 남았다");
    if (client.errors.length) throw new Error(client.errors.join("; "));
    await p.context().close();
  });

  await step("보내기 상태(Codex 작업 중): 기록에 대기를 남기지 않으므로 host 전달이 보내짐이고 대화 끝 대기 말로 기다린다", async () => {
    const thread = "00000000-0000-7000-8000-000000000392";
    const rollout = `${root}/codex/sessions/2026/10/19/rollout-2026-10-19T00-00-00-${thread}.jsonl`;
    const codexReceived = `${root}/codex-received.json`;
    const said = (text) => JSON.stringify({ type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text }] } }) + "\n";
    fs.mkdirSync(`${root}/codex/sessions/2026/10/19`, { recursive: true });
    fs.writeFileSync(rollout, said("코덱스 첫 요청"));
    fs.mkdirSync(`${root}/bin`, { recursive: true });
    // aaa는 Codex 실행에 --no-daemon을 붙인다(spec 11의 Codex fixture와 같다)
    fs.writeFileSync(`${root}/bin/codex`, "#!/bin/sh\n[ \"${1-}\" = --no-daemon ] && shift\nexec /bin/sh \"$@\"\n", { mode: 0o755 });
    // 작업 중 화면(감지 fixture)을 늘 다시 그린다. 받은 글은 기록하지 않는다(Codex는 다음 도구 호출 뒤 읽을 때 적는다)
    fs.writeFileSync(`${root}/codex.py`, `import os, sys, json, subprocess
screen=open(sys.argv[2]).read().replace('\\n','\\r\\n')
def draw(extra=''):
    sys.stdout.write('\\033[2J\\033[H\\033[?2004h'+extra+screen); sys.stdout.flush()
subprocess.run([sys.argv[1],'hook','--agent','codex',json.dumps({'hook_event_name':'SessionStart','session_id':sys.argv[3]})],check=True,stdout=subprocess.DEVNULL)
draw()
buf=b''; text=b''; depth=0; count=0
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
            count+=1; open(sys.argv[4],'w').write(json.dumps({'text':text.decode()},ensure_ascii=False)); text=b''
            draw('CDX_RECEIVED_'+str(count)+'\\r\\n')
        else: text+=b'\\n' if ch==b'\\r' else ch
`);
    const pane = aaa("tab", "create", "--machine", "labA", "--label", "codex-chat", "--cwd", project, "--", `${root}/bin/codex`, "-c",
      `stty raw -echo; exec python3 "$1" "$0" "$2" "$3" "$4"`, AA, `${root}/codex.py`, `${repo}/crates/aaa-agent/src/detect/fixtures/codex-working.txt`, thread, codexReceived).tabs.at(-1).root.pane;
    await waitOutput(pane, "esc to interrupt");
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    await p.getByTestId("agent-codex-chat").and(p.locator('[data-status="working"]')).waitFor({ state: "attached" });
    const box = await openChat(p, "codex-chat");
    await domIs(p, "Codex 기록", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "코덱스 첫 요청"));
    await box.fill("코덱스 작업 중 추가");
    await press(p);
    await waitOutput(pane, "CDX_RECEIVED_1");
    await domIs(p, "Codex 대기 말", () => [...document.querySelectorAll(".chat-msg.user.is-pending")].some((el) => el.textContent.trim() === "코덱스 작업 중 추가")
      && document.querySelector('[data-testid="chat-pending"]')?.textContent.includes("대기 중")
      && !document.querySelector('[data-testid="chat-pending"]').textContent.includes("읽습니다")
      && document.querySelector('[aria-label="메시지"]').value === "" && !document.querySelector('[aria-label="메시지"]').readOnly);
    if (JSON.parse(fs.readFileSync(codexReceived, "utf8")).text !== "코덱스 작업 중 추가") throw new Error("Codex가 받은 글이 다르다");
    // Codex가 읽어 기록하면 대기 말이 보통 말이 된다
    fs.appendFileSync(rollout, said("코덱스 작업 중 추가"));
    await domIs(p, "Codex 흡수 뒤 보통 말", () => !document.querySelector('[data-testid="chat-pending"]')
      && [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "코덱스 작업 중 추가"));
    if (client.errors.length) throw new Error(client.errors.join("; "));
    await p.context().close();
    aaa("pane", "kill", pane);
  });

  // 대기 메시지 「지금 보내기」(plan §8.271)의 가짜 harness. 받은 bytes를 기록하고 머리 줄에 받은 수·대기 수·보낸 수를 그린다.
  // Claude: 작업 중 Enter 글은 대기열(기록에 enqueue)이고 대기 글 아래 `ctrl+x ctrl+s to send now`(dim)를 그린다(실측 2.1.303).
  //   빈 입력의 Ctrl+X Ctrl+S는 대기열 전부를 지금 보낸다(remove + queued_command). Ctrl+U는 입력을 비운다.
  // Codex: 작업 중 Enter 글은 대기 steering 패널(↳)에 둔다. `-hidden` 표지가 있으면 패널 없이 받은 것으로 둔다(실측 0.163은
  //   작업 중 글을 거의 바로 넘긴다). 빈 입력의 Esc는 패널의 글을 모두 rollout에 적는다
  const steerProgram = String.raw`
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
const separator = process.argv.indexOf('--', 2);
const [kind, aa, record, session, file] = process.argv.slice(separator + 1);
spawnSync(aa, ['hook', '--agent', kind, JSON.stringify({ hook_event_name: 'SessionStart', session_id: session, source: 'startup', transcript_path: record })], { stdio: 'ignore' });
fs.writeFileSync(file, '');
let draft = '', queue = [], count = 0, steered = 0, ctrlX = false;
const write = (line) => fs.appendFileSync(record, JSON.stringify(line) + '\n');
const said = (text) => kind === 'codex'
  ? write({ type: 'response_item', payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text }] } })
  : write({ type: 'attachment', attachment: { type: 'queued_command', commandMode: 'prompt', origin: { kind: 'human' }, prompt: text } });
function draw() {
  let s = 'received ' + count + ' queued ' + queue.length + ' steered ' + steered + '\r\n';
  if (kind === 'codex') {
    s += '• Working (2s • esc to interrupt)\r\n';
    if (queue.length) s += '• Messages to be submitted after next tool call (press esc to interrupt and send immediately)\r\n' + queue.map((q) => '  ↳ ' + q + '\r\n').join('');
    const row = 3 + (queue.length ? 1 + queue.length : 0);
    s += '› ' + draft + '\r\n\r\n  GPT-6.1 low · /workspace\r\n  ? for shortcuts\r\n\x1b[' + row + ';' + (3 + draft.length) + 'H';
  } else {
    for (const q of queue) s += '❯ ' + q + '\r\n  \x1b[2mctrl+x ctrl+s to send now\x1b[0m\r\n';
    s += '✻ Working… (1s · esc to interrupt)\r\n\r\n────────────────────────\r\n❯ ' + (draft || (queue.length ? '\x1b[2mPress up to edit queued messages\x1b[0m' : '')) + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  }
  process.stdout.write('\x1b[2J\x1b[H' + s);
}
function sendNow() {
  if (draft !== '' || queue.length === 0) return;
  for (const q of queue) {
    if (kind === 'claude') write({ type: 'queue-operation', operation: 'remove', content: q, reason: 'absorbed_mid_turn' });
    said(q);
  }
  queue = []; steered++;
}
process.stdin.setRawMode(true);
process.stdin.on('data', (data) => {
  fs.appendFileSync(file, data); count += data.length;
  const text = data.toString().replaceAll('\x1b[200~', '').replaceAll('\x1b[201~', '');
  if (text === '\x1b') { if (kind === 'codex') sendNow(); draw(); return; }
  for (const ch of text) {
    if (ch === '\x18') { ctrlX = true; continue; }
    if (ch === '\x13' && ctrlX && kind === 'claude') sendNow();
    else if (ch === '\x15') draft = '';
    else if (ch === '\r') {
      if (draft === '') continue;
      if (kind === 'claude') write({ type: 'queue-operation', operation: 'enqueue', content: draft });
      if (!fs.existsSync(file + '-hidden')) queue.push(draft);
      draft = '';
    } else if (ch >= ' ') draft += ch;
    ctrlX = false;
  }
  draw();
});
draw();
`;
  const steerPane = async (kind, label, record, session) => {
    const dir = `${root}/steer-${kind}`;
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(`${dir}/${kind}`, `#!${process.execPath}\n${steerProgram}`, { mode: 0o755 });
    aaa("tab", "create", "--machine", "labA", "--label", label, "--cwd", project, "--", `${dir}/${kind}`, "--", kind, AA, record, session, `${dir}/bytes`);
    await waitOutput(label, "received 0 queued 0");
    return { bytes: () => fs.readFileSync(`${dir}/bytes`), hide: (on) => on ? fs.writeFileSync(`${dir}/bytes-hidden`, "") : fs.unlinkSync(`${dir}/bytes-hidden`) };
  };
  const steerButtons = (p) => p.evaluate(() => {
    const buttons = [...document.querySelectorAll('[data-testid="chat-pending-steer"]')];
    const pending = [...document.querySelectorAll(".chat-turn-user")].filter((t) => t.querySelector(".chat-msg.user.is-pending"));
    return { count: buttons.length, onLast: buttons.length === 1 && pending.at(-1)?.contains(buttons[0]), label: buttons[0]?.textContent ?? null, phase: buttons[0]?.dataset.phase ?? null, pending: pending.length };
  });
  const noteText = (p) => p.evaluate(() => document.querySelector('[data-testid="chat-pending-note"]')?.textContent ?? null);

  await step("대기 메시지 「지금 보내기」(Claude): 대기 말 맨 끝에 단추 하나(N개), 터미널에 쓰던 글이 있으면 보내지 않고 까닭, 누르면 글을 다시 보내지 않고 sendNow 키만 보내 대기 말이 보통 말이 된다, 옛 host는 단추 없음, 입력 진단에 판정·보냄", async () => {
    const record = `${root}/claude/projects/-lab-steer/steer.jsonl`;
    fs.mkdirSync(`${root}/claude/projects/-lab-steer`, { recursive: true });
    fs.writeFileSync(record, chatLine("스티어 첫 요청"));
    const fake = await steerPane("claude", "steer-claude", record, "steer-claude");
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    await p.getByTestId("agent-steer-claude").and(p.locator('[data-status="working"]')).waitFor({ state: "attached" });
    const box = await openChat(p, "steer-claude");
    await domIs(p, "Claude 기록", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "스티어 첫 요청"));
    for (const [i, text] of ["먼저 높이부터", "그리고 다크 테마도"].entries()) {
      await box.fill(text);
      await press(p);
      await waitOutput("steer-claude", `queued ${i + 1}`);
      await domIs(p, "대기 말과 입력창 풀림", ([n]) => document.querySelectorAll(".chat-msg.user.is-pending").length === n && !document.querySelector('[aria-label="메시지"]').readOnly, [i + 1]);
    }
    // 키가 대기열 전부를 보내므로 단추는 맨 끝 말 하나다
    await domIs(p, "단추 하나", () => document.querySelectorAll('[data-testid="chat-pending-steer"]').length === 1);
    assert.deepEqual(await steerButtons(p), { count: 1, onLast: true, label: "2개 지금 보내기", phase: "idle", pending: 2 });
    await shot(p, "steer-queued-claude");
    // 휴대폰: 손가락 크기 단추
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
    await openChat(phone.page, "steer-claude");
    await domIs(phone.page, "휴대폰 단추", () => document.querySelector('[data-testid="chat-pending-steer"]')?.getBoundingClientRect().height >= 32);
    await shot(phone.page, "steer-queued-claude-phone");
    await phone.page.context().close();

    // 터미널 입력창에 쓰던 글: sendNow가 그 글까지 보내므로 host가 거절하고 까닭을 답한다. 답은 쓰기 뒤에 오므로 답이 온 뒤 bytes가 그대로면 보내지 않았다
    aaa("pane", "send-text", "steer-claude", "쓰던");
    await waitOutput("steer-claude", "❯ 쓰던");
    let before = fake.bytes().length;
    await p.getByTestId("chat-pending-steer").click();
    await domIs(p, "거절 까닭", () => document.querySelector('[data-testid="chat-pending-note"]')?.textContent.includes("터미널 입력창에 쓰던 글")
      && document.querySelector('[data-testid="chat-pending-steer"]')?.dataset.phase === "failed" && !document.querySelector('[data-testid="chat-pending-steer"]').disabled);
    assert.equal(fake.bytes().length, before, "거절했는데 키를 보냈다");
    await shot(p, "steer-refused-draft");
    aaa("pane", "send-keys", "steer-claude", "ctrl+u");
    await waitOutput("steer-claude", "Press up to edit queued messages");

    // 옛 host(front에 steer_queued가 없음): 대기 말은 그대로, 단추는 없다
    const old = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      wsRoute: [/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
        const server = socket.connectToServer();
        server.onMessage((raw) => {
          if (typeof raw === "string" && raw.includes('"t":"front"')) {
            const m = JSON.parse(raw);
            delete m.steer_queued;
            socket.send(JSON.stringify(m));
          } else socket.send(raw);
        });
        socket.onMessage((raw) => server.send(raw));
      }],
    });
    await openChat(old.page, "steer-claude");
    await domIs(old.page, "옛 host 대기 말", () => document.querySelectorAll(".chat-msg.user.is-pending").length === 2 && document.querySelector('[data-testid="chat-composer"]'));
    if (await old.page.getByTestId("chat-pending-steer").count() !== 0) throw new Error("옛 host에 단추가 있다");
    await old.page.context().close();

    // 누름: 글을 다시 보내지 않고 sendNow(Ctrl+X Ctrl+S)만. 대기열 전부가 흡수되어 보통 말이 된다
    before = fake.bytes().length;
    await p.getByTestId("chat-pending-steer").click();
    await waitOutput("steer-claude", "steered 1");
    assert.equal(fake.bytes().subarray(before).toString(), "\x18\x13");
    await domIs(p, "흡수 뒤 보통 말", () => !document.querySelector('[data-testid="chat-pending"]') && !document.querySelector('[data-testid="chat-pending-steer"]')
      && !document.querySelector('[data-testid="chat-pending-note"]')
      && ["먼저 높이부터", "그리고 다크 테마도"].every((t) => [...document.querySelectorAll('[data-role="user"]')].filter((el) => el.textContent.trim() === t).length === 1));
    await shot(p, "steer-sent-claude");
    const events = aaa("debug", "input", "--pane", "steer-claude", "--since", "1h").events;
    if (!events.some((e) => e.event === "steer_queued" && e.detail.includes("reason=Some(Draft)"))) throw new Error("거절 판정 진단 없음");
    const sent = events.find((e) => e.event === "steer_queued" && e.detail.includes('keys=["ctrl+x", "ctrl+s"]') && e.detail.includes("queue_hint=true"));
    if (!sent || !events.some((e) => e.id === sent.id && e.event === "steer_sent")) throw new Error("보냄 판정·전달 진단이 같은 ID로 없다");
    if (JSON.stringify(events).includes("먼저 높이부터")) throw new Error("진단에 사용자 글이 남았다");
    if (client.errors.length) throw new Error(client.errors.join("; "));
    await p.context().close();
    aaa("pane", "kill", "steer-claude");
  });

  await step("대기 메시지 「지금 보내기」(Codex): 대기 steering 패널이 있으면 Esc만 보내 rollout에 오르고, 패널 없이 넘어간 글은 보내지 않고 까닭(이미 읽었을 수 있음)", async () => {
    const thread = "00000000-0000-7000-8000-000000000417";
    const rollout = `${root}/codex/sessions/2026/10/19/rollout-2026-10-19T00-00-01-${thread}.jsonl`;
    fs.mkdirSync(`${root}/codex/sessions/2026/10/19`, { recursive: true });
    fs.writeFileSync(rollout, JSON.stringify({ type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "코덱스 스티어 첫 요청" }] } }) + "\n");
    const fake = await steerPane("codex", "steer-codex", rollout, thread);
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    await p.getByTestId("agent-steer-codex").and(p.locator('[data-status="working"]')).waitFor({ state: "attached" });
    const box = await openChat(p, "steer-codex");
    await domIs(p, "Codex 기록", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "코덱스 스티어 첫 요청"));
    await box.fill("코덱스 방향 바꿔");
    await press(p);
    await waitOutput("steer-codex", "↳ 코덱스 방향 바꿔");
    await domIs(p, "Codex 단추", () => document.querySelector('[data-testid="chat-pending-steer"]')?.textContent === "지금 보내기");
    await shot(p, "steer-queued-codex");
    let before = fake.bytes().length;
    await p.getByTestId("chat-pending-steer").click();
    await waitOutput("steer-codex", "steered 1");
    assert.equal(fake.bytes().subarray(before).toString(), "\x1b");
    await domIs(p, "Codex 흡수 뒤 보통 말", () => !document.querySelector('[data-testid="chat-pending"]')
      && [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "코덱스 방향 바꿔"));

    // 패널 없이 넘어간 글(실측 0.163): Esc는 그냥 중단이라 보내지 않는다
    fake.hide(true);
    await box.fill("이미 넘어간 글");
    await press(p);
    await waitOutput("steer-codex", "received");
    await domIs(p, "숨은 대기 말 단추", () => [...document.querySelectorAll(".chat-msg.user.is-pending")].some((el) => el.textContent.trim() === "이미 넘어간 글")
      && document.querySelector('[data-testid="chat-pending-steer"]'));
    before = fake.bytes().length;
    await p.getByTestId("chat-pending-steer").click();
    await domIs(p, "패널 없음 까닭", () => document.querySelector('[data-testid="chat-pending-note"]')?.textContent.includes("Codex 화면에 대기 글이 보이지 않습니다"));
    assert.equal(fake.bytes().length, before, "패널이 없는데 Esc를 보냈다");
    await shot(p, "steer-refused-codex");
    // Codex가 기록하면 대기 말과 까닭 줄이 함께 사라진다
    fs.appendFileSync(rollout, JSON.stringify({ type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "이미 넘어간 글" }] } }) + "\n");
    await domIs(p, "흡수 뒤 까닭 줄 없음", () => !document.querySelector('[data-testid="chat-pending"]') && !document.querySelector('[data-testid="chat-pending-note"]'));
    fake.hide(false);
    const events = aaa("debug", "input", "--pane", "steer-codex", "--since", "1h").events;
    if (!events.some((e) => e.event === "steer_queued" && e.detail.includes("reason=Some(NoQueue)"))) throw new Error("패널 없음 판정 진단 없음");
    if (!events.some((e) => e.event === "steer_queued" && e.detail.includes('keys=["esc"]'))) throw new Error("Esc 판정 진단 없음");
    if (client.errors.length) throw new Error(client.errors.join("; "));
    await p.context().close();
    aaa("pane", "kill", "steer-codex");
  });

  await step("대화 이미지: Chrome·Safari·iPhone·iPad에서 올린 그림을 coordinator가 읽고 task 첨부로 thread에 넘긴다", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const digest = crypto.createHash("sha256").update(png).digest("hex");
    const wk = await webkit.launch();
    try {
      for (const name of ["chrome", "safari", "iphone", "ipad"]) {
        const client = name === "iphone" || name === "ipad" ? await pairDevice(wk, aaa("web", "pair").code, name) : await pairPage(name === "chrome" ? browser : wk, aaa("web", "pair").code, { width: 1280, height: 800 });
        const p = client.page;
        const box = await openChat(p);
        const composer = p.getByTestId("chat-composer");
        for (const method of ["paste", "drop", "picker"]) {
          const before = (await sentFrames(p)).length;
          await box.fill(`그림 확인 ${name} ${method}`);
          if (method === "picker") {
            const choose = p.waitForEvent("filechooser");
            await composer.getByRole("button", { name: "그림 첨부", exact: true }).click();
            await (await choose).setFiles(name === "iphone"
              ? { name: "IMG_0001.PNG", mimeType: "", buffer: png }
              : `${root}/pic.png`);
          } else if (!await fileEvent(box, method, png, method === "paste" ? "그림의 대체 텍스트" : "")) throw new Error(`${name} ${method} 파일 사건을 처리하지 않았다`);
          await domIs(p, "첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
          const text = await box.inputValue();
          const path = /^@([^\s]+) /.exec(text)?.[1];
          if (path !== `${project}/uploads/${digest.slice(0, 16)}.png` || !fs.readFileSync(path).equals(png)) throw new Error(`${name} ${method}: uploads 이름·원본과 다르다 ${text}`);
          const sent = await sentFrames(p, before);
          if (sent.some((f) => ["input", "submit", "attach"].includes(f.t))) throw new Error(`보내기 전에 pane에 입력했다 ${JSON.stringify(sent)}`);
          await composer.locator(".composer-attachment img").evaluate((img) => img.decode());
          if (method === "paste") {
            // 대화 → 터미널은 입력창을 실제로 내린다. 초안 경로와 썸네일을 함께 복원해야 한다.
            await p.getByTestId("chat-toggle").click();
            await composer.waitFor({ state: "detached" });
            await p.getByTestId("chat-toggle").click();
            await composer.waitFor();
            await inList(p, "activity-reports");
            await domIs(p, "보고서 화면", () => document.querySelector('[data-testid="reports-view"]')?.dataset.shown === "true");
            await openChat(p);
          } else {
            await p.reload();
            await openChat(p);
          }
          assert.equal(await box.inputValue(), text, `${name} ${method}: 화면 이동 뒤 첨부 초안`);
          await composer.locator(".composer-attachment img").evaluate((img) => img.decode());
          assert.equal(await composer.locator(".composer-attachment").count(), 1, `${name} ${method}: 복원한 미리보기 하나`);
          if (method === "paste") await shot(p, `composer-image-${name}`);
          const got = await submit(p);
          if (got.images.length !== 1 || got.images[0].path !== path || got.images[0].sha256 !== digest || !got.text.includes(`그림 확인 ${name} ${method}`)) throw new Error(`coordinator가 읽은 그림/글 ${JSON.stringify(got)}`);
          if (method === "paste") {
            const turn = p.locator(".chat-turn-user").filter({ hasText: `그림 확인 ${name} ${method}` });
            const thumb = turn.getByRole("button", { name: "첨부 그림 확대", exact: true });
            await thumb.locator("img").evaluate((img) => img.decode());
            if (await thumb.evaluate((el) => el.querySelector("img").getBoundingClientRect().bottom > el.getBoundingClientRect().bottom + 1)) throw new Error(`${name} 기록 썸네일 잘림`);
            await thumb.scrollIntoViewIfNeeded();
            await shot(p, `chat-image-${name}`);
          }
          await composer.locator(".composer-attachment").waitFor({ state: "detached" });
          if (method === "picker") {
            // coordinator가 받은 경로를 task 메모에 넣고 실제 thread 시작까지 간다(휴대폰도 같은 경계).
            const title = `대화 그림 ${name}`;
            const image = `uploads/${digest.slice(0, 16)}.png`;
            const before = fs.readdirSync(`${project}/uploads`).sort();
            const taskUpload = await p.evaluate(async (b64) => {
              const r = await fetch("/api/v1/projects/images/uploads", { method: "POST", body: Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)) });
              return r.json();
            }, png.toString("base64"));
            assert.equal(taskUpload.ok?.path, image, `${name}: 대화와 task 올리기의 이름`);
            assert.deepEqual(fs.readdirSync(`${project}/uploads`).sort(), before, `${name}: 같은 내용은 파일 하나`);
            const edited = await p.evaluate(async ({ title, image }) => {
              const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({
                op: "task_edit", project: "images", base_sha256: null,
                ops: [{ op: "add", list: "Backlog", title, notes: [`![그림](${image})`], person: false }],
              }) });
              return r.json();
            }, { title, image });
            assert.equal(edited.ok?.kind, "task_edited", `${name}: task 메모 첨부 ${JSON.stringify(edited)}`);
            const started = aaa("thread", "start", "images", "--from-task", title, "--link-task", "--machine", "labA", "--profile", "fake", "--kind", "dir");
            const thread = aaa("thread", "show", "images", started.id);
            const copied = `${thread.work_dir}/attachments/${digest.slice(0, 16)}.png`;
            assert.deepEqual(fs.readFileSync(copied), png, `${name}: thread 첨부 바이트`);
            const task = fs.readFileSync(`${project}/threads/${started.id}.task.md`, "utf8");
            assert.ok(task.includes(`![그림](attachments/${digest.slice(0, 16)}.png)`), `${name}: task 사본 경로`);
            await until(`${name} 첨부 thread 보고`, () => aaa("thread", "show", "images", started.id).report_hash);
            const brief = fs.readFileSync(`${thread.work_dir}/brief.md`, "utf8");
            assert.ok(brief.includes(`![그림](attachments/${digest.slice(0, 16)}.png)`), `${name}: brief 사본 경로`);
            aaa("thread", "resolve", "images", started.id);
          }
        }
        // 삭제와 거부도 같은 입력 경계에서: 저장된 그림의 mention은 삭제되고 형식 오류 때 초안은 남는다.
        await fileEvent(box, "paste", png);
        await domIs(p, "삭제할 첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
        await p.reload();
        await openChat(p);
        await composer.locator(".composer-attachment img").evaluate((img) => img.decode());
        // 복원한 그림 뒤에 새 그림을 붙여도 ID가 겹치지 않고 하나씩 지워진다.
        await fileEvent(box, "paste", Buffer.concat([png, Buffer.from([0])]));
        await domIs(p, "복원 뒤 새 첨부 완료", () => document.querySelectorAll(".composer-attachment.is-ready").length === 2);
        await composer.getByRole("button", { name: `${digest.slice(0, 16)}.png 첨부 삭제`, exact: true }).click();
        assert.equal(await composer.locator(".composer-attachment").count(), 1, `${name}: 복원한 그림만 삭제`);
        assert.ok(!(await box.inputValue()).includes(`${digest.slice(0, 16)}.png`), `${name}: 삭제한 경로 제거`);
        await composer.getByRole("button", { name: "clipboard.png 첨부 삭제" }).click();
        if ((await box.inputValue()) !== "") throw new Error("삭제한 그림의 경로가 남았다");
        await p.reload();
        await openChat(p);
        assert.equal(await composer.locator(".composer-attachment").count(), 0, `${name}: 지운 미리보기가 돌아오지 않음`);
        await box.fill("실패해도 남을 글");
        await fileEvent(box, "paste", Buffer.from("invalid-image"));
        await composer.getByRole("alert").waitFor();
        if (!(await composer.getByRole("alert").innerText()).includes("PNG·JPEG·GIF·WebP") || (await box.inputValue()) !== "실패해도 남을 글") throw new Error("형식 거부가 초안을 지웠다");
        if (client.errors.length) throw new Error(client.errors.join("; "));
        await p.context().close();
      }
    } finally { await wk.close(); }
    // 새 HTTP 경계의 권한·Origin. 본문을 읽기 전 거부한다.
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const pane = aaa("pane", "list").panes.find((p) => p.name === "composer").id;
    const status = await rd.page.evaluate(async (pane) => (await fetch(`/api/v1/panes/${pane}/image`, { method: "POST", body: new Uint8Array([137,80,78,71,13,10,26,10]) })).status, pane);
    if (status !== 403) throw new Error(`reader upload ${status}`);
    const origin = new URL(rd.page.url()).origin;
    const wrongOrigin = await rd.page.context().request.post(`${origin}/api/v1/panes/${pane}/image`, { headers: { Origin: "https://example.ts.net" }, data: "image" });
    if (wrongOrigin.status() !== 403) throw new Error(`다른 Origin upload ${wrongOrigin.status()}`);
    const anonymous = await browser.newContext();
    const noCookie = await anonymous.request.post(`${origin}/api/v1/panes/${pane}/image`, { headers: { Origin: origin }, data: "image" });
    if (noCookie.status() !== 401) throw new Error(`미인증 upload ${noCookie.status()}`);
    await anonymous.close();
    await rd.page.context().close();
    // 프로젝트 밖의 대화 첨부는 agent에 조각으로 쓰는 별도 저장 경계다(512KiB 넘는 그림).
    const other = aaa("tab", "create", "--machine", "labA", "--label", "composer-other", "--cwd", root, "--", "/bin/sh", "-c",
      '"$0" hook --agent claude \'{"hook_event_name":"SessionStart","session_id":"composer-other","source":"startup"}\'; stty raw -echo; printf "composer-other-ready\\n"; exec cat', AA).tabs.at(-1).root.pane;
    await waitOutput(other, "composer-other-ready");
    const generic = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const box = await openChat(generic.page, "composer-other");
    const large = Buffer.concat([png, Buffer.alloc(1_200_000, 7)]);
    await fileEvent(box, "paste", large);
    await domIs(generic.page, "agent 첨부 응답", () => document.querySelector(".composer-attachment:is(.is-ready, .is-error)") !== null);
    const path = /^@([^\s]+) /.exec(await box.inputValue())?.[1];
    if (!path?.startsWith(`${root}/s/agent/sessions/`) || !fs.readFileSync(path).equals(large)) throw new Error("agent에 저장한 여러 조각 그림이 다르다");
    await generic.page.context().close();
  });

  await step("옛 대화 첨부: 원본을 보존하며 안내하고 다시 올린 새 링크로 시작하면 같은 바이트가 전달된다", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const legacy = `${project}/uploads/0123456789abcdef.png`;
    fs.writeFileSync(legacy, png);
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const edit = async (op) => client.page.evaluate(async (op) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({ op: "task_edit", project: "images", ops: [op], base_sha256: null }) });
      return r.json();
    }, op);
    const title = "옛 대화 그림 복구";
    assert.equal((await edit({ op: "add", list: "Backlog", title, notes: ["![그림](uploads/0123456789abcdef.png)"] })).ok?.kind, "task_edited");
    const start = () => aaa("thread", "start", "images", "--from-task", title, "--machine", "labA", "--profile", "fake", "--kind", "dir");
    assert.throws(start, /이름과 내용 해시가 다르다.*예전 대화 첨부.*할 일에 다시 첨부/);
    assert.deepEqual(fs.readFileSync(legacy), png, "거부한 원본 보존");
    const saved = await client.page.evaluate(async (b64) => {
      const r = await fetch("/api/v1/projects/images/uploads", { method: "POST", body: Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)) });
      return r.json();
    }, png.toString("base64"));
    const expected = `uploads/${crypto.createHash("sha256").update(png).digest("hex").slice(0, 16)}.png`;
    assert.equal(saved.ok?.path, expected);
    assert.equal((await edit({ op: "update", at: { title }, notes: [`![그림](${expected})`] })).ok?.kind, "task_edited");
    const started = start();
    const thread = aaa("thread", "show", "images", started.id);
    assert.deepEqual(fs.readFileSync(`${thread.work_dir}/attachments/${expected.split("/")[1]}`), png);
    assert.deepEqual(fs.readFileSync(legacy), png, "다시 올린 뒤에도 옛 이름·내용 보존");
    await until("새 링크 thread 보고", () => aaa("thread", "show", "images", started.id).report_hash);
    aaa("thread", "resolve", "images", started.id);
    await client.page.context().close();
  });

  await step("대화 Enter: 터치 기기는 줄바꿈·버튼 보내기, 데스크톱 Enter/Shift+Enter, 한국어 IME 확정은 무전송", async () => {
    const wk = await webkit.launch();
    try {
      for (const name of ["iphone", "ipad", "chrome", "safari"]) {
        const client = name === "iphone" || name === "ipad" ? await pairDevice(wk, aaa("web", "pair").code, name) : await pairPage(name === "chrome" ? browser : wk, aaa("web", "pair").code, { width: 1280, height: 800 });
        const p = client.page;
        const box = await openChat(p);
        await box.fill("");
        await box.fill("첫 줄");
        const touch = name === "iphone" || name === "ipad";
        let mark = (await sentFrames(p)).length;
        await box.press(touch ? "Enter" : "Shift+Enter");
        await box.pressSequentially("둘째 줄");
        if ((await box.inputValue()) !== "첫 줄\n둘째 줄" || (await sentFrames(p, mark)).some((f) => f.t === "submit")) throw new Error(`${name} 줄바꿈 대신 전송했다`);
        await box.evaluate((el) => {
          el.dispatchEvent(new CompositionEvent("compositionstart", { data: "한", bubbles: true }));
          el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true, cancelable: true }));
          el.dispatchEvent(new CompositionEvent("compositionend", { data: "한", bubbles: true }));
          el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", keyCode: 229, bubbles: true, cancelable: true }));
        });
        if ((await sentFrames(p, mark)).some((f) => f.t === "submit")) throw new Error(`${name} 조합 확정이 전송했다`);
        await shot(p, `composer-multiline-${name}`);
        const got = await submit(p);
        if (got.text !== "첫 줄\n둘째 줄") throw new Error(`줄바꿈 전달 ${JSON.stringify(got)}`);
        if (!touch) {
          await box.fill("desktop-enter");
          await box.press("Enter");
          await waitOutput("composer", `CMP_RECEIVED_${++received}`);
          await domIs(p, "Enter 전송 완료", () => document.querySelector('[aria-label="메시지"]').value === "");
          if (JSON.parse(fs.readFileSync(receipts, "utf8")).text !== "desktop-enter") throw new Error("desktop Enter 전송이 다르다");
        }
        await p.context().close();
      }
    } finally { await wk.close(); }
  });
}
