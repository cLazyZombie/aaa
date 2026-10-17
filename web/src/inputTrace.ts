// 실기 입력 사건 기록. iPad 화면 키보드 같은 OS 입력기의 사건 순서는 headless 브라우저로 만들 수 없다.
// herdr-web-ui scripts/terminal-ime-diagnostic.ts처럼 실기에서 실제 사건을 모으고, 그 순서를 웹 E2E 재생으로 옮긴다.
//
// 켜기: 목록 아래 「입력 기록」 단추, 또는 주소에 ?trace=input(끄기 ?trace=off). 켜져 있으면 터미널 helper textarea의
// 키·입력·조합·focus 사건과 그때의 textarea 값·선택, 화면 키보드 상태(visualViewport·data-keyboard·터미널 옮김),
// pane으로 보낸 글을 모은다. 「보내기」가 host 상태 폴더 diag/에 남긴다(web.rs diag_input). 기록은 이 기기에만 있다가
// 보낼 때만 host로 간다.

const KEY = "aaa.inputTrace";
/** 모으는 사건 수 상한(보내는 크기를 host 상한 1MiB 안에 둔다) */
const MAX = 3000;

type Ev = Record<string, unknown>;
const events: Ev[] = [];
const listeners = new Set<() => void>();
const t0 = performance.now();

function stored(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

let on = stored();
{
  const q = new URLSearchParams(location.search).get("trace");
  if (q === "input") setTrace(true);
  else if (q === "off") setTrace(false);
}

export function traceOn(): boolean {
  return on;
}

export function setTrace(next: boolean) {
  on = next;
  try {
    if (next) localStorage.setItem(KEY, "1");
    else localStorage.removeItem(KEY);
  } catch {
    // 저장이 막혀도 이 화면 동안은 켠 대로 둔다
  }
  if (!next) events.length = 0;
  listeners.forEach((f) => f());
}

export function traceCount(): number {
  return events.length;
}

export function onTraceChange(f: () => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

let textarea: HTMLTextAreaElement | null = null;
let scroller: HTMLElement | null = null;

/** 사건 하나와 그때의 상태 */
export function trace(type: string, extra: Ev = {}) {
  if (!on || events.length >= MAX) return;
  const vv = window.visualViewport;
  const ta = textarea;
  events.push({
    t: Math.round((performance.now() - t0) * 10) / 10,
    type,
    ...extra,
    value: ta ? ta.value.slice(-40) : null,
    len: ta?.value.length ?? null,
    sel: ta ? [ta.selectionStart, ta.selectionEnd] : null,
    focus: ta !== null && document.activeElement === ta,
    vv: vv ? [Math.round(vv.height), Math.round(vv.offsetTop), Math.round(vv.pageTop)] : null,
    scrollY: Math.round(window.scrollY),
    pan: scroller ? Math.round(scroller.scrollTop) : null,
    kb: document.documentElement.hasAttribute("data-keyboard"),
  });
  if (events.length % 50 === 0 || events.length === 1) listeners.forEach((f) => f());
}

const key = (e: KeyboardEvent): Ev => ({
  key: e.key,
  code: e.code,
  keyCode: e.keyCode,
  composing: e.isComposing,
  mods: `${e.ctrlKey ? "c" : ""}${e.altKey ? "a" : ""}${e.metaKey ? "m" : ""}${e.shiftKey ? "s" : ""}`,
  prevented: e.defaultPrevented,
});

/** 터미널 helper textarea와 그 스크롤 칸에 붙인다. 반환값은 떼기. */
export function attachTrace(ta: HTMLTextAreaElement, scroll: HTMLElement): () => void {
  textarea = ta;
  scroller = scroll;
  const offs: (() => void)[] = [];
  const listen = <T extends Event>(target: EventTarget, type: string, f: (e: T) => Ev, capture = true) => {
    const h = (e: Event) => trace(type, f(e as T));
    target.addEventListener(type, h, { capture, passive: true });
    offs.push(() => target.removeEventListener(type, h, { capture }));
  };
  for (const type of ["keydown", "keypress", "keyup"]) listen<KeyboardEvent>(ta, type, key);
  for (const type of ["beforeinput", "input"])
    listen<InputEvent>(ta, type, (e) => ({
      inputType: e.inputType,
      data: e.data,
      composing: e.isComposing,
    }));
  for (const type of ["compositionstart", "compositionupdate", "compositionend"])
    listen<CompositionEvent>(ta, type, (e) => ({ data: e.data }));
  for (const type of ["focus", "blur"]) listen(ta, type, () => ({}));
  listen(document, "selectionchange", () => ({}));
  listen(scroll, "scroll", () => ({}));
  listen(window, "scroll", () => ({}));
  const vv = window.visualViewport;
  if (vv) for (const type of ["resize", "scroll"]) listen(vv, type, () => ({ viewport: type }));
  return () => {
    offs.forEach((f) => f());
    if (textarea === ta) textarea = null;
    if (scroller === scroll) scroller = null;
  };
}

/** 모은 기록을 host에 남긴다. 보낸 뒤 비운다. */
export async function sendTrace(): Promise<string> {
  const body = {
    ua: navigator.userAgent,
    platform: navigator.platform,
    touch: navigator.maxTouchPoints,
    screen: [screen.width, screen.height, devicePixelRatio],
    standalone: window.matchMedia("(display-mode: standalone)").matches,
    events,
  };
  const res = await fetch("/api/v1/diag/input", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
  });
  const out = (await res.json()) as {
    ok?: { saved: string };
    err?: { message: string };
  };
  if (!out.ok) throw new Error(out.err?.message ?? `HTTP ${res.status}`);
  events.length = 0;
  listeners.forEach((f) => f());
  return out.ok.saved;
}
