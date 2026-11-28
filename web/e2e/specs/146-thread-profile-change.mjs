import assert from "node:assert/strict";
import fs from "node:fs";
import { AA, aaa, browser, pairPage, root, step, until, waitOutput, webkit } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
const claude = `${root}/claude-profile.sh`;
fs.writeFileSync(claude, `while [ "$#" -gt 0 ]; do
  case "$1" in --session-id|--resume|--model) printf '%s %s\\n' "$1" "$2"; shift ;; esac
  shift
done
exec /bin/sh "${root}/fake.sh"
`);
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-b", "claude-a", "claude-b"]')
  + `\n[profiles.fake-b]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`
  + `\n[profiles.claude-a]\nagent = "claude"\nmodel = "model-a"\ncommand = ["/bin/sh", "${claude}"]\n`
  + `\n[profiles.claude-b]\nagent = "claude"\nmodel = "model-b"\ncommand = ["/bin/sh", "${claude}"]\n`);

const reason = "사용자가 채팅에서 작업 모델 변경을 지시";
const resumedReason = "사용자가 채팅에서 대화를 이어 모델 변경을 지시";
const quote = (text) => `'${text.replaceAll("'", "'\\''")}'`;

async function inPane(pane, tag, ...args) {
  const file = `${root}/${tag}`;
  fs.writeFileSync(`${file}.sh`, `${[AA, "--json", ...args].map(quote).join(" ")} > ${quote(file + ".out")} 2> ${quote(file + ".err")}
printf '%s\\n' "$?" > ${quote(file + ".rc")}
printf '${tag}-finished\\n'
`);
  aaa("pane", "run", pane, `post-probe ${file}.sh`);
  await waitOutput(pane, `${tag}-finished`, "recent");
  return {
    rc: Number(fs.readFileSync(`${file}.rc`, "utf8")),
    result: fs.readFileSync(`${file}.out`, "utf8"),
    err: fs.readFileSync(`${file}.err`, "utf8"),
  };
}

export default async function () {
  let original, changed, taskId;
  const thread = (id) => aaa("thread", "show", "demo", id);
  const task = () => aaa("task", "list").tasks.find(t => t.title === "모델을 바꿀 일");
  const queue = () => aaa("task", "queue").entries.find(q => q.task_id === taskId);
  await step("coordinator 권한과 실행 중 pane 종료 확인", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    aaa("machine", "workspaces", "labA", "--allow", "작업 폴더");
    aaa("task", "add", "모델을 바꿀 일", "--profile", "fake", "--repo", "작업 폴더", "--note", "이전 작업 기록");
    taskId = task().id;
    aaa("task", "reserve", taskId);
    original = await until("예약 thread 기동", () => {
      const t = task();
      return t.thread && queue()?.state === "running" && thread(t.thread);
    });
    await waitOutput(original.pane, "fake>");
    const coordinator = aaa("project", "open", "demo", "--profile", "fake").pane.id;
    await waitOutput(coordinator, "fake>");
    const cmd = ["thread", "restart", "demo", original.id, "--profile", "claude-a"];
    for (const [pane, tag, profile, extra, error] of [
      [original.pane, "worker-denied", "claude-a", ["--user-request", reason, "--stop-running"], /권한|보기/],
      [coordinator, "no-reason", "claude-a", ["--stop-running"], /--user-request/],
      [coordinator, "no-stop", "claude-a", ["--user-request", reason], /--stop-running/],
      [coordinator, "cross-resume", "claude-a", ["--user-request", reason, "--stop-running", "--resume"], /--resume/],
      [coordinator, "no-session", "fake-b", ["--user-request", reason, "--stop-running", "--resume"], /세션 ID/],
    ]) {
      const rejected = await inPane(pane, tag, ...cmd.slice(0, -1), profile, ...extra);
      assert.notEqual(rejected.rc, 0);
      assert.match(rejected.result + rejected.err, error);
      const unchanged = thread(original.id);
      assert.equal(unchanged.status, "open");
      assert.equal(unchanged.pane, original.pane);
      assert.equal(unchanged.profile_changes, undefined);
      assert.equal(aaa("pane", "get", original.pane).state, "running");
      assert.equal(task().profile, "fake");
    }
    fs.writeFileSync(`${original.cwd}/saved.txt`, "이전 작업 폴더에 저장");
    const crossRun = await inPane(coordinator, "cross-change", ...cmd, "--user-request", reason, "--stop-running");
    assert.equal(crossRun.rc, 0, JSON.stringify(crossRun));
    const cross = JSON.parse(crossRun.result);
    assert.notEqual(cross.id, original.id);
    assert.match(crossRun.err, /실행 중인 응답은 중단/);
    assert.equal(thread(original.id).status, "parked");
    assert.equal(fs.readFileSync(`${original.cwd}/saved.txt`, "utf8"), "이전 작업 폴더에 저장");
    assert.throws(() => aaa("pane", "get", original.pane));
    assert.equal(task().thread, cross.id);
    assert.equal(task().profile, "claude-a");
    assert.equal(queue().thread, cross.id);
    assert.equal(queue().profile, "claude-a");
    const first = await waitOutput(cross.pane, "--session-id", "recent");
    const key = first.matched_line.match(/--session-id ([0-9a-f-]+)/)?.[1];
    assert.ok(key, JSON.stringify(first));

    const same = await inPane(coordinator, "same-change", "thread", "restart", "demo", cross.id,
      "--profile", "claude-b", "--resume", "--user-request", resumedReason, "--stop-running");
    assert.equal(same.rc, 0, JSON.stringify(same));
    changed = JSON.parse(same.result);
    assert.equal(changed.id, cross.id);
    assert.equal(changed.cwd, cross.cwd);
    assert.notEqual(changed.pane, cross.pane);
    await waitOutput(changed.pane, `--resume ${key}`, "recent");
    await waitOutput(changed.pane, "--model model-b", "recent");
    assert.equal(task().profile, "claude-b");
    assert.equal(queue().profile, "claude-b");
    const history = aaa("task", "log", taskId).entries;
    assert.ok(history.some(e => e.by.includes(reason) && e.before.includes("fake") && e.after.includes("claude-a")));
    assert.ok(history.some(e => e.by.includes(resumedReason) && e.before.includes("claude-a") && e.after.includes("claude-b")));
  }, { always: true });

  const wk = await webkit.launch({ headless: true });
  try {
    for (const [engine, name, width] of [[browser, "Chromium", 1280], [wk, "WebKit", 393]]) {
      await step(`thread 모델 변경 기록 ${name}`, async () => {
        const { page } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 },
          { hash: `#thread=demo/${original.id}` });
        try {
          const history = page.getByTestId("thread-profile-change");
          await history.waitFor();
          assert.match(await history.innerText(), /fake → claude-a/);
          assert.ok((await history.innerText()).includes(reason));
          assert.equal(await page.getByTestId("park-banner-unpark").count(), 0);
          await history.getByRole("link", { name: changed.id }).click();
          const resumed = history.filter({ hasText: "claude-a → claude-b" });
          await resumed.waitFor();
          assert.ok((await resumed.innerText()).includes(resumedReason));
        } finally {
          await page.context().close();
        }
      });
    }
  } finally {
    await wk.close();
  }
}
