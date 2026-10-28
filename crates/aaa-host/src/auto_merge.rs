//! 할 일·thread별 자동 병합(plan §8.530, t-1214). on이면 thread가 `[병합 준비됨]`으로 보고한 뒤 coordinator가 사용자에게 묻지 않고
//! 착지 창을 준다(착지는 그대로 한 번에 하나). off면 지금처럼 사용자 승인을 받는다.
//! - 값은 할 일 하나에 하나이고 host DB가 정본이다(`db/auto_merge.rs`). thread는 이어진 할 일의 값을 보인다: 재시작·이어 하기도 같다.
//! - 프로젝트 기본값은 없다. 새 할 일은 off에서 시작한다(사용자 답 2A).
//! - 누가: 사용자(조작 기기·세션 밖 CLI)와, 사용자 지시 근거를 인용한 그 프로젝트 coordinator(보류 §8.426과 같은 규칙).
//!   thread·평가자 세션은 보기 권한이라 요청 판정에서 막힌다.
//! - 바꾼 기록은 할 일 기록(`auto_merge`, before·after `off`·`on`)과 감사에 남는다. 이미 시작한 착지는 되돌리지 않는다.

use aaa_proto::{ErrorBody, ErrorCode, FrontResponse, TaskAction};

use crate::orch::{ThreadRec, bad, internal};
use crate::{Host, err, tasks};

fn word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// 사용자 지시 근거 한 줄(공백 제거 뒤 비면 없음, 2000바이트 이하, 제어 문자 없음)
fn reason(text: Option<String>) -> Result<Option<String>, ErrorBody> {
    let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if text.as_deref().is_some_and(|t| t.len() > 2000 || t.chars().any(char::is_control)) {
        return Err(bad("사용자 지시 근거는 한 줄(2000바이트 이하)로 쓴다"));
    }
    Ok(text)
}

/// 바꿀 할 일
struct Target {
    task_id: String,
    title: String,
    list: String,
    thread: Option<String>,
}

