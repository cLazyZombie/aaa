// 실제 thread 답·원문 보존과 새 보고/늦은 응답 경합을 검사한다.
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { root, aaa, until, step, browser, webkit, waitOutput, shot } from "../lib/harness.mjs";
import { questionReplyScenario } from "../lib/question-reply.mjs";

const pictures = (p, sel) =>
  p.evaluate((sel) => {
    const imgs = [...document.querySelectorAll(`${sel} img`)];
    return imgs.length > 0 && imgs.every((i) => i.complete && i.naturalWidth > 0) ? imgs.length : 0;
  }, sel);

export default async function ({ page }) {
  let wk;

  await step("늦은 답 응답: 새 보고의 직접 답·카드·보내기 활성을 보존하고 각 보고에 한 번씩 전달한다", async () => {
    wk = await webkit.launch();
    try {
      await questionReplyScenario(browser, 1280);
      await questionReplyScenario(wk, 390);
    } finally { await wk.close(); wk = null; }
  });

  await step("t-0621 원문: 선택지 뒤 E·F 설명·그림과 A~D 선택지 그림을 보존하고 두 질문·4/3개 선택지를 답한다", async () => {
    if (!aaa("project", "list").projects.some((p) => p.name === "demo")) aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-icon-notes.md`, "선택지 뒤 설명 회귀 시험\n");
    const t = aaa("thread", "start", "demo", "--title", "단추 아이콘 원문", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-icon-notes.md`);
    await until("첫 아이콘 보고", () => aaa("thread", "show", t.id).report_hash);
    const original = fs.readFileSync(fileURLToPath(new URL("../../../crates/aaa-proto/tests/fixtures/t0621-questions.md", import.meta.url)), "utf8");
    for (const name of new Set([...original.matchAll(/library\/([a-z-]+\.png)/g)].map((m) => m[1]))) {
      // 실제 보고의 글은 그대로, 그림 bytes는 독립 실험실의 fixture다.
      fs.copyFileSync(`${root}/pic.png`, `${t.work_dir}/library/${name}`);
    }
    const body = "[결정 필요] 질문 2개: 단추 아이콘 원문\n\n" + original;
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    const v = await until("아이콘 원문 두 질문", () => { const v = aaa("thread", "show", t.id); return v.report_hash === hash && v.questions && v; });
    if (v.questions.format_warning || JSON.stringify(v.questions.items.map((q) => q.choices.length)) !== "[4,3]" || !v.questions.items[0].after?.includes("E·F안 그림:")) throw new Error(`아이콘 원문: ${JSON.stringify(v.questions)}`);
    await page.evaluate((id) => { location.hash = `question=${id}`; }, t.id);
    const card = page.getByTestId(`thread-ask-${t.id}`);
    await card.getByTestId("thread-ask-pick-1D").waitFor();
    await until("선택지와 E/F 그림", async () => (await pictures(page, `[data-testid="thread-ask-${t.id}"] .ask-choice-picture`)) === 7 && (await pictures(page, `[data-testid="thread-ask-${t.id}"] .ask-after`)) === 2);
    if ((await card.locator('[role="radiogroup"]').count()) !== 2 || (await card.locator('[role="radio"]').count()) !== 7 || (await card.getByTestId("thread-ask-pick-1E").count()) !== 0) throw new Error("E/F를 선택지로 위조하거나 질문을 빠뜨렸다");
    await shot(page, "53-t0621-icon-question");
    await card.locator(".ask-after").scrollIntoViewIfNeeded();
    await shot(page, "53-t0621-after-options");
    await card.getByTestId("thread-ask-pick-2B").click();
    await card.getByTestId("thread-ask-text-1").fill("E: TowerControl\n원래 픽셀 밀도 유지");
    await card.getByTestId("thread-ask-text-2").fill("선택한 B에 색만 더한다");
    await card.getByTestId("thread-ask-send").click();
    await waitOutput(t.pane, "1번 직접 쓴 답: E: TowerControl");
    await waitOutput(t.pane, "2번 직접 쓴 답(의견): 선택한 B에 색만 더한다");
    await until("답한 아이콘 카드", async () => (await page.getByTestId("ask-done-card").count()) === 1 && (await pictures(page, '[data-testid="ask-done-card"] .ask-after')) === 2);
    const answered = aaa("thread", "show", t.id).questions.answer;
    if (answered.picks[0] !== null || answered.picks[1] !== 1 || answered.text !== null
      || JSON.stringify(answered.comments) !== JSON.stringify(["E: TowerControl\n원래 픽셀 밀도 유지", "선택한 B에 색만 더한다"])) throw new Error(`E 직접 답: ${JSON.stringify(answered)}`);
    if (!(await page.getByTestId("ask-done-text-1").innerText()).includes("원래 픽셀 밀도 유지")
      || !(await page.getByTestId("ask-done-text-2").innerText()).includes("선택한 B에 색만 더한다")) throw new Error("질문별 직접 답이 답한 카드에 없다");
    const followups = fs.readFileSync(`${root}/projects/demo/threads/${t.id}.task.md`, "utf8");
    if (!followups.includes("1번 직접 쓴 답: E: TowerControl\n원래 픽셀 밀도 유지")
      || !followups.includes("2번 직접 쓴 답(의견): 선택한 B에 색만 더한다")) throw new Error("질문별 원문이 Follow-ups에 없다");
    const notice = aaa("inbox", "list", "demo", "--all", "--peek").items.find(i => i.event === "question answered" && i.subject === t.id);
    if (!notice?.summary.includes("1직접 2B+의견") || !notice.summary.includes("1 직접: E: TowerControl")
      || !notice.summary.includes("2B + 의견: 선택한 B에 색만 더한다") || !notice.body.includes("원래 픽셀 밀도 유지")
      || !notice.body.includes("2번 직접 쓴 답(의견): 선택한 B에 색만 더한다")) throw new Error(`질문별 inbox 원문: ${JSON.stringify(notice)}`);
    await shot(page, "53-t0621-answered-notes");
    aaa("thread", "resolve", t.id);
  });

  await step("직접 답 여러 줄·그림(t-1635): Enter는 줄바꿈, 조합 중 ⌘Enter는 보내지 않고, 붙인 그림은 thread 작업 폴더 사본으로 간다", async () => {
    if (!aaa("project", "list").projects.some((p) => p.name === "demo")) aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-answer-lines.md`, "여러 줄 답 시험\n");
    const t = aaa("thread", "start", "demo", "--title", "여러 줄 답", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-answer-lines.md`);
    await until("첫 여러 줄 보고", () => aaa("thread", "show", t.id).report_hash);
    const body = "[결정 필요] 질문 1개: 궤적\n\n## Questions\n### 1. 궤적을 이대로 둘까\n- A 그대로 (추천)\n- B 고친다\n";
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    await until("한 질문 카드", () => { const v = aaa("thread", "show", t.id); return v.report_hash === hash && v.questions && v; });
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.evaluate((id) => { location.hash = `question=${id}`; }, t.id);
    const card = page.getByTestId(`thread-ask-${t.id}`);
    const text = card.getByTestId("thread-ask-text");
    await text.waitFor();
    await card.getByTestId("thread-ask-pick-1B").click();
    await text.click();
    await page.keyboard.type("올라갈 때 한 번 꺾인다");
    await page.keyboard.press("Enter");
    await page.keyboard.type("박쥐 간격에 offset");
    if ((await text.inputValue()) !== "올라갈 때 한 번 꺾인다\n박쥐 간격에 offset" || (await page.getByTestId("ask-done-card").count()) !== 0) throw new Error(`Enter가 줄바꿈이 아니다: ${JSON.stringify(await text.inputValue())}`);
    // 한글 조합 중의 ⌘/Ctrl+Enter(keyCode 229)는 조합을 끝내는 키다: 보내지 않는다
    await text.evaluate((el) => {
      for (const init of [{ isComposing: true, keyCode: 13 }, { isComposing: false, keyCode: 229 }]) {
        el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", metaKey: true, ctrlKey: true, bubbles: true, cancelable: true, ...init }));
      }
    });
    // 붙여 넣은 그림은 프로젝트 uploads/에 내용 hash 이름으로 올라가 칸 아래 썸네일이 된다(입력칸 글에는 섞이지 않는다)
    const png = fs.readFileSync(`${root}/pic.png`);
    const name = `${createHash("sha256").update(png).digest("hex").slice(0, 16)}.png`;
    await text.evaluate((el, b64) => {
      const dt = new DataTransfer();
      dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "", { type: "image/png" }));
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    }, png.toString("base64"));
    await until("붙인 그림 썸네일", async () => (await pictures(page, `[data-testid="thread-ask-${t.id}"] [data-testid="thread-ask-text-field"]`)) === 1);
    if ((await text.inputValue()).includes("uploads/") || (await page.getByTestId("ask-done-card").count()) !== 0) throw new Error("그림 줄이 입력칸에 보이거나 조합 중에 보냈다");
    await shot(page, "53-answer-lines-desktop");
    await page.setViewportSize({ width: 390, height: 844 });
    await text.scrollIntoViewIfNeeded();
    await shot(page, "53-answer-lines-phone");
    await page.setViewportSize({ width: 1920, height: 1080 });
    await text.press("ControlOrMeta+Enter");
    await waitOutput(t.pane, "직접 쓴 답: 올라갈 때 한 번 꺾인다");
    await waitOutput(t.pane, "박쥐 간격에 offset");
    const copy = `${t.work_dir}/attachments/${name}`;
    if (!fs.existsSync(copy) || !fs.readFileSync(copy).equals(png)) throw new Error(`thread 작업 폴더에 그림 사본이 없다: ${copy}`);
    const answered = aaa("thread", "show", t.id).questions.answer;
    if (answered.text !== `올라갈 때 한 번 꺾인다\n박쥐 간격에 offset\n![그림](uploads/${name})`) throw new Error(`기록한 직접 답: ${JSON.stringify(answered)}`);
    // Follow-ups는 thread가 받은 뒤 적힌다: 그 기록(파일 내용)을 기다린다
    const sentLine = `직접 쓴 답: 올라갈 때 한 번 꺾인다\n박쥐 간격에 offset\n![그림](${copy})`;
    await until("thread에 간 글의 사본 경로", () => fs.readFileSync(`${root}/projects/demo/threads/${t.id}.task.md`, "utf8").includes(sentLine));
    const notice = aaa("inbox", "list", "demo", "--all", "--peek").items.find((i) => i.event === "question answered" && i.subject === t.id);
    if (!notice?.body.includes(`박쥐 간격에 offset\n![그림](${root}/projects/demo/uploads/${name})`)) throw new Error(`coordinator inbox 원문: ${JSON.stringify(notice)}`);
    await page.getByTestId("ask-done-card").waitFor();
    // 답 그림 썸네일은 lazy다: 보이게 한 뒤 decode 완료(사건)를 기다린다
    const doneImage = page.getByTestId("ask-done-text-images").locator("img");
    await doneImage.waitFor();
    await doneImage.scrollIntoViewIfNeeded();
    await doneImage.evaluate((i) => i.decode());
    if ((await pictures(page, '[data-testid="ask-done-text-images"]')) !== 1) throw new Error("답한 카드에 붙인 그림이 보이지 않는다");
    if ((await page.getByTestId("ask-done-text").innerText()) !== "직접 쓴 답 올라갈 때 한 번 꺾인다\n박쥐 간격에 offset") throw new Error(`답한 카드 글: ${JSON.stringify(await page.getByTestId("ask-done-text").innerText())}`);
    await shot(page, "53-answer-lines-done");
    await page.setViewportSize({ width: 1280, height: 800 });
    aaa("thread", "resolve", t.id);
  });

  await step("근거가 Questions 위에만: thread 카드는 근거 경고·보고서 열기 r-ID(PC·휴대폰 폭), thread pane에 고칠 방법, post는 거절", async () => {
    fs.writeFileSync(`${root}/task-evidence.md`, "근거 위치 시험\n");
    const t = aaa("thread", "start", "demo", "--title", "근거 위치", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-evidence.md`);
    await until("첫 근거 보고", () => aaa("thread", "show", t.id).report_hash);
    for (const name of ["above-a.png", "above-b.png"]) fs.copyFileSync(`${root}/pic.png`, `${t.work_dir}/library/${name}`);
    // fake thread의 HTML 보고서를 빼야 「보고서 열기」가 그 판의 report.md 본문(그림 포함)을 보인다.
    fs.rmSync(`${t.work_dir}/library/index.html`, { force: true });
    const above = "시안 두 장을 비교했다.\n\n![A안](library/above-a.png)\n![B안](library/above-b.png)\n\n";
    const questions = "## Questions\n### 1. 어느 안으로 할까\n- A 첫 안 (추천) — 밝다\n- B 둘째 안 — 어둡다\n";
    const body = `[결정 필요] 질문 1개: 근거 위치\n\n${above}${questions}`;
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    const v = await until("근거 경고와 r-ID", () => { const v = aaa("thread", "show", t.id); return v.report_hash === hash && v.questions && v.report_id && v; });
    if (v.questions.format_warning !== "근거 확인 필요: 그림 2개가 질문 칸 밖에만 있다. 위 「보고서 열기」에서 본다.") throw new Error(`근거 경고: ${JSON.stringify(v.questions)}`);
    await waitOutput(t.pane, "[aaa 질문 근거 확인]");
    await waitOutput(t.pane, "그림 2개(library/above-a.png, library/above-b.png)");
    await page.evaluate((id) => { location.hash = `question=${id}`; }, v.questions.items[0].id);
    const card = page.getByTestId(`thread-ask-${t.id}`);
    await card.getByTestId("thread-ask-pick-1A").waitFor();
    for (const [label, size] of [["desktop", { width: 1280, height: 800 }], ["phone", { width: 390, height: 844 }]]) {
      await page.setViewportSize(size);
      await page.getByTestId("asks-scroll").evaluate((el) => { el.scrollTop = 0; });
      const open = card.getByTestId("question-report-open");
      if ((await open.count()) !== 1 || !(await open.textContent()).includes(v.report_id)) throw new Error(`보고서 열기: ${await open.allTextContents()}`);
      if ((await card.getByTestId("question-format-warning").count()) !== 0) throw new Error("근거 경고를 형식 오류로 보였다");
      const warning = card.getByTestId("question-evidence-warning");
      if ((await warning.textContent()) !== v.questions.format_warning) throw new Error(`카드 근거 경고: ${await warning.textContent()}`);
      // 첫 화면에서 스크롤 없이 경고와 보고서 열기가 보인다.
      for (const [name, el] of [["보고서 열기", open], ["근거 경고", warning]]) {
        const box = await el.boundingBox();
        if (!box || box.y < 0 || box.y + box.height > size.height || box.x + box.width > size.width) throw new Error(`${label} ${name} 위치: ${JSON.stringify(box)}`);
      }
      await shot(page, `53-evidence-${label}`);
      await open.click();
      // 보고 본문 그림은 lazy라 하나씩 보이게 한 뒤 decode 완료(사건)를 기다린다.
      const preview = card.getByTestId("question-report-preview");
      await preview.waitFor();
      await preview.scrollIntoViewIfNeeded();
      await shot(page, `53-evidence-${label}-opened`);
      await preview.locator("img").nth(1).waitFor();
      for (const img of await preview.locator("img").all()) { await img.scrollIntoViewIfNeeded(); await img.evaluate((i) => i.decode()); }
      if ((await pictures(page, '[data-testid="question-report-preview"]')) !== 2) throw new Error("보고서 미리보기 그림 2장이 아니다");
      await shot(page, `53-evidence-${label}-report`);
      await open.click();
    }
    await page.setViewportSize({ width: 1280, height: 800 });
    const notice = aaa("inbox", "list", "demo", "--all", "--peek").items.filter((i) => i.event === "질문 근거 위치 확인 필요" && i.subject === t.id);
    if (notice.length !== 1 || !notice[0].body.includes("`### N.` 아래(선택지 위)로 옮긴다")) throw new Error(`coordinator 근거 알림: ${JSON.stringify(notice)}`);
    // 그 판 library에 없는 파일을 가리키면(이전 판 파일 등) 카드에 그림 대신 「표시할 수 없음」만 남는다: 빠진 경로를 알린다.
    const gone = "[결정 필요] 질문 1개: 빠진 그림\n\n## Questions\n### 1. 어느 안으로 할까\n![지난 판](library/map-colors/single-day.png)\n\n- A 첫 안 (추천)\n- B 둘째 안\n";
    fs.writeFileSync(`${t.work_dir}/report.md`, gone);
    const goneHash = createHash("sha256").update(gone).digest("hex");
    const missing = await until("빠진 파일 경고", () => { const v = aaa("thread", "show", t.id); return v.report_hash === goneHash && v.questions?.format_warning && v; });
    if (missing.questions.format_warning !== "파일 확인 필요: 본문이 가리키는 library 파일 1개가 이 보고 판에 없다(library/map-colors/single-day.png).") throw new Error(`빠진 파일 경고: ${missing.questions.format_warning}`);
    await waitOutput(t.pane, "[aaa 보고 파일 확인]");
    await waitOutput(t.pane, "library 파일 1개가 이 보고 판의 host 사본에 없다(library/map-colors/single-day.png)");
    await page.evaluate((id) => { location.hash = `question=${id}`; }, missing.questions.items[0].id);
    const fileWarning = card.getByTestId("question-file-warning");
    await fileWarning.waitFor();
    if ((await card.getByTestId("question-format-warning").count()) !== 0) throw new Error("빠진 파일을 형식 오류로 보였다");
    await shot(page, "53-missing-file-desktop");
    // 파일을 두고 보고를 고쳐 쓰면 경고가 사라지고 그림이 보인다.
    fs.mkdirSync(`${t.work_dir}/library/map-colors`, { recursive: true });
    fs.copyFileSync(`${root}/pic.png`, `${t.work_dir}/library/map-colors/single-day.png`);
    const fixed = gone.replace("빠진 그림", "그림 보충");
    fs.writeFileSync(`${t.work_dir}/report.md`, fixed);
    const fixedHash = createHash("sha256").update(fixed).digest("hex");
    const ok = await until("고친 보고", () => { const v = aaa("thread", "show", t.id); return v.report_hash === fixedHash && v.questions && v; });
    if (ok.questions.format_warning) throw new Error(`고친 뒤에도 경고: ${ok.questions.format_warning}`);
    await page.evaluate((id) => { location.hash = `question=${id}`; }, ok.questions.items[0].id);
    await until("질문 칸 그림", async () => (await pictures(page, `[data-testid="thread-ask-${t.id}"] .thread-ask-q`)) === 1);
    if ((await card.getByTestId("question-file-warning").count()) !== 0) throw new Error("고친 카드에 빠진 파일 경고가 남았다");
    // post는 프로젝트 library에서 읽는다. 올린 자료만 쓴다.
    const preview = `${root}/evidence-preview`;
    fs.mkdirSync(preview, { recursive: true });
    for (const name of ["above-a.png", "above-b.png"]) fs.copyFileSync(`${root}/pic.png`, `${preview}/${name}`);
    aaa("library", "publish", preview, "--name", "preview");
    // 같은 글을 사용자 확인 post로 보내면 카드가 비므로 받지 않는다. 고칠 곳을 돌려준다.
    const file = `${root}/evidence-post.md`;
    fs.writeFileSync(file, above.replaceAll("library/", "library/preview/") + questions);
    let refused;
    try { aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "근거 위치 post", "--body-file", file); } catch (e) { refused = e.message; }
    if (!refused?.includes("끝 코드 1") || !refused.includes("그림 2개(library/preview/above-a.png") || !refused.includes("`### N.` 아래(선택지 위)로 옮긴다")) throw new Error(`post 거절: ${refused}`);
    fs.writeFileSync(file, questions.replace("- A 첫 안", "![없음](library/preview/gone.png)\n\n- A 첫 안"));
    let absent;
    try { aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "빠진 파일 post", "--body-file", file); } catch (e) { absent = e.message; }
    if (!absent?.includes("끝 코드 1") || !absent.includes("library 파일 1개가 프로젝트 library에 없다(library/preview/gone.png)")) throw new Error(`빠진 파일 post 거절: ${absent}`);
    fs.writeFileSync(file, questions.replace("- A 첫 안", above.replaceAll("library/", "library/preview/") + "- A 첫 안"));
    aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "근거 위치 post", "--body-file", file);
    aaa("thread", "resolve", t.id);
  });

}
