// 보고서 그림 누름 알림(plan §8.350). host가 `/v/<cap>/` HTML 탐색 응답 끝에만 붙인다. 이 문서와 같은 sandbox(opaque origin)
// 안에서 돌고, 그림을 누르면 이 문서의 그림 주소 목록과 누른 자리를 부모(앱)에게 알리기만 한다. 앱은 그 iframe에서 온, 그
// 보고서 `/v/<cap>/` 아래 주소만 받는다. 링크·단추 안 그림과 보고서가 제 script로 처리한 누름(preventDefault)은 건드리지 않는다.
(() => {
  if (window.parent === window) return;
  const own = (img) => !img.closest("a[href], button");
  const style = document.createElement("style");
  style.textContent = "img:not(a[href] img):not(button img){cursor:zoom-in}";
  (document.head || document.documentElement).append(style);
  addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const img = e.target instanceof Element ? e.target.closest("img") : null;
    if (!img || !own(img)) return;
    const list = Array.from(document.images).filter(own);
    const images = list.map((i) => ({ src: i.currentSrc || i.src, alt: i.alt }));
    parent.postMessage({ type: "aaa:report-image", at: list.indexOf(img), images }, location.origin);
  });
  // iOS(WebKit 터치)는 click을 듣는 요소에서만 누름을 click으로 만든다: window에 위임만 하면 그림 누름이 오지 않는다.
  // 그림마다(뒤에 생긴 그림도) 빈 listener를 단다. 같은 함수라 두 번 달리지 않는다
  const tappable = () => {};
  const mark = () => {
    for (const i of document.images) i.addEventListener("click", tappable);
  };
  mark();
  const hasImage = (n) => n instanceof Element && (n.matches("img") || n.querySelector("img") !== null);
  new MutationObserver((records) => {
    if (records.some((r) => Array.from(r.addedNodes).some(hasImage))) mark();
  }).observe(document.documentElement, { childList: true, subtree: true });
})();
