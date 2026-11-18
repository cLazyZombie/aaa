import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, root, shot, step, waitOutput } from "../lib/harness.mjs";

const script = `${root}/codex-speed.sh`;
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "ultrafast"]')
  + `\n[profiles.ultrafast]\nagent = "codex"\ncommand = ["/bin/sh", "${script}"]\nargs = ["-c", 'service_tier="ultrafast"']\n`);
// 대화 기록 없이 Codex와 같은 OSC 제목을 낸다. 둘째 pane은 첫 제목을 늦게 내보낸다.
fs.writeFileSync(script, `#!/bin/sh
if [ "$AAA_THREAD" = t-0001 ]; then
  printf '\\033]0;Ready | demo | Ultrafast\\007'
fi
printf 'speed-fixture-ready\\n'
while IFS= read -r line; do
  case "$line" in
    set-standard) printf '\\033]0;Ready | demo | Standard\\007' ;;
    set-ultrafast) printf '\\033]0;Ready | demo | Ultrafast\\007' ;;
  esac
done
`);

export default async function ({ page }) {
  await step("대화 기록 없이 두 Ultrafast 번개 표시와 실행 중 속도 전환", async () => {
    aaa("project", "create", "demo");
    const task = `${root}/speed-task.md`;
    fs.writeFileSync(task, "실행 속도 표지 확인\n");
    const start = title => aaa("thread", "start", "demo", "--title", title, "--machine", "labA",
      "--profile", "ultrafast", "--kind", "dir", "--task-file", task);
    const first = start("제목이 있는 Ultrafast");
    const second = start("제목을 기다리는 Ultrafast");
    await Promise.all([waitOutput(first.pane, "speed-fixture-ready"), waitOutput(second.pane, "speed-fixture-ready")]);

    const mark = t => page.getByTestId(`thread-demo-${t.id}`).getByTestId("fast-mark");
    const ultrafast = t => mark(t).and(page.getByRole("img", { name: "ultrafast mode", exact: true }));
    await Promise.all([ultrafast(first).waitFor(), ultrafast(second).waitFor()]);
    for (const t of [first, second]) {
      const view = aaa("thread", "show", t.id);
      assert.equal(view.speed, "ultrafast");
      assert.equal(view.agent?.model_observation ?? null, null);
    }
    await shot(page, "codex-speed");

    aaa("pane", "run", second.pane, "set-standard");
    await mark(second).waitFor({ state: "detached" });
    assert.ok(await ultrafast(first).isVisible());
    aaa("pane", "run", second.pane, "set-ultrafast");
    await ultrafast(second).waitFor();
  });
}
