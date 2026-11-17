// 기획 트리 저장소 필터(t-1946): 트리 위 저장소 칩으로 고른 저장소만 트리에 보이고, 고른 것은 이 브라우저(localStorage)에만 남아
// 새로 고쳐도 같다. 휴대폰 drawer·문서 화면 「트리」 칸도 같은 필터다. 고정 sleep 없이 DOM을 기다린다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { aaa, root, step, browser, pairPage, inList, drawerSettled, shot, hashIs } from "../lib/harness.mjs";

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

/** origin(bare)과 그 clone */
function repo(name, files) {
  const origin = `${root}/origins/${name}.git`, seed = `${root}/seed/${name}`, clone = `${root}/${name}`;
  fs.mkdirSync(seed, { recursive: true });
  git(root, "init", "--bare", "-q", origin);
  git(seed, "init", "-q");
  for (const [rel, body] of Object.entries(files)) {
    fs.mkdirSync(`${seed}/${rel}`.replace(/\/[^/]+$/, ""), { recursive: true });
    fs.writeFileSync(`${seed}/${rel}`, body);
  }
  git(seed, "add", ".");
  git(seed, "commit", "-q", "-m", "처음");
  git(seed, "remote", "add", "origin", origin);
  git(seed, "push", "-q", "origin", "HEAD:main");
  git(root, "clone", "-q", origin, clone);
  return clone;
}

const REPOS = [
  ["game", "game-design", { "game-design/README.md": "# 게임 기획\n", "game-design/combat/README.md": "# 전투\n", "game-design/world.md": "# 세계\n" }],
  ["engine", "docs", { "docs/README.md": "# 엔진\n", "docs/render.md": "# 그리기\n" }],
  ["tool", "docs", { "docs/README.md": "# 도구\n", "docs/web.md": "# 웹\n" }],
  ["puzzle", "docs/design", { "docs/design/README.md": "# 퍼즐 기획\n", "docs/design/levels.md": "# 단계\n" }],
];
const NAMES = REPOS.map(([n]) => n);

/** 트리에 보이는 저장소 줄 이름(그 칸 안) */
const shown = (scope) => scope.locator('[data-testid="design-repo"]').evaluateAll((els) => els.map((e) => e.getAttribute("data-repo")));
/** 이 브라우저에 남긴 필터 값(원문) */
const stored = (p) => p.evaluate(() => localStorage.getItem("herdr-web-ui:design-repos"));
/** 열린 체크 메뉴(데스크톱 popover·휴대폰 sheet)의 항목 */
const item = (p, n) => p.locator(".menu-item, .row-sheet-item", { hasText: new RegExp(`^${n}$`) });
/** 체크 메뉴 닫기: 휴대폰 sheet는 「닫기」, popover는 Esc. sheet는 미끄러져 나가는 마지막 장면(.is-leaving)까지 사라진 뒤다 */
async function closeMenu(p) {
  if (await p.locator(".row-sheet").count() > 0) await p.getByRole("button", { name: "닫기", exact: true }).click();
  else await p.keyboard.press("Escape");
  await p.locator(".row-menu").waitFor({ state: "detached" });
  await p.locator(".modal-scrim").waitFor({ state: "detached" });
}

