// 대화 첨부는 저장 → 초안의 @경로 → 보내기 → coordinator의 파일 읽기까지 실제 host·agent로 본다.
// 참고: herdr-web-ui Composer(onPaste/onDrop/uploadImages), compose(imageMention/insertLeadingMention).
import fs from "node:fs";
import crypto from "node:crypto";
import { aaa, step, browser, webkit, pairPage, inList, domIs, root, AA, sentFrames, waitOutput, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

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
            if not os.path.exists(sys.argv[4]):
                with open(sys.argv[3],'a') as f: f.write(json.dumps({'type':'user','message':{'content':message}},ensure_ascii=False)+'\\n')
            subprocess.run([sys.argv[1],'hook','--agent','claude',json.dumps({'hook_event_name':'UserPromptSubmit','session_id':'composer'})],check=True,stdout=subprocess.DEVNULL)
            print('\\r\\nCMP_RECEIVED_'+str(count),flush=True)
        else: text+=b'\\n' if ch==b'\\r' else ch
`);
    aaa("tab", "create", "--machine", "labA", "--label", "composer", "--cwd", project, "--", "/bin/sh", "-c",
      `"$0" hook --agent claude '{"hook_event_name":"SessionStart","session_id":"composer","source":"startup","transcript_path":"${transcript}"}'; stty raw -echo; exec python3 "$1" "$0" "$2" "$3" "$4"`, AA, `${root}/composer.py`, receipts, transcript, hold);
    await waitOutput("composer", "composer-ready");
  }, { always: true });

  let received = 0;
  const submit = async (p) => {
    await p.getByRole("button", { name: "메시지 보내기", exact: true }).click();
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await domIs(p, "전송 완료", () => document.querySelector('[aria-label="메시지"]').value === "");
    return JSON.parse(fs.readFileSync(receipts, "utf8"));
  };

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

  await step("처리 확인: PTY 접수·기록 수신을 구분하고 미확인·늦은 확인·같은 글 재전송을 안전하게 안내한다", async () => {
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = client.page;
    const box = await openChat(p);
    await domIs(p, "기존 같은 글 기록", () => document.querySelector('[data-role="user"]')?.textContent.includes("같은 글 재전송"));
    fs.writeFileSync(hold, "hold");
    // Node의 현재 시각은 CDP 왕복 중 과거가 될 수 있다. 제출 전에 고정한 가상 시각으로 멈춘다.
    await p.clock.install({ time: new Date("2030-01-01T00:00:00Z") });
    await p.clock.pauseAt(new Date("2030-01-02T00:00:00Z"));
    await box.fill("같은 글 재전송");
    const before = (await sentFrames(p)).length;
    await submit(p);
    await domIs(p, "기록 확인 대기", () => document.querySelector('[data-testid="chat-delivered"]')?.getAttribute("data-state") === "waiting");
    await box.fill("새 초안은 보존");
    // unrelated transcript activity and an older identical message are not this send's receipt.
    fs.appendFileSync(transcript, chatLine("다른 기기 메시지"));
    await p.clock.runFor(10_001);
    await domIs(p, "아직 처리되지 않음", () => document.querySelector('[data-testid="chat-delivered"]')?.getAttribute("data-state") === "unconfirmed");
    await shot(p, "chat-input-unconfirmed");
    if (!(await p.getByTestId("chat-delivered").innerText()).includes("아직 처리되지 않음")) throw new Error("미처리 안내가 없다");
    if ((await sentFrames(p, before)).filter((f) => f.t === "submit").length !== 1) throw new Error("미확인 입력을 자동 재전송했다");
    const restore = p.getByRole("button", { name: "보낸 글 복원", exact: true });
    if (!await restore.isDisabled() || await box.inputValue() !== "새 초안은 보존") throw new Error("미확인 안내가 새 초안을 덮었다");
    await box.fill("");
    await restore.click();
    if (await box.inputValue() !== "같은 글 재전송") throw new Error("보낸 글을 복원하지 못했다");
    fs.appendFileSync(transcript, chatLine("같은 글 재전송"));
    await p.clock.resume();
    await domIs(p, "보낸 글의 새 사용자 기록", () => [...document.querySelectorAll('[data-role="user"]')].filter((el) => el.textContent.trim() === "같은 글 재전송").length === 2);
    await domIs(p, "기록 반영 뒤 안내 사라짐", () => document.querySelector('[data-testid="chat-delivered"]') === null);
    await shot(p, "chat-input-reflected");
    if ((await sentFrames(p, before)).filter((f) => f.t === "submit").length !== 1 || await box.inputValue() !== "같은 글 재전송") throw new Error("확인 사건이 재전송하거나 초안을 지웠다");
    fs.unlinkSync(hold);
    await box.fill("386 추천대로 해");
    await submit(p);
    await domIs(p, "즉시 반영된 사용자 메시지", () => [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "386 추천대로 해"));
    await domIs(p, "즉시 반영 뒤 안내 없음", () => document.querySelector('[data-testid="chat-delivered"]') === null);
    await shot(p, "chat-input-reflected-immediate");
    if ((await sentFrames(p, before)).filter((f) => f.t === "submit").length !== 2 || await box.inputValue() !== "") throw new Error("반영 확인이 글을 다시 보내거나 보낸 초안을 남겼다");
    await p.context().close();
  });

  await step("대화 이미지: Chrome·Safari·iPhone·iPad의 붙여넣기·드롭·파일 선택은 uploads에 저장하고 보내기로 coordinator가 읽는다", async () => {
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
            await (await choose).setFiles(`${root}/pic.png`);
          } else if (!await fileEvent(box, method, png, method === "paste" ? "그림의 대체 텍스트" : "")) throw new Error(`${name} ${method} 파일 사건을 처리하지 않았다`);
          await domIs(p, "첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
          const text = await box.inputValue();
          const path = /^@([^\s]+) /.exec(text)?.[1];
          if (!path?.startsWith(`${project}/uploads/`) || !fs.readFileSync(path).equals(png)) throw new Error(`${name} ${method}: uploads 원본과 다르다 ${text}`);
          const sent = await sentFrames(p, before);
          if (sent.some((f) => ["input", "submit", "attach"].includes(f.t))) throw new Error(`보내기 전에 pane에 입력했다 ${JSON.stringify(sent)}`);
          await composer.locator(".composer-attachment img").evaluate((img) => img.decode());
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
        }
        // 삭제와 거부도 같은 입력 경계에서: 저장된 그림의 mention은 삭제되고 형식 오류 때 초안은 남는다.
        await fileEvent(box, "paste", png);
        await domIs(p, "삭제할 첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
        await composer.getByRole("button", { name: "clipboard.png 첨부 삭제" }).click();
        if ((await box.inputValue()) !== "") throw new Error("삭제한 그림의 경로가 남았다");
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
