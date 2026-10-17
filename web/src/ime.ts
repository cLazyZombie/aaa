// 웹 터미널 한글 IME 보정(§8.32). xterm 6.0.0 CompositionHelper 내부 두 곳을 이 인스턴스에서만 바꾼다.
//
// 결함: 조합 밖에서 keydown(229)이 오면 xterm은 setTimeout(0) 뒤 textarea에 늘어난 글을 「조합 아닌 글」로 보낸다.
// 브라우저가 첫 자모를 textarea에 넣은 뒤 다음 task에 compositionstart를 보내면(keydown과 조합 사건이 따로 온다)
// 첫 자모(ㅅ)가 먼저 나가고, 조합 시작 위치가 그 자모 뒤로 잡혀 완성된 음절(상)은 잘린다. 「상태」가 「ㅅㅌ」이 된다.
//
// 보정: keydown(229) 뒤 늘어난 글은 keyup(늦어도 200ms)까지 미룬다. 그 사이 조합이 시작되면 보내지 않고,
// 조합 시작 위치를 keydown 때의 끝으로 되돌린다. 조합이 없으면 xterm과 같은 규칙으로 보낸다(숫자·문장부호).
import type { Terminal } from "@xterm/xterm";

const SETTLE_MS = 200;

interface Helper {
  _textarea: HTMLTextAreaElement;
  _isComposing: boolean;
  _dataAlreadySent: string;
  _compositionPosition: { start: number; end: number };
  _coreService: { triggerDataEvent(data: string, wasUserInput?: boolean): void };
  _handleAnyTextareaChanges(): void;
  compositionstart(): void;
}

/** 보정을 건다. xterm 내부가 예상과 다르면(판이 바뀜) 걸지 않고 false. */
export function fixImeComposition(term: Terminal): boolean {
  const h = (term as unknown as { _core?: { _compositionHelper?: Helper } })._core?._compositionHelper;
  if (
    !h ||
    typeof h._handleAnyTextareaChanges !== "function" ||
    typeof h.compositionstart !== "function" ||
    !h._compositionPosition ||
    !h._coreService ||
    !(h._textarea instanceof HTMLTextAreaElement)
  )
    return false;
  const ta = h._textarea;
  let pending: { old: string; timer: number } | null = null;

  const flush = () => {
    if (!pending) return;
    const { old, timer } = pending;
    pending = null;
    window.clearTimeout(timer);
    ta.removeEventListener("keyup", flush);
    if (h._isComposing) return;
    // 아래는 xterm 6.0.0 _handleAnyTextareaChanges의 판정 그대로다
    const now = ta.value;
    const diff = now.replace(old, "");
    h._dataAlreadySent = diff;
    if (now.length > old.length) h._coreService.triggerDataEvent(diff, true);
    else if (now.length < old.length) h._coreService.triggerDataEvent("\x7f", true);
    else if (now !== old) h._coreService.triggerDataEvent(now, true);
  };

  h._handleAnyTextareaChanges = () => {
    if (pending) flush();
    pending = { old: ta.value, timer: window.setTimeout(flush, SETTLE_MS) };
    ta.addEventListener("keyup", flush);
  };

  const start = h.compositionstart.bind(h);
  h.compositionstart = () => {
    start();
    if (!pending) return;
    // 조합이 시작됐다: 미뤄 둔 첫 자모는 조합의 일부다. 보내지 않고, 조합 시작을 keydown 때 끝으로 둔다
    const { old, timer } = pending;
    pending = null;
    window.clearTimeout(timer);
    ta.removeEventListener("keyup", flush);
    h._dataAlreadySent = "";
    h._compositionPosition.start = old.length;
  };
  return true;
}
