// 웹 머신 추가 M2(plan §8.384): 설정 「머신」 → 「SSH로 추가」. 실험실 sshd(비특권·loopback·비밀번호 끔)를 이 시험이 띄우고, host는
// host.toml `[host] ssh_config`의 실험실 ssh 설정(별칭 lab·lab-down, 실험실 키만)으로 접속한다. 접속 → host key 지문(ssh-keygen -lf와
// 같음) → 「믿음」 확인 창 → OS 감지 → 준비됨, 비밀번호 꼴 대상은 보내기 전에 막음, 닫힌 포트는 까닭, 보기 기기는 목록만, 휴대폰 390 폭,
// 지우기. M3(plan §8.420)는 화면의 설정 전송·SSH 거부 표시와 실제 install의 비특권 SSH 거부를 각각 검사한다.
// 큰 실행 파일의 전송·SHA/판 대조·실패 뒤 정리는 host의 실험실 SSH 시험이 맡는다. 실제 설치·등록 끝까지는
// host 시험(대본 원격)과 CI의 root sshd가 본다. sshd가 없는 머신에서는 이 파일을 건너뛴다.
import fs from "node:fs";
import { createHash } from "node:crypto";
import { machinePassword } from "../lib/machine-password.mjs";
import { execFile, execFileSync, spawn } from "node:child_process";
import net from "node:net";
import readline from "node:readline";
import { root, procs, aaa, AA, step, browser, pairPage, openSettings, shot, inTime } from "../lib/harness.mjs";

const lab = `${root}/sshlab`;
const sshd = ["/usr/sbin/sshd", "/usr/bin/sshd"].find((p) => fs.existsSync(p));
const freePort = () =>
  new Promise((done) => {
    const s = net.createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => done(port));
    });
  });
const [port, closed] = sshd ? [await freePort(), await freePort()] : [0, 0];
if (sshd) {
  fs.mkdirSync(lab, { recursive: true });
  for (const k of ["hostkey", "userkey"]) execFileSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", `${lab}/${k}`]);
  fs.copyFileSync(`${lab}/userkey.pub`, `${lab}/authorized_keys`);
  fs.writeFileSync(
    `${lab}/sshd_config`,
    `ListenAddress 127.0.0.1:${port}\nHostKey ${lab}/hostkey\nPidFile ${lab}/sshd.pid\nAuthorizedKeysFile ${lab}/authorized_keys\n` +
      "PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n",
  );
  fs.writeFileSync(
    `${lab}/ssh_config`,
    `Host lab\n  HostName 127.0.0.1\n  Port ${port}\n  IdentityFile ${lab}/userkey\n  IdentitiesOnly yes\n` +
      `Host lab-down\n  HostName 127.0.0.1\n  Port ${closed}\n  IdentityFile ${lab}/userkey\n  IdentitiesOnly yes\n`,
  );
  // 실험실 host가 이 ssh 설정을 쓰게 한다(host가 뜨기 전)
  const toml = `${root}/c/host.toml`;
  fs.writeFileSync(toml, fs.readFileSync(toml, "utf8").replace('agent_listen = ""', `agent_listen = ""\nssh_config = "${lab}/ssh_config"`));
}