export default async function () {
  await step("기획 저장소 넷 준비", async () => {
    for (const d of ["origins", "seed"]) fs.mkdirSync(`${root}/${d}`, { recursive: true });
    const spaces = REPOS.map(([name, design, files]) => `[[workspaces]]\npath = "${repo(name, files)}"\nmachine = "labA"\ndesign = "${design}"\n`);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`, `schema = 1\n\n${spaces.join("\n")}`);
    assert.deepEqual(aaa("plan", "list").repos.map((r) => r.repo), NAMES);
  }, { always: true });

  for (const width of [1280, 390]) {
    await step(`저장소 고르기 → 고른 것만 보이고 새로 고쳐도 같다: ${width}px`, async () => {
      const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width, height: 844 });
      try {
        await inList(p, "activity-design");
        await p.locator('[data-testid="design-repo"][data-repo="puzzle"]').waitFor({ state: "attached" });
        await drawerSettled(p);
        assert.deepEqual(await shown(p.getByTestId("design-panel")), NAMES);
        await shot(p, `design-filter-${width}-all`);
        assert.match(await p.getByTestId("design-repo-filter").textContent(), /저장소: 전체/);
        assert.equal(await stored(p), null, "처음엔 남긴 것이 없다");
        // 「저장소: 전체 ▾」 → 체크 메뉴(데스크톱 popover, 휴대폰 sheet)에서 둘을 연달아 고른다. 메뉴는 고를 때마다 열려 있다
        await p.getByTestId("design-repo-filter").click();
        for (const n of ["game", "puzzle"]) await item(p, n).click();
        await p.locator('[data-testid="design-repo"][data-repo="engine"]').waitFor({ state: "detached" });
        await shot(p, `design-filter-${width}-menu`);
        await closeMenu(p);
        await drawerSettled(p);
        // sheet 닫기(기록 한 칸 뒤로)가 그 밑 drawer까지 닫지 않는다
        if (width < 700) assert.equal(await p.locator("#workspace-drawer.is-open").count(), 1, "drawer가 열려 있다");
        assert.ok(await p.getByTestId("design-repo-filter").isVisible(), "필터 단추가 보인다");
        assert.equal(await stored(p), JSON.stringify(["game", "puzzle"]));
        assert.match(await p.getByTestId("design-repo-filter").textContent(), /저장소: game, puzzle/);
        await p.locator('[data-testid="design-repo"][data-repo="engine"]').waitFor({ state: "detached" });
        assert.deepEqual(await shown(p.getByTestId("design-panel")), ["game", "puzzle"]);
        await shot(p, `design-filter-${width}-picked`);
        await p.reload();
        // 데스크톱은 연 칸을 기억해 사이드바가 이미 「기획」이다(다시 누르면 닫힌다). 휴대폰은 drawer를 연다
        await p.locator('[data-testid="design-repo"][data-repo="puzzle"]').waitFor({ state: "attached" });
        if (width < 700) {
          await p.locator(".view [data-testid='drawer-toggle']:visible").click();
          await p.locator("#workspace-drawer.is-open").waitFor();
        }
        await drawerSettled(p);
        assert.deepEqual(await shown(p.getByTestId("design-panel")), ["game", "puzzle"], "새로 고쳐도 같은 필터");
        await shot(p, `design-filter-${width}-reloaded`);
      } finally { await p.context().close(); }
    });
  }

  await step("휴대폰 문서 화면 「트리」 칸도 같은 필터이고, 거기서 「전체」로 되돌리면 drawer 트리와 남긴 값도 바뀐다", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    try {
      await p.evaluate(() => localStorage.setItem("herdr-web-ui:design-repos", JSON.stringify(["puzzle"])));
      await p.reload();
      await inList(p, "activity-design");
      await p.locator('#sidebar-column [data-testid="design-repo"][data-repo="puzzle"]').waitFor({ state: "attached" });
      await drawerSettled(p);
      assert.deepEqual(await shown(p.locator('#sidebar-column [data-testid="design-panel"]')), ["puzzle"]);
      await inList(p, p.locator('#sidebar-column [data-testid="design-doc"][data-path="docs/design/levels.md"]'));
      await hashIs(p, `#design=${encodeURIComponent("puzzle/docs/design/levels.md")}`);
      await p.getByTestId("design-page-tree").click();
      const pane = p.locator('.design-tree-pane [data-testid="design-panel"]');
      await pane.locator('[data-testid="design-repo"][data-repo="puzzle"]').waitFor({ state: "attached" });
      assert.deepEqual(await shown(pane), ["puzzle"], "문서 화면 트리 칸도 고른 것만");
      assert.match(await pane.getByTestId("design-repo-filter").textContent(), /저장소: puzzle/);
      await pane.getByTestId("design-repo-filter").click();
      await item(p, "전체").click();
      await pane.locator('[data-testid="design-repo"][data-repo="game"]').waitFor({ state: "attached" });
      await closeMenu(p);
      assert.ok(await pane.getByTestId("design-repo-filter").isVisible(), "sheet를 닫아도 「트리」 칸에 남는다");
      assert.deepEqual(await shown(pane), NAMES);
      assert.deepEqual(await shown(p.locator('#sidebar-column [data-testid="design-panel"]')), NAMES, "drawer 트리도 같은 필터");
      assert.equal(await stored(p), null, "「전체」는 남긴 값을 지운다");
      await shot(p, "design-filter-390-pane-all");
    } finally { await p.context().close(); }
  });

  await step("남긴 값이 깨졌거나 없는 저장소뿐이면 전부 보이고, 있는 저장소만 골라 보인다", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      // 데스크톱은 연 칸을 기억해 새로 고쳐도 사이드바가 「기획」이다: 한 번만 연다
      await inList(p, "activity-design");
      for (const [raw, want] of [["{깨짐", NAMES], [JSON.stringify(["gone"]), NAMES], [JSON.stringify(["gone", "tool"]), ["tool"]]]) {
        await p.evaluate((raw) => localStorage.setItem("herdr-web-ui:design-repos", raw), raw);
        await p.reload();
        await p.locator(`[data-testid="design-repo"][data-repo="${want.at(-1)}"]`).waitFor({ state: "attached" });
        assert.deepEqual(await shown(p.getByTestId("design-panel")), want, raw);
        assert.equal(await stored(p), raw, "읽기만 하고 남긴 값은 고치지 않는다");
      }
    } finally { await p.context().close(); }
  });
}
