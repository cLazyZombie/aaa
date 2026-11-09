// 실제 host의 보고 재수집·목록 갱신·부분 답과 브라우저 새로고침을 본다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, browser, webkit, pairPage, domIs, coordinatorPost, waitOutput } from "../lib/harness.mjs";

const questions = `## Questions
### 1. 단추 자리
기존 화면의 단추를 고른다.
![시안](library/draft.png)
- A 오른쪽 아래 (추천)
- B 가운데
### 2. 눈 아이콘
기존 색을 고른다.
- A 계속 표시 (추천)
- B 누를 때 표시
`;
const call = (p, data) => p.evaluate(async data => (await fetch("/api/v1/call", {
  method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data),
})).json(), data);

function draftScenario(engine, width, label) {
  let t, page, errors, current;
  const card = () => page.getByTestId(`thread-ask-${t.id}`);
  const write = async (note, content = questions) => {
    const count = [...content.matchAll(/^### /gm)].length;
    const body = `[결정 필요] 질문 ${count}개: 입력 보존\n\n${note}\n\n${content}`;
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    current = await until("새 보고 수집", () => {
      const v = aaa("thread", "show", t.id);
      return v.report_hash === hash && v.questions && v.questions;
    });
    if (page) await domIs(page, "새 질문 ID 반영", id =>
      document.querySelector('.thread-ask-card h3 code')?.textContent === id, current.items[0].id);
  };
  const check = async (pick = "1B", text = "직접 쓴 답\n원래 색 유지") => {
    assert.equal(await card().getByTestId(`thread-ask-pick-${pick}`).getAttribute("aria-checked"), "true", `${label}: 선택 보존`);
    assert.equal(await card().getByTestId("thread-ask-text-2").inputValue(), text, `${label}: 직접 답 보존`);
  };
  return {
    prepare: async () => {
      fs.writeFileSync(`${root}/draft-task.md`, "질문 입력 보존 시험\n");
      t = aaa("thread", "start", "demo", "--title", `입력 보존 ${label}`, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/draft-task.md`);
      await until("처음 보고", () => aaa("thread", "show", t.id).report_hash);
      fs.copyFileSync(`${root}/pic.png`, `${t.work_dir}/library/draft.png`);
      await write("첫 보고");
      ({ page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { hash: `#question=${t.id}`, serviceWorkers: "block" }));
      await card().getByTestId("thread-ask-pick-1B").click();
      await card().getByTestId("thread-ask-text-2").fill("직접 쓴 답\n원래 색 유지");
      await check();
    },
    repost: async () => {
      await page.evaluate(id => { location.hash = `question=${id}`; }, current.items[0].id);
      await card().waitFor();
      const input = card().getByTestId("thread-ask-text-2");
      await input.focus();
      await input.evaluate(node => { node.setSelectionRange(2, 4); window.__draftInput = node; });
      const ids = current.items.map(q => q.id);
      await write("진행 내용만 갱신");
      assert.deepEqual(current.items.map(q => q.id), ids, "주변 글만 갱신하면 같은 q-ID로 게시한다");
      await check();
      assert.deepEqual(await input.evaluate(node => ({ same: node === window.__draftInput, focused: node === document.activeElement, selection: [node.selectionStart, node.selectionEnd] })),
        { same: true, focused: true, selection: [2, 4] }, "재게시가 편집 중인 입력칸·커서를 유지한다");
    },
    material: async () => {
      // 옛 q-ID 링크는 바뀐 자료의 질문으로 자동 전환하지 않는다. thread 카드에서 변경을 본다.
      await page.evaluate(id => { location.hash = `question=${id}`; }, t.id);
      await card().waitFor();
      const ids = current.items.map(q => q.id);
      fs.appendFileSync(`${t.work_dir}/library/draft.png`, "changed-material");
      await write("같은 URL의 시안 변경");
      assert.notEqual(current.items[0].id, ids[0], "같은 URL도 그림이 달라지면 새 질문이다");
      assert.equal(current.items[1].id, ids[1], "자료가 같은 다른 질문은 번호를 유지한다");
      assert.equal(await card().getByTestId("thread-ask-pick-1B").getAttribute("aria-checked"), "false");
      assert.equal(await card().getByTestId("thread-ask-text-2").inputValue(), "", "옛 자료의 브라우저 초안을 새 판단에 붙이지 않는다");
      await card().getByTestId("thread-ask-pick-1B").click();
      await card().getByTestId("thread-ask-text-2").fill("직접 쓴 답\n원래 색 유지");
      await check();
    },
    reload: async () => {
      await page.reload();
      await card().getByTestId("thread-ask-pick-1B").waitFor();
      await check();
      const post = await coordinatorPost({ request: true, title: `다른 카드 ${label}`, body: questions.replace("![시안](library/draft.png)\n", "") });
      const req = (await call(page, { op: "asks" })).ok.requests.find(r => r.title === `다른 카드 ${label}`);
      await page.evaluate(id => { location.hash = `ask=r:${id}`; }, req.id);
      const other = page.getByTestId(`request-ask-${req.id}`);
      await other.getByTestId("thread-ask-text-2").fill("coordinator 초안");
      await other.getByTestId("thread-ask-pick-1A").click();
      await page.evaluate(id => { location.hash = `question=${id}`; }, t.id);
      await card().waitFor();
      await check();
      await page.evaluate(id => { location.hash = `ask=r:${id}`; }, req.id);
      await other.waitFor();
      assert.equal(await other.getByTestId("thread-ask-text-2").inputValue(), "coordinator 초안");
      await page.reload();
      await other.waitFor();
      assert.equal(await other.getByTestId("thread-ask-text-2").inputValue(), "coordinator 초안");
      assert.equal(await other.getByTestId("thread-ask-pick-1A").getAttribute("aria-checked"), "true");
      aaa("post", "handled", post.id);
      await page.evaluate(id => { location.hash = `question=${id}`; }, t.id);
      await card().waitFor();
    },
    partial: async () => {
      aaa("question", "answer", current.items[0].id, "A");
      await domIs(page, "서버의 일부 답 반영", () => document.querySelector('[data-testid="thread-ask-pick-1A"]')?.getAttribute("aria-checked") === "true");
      await check("1A");
      const ids = current.items.map(q => q.id);
      await write("부분 답 뒤 진행·Next만 갱신\n## 진행\n- [x] 조사\n## Next\n검사한다");
      assert.deepEqual(current.items.map(q => q.id), ids, "부분 답 뒤에도 질문 번호를 보존한다");
      assert.deepEqual(current.draft.picks, [0, null], "서버의 부분 선택도 새 보고에 이어진다");
      assert.equal(aaa("question", "list").questions.filter(q => ids.includes(q.id) && q.state === "open").length, 1, "미답 질문만 계속 열린다");
      await check("1A");
      await card().getByTestId("thread-ask-pick-1B").click();
      await check("1B");
      await page.reload();
      await card().waitFor();
      await check("1B");
    },
    failure: async () => {
      // 실제 요청을 서버에 전달하지 않는 네트워크 실패. 초안은 지우지 않아야 한다.
      await page.route("**/api/v1/call", route => route.request().postDataJSON()?.op === "thread_answer" ? route.abort("failed") : route.fallback());
      await card().getByTestId("thread-ask-send").click();
      await card().getByRole("alert").waitFor();
      await check("1B");
      await page.unrouteAll({ behavior: "wait" });
      await page.reload();
      await card().waitFor();
      await check("1B");
      await write("선택지가 달라짐", questions.replace("- B 가운데", "- B 왼쪽 위"));
      assert.equal(await card().getByTestId("thread-ask-pick-1B").getAttribute("aria-checked"), "false");
      assert.equal(await card().getByTestId("thread-ask-text-2").inputValue(), "");
    },
    send: async () => {
      await card().getByTestId("thread-ask-pick-1B").click();
      await card().getByTestId("thread-ask-text-2").fill("새 질문 답");
      await card().getByTestId("thread-ask-send").click();
      await waitOutput(t.pane, "2번 직접 쓴 답: 새 질문 답");
      const answer = await until("답 저장", () => aaa("thread", "show", t.id).questions?.answer);
      assert.deepEqual(answer.picks, [1, null]);
      assert.deepEqual(answer.comments, [null, "새 질문 답"]);
      await until("성공한 답 초안 삭제", () => page.evaluate(id => localStorage.getItem(`aaa:question-draft:${JSON.stringify(["question", "demo", id])}`) === null, t.id));
    },
    single: async () => {
      const single = questions.slice(0, questions.indexOf("### 2.")).replace("단추 자리", "다음 화면 단추 자리");
      // 이미 답한 질문은 host가 제거한다. 다음 입력 시험은 별개의 실제 질문으로 연다.
      await write("질문 하나", single);
      assert.equal(await card().getByTestId("thread-ask-text").inputValue(), "");
      await card().getByTestId("thread-ask-pick-1B").click();
      const input = card().getByTestId("thread-ask-text");
      const text = "한글 직접 답 끝  ";
      await input.fill(text);
      await page.reload();
      await input.waitFor();
      assert.equal(await input.inputValue(), text, "공통 직접 답의 원문도 새로고침 뒤 남는다");
      assert.equal(await card().getByTestId("thread-ask-pick-1B").getAttribute("aria-checked"), "true");
      await write("질문 하나의 주변 글 갱신", single);
      assert.equal(await input.inputValue(), text);
      await card().getByTestId("thread-ask-send").click();
      await waitOutput(t.pane, `직접 쓴 답: ${text.trim()}`);
      await until("질문 하나의 답 저장", () => aaa("thread", "show", t.id).questions?.answer?.text === text.trim());
      assert.deepEqual(errors, []);
      const final = questions.replace("단추 자리", "다음 시안 자리").replace("눈 아이콘", "다음 시안 아이콘");
      await write("부분 답을 모아 전달", final);
      const ids = current.items.map(q => q.id);
      aaa("question", "answer", ids[0], "A", "--text", "간격 유지\n부분 의견 보존");
      await write("부분 답 뒤 주변 글 갱신", final);
      assert.deepEqual(current.items.map(q => q.id), ids);
      assert.deepEqual(current.draft.comments, ["간격 유지\n부분 의견 보존", null]);
      aaa("question", "answer", ids[1], "B");
      // fake 셸은 입력 줄 사이에 prompt를 찍는다. 두 출력 사건과 저장 원문의 줄바꿈을 따로 본다.
      await waitOutput(t.pane, "1번 직접 쓴 답(의견): 간격 유지");
      await waitOutput(t.pane, "부분 의견 보존");
      const complete = await until("현재 보고에 부분 답 전달 완료", () => aaa("thread", "show", t.id).questions?.answer);
      assert.deepEqual(complete.picks, [0, 1]);
      assert.deepEqual(complete.comments, ["간격 유지\n부분 의견 보존", null]);
      assert.ok(fs.readFileSync(`${root}/projects/demo/threads/${t.id}.task.md`, "utf8").includes("1번 직접 쓴 답(의견): 간격 유지\n부분 의견 보존"));
      await page.context().close();
      aaa("thread", "resolve", t.id);
    },
  };
}

export default async function () {
  let wk;
  await step("준비: 질문 초안 실험실", async () => {
    if (!aaa("project", "list").projects.some(p => p.name === "demo")) aaa("project", "create", "demo");
    wk = await webkit.launch();
  }, { always: true });
  try {
  const c = draftScenario(browser, 1280, "Chromium");
  await step("질문 초안 준비: Chromium", c.prepare);
  await step("동일 질문 재게시 입력 보존: Chromium", c.repost);
  await step("같은 URL의 그림 변경은 옛 초안을 분리: Chromium", c.material);
  await step("새로고침·카드 왕복 초안 보존: Chromium", c.reload);
  await step("부분 답 갱신은 다른 입력을 보존: Chromium", c.partial);
  await step("실패한 전송과 바뀐 질문 경계: Chromium", c.failure);
  await step("보존한 답을 현재 보고에 한 번 전송: Chromium", c.send);
  await step("질문 하나의 직접 답·선택 초안 보존: Chromium", c.single);
  const m = draftScenario(wk, 1280, "Mac Safari");
  await step("질문 초안 준비: Mac Safari", m.prepare);
  await step("동일 질문 재게시 입력 보존: Mac Safari", m.repost);
  await step("같은 URL의 그림 변경은 옛 초안을 분리: Mac Safari", m.material);
  await step("새로고침·카드 왕복 초안 보존: Mac Safari", m.reload);
  await step("부분 답 갱신은 다른 입력을 보존: Mac Safari", m.partial);
  await step("실패한 전송과 바뀐 질문 경계: Mac Safari", m.failure);
  await step("보존한 답을 현재 보고에 한 번 전송: Mac Safari", m.send);
  await step("질문 하나의 직접 답·선택 초안 보존: Mac Safari", m.single);
  const p = draftScenario(wk, 390, "iPhone Safari");
  await step("질문 초안 준비: iPhone Safari", p.prepare);
  await step("동일 질문 재게시 입력 보존: iPhone Safari", p.repost);
  await step("같은 URL의 그림 변경은 옛 초안을 분리: iPhone Safari", p.material);
  await step("새로고침·카드 왕복 초안 보존: iPhone Safari", p.reload);
  await step("부분 답 갱신은 다른 입력을 보존: iPhone Safari", p.partial);
  await step("실패한 전송과 바뀐 질문 경계: iPhone Safari", p.failure);
  await step("보존한 답을 현재 보고에 한 번 전송: iPhone Safari", p.send);
  await step("질문 하나의 직접 답·선택 초안 보존: iPhone Safari", p.single);
  } finally { await wk.close(); }
}
