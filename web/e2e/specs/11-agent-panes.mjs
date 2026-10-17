// 웹 E2E agent pane: 확인창·끝남(안 봄)·대화 기록·그림 붙여 넣기.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 11-agent-panes(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import {
  AA, root, aaa, waitOutput, until, domIs, screenText, pasteFile, bannerPath, shot, pairPage, step, browser,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("확인창 질문: 카드·상태 표시 → reader는 못 답함 → 예", async () => {
    // 세션 안에서 Claude처럼 선택 창을 그리고 hook 사건(PermissionRequest)을 보낸다
    const script =
      'printf "Allow Bash(make test)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
      '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"make test"}}\'; ' +
      "stty -echo; exec cat > /dev/null";
    aaa("tab", "create", "--machine", "labA", "--label", "q1", "--", "/bin/sh", "-c", script, AA);
    // 목록에는 그 줄의 빨간 ?만(차이 목록 Q1·S2, 참고 Sidebar.tsx StatusBadge): 카드는 그 pane을 열면 터미널 아래에
    const state = (pg) => pg.getByTestId("agent-q1");
    await state(page).and(page.locator('[data-status="blocked"]')).waitFor();
    if ((await state(page).locator(".sidebar-status-mark").innerText()) !== "?") throw new Error("질문 줄에 ? 표시가 없다");
    if ((await state(page).getAttribute("aria-label")) !== "agent 답을 기다림") throw new Error(`상태 말: ${await state(page).getAttribute("aria-label")}`);
    if ((await page.locator(".question").count()) !== 0) throw new Error("pane을 열기 전에 목록에 질문 카드가 있다");
    await page.getByTestId("pane-q1").click();
    const q = aaa("question", "list").questions.find((x) => x.text.includes("make test"));
    const card = page.getByTestId(`dock-question-${q.id}`);
    await card.waitFor();
    if (!(await card.innerText()).includes("make test")) throw new Error(`카드 본문: ${await card.innerText()}`);
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await state(rd.page).and(rd.page.locator('[data-status="blocked"]')).waitFor();
    await rd.page.getByTestId("pane-q1").click();
    const rcard = rd.page.getByTestId(`dock-question-${q.id}`);
    await rcard.waitFor();
    if ((await rcard.getByRole("button", { name: "예" }).count()) !== 0) throw new Error("reader에게 답 버튼");
    if (!(await rcard.innerText()).includes("보기 권한이라 답할 수 없다")) throw new Error(`reader 카드: ${await rcard.innerText()}`);
    await shot(page, "desktop-question");
    await card.getByRole("button", { name: "예" }).click();
    await card.waitFor({ state: "detached" });
    await rcard.waitFor({ state: "detached" });
    const all = aaa("question", "list", "--all").questions;
    if (all.length !== 1 || all[0].state !== "answered" || all[0].answer !== "yes") throw new Error(JSON.stringify(all));
    await state(page).and(page.locator('[data-status="working"]')).waitFor();
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("확인창 답이 실패하면 카드 안에 보인다(herdr-web-ui PromptCard.tsx pending·role=alert, QA t-0337 Q3): 휴대폰에서 답하는 동안 선택지가 막히고, host가 선택 창을 못 찾은 오류가 그 카드에 뜨며 다시 누를 수 있다", async () => {
    // 선택 창 끝 줄(「Esc to cancel」)이 없는 확인창: host는 MENU_WAIT 동안 찾다가 오류로 답한다
    const script =
      'printf "Allow Bash(make lint)?\\n\\342\\235\\257 1. Yes\\n  2. No\\n"; ' +
      '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"make lint"}}\'; ' +
      "stty -echo; exec cat > /dev/null";
    aaa("tab", "create", "--machine", "labA", "--label", "q2", "--", "/bin/sh", "-c", script, AA);
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await ph.page.getByTestId("agent-q2").and(ph.page.locator('[data-status="blocked"]')).waitFor();
    await ph.page.getByTestId("pane-q2").click();
    const q = aaa("question", "list").questions.find((x) => x.text.includes("make lint"));
    const id = `dock-question-${q.id}`;
    const card = ph.page.getByTestId(id);
    await card.waitFor();
    const answered = ph.page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "question_answer");
    await card.getByRole("button", { name: "예" }).click();
    // 답이 가는 동안: 카드는 aria-busy, 선택지는 막힘(누른 순간의 모습을 본다. 답은 MENU_WAIT 뒤에 온다)
    const busy = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(busy) !== JSON.stringify(["true", [true, true]])) throw new Error(`답하는 동안 카드 ${JSON.stringify(busy)}`);
    const res = await (await answered).json();
    if (res.ok !== undefined) throw new Error(`실패해야 할 답이 성공했다: ${JSON.stringify(res)}`);
    await domIs(ph.page, "카드 안 오류", (sel) => document.querySelector(sel)?.textContent.includes("터미널에서 직접 답한다"), `[data-testid="${id}"] [role=alert]`);
    const after = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(after) !== JSON.stringify(["false", [false, false]])) throw new Error(`오류 뒤 카드 ${JSON.stringify(after)}`);
    // 오류는 카드에만(휴대폰은 pane을 보는 동안 목록 머리 띠가 숨는다)
    if (!(await card.getByRole("alert").isVisible())) throw new Error("휴대폰에서 카드 오류가 보이지 않는다");
    await shot(ph.page, "phone-question-error");
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    aaa("pane", "kill", "q2");
  });

  await step("끝남(안 봄)(herdr PaneState::seen, 차이 목록 S1): 보는 화면 없이 턴이 끝나면 초록 점, 그 pane을 열면 사라지고, 보는 동안 끝난 턴은 남지 않으며, 가려진 동안 끝나면 다시 보일 때 사라진다", async () => {
    // 줄 하나를 받을 때마다 턴을 열고(UserPromptSubmit) 다음 줄에 닫는다(Stop): 시험이 턴의 끝을 정한다
    const script =
      'while read a; do "$0" hook --agent claude \'{"hook_event_name":"UserPromptSubmit"}\'; ' +
      'read b; "$0" hook --agent claude \'{"hook_event_name":"Stop"}\'; done';
    aaa("tab", "create", "--machine", "labA", "--label", "seen1", "--", "/bin/sh", "-c", script, AA);
    const inUse = (p, on) =>
      p.evaluate((on) => {
        if (on) delete document.hasFocus;
        else document.hasFocus = () => false;
        window.dispatchEvent(new Event(on ? "focus" : "blur"));
      }, on);
    const state = page.getByTestId("agent-seen1");
    const is = (s) => state.and(page.locator(`[data-status="${s}"]`)).waitFor();
    const using = async () => (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use));
    const turn = async (end, inUseNow) => {
      aaa("pane", "run", "seen1", "go");
      await is("working");
      // presence는 턴을 열기 전에 보냈고 host를 한 바퀴 돈 working이 왔다: host가 이미 안다(기다리지 않고 확인만 한다)
      const n = await using();
      if ((n > 0) !== inUseNow) throw new Error(`host가 아는 쓰는 화면 수 ${n}(쓰는 중 ${inUseNow})`);
      aaa("pane", "run", "seen1", "end");
      await is(end);
    };
    await inUse(page, true);
    // 1) 다른 pane(q1)을 보는 동안 끝났다: 끝남(안 봄), 초록 점과 그 말
    await turn("done", true);
    if ((await state.locator(".sidebar-status-dot").count()) !== 1) throw new Error("끝남에 점이 없다");
    if ((await state.getAttribute("aria-label")) !== "agent 끝남(안 봄)") throw new Error(`상태 말: ${await state.getAttribute("aria-label")}`);
    await shot(page, "desktop-unseen-done");
    // 2) 그 pane을 열면 봤다
    await page.getByTestId("pane-seen1").click();
    await is("idle");
    // 3) 보는 동안 끝난 턴은 끝남으로 남지 않는다
    await turn("idle", true);
    // 4) 가려진(쓰는 화면이 아닌) 동안 끝났다: 열어 둔 pane이라도 끝남, 다시 쓰는 화면이 되면 봤다
    await inUse(page, false);
    await turn("done", false);
    await inUse(page, true);
    await is("idle");
    aaa("pane", "kill", "seen1");
  });

  await step("대화 기록(읽기 전용): Claude 기록에서 사람·답·도구 줄만, 생각·도구 결과는 빼고, 새 줄을 따라가고, 기록 폴더 밖은 거부", async () => {
    const dir = `${root}/claude/projects/-lab-chat`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/s1.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    fs.writeFileSync(
      tr,
      L({ type: "user", message: { role: "user", content: "결제 지연을 줄여 줘" } }) +
        L({ type: "user", message: { role: "user", content: "<system-reminder>끼운 글</system-reminder>" } }) +
        L({
          type: "assistant",
          message: {
            content: [
              { type: "thinking", thinking: "속생각" },
              { type: "text", text: "캐시를 넣었다" },
              { type: "tool_use", name: "Bash", input: { command: "cargo test" } },
            ],
          },
        }) +
        L({ type: "user", message: { content: [{ type: "tool_result", content: "도구결과본문" }] } }),
    );
    const hookPane = (name, path) =>
      aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
        JSON.stringify({ hook_event_name: "Stop", session_id: "s1", transcript_path: path }));
    hookPane("chat1", tr);
    await page.getByTestId("pane-chat1").click();
    await until("대화 단추", async () => (await page.getByTestId("chat-toggle").count()) === 1);
    await page.getByTestId("chat-toggle").click();
    const chat = page.getByTestId("chat");
    const roles = async () => chat.locator(".chat-msg").evaluateAll((els) => els.map((e) => [e.dataset.role, e.textContent]));
    await until("대화 줄", async () => (await roles()).length === 3);
    const want = [["user", "결제 지연을 줄여 줘"], ["assistant", "캐시를 넣었다"], ["tool", "Bash: cargo test"]];
    if (JSON.stringify(await roles()) !== JSON.stringify(want)) throw new Error(JSON.stringify(await roles()));
    const all = await chat.innerText();
    if (all.includes("속생각") || all.includes("도구결과본문") || all.includes("끼운 글")) throw new Error(`빼야 할 글이 보인다: ${all}`);
    // 새 줄이 생기면 따라간다
    fs.appendFileSync(tr, L({ type: "assistant", message: { content: [{ type: "text", text: "시험도 통과했다" }] } }));
    await until("새 줄", async () => (await roles()).length === 4);
    await shot(page, "desktop-chat");
    // 터미널로 돌아간다
    await page.getByTestId("chat-toggle").click();
    if (await page.getByTestId("terminal").isHidden()) throw new Error("터미널로 돌아가지 않았다");
    // CLI도 같은 기록을 본다
    const c = aaa("pane", "chat", "chat1");
    if (c.messages.length !== 4 || c.harness !== "claude") throw new Error(JSON.stringify(c));
    // 기록 폴더 밖을 가리키는 hook(세션 안 누구나 보낼 수 있다)은 읽지 않는다
    fs.writeFileSync(`${root}/secret.jsonl`, L({ type: "user", message: { role: "user", content: "비밀" } }));
    hookPane("chat2", `${root}/secret.jsonl`);
    await until("chat2 harness", () => aaa("pane", "list").panes.find((p) => p.name === "chat2")?.agent?.harness === "claude");
    let refused = "";
    try {
      aaa("pane", "chat", "chat2");
    } catch (e) {
      // 오류 JSON은 stderr로 온다(herdr와 같다)
      refused = String(e.stderr);
    }
    if (!refused.includes("기록 폴더 밖") || refused.includes("비밀")) throw new Error(`밖의 파일: ${refused}`);
    for (const n of ["chat1", "chat2"]) aaa("pane", "kill", n);
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("그림 붙여 넣기·끌어 놓기(herdr-web-ui): 셸·harness 입력창에 따옴표 경로(Enter 없음), 글이 있으면 글, 여러 파일은 놓은 순서, 글 놓기, 확인창·형식·reader 거부", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const banner = page.getByTestId("attach-banner");
    // 1) 일반 셸(t1): 대상 머신 세션 폴더에 저장하고 셸에도 따옴표 경로와 빈칸을 넣는다(결정 20 D7, Enter 없음)
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("$"));
    // 띠는 터미널 위에 뜬다(herdr-web-ui .terminal-banners): 흐름에 있으면 띠가 날 때 칸이 줄어 PTY 크기가 바뀐다
    const termBox = () => page.locator(".term-scroll").evaluate((el) => `${el.clientWidth}x${el.clientHeight}`);
    const boxBefore = await termBox();
    await pasteFile(page, png, "image/png");
    await until("셸 경로 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    if ((await termBox()) !== boxBefore) throw new Error(`그림 띠가 터미널 칸을 바꿨다: ${boxBefore} → ${await termBox()}`);
    const p1 = bannerPath(await banner.innerText());
    if (!p1?.startsWith(`${root}/s/agent/sessions/`)) throw new Error(`저장 경로: ${await banner.innerText()}`);
    if (!fs.readFileSync(p1).equals(png)) throw new Error("저장한 그림이 다르다");
    if ((fs.statSync(p1).mode & 0o777) !== 0o600) throw new Error(`권한 ${(fs.statSync(p1).mode & 0o777).toString(8)}`);
    // 긴 경로는 화면에서 줄이 접힌다: 접힌 줄을 이어 본다(herdr recent-unwrapped)
    await waitOutput("t1", `'${p1}'`, "recent-unwrapped");
    // Enter가 없었다: 셸은 실행하지 않았다. 그 줄을 지우고 표지를 실행한다: 셸은 입력을 차례로 처리하므로 경로를 실행했다면
    // 그 오류가 표지보다 먼저 나온다
    aaa("pane", "send-text", "t1", "\u0015");
    aaa("pane", "run", "t1", "echo paste-fence-$((2+2))");
    await waitOutput("t1", "paste-fence-4");
    // 붙인 파일은 있으니 실행했다면 「Permission denied」다(0600). 옛 단언은 「No such file」만 봐 이 결함을 잡지 못했다
    const ran = /No such file|Permission denied|command not found|cannot execute/.exec(aaa("pane", "read", "t1", "--source", "visible").text.replaceAll("\n", ""));
    if (ran) throw new Error(`셸이 경로를 실행했다: ${ran[0]}`);
    // 2) 형식이 다른 파일(SVG)은 host가 signature로 거부한다
    await pasteFile(page, Buffer.from("<svg xmlns='http://www.w3.org/2000/svg'/>"), "image/svg+xml", "x.svg");
    await until("형식 거부", async () => (await page.locator(".banner.err").innerText()).includes("PNG·JPEG·GIF·WebP"));
    if ((await termBox()) !== boxBefore) throw new Error(`오류 띠가 터미널 칸을 바꿨다: ${boxBefore} → ${await termBox()}`);

    // 3) harness pane(hook 보냄, 입력 대기, Claude처럼 bracketed paste를 켬): 여러 조각(1.2MB) 그림의 경로를 bracketed paste로
    // 넣고 Enter는 보내지 않는다(감싸기는 프로그램이 켠 모드를 따른다, herdr-web-ui xterm paste)
    const big = Buffer.concat([png.subarray(0, 8), Buffer.alloc(1_200_000, 7)]);
    const recv = `${root}/h1.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h1", "--", "/bin/sh", "-c",
      '"$0" hook --agent claude \'{"hook_event_name":"Stop"}\'; stty raw -echo; printf "\\033[?2004h\\342\\235\\257 \\n"; exec cat > "$1"', AA, recv);
    await until("h1 harness 상태", async () => (await page.getByTestId("agent-h1").count()) === 1);
    await page.getByTestId("pane-h1").click();
    await until("h1 터미널", async () => (await screenText(page)).includes("❯"));
    await pasteFile(page, big, "image/png");
    await until("입력창 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    const p2 = bannerPath(await banner.innerText());
    if (!fs.readFileSync(p2).equals(big)) throw new Error("여러 조각 그림이 다르다");
    await until("PTY가 받은 경로", () => fs.readFileSync(recv, "latin1") === `\x1b[200~'${p2}' \x1b[201~`);
    await shot(page, "desktop-attach");
    // 받은 것 가운데 이 뒤에 온 것
    let seen = fs.readFileSync(recv, "latin1").length;
    const recvNext = () => fs.readFileSync(recv, "latin1").slice(seen);
    // 3a) 클립보드에 글과 그림이 같이 있으면 글이다(복사한 경로도 글로 남는다). 프로그램이 bracketed paste를 켰으니 xterm이 감싼다
    await page.evaluate(
      ({ b64 }) => {
        const dt = new DataTransfer();
        dt.setData("text/plain", "mixed-text");
        dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "m.png", { type: "image/png" }));
        document.querySelector('[data-testid="terminal"] textarea').dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      },
      { b64: png.toString("base64") },
    );
    await until("글만 붙여 넣음", () => recvNext() === "\x1b[200~mixed-text\x1b[201~");
    // 그림까지 올렸다면 그 경로는 다음 3b의 올리기보다 먼저 온다(올리기는 한 줄로 선다, Term.tsx fileQueueRef). 그래서 받은 글
    // 길이만큼만 넘긴다: 늦게 온 경로는 3b의 「두 경로」 앞에 남아 그 단언이 실패한다
    seen += "\x1b[200~mixed-text\x1b[201~".length;
    // 3b) 파일 여러 개를 끌어 놓으면 놓은 순서대로 올리고 경로를 차례로 넣는다
    const pa = Buffer.concat([png.subarray(0, 8), Buffer.alloc(300_000, 1)]);
    const pb = Buffer.concat([png.subarray(0, 8), Buffer.alloc(2_000, 2)]);
    const drop = (files, text) =>
      page.evaluate(
        ({ files, text }) => {
          const dt = new DataTransfer();
          for (const f of files) dt.items.add(new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: "image/png" }));
          if (text) dt.setData("text/plain", text);
          const at = document.querySelector('[data-testid="terminal"] .xterm-screen');
          const over = new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true });
          at.dispatchEvent(over);
          at.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
          // dropEffect는 볼 수 없다: 지어낸 DataTransfer는 끌기 중이 아니라 dropEffect 대입을 버린다(Chromium에서 확인)
          return { prevented: over.defaultPrevented };
        },
        { files: files.map(([name, b]) => ({ name, b64: b.toString("base64") })), text },
      );
    const over = await drop([["a.png", pa], ["b.png", pb]]);
    if (!over.prevented) throw new Error(`끌어 오기를 받지 않았다 ${JSON.stringify(over)}`);
    const two = await until("두 경로", () => {
      const m = /^\x1b\[200~'([^']+)' \x1b\[201~\x1b\[200~'([^']+)' \x1b\[201~$/.exec(recvNext());
      return m ? [m[1], m[2]] : null;
    }).catch((e) => {
      throw new Error(`${e.message}: 받은 것 ${JSON.stringify(recvNext())}`);
    });
    if (!fs.readFileSync(two[0]).equals(pa) || !fs.readFileSync(two[1]).equals(pb)) throw new Error(`놓은 순서가 아니다: ${two}`);
    seen = fs.readFileSync(recv, "latin1").length;
    // 3c) 글을 끌어 놓으면 붙여 넣는다(Enter 없음)
    await drop([], "dropped-text");
    await until("놓은 글", () => recvNext() === "\x1b[200~dropped-text\x1b[201~");

    // 4) 확인창이 떠 있는 harness pane에는 넣지 않는다
    const recv2 = `${root}/h2.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h2", "--", "/bin/sh", "-c",
      'printf "Allow Bash(rm x)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
        '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}\'; ' +
        'stty raw -echo; exec cat > "$1"', AA, recv2);
    await page.getByTestId("agent-h2").and(page.locator('[data-status="blocked"]')).waitFor();
    await page.getByTestId("pane-h2").click();
    await until("h2 터미널", async () => (await screenText(page)).includes("Esc to cancel"));
    await pasteFile(page, png, "image/png");
    await until("확인창 배너", async () => (await banner.innerText()).includes("확인창이 떠 있어"));
    // host는 넣지 않기로 정한 뒤 답했다. 같은 host → agent 길로 표지 글을 보내 그것만 왔는지 본다(넣었다면 경로가 먼저 온다)
    aaa("pane", "send-text", "h2", "fence-h2");
    await until("h2 표지", () => fs.readFileSync(recv2, "latin1").includes("fence-h2"));
    if (fs.readFileSync(recv2, "latin1") !== "fence-h2") throw new Error(`확인창에 입력이 갔다: ${JSON.stringify(fs.readFileSync(recv2, "latin1"))}`);

    // 5) reader 연결은 화면 밖에서 직접 보내도 host가 거부한다
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const h1id = aaa("pane", "list").panes.find((p) => p.name === "h1").id;
    const got = await rd.page.evaluate(
      async ({ id, b64 }) => {
        const ws = new WebSocket(`ws://${location.host}/api/v1/panes/${id}/ws`);
        return await new Promise((resolve) => {
          ws.onopen = () => ws.send(JSON.stringify({ t: "attach", offset: 0, total: 70, sha256: "0".repeat(64), data: b64 }));
          ws.onmessage = (m) => {
            if (typeof m.data !== "string") return;
            const msg = JSON.parse(m.data);
            if (msg.t === "error" || msg.t === "attached") resolve(msg);
          };
          setTimeout(() => resolve({ t: "timeout" }), 5000);
        });
      },
      { id: h1id, b64: png.toString("base64") },
    );
    if (got.t !== "error" || got.code !== "forbidden") throw new Error(`reader 첨부: ${JSON.stringify(got)}`);
    await rd.page.context().close();
    for (const n of ["h1", "h2"]) aaa("pane", "kill", n);
  });
}
