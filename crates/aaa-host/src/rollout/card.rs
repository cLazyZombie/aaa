//! 웹 조회는 실행기와 독립이다. DB 정본의 표시용 필드만 내보낸다.
use super::*;
impl Host {
    pub(crate) fn rollout_card(&self) -> Result<Option<RolloutCard>, ErrorBody> {
        let Some(id) = self.db.meta("rollout:active").map_err(internal)? else {
            return Ok(None);
        };
        Ok(Some(self.card_of(self.load_rollout(&id)?)))
    }
    pub(crate) fn card_of(&self, r: RolloutView) -> RolloutCard {
        RolloutCard {
            id: r.id,
            state: r.state,
            version: r.version,
            sha256: r.spec.sha256,
            commit: r.spec.commit,
            restart: r.spec.restart,
            cursor: r.cursor,
            steps: r
                .steps
                .into_iter()
                .map(|s| RolloutCardStep {
                    machine: self.machine_name(&s.machine),
                    phase: s.phase,
                    completed: s.completed,
                    started_ms: s.started_ms,
                    transferred_bytes: s.transferred_bytes,
                    total_bytes: s.total_bytes,
                })
                .collect(),
            // 실행기 오류에는 경로·PID·self-check 출력이 섞인다. 원문은 coordinator 정본에만 둔다.
            error: r.error.as_ref().map(|_| match r.state {
                RolloutState::Recovering => "단계가 실패해 해당 머신을 복구하고 있습니다. 상세 진단은 coordinator의 배포 기록에서 확인하세요.",
                RolloutState::StoppedRolledBack => "단계가 실패해 해당 머신을 이전 실행으로 복구했습니다. 상세 진단은 coordinator의 배포 기록에서 확인하세요.",
                RolloutState::RecoveryFailed => "배포와 자동 복구를 확인하지 못했습니다. coordinator가 배포 기록과 해당 머신을 확인해야 합니다.",
                _ => "배포 단계에서 오류가 발생했습니다. 상세 진단은 coordinator의 배포 기록에서 확인하세요.",
            }.into()),
            recovery_phase: r.recovery.as_ref().map(|g| g.phase),
            recovery_sha256: r.recovery.as_ref().map(|g| g.spec.baseline.sha256.clone()),
            created_ms: r.created_ms,
            updated_ms: r.updated_ms,
        }
    }
    pub(super) fn announce_rollout_finished(&self, r: &RolloutView, item: &InboxItem) {
        // 내구 정본/inbox 뒤 투영/깨움, 앱 안 알림. 외부 push/webhook은 호출하지 않는다.
        self.inbox_publish(item);
        self.send_notice(NoticeView {
            kind: if r.state == RolloutState::Done { NoticeKind::RolloutFinished } else { NoticeKind::RolloutFailed },
            pane: None,
            machine: String::new(),
            project: Some(r.project.clone()),
            thread: None,
            title: item.summary.clone(),
            agent: None,
            turn_ms: None,
            long: true,
            at_ms: item.created_at_ms,
        });
    }
}

