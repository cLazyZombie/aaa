//! 설치 앱 배지: 기기별 알림 수가 아니라 host의 아직 볼 일 수다.
use std::collections::HashSet;

use crate::{Host, orch::internal};
use aaa_proto::{ErrorBody, ReportAttention, ThreadGroup, ThreadStatus};

impl Host {
    pub(crate) fn app_badge_count(&self) -> Result<u32, ErrorBody> {
        let reports = self.db.library_unseen_count().map_err(internal)?;
        // 같은 pane의 thread 확인 필요·미처리 보고·열린 질문은 한 자리다.
        let mut panes = HashSet::new();
        for t in self.threads_of(None, None)? {
            if t.status != ThreadStatus::Resolved
                && (t.group == ThreadGroup::WaitingOnYou
                    || (t.report_attention == Some(ReportAttention::NeedsUser) && t.report_hash != t.acked_hash)
                    || t.questions.as_ref().is_some_and(|q| q.answer.is_none()))
            {
                panes.insert(t.pane.unwrap_or_else(|| format!("{}/{}", t.project, t.id)));
            }
        }
        panes.extend(self.questions(false).into_iter().map(|q| q.pane));
        let posts = self
            .db
            .inbox_list(None, false)
            .map_err(internal)?
            .into_iter()
            .filter(|i| i.item_kind == "post" && i.event == "post request for user")
            .filter(|i| !self.request_has_open_question(i))
            .count();
        Ok(reports.saturating_add(panes.len() as u32).saturating_add(posts as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::LibraryReportRow, orch::ThreadRec};
    use aaa_proto::{BriefState, InboxItem, QuestionState, QuestionView, Role, ThreadKind};

    #[test]
    fn counts_shared_unseen_and_open_decisions_once_per_pane() {
        let path = std::env::temp_dir().join(format!("aaa-badge-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, path.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        assert_eq!(h.app_badge_count().unwrap(), 0);
        for (name, to_user, seen_at) in [("unseen", true, None), ("seen", true, Some(20)), ("work", false, None)] {
            h.db.library_report_put(&LibraryReportRow {
                project: "demo".into(),
                name: name.into(),
                to_user,
                sent_at: Some(10),
                seen_at,
                ..Default::default()
            })
            .unwrap();
        }
        let mut t = ThreadRec::starting("demo", "decision", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
        t.id = "t-0001".into();
        t.pane = Some("p1".into());
        t.status = ThreadStatus::Open;
        t.brief = BriefState::Delivered;
        h.db.thread_import("demo", &t.id, "open", &serde_json::to_string(&t).unwrap()).unwrap(); // pane이 없어 확인 필요
        let mut q = QuestionView {
            short_id: None,
            id: "q1".into(),
            pane: "p1".into(),
            machine: "mac-a".into(),
            project: Some("demo".into()),
            thread: Some(t.id.clone()),
            question_kind: "approval".into(),
            text: "approve?".into(),
            options: vec![],
            state: QuestionState::Open,
            answer: None,
            created_at_ms: 1,
            menu: None,
        };
        h.save_question(&q);
        let mut item = InboxItem {
            id: "i1".into(),
            project: "demo".into(),
            item_kind: "post".into(),
            subject: "coordinator".into(),
            event: "post request for user".into(),
            summary: "decision".into(),
            body: String::new(),
            done: false,
            seen: true,
            created_at_ms: 1,
            done_at_ms: None,
        };
        h.db.inbox_put(&item).unwrap();
        assert_eq!(h.app_badge_count().unwrap(), 3, "보고 1 + 같은 pane 1 + 요청 1");
        h.db.library_opened_at("demo", "unseen", 30).unwrap();
        item.done = true;
        h.db.inbox_put(&item).unwrap();
        assert_eq!(h.app_badge_count().unwrap(), 1, "다른 기기에서 보고 처리한 상태");
        t.status = ThreadStatus::Resolved;
        h.save(&mut t);
        q.state = QuestionState::Answered;
        h.save_question(&q);
        assert_eq!(h.app_badge_count().unwrap(), 0);
        // pane이 닫힌 보고도 표지가 실제 사본 hash와 맞고 아직 ack하지 않았으면 결정이다.
        use sha2::{Digest, Sha256};
        let report = "[결정 필요] 설치 방식 선택\n";
        let report_path = h.project_dir("demo").unwrap().join("threads").join("t-0001.md");
        std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        std::fs::write(&report_path, report).unwrap();
        t.status = ThreadStatus::Open;
        t.report_hash = Some(hex::encode(Sha256::digest(report.as_bytes())));
        h.save(&mut t);
        assert_eq!(h.app_badge_count().unwrap(), 1, "미처리 결정 보고");
        t.acked_hash = t.report_hash.clone();
        h.save(&mut t);
        assert_eq!(h.app_badge_count().unwrap(), 0, "처리한 결정 보고");
        drop(h);
        std::fs::remove_dir_all(path).unwrap();
    }
}
