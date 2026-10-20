// 웹 E2E 대화에서 연 보고서의 닫기(사용자 10-20 「iPhone에서 대화창에서 보고서 글을 열면 다시 대화로 돌아갈 방법이 없어」, plan §8.300).
// 대화 글의 보고서 주소(`/v/<cap>/` 맨 주소·Markdown 링크·backtick 주소), 앱 보고서 주소(`/#report=`), 파일 경로(보고 사본)를
// 휴대폰(Safari 탭·홈 화면 앱)에서 누르면 새 창이 아니라 앱 안 보기로 열리고, 닫기·뒤로(edge swipe)가 같은 pane·같은 굴림 자리로
// 돌아온다. 데스크톱은 새 탭 습관 그대로다. 실제 iOS 홈 화면 앱이 새 창을 어떻게 띄우는지는 headless가 보이지 않는다(실기 확인).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 38-chat-report-close(새 실험실).

import fs from "node:fs";
import { aaa, until, step, browser, webkit, pairPage, inList, domIs, shot, root, AA, base, paneHash } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const LABEL = "rep1";

/** 보고서 링크가 기록 가운데에 든 긴 Claude 대화(hook으로 기록 자리를 알린다, 22-chat-surface와 같은 길) */
function chatPane(lines) {
  const dir = `${root}/claude/projects/-lab-${LABEL}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  const say = (text) => L({ type: "assistant", message: { content: [{ type: "text", text }] } });
  let body = "";
  for (let i = 0; i < 20; i++) body += L({ type: "user", message: { role: "user", content: `질문 ${i}` } }) + say(`답 ${i}\n\n${"긴 줄 ".repeat(40)}`);
  for (const text of lines) body += say(text) + say(`${"사이 글 ".repeat(60)}`);
  for (let i = 20; i < 40; i++) body += L({ type: "user", message: { role: "user", content: `질문 ${i}` } }) + say(`답 ${i}\n\n${"긴 줄 ".repeat(40)}`);
  fs.writeFileSync(tr, body);
  aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b38", source: "startup", transcript_path: tr }));
}

async function openChat(p) {
  await inList(p, `pane-${LABEL}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  await p.getByTestId("chat-composer").waitFor();
  await until("대화 기록", async () => (await p.locator('[data-testid="chat"] .chat-msg').count()) >= 60);
}

/** 대화 보기의 굴림 자리와 주소(같은 pane·같은 자리로 돌아왔는지 본다) */
const where = (p) => p.evaluate(() => ({ hash: location.hash, top: Math.round(document.querySelector('[data-testid="chat"]').scrollTop) }));

/**
 * 그 링크를 기록 가운데로 굴려 누르고, 앱 안 보기가 열리는지(새 창이 열리면 실패) 본다. 닫는 길(`close`: 닫기 단추, `back`: 브라우저
 * 뒤로 = iOS 가장자리 쓸기) 뒤 같은 주소·같은 굴림 자리인지 본다. 닫기는 위 안전 영역 아래, 화면 안에 손이 닿는 크기로 있어야 한다.
 */
async function openAndClose(p, link, name, how, shotName) {
  await link.evaluate((el) => el.scrollIntoView({ block: "center" }));
  await p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const before = await where(p);
  const ctx = p.context();
  const popup = ctx.waitForEvent("page", { timeout: 20_000 }).then((w) => ({ popup: w }), () => null);
  await link.click();
  const viewer = p.getByTestId("file-viewer");
  const opened = await Promise.race([popup, viewer.waitFor().then(() => ({ viewer: true }))]);
  if (opened?.popup) {
    await opened.popup.waitForLoadState().catch(() => {});
    if (shotName) await shot(opened.popup, `${shotName}-popup`);
    const back = await opened.popup.evaluate(() => history.length);
    await opened.popup.close();
    throw new Error(`${name}: 새 창으로 열렸다(앱 화면·닫기 없음, 새 창 기록 ${back}칸)`);
  }
  // 「새 창이 없음」은 앱 안 보기가 그려진 울타리 뒤에 본다
  if (ctx.pages().length !== 1) throw new Error(`${name}: 앱 안 보기와 함께 새 창도 열렸다`);
  const close = viewer.getByRole("button", { name: /닫기/ });
  await close.waitFor();
  // 휴대폰 sheet는 아래에서 올라온다: 다 올라온 뒤에 자리를 잰다
  await viewer.evaluate((el) => Promise.all(el.closest(".modal-scrim").getAnimations({ subtree: true }).map((a) => a.finished)));
  const box = await close.boundingBox();
  const vp = p.viewportSize();
  if (!box || box.y < 0 || box.x < 0 || box.x + box.width > vp.width + 1 || box.y + box.height > vp.height || box.width < 44 || box.height < 44)
    throw new Error(`${name}: 닫기 단추 자리·크기 ${JSON.stringify(box)} (화면 ${vp.width}×${vp.height}, 44px 이상)`);
  // 손가락(coarse)이면 머리 단추 모두 44px 이상(herdr/FileViewer.css 규칙이 primitives.css에 밀리지 않게)
  const small = await viewer.evaluate((el) => matchMedia("(pointer: coarse)").matches
    ? [...el.querySelectorAll(".file-viewer-header .file-viewer-action")].map((b) => b.getBoundingClientRect()).filter((r) => r.width < 44 || r.height < 44).map((r) => r.width)
    : []);
  if (small.length) throw new Error(`${name}: 44px보다 작은 머리 단추 ${JSON.stringify(small)}`);
  if (shotName) await shot(p, shotName);
  if (how === "close") await close.click();
  else await p.evaluate(() => history.back());
  await viewer.waitFor({ state: "detached" });
  const after = await where(p);
  if (after.hash !== before.hash || Math.abs(after.top - before.top) > 1)
    throw new Error(`${name}(${how}): 돌아온 자리 ${JSON.stringify(after)} ≠ 연 자리 ${JSON.stringify(before)}`);
}

