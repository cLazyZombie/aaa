import { inTime } from "./harness.mjs";

/** 원래 surface 움직임의 끝을 기다린 뒤 정상 클릭한다. */
async function clickAfterMotion(item, selector, description) {
  // 원래 Playwright 클릭의 30초 예산을 움직임 대기와 함께 쓴다.
  const deadline = performance.now() + 30_000;
  await inTime(item.evaluate(async (element, selector) => {
    const sheet = element.closest(selector);
    if (!sheet) return;
    const animations = sheet.closest(".modal-scrim").getAnimations({ subtree: true });
    await Promise.all(animations.map(animation => animation.finished.catch(() => null)));
  }, selector), 20_000, description);
  await item.click({ timeout: Math.max(1, deadline - performance.now()) });
}

/** popover도 같은 선택 경로를 쓴다. */
export const clickMenuItem = item => clickAfterMotion(item, ".row-sheet", "메뉴 시트의 원래 애니메이션 종료");

/** 확인창도 휴대폰에서는 같은 원래 시트 애니메이션으로 움직인다. */
export const clickDialogButton = item => clickAfterMotion(item, ".confirm-dialog", "확인창의 원래 애니메이션 종료");
