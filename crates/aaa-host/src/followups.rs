//! 완료 보고의 후속 원장. 보고의 교체·resolve·inbox 정리는 후속을 처리한 근거가 아니다.
use aaa_proto::{ErrorBody, ErrorCode, ThreadFollowup, ThreadView};
use sha2::{Digest, Sha256};

use crate::{
    Host, err,
    orch::{ThreadRec, internal},
    tasks,
};

pub(crate) fn pending(rec: &ThreadRec) -> bool {
    rec.task_clean.followups.iter().any(|f| f.handled_ms.is_none())
}

pub(crate) fn capture(rec: &mut ThreadRec, revision: &str, report: &str) {
    if crate::report_mark::marker(report) != Some("[완료]")
        || rec.task_clean.followup_revisions.iter().any(|s| s == revision)
    {
        return;
    }
    rec.task_clean.followup_revisions.push(revision.into());
    for (n, line) in aaa_proto::next_lines(report).into_iter().enumerate() {
        let id = hex::encode(Sha256::digest(format!("{revision}:{n}")));
        if rec.task_clean.followups.iter().any(|f| f.id == id || (f.line == line && f.handled_ms.is_none())) {
            continue;
        }
        rec.task_clean.followups.push(ThreadFollowup {
            id,
            revision: revision.into(),
            line,
            task: None,
            reason: None,
            actor: None,
            handled_ms: None,
        });
        // 새 후속이 오면 이미 내렸던 자동 정리 판정을 다시 한다.
        rec.task_clean.judged_ms = None;
    }
}

impl Host {
    pub(crate) fn followup_notice(&self, rec: &ThreadRec) {
        if pending(rec) {
            self.inbox_once(&rec.project, "task-followup", &rec.id, "task needs check",
                &format!("{} 후속 남음: 완료 보고의 Next를 후속 할 일에 잇거나 버림·정리 완료 사유를 기록한다. aaa thread show {}", rec.id, rec.id));
        } else {
            self.inbox_finish_where(&rec.project, |i| i.item_kind == "task-followup" && i.subject == rec.id);
        }
    }

