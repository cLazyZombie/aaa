// 에이전트 패널의 저장소 폴더(사용자 결정 10-19 안 A, plan §8.260): thread가 일하는 저장소 이름으로 agent 줄을 묶는다.
// 이름은 host가 주는 `ThreadView.repo`(그 머신의 저장소 경로)의 마지막 폴더다. 머신마다 경로가 달라도(`/Volumes/…/rc`·`/Users/…/rc`)
// 이름이 같으면 한 폴더다. agent도 worktree를 `<work_root>/<저장소 마지막 폴더>/<project>-<thread>`에 만든다(aaa-agent threads.rs).

/** 저장소가 없는 agent(작업 폴더 thread·thread 아닌 agent·옛 host의 thread)의 폴더 */
const OTHER_FOLDER = "기타";

/** 경로의 마지막 폴더 이름(`/`·`\` 모두, 끝 구분자는 무시) */
function lastFolder(path: string): string | null {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : null;
}

/** thread가 일하는 저장소 이름 */
export function threadRepo(t: { repo?: string | null } | undefined): string | null {
  return t?.repo ? lastFolder(t.repo) : null;
}

export interface AgentFolder<R> {
  name: string;
  rows: R[];
}

/**
 * coordinator 줄은 폴더 밖 맨 위에, 나머지는 저장소 폴더로. 폴더는 이름순(자리가 활동에 따라 뛰지 않는다)이고 「기타」는 끝이다.
 * 폴더 안 차례는 받은 차례(설정 「에이전트 정렬」이 이미 정했다) 그대로다. 저장소 폴더가 하나도 없으면(모두 「기타」) `folders`는
 * 비고 줄은 `flat`에 받은 차례로 남는다: 묶을 것이 없는 목록은 폴더 머리 없이 지금처럼 그린다.
 */
export function agentFolders<R>(
  rows: readonly R[],
  repoOf: (r: R) => string | null,
  isCoordinator: (r: R) => boolean,
): { coordinators: R[]; folders: AgentFolder<R>[]; flat: R[] } {
  const coordinators: R[] = [];
  const byName = new Map<string, R[]>();
  for (const r of rows) {
    if (isCoordinator(r)) {
      coordinators.push(r);
      continue;
    }
    const name = repoOf(r) ?? OTHER_FOLDER;
    const list = byName.get(name);
    if (list) list.push(r);
    else byName.set(name, [r]);
  }
  const names = [...byName.keys()].filter((n) => n !== OTHER_FOLDER).sort((a, b) => a.localeCompare(b));
  if (names.length === 0) return { coordinators: [], folders: [], flat: [...rows] };
  if (byName.has(OTHER_FOLDER)) names.push(OTHER_FOLDER);
  return { coordinators, folders: names.map((name) => ({ name, rows: byName.get(name)! })), flat: [] };
}
