// 긴 목록: DOM 복제나 높이 강제 없이 마지막 행의 접근·선택을 검증한다.
import { panelRows } from "./panel-rows.mjs";
import assert from "node:assert/strict";
import { domIs, inTime } from "./harness.mjs";

/** scrollIntoView/click의 자동 스크롤 전에 칸의 경계와 직접 스크롤을 확인한다. */
export async function reachEnd(p, selector, rowSelector, wheel, afterEnd) {
  const list = p.locator(selector), last = list.locator(rowSelector).last();
  await last.waitFor();
  await panelRows(p, `${selector} ${rowSelector}`);
  const geometry = await list.evaluate((el, value) => {
    window.__panelScrollTrace.begin(value);
    const r = el.getBoundingClientRect();
    const geometry = { scrollTop: el.scrollTop, h: el.clientHeight, sh: el.scrollHeight, sw: el.scrollWidth, w: el.clientWidth, top: r.top, bottom: r.bottom, viewport: innerHeight, overflow: getComputedStyle(el).overflowY };
    window.__panelScrollTrace.mark("geometry", { geometry }, false);
    return geometry;
  }, { selector, rowSelector, wheel });
  assert.ok(geometry.h > 0 && geometry.sh > geometry.h + 100, `긴 목록의 제한된 높이: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.top >= 0 && geometry.bottom <= geometry.viewport + 1, `화면 안 목록: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.sw <= geometry.w + 1, `가로 넘침: ${selector}`);
  assert.match(geometry.overflow, /auto|scroll/);
  await list.evaluate((el) => { window.__panelScrollTrace.phase("reset"); el.scrollTop = 0; window.__panelScrollTrace.mark("reset-assigned", {}, false); });
  if (wheel) {
    const r = await list.boundingBox();
    await p.mouse.move(r.x + r.width / 2, r.y + r.height / 2);
    // scrollTop은 DOM mutation이 아니다. 다른 화면 갱신에 기대지 않고 scroll 사건을 기다린다.
    const moved = inTime(list.evaluate((el) => new Promise((resolve, reject) => {
      const finish = () => {
        if (el.scrollTop <= 100) return;
        clearTimeout(limit);
        el.removeEventListener("scroll", finish);
        resolve();
      };
      const limit = setTimeout(() => {
        el.removeEventListener("scroll", finish);
        reject(new Error("휠 뒤 목록이 움직이지 않았다"));
      }, 20_000);
      el.addEventListener("scroll", finish);
      window.__panelScrollTrace.phase("wheel");
      finish();
    })), 20_000, "휠의 scroll 사건");
    await p.mouse.wheel(0, 700);
    await moved;
  }
  // WebKit 휴대폰에서도 실제 overflow 컨테이너를 굴린다(합성 touchmove는 native 스크롤을 하지 않는다).
  await list.evaluate((el) => { window.__panelScrollTrace.phase("end-request"); const requested = el.scrollHeight; el.scrollTop = requested; window.__panelScrollTrace.mark("end-assigned", { requested }, false); });
  if (afterEnd) await afterEnd();
  await domIs(p, "목록 끝 도착", (s) => { const e = document.querySelector(s); return e.scrollTop > 100 && Math.abs(e.scrollHeight - e.clientHeight - e.scrollTop) <= 2; }, selector);
  await domIs(p, `마지막 행이 목록 안에 보이고 눌린다: ${selector}`, ({ selector, rowSelector }) => {
    // React 목록 교체와 locator 평가 사이에 분리된 옛 행을 잡지 않는다. 같은 DOM 평가에서 현재 행·부모를 함께 읽는다.
    const parentEl = document.querySelector(selector), el = parentEl?.querySelectorAll(rowSelector).item(parentEl.querySelectorAll(rowSelector).length - 1);
    if (!el || !parentEl) return false;
    const r = el.getBoundingClientRect(), parent = parentEl.getBoundingClientRect();
    const x = r.left + r.width / 2, y = Math.min(r.bottom, parent.bottom) - 3;
    return r.top >= parent.top - 1 && r.bottom <= parent.bottom + 1 && el.contains(document.elementFromPoint(x, y));
  }, { selector, rowSelector });
  if (await list.evaluate((el) => !!el.closest("#sidebar-column"))) {
    const fixed = await p.evaluate(() => [".list-head", ".list-foot"].every((s) => {
      const r = document.querySelector(`#sidebar-column ${s}`).getBoundingClientRect();
      return r.top >= 0 && r.bottom <= innerHeight + 1;
    }));
    assert.ok(fixed, "스크롤 뒤에도 패널 머리와 바닥은 화면 안");
  }
  await panelRows(p, `${selector} ${rowSelector}`);
  assert.equal(await p.evaluate(() => window.scrollY), 0, "앱 문서는 스크롤하지 않는다");
  return last;
}
