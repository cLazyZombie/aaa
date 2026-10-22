// 두 비동기 작업의 거부를 바로 처리한다. click 실패 뒤 context가 닫혀도 원래 오류를 보존한다.
export async function clickForPopup(page, link) {
  const [tab] = await Promise.all([page.waitForEvent("popup"), link.click()]);
  return tab;
}
