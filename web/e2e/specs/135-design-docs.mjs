// 기획(t-1894): 프로젝트 저장소의 기획 폴더(`aaa.toml [[workspaces]]`의 `design`)를 실제 git origin·agent로 읽는다.
// 사이드바 「기획」 트리 → 문서 → 같은 commit의 그림·영상(Range) → 문서 안 링크로 다른 문서 → 주소로 다시 열기, 기획 폴더 없는 저장소 줄.
// 작업 파일은 옛 판이고 origin만 앞서 있다: 화면은 origin 기본 branch의 글이어야 한다. 고정 sleep 없이 DOM·응답을 기다린다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import zlib from "node:zlib";
import { aaa, root, step, browser, webkit, pairPage, inList, domIs, drawerSettled, shot, hashIs } from "../lib/harness.mjs";
import { reachEnd } from "../lib/panel-scroll.mjs";
import { installPanelScrollTrace } from "../lib/panel-scroll-diagnostics.mjs";

/** 단색 PNG(w×h) */
function png(w, h, [r, g, b]) {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const x of buf) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0); body.copy(out, 4); out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0); head.writeUInt32BE(h, 4); head[8] = 8; head[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => [r, g, b]).flat())]);
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head),
    chunk("IDAT", zlib.deflateSync(Buffer.concat(Array(h).fill(row)))), chunk("IEND", Buffer.alloc(0))]);
}

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

function write(dir, files) {
  for (const [rel, body] of Object.entries(files)) {
    fs.mkdirSync(`${dir}/${rel}`.replace(/\/[^/]+$/, ""), { recursive: true });
    fs.writeFileSync(`${dir}/${rel}`, body);
  }
}

/** origin(bare)과 그 clone. clone의 작업 파일은 첫 판이고 origin은 `later`까지 앞서 있다 */
function repo(name, first, later) {
  const origin = `${root}/origins/${name}.git`, seed = `${root}/seed/${name}`, clone = `${root}/${name}`;
  fs.mkdirSync(seed, { recursive: true });
  git(root, "init", "--bare", "-q", origin);
  git(seed, "init", "-q");
  write(seed, first);
  git(seed, "add", ".");
  git(seed, "commit", "-q", "-m", "처음");
  git(seed, "remote", "add", "origin", origin);
  git(seed, "push", "-q", "origin", "HEAD:main");
  git(root, "clone", "-q", origin, clone);
  if (later) {
    write(seed, later);
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "기획 더함");
    git(seed, "push", "-q", "origin", "HEAD:main");
  }
  return { clone, seed, head: git(seed, "rev-parse", "HEAD") };
}

const LOOP = `# 게임 루프

마을에서 출발해 숲을 지나 성으로 간다. 자세한 것은 [기술 목록](skills.md).

![지도](media/map.png)

![흐름 영상](media/clip.mp4)

> [!계획]
> 성 안 보스를 더한다.
`;

const api = (page, body) => page.evaluate(async (body) => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return r.json();
}, body);

