// 웹 머신 추가 M2(plan §8.375): 설정 「머신」 → 「SSH로 추가」. 실험실 sshd(비특권·loopback·비밀번호 끔)를 이 시험이 띄우고, host는
// host.toml `[host] ssh_config`의 실험실 ssh 설정(별칭 lab·lab-down, 실험실 키만)으로 접속한다. 접속 → host key 지문(ssh-keygen -lf와
// 같음) → 「믿음」 확인 창 → OS 감지 → 준비됨, 비밀번호 꼴 대상은 보내기 전에 막음, 닫힌 포트는 까닭, 보기 기기는 목록만, 휴대폰 390 폭,
// 지우기. 원격에는 쓰지 않는다. sshd가 없는 머신에서는 이 파일을 건너뛴다.
import fs from "node:fs";
import { execFileSync, spawn } from "node:child_process";
import net from "node:net";
import readline from "node:readline";
import { root, procs, aaa, step, browser, pairPage, openSettings, shot, inTime } from "../lib/harness.mjs";

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
    console.log("sshd가 없어 71-machine-add를 건너뛴다");
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
    await shot(page, "71-trust-desktop");
    await job.getByRole("button", { name: "이 지문을 믿음…" }).click();
    const ask = page.getByRole("alertdialog", { name: "lab의 host key를 믿을까요?" });
    await ask.getByText(fingerprint).waitFor();
    await ask.getByRole("button", { name: "믿음" }).click();
    await ask.waitFor({ state: "detached" });
    await job.and(page.locator('[data-phase="ready"]')).waitFor();
    const os = { darwin: "macOS", linux: "Linux" }[process.platform];
    await job.getByTestId("machine-setup-detected").filter({ hasText: `${os} · ` }).waitFor();
    await shot(page, "71-ready-desktop");
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
    if ((await p.getByTestId("machine-setup-target").count()) || (await p.getByRole("button", { name: /지우기|믿음/ }).count())) {
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
    await shot(ph, "71-machine-add-phone");
    await p.getByRole("button", { name: "lab-down 작업 지우기" }).click();
    await p.locator('li[data-target="lab-down"]').waitFor({ state: "detached" });
    await ph.context().close();
  });
}
