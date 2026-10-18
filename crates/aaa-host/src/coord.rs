//! coordinator 일 흐름(부록 A, 0.2.35 의미): `context`, `thread next`, `assignable`, `thread start --from-task`.
//!
//! - TASKS.md는 coordinator가 쓰고 aaa는 읽기만 한다. 완료는 줄을 지워 표시한다.
//! - `start_threads`(safety, propose·auto)는 coordinator가 따르는 안내 값이다. aaa가 강제하지 않는다(0.2.35와 같다).
//! - 권한을 건너뛰는 실행 인자는 사람만 바꾸는 host.toml `[safety.*]`(yolo·thread_agent_args)와 그 머신 agent.toml의 profile
//!   `args`에 둔다(coordinator가 바꿀 수 없다, plan §8.162).

use std::path::Path;
use std::sync::Arc;

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, FrontResponse, MessageMode, OwnerChoice, ProjectContext,
    ThreadGroup, ThreadStatus,
};

use crate::orch::StartArgs;
use crate::tasks::{self, Owner};
use crate::{Host, err};

fn bad(m: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, m)
}

fn next_file(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join("threads").join(format!("{id}.next.md"))
}

/// `- `·`* `·`N. ` 표시를 뗀 줄
fn item(l: &str) -> Option<String> {
    let t = l.trim();
    if t.is_empty() {
        return None;
    }
    let s = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r))
        .unwrap_or(t);
    Some(s.to_string())
}

impl Host {
    /// 보고서 `## Next` 줄 + `threads/<id>.next.md`
    fn next_lines(&self, dir: &Path, report_next: &[String], id: &str) -> Vec<String> {
        let mut lines = report_next.to_vec();
        if let Ok(t) = std::fs::read_to_string(next_file(dir, id)) {
            lines.extend(t.lines().filter_map(item));
        }
        lines
    }

    pub(crate) async fn thread_next(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        line: Option<u32>,
        add: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let rec = self.thread_show(project, id)?;
        if line.is_some() && add.is_some() {
            return Err(bad("--line과 --add는 함께 쓰지 않는다"));
        }
        if let Some(text) = add {
            let text = text.trim();
            if text.is_empty() || text.contains('\n') {
                return Err(bad("Next 줄은 비지 않은 한 줄이다"));
            }
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(next_file(&dir, id))
                .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
            writeln!(f, "- {text}").map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
            self.db.audit(actor, "thread_next_add", &format!("{project}/{id}"));
        }
        let lines = self.next_lines(&dir, &rec.next, id);
        let message = match line {
            None => None,
            Some(n) => {
                let Some(text) = n.checked_sub(1).and_then(|i| lines.get(i as usize)) else {
                    return Err(bad(format!("{id}에 Next {n}번째 줄이 없다({}줄)", lines.len())));
                };
                Some(self.thread_message(project, id, text.clone(), MessageMode::Queue, actor).await?)
            }
        };
        Ok(FrontResponse::Next { lines, message })
    }

    /// 머신마다 맡길 수 있는 profile(그 프로젝트 safety `thread_profiles` ∩ 그 머신 agent.toml과 설치·로그인한 기본
    /// profile). 허용 목록에 이름이 있는 기본 profile은 설치를 몰라도 넣는다(참고 `usable`). 끊긴 머신은 모른다.
    pub(crate) async fn assignable(&self, project: &str) -> Vec<OwnerChoice> {
        let safety = self.safety(project);
        let mut out = Vec::new();
        for m in self.machine_views() {
            let mut profiles = Vec::new();
            let connected = m.connection == aaa_proto::Connection::Connected;
            if connected
                && let Some(conn) = self.conn_of(&m.id)
                && let Ok(AgentResponse::Profiles { profiles: ps }) = conn.request(AgentRequest::Profiles).await
            {
                profiles = ps.into_iter().map(|p| p.name).filter(|n| safety.profile_allowed(n)).collect();
                if self.machine_feature(&m.id, "profiles.v2") {
                    for b in safety.thread_profiles.iter().flatten() {
                        if aaa_config::profile_edit::builtin(b).is_some() && !profiles.contains(b) {
                            profiles.push(b.clone());
                        }
                    }
                }
            }
            out.push(OwnerChoice { machine: m.name, connected, profiles });
        }
        out
    }

