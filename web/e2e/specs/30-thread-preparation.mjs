// 실제 checkout의 smudge filter가 socket 해제 사건을 기다린다. 준비 중인 행을 desktop·phone에서 확인한다.
import fs from "node:fs";
import net from "node:net";
import { execFileSync } from "node:child_process";
import { root, aaa, step, browser, pairPage, inList, until, shot } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("느린 thread 준비: 배경 시작은 바로 답하고 desktop·phone에 준비 중, 해제 뒤 세션을 연다", async () => {
    const repo = `${root}/slow-repo`;
    fs.mkdirSync(repo);
    const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { stdio: "pipe" });
    git("init", "-q", "-b", "main");
    fs.writeFileSync(`${repo}/.gitattributes`, "slow.txt filter=gate\n");
    fs.writeFileSync(`${repo}/slow.txt`, "content\n");
    git("add", ".");
    git("commit", "-qm", "fixture");
    let socket;
    let ready;
    const entered = new Promise((resolve) => { ready = resolve; });
    const server = net.createServer((s) => {
      socket = s;
      s.once("data", (bytes) => {
        if (bytes.toString() !== "checkout") throw new Error(`Git 표지 ${bytes}`);
        ready();
      });
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const filter = `${root}/gate.py`;
    fs.writeFileSync(filter, `import socket, sys\ndata = sys.stdin.buffer.read()\nwith socket.create_connection(('127.0.0.1', ${server.address().port}), timeout=30) as s:\n s.sendall(b'checkout')\n if s.recv(1) != b'!': sys.exit(42)\nsys.stdout.buffer.write(data)\n`);
    git("config", "filter.gate.smudge", `python3 ${filter}`);
    git("config", "filter.gate.required", "true");
    let phone;
    try {
      aaa("project", "create", "demo");
      fs.writeFileSync(`${root}/prepare-task.md`, "기다리는 준비 시험\n");
      const thread = aaa("thread", "start", "demo", "--title", "느린 준비", "--machine", "labA", "--profile", "fake", "--repo", repo, "--task-file", `${root}/prepare-task.md`, "--background");
      if (thread.status !== "starting" || thread.pane !== null) throw new Error(`배경 시작 ${JSON.stringify(thread)}`);
      await entered;
      const check = async (p, name) => {
        await inList(p, `thread-demo-${thread.id}`);
        const row = p.getByTestId(`thread-demo-${thread.id}`);
        await until(`${name} 준비 중`, async () => (await row.textContent()).includes("준비 중"));
        await p.locator(name === "phone" ? ".thread-view .meta-chip" : ".thread-view .head-chip").filter({ hasText: "준비 중" }).waitFor();
        await shot(p, `thread-preparing-${name}`);
      };
      await check(page, "desktop");
      phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      try { await check(phone.page, "phone"); }
      catch (error) {
        await shot(phone.page, "thread-preparing-phone-failure");
        throw new Error(`${error.message} | phone: ${await phone.page.locator("body").innerText()}`);
      }
      socket.end("!");
      await until("준비 뒤 세션 시작", () => aaa("thread", "show", "demo", thread.id).status === "open");
      if (!aaa("thread", "show", "demo", thread.id).pane) throw new Error("준비 뒤 pane 없음");
    } finally {
      socket?.destroy();
      server.close();
      if (phone) await phone.page.context().close();
    }
  });
}
