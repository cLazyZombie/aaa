// t-0563: 실제 host·agent의 입력칸. OS 입력기 대신 조합 사건을 재생하고, native textarea로 줄 경계를 대조한다.
import fs from "node:fs";
import { aaa, AA, root, step, browser, webkit, pairPage, paneHash, shots, sentFrames, domIs } from "../lib/harness.mjs";
import { composerStability } from "../lib/composer-stability.mjs";
import { pairDevice } from "../lib/devices.mjs";

const selector = '[data-testid="chat-composer"] .composer-text';

// 제품의 textarea와 같은 CSS·폭인 독립 native textarea가 줄을 접는 자리다. canvas 글 폭 추정은 쓰지 않는다.
async function geometry(box) {
  return box.evaluate((el) => {
    const cs = getComputedStyle(el);
    const rect = (node) => {
      const r = node.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const surface = el.closest(".composer-surface");
    const lh = parseFloat(cs.lineHeight);
    const padding = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
    const probe = document.createElement("textarea");
    for (const key of cs) probe.style.setProperty(key, cs.getPropertyValue(key));
    Object.assign(probe.style, {
      position: "fixed", left: "-10000px", top: "0", visibility: "hidden",
      width: `${el.getBoundingClientRect().width}px`, height: "0", minHeight: "0", maxHeight: "none",
    });
    probe.tabIndex = -1;
    probe.setAttribute("aria-hidden", "true");
    document.body.append(probe);
    const rows = (value) => { probe.value = value; return Math.round((probe.scrollHeight - padding) / lh); };
    const expectedRows = rows(el.value);
    // 같은 한글 열에서 첫 줄이 넘는 글자 위치. 뒤에 더 쳐도 이 경계가 움직이면 안 된다.
    const sample = "가나다라마바사아자차카타파하".repeat(30);
    let low = 1, high = sample.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (rows(sample.slice(0, mid)) > 1) high = mid;
      else low = mid + 1;
    }
    probe.remove();
    return {
      text: rect(el), surface: rect(surface), send: rect(surface.querySelector(".composer-send")),
      attach: rect(surface.querySelector('[aria-label="그림 첨부"]')),
      gauge: rect(surface.querySelector('[data-testid="context-gauge"]')),
      gap: parseFloat(getComputedStyle(surface.querySelector(".composer-controls-right")).columnGap),
      lines: surface.dataset.lines, firstBreak: low, expectedRows,
      rows: Math.round((el.scrollHeight - padding) / lh), lh, padding,
      clipped: el.scrollWidth > el.clientWidth + 1,
      focused: document.activeElement === el, value: el.value,
    };
  });
}

// ResizeObserver는 그리기가 끝난 크기 사건이다. 창 크기 변경에 따른 제품 ResizeObserver·높이 효과까지 기다린다.
const resized = (box) => box.evaluate((el) => new Promise((done) => {
  const observer = new ResizeObserver(() => { observer.disconnect(); done(); });
  observer.observe(el);
}));