    /// TASKS.md owner로 써도 되는가(0.2.35 `assignable --check`). 맨 profile은 기본 머신(host 로컬)을 본다.
    pub(crate) fn check_owner(&self, owners: &[OwnerChoice], raw: &str) -> Result<String, ErrorBody> {
        let raw = raw.trim().trim_start_matches('(').trim_end_matches(')').trim();
        let all: Vec<String> = owners.iter().flat_map(|o| o.profiles.clone()).collect();
        let local = self.machine_views().into_iter().find(|m| m.local).map(|m| m.name);
        let on = |machine: &str| owners.iter().find(|o| o.machine == machine);
        match Owner::parse(Some(raw), &all) {
            Owner::Unassigned => Ok(format!("`{raw}`: 미할당(thread를 시작할 때 --profile을 고른다)")),
            Owner::Me => Ok("`me`: 사용자의 할 일".into()),
            Owner::Person(p) => {
                Err(bad(format!("`{p}`는 사람 이름으로 읽힌다. agent owner는 profile·@machine·profile@machine이다")))
            }
            Owner::Agent { profile, machine } => {
                let m = machine.or(local).ok_or_else(|| bad("로컬 agent가 없다. @machine을 붙인다"))?;
                let o = on(&m).ok_or_else(|| bad(format!("모르는 머신: {m}")))?;
                if let Some(p) = profile {
                    if !o.connected {
                        return Err(bad(format!("{m}이 연결되지 않아 profile 목록을 모른다")));
                    }
                    if !o.profiles.contains(&p) {
                        return Err(bad(format!(
                            "{m}에서 쓸 수 있는 profile이 아니다: {p}({})",
                            o.profiles.join(", ")
                        )));
                    }
                }
                Ok(format!("`{raw}`는 쓸 수 있는 owner다"))
            }
        }
    }

    /// `thread start --from-task`: owner를 profile·machine 기본값으로, 메모를 할 일에 붙인다
    /// 돌려주는 것은 경고 한 줄이다(이미 열린 thread가 맡은 task: 참고에는 그 거부가 없어 막지 않는다).
    pub(crate) async fn apply_from_task(&self, a: &mut StartArgs, title: &str) -> Result<Option<String>, ErrorBody> {
        let dir = self.project_dir(&a.project)?;
        let text = std::fs::read_to_string(dir.join("TASKS.md")).unwrap_or_default();
        let list = tasks::parse(&text);
        let t = tasks::find(&list, title).ok_or_else(|| bad(format!("TASKS.md에 없는 task: {title}")))?.clone();
        let mut warning = None;
        if let Some(tid) = &t.thread
            && self.thread_show(&a.project, tid).is_ok_and(|r| r.status == ThreadStatus::Open)
        {
            warning = Some(format!("열린 {tid}가 이미 이 task를 맡고 있다. 그래도 시작했다"));
        }
        let all: Vec<String> = self.assignable(&a.project).await.into_iter().flat_map(|o| o.profiles).collect();
        match Owner::parse(t.owner.as_deref(), &all) {
            Owner::Me => return Err(bad(format!("사용자의 할 일(me)이다: {}", t.title))),
            Owner::Person(p) => return Err(bad(format!("사람({p})이 맡은 task다: {}", t.title))),
            Owner::Unassigned => {}
            Owner::Agent { profile, machine } => {
                if let Some(w) = profile {
                    if a.profile.is_empty() {
                        a.profile = w;
                    } else if a.profile != w {
                        return Err(bad(format!("task owner와 --profile이 다르다: {w} / {}", a.profile)));
                    }
                }
                if let Some(w) = machine {
                    match &a.machine {
                        None => a.machine = Some(w),
                        Some(h) if *h != w => return Err(bad(format!("task owner와 --machine이 다르다: {w} / {h}"))),
                        _ => {}
                    }
                }
            }
        }
        if a.title.trim().is_empty() {
            a.title = t.title.clone();
        }
        if a.task.trim().is_empty() {
            a.task = t.title.clone();
        }
        if !t.notes.is_empty() {
            a.task = format!("{}\n\n## Notes from the task list\n\n{}\n", a.task.trim_end(), t.notes.join("\n"));
        }
        Ok(warning)
    }

    /// coordinator용 요약. herdr-projects `context`처럼 처리 안 한 inbox 항목을 보이고, `peek`이 아니면 「봤다」로 표시한다
    /// (본 항목은 다음 nudge에 다시 나오지 않는다).
    pub(crate) async fn context(self: &Arc<Self>, project: &str, peek: bool) -> Result<ProjectContext, ErrorBody> {
        let dir = self.project_dir(project)?;
        let tasks = tasks::parse(&std::fs::read_to_string(dir.join("TASKS.md")).unwrap_or_default());
        let mut threads: Vec<_> =
            self.threads_of(Some(project), None)?.into_iter().filter(|t| t.group != ThreadGroup::Resolved).collect();
        for t in &mut threads {
            t.next = self.next_lines(&dir, &t.next, &t.id);
        }
        let page = self.inbox(project, false, peek)?;
        Ok(ProjectContext {
            project: project.to_string(),
            path: dir.display().to_string(),
            start_threads: self.safety(project).start_threads,
            tasks,
            threads,
            assignable: self.assignable(project).await,
            inbox_open: page.items.len() as u32 + page.omitted_open,
            inbox: page.items,
            evaluator: self.evaluator_rec(project).map(|r| Box::new(self.evaluator_view(&r))),
        })
    }
}