export default async function () {
  let app;
  // 압축되지 않는 약 0.9MiB 파일: host가 agent에서 512KiB 조각을 이어 받는 길과 조각 경계의 Range를 본다
  const noise = crypto.randomBytes(900 * 1024);
  const loopDoc = "app/docs/loop/README.md";

  await step("기획 저장소 준비: origin이 앞선 저장소 하나와 기획 폴더 없는 저장소", async () => {
    for (const d of ["origins", "seed"]) fs.mkdirSync(`${root}/${d}`, { recursive: true });
    app = repo("app", { "README.md": "# app\n", "docs/README.md": "# 개요\n\n작은 견본 게임.\n" }, {
      "docs/loop/README.md": LOOP,
      "docs/loop/skills.md": "# 기술 목록\n\n| 이름 | 효과 |\n| --- | --- |\n| 돌진 | 앞으로 세 칸 |\n",
      "docs/loop/media/map.png": png(240, 120, [60, 140, 90]),
      "docs/loop/media/clip.mp4": fs.readFileSync(new URL("../fixtures/report-video.mp4", import.meta.url)),
      "docs/loop/media/noise.bin": noise,
    });
    const tool = repo("tool", { "README.md": "# tool\n" });
    // origin을 받지 못하는 저장소: 가진 판으로 읽고 까닭을 싣는다
    const lore = repo("lore", { "notes/README.md": "# 설정 노트\n" });
    git(lore.clone, "remote", "set-url", "origin", `${root}/origins/gone.git`);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`,
      `schema = 1\n\n[[workspaces]]\npath = "${app.clone}"\nmachine = "labA"\ndesign = "docs"\n\n` +
      `[[workspaces]]\npath = "${lore.clone}"\nmachine = "labA"\ndesign = "notes"\n\n[[workspaces]]\npath = "${tool.clone}"\nmachine = "labA"\n`);
    const tree = aaa("plan", "list");
    assert.deepEqual(tree.repos.map((r) => [r.repo, r.folder]), [["app", "docs"], ["lore", "notes"], ["tool", null]]);
    assert.match(tree.repos[1].tree.fetch_error ?? "", /origin을 받지 못했다/);
    assert.deepEqual(tree.repos[1].tree.docs.map((d) => d.path), ["notes/README.md"], "가진 판의 문서");
    const t = tree.repos[0].tree;
    assert.equal(t.commit, app.head, "origin 기본 branch의 판");
    assert.equal(t.base, "origin/main");
    assert.deepEqual(t.docs.map((d) => [d.path, d.title]), [["docs/README.md", "개요"], ["docs/loop/README.md", "게임 루프"], ["docs/loop/skills.md", "기술 목록"]]);
    assert.ok(!fs.existsSync(`${app.clone}/docs/loop`), "작업 파일은 그대로");
  }, { always: true });

  for (const [name, width] of [["chromium", 1280], ["chromium", 390], ["webkit", 1280], ["webkit", 390]]) {
    await step(`트리·문서·그림·영상·문서 안 링크·주소 다시 열기: ${name} ${width}px`, async () => {
      const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
      const engine = wk ?? browser;
      try {
        const { page: p } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
        try {
          const media = [];
          p.on("response", (r) => { if (r.url().includes("/design/app/")) media.push([new URL(r.url()).pathname.split("/").pop(), r.status()]); });
          await inList(p, "activity-design");
          const tool = p.locator('[data-testid="design-repo"][data-repo="tool"]');
          await tool.waitFor({ state: "attached" });
          assert.match(await tool.textContent(), /기획 폴더 없음/);
          // origin을 받지 못한 저장소 줄: 경고와 가진 판의 문서
          const lore = p.locator('[data-testid="design-repo"][data-repo="lore"]');
          await lore.getByTestId("design-repo-error").waitFor();
          assert.match(await lore.getByTestId("design-repo-error").locator("svg").getAttribute("aria-label"), /origin을 받지 못해 가진 판/);
          assert.equal(await lore.getByTestId("design-doc").count(), 1);
          await inList(p, p.locator('[data-testid="design-dir"][data-path="docs/loop"]'));
          await inList(p, p.locator(`[data-testid="design-doc"][data-path="docs/loop/README.md"]`));
          const body = p.getByTestId("design-body");
          await body.getByRole("heading", { name: "게임 루프" }).waitFor();
          assert.equal(await p.evaluate(() => location.hash), `#design=${encodeURIComponent(loopDoc)}`);
          assert.match(await p.getByTestId("design-meta").textContent(), new RegExp(app.head.slice(0, 7)));
          const img = body.locator("img").first();
          await domIs(p, "기획 그림을 받음", () => { const i = document.querySelector("[data-testid=design-body] img"); return !!i && i.complete && i.naturalWidth > 0; });
          assert.match(await img.getAttribute("src"), new RegExp(`/api/v1/projects/demo/design/app/docs/loop/media/map\\.png\\?c=${app.head}$`), "같은 commit의 그림");
          assert.equal(await img.evaluate((i) => i.naturalWidth), 240);
          // 영상: WebKit은 Range(206)로만 재생한다
          const video = body.locator('video[aria-label="흐름 영상"]');
          await body.getByRole("button", { name: "흐름 영상 재생", exact: true }).click();
          await video.evaluate((v) => new Promise((resolve, reject) => {
            const done = () => { if (v.currentTime <= 0) return; v.removeEventListener("timeupdate", done); v.pause(); resolve(); };
            if (v.currentTime > 0) return done();
            v.addEventListener("timeupdate", done);
            v.addEventListener("error", () => reject(new Error(`영상 오류 ${v.error?.code}`)), { once: true });
          }));
          if (name === "webkit") assert.ok(media.some(([f, s]) => f === "clip.mp4" && s === 206), JSON.stringify(media));
          await shot(p, `design-doc-${name}-${width}`);
          // 문서 안 상대 링크는 앱 안에서 그 문서를 연다
          await body.getByRole("button", { name: "기술 목록", exact: true }).click();
          await body.getByRole("heading", { name: "기술 목록" }).waitFor();
          assert.equal(await p.evaluate(() => location.hash), `#design=${encodeURIComponent("app/docs/loop/skills.md")}`);
          assert.match(await p.getByTestId("design-crumb").textContent(), /app \/ docs \/ loop \/ skills\.md/);
          // 주소로 다시 열면 같은 문서와 트리의 그 줄
          await p.reload();
          await p.getByTestId("design-body").getByRole("heading", { name: "기술 목록" }).waitFor();
          const row = p.locator('[data-testid="design-doc"][data-path="docs/loop/skills.md"]');
          await row.waitFor({ state: "attached" });
          assert.equal(await row.getAttribute("aria-current"), "true");
          if (width < 700) {
            await p.locator(".view [data-testid='drawer-toggle']:visible").click();
            await p.locator("#workspace-drawer.is-open").waitFor();
            await drawerSettled(p);
            assert.ok(await row.isVisible());
            await shot(p, `design-tree-${name}-${width}`);
          }
        } finally { await p.context().close(); }
      } finally { await wk?.close(); }
    });
  }

  await step("읽기 권한: 보기 기기도 읽고, 기획 폴더 밖·기기 없는 요청·없는 범위는 거부", async () => {
    const reader = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 })).page;
    try {
      const doc = await api(reader, { op: "design_read", project: "demo", repo: "app", path: "docs/loop/skills.md" });
      assert.equal(doc.ok?.doc?.title, "기술 목록", JSON.stringify(doc));
      const status = (url, init) => reader.evaluate(async ([url, init]) => {
        const r = await fetch(url, init);
        return [r.status, r.headers.get("content-range"), r.headers.get("content-security-policy") !== null];
      }, [url, init]);
      const at = `/api/v1/projects/demo/design/app`;
      const size = png(240, 120, [60, 140, 90]).length;
      assert.deepEqual((await status(`${at}/README.md?c=${app.head}`)).slice(0, 2), [400, null], "기획 폴더 밖");
      assert.deepEqual(await status(`${at}/docs/loop/media/map.png?c=${app.head}`, { headers: { range: "bytes=999999-" } }), [416, `bytes */${size}`, true]);
      assert.deepEqual((await status(`${at}/docs/loop/media/map.png?c=${app.head}`, { headers: { range: "bytes=0-7" } })).slice(0, 2), [206, `bytes 0-7/${size}`]);
      assert.equal((await status(`${at}/docs/loop/media/map.png?c=${app.head}`, { credentials: "omit" }))[0], 401, "기기 없는 요청");
      const bytes = (url, range) => reader.evaluate(async ([url, range]) => {
        const r = await fetch(url, range ? { headers: { range } } : {});
        const b = new Uint8Array(await r.arrayBuffer());
        const sha = [...new Uint8Array(await crypto.subtle.digest("SHA-256", b))].map((x) => x.toString(16).padStart(2, "0")).join("");
        return [r.status, b.length, sha, r.headers.get("content-range")];
      }, [url, range]);
      const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
      const big = `${at}/docs/loop/media/noise.bin?c=${app.head}`;
      assert.deepEqual(await bytes(big), [200, noise.length, sha(noise), null], "조각을 이어 받은 전체");
      assert.deepEqual(await bytes(big, "bytes=524280-524295"), [206, 16, sha(noise.subarray(524280, 524296)), `bytes 524280-524295/${noise.length}`], "512KiB 조각 경계를 넘는 범위");
      assert.deepEqual((await bytes(big, "bytes=-10")).slice(0, 2), [206, 10]);
      const outside = await api(reader, { op: "design_read", project: "demo", repo: "app", path: "README.md" });
      assert.equal(outside.err?.code, "bad_request", JSON.stringify(outside));
    } finally { await reader.context().close(); }
  });

  await step("문서 「새로 읽기」는 트리를 다시 읽어 origin의 새 판을 보인다", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: `#design=${encodeURIComponent("app/docs/loop/skills.md")}` });
    try {
      const body = p.getByTestId("design-body");
      await body.getByRole("heading", { name: "기술 목록", exact: true }).waitFor();
      assert.match(await p.getByTestId("design-meta").textContent(), new RegExp(app.head.slice(0, 7)));
      fs.writeFileSync(`${app.seed}/docs/loop/skills.md`, "# 기술 목록 둘\n\n| 이름 | 효과 |\n| --- | --- |\n| 돌진 | 앞으로 네 칸 |\n");
      git(app.seed, "commit", "-q", "-am", "기술 고침");
      git(app.seed, "push", "-q", "origin", "HEAD:main");
      const next = git(app.seed, "rev-parse", "HEAD");
      // agent의 1분 받기 간격과 별개로 clone이 새 판을 가진 상태
      git(app.clone, "fetch", "-q", "origin");
      await p.getByTestId("design-doc-reload").click();
      await body.getByRole("heading", { name: "기술 목록 둘", exact: true }).waitFor();
      assert.match(await p.getByTestId("design-meta").textContent(), new RegExp(next.slice(0, 7)));
      assert.match(await p.locator('[data-testid="design-doc"][data-path="docs/loop/skills.md"]').textContent(), /기술 목록 둘/, "트리도 새 판");
    } finally { await p.context().close(); }
  });

  // 긴 기획 트리(t-1934): 트리가 화면보다 길면 트리 칸만 굴러 끝 문서까지 닿는다(사이드바 칸이 높이를 받지 못해 잘리던 결함).
  // 넓은 창은 사이드바, 휴대폰은 drawer와 문서 화면의 「트리」 칸을 실제 휠로 굴린다
  const many = 60;
  await step("긴 기획 트리 준비: 문서 60개 폴더가 있는 저장소를 더한다", async () => {
    const docs = Object.fromEntries(Array.from({ length: many }, (_, i) => [`docs/monsters/m${String(i + 1).padStart(2, "0")}.md`, `# 몬스터 ${i + 1}\n`]));
    const big = repo("big", { "README.md": "# big\n", "docs/README.md": "# 큰 기획\n", ...docs });
    fs.appendFileSync(`${root}/projects/demo/aaa.toml`, `\n[[workspaces]]\npath = "${big.clone}"\nmachine = "labA"\ndesign = "docs"\n`);
    const tree = aaa("plan", "list");
    assert.deepEqual(tree.repos.map((r) => r.repo).at(-1), "big");
    assert.equal(tree.repos.at(-1).tree.docs.length, many + 1);
  }, { always: true });

  for (const [name, width] of [["chromium", 1280], ["chromium", 390], ["webkit", 1280], ["webkit", 390]]) {
    await step(`긴 기획 트리 ${name} ${width}px: 트리 칸이 끝 문서까지 굴러가고 연다`, async () => {
      const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
      const engine = wk ?? browser;
      const phone = width < 700;
      // 실제 휠로 굴린다. 휴대폰 흉내 WebKit은 mouse.wheel을 받지 않아 칸의 scrollTop을 직접 옮긴다(panel-scroll.mjs와 같다)
      const wheel = !(phone && name === "webkit");
      try {
        const { page: p } = await pairPage(engine, aaa("web", "pair").code, { width, height: phone ? 844 : 800 }, { init: installPanelScrollTrace });
        try {
          const panel = '#sidebar-column [data-testid="design-panel"]';
          await inList(p, "activity-design");
          await inList(p, p.locator(`${panel} [data-testid="design-dir"][data-path="docs/monsters"]`));
          await domIs(p, "폴더 펼침", (n) => document.querySelectorAll('#sidebar-column [data-testid="design-doc"][data-path^="docs/monsters/"]').length === n, many);
          await shot(p, `design-long-tree-${name}-${width}-top`);
          const last = await reachEnd(p, panel, '[data-testid="design-doc"]', wheel);
          assert.equal(await last.getAttribute("data-path"), `docs/monsters/m${many}.md`);
          await shot(p, `design-long-tree-${name}-${width}-end`);
          await last.click();
          await hashIs(p, `#design=${encodeURIComponent(`big/docs/monsters/m${many}.md`)}`);
          await p.getByTestId("design-body").getByRole("heading", { name: `몬스터 ${many}`, exact: true }).waitFor();
          if (phone) {
            // 휴대폰 문서 화면의 「트리」 칸: 같은 트리를 칸 안에서 굴린다(연 문서의 폴더는 펼쳐져 있다)
            await p.getByTestId("design-page-tree").click();
            const pane = '.design-tree-pane [data-testid="design-panel"]';
            await p.locator(`${pane} [data-testid="design-doc"][data-path="docs/monsters/m${many}.md"]`).waitFor({ state: "attached" });
            const end = await reachEnd(p, pane, '[data-testid="design-doc"]', wheel);
            assert.equal(await end.getAttribute("aria-current"), "true");
            // 끝 문서 아래 여백: 칸이 화면 바닥에 붙어 끝 줄이 잘려 보이지 않는다
            const gap = await p.evaluate(() => innerHeight - document.querySelector('.design-tree-pane [data-path="docs/monsters/m60.md"]').getBoundingClientRect().bottom);
            assert.ok(gap >= 4, `끝 문서 아래 여백 ${gap}px`);
            await shot(p, `design-long-tree-${name}-${width}-pane`);
          }
        } finally { await p.context().close(); }
      } finally { await wk?.close(); }
    });
  }
}
