import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { terminateSessionHosts, terminateDeployGuards } from "../e2e/lib/session-cleanup.mjs";

function alive(pid) {
  const out = execFileSync("ps", ["-axo", "pid=,stat="], { encoding: "utf8" });
  return out.split("\n").some((line) => {
    const m = /^\s*(\d+)\s+(\S+)/.exec(line);
    return m && Number(m[1]) === pid && !m[2].startsWith("Z");
  });
}

test("session host뿐 아니라 분리된 writer 자손의 종료 뒤 폴더를 지운다", { skip: process.platform === "win32", timeout: 30_000 }, async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-cleanup-"));
  const sessions = path.join(root, "sessions");
  const dir = path.join(sessions, "owned");
  fs.mkdirSync(dir, { recursive: true });
  const children = [];
  const pids = [];
  t.after(async () => {
    for (const pid of pids) {
      try { process.kill(pid, "SIGKILL"); }
      catch (e) { if (e.code !== "ESRCH") throw e; }
    }
    for (const child of children) if (child.exitCode === null && child.signalCode === null) await once(child, "exit");
    const deadline = Date.now() + 5000;
    while (pids.some(alive)) {
      assert.ok(Date.now() < deadline, "회귀 시험이 띄운 자손 종료");
      await delay(10);
    }
    fs.rmSync(root, { recursive: true, force: true });
  });
  // writer는 별도 process group이다. session host PID/group만 죽이면 계속 mkdir/write를 한다.
  const writer = `
    const fs = require('node:fs');
    const path = require('node:path');
    const dir = path.join(process.argv[1], 'work');
    function write() {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'report'), 'fake');
      setImmediate(write);
    }
    write();
    process.send(process.pid);
  `;
  const parent = spawn(process.execPath, ["-e", `
    const { spawn } = require('node:child_process');
    const child = spawn(process.execPath, ['-e', ${JSON.stringify(writer)}, process.argv[1]], {
      detached: true, stdio: ['ignore', 'ignore', 'inherit', 'ipc'],
    });
    child.once('message', pid => console.log(pid));
  `, dir], { stdio: ["ignore", "pipe", "inherit"] });
  children.push(parent);
  pids.push(parent.pid);
  const [chunk] = await once(parent.stdout, "data");
  const writerPid = Number(chunk.toString().trim());
  assert.ok(Number.isInteger(writerPid) && writerPid > 0);
  pids.push(writerPid);
  assert.ok(alive(parent.pid) && alive(writerPid), "부모와 writer의 준비 사건");
  fs.writeFileSync(path.join(dir, "meta.json"), JSON.stringify({ host_pid: parent.pid }));

  // 다른 프로세스를 가리키는 가짜 meta는 명령줄이 이 세션을 가리키지 않아 보존한다.
  const outsider = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
  children.push(outsider);
  pids.push(outsider.pid);
  const foreign = path.join(sessions, "foreign");
  fs.mkdirSync(foreign);
  fs.writeFileSync(path.join(foreign, "meta.json"), JSON.stringify({ host_pid: outsider.pid }));

  // 실제 관계없는 프로세스 argv 합계를 Node의 기본 1 MiB보다 크게 만든다.
  // 전체 명령줄 ps를 읽던 정리는 ENOBUFS로 실패하지만 후보 PID별 신원 검사는 이들과 무관하다.
  for (let i = 0; i < 10; i++) {
    const wide = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)", "x".repeat(120_000)], { stdio: "ignore" });
    children.push(wide);
    pids.push(wide.pid);
    await once(wide, "spawn");
  }

  await terminateSessionHosts(sessions);
  assert.equal(alive(parent.pid), false, "session host 종료를 기다린다");
  assert.equal(alive(writerPid), false, "writer 자손 종료를 기다린다");
  assert.equal(alive(outsider.pid), true, "명령줄이 다른 PID는 보존한다");
  for (const child of children.slice(2)) assert.equal(alive(child.pid), true, "관계없는 긴 argv 프로세스를 보존한다");
  fs.rmSync(root, { recursive: true });
  assert.equal(fs.existsSync(root), false, "재시도 없이 한 번에 폴더를 지운다");
});

test("배포 감시는 기록 PID의 명령줄을 조회하고 다른 프로세스는 보존한다", { skip: process.platform === "win32", timeout: 30_000 }, async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-guard-cleanup-"));
  const dir = path.join(root, "agent", "deployments");
  fs.mkdirSync(dir, { recursive: true });
  const children = [];
  t.after(async () => {
    for (const child of children) {
      if (child.exitCode === null && child.signalCode === null) {
        const ended = once(child, "exit");
        child.kill("SIGKILL");
        await ended;
      }
    }
    fs.rmSync(root, { recursive: true });
  });
  for (const [name, args] of [["owned", ["deployment-guard", root]], ["foreign", ["unrelated"]]]) {
    const child = spawn(process.execPath, ["-e", "process.send('ready'); process.stdin.resume();", ...args], {
      stdio: ["pipe", "ignore", "inherit", "ipc"],
    });
    children.push(child);
    await once(child, "message");
    fs.writeFileSync(path.join(dir, `${name}.guard.json`), JSON.stringify({ worker_pid: child.pid }));
  }
  await terminateDeployGuards(root);
  assert.equal(alive(children[0].pid), false, "소유한 배포 감시 종료를 기다린다");
  assert.equal(alive(children[1].pid), true, "기록 PID여도 명령줄이 다르면 보존한다");
});
