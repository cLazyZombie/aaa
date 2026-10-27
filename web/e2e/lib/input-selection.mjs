// setSelectionRange의 반환보다 native select 사건은 늦다. 다음 전환의 기록에 섞이지 않게
// 구독을 먼저 걸고 실제 선택 사건까지 마친다. WebKit은 같은 범위면 사건을 만들지 않는다.
export function selectInputAndWait(el, [start, end = start, direction = "forward"]) {
  el.focus();
  if (el.selectionStart === start && el.selectionEnd === end && el.selectionDirection === direction) return;
  return new Promise((resolve, reject) => {
    const selected = () => { clearTimeout(timer); resolve(); };
    const timer = setTimeout(() => {
      el.removeEventListener("select", selected);
      reject(new Error("5000ms 안에 입력 선택의 native select 사건이 오지 않았다"));
    }, 5000);
    el.addEventListener("select", selected, { once: true });
    el.setSelectionRange(start, end, direction);
  });
}
