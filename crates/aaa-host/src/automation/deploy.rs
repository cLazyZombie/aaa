//! 배포 제안은 자료다. build·thread 시작·rollout 실행은 호출하지 않는다.
use super::runtime::Event;
use crate::{Host, err};
use aaa_proto::{ErrorBody, ErrorCode, GitChange, InboxItem, NoticeKind, NoticeView};
impl Host {
    pub(super) fn propose_rollout(
        &self,
        project: &str,
        rule: &crate::routine::Routine,
        id: &str,
        events: &[Event],
        now: u64,
    ) -> Result<(), ErrorBody> {
        let internal = |e: anyhow::Error| err(ErrorCode::Internal, e.to_string());
        let mut latest = None;
        for event in events {
            let value: serde_json::Value = serde_json::from_str(&event.body).map_err(|source_error| {
                aaa_common::diagnostic::context(
                    source_error,
                    err(ErrorCode::BadRequest, "배포 제안 Git 자료 형식이 아니다"),
                )
            })?;
            let change: GitChange = serde_json::from_value(value["change"].clone()).map_err(|source_error| {
                aaa_common::diagnostic::context(source_error, err(ErrorCode::BadRequest, "배포 제안 Git 범위가 없다"))
            })?;
            if event.source != "git"
                || change.branch != "main"
                || change.kind != "pushed"
                || !change.after.as_deref().is_some_and(|s| s.len() == 40 && aaa_common::git_watch::valid_sha(s))
                || change.commits.is_empty()
            {
                return Err(err(ErrorCode::BadRequest, "배포 제안은 유효한 main push 자료만 받는다"));
            }
            latest = Some(change);
        }
        let change = latest.ok_or_else(|| err(ErrorCode::BadRequest, "배포 제안 입력이 비었다"))?;
        let commit = change.after.unwrap();
        let item = InboxItem {
            id: id.into(),
            project: project.into(),
            item_kind: "deploy_proposal".into(),
            subject: rule.name.clone(),
            event: "proposed".into(),
            summary: format!("배포 제안 · {}", &commit[..12]),
            body: format!(
                "상태: proposed(배포 시작 전)\nmain 커밋: {commit}\n새 커밋: {}{}개\n\n저장소 폴더에서 한 번 빌드한다:\nscripts/deploy-build.sh {commit} --out ~/aaa-candidate-{}\n\nprovenance의 SHA를 확인한 뒤 사람이 aaa 밖 터미널에서 deploy start의 yes를 입력하거나, 사용자가 coordinator에게 배포를 지시한다. 빌드·배포는 자동으로 실행하지 않았다. 웹 카드는 조회 전용이다.",
                if change.truncated { "최소 " } else { "" },
                change.commits.len(),
                &commit[..12]
            ),
            done: false,
            seen: false,
            created_at_ms: now,
            done_at_ms: None,
        };
        // 항목 정리 후 crash 재개에서도 같은 제안을 되살리지 않는다. 기존 원자 표지 저장을 쓴다.
        let marker = format!("deploy_proposed:{project}:{id}");
        if self.db.inbox_insert_marked(&item, &marker, "proposed", false).map_err(internal)? {
            self.inbox_publish(&item);
            self.send_notice(NoticeView {
                kind: NoticeKind::Automation,
                pane: None,
                machine: String::new(),
                project: Some(project.into()),
                thread: None,
                title: item.summary,
                agent: None,
                turn_ms: None,
                long: true,
                at_ms: now,
            });
        }
        Ok(())
    }
}
