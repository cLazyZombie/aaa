// 프로젝트 화면 머리의 「coordinator」(herdr-projects popup Threads `c` → `profile_picker("Start or focus a coordinator with")` →
// `open <p> --profile <고른 것>`, plan §8.180). 고르기 목록은 host 머신 profile 가운데 safety `coordinator_profiles`가 허용한
// 것이고(참고 `usable`) 프로젝트 기본 coordinator profile이 맨 위다. 고르면 host가 도는 coordinator를 알리거나 띄우고 그 pane을 연다.
// 목록과 허용·기본값은 단추를 누를 때 받는다: agent.toml·로그인한 harness·safety는 화면을 연 뒤에도 바뀐다.
// 거부(허용 목록·보관·없는 profile)는 host가 하고 그 글을 머리 아래 줄(onError)에 보인다.
import { useRef, useState } from "react";
import { Bot, LoaderCircle } from "lucide-react";
import { ApiError, call } from "./api";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { AgentMark } from "./herdr/AgentMark.tsx";

export function CoordinatorOpen({
  project,
  onPane,
  onError,
}: {
  project: string;
  onPane: (id: string) => void;
  onError: (message: string | null) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [items, setItems] = useState<RowMenuItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const fail = (e: unknown) => onError(e instanceof ApiError ? e.message : String(e));

  const open = async (profile: string) => {
    setBusy(true);
    onError(null);
    try {
      const r = await call({ op: "project_open", project, profile });
      if (r.kind === "project_opened") onPane(r.pane.id);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const toggle = async () => {
    if (busy) return;
    if (items) {
      setItems(null);
      return;
    }
    setBusy(true);
    onError(null);
    try {
      // 프로젝트 폴더는 host 머신에 있다: machine을 비우면 host가 로컬 agent에 묻는다
      const [r, ps] = await Promise.all([call({ op: "profiles", machine: null }), call({ op: "projects", all: true })]);
      const all = r.kind === "profiles" ? r.profiles : [];
      const view = ps.kind === "projects" ? ps.projects.find((p) => p.name === project) : undefined;
      const allowed = view?.coordinator_profiles;
      const usable = all.filter((p) => !allowed || allowed.includes(p.name));
      const first = view?.coordinator_profile ?? "";
      usable.sort((a, b) => Number(b.name === first) - Number(a.name === first));
      if (usable.length === 0) {
        onError(
          allowed
            ? `${project}의 coordinator에 허용된 profile이 host 머신에 없다(허용: ${allowed.join(", ") || "없음"}). 사람이 \`aaa profile allow coordinator <이름> --project ${project}\`로 정한다`
            : "host 머신에 profile이 없다. 사람이 `aaa profile add`로 더한다",
        );
        return;
      }
      setItems(
        usable.map((p) => ({
          id: p.name,
          label: p.name,
          hint: p.name === first ? "기본" : undefined,
          title: p.description || undefined,
          icon: Bot,
          glyph: p.harness === "custom" ? undefined : <AgentMark agent={p.harness} size={16} />,
          run: () => void open(p.name),
        })),
      );
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  // 바쁜 동안 disabled로 두지 않는다: 메뉴가 닫히며 돌려준 focus를 잃는다
  return (
    <>
      <button
        ref={button}
        type="button"
        className="btn project-coordinator"
        data-testid="project-coordinator"
        aria-label="coordinator 열기"
        aria-haspopup="menu"
        aria-expanded={items !== null}
        aria-busy={busy}
        aria-disabled={busy}
        title="도는 coordinator를 열거나, 없으면 띄운다"
        onClick={() => void toggle()}
      >
        {busy ? <LoaderCircle className="is-spinning" aria-hidden="true" /> : <Bot aria-hidden="true" />}
        <span className="project-coordinator-label">coordinator</span>
      </button>
      {items && button.current && (
        <RowMenu
          anchor={button.current}
          title="coordinator 열기"
          header={
            <span className="coordinator-menu-head">
              <span className="row-sheet-title">coordinator 열기</span>
              <span className="row-sheet-subtitle">도는 것이 있으면 그 pane을, 없으면 띄운다</span>
            </span>
          }
          items={items}
          onClose={() => setItems(null)}
        />
      )}
    </>
  );
}
