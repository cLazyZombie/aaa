/**
 * In-app alerts: a notice that drops from the top edge while the app is on screen.
 * A push to a device whose app is visible arrives silent (public/sw.js), and the tab
 * alerts stay quiet in a visible tab (lib/notifications.ts), so without this an agent
 * that waits on the user while the app is open says so nowhere but its sidebar row.
 *
 * App decides what is news (shared/notify-policy.ts, as the other alerts do) and calls
 * showDroplet(); components/Droplet.tsx is the one place that draws them.
 *
 * aaa: herdr-web-ui src/lib/droplet.ts를 옮겼다. 알림 대상은 머신·pane 대신 aaa 주소 hash(`pane=…`·`thread=…`)
 * 하나(`target`)다. pane 상태로 알릴지 정하는 함수(dropletAllows·seedStatuses·trackTurn·endedTurn)는
 * 턴 끝 알림(차이 목록 N3)과 함께 옮긴다(plan §8.70).
 */

export type DropletKind = "blocked" | "done" | "ended";

export interface DropletNotice {
  /** where a tap goes: the address hash of a pane or a thread */
  target: string;
  agent: string | null;
  /** the pane's name, as the sidebar shows it */
  title: string;
  /** the machine's name, shown beside what happened when there is more than one */
  machine: string | null;
  kind: DropletKind;
}

export interface QueuedDroplet extends DropletNotice {
  id: number;
}

/** The same notice for the same pane this soon again is one notice. */
export const DROPLET_REPEAT_MS = 400;

/** Only movement this recent counts as a flick: a finger that rested first has not slowed it. */
export const FLICK_WINDOW_MS = 100;

export interface PressSample {
  y: number;
  t: number;
}

/**
 * The vertical speed at release (px per ms, up is negative), over the samples of the last
 * FLICK_WINDOW_MS only. Measured over the whole press, a hold before a flick reads as slow.
 */
export function flickVelocity(samples: readonly PressSample[], y: number, now: number): number {
  const from = samples.find((sample) => now - sample.t <= FLICK_WINDOW_MS);
  return from ? (y - from.y) / Math.max(1, now - from.t) : 0;
}

type Listener = (notice: QueuedDroplet) => void;
const listeners = new Set<Listener>();
let nextId = 1;
const lastShown = new Map<string, number>();

/** Shows a notice in the open app; false when nothing draws them (no Droplet mounted) or it repeats one just shown. */
export function showDroplet(notice: DropletNotice, now = Date.now()): boolean {
  if (listeners.size === 0) return false;
  const key = `${notice.target}\n${notice.kind}`;
  const last = lastShown.get(key);
  if (last !== undefined && now - last < DROPLET_REPEAT_MS) return false;
  lastShown.set(key, now);
  const queued = { ...notice, id: nextId++ };
  for (const listener of listeners) listener(queued);
  return true;
}

export function onDroplet(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
