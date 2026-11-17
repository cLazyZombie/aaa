import type { DesignRepoView, FrontRequest, FrontResponse } from "./gen/proto";

/** host 목록은 바로 그리고 저장소마다 독립 요청으로 갱신한다. 재시도하지 않으며 전송 오류도 그 저장소에만 남긴다. */
export async function loadDesignRepos(
  call: (request: FrontRequest) => Promise<FrontResponse>,
  listed: (repos: DesignRepoView[]) => void,
  received: (repo: DesignRepoView) => void,
  active: () => boolean,
): Promise<void> {
  const snapshot = await call({ op: "design_tree", cached: true });
  if (!active()) return;
  if (snapshot.kind !== "design_tree") throw new Error("기획 목록 응답이 아니다");
  listed(snapshot.repos);
  await Promise.all(snapshot.repos.filter((repo) => repo.folder !== null).map(async (repo) => {
    try {
      const response = await call({ op: "design_tree", repo: repo.repo });
      if (!active()) return;
      const next = response.kind === "design_tree" ? response.repos.find((r) => r.repo === repo.repo) : undefined;
      if (!next) throw new Error("기획 저장소 응답이 없다");
      received(next);
    } catch (error) {
      if (active()) received({ ...repo, error: `기획을 읽지 못했다: ${error instanceof Error ? error.message : String(error)}` });
    }
  }));
}
