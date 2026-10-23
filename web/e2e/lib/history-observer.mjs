// Chromium renderer의 history.length는 이동 때 보정되는 낡은 값일 수 있다.
// 브라우저가 가진 실제 기록을 비교한다. URL 전체·pairing query는 보존하지 않는다.
export async function observeHistory(page) {
  const chromium = page.context().browser().browserType().name() === "chromium";
  const client = chromium ? await page.context().newCDPSession(page) : null;
  return {
    async snapshot() {
      if (!client) return { length: await page.evaluate(() => history.length) };
      const { currentIndex, entries } = await client.send("Page.getNavigationHistory");
      return {
        length: entries.length,
        currentIndex,
        entries: entries.map(({ id, url }) => {
          const location = new URL(url);
          return { id, hash: location.hash, settings: location.searchParams.get("settings") };
        }),
      };
    },
    async close() { await client?.detach(); },
  };
}

export function assertHistoryUnchanged(before, after) {
  if (JSON.stringify(before) !== JSON.stringify(after))
    throw new Error(`메뉴 토글·기록 순회가 기록을 바꿨다: 기대=${JSON.stringify(before)}, 실제=${JSON.stringify(after)}`);
}