impl Host {
    /// 이어진 할 일의 자동 병합. 할 일이 없거나 읽지 못하면 off다
    pub(crate) fn thread_auto_merge(&self, r: &ThreadRec) -> bool {
        r.task_id.as_ref().is_some_and(|id| {
            self.db
                .auto_merge(&r.project, id)
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "자동 병합을 읽지 못해 off로 보인다"))
                .unwrap_or(false)
        })
    }

    fn auto_merge_actor(
        &self,
        project: &str,
        actor: &str,
        user_request: Option<String>,
    ) -> Result<Option<String>, ErrorBody> {
        let reason = reason(user_request)?;
        if self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            if reason.is_none() {
                return Err(err(
                    ErrorCode::Forbidden,
                    "coordinator는 사용자가 자동 병합을 켜거나 끄라고 했을 때만 --user-request '<사용자 지시>'와 함께 바꾼다",
                ));
            }
            return Ok(reason);
        }
        if actor.starts_with("device:") || (actor.starts_with("local:") && !crate::front::in_session(actor)) {
            return Ok(reason);
        }
        Err(err(
            ErrorCode::Forbidden,
            "자동 병합은 사용자 직접 조작이나 그 프로젝트 coordinator의 사용자 지시 인용만 바꾼다",
        ))
    }

    /// `t-0001`이면 그 thread의 할 일, 아니면 할 일 선택자(`d-0001`·ULID·제목). 활성 목록이 먼저이고 완료 보관도 받는다
    fn auto_merge_target(&self, project: &str, target: &str) -> Result<Target, ErrorBody> {
        let board = self.task_board(project)?;
        let of = |t: &aaa_proto::TaskView| -> Option<Target> {
            Some(Target {
                task_id: t.id.clone()?,
                title: t.title.clone(),
                list: t.list.clone(),
                thread: t.thread.clone(),
            })
        };
        let archived = |pick: &dyn Fn(&aaa_proto::ArchivedTask) -> bool| -> Result<Option<Target>, ErrorBody> {
            Ok(self
                .db
                .archive_list(project)
                .map_err(internal)?
                .into_iter()
                .rev()
                .find(|r| !r.restored && pick(r))
                .map(|r| Target { task_id: r.task_id, title: r.task.title, list: r.task.list, thread: r.task.thread }))
        };
        if aaa_proto::canonical_short_id(target, 't').as_deref() == Some(target) {
            let rec = self.load(project, target)?;
            if let Some(id) = &rec.task_id {
                if let Some(t) = board.tasks.iter().find(|t| t.id.as_ref() == Some(id)).and_then(of) {
                    return Ok(Target { thread: Some(rec.id), ..t });
                }
                if let Some(t) = archived(&|r| &r.task_id == id)? {
                    return Ok(Target { thread: Some(rec.id), ..t });
                }
            }
            // 이 칸 전에 띄운 thread: 할 일 목록의 ` · t-0001` 연결로 찾는다
            if let Some(t) = board.tasks.iter().find(|t| t.thread.as_deref() == Some(target)).and_then(of) {
                return Ok(t);
            }
            return Err(err(ErrorCode::NotFound, format!("{target}에 이어진 할 일이 없다(자동 병합은 할 일에 둔다)")));
        }
        let value = self.task_selector(project, target)?;
        if let Some(t) = tasks::find(&board.tasks, &value) {
            return of(t).ok_or_else(|| internal("할 일에 ID가 없다"));
        }
        archived(&|r| r.task_id == value || r.task.title.eq_ignore_ascii_case(value.trim()))?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("할 일이 없다: {target}")))
    }

    pub(crate) fn auto_merge_set(
        &self,
        project: &str,
        target: &str,
        on: bool,
        user_request: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let user_request = self.auto_merge_actor(project, actor, user_request)?;
        let t = self.auto_merge_target(project, target)?;
        let log = crate::task_edit::Record {
            action: TaskAction::AutoMerge,
            list: t.list.clone(),
            from_list: None,
            title: t.title.clone(),
            before: Some(word(!on).into()),
            after: Some(word(on).into()),
            thread: t.thread.clone(),
            restored: None,
        };
        let changed =
            self.db.auto_merge_set(project, &t.task_id, on, actor, user_request.as_deref(), &log).map_err(internal)?;
        if changed {
            self.db.audit(
                actor,
                "task_auto_merge",
                &format!(
                    "{project}/{} {}{}",
                    t.task_id,
                    word(on),
                    user_request.as_deref().map(|r| format!(" 사용자 지시: {r}")).unwrap_or_default()
                ),
            );
            // 옛 thread도 다음부터 바로 찾게 할 일 ID를 채운다. 저장이 thread 사건(새 값)을 낸다
            if let Some(tid) = &t.thread
                && let Ok(rec) = self.load(project, tid)
            {
                if rec.task_id.is_none() {
                    aaa_common::diagnostic::ignored(
                        self.update_rec(project, tid, |r| r.task_id = Some(t.task_id.clone())),
                        "연결 칸은 다음 변경 때 다시 채운다",
                    );
                } else {
                    aaa_common::diagnostic::ignored(
                        self.thread_events.send(self.thread_view(&rec)),
                        "수신자가 끝난 뒤에도 현재 작업은 마친다",
                    );
                }
            }
            // 사용자가 바꾼 것은 coordinator가 다음 턴에 알게 남긴다(이미 병합 대기인 thread의 착지 창을 정하는 근거)
            if self.coordinator_project_of_actor(actor).as_deref() != Some(project) {
                let short = self.db.task_short_id(project, &t.task_id).map_err(internal)?;
                let summary = format!(
                    "사용자가 {short}{}의 자동 병합을 {}: {}",
                    t.thread.as_deref().map(|x| format!("({x})")).unwrap_or_default(),
                    if on { "켰다" } else { "껐다" },
                    if on {
                        "병합 준비되면 사용자에게 묻지 않고 착지 창을 준다"
                    } else {
                        "병합 전에 사용자 승인을 받는다(이미 시작한 착지는 그대로)"
                    }
                );
                self.inbox_add(project, "task", &short, &format!("auto-merge {}", word(on)), &summary);
            }
            let sha = crate::file_rev::read(&self.project_dir(project)?.join("TASKS.md"))
                .map(|r| r.sha256)
                .unwrap_or_default();
            aaa_common::diagnostic::ignored(
                self.task_events.send((project.to_string(), sha)),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
        Ok(FrontResponse::AutoMerge {
            short_id: Some(self.db.task_short_id(project, &t.task_id).map_err(internal)?),
            task_id: t.task_id,
            thread: t.thread,
            on,
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use aaa_proto::{DeviceRole, ErrorCode, FrontRequest as R, FrontResponse, Role, ThreadKind, ThreadStatus};

    use crate::Host;

    fn host() -> std::sync::Arc<Host> {
        let root = std::env::temp_dir().join(format!("aaa-auto-merge-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        let dir = h.project_dir("demo").unwrap();
        std::fs::write(dir.join("TASKS.md"), "## Backlog\n- [ ] 새 일\n- [ ] 이은 일 · t-0007\n- [ ] 옛 일 · t-0008\n")
            .unwrap();
        h.task_identify("demo").unwrap();
        h
    }

    fn set(target: &str, on: bool, user_request: Option<&str>) -> R {
        R::AutoMergeSet {
            project: "demo".into(),
            target: target.into(),
            on,
            user_request: user_request.map(str::to_string),
        }
    }

    fn thread(h: &Host, id: &str, task_id: Option<String>) {
        let mut rec =
            crate::orch::ThreadRec::starting("demo", "이은 일", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = id.into();
        rec.status = ThreadStatus::Open;
        rec.task_id = task_id;
        h.db.thread_import("demo", id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    }

    /// 사용자가 할 일 ID로 켜면 목록·thread·기록·coordinator inbox에 같은 값이 보이고, 같은 값은 다시 쓰지 않는다
    #[tokio::test]
    async fn user_sets_task_and_thread_shows_it() {
        let h = host();
        let board = h.task_board("demo").unwrap();
        let linked = board.tasks.iter().find(|t| t.title == "이은 일").unwrap().clone();
        assert!(board.tasks.iter().all(|t| !t.auto_merge), "기본 off");
        thread(&h, "t-0007", linked.id.clone());
        let r = h.call(DeviceRole::Controller, "device:d", set(linked.short_id.as_deref().unwrap(), true, None)).await;
        let Ok(FrontResponse::AutoMerge { on: true, changed: true, thread: Some(t), .. }) = r else { panic!("{r:?}") };
        assert_eq!(t, "t-0007");
        let board = h.task_board("demo").unwrap();
        assert!(board.tasks.iter().find(|t| t.title == "이은 일").unwrap().auto_merge);
        assert!(!board.tasks.iter().find(|t| t.title == "새 일").unwrap().auto_merge);
        assert!(h.thread_show("demo", "t-0007").unwrap().auto_merge);
        // 같은 값: 기록·inbox를 더하지 않는다
        let again = h.call(DeviceRole::Controller, "device:d", set("이은 일", true, None)).await;
        assert!(matches!(again, Ok(FrontResponse::AutoMerge { on: true, changed: false, .. })), "{again:?}");
        let log = h.db.task_log_list("demo", None, 50, None).unwrap();
        let merges: Vec<_> = log.iter().filter(|e| e.action == aaa_proto::TaskAction::AutoMerge).collect();
        assert_eq!(merges.len(), 1);
        assert_eq!((merges[0].before.as_deref(), merges[0].after.as_deref()), (Some("off"), Some("on")));
        assert_eq!(merges[0].thread.as_deref(), Some("t-0007"));
        let inbox = h.db.inbox_list(Some("demo"), true).unwrap();
        assert_eq!(inbox.iter().filter(|i| i.event == "auto-merge on").count(), 1);
        // thread 번호 별칭으로 끈다
        let off = h.call(DeviceRole::Controller, "device:d", set("t-7", false, None)).await;
        assert!(matches!(off, Ok(FrontResponse::AutoMerge { on: false, changed: true, .. })), "{off:?}");
        assert!(!h.thread_show("demo", "t-0007").unwrap().auto_merge);
    }

    /// 이 칸 전에 띄운 thread는 할 일 목록의 연결로 찾고, 찾은 할 일 ID를 thread 기록에 채운다
    #[tokio::test]
    async fn legacy_thread_resolves_through_task_link() {
        let h = host();
        thread(&h, "t-0008", None);
        let r = h.call(DeviceRole::Controller, "device:d", set("t-0008", true, None)).await;
        assert!(matches!(r, Ok(FrontResponse::AutoMerge { on: true, changed: true, .. })), "{r:?}");
        assert!(h.load("demo", "t-0008").unwrap().task_id.is_some());
        assert!(h.thread_show("demo", "t-0008").unwrap().auto_merge);
        // 할 일 없는 thread는 거부한다
        thread(&h, "t-0009", None);
        let e = h.call(DeviceRole::Controller, "device:d", set("t-0009", true, None)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e:?}");
    }

    /// 보기 기기·thread 세션·남의 pane 셸은 바꾸지 못한다(coordinator 경로는 m13 실제 CLI가 본다)
    #[tokio::test]
    async fn only_user_or_quoting_coordinator_changes_it() {
        let h = host();
        for (role, actor) in [
            (DeviceRole::Reader, "device:r"),
            (DeviceRole::Reader, "worker:t-0007"),
            (DeviceRole::Local, "local:cli@shell-pane"),
        ] {
            let e = h.call(role, actor, set("새 일", true, Some("사용자가 켜라고 했다"))).await.unwrap_err();
            assert!(matches!(e.code, ErrorCode::Forbidden), "{actor}: {e:?}");
        }
        let long = "x".repeat(2001);
        let e = h.call(DeviceRole::Local, "local:cli", set("새 일", true, Some(&long))).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(h.task_board("demo").unwrap().tasks.iter().all(|t| !t.auto_merge));
        // 세션 밖 CLI(사람 터미널)는 근거 없이 된다
        let ok = h.call(DeviceRole::Local, "local:cli", set("새 일", true, None)).await;
        assert!(matches!(ok, Ok(FrontResponse::AutoMerge { on: true, thread: None, .. })), "{ok:?}");
    }
}
