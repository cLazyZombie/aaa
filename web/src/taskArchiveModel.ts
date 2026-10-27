// 완료 목록의 정렬 선택·날짜 묶음·시각 글(TaskArchiveView.tsx). 정렬은 host가 페이지를 자르기 전에 하고, 화면은 받은 차례대로
// 날짜 머리만 끼운다. 모두 내림차순이다(최근·큰 번호가 위): Things Logbook·Gitea `recentclose`처럼 완료 기록은 최근 것을 찾는다.
import { logIgnoredFailure } from "./errorDiagnostic.ts";
import type { ArchivedTask, TaskArchiveSort } from "./gen/proto";

export const ARCHIVE_SORTS: readonly { key: TaskArchiveSort; label: string; hint: string }[] = [
  { key: "completed", label: "끝난 날짜", hint: "최근 끝난 일이 위" },
  { key: "started", label: "시작 날짜", hint: "최근 시작한 일이 위" },
  { key: "number", label: "번호", hint: "큰 번호가 위" },
];

/** 기기마다 기억한다(사용자 10-26 「고른 정렬은 기기에 기억」). 저장이 막히면 기본 정렬이다 */
const SORT_KEY = "aaa:tasks:completed-sort";
export function readArchiveSort(): TaskArchiveSort {
  try {
    const v = localStorage.getItem(SORT_KEY);
    return ARCHIVE_SORTS.some((s) => s.key === v) ? (v as TaskArchiveSort) : "completed";
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "taskArchiveModel.ts:18");
    return "completed";
  }
}
export function writeArchiveSort(sort: TaskArchiveSort): void {
  try {
    localStorage.setItem(SORT_KEY, sort);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "taskArchiveModel.ts:25");
    // 저장이 막혀도 이 화면 동안은 고른 정렬을 쓴다
  }
}

/** 정렬 기준의 시각. 번호순은 끝난 시각을 보인다 */
export function archiveTime(a: ArchivedTask, sort: TaskArchiveSort): number | null {
  return sort === "started" ? (a.started_ms ?? null) : a.completed_ms;
}

const pad = (n: number) => String(n).padStart(2, "0");
const WEEKDAY = ["일", "월", "화", "수", "목", "금", "토"];
const dayStart = (ms: number) => { const d = new Date(ms); return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime(); };

/** 날짜 묶음 머리: 오늘·어제·10월 24일 금요일, 다른 해는 연도를 붙인다 */
export function dayLabel(ms: number, now: number): string {
  const today = dayStart(now), day = dayStart(ms);
  const d = new Date(ms);
  const date = `${d.getMonth() + 1}월 ${d.getDate()}일 ${WEEKDAY[d.getDay()]}요일`;
  if (day === today) return `오늘 · ${date}`;
  // 서머타임 날은 24시간이 아니다: 하루 앞의 날짜 시작과 비교한다
  if (day === dayStart(today - 12 * 3600000)) return `어제 · ${date}`;
  return d.getFullYear() === new Date(now).getFullYear() ? date : `${d.getFullYear()}년 ${date}`;
}

/** 행 끝 시각: 날짜로 묶은 목록은 시:분, 번호순은 날짜까지. 24시간제로 브라우저마다 같게 쓴다 */
export function rowTime(ms: number, withDate: boolean, now: number): string {
  const d = new Date(ms);
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (!withDate) return hm;
  const year = d.getFullYear() === new Date(now).getFullYear() ? "" : `${d.getFullYear()}.`;
  return `${year}${d.getMonth() + 1}.${d.getDate()} ${hm}`;
}

/** 상세의 전체 시각 */
export function fullTime(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 시작부터 끝까지 걸린 시간(분 단위로 줄인다) */
export function spanText(from: number, to: number): string {
  const min = Math.max(0, Math.round((to - from) / 60000));
  if (min < 60) return `${min}분`;
  const h = Math.floor(min / 60), m = min % 60;
  if (h < 24) return m ? `${h}시간 ${m}분` : `${h}시간`;
  const d = Math.floor(h / 24), rh = h % 24;
  return rh ? `${d}일 ${rh}시간` : `${d}일`;
}

export type ArchiveGroup = { key: string; label: string | null; entries: ArchivedTask[] };

/**
 * 받은 차례를 그대로 두고 같은 날짜끼리 묶는다. 번호순은 묶지 않는다. 시작 날짜순의 시작 기록 없는 일(thread 없이 체크로 끝남)은
 * host가 맨 끝에 두며 「시작 기록 없음」 묶음이다.
 */
export function archiveGroups(entries: readonly ArchivedTask[], sort: TaskArchiveSort, now: number): ArchiveGroup[] {
  if (sort === "number") return entries.length ? [{ key: "all", label: null, entries: [...entries] }] : [];
  const out: ArchiveGroup[] = [];
  for (const a of entries) {
    const at = archiveTime(a, sort);
    const key = at === null ? "none" : String(dayStart(at));
    const last = out.at(-1);
    if (last?.key === key) last.entries.push(a);
    else out.push({ key, label: at === null ? "시작 기록 없음" : dayLabel(at, now), entries: [a] });
  }
  return out;
}

/** 완료 근거를 짧은 칩 글로. 모르는 근거는 원문이다 */
export function reasonLabel(reason: string): string {
  return ({ "origin/main 병합": "main 병합", "보고 [완료]": "보고 완료", "완료 체크 [x]": "체크 완료" } as Record<string, string>)[reason] ?? reason;
}