export default async function ({ page }) {
  if (!sshd) {
    console.log("sshd가 없어 72-machine-add를 건너뛴다");
    return;
  }
  // ssh-keygen -lf가 보이는 host key 지문(사람이 그 머신에서 맞춰 보는 값)
  const fingerprint = execFileSync("ssh-keygen", ["-lf", `${lab}/hostkey.pub`], { encoding: "utf8" }).split(" ")[1];
  await step("실험실 sshd 시작", async () => {
    const p = spawn(sshd, ["-f", `${lab}/sshd_config`, "-D", "-e"], { stdio: ["ignore", "ignore", "pipe"] });
    procs.set("sshd", p);
    await inTime(
      new Promise((done, fail) => {
        readline.createInterface({ input: p.stderr }).on("line", (l) => l.includes("Server listening") && done());
        p.once("exit", (c) => fail(new Error(`sshd가 끝났다: ${c}`)));
      }),
      20_000,
      "sshd listen",
    );
  });

  let mp;
  await step("SSH로 추가: 접속 → 지문(ssh-keygen과 같음) → 믿음 확인 창 → 감지 → 준비됨", async () => {
    aaa("project", "create", "demo");
    const dialog = await openSettings(page, "머신");
    mp = dialog.getByTestId("machine-setup");
    // 비밀번호 꼴은 보내기 전에 막는다
    await mp.getByTestId("machine-setup-target").fill("me:pw@lab");
    await mp.getByText("비밀번호는 넣지 않는다").waitFor();
    if (!(await mp.getByTestId("machine-setup-start").isDisabled())) throw new Error("비밀번호 꼴 대상에 접속 단추가 열렸다");
    await mp.getByTestId("machine-setup-target").fill("lab");
    await mp.getByTestId("machine-setup-start").click();
    const job = mp.locator('li[data-target="lab"]');
    await job.and(page.locator('[data-phase="trust"]')).waitFor();
    const shown = (await job.getByTestId("machine-setup-keys").locator("code").innerText()).trim();
    if (shown !== fingerprint) throw new Error(`지문: 화면 ${shown} · ssh-keygen ${fingerprint}`);
    await job.getByText(`@127.0.0.1:${port}`).waitFor();
    await shot(page, "72-trust-desktop");
    await job.getByRole("button", { name: "이 지문을 믿음…" }).click();
    const ask = page.getByRole("alertdialog", { name: "lab의 host key를 믿을까요?" });
    await ask.getByText(fingerprint).waitFor();
    await ask.getByRole("button", { name: "믿음" }).click();
    await ask.waitFor({ state: "detached" });
    await job.and(page.locator('[data-phase="ready"]')).waitFor();
    const os = { darwin: "macOS", linux: "Linux" }[process.platform];
    await job.getByTestId("machine-setup-detected").filter({ hasText: `${os} · ` }).waitFor();
    await shot(page, "72-ready-desktop");
    const known = fs.readFileSync(`${root}/s/host/ssh/known_hosts`, "utf8").trim().split("\n");
    if (known.length !== 1 || !known[0].startsWith(`[127.0.0.1]:${port} `)) throw new Error(`aaa known_hosts: ${known}`);
  });

  await step("닫힌 포트: 실패와 까닭", async () => {
    await mp.getByTestId("machine-setup-target").fill("lab-down");
    await mp.getByTestId("machine-setup-start").click();
    const down = mp.locator('li[data-target="lab-down"]');
    await down.and(page.locator('[data-phase="failed"]')).waitFor();
    await down.getByText("연결이 거부됐다").waitFor();
  });

  await step("보기 기기: 목록과 단계만, 시작·믿음·지우기 없음", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    const d = await openSettings(rd, "머신");
    const p = d.getByTestId("machine-setup");
    await p.locator('li[data-target="lab"][data-phase="ready"]').waitFor();
    if (
      (await p.getByTestId("machine-setup-target").count()) ||
      (await p.getByTestId("machine-setup-settings").count()) ||
      (await p.getByRole("button", { name: /지우기|믿음/ }).count())
    ) {
      throw new Error("보기 기기에 SSH 추가 조작");
    }
    await rd.context().close();
  });

  await step("휴대폰 390: 폭 안에, 지우기", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const d = await openSettings(ph, "머신");
    const p = d.getByTestId("machine-setup");
    await p.locator('li[data-target="lab"]').waitFor();
    await p.scrollIntoViewIfNeeded();
    const fits = await ph.evaluate(() => {
      const el = document.querySelector('[data-testid="settings-machines"]');
      const box = el.getBoundingClientRect();
      return box.left >= 0 && box.right <= innerWidth + 0.5 && el.scrollWidth <= el.clientWidth + 1 ? true : JSON.stringify({ l: box.left, r: box.right, sw: el.scrollWidth, cw: el.clientWidth });
    });
    if (fits !== true) throw new Error(`휴대폰 폭: ${fits}`);
    await shot(ph, "72-machine-add-phone");
    await p.getByRole("button", { name: "lab-down 작업 지우기" }).click();
    await p.locator('li[data-target="lab-down"]').waitFor({ state: "detached" });
    await ph.context().close();
  });

  await step("실제 install 계획: 비특권 sshd를 거부하고 상태를 쓰지 않는다", async () => {
    const state = `${lab}/executor-state`;
    const sha = createHash("sha256").update(fs.readFileSync(AA)).digest("hex");
    // loopback sshd가 있는 같은 머신의 실제 binary를 직접 실행한다. 원격 파일 복사는 host keeper가 본다.
    const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
    const command = `${quote(AA)} --state-dir ${quote(state)} install-probe --executor-sha256 ${sha}`;
    const result = await new Promise((resolve) => {
      const child = execFile("ssh", ["-F", `${lab}/ssh_config`, "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", `UserKnownHostsFile=${root}/s/host/ssh/known_hosts`, "lab", command],
        { encoding: "utf8" }, (error, stdout, stderr) => resolve({ code: error?.code ?? 0, stdout, stderr }));
      procs.set("install-probe-ssh", child);
      child.stdin.end();
    });
    if (result.code !== 3 || result.stdout || result.stderr) throw new Error(`비특권 SSH 거부: ${JSON.stringify(result)}`);
    if (fs.existsSync(state)) throw new Error("SSH 검증 전에 상태를 썼다");
  });

  await step("설치 설정 → 계획 보기: 설정 전송과 SSH 거부 까닭 표시", async () => {
    const job = mp.locator('li[data-target="lab"]');
    const form = job.getByTestId("machine-setup-settings");
    await form.waitFor();
    if ((await form.getByTestId("machine-setup-name").inputValue()) !== "lab") throw new Error("기본 이름은 대상 별칭");
    const home = (await job.getByTestId("machine-setup-detected").innerText()).split(" · ")[2];
    if ((await form.getByTestId("machine-setup-work-root").inputValue()) !== `${home}/aaa-work`) throw new Error("기본 작업 root");
    await form.getByTestId("machine-setup-name").fill("lab b");
    await form.getByText("머신 이름은 영문").waitFor();
    await form.getByTestId("machine-setup-name").fill("lab-b");
    await form.getByTestId("machine-setup-host").fill("127.0.0.1:7430");
    await form.getByTestId("machine-setup-plan-start").scrollIntoViewIfNeeded();
    await shot(page, "72-settings-desktop");
    // 이 단계의 계약은 폼과 응답 표시다. 전송·정리의 실물 검사는 host keeper,
    // 실제 SSH 출처 거부는 바로 앞 단계가 지킨다. 브라우저 30초 대기에 binary 전송을 섞지 않는다.
    const current = await page.evaluate(async () => (await fetch("/api/v1/call", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "machine_setup_list" }),
    })).json());
    const source = current.ok.setups.find((s) => s.target === "lab");
    if (!source) throw new Error("준비된 실제 SSH 작업이 없다");
    let failed;
    let submitted = 0;
    const intercept = async (route) => {
      const req = route.request().postDataJSON();
      if (req.op === "machine_setup_plan") {
        submitted++;
        failed = { ...source, phase: "failed", settings: req.settings, error: "원격 install 계획: 그 머신의 install이 이 접속을 시스템 SSH 서버로 확인하지 못했다", updated_ms: source.updated_ms + 1 };
        await route.fulfill({ json: { ok: { kind: "machine_setup", setup: failed } } });
      } else if (req.op === "machine_setup_list" && failed) {
        await route.fulfill({ json: { ok: { kind: "machine_setups", setups: [failed] } } });
      } else await route.continue();
    };
    await page.route("**/api/v1/call", intercept);
    try {
      const request = page.waitForRequest((r) => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "machine_setup_plan");
      await form.getByTestId("machine-setup-plan-start").click();
      const req = (await request).postDataJSON();
      if (req.job !== source.job || req.settings.name !== "lab-b" || req.settings.host !== "127.0.0.1:7430" || req.settings.work_root !== `${home}/aaa-work`) throw new Error(`설치 설정 전송이 다르다: ${JSON.stringify(req)}`);
      await job.and(page.locator('[data-phase="failed"]')).waitFor();
      const why = job.getByText("시스템 SSH 서버로 확인하지 못했다", { exact: false });
      // 실패 단계에 이르렀으므로 까닭은 이미 그려져 있다: 다르면 작업 줄을 그대로 적는다
      if (!(await why.isVisible())) throw new Error(`까닭이 다르다: ${await job.innerText()}`);
      await why.scrollIntoViewIfNeeded();
      await shot(page, "72-install-refused-desktop");
      if (submitted !== 1) throw new Error(`계획을 ${submitted}번 보냈다`);
    } finally { await page.unroute("**/api/v1/call", intercept); }
  });
  await step("Windows 전용 가림 입력 Chromium 1280: 취소·일회 전달·거부·페이지 종료·기록 누출", () => machinePassword("chromium", 1280));
  await step("Windows 전용 가림 입력 Chromium 390: 취소·일회 전달·거부·페이지 종료·기록 누출", () => machinePassword("chromium", 390));
  await step("Windows 전용 가림 입력 WebKit 1280: 취소·일회 전달·거부·페이지 종료·기록 누출", () => machinePassword("webkit", 1280));
  await step("Windows 전용 가림 입력 WebKit 390: 취소·일회 전달·거부·페이지 종료·기록 누출", () => machinePassword("webkit", 390));
}