pub(super) fn completion_item(r: &RolloutView) -> Option<InboxItem> {
    if recovery::active(r.state) {
        return None;
    }
    let what = if r.spec.restart.is_some() { "재시작" } else { "배포" };
    let state = match r.state {
        RolloutState::Done => format!("{what} 완료"),
        RolloutState::StoppedRolledBack => format!("{what} 실패 · 복구 완료"),
        RolloutState::RecoveryFailed => format!("{what} 실패 · 복구 확인 필요"),
        _ => format!("{what} 중단"),
    };
    Some(InboxItem {
        short_id: None,
        id: format!("rollout-{}", r.id),
        project: r.project.clone(),
        item_kind: "rollout".into(),
        subject: r.id.clone(),
        event: "rollout finished".into(),
        summary: format!("{state} · {}", r.id.get(..8).unwrap_or(&r.id)),
        // inbox peek는 Reader도 읽는다. 원본 진단은 rollout 정본에만 보존한다.
        body: format!(
            "rollout: {}\nSHA-256: {}\n자세한 결과는 coordinator의 배포 기록에서 확인하세요.",
            r.id, r.spec.sha256
        ),
        done: false,
        seen: false,
        created_at_ms: aaa_common::util::now_ms(),
        done_at_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lab() -> (Arc<Host>, aaa_config::HostConfig, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("aaa-rollout-card-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        (Host::open(cfg.clone(), root.join("host")).unwrap(), cfg, root)
    }
    #[tokio::test]
    async fn saved_rollout_wakes_readers_and_projects_only_display_fields() {
        let (h, cfg, root) = lab();
        assert_eq!(h.rollout_card().unwrap(), None);
        let mut updates = h.rollout_changed.subscribe();
        let mut r = crate::rollout::tests::record();
        r.actor = "private-actor-marker".into();
        r.spec.from = "/private/candidate-marker".into();
        r.error =
            Some("staging 생성 실패 /private/install-marker/.aaa-update-987654.tmp: self-check pid-marker".into());
        r.cursor = 6;
        for step in &mut r.steps[..6] {
            step.completed = true;
        }
        h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
        h.save_rollout(&mut r).unwrap();
        assert!(updates.has_changed().unwrap(), "DB commit 뒤 조회 화면을 깨워야 한다");
        updates.borrow_and_update();
        let card = h.rollout_card().unwrap().unwrap();
        assert_eq!(card.id, r.id);
        assert_eq!(card.state, RolloutState::Running);
        assert_eq!(card.cursor, 6);
        assert!(card.steps[..6].iter().all(|s| s.completed) && !card.steps[6].completed);
        let json = serde_json::to_string(&card).unwrap();
        for private in [&r.actor, &r.spec.from, &r.steps[6].job_id, "install-marker", "987654", "pid-marker"] {
            assert!(!json.contains(private), "저장된 내부 실행 자료를 웹에 내보내면 안 된다");
        }
        assert_eq!(h.load_rollout(&r.id).unwrap().error, r.error, "coordinator 정본은 진단을 보존한다");
        drop(h);
        let reopened = Host::open(cfg, root.join("host")).unwrap();
        assert_eq!(reopened.rollout_card().unwrap(), Some(card));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn legacy_terminal_does_not_backfill_inbox_or_recreate_project() {
        let (h, _, root) = lab();
        let mut r = crate::rollout::tests::record();
        r.state = RolloutState::Stopped;
        h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
        h.drive_rollout().await.unwrap();
        assert!(h.db.inbox_list(Some(&r.project), true).unwrap().is_empty(), "옛 종료를 새로 알리지 않는다");
        assert!(!root.join("projects").join(&r.project).exists(), "지운 프로젝트를 다시 만들지 않는다");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn terminal_commit_recovery_emits_one_durable_inbox_and_app_notice() {
        let (h, cfg, root) = lab();
        std::fs::create_dir_all(root.join("projects/demo")).unwrap();
        std::fs::write(root.join("projects/demo/PROJECT.md"), "# demo\n").unwrap();
        let mut notices = h.notice_events.subscribe();
        for state in
            [RolloutState::Done, RolloutState::Stopped, RolloutState::StoppedRolledBack, RolloutState::RecoveryFailed]
        {
            let mut r = crate::rollout::tests::record();
            r.state = state;
            r.error =
                Some("staging /private/reader-install-marker/.aaa-update-987654.tmp: self-check pid-marker".into());
            r.cursor = if state == RolloutState::Done { r.steps.len() as u32 } else { 6 };
            for step in r.steps.iter_mut().take(r.cursor as usize) {
                step.completed = true;
            }
            // 정상 terminal 저장에서만 앱 알림을 한 번 발행한다.
            h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
            h.save_rollout(&mut r).unwrap();
            let n = notices.try_recv().unwrap();
            assert_eq!(
                n.kind,
                if state == RolloutState::Done { NoticeKind::RolloutFinished } else { NoticeKind::RolloutFailed }
            );
            let mut items = h.db.inbox_list(Some(&r.project), true).unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].subject, r.id);
            assert_eq!(items[0].item_kind, "rollout", "외부 사용자 post 알림과 구분한다");
            assert!(items[0].body.contains(&r.spec.sha256));
            let response = h
                .call(
                    DeviceRole::Reader,
                    "web:reader",
                    FrontRequest::Inbox { project: r.project.clone(), all: true, peek: true },
                )
                .await
                .unwrap();
            let FrontResponse::Inbox { items: reader_items, .. } = response else {
                panic!("Reader inbox 응답이 아니다")
            };
            assert_eq!(reader_items.len(), 1);
            let wire = serde_json::to_string(&reader_items).unwrap();
            for private in ["reader-install-marker", "987654", "pid-marker"] {
                assert!(!wire.contains(private), "Reader inbox로 원문 진단이 우회 노출됐다: {private}");
            }
            assert_eq!(h.load_rollout(&r.id).unwrap().error, r.error, "coordinator 정본은 원문을 보존한다");
            h.save_rollout(&mut r).unwrap();
            h.drive_rollout().await.unwrap();
            assert!(notices.try_recv().is_err(), "같은 종료를 두 번 알리면 안 된다");
            assert_eq!(h.db.inbox_list(Some(&r.project), true).unwrap().len(), 1);
            let item = &mut items[0];
            item.done = true;
            item.done_at_ms = Some(aaa_common::util::now_ms());
            h.db.inbox_put(item).unwrap();
            h.db.inbox_prune(item.done_at_ms.unwrap() + 1).unwrap();
            assert!(h.db.inbox_list(Some(&r.project), true).unwrap().is_empty());
            h.save_rollout(&mut r).unwrap();
            assert!(notices.try_recv().is_err(), "정리 뒤 같은 저장도 재발행하지 않는다");
            assert!(h.db.inbox_list(Some(&r.project), true).unwrap().is_empty());
        }
        // terminal+inbox commit 뒤 앱 알림 전에 중단한 경계: 재시작은 내구 inbox만 복원한다.
        let mut r = crate::rollout::tests::record();
        r.state = RolloutState::Stopped;
        let item = completion_item(&r).unwrap();
        h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
        assert!(h.db.save_rollout_record(&r.id, &serde_json::to_string(&r).unwrap(), Some(&item)).unwrap());
        drop(h);
        let reopened = Host::open(cfg, root.join("host")).unwrap();
        let mut notices = reopened.notice_events.subscribe();
        reopened.drive_rollout().await.unwrap();
        assert!(notices.try_recv().is_err(), "inbox를 정리하고 재시작해도 이미 알린 종료는 반복하지 않는다");
        let items = reopened.db.inbox_list(Some("demo"), true).unwrap();
        assert_eq!(items.len(), 1, "commit한 종료 inbox는 재시작 뒤에도 남는다");
        assert_eq!(items[0].subject, r.id);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
