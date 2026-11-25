// 실제 TLS agent의 보고 사본과 coordinator 대화 기록 양쪽을 통과시킨다.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { once } from "node:events";
import fs from "node:fs";
import { root, AA, aaa, base, browser, inTime, pairPage, specCleanup, start, step, stop, until, waitOutput, webkit } from "../lib/harness.mjs";
import { terminateSessionHosts } from "../lib/session-cleanup.mjs";

const config = `${root}/c/host.toml`;
const transcript = `${root}/claude/projects/-lab-media/chat.jsonl`;
const clipPath = "영상 시안.mp4";
const quote = s => "'" + s.replaceAll("'", "'\\''") + "'";
const hook = { hook_event_name: "SessionStart", session_id: "e13d00a0-8ff8-4543-80a3-21182e4796f1", source: "startup", transcript_path: transcript };
fs.mkdirSync(`${root}/claude/projects/-lab-media`, { recursive: true });
fs.writeFileSync(transcript, "");
fs.writeFileSync(config, fs.readFileSync(config, "utf8").replace('agent_listen = ""', 'agent_listen = "127.0.0.1:0"') +
  `\n[profiles.media]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/media.sh"]\n`);
fs.writeFileSync(`${root}/media.sh`, `#!/bin/sh
if [ "$1" = "--version" ]; then printf 'Claude Code test\\n'; exit 0; fi
${quote(AA)} hook --agent claude ${quote(JSON.stringify(hook))} > ${quote(`${root}/media-hook.json`)}
printf 'media-ready> '
while IFS= read -r line; do printf 'media-ready> '; done
`);

async function startRemote() {
  const address = aaa("status").agent_listen;
  const remote = { AAA_CONFIG_DIR: `${root}/remote/c`, AAA_STATE_DIR: `${root}/remote/s` };
  fs.mkdirSync(remote.AAA_CONFIG_DIR, { recursive: true });
  fs.writeFileSync(`${remote.AAA_CONFIG_DIR}/agent.toml`, `schema = 1\n[agent]\nwork_root = "${root}/remote/work"\n`);
  aaa("machine", "enroll", "--open", "5m");
  const enrolled = once(start("media-enroll", remote, "agent", "enroll", address, "--name", "labB"), "exit");
  const code = await until("원격 agent 승인 코드", () => /--code\s+(\S+)/.exec(fs.readFileSync(`${root}/media-enroll.log`, "utf8"))?.[1]);
  aaa("machine", "approve", "labB", "--code", code);
  assert.equal((await inTime(enrolled, 20_000, "원격 등록 종료"))[0], 0);
  const running = specCleanup.own(
    () => start("media-remote", remote, "agent", "--host", address, "--name", "labB"),
    async () => {
      await stop("media-remote");
      await terminateSessionHosts(`${remote.AAA_STATE_DIR}/agent/sessions`);
    },
  );
  await running.value;
  await until("TLS agent 연결", () => aaa("machine", "list").machines.some(m => m.name === "labB" && m.connection === "connected"));
  return running.close;
}

const readyImage = async (locator) => {
  await locator.scrollIntoViewIfNeeded();
  await locator.evaluate(img => img.decode());
};

