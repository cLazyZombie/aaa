// ConversationRefresh의 최소 간격만 건너뛴다. 실제 파일 변화·push·HTTP·DOM을
// 대신 만들지 않으며 native frame, timer, Date와 CPU 부하의 흐름은 유지한다.
// Playwright clock은 rAF callback 안의 performance.now도 멈추므로 E37의
// 실제 25ms CPU 점유와 함께 쓸 수 없다.
export function installChatRefreshClock() {
  const nativeNow = performance.now.bind(performance);
  let elapsed = 0;
  Object.defineProperty(performance, "now", { configurable: true, value: () => nativeNow() + elapsed });
  window.advanceChatRefreshClock = () => { elapsed += 2000; };
}

export const advanceChatRefresh = page => page.evaluate(() => window.advanceChatRefreshClock());