    pub(crate) fn unresolved_followup_inbox(&self, item: &aaa_proto::InboxItem) -> bool {
        item.item_kind == "task-followup"
            && self
                .load(&item.project, &item.subject)
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "후속 원장을 읽지 못해 inbox를 보존한다"))
                .map_or(true, |r| pending(&r))
    }

    /// 파일 잠금 → rec 잠금 순서로 task 삭제와 직렬화한다. 같은 프로젝트의 실재 ID만 잇는다.
    pub(crate) fn thread_followup(
        &self,
        project: &str,
        thread: &str,
        id: &str,
        task: Option<&str>,
        reason: Option<&str>,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        // thread 안 agent는 담당 결과를 스스로 버리지 않는다. task 편집과 같은 coordinator 검증을 쓴다.
        let actor = self.task_writer(project, actor)?;
        if task.is_some() == reason.is_some() {
            return Err(err(ErrorCode::BadRequest, "후속 할 일과 버림 사유 중 하나만 쓴다"));
        }
        let reason = reason.map(str::trim);
        if reason.is_some_and(|s| s.is_empty() || s.len() > 2000 || s.chars().any(char::is_control)) {
            return Err(err(ErrorCode::BadRequest, "사유는 비지 않은 한 줄(2000바이트 이하)로 쓴다"));
        }
        let dir = self.project_dir(project)?;
        let path = dir.join("TASKS.md");
        let lock = self.file_locks.of(&path);
        let _guard = lock.lock().unwrap();
        let source = self.load(project, thread)?;
        let target = if let Some(value) = task {
            let long = self
                .db
                .task_long_id(project, value.trim())
                .map_err(internal)?
                .ok_or_else(|| err(ErrorCode::NotFound, "같은 프로젝트의 후속 할 일 ID가 필요하다"))?;
            let text = std::fs::read_to_string(&path).map_err(internal)?;
            let active = tasks::parse(&text).into_iter().find(|t| t.id.as_deref() == Some(long.as_str()));
            let archived = self.db.archive_list(project).map_err(internal)?.into_iter().find(|a| a.task_id == long);
            let linked = active
                .as_ref()
                .or(archived.as_ref().map(|a| &a.task))
                .ok_or_else(|| err(ErrorCode::NotFound, "후속 할 일이 없다"))?;
            if linked.thread.as_deref() == Some(thread) || source.task_clean.cleaned.iter().any(|s| s == &linked.title)
            {
                return Err(err(ErrorCode::BadRequest, "원래 맡은 할 일을 자기 후속으로 잇지 않는다"));
            }
            Some(self.db.task_short_id(project, &long).map_err(internal)?)
        } else {
            None
        };
        let entry = source
            .task_clean
            .followups
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| err(ErrorCode::NotFound, "후속 ID가 없다. thread show로 현재 원장을 읽는다"))?;
        if entry.handled_ms.is_some() {
            return Err(err(ErrorCode::Conflict, "이미 처리한 후속이다"));
        }
        // update_rec 안에서 다시 검사한다. 두 기기의 동시 처리는 한 번만 확정한다.
        let mut changed = false;
        let rec = self.update_rec(project, thread, |r| {
            if let Some(f) = r.task_clean.followups.iter_mut().find(|f| f.id == id && f.handled_ms.is_none()) {
                f.task = target.clone();
                f.reason = reason.map(str::to_string);
                f.actor = Some(actor.clone());
                f.handled_ms = Some(aaa_common::util::now_ms());
                r.task_clean.judged_ms = None;
                changed = true;
            }
        })?;
        if !changed {
            return Err(err(ErrorCode::Conflict, "후속이 그사이 처리됐다"));
        }
        self.db.audit(&actor, "thread_followup", &format!("{project}/{thread} {id} task={target:?} reason={reason:?}"));
        self.followup_notice(&rec);
        self.flow(&rec, "followup_handled", Some(format!("threads/{thread}.md")), "완료 보고의 후속을 기록했다");
        let text = std::fs::read_to_string(&path).map_err(internal)?;
        aaa_common::diagnostic::ignored(
            self.task_events.send((project.into(), tasks::sha256(&text))),
            "후속 처리 뒤 할 일 갱신",
        );
        self.tick_wake.notify_one();
        Ok(self.thread_view(&rec))
    }

    /// 자동 보관·완료 체크·삭제·연결 해제로 후속을 숨기지 않는다. 원문 편집에도 같은 규칙을 쓴다.
    pub(crate) fn protect_task_followups(&self, project: &str, before: &str, after: &str) -> Result<(), ErrorBody> {
        let now = tasks::parse(after);
        for old in tasks::parse(before) {
            let Some(thread) = old.thread.as_deref() else { continue };
            let rec = match self.load(project, thread) {
                Ok(rec) => rec,
                Err(e) if e.code == ErrorCode::NotFound => continue,
                Err(e) => return Err(e),
            };
            if !pending(&rec) {
                continue;
            }
            let kept = now.iter().find(|t| match &old.id {
                Some(id) => t.id.as_ref() == Some(id),
                None => t.title == old.title,
            });
            if !kept.is_some_and(|t| t.thread == old.thread && (!t.done || old.done)) {
                return Err(err(
                    ErrorCode::BadRequest,
                    format!("{thread} 후속 남음: 후속 할 일 연결이나 버림·정리 완료 사유를 먼저 기록한다"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{Role, TaskOp, ThreadKind, ThreadStatus};
    use std::{path::PathBuf, sync::Arc};

    struct Lab {
        h: Arc<Host>,
        root: PathBuf,
        thread: String,
    }
    impl Lab {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("aaa-followup-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = root.join("projects").display().to_string();
            cfg.notify.web_push = false;
            let h = Host::open(cfg, root.join("host")).unwrap();
            h.project_create("demo", Default::default(), "local:cli").unwrap();
            let thread =
                h.db.thread_insert_next("demo", |id| {
                    let mut r = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
                    r.id = id.into();
                    r.status = ThreadStatus::Resolved;
                    r.task_clean.why = Some("merged".into());
                    serde_json::to_string(&r).unwrap()
                })
                .unwrap();
            let dir = h.project_dir("demo").unwrap();
            std::fs::write(dir.join("TASKS.md"), format!("## Backlog\n- [ ] 시안 · {thread}\n- [ ] 구현\n")).unwrap();
            h.task_board("demo").unwrap();
            Self { h, root, thread }
        }
        fn report(&self, body: &str) -> ThreadRec {
            let revision = hex::encode(Sha256::digest(body));
            std::fs::write(self.h.project_dir("demo").unwrap().join(format!("threads/{}.md", self.thread)), body)
                .unwrap();
            let rec = self
                .h
                .update_rec("demo", &self.thread, |r| {
                    r.report_hash = Some(revision.clone());
                    capture(r, &revision, body);
                })
                .unwrap();
            self.h.followup_notice(&rec);
            rec
        }
        fn tick(&self) {
            self.h.task_clean_pass("demo", &self.h.project_dir("demo").unwrap());
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn completed_next_survives_merge_inbox_cleanup_and_task_mutations_until_linked() {
        let lab = Lab::new();
        let h = &lab.h;
        let report = "[완료] 시안 완료\n\n## Next\n- A안 구현 할 일을 만든다\n";
        let rec = lab.report(report);
        lab.report(report); // 같은 사본은 중복 원장을 만들지 않는다.
        lab.report(&report.replace("시안 완료", "시안 완료 수정")); // 새 판의 같은 미처리 줄도 합친다.
        lab.tick();
        assert_eq!(h.load("demo", &lab.thread).unwrap().task_clean.followups.len(), 1);
        let board = h.task_board("demo").unwrap();
        let original = board.tasks.iter().find(|t| t.title == "시안").unwrap();
        assert!(original.attention.as_ref().unwrap().contains("후속 남음"));
        assert!(h.db.archive_list("demo").unwrap().is_empty(), "병합 근거보다 후속이 앞선다");
        let anchor = aaa_proto::TaskAnchor {
            list: original.list.clone(),
            title: original.title.clone(),
            block_sha256: original.block_sha256.clone(),
            line: original.line,
        };
        let delete = h.task_edit("demo", vec![TaskOp::Delete { at: anchor }], None, None, "local:cli").await;
        assert!(delete.unwrap_err().message.contains("후속 남음"));
        let before = std::fs::read_to_string(h.project_dir("demo").unwrap().join("TASKS.md")).unwrap();
        for after in [before.replace("- [ ] 시안", "- [x] 시안"), before.replace(&format!(" · {}", lab.thread), "")]
        {
            let op = TaskOp::RawSave { text: after, expected_sha256: tasks::sha256(&before) };
            assert!(
                h.task_edit("demo", vec![op], None, None, "local:cli").await.unwrap_err().message.contains("후속 남음")
            );
        }
        let item =
            h.db.inbox_list(Some("demo"), false).unwrap().into_iter().find(|i| i.item_kind == "task-followup").unwrap();
        h.inbox_done("demo", &[], true, "local:cli").unwrap();
        assert!(h.db.inbox_list(Some("demo"), false).unwrap().iter().any(|i| i.id == item.id));
        assert!(h.inbox_done("demo", &[item.id], false, "local:cli").unwrap_err().message.contains("후속 남음"));
        let id = &rec.task_clean.followups[0].id;
        assert!(h.thread_followup("demo", &lab.thread, id, None, Some(" "), "local:cli").is_err());
        assert!(
            h.thread_followup("demo", &lab.thread, id, Some(original.short_id.as_ref().unwrap()), None, "local:cli")
                .is_err()
        );
        assert!(h.thread_followup("demo", &lab.thread, id, Some("d-9999"), None, "local:cli").is_err());
        let target = board.tasks.iter().find(|t| t.title == "구현").unwrap();
        assert_eq!(
            h.thread_followup("demo", &lab.thread, id, None, Some("자기 버림"), "local:cli@worker").unwrap_err().code,
            ErrorCode::Forbidden
        );
        let gate = std::sync::Barrier::new(2);
        let attempt = || {
            gate.wait();
            h.thread_followup("demo", &lab.thread, id, target.short_id.as_deref(), None, "local:cli")
        };
        let attempts = std::thread::scope(|scope| {
            let a = scope.spawn(attempt);
            let b = scope.spawn(attempt);
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(attempts.iter().filter(|r| r.is_ok()).count(), 1, "두 기기는 한 번만 확정한다");
        assert_eq!(attempts.iter().find_map(|r| r.as_ref().err()).unwrap().code, ErrorCode::Conflict);
        let result = attempts.into_iter().find_map(Result::ok).unwrap();
        assert_eq!(result.followups[0].task, target.short_id);
        lab.report(&report.replace("시안 완료", "시안 완료 수정"));
        assert!(!pending(&h.load("demo", &lab.thread).unwrap()), "합쳤던 판을 다시 받아도 처리한 줄을 되살리지 않는다");
        assert!(h.thread_followup("demo", &lab.thread, id, None, Some("덮어쓰기"), "local:cli").is_err());
        assert!(!h.db.inbox_list(Some("demo"), false).unwrap().iter().any(|i| i.item_kind == "task-followup"));
        lab.tick();
        assert_eq!(h.task_board("demo").unwrap().tasks.len(), 1);
        assert_eq!(h.db.archive_list("demo").unwrap()[0].task.title, "시안");
    }

    #[test]
    fn no_linked_task_and_replaced_reports_keep_pending_with_durable_cleanup_reason() {
        let lab = Lab::new();
        let h = &lab.h;
        std::fs::write(h.project_dir("demo").unwrap().join("TASKS.md"), "## Backlog\n").unwrap();
        let rec = lab.report("[완료] 시안\n## Next\n- 정리: worktree와 branch를 지운다\n");
        lab.report("[진행 중] 다시 작업\n## Next\n");
        lab.report("[완료] 새 보고\n## Next\n");
        assert!(pending(&h.load("demo", &lab.thread).unwrap()), "새 보고로 이전 후속을 지우지 않는다");
        let id = &rec.task_clean.followups[0].id;
        let result = h
            .thread_followup("demo", &lab.thread, id, None, Some("정리 완료: worktree와 branch를 지웠다"), "local:cli")
            .unwrap();
        assert_eq!(result.followups[0].actor.as_deref(), Some("local:cli"));
        assert!(result.followups[0].handled_ms.is_some());
        lab.report("[완료] 시안\n## Next\n- 정리: worktree와 branch를 지운다\n");
        assert!(!pending(&h.load("demo", &lab.thread).unwrap()), "같은 원본은 처리 기록을 유지한다");
        let reopened = Host::open(h.cfg.clone(), lab.root.join("host")).unwrap();
        let persisted = reopened.thread_show("demo", &lab.thread).unwrap();
        assert_eq!(persisted.followups[0].reason, result.followups[0].reason);
        assert!(reopened.db.inbox_list(Some("demo"), false).unwrap().iter().all(|i| i.item_kind != "task-followup"));
    }
}