export default async function () {
  await step("줄바꿈 실험실 준비", async () => {
    const dir = `${root}/claude/projects/-lab-wrap`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/s1.jsonl`;
    // t-0622: 실제 usage가 있어야 입력 중 숨긴 게이지의 빈자리도 검사한다.
    fs.writeFileSync(transcript, [
      { type: "user", message: { role: "user", content: "입력 시험" } },
      { type: "assistant", requestedModel: "claude-haiku-4-5", message: {
        model: "claude-haiku-4-5", content: [{ type: "text", text: "입력 준비" }],
        usage: { input_tokens: 76000, output_tokens: 10 },
      } },
    ].map((line) => JSON.stringify(line) + "\n").join(""));
    const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "wrap-session", source: "startup", transcript_path: transcript });
    aaa("tab", "create", "--machine", "labA", "--label", "wrap", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA, hook);
  }, { always: true });

  await step("입력칸 숨긴 게이지 자리 회수와 줄바꿈 폭 고정: Chromium·WebKit 데스크톱·휴대폰, 한글 조합 중·확정·영문·붙여넣기·창 폭 변경", async () => {
    const wk = await webkit.launch();
    const failures = [];
    const log = [];
    try {
      for (const [name, engine, phone] of [["chromium-desktop", browser, false], ["webkit-desktop", wk, false], ["chromium-phone", browser, true], ["webkit-phone", wk, true]]) {
        const hash = paneHash("wrap");
        const { page: p } = phone
          ? await pairDevice(engine, aaa("web", "pair").code, "iphone", { hash })
          : await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash });
        if (!(await p.getByTestId("chat-composer").count())) await p.getByTestId("chat-toggle").click();
        const box = p.locator(selector);
        await box.waitFor();
        await domIs(p, "실제 컨텍스트 게이지", () => document.querySelector('[data-testid="context-gauge-value"]')?.textContent === "38%");
        await box.fill("");
        await box.focus();
        const empty = await geometry(box);
        await box.fill("가");
        const initial = await geometry(box);
        const reclaimed = empty.gauge.width > 0 ? empty.gauge.width + empty.gap : 0;
        if (Math.abs(initial.text.width - empty.text.width - reclaimed) > 0.5) failures.push(`${name}: 숨긴 게이지 폭·간격 ${reclaimed}px을 회수하지 않음`);
        if (initial.gauge.width !== 0 || Math.abs(initial.send.left - initial.text.right) > 0.5) failures.push(`${name}: 보내기 앞에 숨긴 게이지 빈자리가 남음`);
        if (reclaimed > 0 && initial.firstBreak <= empty.firstBreak) failures.push(`${name}: 넓어진 칸에서 첫 줄이 더 많은 한글을 담지 못함`);
        log.push({ device: name, state: "빈 입력칸", ...empty }, { device: name, state: "첫 입력", ...initial });
        const from = (await sentFrames(p)).length;
        const check = async (state, width = initial.text.width) => {
          const g = await geometry(box);
          log.push({ device: name, state, ...g });
          const bad = (why) => failures.push(`${name} ${state}: ${why}`);
          if (Math.abs(g.text.width - width) > 0.5) bad(`글 칸 폭 ${width} → ${g.text.width}`);
          if (width === initial.text.width && g.firstBreak !== initial.firstBreak) bad(`첫 줄 경계 ${initial.firstBreak} → ${g.firstBreak}`);
          if (g.send.left < g.text.right - 0.5 || g.attach.right > g.text.left + 0.5) bad("단추가 글 칸과 겹침");
          if (g.send.top >= g.text.bottom - 0.5 || g.attach.top >= g.text.bottom - 0.5) bad("단추만 아래 줄로 이동");
          if (g.rows !== g.expectedRows) bad(`실제 ${g.rows}줄 ≠ 같은 폭 native textarea ${g.expectedRows}줄`);
          if ((g.lines === "one") !== (g.expectedRows <= 1)) bad(`표지 ${g.lines} ≠ 실제 ${g.expectedRows}줄`);
          if (g.clipped || !g.focused) bad("가로 잘림 또는 focus 잃음");
          return g;
        };
        const capture = async (state) => shots && p.getByTestId("chat-composer").screenshot({ path: `${shots}/wrap-${name}-${state}.png`, animations: "disabled" });
        // 예전 폭이면 접힐 한글이 회수한 자리까지 한 줄로 들어간다.
        if (reclaimed > 0) {
          await box.fill("가나다라마바사아자차카타파하".repeat(30).slice(0, empty.firstBreak));
          const boundary = await check("이전 첫 줄 경계");
          if (boundary.rows !== 1) failures.push(`${name}: 이전 첫 줄 경계의 글이 회수한 폭에 들어가지 않음`);
          await capture("reclaimed-boundary");
        }
        // 한 음절의 자모가 변하고 확정되는 순서. 매 사건은 별도 브라우저 task이므로 React commit 뒤 크기도 검사한다.
        await box.fill("");
        const syllables = "가나다라마바사아자차카타파하".repeat(12);
        for (let i = 0; i < syllables.length; i++) {
          await box.evaluate((el) => el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true })));
          for (const data of ["ㄱ", syllables[i]]) {
            await box.evaluate((el, [base, data]) => {
              el.dispatchEvent(new CompositionEvent("compositionupdate", { data, bubbles: true }));
              el.setRangeText(data, 0, el.value.length, "end");
              el.setRangeText(base, 0, 0, "end");
              el.setSelectionRange(el.value.length, el.value.length);
              el.dispatchEvent(new InputEvent("input", { data, inputType: "insertCompositionText", isComposing: true, bubbles: true }));
            }, [syllables.slice(0, i), data]);
            await check(`조합-${i}-${data}`);
          }
          await box.evaluate((el, data) => el.dispatchEvent(new CompositionEvent("compositionend", { data, bubbles: true })), syllables[i]);
          if (i === initial.firstBreak - 2 || i === initial.firstBreak || i === initial.firstBreak + 8) await capture(`korean-${i}`);
        }
        await check("한글 확정");
        await capture("korean-end");
        await box.fill("");
        await box.pressSequentially("English words with spaces and a long token ".repeat(5));
        await check("영문");
        await box.pressSequentially("x".repeat(180));
        await check("긴 영문 토큰");
        // plain text paste의 제품 handler를 거친 뒤 브라우저 편집 명령으로 기본 삽입을 실행한다. 시스템 clipboard는 쓰지 않는다.
        await box.fill("");
        const pasted = "붙여넣은 첫 줄\n" + "긴 글과 공백 ".repeat(30);
        await box.evaluate((el, value) => {
          const data = new DataTransfer();
          data.setData("text/plain", value);
          if (el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }))) document.execCommand("insertText", false, value);
        }, pasted);
        if (await box.inputValue() !== pasted) failures.push(`${name}: 붙여넣기 값 불일치`);
        await check("붙여넣기");
        await capture("paste");
        const original = p.viewportSize();
        for (const size of [phone ? { width: 844, height: 390 } : { width: 1000, height: 800 }, original]) {
          await p.setViewportSize(size);
          await resized(box);
          const width = (await geometry(box)).text.width;
          await check(`창 폭 ${size.width}`, width);
          // 바뀐 창 폭에서도 긴 글과 짧은 글이 같은 폭이다.
          await box.fill("가");
          await check(`창 폭 ${size.width} 짧음`, width);
          await box.fill(pasted);
          await check(`창 폭 ${size.width} 다시 김`, width);
        }
        await box.fill("");
        const cleared = await check("지움", empty.text.width);
        if (cleared.firstBreak !== empty.firstBreak) failures.push(`${name}: 지운 뒤 빈 입력칸의 줄 경계 복원 실패`);
        if (Math.abs(cleared.surface.height - initial.surface.height) > 0.5) failures.push(`${name}: 지운 뒤 한 줄 높이 복원 실패`);
        if ((await sentFrames(p, from)).some((x) => ["input", "submit", "bin"].includes(x.t))) failures.push(`${name}: 편집 중 pane에 전송함`);
        await p.context().close();
      }
    } finally {
      if (shots) fs.writeFileSync(`${shots}/wrap-geometry.json`, JSON.stringify(log, null, 2));
      await wk.close();
    }
    aaa("pane", "kill", "wrap");
    if (failures.length) throw new Error(`${failures.length}건: ${failures.slice(0, 24).join("; ")}`);
  });
  await step("첨부 뒤 여러 줄 한글 조합의 높이·스크롤 안정: Chromium·WebKit 데스크톱·휴대폰", composerStability);
}
