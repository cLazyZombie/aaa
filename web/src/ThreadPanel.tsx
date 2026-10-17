import { useEffect, useState, type ReactNode } from "react";
import type { DeviceRole, LibraryView, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { Icon } from "./icons";

export const GROUP_LABEL: Record<string, string> = {
  waiting_on_you: "사람 차례",
  ready_for_review: "검토 대기",
  working: "작업 중",
  idle: "쉼",
  resolved: "끝남",
};

/** 짧은 Markdown 보기(제목·목록·code block·문단, 굵게·code). HTML은 해석하지 않고 글자로 둔다. */
export function Markdown({ text }: { text: string }) {
  const out: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) body.push(lines[i++]);
      i++;
      out.push(<pre key={key++}>{body.join("\n")}</pre>);
      continue;
    }
    const h = /^(#{1,4})\s+(.*)$/.exec(line);
    if (h) {
      const level = Math.min(h[1].length + 1, 5);
      const Tag = `h${level}` as "h2" | "h3" | "h4" | "h5";
      out.push(<Tag key={key++}>{inline(h[2])}</Tag>);
      i++;
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const items: { depth: number; text: string }[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
        const m = /^(\s*)([-*]|\d+\.)\s+(.*)$/.exec(lines[i])!;
        items.push({ depth: Math.floor(m[1].length / 2), text: m[3] });
        i++;
      }
      out.push(
        <ul key={key++}>
          {items.map((it, n) => (
            <li key={n} style={{ marginLeft: it.depth * 16 }}>
              {inline(it.text)}
            </li>
          ))}
        </ul>,
      );
      continue;
    }
    if (!line.trim()) {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(#{1,4}\s|```|\s*([-*]|\d+\.)\s)/.test(lines[i])) para.push(lines[i++]);
    out.push(<p key={key++}>{inline(para.join(" "))}</p>);
  }
  return <div className="md">{out}</div>;
}

function inline(s: string): ReactNode[] {
  const parts = s.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return parts.map((p, n) => {
    if (p.startsWith("`") && p.endsWith("`") && p.length > 1) return <code key={n}>{p.slice(1, -1)}</code>;
    if (p.startsWith("**") && p.endsWith("**") && p.length > 3) return <strong key={n}>{p.slice(2, -2)}</strong>;
    return p;
  });
}

const fmtSize = (n: number) =>
  n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)}MB` : n >= 1024 ? `${(n / 1024).toFixed(1)}KB` : `${n}B`;

const encodePath = (p: string) => p.split("/").map(encodeURIComponent).join("/");

/** thread 보고서와 결과물. 결과물 HTML은 sandbox iframe(opaque origin)으로 보여 이 화면의 권한에 닿지 못한다. */
export function ThreadPanel({
  t,
  role,
  onBack,
}: {
  t: ThreadView;
  role: DeviceRole;
  onBack: () => void;
}) {
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setMsg(null);
    call({ op: "thread_library", project: t.project, thread: t.id })
      .then((r) => {
        if (live && r.kind === "library") setLib(r);
      })
      .catch((e: unknown) => live && setMsg(e instanceof ApiError ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, [t.project, t.id, t.report_hash]);

  const ack = async () => {
    setMsg(null);
    try {
      await call({ op: "thread_ack", project: t.project, thread: t.id });
    } catch (e) {
      setMsg(e instanceof ApiError ? e.message : String(e));
    }
  };
  const index = lib?.files.find((f) => f.path === "index.html" && !f.on_agent);
  return (
    <div className="thread-view" data-testid="thread-view">
      <header className="term-head">
        <button className="back ghost icon" onClick={onBack} aria-label="목록으로">
          <Icon name="back" />
        </button>
        <div className="term-title">
          <strong>
            {t.project}/{t.id}
          </strong>
          <span className="muted">
            {t.profile}@{t.machine}
          </span>
        </div>
        <span className={`chip head-chip ${t.group}`}>
          {t.group === "working" && <span className="dot working" aria-hidden />}
          {GROUP_LABEL[t.group] ?? t.group}
        </span>
        {role !== "reader" && t.group === "ready_for_review" && (
          <button className="primary" onClick={ack}>
            <Icon name="check" />
            확인(ack)
          </button>
        )}
      </header>
      {msg && <div className="banner err">{msg}</div>}
      <div className="thread-body">
        <div className="thread-col">
          <div className="t-hero">
            <h1>{t.title}</h1>
            <div className="t-meta">
              <span className={`chip meta-chip ${t.group}`}>{GROUP_LABEL[t.group] ?? t.group}</span>
              <span className="mono">
                {t.project}/{t.id}
              </span>
              <span>
                {t.profile}@{t.machine}
              </span>
              {t.branch && <span className="mono">{t.branch}</span>}
              {t.agent?.percent != null && <span>{t.agent.percent}%</span>}
            </div>
          </div>
          <section className="card report" data-testid="report">
            <h2>보고</h2>
            {lib?.report ? <Markdown text={lib.report} /> : <p className="muted">아직 회수한 보고가 없다</p>}
          </section>
          {lib && lib.files.length > 0 && (
            <section className="card files" data-testid="files">
              <h2>
                결과물 <span className="muted">{lib.files.length}</span>
              </h2>
              {index && (
                <iframe
                  className="library-frame"
                  data-testid="library-frame"
                  title="결과물 index.html"
                  sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads"
                  src={lib.base + "index.html"}
                />
              )}
              <ul>
                {lib.files.map((f) => (
                  <li key={f.path}>
                    <Icon name="file" />
                    <a href={lib.base + encodePath(f.path)} target="_blank" rel="noopener noreferrer">
                      {f.path}
                    </a>
                    {f.on_agent && <span className="tag">agent에서 읽음</span>}
                    <span className="size">{fmtSize(f.size)}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
