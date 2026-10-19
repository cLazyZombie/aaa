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

export default async function () {
  const project = `${root}/projects/images`;
  const receipts = `${root}/composer-received.json`;
  await step("대화 첨부 실험실 준비", async () => {
    aaa("project", "create", "images");
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
            message=text.decode(); text=b''; count+=1
            paths=re.findall(r'@([^\\s]+)',message)
            images=[{'path':p,'sha256':hashlib.sha256(open(p,'rb').read()).hexdigest()} for p in paths]
            open(sys.argv[2],'w').write(json.dumps({'text':message,'images':images},ensure_ascii=False))
            subprocess.run([sys.argv[1],'hook','--agent','claude',json.dumps({'hook_event_name':'UserPromptSubmit','session_id':'composer'})],check=True,stdout=subprocess.DEVNULL)
            print('\\r\\nCMP_RECEIVED_'+str(count),flush=True)
        else: text+=b'\\n' if ch==b'\\r' else ch
`);
    aaa("tab", "create", "--machine", "labA", "--label", "composer", "--cwd", project, "--", "/bin/sh", "-c",
      '"$0" hook --agent claude \'{"hook_event_name":"SessionStart","session_id":"composer","source":"startup"}\'; stty raw -echo; exec python3 "$1" "$0" "$2"', AA, `${root}/composer.py`, receipts);
    await waitOutput("composer", "composer-ready");
  }, { always: true });

  let received = 0;
  const submit = async (p) => {
    await p.getByRole("button", { name: "메시지 보내기", exact: true }).click();
    await waitOutput("composer", `CMP_RECEIVED_${++received}`);
    await domIs(p, "전송 완료", () => document.querySelector('[aria-label="메시지"]').value === "");
    return JSON.parse(fs.readFileSync(receipts, "utf8"));
  };

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
