// session host는 agent와 분리되어 있다. 이 실험실의 meta/명령줄로 확인한 PID와 그 자손만 정리한다.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

function processes() {
  return execFileSync("ps", ["-axo", "pid=,ppid=,stat="], { encoding: "utf8" })
    .split("\n")
    .flatMap((line) => {
      const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s*$/.exec(line);
      return m ? [{ pid: Number(m[1]), parent: Number(m[2]), state: m[3] }] : [];
    });
}

// 컴파일러 등 관계없는 긴 argv를 모으면 ps 출력이 Node 버퍼를 넘는다.
// 종료 후보의 명령줄만 따로 읽고 기존 신원 대조는 그대로 유지한다.
function commandOf(pid) {
  try { return execFileSync("ps", ["-p", String(pid), "-o", "command="], { encoding: "utf8" }).trimEnd(); }
  catch (e) { if (e.status === 1) return null; throw e; }
}

const live = (p) => p && !p.state.startsWith("Z");
function signal(p, sig) {
  try { process.kill(p.pid, sig); }
  catch (e) { if (e.code !== "ESRCH") throw e; }
}

async function waitFor(what, done) {
  const deadline = Date.now() + 20_000;
  while (!done()) {
    if (Date.now() >= deadline) throw new Error(`세션 정리 대기: ${what}`);
    await delay(10);
  }
}

async function terminateTree(root) {
  const frozen = [];
  async function freeze(p) {
    const current = processes().find((row) => row.pid === p.pid);
    if (!live(current) || commandOf(p.pid) !== p.command) return;
    // 부모를 먼저 멈춰야 자손 목록을 읽는 동안 새 writer가 생기지 않는다.
    frozen.push(p);
    signal(p, "SIGSTOP");
    await waitFor(`PID ${p.pid} 정지`, () => {
      const row = processes().find((row) => row.pid === p.pid);
      return !live(row) || row.state.startsWith("T");
    });
    for (const child of processes().filter((row) => row.parent === p.pid && live(row))) {
      const command = commandOf(child.pid);
      if (command !== null) await freeze({ ...child, command });
    }
  }
  try {
    await freeze(root);
  } finally {
    // 자손부터 끝내도 부모는 멈춰 있으므로 후속 cp/mkdir를 실행할 수 없다.
    for (const p of frozen.reverse()) signal(p, "SIGKILL");
    await waitFor("session host와 자손 종료", () => {
      const table = processes();
      return frozen.every((p) => !live(table.find((row) => row.pid === p.pid)) || commandOf(p.pid) !== p.command);
    });
  }
}

export async function terminateSessionHosts(dir) {
  if (!fs.existsSync(dir)) return;
  for (const id of fs.readdirSync(dir)) {
    const file = path.join(dir, id, "meta.json");
    let meta;
    try { meta = JSON.parse(fs.readFileSync(file, "utf8")); }
    catch (e) { if (e.code === "ENOENT") continue; throw e; }
    const p = processes().find((p) => p.pid === meta.host_pid);
    if (!live(p)) continue;
    const command = commandOf(p.pid);
    if (command !== null && command.includes(path.join(dir, id))) await terminateTree({ ...p, command });
  }
}

// 독립 배포 감시(재시작 rollout, W6c)도 agent의 자식이 아니다(setsid). 기록 PID의 명령줄이 이 실험실 상태 폴더의
// `deployment-guard`일 때만 끝낸다.
export async function terminateDeployGuards(stateDir) {
  const dir = path.join(stateDir, "agent", "deployments");
  if (!fs.existsSync(dir)) return;
  for (const name of fs.readdirSync(dir).filter((n) => n.endsWith(".guard.json"))) {
    let view;
    try { view = JSON.parse(fs.readFileSync(path.join(dir, name), "utf8")); }
    catch (e) { if (e.code === "ENOENT") continue; throw e; }
    const p = processes().find((p) => p.pid === view.worker_pid);
    if (live(p) && p.command.includes("deployment-guard") && p.command.includes(stateDir)) await terminateTree(p);
  }
}