async function checkConversation(engine, viewport, pane, imageUrl, clipUrl) {
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, viewport, { hash: `#pane=${pane}`, serviceWorkers: "block" });
  const requests = [];
  page.on("response", r => {
    if (new URL(r.url()).pathname === clipUrl) requests.push([r.request().headers().range, r.status(), r.headers()["content-range"]]);
  });
  const player = page.getByTestId("lightbox").locator("video");
  try {
    const chat = page.getByTestId("chat");
    await page.getByTestId("chat-toggle").waitFor();
    if (!(await chat.count())) await page.getByTestId("chat-toggle").click();
    const attached = chat.getByRole("button", { name: "첨부 그림 확대" }).locator("img");
    const image = chat.getByRole("button", { name: "결과 그림 확대" });
    const clip = chat.getByRole("button", { name: "원격 영상 크게 재생" });
    const clipThumb = clip.locator('img[src^="data:image/jpeg"]');
    const modal = page.getByTestId("lightbox");
    await readyImage(attached);
    await readyImage(image.locator("img"));
    await readyImage(clipThumb);
    assert.equal(await image.locator("img").getAttribute("src"), imageUrl);
    assert.equal(await clip.locator("video").count(), 0, "첫 프레임을 얻은 뒤 영상 디코더를 놓는다");
    const box = await attached.boundingBox();
    for (const thumb of [image.locator("img"), clipThumb]) {
      const size = await thumb.boundingBox();
      assert.deepEqual([size.width, size.height], [box.width, box.height], "사용자 업로드와 같은 크기");
      assert.ok(size.x >= 0 && size.x + size.width <= viewport.width, "썸네일이 화면에 들어간다");
    }
    assert.equal(await chat.locator(".chat-msg.user .chat-reply-media").count(), 0);
    assert.equal(await chat.locator('img[src*="example.test"]').count(), 0);

    await image.click();
    await readyImage(modal.locator("img"));
    assert.equal(await modal.getByRole("link", { name: "원본" }).getAttribute("href"), imageUrl);
    assert.equal((await modal.boundingBox()).width, viewport.width);
    if (viewport.width < 700) {
      await inTime(page.evaluate(() => new Promise(resolve => {
        addEventListener("popstate", () => resolve(), { once: true });
        history.back();
      })), 20_000, "휴대폰 확대 화면 뒤로");
    } else {
      await page.keyboard.press("Escape");
    }
    await modal.waitFor({ state: "hidden" });
    await clip.click();
    await player.waitFor();
    const playback = await inTime(player.evaluate(v => new Promise((resolve, reject) => {
      const played = () => {
        if (v.currentTime <= 0) return;
        v.removeEventListener("timeupdate", played);
        v.pause();
        resolve({ controls: v.controls, inline: v.playsInline, width: v.getBoundingClientRect().width });
      };
      v.addEventListener("timeupdate", played);
      v.addEventListener("error", () => reject(new Error(`영상 오류 ${v.error?.code}`)), { once: true });
      played();
    })), 20_000, "확대 영상 재생");
    assert.equal(await player.getAttribute("src"), clipUrl);
    assert.ok(playback.controls && playback.inline && playback.width > box.width && playback.width <= viewport.width);
    const seeked = await inTime(player.evaluate(v => new Promise(resolve => {
      v.addEventListener("seeked", () => resolve(v.currentTime), { once: true });
      v.currentTime = 1.5;
    })), 20_000, "확대 영상 탐색");
    assert.equal(seeked, 1.5);
    await modal.getByRole("button", { name: "닫기" }).click();
    await modal.waitFor({ state: "hidden" });
    assert.equal(await clipThumb.isVisible(), true);
    assert.deepEqual(errors, []);
  } catch (error) {
    const state = await player.evaluateAll(videos => videos.map(v => ({
      time: v.currentTime, duration: v.duration, seeking: v.seeking, paused: v.paused,
      ready: v.readyState, network: v.networkState, error: v.error?.code,
    }))).catch(() => []);
    throw new Error(`${engine.browserType().name()} ${viewport.width}px: ${error.message}, 재생기 ${JSON.stringify(state)}, 응답 ${JSON.stringify(requests)}`, { cause: error });
  } finally {
    await page.context().close();
  }
}

