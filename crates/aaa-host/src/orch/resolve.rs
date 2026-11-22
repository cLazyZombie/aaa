//! thread 확인·resolve·다시 열기.

use super::*;

impl Host {
    pub(crate) fn thread_ack(&self, project: &str, id: &str, actor: &str) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        let Some(h) = rec.report_hash.clone() else { return Err(bad("아직 보고가 없다")) };
        if rec.acked_hash.as_ref() == Some(&h) {
            return Ok(self.thread_view(&rec));
        }
        let short = h.chars().take(12).collect::<String>();
        let rec = self.update_rec(project, id, |r| r.acked_hash = Some(h))?;
        self.db.audit(actor, "thread_ack", &format!("{project}/{id}"));
        self.flow(&rec, "review_acked", Some(format!("threads/{id}.md")), format!("보고 {short}"));
        self.tick_wake.notify_one();
        Ok(self.thread_view(&rec))
    }

    /// 끝낸다: 마지막 사본(실패면 거부) → 상태를 끝남으로 → 세션 종료 → 감시 해제 → worktree 정리(force 없음, 병합된 branch만
    /// 삭제) → 「resolved」 항목(herdr-projects threads.rs `resolve`). 상태를 세션보다 먼저 써서, 그 사이 ticker가 닫힌 pane을
    /// 확인 필요로 보지 않는다.
    pub(crate) async fn thread_resolve(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        opts: ResolveOpts,
        why: ResolveWhy,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let _guard = self.claim_thread(project, id)?;
        let mut rec = self.load(project, id)?;
        if matches!(why, ResolveWhy::Manual) {
            crate::followups::check_cli_resolve(&rec, actor)?;
        }
        if rec.status == ThreadStatus::Starting {
            rec = self.recover_thread_start(&rec)?;
            if rec.status == ThreadStatus::Starting {
                return Err(bad("기동 결과가 아직 불명이다. 실제 세션과 대조한 뒤 끝낸다"));
            }
        }
        if rec.status == ThreadStatus::Resolved && rec.resolve_pending.is_none() {
            return Ok(self.thread_view(&rec));
        }
        let opts = rec.resolve_pending.unwrap_or(opts);
        let conn = self.live_conn(&rec.machine_id)?;
        if rec.cwd.is_none() && rec.status == ThreadStatus::Failed && rec.kind != ThreadKind::Adopted {
            if !self.machine_feature(&rec.machine_id, "thread_journal.v1") {
                return Err(bad(
                    "준비 자리를 조회할 수 없는 agent다. agent를 갱신한 뒤 resolve한다(확인 없이 정리 완료로 두지 않는다)",
                ));
            }
            match conn
                .request(AgentRequest::ThreadPlacement {
                    project: project.into(),
                    thread: id.into(),
                    title: rec.title.clone(),
                    kind: rec.kind,
                    repo: rec.repo.clone(),
                })
                .await?
            {
                AgentResponse::ThreadPlacement { placement: Some(place) } => {
                    rec = self.update_rec(project, id, |r| {
                        r.cwd = Some(place.cwd);
                        r.work_dir = Some(place.work_dir);
                        r.branch = place.branch;
                        r.base = place.base;
                    })?;
                }
                AgentResponse::ThreadPlacement { placement: None } => {}
                other => return Err(internal(format!("준비 자리 응답이 아니다: {other:?}"))),
            }
        }
        let report_lock = self.report_collect_lock(project, id);
        let report_guard = report_lock.lock().await;
        if rec.status != ThreadStatus::Resolved && !opts.skip_copy && rec.work_dir.is_some() {
            let has_report = self.current_report_hash(&rec, &conn).await.is_ok();
            if has_report {
                self.collect(project, id, true)
                    .await
                    .map_err(|e| {
                        bad(format!(
                            "마지막 사본이 실패해 resolve하지 않는다: {} (`--skip-copy`면 사본 없이 끝내고 worktree는 남긴다)",
                            e.message
                        ))
                    })?;
            }
        }
        // 마지막 사본에서 새 후속을 받았을 수 있다. 종료 상태·pane·worktree를 바꾸기 전에 다시 확인한다.
        rec = self.load(project, id)?;
        let last_report = self.verified_report(&rec);
        if matches!(why, ResolveWhy::Manual) {
            crate::followups::check_cli_resolve(&rec, actor)?;
        }
        let rec = self.update_rec(project, id, |r| {
            if let (Some(revision), Some(body)) = (r.report_hash.clone(), last_report.as_deref()) {
                crate::followups::capture(r, &revision, body);
            }
            r.status = ThreadStatus::Resolved;
            r.resolve_pending = Some(opts);
            // 이어진 task 판정의 까닭은 처음 끝낸 까닭이다: 정리가 끊겨 `aaa thread resolve`로 이어 끝내도 병합은 병합이다
            if r.task_clean.why.is_none() {
                r.task_clean.why = Some(why.as_str().into());
            }
            r.task_clean.discard |= opts.skip_copy || opts.discard_uncopied;
            r.error = Some("정리 중: 응답을 기록할 때까지 다시 resolve로 이어갈 수 있다".into());
            r.last_group = Some(ThreadGroup::Resolved);
        })?;
        drop(report_guard);
        if let Some(revision) = &rec.report_hash {
            let path = self.project_dir(project)?.join("threads").join(format!("{id}.md"));
            match std::fs::read_to_string(path) {
                Ok(body) => self.complete_thread_report(project, id, &body, revision)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(crate::library_publish::io_error(e)),
            }
        }
        self.followup_notice(&rec);
        self.db.audit(actor, "thread_resolve", &format!("{project}/{id}"));
        // 그 사본이 「진행 중」(지울 수 없음)에서 풀린다: 보고서 화면이 다시 읽는다(plan §8.256)
        self.reports_bump();
        // 데려온 폴더는 사람 것이다: pane은 닫되 그 폴더·branch는 정리하지 않는다.
        let adopted = rec.kind == ThreadKind::Adopted;
        // pane을 닫을 때 끝나기를 기다릴 사건 구독은 끝내기 전에 잡는다(그 사이 온 끝남을 놓치지 않는다)
        let pane_rx = rec.pane.as_deref().and_then(|p| {
            self.pane_events(p)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        });
        if let Some(pane) = rec.pane.as_ref()
            && self.pane_view_of(pane, aaa_proto::DeviceRole::Local).is_some_and(|p| p.state.is_running())
        {
            aaa_common::diagnostic::ignored(
                self.kill_session(&conn, pane).await,
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        aaa_common::diagnostic::ignored(
            conn.request(AgentRequest::ThreadUnwatch { thread: format!("{project}/{id}") }).await,
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
        let mut notes = Vec::new();
        let mut cleanup_error = None;
        let mut cleanup_head = None;
        if adopted {
            notes.push("폴더·branch는 그대로 둔다(데려온 폴더는 사람 것)".to_string());
        }
        // 보류 뒤 다른 머신에서 이으며 남긴 자리는 지금 머신 정리가 닿지 않는다(park.rs)
        for left in &rec.left_places {
            notes.push(format!(
                "앞 자리 {} `{}`는 남아 있다(그 머신에서 지운다)",
                self.machine_name(&left.machine_id),
                left.cwd.as_deref().unwrap_or("-")
            ));
        }
        // 집에 다 오지 않았으면 worktree를 남긴다(herdr-projects `clean` `copy_complete`): 사본에서 빠진 것(symlink·한도를
        // 넘은 파일)과 사본 없이 끝낸 작업 폴더(`.aaa/`)는 git이 무시하는 파일이라 force 없는 `worktree remove`도 지운다
        let uncopied = rec.kind == ThreadKind::Worktree
            && !opts.keep_worktree
            && !opts.discard_uncopied
            && (opts.skip_copy || rec.copy.as_deref() == Some("partial"));
        if uncopied {
            notes.push(if opts.skip_copy {
                "마지막 사본 없이 끝냈다".to_string()
            } else {
                format!("집에 오지 않은 것: {}", copy_notes_summary(&rec.copy_notes))
            });
        }
        if let Some(cwd) = rec.cwd.as_ref().filter(|_| !adopted) {
            let repo = if rec.kind == ThreadKind::Worktree { rec.repo.clone() } else { None };
            let merged_head = if repo.is_some() { self.main_branch_head(&rec).await } else { None };
            match conn
                .request(AgentRequest::ThreadCleanup {
                    repo,
                    cwd: cwd.clone(),
                    branch: rec.branch.clone(),
                    merged_head: merged_head.clone(),
                    // 옛 agent는 `uncopied`를 모른다: 남기기를 함께 켜야 지우지 않는다
                    keep_worktree: opts.keep_worktree || uncopied,
                    uncopied,
                })
                .await
            {
                Ok(AgentResponse::Cleanup(c)) => {
                    if c.worktree_removed && c.branch_deleted && !opts.keep_worktree && !uncopied {
                        cleanup_head = merged_head;
                    }
                    // 사람이 남기라고 한 worktree(--keep-worktree)·집에 다 오지 않아 남긴 worktree와 그 branch는 청소하지
                    // 않는다(참고 sweep `kept_worktree`, `clean` `copy_complete`)
                    let kept = opts.keep_worktree || uncopied;
                    let left = crate::lifecycle::Leftover {
                        worktree: rec.kind == ThreadKind::Worktree && !kept && !c.worktree_removed,
                        branch: rec.branch.is_some() && !kept && !c.branch_deleted,
                    };
                    aaa_common::diagnostic::ignored(
                        self.update_rec(project, id, |r| r.leftover = Some(left)),
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                    notes.push(format!(
                        "worktree {}, branch {}",
                        if c.worktree_removed { "삭제" } else { "유지" },
                        if c.branch_deleted { "삭제" } else { "유지" }
                    ));
                    notes.extend(c.notes);
                }
                Ok(other) => cleanup_error = Some(internal(format!("정리 응답이 아니다: {other:?}"))),
                Err(e) => cleanup_error = Some(e),
            }
        }
        // pane 수명과 폴더 보존은 별개다. 이미 끝난 pane·남긴 worktree·adopted도 기록과 layout에서 지운다.
        if let Some(pane) = &rec.pane {
            // 끝내기는 위에서 보냈다: 끝남을 기다려 layout과 기록에서 지운다(`pane close`의 뒷부분)
            let closed = match pane_rx {
                None => Ok(false),
                Some(rx) => {
                    match self
                        .wait_pane(pane, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다")
                        .await
                    {
                        Ok(()) => self.forget(pane, &rec.machine_id, actor).await.map(|_| true),
                        Err(e) => Err(e),
                    }
                }
            };
            notes.push(match closed {
                Ok(true) => "pane을 닫았다".into(),
                Ok(false) => "pane은 이미 닫혔다".into(),
                Err(e) => format!("pane을 닫지 못했다: {}", e.message),
            });
        }
        if let Some(e) = cleanup_error {
            let message = format!("정리 미완료: {}. `aaa thread resolve {project} {id}`로 이어서 한다", e.message);
            aaa_common::diagnostic::ignored(
                self.update_rec(project, id, |r| r.error = Some(message.clone())),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
            return Err(ErrorBody { message, ..e });
        }
        // resolve_pending 중에는 확인 카드를 만들지 않는다. pane 정리가 끝난 뒤 병합을 확인하고
        // 그 근거와 종료 확정을 함께 저장해야 ticker 전에도 불필요한 확인 카드가 보이지 않는다.
        let merged_main = self.task_main_merge(&rec).await;
        // 이어진 task 완료 보관은 ticker가 한다.
        let resolved = self.update_rec(project, id, |r| {
            if merged_main.is_some() {
                r.task_clean.merged_main = merged_main.clone();
            }
            if let Some(head) = &cleanup_head {
                crate::followups::resolve_cleanup(r, head);
            }
            r.resolve_pending = None;
            r.error = None;
        })?;
        self.followup_notice(&resolved);
        self.tick_wake.notify_one();
        self.flow(
            &rec,
            "thread_resolved",
            rec.report_hash.as_ref().map(|_| format!("threads/{id}.md")),
            notes.join("; "),
        );
        let mut summary = match why {
            ResolveWhy::Auto { days: None } => format!(
                "{}: 자동화 {}의 완료 뒤 쉼 시간이 지나 저절로 끝냈다",
                crate::transitions::label(&rec),
                rec.automation.as_deref().unwrap_or("")
            ),
            ResolveWhy::Manual => format!("{}: 끝냈다(보고와 library는 남는다)", crate::transitions::label(&rec)),
            // herdr-projects steps.rs `resolve_after_copy`의 auto 문구
            ResolveWhy::Auto { days: Some(days) } => format!(
                "{}: 쉼으로 auto_resolve_days({days}일)가 지나 저절로 끝냈다. `aaa thread resolve {project} {id} --reopen`으로 되돌린다",
                crate::transitions::label(&rec)
            ),
        };
        if !notes.is_empty() {
            summary.push_str(&format!(": {}", notes.join("; ")));
        }
        // 대화 전용 thread는 사용자 모르게 바뀐다(보류와 같다, park.rs): 진행 모델을 바꾸거나 버려 끝낸 것은 알림함 항목을 내지 않는다
        if !rec.chat_only() {
            self.inbox_add(project, "thread-state", id, "resolved", &summary);
        }
        Ok(self.thread_view(&self.load(project, id).unwrap_or(rec)))
    }

    /// 끝낸 thread를 다시 연다(herdr-projects `thread resolve --reopen`): 기록만 열린 것으로 돌리고 아무것도 띄우지 않는다.
    /// 지워진 worktree는 돌아오지 않는다(작업 폴더 thread는 남아 있다).
    pub(crate) fn thread_reopen(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        // 다시 열린 작업의 사본을 이전 정리 계획이 지우지 않도록 상태 전이와 정리를 직렬화한다.
        let library_ops = self.library_ops.lock().unwrap();
        let rec = self.load(project, id)?;
        if rec.resolve_pending.is_some() {
            return Err(bad("정리 미완료다. 먼저 같은 thread resolve로 마친다"));
        }

        if rec.status != ThreadStatus::Resolved {
            return Err(bad(format!("{id}는 끝낸 thread가 아니다(지금 {})", rec.status.as_str())));
        }
        self.db
            .thread_restart_claim(
                project,
                id,
                self.settings(project).max_parallel_threads,
                self.automation_restart_admission(&rec)?.as_ref(),
            )
            .map_err(|e| err(ErrorCode::Unavailable, e.to_string()))?;
        let rec = self.update_rec(project, id, |r| {
            r.status = ThreadStatus::Open;
            // 다음 셈은 처음 보는 것으로 친다(전이 항목을 내지 않는다). 병합을 본 때도 다시 잰다
            r.last_group = None;
            // 이어진 task는 다음 끝남에서 다시 판정한다(아직 판정 전이면 하지 않는다)
            r.task_clean = TaskClean {
                followups: r.task_clean.followups.clone(),
                followup_revisions: r.task_clean.followup_revisions.clone(),
                ..Default::default()
            };
        })?;
        drop(library_ops);
        self.db.audit(actor, "thread_reopen", &format!("{project}/{id}"));
        self.reports_bump();
        self.flow(&rec, "thread_reopened", None, "다시 열었다(아무것도 띄우지 않았다)");
        let mut view = self.thread_view(&rec);
        view.warnings
            .push(format!("아무것도 띄우지 않았다. `aaa thread restart {project} {id}`가 agent를 다시 띄운다"));
        Ok(view)
    }

    /// agent가 다시 붙었다: 그 머신의 열린 thread 보고 감시를 다시 건다(놓친 변화는 등록 즉시 hash로 알 수 있다)
    pub(crate) fn rewatch(self: &Arc<Self>, mid: String) {
        let me = self.clone();
        tokio::spawn(async move {
            let Some(conn) = me.conn_of(&mid) else { return };
            let Ok(rows) = me.db.threads(None).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                return;
            };
            for r in rows.iter().filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            }) {
                // 아직 brief를 받지 않은 thread는 다시 보기 시작한다(host가 다시 뜬 뒤에도, herdr-projects ticker
                // `brief_pass`처럼 기록에서 다시 찾는다)
                if r.machine_id == mid && r.status == ThreadStatus::Open && r.brief == BriefState::Pending {
                    me.spawn_brief(&r.project, &r.id);
                }
                if r.machine_id == mid
                    && r.status == ThreadStatus::Open
                    && let Some(w) = r.work_dir
                {
                    aaa_common::diagnostic::ignored(
                        conn.request(AgentRequest::ThreadWatch {
                            thread: format!("{}/{}", r.project, r.id),
                            work_dir: w,
                        })
                        .await,
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                }
            }
        });
    }
}
