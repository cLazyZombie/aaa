import type { Terminal } from "@xterm/xterm";

/** xterm 입력 controller의 읽기 전용 상태. 앱은 DOM 사건으로 이 상태를 다시 추정하지 않는다. */
export interface CompositionState {
  active: boolean;
  pending: boolean;
  sending: boolean;
  text: string;
  row: number;
  col: number;
  visible: boolean;
}

export interface TerminalComposition {
  readonly state: CompositionState;
  observeState(listener: () => void): { dispose(): void };
  setRenderer(render: (state: CompositionState) => void): { dispose(): void };
}

/** 5.5.0 bridge의 한 경계. 버전 변경 때 조용히 별도 입력 경로로 빠지지 않는다. */
export function terminalComposition(term: Terminal): TerminalComposition {
  const helper = (term as unknown as { _core: { _compositionHelper?: TerminalComposition } })._core._compositionHelper;
  if (!helper?.observeState || !helper.setRenderer) throw new Error("xterm IME controller 연결이 없습니다");
  return helper;
}