export default async function ({ page }) {
  let pane, picture, video, imageUrl, clipUrl, thread;
  const fixture = new URL("../fixtures/report-video.mp4", import.meta.url);
  await step("대화 첨부 준비: host 이미지와 TLS agent 영상의 불변 보고 판", async () => {
    aaa("project", "create", "demo");
    const disconnect = await startRemote();
    picture = aaa("library", "publish", `${root}/pic.png`, "--name", "chat-picture").report_id;
    fs.writeFileSync(`${root}/media-task.md`, "재생 시안을 보고한다\n");
    thread = aaa("thread", "start", "demo", "--title", "원격 영상", "--machine", "labB", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/media-task.md`);
    await until("원격 agent 첫 보고", () => aaa("thread", "show", thread.id).report_hash);
    fs.copyFileSync(fixture, `${thread.work_dir}/library/${clipPath}`);
    const report = "[진행 중] 원격 영상 시안\n";
    fs.writeFileSync(`${thread.work_dir}/report.md`, report);
    const digest = createHash("sha256").update(report).digest("hex");
    video = await until("원격 영상을 복사한 보고 판", () => {
      const view = aaa("library", "show", thread.id);
      return createHash("sha256").update(view.report ?? "").digest("hex") === digest &&
        view.files.some(f => f.path === clipPath && !f.on_agent) && view.report_id;
    });
    await disconnect();
    imageUrl = `/api/v1/chat-media/${picture}/pic.png`;
    clipUrl = `/api/v1/chat-media/${video}/${encodeURIComponent(clipPath)}`;

    pane = aaa("project", "open", "demo", "--profile", "media").pane.id;
    await waitOutput(pane, "media-ready>");
    const png = fs.readFileSync(`${root}/pic.png`).toString("base64");
    const uploaded = await page.evaluate(async ({ pane, png }) => {
      const body = Uint8Array.from(atob(png), char => char.charCodeAt(0));
      const response = await fetch(`/api/v1/panes/${pane}/image`, { method: "POST", body });
      return response.json();
    }, { pane, png });
    assert.ok(uploaded.ok?.path, JSON.stringify(uploaded));
    fs.writeFileSync(transcript, [
      { type: "user", message: { role: "user", content: `@${uploaded.ok.path} 화면을 보여줘.\n![사용자 문법](library/${picture}/pic.png)` } },
      { type: "assistant", message: { role: "assistant", content: [{ type: "text", text:
        `확인한 화면입니다.\n\n![결과 그림](library/${picture}/pic.png) ![원격 영상](library/${video}/${encodeURIComponent(clipPath)})\n\n![외부 그림](https://example.test/pic.png)` }] } },
    ].map(x => JSON.stringify(x)).join("\n") + "\n");
  });

  await step("대화 첨부 권한: pairing, 보고 판 경계, 영상 HEAD·Range", async () => {
    const request = page.context().request;
    const body = fs.readFileSync(fixture);
    const head = await request.head(new URL(clipUrl, base).href);
    assert.equal(head.status(), 200);
    assert.equal(head.headers()["content-length"], String(body.length));
    assert.equal(head.headers()["content-type"], "video/mp4");
    assert.equal((await head.body()).length, 0);
    const range = await request.get(new URL(clipUrl, base).href, { headers: { range: "bytes=2-11" } });
    assert.equal(range.status(), 206);
    assert.equal(range.headers()["content-range"], `bytes 2-11/${body.length}`);
    assert.deepEqual(await range.body(), body.subarray(2, 12));
    const invalid = await request.get(new URL(clipUrl, base).href, { headers: { range: `bytes=${body.length}-` } });
    assert.equal(invalid.status(), 416);
    const anon = await browser.newContext();
    try {
      assert.equal((await anon.request.get(new URL(imageUrl, base).href)).status(), 401);
    } finally {
      await anon.close();
    }
    assert.equal((await request.get(new URL(imageUrl, base).href, { headers: { "sec-fetch-site": "cross-site" } })).status(), 403);
    assert.equal((await request.get(new URL(imageUrl, base).href, { headers: { origin: "null" } })).status(), 403);
    assert.equal((await request.get(base + "api/v1/chat-media/chat-picture/pic.png")).status(), 400);
    assert.equal((await request.get(base + `api/v1/chat-media/${video}/index.html`)).status(), 400);
    assert.equal((await request.get(base + `api/v1/chat-media/${picture}/${encodeURIComponent(clipPath)}`)).status(), 404);
  });

  await step("실제 coordinator 대화: Chromium·WebKit 데스크톱·휴대폰 미리보기·확대·재생", async () => {
    const safari = await webkit.launch();
    try {
      for (const engine of [browser, safari]) {
        for (const width of [1160, 390]) {
          await checkConversation(engine, { width, height: 780 }, pane, imageUrl, clipUrl);
        }
      }
    } finally {
      await safari.close();
    }
  });
}
