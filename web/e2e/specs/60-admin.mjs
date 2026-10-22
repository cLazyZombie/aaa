// 웹 관리 W5b(plan §8.355): 설정 「머신」에서 등록 창 열기·닫기, 「직접 설치」 안내의 명령으로 새 머신 등록(M1, plan §8.383: 주소·이름·
// host 지문), 새 머신 승인(6자리 코드: 틀리면 말하고 맞으면 등록), host 머신은 폐기
// 단추 없음, 머신 상세(M0, plan §8.372: OS·판·실행 파일 SHA-256·기능, 보기 기기도 봄), 다른 머신 폐기. 「폰 및 기기」의 마지막 조작
// 기기(폐기 꺼짐)·이 기기 폐기 확인(「이 화면이 끊깁니다」), tailnet 공개
// 켜기(확인 창)·끄기(공개 주소로 연 화면은 「다른 경로를 확인했다」를 켜야 열림). 보고서 올리기(폴더·여러 파일), 공유 링크 키 교체
// (살아 있는 링크 수, 옛 링크 끊김), 보기 전용 기기, 휴대폰 390.
// host는 PATH 앞의 가짜 `tailscale`을 부른다(실제 tailscale을 건드리지 않는다): serve 상태는 실험실 파일 하나다. 새 머신은 같은 Mac에서
// 다른 설정·상태 폴더로 띄운 agent다(host agent_listen을 loopback에 연다).
import crypto from "node:crypto";
import fs from "node:fs";
import { spawn } from "node:child_process";
import readline from "node:readline";
import { root, AA, env, procs, aaa, step, browser, pairPage, domIs, shot, openSettings, inList, inTime, start, stop, until, base } from "../lib/harness.mjs";

const bin = `${root}/fakebin`;
fs.mkdirSync(bin, { recursive: true });
const serveState = `${bin}/ts-serve.json`;
const calls = `${bin}/ts-calls.log`;
fs.writeFileSync(serveState, "{}");
// tailscale CLI 흉내: status는 늘 Running, serve는 상태 파일을 쓰고 읽는다. 받은 인자는 한 줄씩 남긴다
fs.writeFileSync(
  `${bin}/tailscale`,
  `#!/bin/sh
echo "$*" >> "${calls}"
if [ "$1 $2" = "status --json" ]; then printf '{"BackendState":"Running","Self":{"DNSName":"mac-a.example.ts.net."}}'; exit 0; fi
if [ "$1 $2" = "serve status" ]; then cat "${serveState}"; exit 0; fi
if [ "$1 $2" = "serve --bg" ]; then
  port=\${4#--https=}
  printf '{"TCP":{"%s":{"HTTPS":true}},"Web":{"mac-a.example.ts.net:%s":{"Handlers":{"/":{"Proxy":"%s"}}}}}' "$port" "$port" "$5" > "${serveState}"
  echo "Available within your tailnet"; exit 0
fi
if [ "$3" = off ]; then printf '{}' > "${serveState}"; exit 0; fi
exit 1
`,
  { mode: 0o755 },
);
export const labEnv = { PATH: `${bin}:${process.env.PATH}` };
// 새 머신(labB)이 붙을 agent 연결: 실험실 host는 기본으로 닫아 둔다
const toml = `${root}/c/host.toml`;
fs.writeFileSync(toml, fs.readFileSync(toml, "utf8").replace('agent_listen = ""', 'agent_listen = "127.0.0.1:0"'));
// labB의 설정·상태(신원·host 고정)는 따로 둔다
const nodeB = { AAA_CONFIG_DIR: `${root}/b/c`, AAA_STATE_DIR: `${root}/b/s` };
fs.mkdirSync(`${root}/b/c`, { recursive: true });
fs.mkdirSync(`${root}/b/work`, { recursive: true });
fs.writeFileSync(`${root}/b/c/agent.toml`, `schema = 1\n[agent]\nwork_root = "${root}/b/work"\n`);
// 올릴 보고서: 폴더(맨 위 index.html, 하위 그림, 숨은 파일)와 낱개 파일
fs.mkdirSync(`${root}/up/out/img`, { recursive: true });
fs.writeFileSync(`${root}/up/out/index.html`, "<!doctype html><meta charset=utf-8><h1>계획 검토</h1>");
fs.writeFileSync(`${root}/up/out/img/a.png`, "png");
fs.writeFileSync(`${root}/up/out/.DS_Store`, "x");
fs.writeFileSync(`${root}/up/a.md`, "# a\n");
fs.writeFileSync(`${root}/up/b.md`, "# b\n");

