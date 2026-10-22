// 첨부 + 여러 줄 + 한글 조합. 실제 textarea의 강제 layout과 그려진 frame을 모두 관측한다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { aaa, AA, root, browser, webkit, pairPage, waitOutput, domIs, shots, sentFrames } from "./harness.mjs";
import { pairDevice } from "./devices.mjs";

const selector = '[data-testid="chat-composer"] .composer-text';
const frames = (box) => box.evaluate(async () => {
  for (let i = 0; i < 2; i++) await new Promise(requestAnimationFrame);
});

export async function composerStability() {
  aaa("project", "create", "stable");
  const pane = aaa("project", "open", "stable", "--profile", "fake").pane.id;
  await waitOutput(pane, "fake>");
  const dir = `${root}/claude/projects/-lab-stable`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s1.jsonl`;
  fs.writeFileSync(transcript, Array.from({ length: 40 }, (_, i) => JSON.stringify({
    type: "assistant", message: { content: [{ type: "text", text: `앞선 답변 ${i}\n\n대화 기록입니다.` }] },
  }) + "\n").join(""));
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "stable-session", source: "startup", transcript_path: transcript });
  fs.writeFileSync(`${dir}/run.sh`, `'${AA}' hook --agent claude '${hook}'\nexec cat\n`);
  aaa("pane", "run", pane, `post-probe ${dir}/run.sh`);
  const wk = await webkit.launch();
  const log = [];
  const failures = [];
  try {
    for (const [name, engine, phone] of [["webkit-desktop", wk, false], ["chromium-desktop", browser, false], ["webkit-phone", wk, true], ["chromium-phone", browser, true]]) {
      const hash = `#pane=${encodeURIComponent(pane)}`;
      const { page: p } = phone
        ? await pairDevice(engine, aaa("web", "pair").code, "iphone", { hash })
        : await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash });
      try {
        if (!(await p.getByTestId("chat-composer").count())) await p.getByTestId("chat-toggle").click();
        const group = p.getByTestId("chat-composer");
        const box = p.locator(selector);
        await box.waitFor();
        await domIs(p, "첨부 가능", () => !document.querySelector('[aria-label="그림 첨부"]')?.disabled);
        await group.locator('input[type="file"]').setInputFiles({ name: "image.png", mimeType: "image/png", buffer: fs.readFileSync(`${root}/pic.png`) });
        await domIs(p, "그림 첨부 완료", () => document.querySelector(".composer-attachment")?.classList.contains("is-ready"));
        const mention = await box.inputValue();
        // 지금 폭에서 한 줄을 조금 넘는 한국어 문장: 첨부 경로·빈 줄 뒤에 자동 줄바꿈도 만든다.
        const paragraph = await box.evaluate((el) => {
          const cs = getComputedStyle(el);
          const width = el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
          const columns = Math.ceil(width / parseFloat(cs.fontSize));
          return "설정메뉴와선택한내용을보여줘".repeat(columns).slice(0, columns + 3);
        });
        // 실제 대화를 읽던 위치. 「맨 아래로」 단추가 떠 있는 상태에서 입력한다.
        await p.locator('[data-testid="chat"]').evaluate((el) => { el.scrollTop = Math.max(0, el.scrollHeight - el.clientHeight - 150); });
        await p.getByRole("button", { name: "맨 아래로", exact: true }).waitFor();
        const from = (await sentFrames(p)).length;
        for (const capped of [false, true]) {
          const prefix = mention + "\n\n" + (capped ? "여러 줄을 입력하는 중입니다.\n".repeat(25) : paragraph + "\n") + "입력: ";
          await box.fill(prefix);
          await box.focus();
          await box.evaluate((el) => { el.setSelectionRange(el.value.length, el.value.length); el.scrollTop = el.scrollHeight; });
          await frames(box);
          const initial = await box.evaluate((el) => ({ h: el.clientHeight, top: el.scrollTop, max: el.scrollHeight - el.clientHeight }));
          assert.equal(capped ? initial.max > 0 : initial.max <= 1, true, `${name}: ${capped ? "긴 글의 스크롤" : "짧은 글의 전체 표시"}`);
          await box.evaluate((el) => {
            const getHeight = Object.getOwnPropertyDescriptor(Element.prototype, "scrollHeight").get;
            const surface = el.closest(".composer-surface");
            const samples = [];
            const record = (what) => {
              const r = el.getBoundingClientRect();
              samples.push({ what, h: r.height, y: r.y, top: el.scrollTop, max: getHeight.call(el) - el.clientHeight,
                surface: surface.getBoundingClientRect().height, page: document.scrollingElement.scrollTop });
            };
            // 원래 getter가 강제하는 layout 직후 기록한다. 크기·스크롤은 바꾸지 않는다.
            Object.defineProperty(el, "scrollHeight", { configurable: true, get() { const value = getHeight.call(this); record("measure"); return value; } });
            const observer = new ResizeObserver(() => record("resize"));
            observer.observe(el); observer.observe(surface);
            const scrolled = () => record("scroll");
            el.addEventListener("scroll", scrolled);
            window.__composerStability = { samples, record, stop() { observer.disconnect(); el.removeEventListener("scroll", scrolled); delete el.scrollHeight; } };
            record("start");
          });
          // macOS 두벌식 자모 → 음절 → 확정. 각 사건 뒤 React commit과 두 frame을 관측한다.
          for (let i = 0; i < 6; i++) {
            await box.dispatchEvent("compositionstart", { data: "" });
            for (const data of ["ㄱ", "가"]) {
              await box.evaluate((el, [base, data]) => {
                el.dispatchEvent(new CompositionEvent("compositionupdate", { data, bubbles: true }));
                el.setRangeText(base + data, 0, el.value.length, "end");
                el.dispatchEvent(new InputEvent("input", { data, inputType: "insertCompositionText", isComposing: true, bubbles: true }));
              }, [prefix + "가".repeat(i), data]);
              await frames(box);
              await p.evaluate(() => window.__composerStability.record("frame"));
            }
            await box.dispatchEvent("compositionend", { data: "가" });
          }
          // trusted 키 입력도 같은 경로를 거친다. 줄이 접힐 만큼 긴 글자는 덧붙이지 않는다.
          await box.pressSequentially("abc");
          await frames(box);
          const samples = await p.evaluate(() => {
            const trace = window.__composerStability;
            trace.record("end"); trace.stop(); return trace.samples;
          });
          log.push({ name, capped, initial, samples });
          const start = samples[0];
          const bad = samples.filter((s) => Math.abs(s.h - initial.h) > 0.5 || Math.abs(s.top - initial.top) > 1
            || Math.abs(s.y - start.y) > 0.5 || Math.abs(s.surface - start.surface) > 0.5 || s.page !== 0);
          if (bad.length) failures.push(`${name} ${capped ? "긴 글" : "짧은 글"}: ${JSON.stringify(bad.slice(0, 3))}`);
          assert.equal(await box.inputValue(), prefix + "가".repeat(6) + "abc", `${name}: 조합 글 보존`);
          assert.equal(await box.evaluate((el) => document.activeElement === el), true, `${name}: focus 보존`);
          if (shots) await group.screenshot({ path: `${shots}/composer-stable-${name}-${capped ? "capped" : "short"}.png`, animations: "disabled" });
        }
        await box.fill("");
        await frames(box);
        const cleared = await box.evaluate((el) => ({ top: el.scrollTop, h: el.clientHeight, sh: el.scrollHeight, lh: parseFloat(getComputedStyle(el).lineHeight) }));
        assert.equal(cleared.top, 0, `${name}: 삭제 뒤 스크롤 초기화`);
        assert.ok(cleared.h < cleared.lh * 2 && cleared.sh <= cleared.h + 1, `${name}: 한 줄 높이 복원`);
        assert.equal((await sentFrames(p, from)).some((f) => ["input", "submit", "bin"].includes(f.t)), false, `${name}: 편집 중 전송 없음`);
      } finally {
        await p.evaluate(() => window.__composerStability?.stop());
        await p.context().close();
      }
    }
  } finally {
    if (shots) fs.writeFileSync(`${shots}/composer-stability.json`, JSON.stringify(log, null, 2));
    await wk.close();
  }
  assert.deepEqual(failures, [], "입력 중 높이·스크롤 왕복 없음");
}