export default async function () {
  let report;
  let links;
  await step("준비: 보고서 publish와 보고서 링크가 든 대화", async () => {
    aaa("project", "create", "demo");
    const input = `${root}/report-38`;
    fs.mkdirSync(input, { recursive: true });
    fs.writeFileSync(`${input}/index.html`, `<!doctype html><html lang=ko><meta charset=utf-8><meta name=viewport content="width=device-width"><h1>대화 보고서</h1>${"<p>긴 보고서 문단</p>".repeat(200)}</html>`);
    report = aaa("library", "publish", input, "--project", "demo", "--name", "chatrep", "--to-user");
    fs.mkdirSync(`${root}/files`, { recursive: true });
    fs.writeFileSync(`${root}/files/report-copy.md`, `# 보고 사본\n\n${"보고 사본 줄\n\n".repeat(80)}`);
    const url = new URL(report.base, base).href;
    links = {
      bare: { text: `보기: ${url}`, link: (chat) => chat.getByRole("link", { name: url }).first() },
      markdown: { text: `[대화 보고서 열기](${url}index.html)`, link: (chat) => chat.getByRole("link", { name: "대화 보고서 열기" }) },
      code: { text: `주소는 \`${url}\` 이다`, link: (chat) => chat.locator("a.markdown-code-link").first() },
      app: { text: `[앱 보고서 화면](${base}#report=demo/chatrep)`, link: (chat) => chat.getByRole("link", { name: "앱 보고서 화면" }) },
      file: { text: `보고 사본: ${root}/files/report-copy.md`, link: (chat) => chat.getByRole("button", { name: "report-copy.md", exact: false }).first() },
    };
    chatPane(Object.values(links).map((l) => l.text));
  });

  await step("대화에서 연 보고서: iPhone 390·430 Safari 탭·홈 화면 앱(WebKit)에서 닫기·뒤로가 같은 pane·같은 굴림 자리로 돌아온다", async () => {
    const wk = await webkit.launch();
    try {
      for (const width of [390, 430]) for (const standalone of [false, true]) {
        const tag = `${width}-${standalone ? "app" : "safari"}`;
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone });
        try {
          if (width !== 390) await p.setViewportSize({ width, height: 932 });
          await openChat(p);
          const chat = p.getByTestId("chat");
          const pane = paneHash(LABEL);
          await until("pane 주소", async () => (await p.evaluate(() => location.hash)) === pane);
          for (const [kind, l] of Object.entries(links))
            for (const how of ["close", "back"])
              await openAndClose(p, l.link(chat), `${tag} ${kind}`, how, how === "close" && (kind === "bare" || kind === "file") ? `chat-report-${tag}-${kind}` : null);
          await domIs(p, "대화 그대로", () => document.querySelector('[data-testid="chat"]') !== null);
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });

  await step("데스크톱은 대화의 보고서 주소를 새 탭으로 연다(새 탭 습관 유지)", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      await openChat(p);
      const link = links.bare.link(p.getByTestId("chat"));
      await link.evaluate((el) => el.scrollIntoView({ block: "center" }));
      const [w] = await Promise.all([p.context().waitForEvent("page"), link.click()]);
      await w.waitForLoadState();
      if (!w.url().includes(report.base)) throw new Error(`새 탭 주소 ${w.url()}`);
      if ((await p.getByTestId("file-viewer").count()) !== 0) throw new Error("데스크톱에서 앱 안 보기도 열었다");
      await w.close();
    } finally {
      await p.context().close();
    }
  });
}