const tab = async (dialog, name) => {
  await dialog.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name }).click();
  await dialog.getByRole("tabpanel", { name }).waitFor();
};
const status = (url) => fetch(new URL(url, base)).then((r) => r.status);

export default async function ({ page }) {
  let dialog;
  let mp;
  await step("새 머신 등록: 창 열기 → 대기 목록 → 틀린 코드는 말하고 맞는 코드로 승인 → 연결, host 머신은 폐기 단추 없음", async () => {
    aaa("project", "create", "demo");
    const addr = aaa("status").agent_listen;
    dialog = await openSettings(page, "머신");
    mp = dialog.getByTestId("settings-machines");
    await mp.getByTestId("machine-list").locator('[data-name="labA"]').waitFor();
    if (await mp.getByRole("button", { name: "labA 폐기" }).count()) throw new Error("host 머신에 폐기 단추");
    await mp.getByTestId("enroll-open-button").click();
    await mp.getByTestId("enroll-open").waitFor();
    // 「직접 설치」 안내(M1, plan §8.383): host가 연 주소(실험실은 loopback)와 이름을 넣은 명령을 그대로 새 머신에서 친다
    const guide = mp.getByTestId("enroll-guide");
    await guide.locator("summary").click();
    await guide.getByTestId("enroll-guide-address").locator(`option[value="${addr}"]`).waitFor({ state: "attached" });
    await guide.getByText("host 머신 안에서만 닿는다").waitFor();
    await guide.getByTestId("enroll-guide-name").fill("labA");
    await guide.getByText("이미 있는 머신 이름이다").waitFor();
    await guide.getByTestId("enroll-guide-name").fill("labB");
    const steps = guide.getByTestId("enroll-guide-steps");
    const enrollCmd = (await steps.locator("li").nth(1).locator("code").innerText()).trim();
    if (enrollCmd !== `~/.local/bin/aaa agent enroll ${addr} --name labB`) throw new Error(`안내 명령 ${enrollCmd}`);
    const fpShown = (await steps.locator("li").nth(2).innerText()).match(/fingerprint가 ([0-9a-f]{16})…/)?.[1];
    await shot(page, "60-enroll-guide-desktop");
    // 새 머신 화면: 확인 코드 줄을 읽는다(그 머신 사람이 보는 것). 안내 명령의 실행 파일 자리만 실험실 binary로 바꾼다
    const p = spawn(AA, enrollCmd.split(" ").slice(1), { env: { ...env, ...nodeB }, stdio: ["ignore", "pipe", "pipe"] });
    procs.set("enroll", p);
    const exited = new Promise((done) => p.once("exit", done));
    let fpSeen = null;
    const code = await inTime(
      new Promise((done, fail) => {
        readline.createInterface({ input: p.stdout }).on("line", (l) => {
          fpSeen = l.match(/host fingerprint ([0-9a-f]{16})…/)?.[1] ?? fpSeen;
          const m = l.match(/--code (\d{6})/);
          if (m) done(m[1]);
        });
        p.once("exit", (c) => fail(new Error(`enroll이 코드 전에 끝났다: ${c}`)));
      }),
      20_000,
      "새 머신 확인 코드",
    );
    const row = mp.getByTestId("enroll-pending").locator('[data-name="labB"]');
    await row.waitFor();
    await row.getByRole("button", { name: "승인…" }).click();
    const ap = page.getByRole("dialog", { name: "labB 승인" });
    await ap.getByText("「labB」인지").waitFor();
    if (!(await ap.getByTestId("enroll-approve").isDisabled())) throw new Error("코드 없이 승인 단추가 열렸다");
    await ap.getByTestId("enroll-code").fill(code === "000000" ? "111 111" : "000 000");
    await ap.getByTestId("enroll-approve").click();
    await ap.getByRole("alert").waitFor();
    await shot(page, "60-enroll-approve-desktop");
    await ap.getByTestId("enroll-code").fill(`${code.slice(0, 3)} ${code.slice(3)}`);
    await ap.getByTestId("enroll-approve").click();
    await ap.waitFor({ state: "detached" });
    const c = await inTime(exited, 20_000, "enroll 끝");
    if (c !== 0) throw new Error(`enroll 끝 코드 ${c}`);
    // 새 머신 화면의 host 지문이 안내가 보인 것과 같다(사람이 맞춰 보는 것)
    if (!fpShown || fpShown !== fpSeen) throw new Error(`host 지문: 안내 ${fpShown} · 새 머신 ${fpSeen}`);
    start("agentB", nodeB, "agent", "--host", addr, "--name", "labB");
    await until("labB 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labB" && m.connection === "connected"));
    // 머신 목록은 host state 사건을 따른다: 연결이 바로 보인다
    await mp.getByTestId("machine-list").locator('[data-name="labB"]').filter({ hasText: "연결됨" }).waitFor();
    await mp.getByRole("button", { name: "창 닫기" }).click();
    await mp.getByTestId("enroll-open-button").waitFor();
    await shot(page, "60-machines-desktop");
  });

  await step("머신 상세(M0): 줄에 OS·CPU·판, 「자세히」에 연결·판·실행 파일 SHA-256·agent 기능", async () => {
    const b = aaa("machine", "list").machines.find((m) => m.name === "labB");
    // 둘째 agent도 같은 실행 파일이다: host가 받은 SHA는 그 파일의 SHA다
    const sha = crypto.createHash("sha256").update(fs.readFileSync(AA)).digest("hex");
    if (b.sha256 !== sha || !b.software || !b.arch || b.software_differs) throw new Error(`host가 받은 labB 판 ${JSON.stringify(b)}`);
    const os = { darwin: "macOS", linux: "Linux", win32: "Windows" }[process.platform];
    const line = mp.getByTestId("machine-list").locator('[data-name="labB"] .machine-line');
    await line.filter({ hasText: `${os} · ` }).filter({ hasText: b.software }).waitFor();
    const more = mp.getByRole("button", { name: "labB 자세히" });
    await more.click();
    const d = mp.getByTestId("machine-detail-labB");
    await d.locator('[data-label="SHA-256"] dd').filter({ hasText: sha }).waitFor();
    await d.locator('[data-label="판"] dd').filter({ hasText: b.software }).waitFor();
    if ((await more.getAttribute("aria-expanded")) !== "true") throw new Error("자세히 단추가 펼침을 알리지 않는다");
    await d.locator('[data-label="연결"] dd').filter({ hasText: "연결됨" }).waitFor();
    await d.getByText(/^agent 기능 \d+개$/).click();
    await d.getByText("work_root.manage.v1").waitFor();
    await shot(page, "60-machine-detail-desktop");
    await more.click();
    await d.waitFor({ state: "detached" });
  });

  await step("머신 폐기: 확인 창 → 목록과 host에서 빠진다", async () => {
    await mp.getByRole("button", { name: "labB 폐기" }).click();
    const confirm = page.getByRole("alertdialog", { name: "labB의 등록을 폐기할까요?" });
    await confirm.getByRole("button", { name: "폐기" }).click();
    await confirm.waitFor({ state: "detached" });
    await mp.getByTestId("machine-list").locator('[data-name="labB"]').waitFor({ state: "detached" });
    if (aaa("machine", "list").machines.some((m) => m.name === "labB")) throw new Error("host에 labB가 남았다");
    await stop("agentB");
  });

  await step("마지막 조작 기기: 폐기 단추가 꺼지고 까닭을 적는다", async () => {
    await tab(dialog, "폰 및 기기");
    await dialog.getByTestId("last-controller").waitFor();
    const mine = dialog.locator(".devices-row", { has: page.locator(".devices-current") });
    if (!(await mine.getByRole("button", { name: "폐기" }).isDisabled())) throw new Error("마지막 조작 기기의 폐기 단추가 열렸다");
  });

  await step("tailnet 공개: 켜기는 확인 창 하나, 이 주소(loopback)에서 끄면 다른 경로만 보이고 바로 끈다", async () => {
    const pc = dialog.getByTestId("publish-controls");
    await pc.getByRole("button", { name: "tailnet에 공개" }).click();
    const on = page.getByRole("alertdialog", { name: "tailnet에 공개할까요?" });
    await on.getByRole("button", { name: "공개" }).click();
    await on.waitFor({ state: "detached" });
    await pc.getByRole("button", { name: "공개 끄기" }).waitFor();
    const st = aaa("web", "status");
    if (st.url !== "https://mac-a.example.ts.net" || st.owned_port !== 443) throw new Error(`공개 상태 ${JSON.stringify(st)}`);
    await pc.getByRole("button", { name: "공개 끄기" }).click();
    const off = page.getByRole("dialog", { name: "공개를 끌까요?" });
    await off.getByText("host Mac에서 http://127.0.0.1:").waitFor();
    if ((await off.getByTestId("publish-off-here").count()) || (await off.getByTestId("publish-off-checked").count())) throw new Error("loopback 화면에 「끊길 수 있음」");
    await off.getByTestId("publish-off").click();
    await off.waitFor({ state: "detached" });
    await pc.getByRole("button", { name: "tailnet에 공개" }).waitFor();
    if (aaa("web", "status").url !== null || !fs.readFileSync(calls, "utf8").includes("serve --https=443 off")) throw new Error("공개를 끄지 않았다");
  });

  await step("공개 주소로 연 화면에서 끄기: 「이 화면이 끊길 수 있음」, 다른 경로를 확인해야 끄기가 열린다", async () => {
    aaa("web", "publish");
    // 이 화면이 공개 주소라고 host 답을 바꾼다(실험실은 TLS 공개 주소를 열 수 없다). 끄기 요청은 host가 그대로 받는다
    const here = async (route) => {
      const req = route.request();
      if (req.method() !== "POST" || JSON.parse(req.postData() ?? "{}").op !== "web_status") return route.continue();
      const res = await route.fetch();
      const body = await res.json();
      body.ok.url = new URL(base).origin;
      return route.fulfill({ response: res, json: body });
    };
    const { page: hp } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { route: ["**/api/v1/call", here] });
    const d = await openSettings(hp, "폰 및 기기");
    await d.getByTestId("publish-controls").getByRole("button", { name: "공개 끄기" }).click();
    const off = hp.getByRole("dialog", { name: "공개를 끌까요?" });
    await off.getByTestId("publish-off-here").filter({ hasText: "이 화면이 끊길 수 있음" }).waitFor();
    if (!(await off.getByTestId("publish-off").isDisabled())) throw new Error("확인 전에 끄기가 열렸다");
    await off.getByTestId("publish-off-checked").check();
    await shot(hp, "60-publish-off-here-desktop");
    await off.getByTestId("publish-off").click();
    await off.waitFor({ state: "detached" });
    if (aaa("web", "status").url !== null) throw new Error("공개 주소 화면에서 끄지 않았다");
    await hp.context().close();
  });

  await step("보고서 올리기: 폴더는 맨 위 폴더를 빼고 숨은 파일은 빼서 사용자 보고서로, 여러 파일은 작업 보고로", async () => {
    await page.keyboard.press("Escape");
    await dialog.waitFor({ state: "detached" });
    await inList(page, "activity-reports");
    const panel = page.getByTestId("reports-panel");
    await domIs(page, "보고서 목록 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
    await panel.getByTestId("reports-upload").click();
    let up = page.getByRole("dialog", { name: "보고서 올리기" });
    await up.getByTestId("upload-folder").setInputFiles(`${root}/up/out`);
    await up.getByTestId("upload-plan").getByText("img/a.png").waitFor();
    const planned = await up.getByTestId("upload-plan").innerText();
    if (!planned.includes("index.html") || planned.includes("DS_Store") || planned.includes("out/")) throw new Error(`올릴 목록: ${planned}`);
    await up.getByText("숨은 파일 1개는 뺐다").waitFor();
    await up.getByTestId("upload-name").fill("plan-review");
    await up.getByTestId("upload-to-user").check();
    await up.getByTestId("upload-title").fill("계획 검토");
    await shot(page, "60-upload-desktop");
    await up.getByTestId("upload-submit").click();
    await up.waitFor({ state: "detached" });
    await panel.getByRole("link", { name: "계획 검토" }).waitFor();
    const r = aaa("library", "list", "--project", "demo").reports.find((x) => x.name === "plan-review");
    if (!r?.to_user || r.title !== "계획 검토") throw new Error(`올린 보고서 ${JSON.stringify(r)}`);
    if (fs.existsSync(`${root}/s/library-uploads`) && fs.readdirSync(`${root}/s/library-uploads`).length) throw new Error("올리기 폴더가 남았다");
    // 여러 파일: index.html이 없으면 사용자 보고서로 보내지 못한다고 말한다
    await panel.getByTestId("reports-upload").click();
    up = page.getByRole("dialog", { name: "보고서 올리기" });
    await up.getByTestId("upload-files").setInputFiles([`${root}/up/a.md`, `${root}/up/b.md`]);
    await up.getByTestId("upload-name").fill("notes");
    await up.getByTestId("upload-to-user").check();
    await up.getByRole("alert").filter({ hasText: "index.html" }).waitFor();
    if (!(await up.getByTestId("upload-submit").isDisabled())) throw new Error("index.html 없는 사용자 보고서를 올리려 한다");
    await up.getByTestId("upload-to-user").uncheck();
    await up.getByTestId("upload-submit").click();
    await up.waitFor({ state: "detached" });
    await panel.getByTestId("reports-tab-work").click();
    await panel.getByText("demo/notes").waitFor();
  });

  await step("공유 링크 키 교체: 확인 창에 살아 있는 링크 수와 「모두 끊긴다」, 바꾸면 옛 링크가 열리지 않는다", async () => {
    const shared = aaa("library", "link", "demo", "plan-review");
    if ((await status(shared.base)) !== 200) throw new Error("공유 링크가 처음부터 열리지 않는다");
    dialog = await openSettings(page, "이 aaa");
    await dialog.getByTestId("key-rotate-open").click();
    const ask = page.getByRole("alertdialog", { name: "공유 링크 키를 바꿀까요?" });
    // 사용자 보고서는 올릴 때 링크를 자동 발급한다(밖 알림용): host 목록의 살아 있는 링크 수와 같아야 한다
    const live = aaa("library", "list", "--project", "demo").reports.filter((x) => x.link && !x.link.revoked).length;
    if (live < 1) throw new Error("살아 있는 링크가 없다");
    await ask.getByText(`살아 있는 공유 링크 ${live}개가 모두 끊긴다`).waitFor();
    await ask.getByRole("button", { name: "키 교체" }).click();
    await ask.waitFor({ state: "detached" });
    await dialog.getByTestId("key-rotate").getByRole("status").waitFor();
    if ((await status(shared.base)) === 200) throw new Error("키를 바꿨는데 옛 공유 링크가 열린다");
  });

  await step("보기 전용 기기: 머신 목록만, 등록·폐기·공개·올리기·키 교체 없음", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    const d = await openSettings(rd, "머신");
    const p = d.getByTestId("settings-machines");
    await p.getByTestId("machine-list").locator('[data-name="labA"]').waitFor();
    if ((await p.getByTestId("enroll-open-button").count()) || (await p.getByRole("button", { name: /폐기/ }).count())) throw new Error("보기 기기에 등록·폐기");
    if (await p.getByTestId("enroll-guide").count()) throw new Error("보기 기기에 직접 설치 안내(host 주소·지문)");
    // 상세는 읽기만이라 보기 기기도 본다
    await p.getByRole("button", { name: "labA 자세히" }).click();
    await p.getByTestId("machine-detail-labA").locator('[data-label="연결"] dd').filter({ hasText: "host 머신" }).waitFor();
    await tab(d, "폰 및 기기");
    await d.locator(".devices-panel").waitFor();
    if (await d.getByTestId("publish-controls").count()) throw new Error("보기 기기에 공개 단추");
    await tab(d, "이 aaa");
    await d.getByTestId("settings-this-aaa").waitFor();
    if (await d.getByTestId("key-rotate").count()) throw new Error("보기 기기에 키 교체");
    await rd.keyboard.press("Escape");
    await inList(rd, "activity-reports");
    await domIs(rd, "보고서 목록 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
    if (await rd.getByTestId("reports-upload").count()) throw new Error("보기 기기에 올리기");
    await rd.context().close();
  });

  await step("휴대폰 390: 머신 페이지가 폭 안에, 이 기기 폐기는 「이 화면이 끊깁니다」 확인 뒤 pairing 화면", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    let d = await openSettings(ph, "머신");
    await d.getByTestId("machine-list").locator('[data-name="labA"]').waitFor();
    // 상세·직접 설치 안내를 펼친 채로 잰다: SHA-256 64자와 긴 명령은 폭 안에서 줄을 바꾼다
    await d.getByTestId("enroll-guide").locator("summary").click();
    await d.getByTestId("enroll-guide-steps").locator("code").first().waitFor();
    await d.getByRole("button", { name: "labA 자세히" }).click();
    await d.getByTestId("machine-detail-labA").locator('[data-label="SHA-256"] dd').waitFor();
    const fits = await ph.evaluate(() => {
      const el = document.querySelector('[data-testid="settings-machines"]');
      const box = el.getBoundingClientRect();
      return box.left >= 0 && box.right <= innerWidth + 0.5 && el.scrollWidth <= el.clientWidth + 1 ? true : JSON.stringify({ l: box.left, r: box.right, sw: el.scrollWidth, cw: el.clientWidth });
    });
    if (fits !== true) throw new Error(`휴대폰 폭: ${fits}`);
    await shot(ph, "60-machines-phone");
    // 좁은 설정 창은 목록과 쪽이 번갈아 보인다: 다시 열어 고른다
    await ph.keyboard.press("Escape");
    d = await openSettings(ph, "폰 및 기기");
    const mine = d.locator(".devices-row", { has: ph.locator(".devices-current") });
    await mine.getByRole("button", { name: "폐기" }).click();
    const ask = ph.getByRole("alertdialog", { name: "이 기기를 폐기할까요?" });
    await ask.getByText("이 화면이 끊깁니다").waitFor();
    await shot(ph, "60-revoke-self-phone");
    await ask.getByRole("button", { name: "이 기기 폐기" }).click();
    await ph.getByLabel("페어링 코드").waitFor();
    await ph.context().close();
  });
}
