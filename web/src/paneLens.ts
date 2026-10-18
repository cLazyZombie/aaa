// pane이 열리는 보기: 대화(chat) 또는 터미널(herdr-web-ui `src/lib/paneLens.ts` `defaultLens`·`PaneLenses`와 App.tsx `storedView`,
// settings.ts `forgetPaneViews`). 손으로 고른 보기는 이 기기에 pane마다 남고 늘 이긴다. 고른 적이 없으면 설정 「패널 기본 화면」의
// 기본이고, pane에 agent가 있는지 안 뒤로는 이 화면 동안 그 보기로 둔다(agent가 뜨거나 끝나도 쓰던 보기를 바꾸지 않는다).
// 다른 곳: 참고의 열쇠 머리 `herdr-web-ui:view:`는 `aaa:view:`, 열쇠는 머신·pane ID(참고 `paneStorageId`).
import type { DefaultView } from "./settings";

export type PaneLens = "chat" | "terminal";

/**
 * 아무도 고르지 않은 pane의 보기: 설정의 기본(대화는 agent가 있어야 한다. 셸은 보일 대화가 없다), 자동이면 터치 화면의
 * agent pane만 대화. agent가 있는지 아직 모르면(null) 있는 것으로 센다.
 */
export function defaultLens(hasAgent: boolean | null, view: DefaultView, coarse: boolean): PaneLens {
  if (view === "chat") return hasAgent !== false ? "chat" : "terminal";
  if (view === "terminal") return "terminal";
  return hasAgent !== false && coarse ? "chat" : "terminal";
}

/**
 * 이 화면이 보인 pane마다의 기본 보기. agent가 있는지 안 뒤로 남는다: 셸에서 agent가 뜨거나 agent가 끝나도 쓰던 보기 그대로다
 * (다시 셈하면 쓰는 중인 터미널이 대화로 바뀌었다). 설정 기본이 바뀌면 새 표를 쓰고, 손으로 고른 보기는 따로 두어 늘 이긴다.
 */
export class PaneLenses {
  private readonly settled = new Map<string, PaneLens>();
  readonly view: DefaultView;
  // node 시험의 형 지우기는 생성자 매개변수 속성을 받지 않아 따로 적는다
  constructor(view: DefaultView) {
    this.view = view;
  }

  of(key: string, hasAgent: boolean | null, coarse: boolean): PaneLens {
    const settled = this.settled.get(key);
    if (settled !== undefined) return settled;
    const lens = defaultLens(hasAgent, this.view, coarse);
    if (hasAgent !== null) this.settled.set(key, lens);
    return lens;
  }

  /** 이 화면이 agent를 띄운 pane: 첫 상태가 무엇이라 하든 agent의 보기다 */
  startedAgent(key: string, coarse: boolean): void {
    this.settled.set(key, defaultLens(true, this.view, coarse));
  }
}

/** 이 화면의 표(참고 App은 설정 기본이 바뀌면 새 PaneLenses를 만든다) */
let lenses: PaneLenses | null = null;
export function lensesFor(view: DefaultView): PaneLenses {
  if (lenses === null || lenses.view !== view) lenses = new PaneLenses(view);
  return lenses;
}

export const PANE_VIEW_KEY_PREFIX = "aaa:view:";

/** 손으로 고른 보기(없으면 null) */
export function pickedLens(key: string): PaneLens | null {
  try {
    const stored = localStorage.getItem(`${PANE_VIEW_KEY_PREFIX}${key}`);
    return stored === "chat" || stored === "terminal" ? stored : null;
  } catch {
    return null;
  }
}

export function rememberLens(key: string, lens: PaneLens): void {
  try {
    localStorage.setItem(`${PANE_VIEW_KEY_PREFIX}${key}`, lens);
  } catch {
    // 저장이 막혀도 이 화면 동안은 고른 대로 둔다
  }
}

/** 이 기기에서 pane마다 고른 보기를 모두 잊는다: 모두 다시 기본 보기로 열린다 */
export function forgetPaneViews(given?: Pick<Storage, "length" | "key" | "removeItem">): number {
  const keys: string[] = [];
  try {
    // localStorage를 읽는 것부터 막힌 곳에서 던진다
    const storage = given ?? localStorage;
    for (let index = 0; index < storage.length; index++) {
      const key = storage.key(index);
      if (key?.startsWith(PANE_VIEW_KEY_PREFIX)) keys.push(key);
    }
    for (const key of keys) storage.removeItem(key);
  } catch {
    // 막힌 곳에는 잊을 것도 없다
  }
  return keys.length;
}
