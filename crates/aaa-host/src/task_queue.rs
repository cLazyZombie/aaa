//! host 예약. Markdown은 내용, SQLite는 승인판·차례·기동 의도의 정본이다.
use crate::orch::{StartArgs, internal};
use crate::{Host, err, file_rev, tasks};
use aaa_proto::{
    ErrorBody, ErrorCode, FrontResponse, QueueOp, QueueState as S, TaskAnchor, TaskOp, TaskReservation, TaskView,
    ThreadStatus,
};
use std::sync::Arc;

pub(crate) fn split_id(text: &str) -> (&str, Option<&str>) {
    if let Some((body, tail)) = text.trim_end().rsplit_once(" <!-- aaa-task:")
        && let Some(id) = tail.strip_suffix(" -->")
        && id.len() == 26
        && id.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return (body, Some(id));
    }
    (text, None)
}
pub(crate) fn id_suffix(id: Option<&str>) -> String {
    id.map(|id| format!(" <!-- aaa-task:{id} -->")).unwrap_or_default()
}
fn identify(text: &str) -> Result<String, ErrorBody> {
    let mut ids = std::collections::HashSet::new();
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if !tasks::is_task(line) {
            out.push_str(line);
            continue;
        }
        if let Some(id) = split_id(line).1 {
            if !ids.insert(id.to_string()) {
                return Err(err(ErrorCode::BadRequest, "중복 task ID"));
            }
            out.push_str(line);
        } else {
            let body = line.trim_end_matches(['\r', '\n']);
            let id = aaa_common::util::new_id();
            ids.insert(id.clone());
            out.push_str(body);
            out.push_str(&id_suffix(Some(&id)));
            out.push_str(&line[body.len()..]);
        }
    }
    Ok(out)
}
fn at(t: &TaskView) -> TaskAnchor {
    TaskAnchor { list: t.list.clone(), title: t.title.clone(), block_sha256: t.block_sha256.clone(), line: t.line }
}
fn empty_ctx() -> crate::task_edit::Ctx<'static> {
    crate::task_edit::Ctx {
        owner_ok: &|_, _| Ok(()),
        profiles: &[],
        repo_ok: &|s| Ok(s.into()),
        removed: &|_| Err("삭제 복원 경로가 아니다".into()),
    }
}

pub(crate) fn model_selection(host: &Host, profile: &str) -> Result<(String, String, String), ErrorBody> {
    let FrontResponse::ProfileResolved { profile: p } = host.profile_resolve(Some(profile))? else {
        return Err(internal("모델 해석 응답이 아니다"));
    };
    let revision =
        tasks::sha256(&serde_json::to_string(&(&p.harness, &p.model, &p.effort, &p.args)).map_err(internal)?);
    Ok((p.model.unwrap_or_default(), p.effort.unwrap_or_default(), revision))
}

impl Host {
    /// 1A: coordinator 자기 신고는 사용자 변경 승인이 아니다. API 원문 쓰기도 같은 정책이다.
    pub(crate) fn protect_task_choices(&self, before: &str, after: &str, actor: &str) -> Result<(), ErrorBody> {
        if !actor.starts_with("coordinator:") {
            return Ok(());
        }
        let profiles: Vec<String> = self.projects().iter().flat_map(|p| self.known_profiles(&p.name)).collect();
        let newer = tasks::parse(after);
        let mut approved = Vec::new();
        for project in self.projects() {
            approved.extend(self.db.queue_list(&project.name).map_err(internal)?);
        }
        for old in tasks::parse(before) {
            let new = newer.iter().find(|t| match &old.id {
                Some(id) => t.id.as_ref() == Some(id),
                None => t.title == old.title,
            });
            let Some(new) = new.or_else(|| (newer.len() == 1).then(|| &newer[0])) else {
                continue;
            };
            // 빈 칸으로 승인한 기본 모델·자동 머신도 예약 뒤에는 고정 선택이다.
            if old.owner != new.owner
                && approved.iter().any(|q| {
                    old.id.as_ref() == Some(&q.task_id)
                        && matches!(q.state, S::Queued | S::Check | S::Starting | S::Running)
                })
            {
                return Err(err(
                    ErrorCode::Forbidden,
                    "예약에서 승인한 모델·머신은 사용자가 웹이나 aaa 밖 CLI에서 직접 바꾼다",
                ));
            }
            let (op, om) = tasks::agent_parts(old.owner.as_deref(), &profiles).unwrap_or_default();
            let (np, nm) = tasks::agent_parts(new.owner.as_deref(), &profiles).unwrap_or_default();
            if (op.is_some() && op != np) || (om.is_some() && om != nm) {
                return Err(err(ErrorCode::Forbidden, "정해진 모델·머신은 사용자가 웹이나 aaa 밖 CLI에서 직접 바꾼다"));
            }
        }
        Ok(())
    }

    /// 사용자 API의 선택 변경만 새 출처로 인정한다. 파일 직접 수정·coordinator 자기 승인은 근거가 아니다.
    pub(crate) fn record_task_choices(
        &self,
        project: &str,
        before: &str,
        after: &str,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        if actor == "host" || actor.starts_with("coordinator:") {
            return Ok(());
        }
        let old = tasks::parse(before);
        let mut new = tasks::parse(after);
        tasks::fill_agent(&mut new, &self.known_profiles(project));
        self.db
            .queue_change(project, actor, |rows| {
                for q in rows {
                    let a = old.iter().find(|t| t.id.as_ref() == Some(&q.task_id));
                    let b = new.iter().find(|t| t.id.as_ref() == Some(&q.task_id));
                    if let (Some(a), Some(b)) = (a, b)
                        && a.owner != b.owner
                    {
                        q.selection = aaa_proto::TaskChoicePin {
                            owner: b.owner.clone(),
                            profile: b.profile.clone().unwrap_or_else(|| self.default_thread_profile(project)),
                            machine: b.machine.clone(),
                            actor: actor.into(),
                            at_ms: aaa_common::util::now_ms(),
                        };
                    }
                }
                Ok(())
            })
            .map_err(internal)
    }

    /// 명시 쓰기에서만 최소 ID 이행. 파일 밖 바이트·메모·줄 끝을 보존하며 이력도 남긴다.
    pub(crate) fn task_identify(&self, project: &str) -> Result<(), ErrorBody> {
        let dir = self.project_dir(project)?;
        let rev = file_rev::read(&dir.join("TASKS.md")).map_err(internal)?;
        let text = identify(&rev.text)?;
        if text == rev.text {
            return Ok(());
        }
        let (conflict, _) = self.task_write(
            project,
            &dir,
            &[TaskOp::RawSave { expected_sha256: rev.sha256, text }],
            None,
            "host",
            &empty_ctx(),
            false,
        )?;
        if conflict.is_some() {
            return Err(err(ErrorCode::Conflict, "ID 부여 중 task가 바뀌었다. 다시 읽는다"));
        }
        Ok(())
    }

    pub(crate) async fn task_queue(
        self: &Arc<Self>,
        project: &str,
        action: QueueOp,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        if matches!(action, QueueOp::List) {
            return Ok(FrontResponse::TaskQueue { entries: self.db.queue_list(project).map_err(internal)? });
        }
        let by = self.task_writer(project, actor)?;
        self.check_active(project)?;
        let _gate = self.queue_gate.lock().await;
        match action {
            QueueOp::List => unreachable!(),
            QueueOp::Reserve { task, block, profile, profile_revision: approved_profile } => {
                let board = self.task_board(project)?;
                let old = tasks::find(&board.tasks, &task)
                    .or_else(|| board.tasks.iter().find(|t| t.id.as_deref() == Some(&task)))
                    .ok_or_else(|| err(ErrorCode::NotFound, "task가 없다"))?;
                if old.id.as_ref().is_some_and(|id| {
                    self.db.queue_list(project).is_ok_and(|rows| {
                        rows.iter().any(|r| {
                            &r.task_id == id && r.approved_block == block && matches!(r.state, S::Starting | S::Running)
                        })
                    })
                }) {
                    return Ok(FrontResponse::TaskQueue { entries: self.db.queue_list(project).map_err(internal)? });
                }
                if old.block_sha256 != block {
                    return Err(err(ErrorCode::Conflict, "예약할 task가 바뀌었다. 내용을 다시 확인한다"));
                }
                if old.done || !old.problems.is_empty() || old.repo.is_none() || old.notes.is_empty() {
                    return Err(err(
                        ErrorCode::BadRequest,
                        "예약 전에 설명·완료 범위 메모와 저장소(또는 작업 폴더)를 정하고 형식 문제를 고친다",
                    ));
                }
                if let Some(tid) = &old.thread
                    && self.load(project, tid).is_ok_and(|r| {
                        matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) || r.resolve_pending.is_some()
                    })
                {
                    return Err(err(ErrorCode::Conflict, format!("이미 {tid}가 맡고 있다")));
                }
                let previous = self
                    .db
                    .queue_list(project)
                    .map_err(internal)?
                    .into_iter()
                    .find(|q| old.id.as_ref() == Some(&q.task_id));
                if by.starts_with("coordinator:")
                    && previous.as_ref().is_some_and(|q| {
                        q.selection.owner != old.owner
                            || q.selection.profile != profile
                            || q.selection.machine != old.machine
                    })
                {
                    return Err(err(
                        ErrorCode::Forbidden,
                        "모델·머신 변경의 사용자 출처가 없다. 사용자가 직접 변경/예약한다",
                    ));
                }
                let expected = old.profile.clone().unwrap_or_else(|| self.default_thread_profile(project));
                if profile != expected {
                    return Err(err(ErrorCode::Conflict, "확인한 기본/고정 모델이 바뀌었다. 다시 확인한다"));
                }
                let (model, effort, profile_revision) = model_selection(self, &profile)?;
                if profile_revision != approved_profile {
                    return Err(err(ErrorCode::Conflict, "확인한 모델·effort 설정이 바뀌었다"));
                }
                let title = old.title.clone();
                self.task_identify(project)?;
                let board = self.task_board(project)?;
                let mut t = tasks::find(&board.tasks, &title)
                    .ok_or_else(|| err(ErrorCode::Conflict, "task가 바뀌었다"))?
                    .clone();
                // ID 이행 외에 내용이 바뀌었으면 이전 승인으로 등록하지 않는다.
                if (
                    old.title.clone(),
                    old.owner.clone(),
                    old.repo.clone(),
                    old.notes.clone(),
                    old.thread.clone(),
                    old.done,
                ) != (t.title.clone(), t.owner.clone(), t.repo.clone(), t.notes.clone(), t.thread.clone(), t.done)
                {
                    return Err(err(ErrorCode::Conflict, "예약 중 task 내용이 바뀌었다"));
                }
                if t.thread.is_some() {
                    self.task_write(
                        project,
                        &self.project_dir(project)?,
                        &[TaskOp::Update {
                            at: at(&t),
                            title: None,
                            owner: None,
                            person: false,
                            profile: None,
                            machine: None,
                            notes: None,
                            thread: Some(String::new()),
                            done: None,
                            repo: None,
                        }],
                        None,
                        "host",
                        &empty_ctx(),
                        false,
                    )?;
                    t = tasks::find(&self.task_board(project)?.tasks, &title)
                        .ok_or_else(|| internal("연결 해제 뒤 task가 없다"))?
                        .clone();
                }
                let now = aaa_common::util::now_ms();
                let id = t.id.clone().ok_or_else(|| internal("task ID가 없다"))?;
                self.db
                    .queue_change(project, &by, |rows| {
                        let position = rows
                            .iter()
                            .filter(|r| matches!(r.state, S::Queued | S::Check))
                            .map(|r| r.position)
                            .max()
                            .unwrap_or(0)
                            + 1;
                        let existing = rows.iter().position(|r| r.task_id == id);
                        if let Some(i) = existing
                            && matches!(rows[i].state, S::Starting | S::Running)
                        {
                            return Ok(());
                        }
                        let updating = existing.filter(|i| matches!(rows[*i].state, S::Queued | S::Check));
                        let selection = if by.starts_with("coordinator:") {
                            previous.as_ref().map(|q| q.selection.clone())
                        } else {
                            None
                        }
                        .unwrap_or_else(|| aaa_proto::TaskChoicePin {
                            owner: t.owner.clone(),
                            profile: profile.clone(),
                            machine: t.machine.clone(),
                            actor: by.clone(),
                            at_ms: now,
                        });
                        let row = TaskReservation {
                            selection,
                            task_id: id,
                            approved_block: t.block_sha256.clone(),
                            profile,
                            model,
                            effort,
                            profile_revision,
                            machine: t.machine.clone(),
                            task: t,
                            position: updating.map_or(position, |i| rows[i].position),
                            state: S::Queued,
                            reason: "자리 대기".into(),
                            launch_id: None,
                            thread: None,
                            actor: by.clone(),
                            created_ms: updating.map_or(now, |i| rows[i].created_ms),
                            updated_ms: now,
                        };
                        if let Some(i) = existing {
                            rows[i] = row;
                        } else {
                            rows.push(row);
                        }
                        Ok(())
                    })
                    .map_err(internal)?;
            }
            QueueOp::Cancel { task_id } => self
                .db
                .queue_change(project, &by, |rows| {
                    let row =
                        rows.iter_mut().find(|r| r.task_id == task_id).ok_or_else(|| anyhow::anyhow!("없는 예약"))?;
                    if matches!(row.state, S::Queued | S::Check) {
                        row.state = S::Cancelled;
                        row.reason = "예약 취소".into();
                        row.updated_ms = aaa_common::util::now_ms();
                    }
                    Ok(())
                })
                .map_err(internal)?,
            QueueOp::Move { task_id, position } => self
                .db
                .queue_change(project, &by, |rows| {
                    let mut pending: Vec<_> = rows
                        .iter()
                        .filter(|r| matches!(r.state, S::Queued | S::Check))
                        .map(|r| r.task_id.clone())
                        .collect();
                    let old = pending
                        .iter()
                        .position(|id| id == &task_id)
                        .ok_or_else(|| anyhow::anyhow!("차례를 바꿀 예약이 없다"))?;
                    let id = pending.remove(old);
                    pending.insert(position.saturating_sub(1).min(pending.len() as u32) as usize, id);
                    for (i, id) in pending.iter().enumerate() {
                        let row = rows.iter_mut().find(|r| &r.task_id == id).unwrap();
                        row.position = i as u32 + 1;
                        row.updated_ms = aaa_common::util::now_ms();
                    }
                    Ok(())
                })
                .map_err(internal)?,
        }
        self.queue_changed(project);
        self.tick_wake.notify_one();
        Ok(FrontResponse::TaskQueue { entries: self.db.queue_list(project).map_err(internal)? })
    }

    fn queue_changed(&self, project: &str) {
        let _ = self.task_events.send((project.into(), "queue".into()));
    }
    fn queue_status(&self, project: &str, id: &str, state: S, reason: &str) -> Result<(), ErrorBody> {
        let changed = self
            .db
            .queue_change(project, "host", |rows| {
                let Some(r) = rows.iter_mut().find(|r| r.task_id == id) else { return Ok(false) };
                if r.state == state && r.reason == reason {
                    return Ok(false);
                }
                r.state = state;
                r.reason = reason.into();
                r.updated_ms = aaa_common::util::now_ms();
                Ok(true)
            })
            .map_err(internal)?;
        if changed {
            self.queue_changed(project);
        }
        Ok(())
    }

    /// 예약 외 직접 시작도 같은 재연결 대조를 쓴다. 호출자가 thread 작업 잠금을 소유한다.
    pub(crate) fn recover_thread_start(
        &self,
        rec: &crate::orch::ThreadRec,
    ) -> Result<crate::orch::ThreadRec, ErrorBody> {
        if self.restoring.lock().unwrap().contains(&rec.machine_id) {
            return Ok(rec.clone());
        }
        let matches: Vec<_> = {
            let i = self.inner.lock().unwrap();
            i.panes
                .iter()
                .filter(|(_, p)| {
                    p.machine_id == rec.machine_id
                        && p.info.name == format!("{}/{}", rec.project, rec.id)
                        && p.info.cwd == rec.cwd
                        && p.info.state.is_running()
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        if matches.len() == 1 {
            return self.update_rec(&rec.project, &rec.id, |r| {
                r.pane = matches.first().cloned();
                r.status = ThreadStatus::Open;
                r.error = None;
            });
        }
        if !rec.launch_sent {
            return self.update_rec(&rec.project, &rec.id, |r| {
                r.status = if r.launch_existing_slot { ThreadStatus::Open } else { ThreadStatus::Failed };
                r.error = Some("host가 기동 전에 중단됨 · 원래 자리 보존 · 명시 재예약/재시작 필요".into());
            });
        }
        Ok(rec.clone())
    }

    pub(crate) fn recover_launches(self: &Arc<Self>, project: &str) {
        for text in self.db.threads(Some(project)).unwrap_or_default() {
            let Ok(rec) = serde_json::from_str::<crate::orch::ThreadRec>(&text) else {
                continue;
            };
            if rec.status != ThreadStatus::Starting {
                continue;
            }
            let Ok(_claim) = self.claim_thread(project, &rec.id) else {
                continue;
            };
            if let Ok(r) = self.recover_thread_start(&rec)
                && r.status == ThreadStatus::Open
            {
                self.spawn_brief(project, &r.id);
            }
        }
    }

    pub(crate) async fn queue_pass(self: &Arc<Self>, project: &str) -> Result<(), ErrorBody> {
        let _gate = self.queue_gate.lock().await;
        let rows = self.db.queue_list(project).map_err(internal)?;
        if rows.is_empty() {
            return Ok(());
        }
        let owners =
            if rows.iter().any(|r| r.state == S::Queued) { self.assignable(project).await } else { Vec::new() };
        for row in rows {
            if row.state == S::Cancelled {
                continue;
            }
            if let Some(tid) = &row.thread {
                let mut r = self.load(project, tid)?;
                if r.status == ThreadStatus::Starting
                    && let Ok(_claim) = self.claim_thread(project, tid)
                {
                    r = self.recover_thread_start(&r)?;
                    if r.status == ThreadStatus::Open {
                        self.spawn_brief(project, tid);
                    }
                }
                let (state, why) = match r.status {
                    ThreadStatus::Starting => (S::Starting, "기동 결과 대조 중 · 같은 기동을 다시 보내지 않음"),
                    ThreadStatus::Open if r.error.is_some() => (S::Check, r.error.as_deref().unwrap()),
                    ThreadStatus::Open => (S::Running, "시작한 thread"),
                    ThreadStatus::Resolved if r.resolve_pending.is_some() => (S::Running, "resolve 정리 중"),
                    ThreadStatus::Resolved => (S::Finished, "resolve 완료"),
                    ThreadStatus::Failed => (S::Check, r.error.as_deref().unwrap_or("시작 실패 · 명시 재예약 필요")),
                };
                self.queue_status(project, &row.task_id, state, why)?;
                // 파일과 DB는 별개다. 기동 의도가 남아 있으면 같은 ID의 task에 연결을 복구한다.
                if matches!(state, S::Starting | S::Running)
                    && let Some(t) =
                        self.task_board(project)?.tasks.into_iter().find(|t| t.id.as_deref() == Some(&row.task_id))
                    && t.thread.is_none()
                {
                    let _ = self.link_started(project, &t, tid, "host").await;
                }
                continue;
            }
            if row.state != S::Queued {
                continue;
            }
            let board = self.task_board(project)?;
            let Some(t) = board.tasks.iter().find(|t| t.id.as_deref() == Some(&row.task_id)) else {
                self.queue_status(project, &row.task_id, S::Check, "task가 없어졌다")?;
                continue;
            };
            if board.tasks.iter().filter(|t| t.id.as_deref() == Some(&row.task_id)).count() != 1
                || t.block_sha256 != row.approved_block
            {
                self.queue_status(project, &row.task_id, S::Check, "예약 내용이 바뀌었다 · 내용을 확인해 예약 갱신")?;
                continue;
            }
            if !self.known_profiles(project).contains(&row.profile) {
                self.queue_status(project, &row.task_id, S::Check, "알려지지 않은 모델")?;
                continue;
            }
            if model_selection(self, &row.profile).map_or(true, |(_, _, revision)| revision != row.profile_revision) {
                self.queue_status(project, &row.task_id, S::Check, "모델·effort 설정이 바뀌었다 · 예약 갱신 필요")?;
                continue;
            }
            if let Some(repo) = t.repo.as_deref()
                && let Err(reason) = self.task_repo_ok(project, repo)
            {
                self.queue_status(project, &row.task_id, S::Check, &reason)?;
                continue;
            }
            let mut content = format!("{}\n{}", t.title, t.notes.join("\n"));
            let (_, missing) = self.task_attachments(project, &mut content);
            if let Some(reason) = missing.first() {
                self.queue_status(project, &row.task_id, S::Check, reason)?;
                continue;
            }
            if self.open_worker_threads(project) >= self.settings(project).max_parallel_threads {
                self.queue_status(project, &row.task_id, S::Queued, "자리 대기")?;
                continue;
            }
            let mut a = StartArgs {
                reservation: Some((row.task_id.clone(), row.approved_block.clone())),
                account: None,
                holds: vec![],
                project: project.into(),
                title: String::new(),
                machine: row.machine.clone(),
                profile: row.profile.clone(),
                kind: aaa_proto::ThreadKind::Dir,
                repo: None,
                base: None,
                task: String::new(),
                role: aaa_proto::Role::Worker,
                attachments: vec![],
            };
            if a.machine.is_none() {
                a.machine = self.machine_order().0.into_iter().find(|m| {
                    owners.iter().any(|o| &o.machine == m && o.connected && o.profiles.contains(&row.profile))
                        && (t.repo.as_deref() == Some(tasks::REPO_DIR)
                            || self.settings(project).repos.iter().any(|r| {
                                crate::task_repo::repo_name(&r.path) == t.repo.as_deref()
                                    && r.machine.as_deref().or(self.machine_order().1.as_deref()) == Some(m.as_str())
                            }))
                });
            }
            if a.machine.as_ref().is_none_or(|m| {
                !owners.iter().any(|o| &o.machine == m && o.connected && o.profiles.contains(&row.profile))
            }) {
                self.queue_status(project, &row.task_id, S::Queued, "지정 조건의 머신 연결·모델 가용성 대기")?;
                continue;
            }
            let prep = async {
                let (warning, picked) = self.apply_from_task(&mut a, &t.title, Some(&row.approved_block)).await?;
                if let Some(w) = warning {
                    return Err(err(ErrorCode::BadRequest, w));
                }
                self.apply_task_repo(&mut a, picked.repo.as_deref(), None)?;
                self.thread_start_mode(a, "host", true).await
            }
            .await;
            match prep {
                Ok(v) => {
                    let _ = self.link_started(project, t, &v.id, "host").await;
                    self.queue_changed(project);
                }
                Err(e) => {
                    let transient = matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Unavailable)
                        || e.message.starts_with("start_wait:");
                    self.queue_status(project, &row.task_id, if transient { S::Queued } else { S::Check }, &e.message)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn archive_recover(&self, project: &str) -> Result<(), ErrorBody> {
        let rev = file_rev::read(&self.project_dir(project)?.join("TASKS.md")).map_err(internal)?;
        for mut row in self.db.archive_list(project).map_err(internal)? {
            if !row.committed && row.removed_sha256 == rev.sha256 {
                row.committed = true;
                self.db.archive_save(project, &row).map_err(internal)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn task_archive(
        &self,
        project: &str,
        restore: Option<&str>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        self.archive_recover(project)?;
        if let Some(id) = restore {
            let _by = self.task_writer(project, actor)?;
            let mut row = self
                .db
                .archive_list(project)
                .map_err(internal)?
                .into_iter()
                .find(|r| r.task_id == id)
                .ok_or_else(|| err(ErrorCode::NotFound, "완료 보관에 없는 ID"))?;
            let dir = self.project_dir(project)?;
            let path = dir.join("TASKS.md");
            let lock = self.file_locks.of(&path);
            let _lock = lock.lock().unwrap();
            file_rev::save(&path, |rev| {
                let all = tasks::parse(&rev.text);
                if all.iter().any(|t| t.id.as_deref() == Some(id)) {
                    return Ok(file_rev::Edit::Keep(()));
                }
                if all.iter().any(|t| t.title.eq_ignore_ascii_case(&row.task.title)) {
                    return Err(err(ErrorCode::Conflict, "같은 제목의 활성 task가 있다"));
                }
                // 연결을 해제하되 ID·메모·첨부는 보존한다. 예약/재기동은 복원하지 않는다.
                let op = TaskOp::Update {
                    at: at(&row.task),
                    title: None,
                    owner: None,
                    person: false,
                    profile: None,
                    machine: None,
                    notes: None,
                    thread: Some(String::new()),
                    done: Some(false),
                    repo: None,
                };
                let block = crate::task_edit::apply(&row.body, &[op], &empty_ctx())
                    .map_err(|e| internal(format!("{e:?}")))?
                    .text;
                let removed = |_| Ok(crate::task_edit::Removed { list: row.task.list.clone(), text: block.clone() });
                let ctx = crate::task_edit::Ctx { removed: &removed, ..empty_ctx() };
                let text = crate::task_edit::apply(
                    &rev.text,
                    &[TaskOp::Restore { log_id: 0, list: Some(row.task.list.clone()) }],
                    &ctx,
                )
                .map_err(|e| internal(format!("{e:?}")))?
                .text;
                Ok(file_rev::Edit::Write(text, ()))
            })
            .map_err(|e| match e {
                file_rev::SaveError::Edit(e) => e,
                file_rev::SaveError::Io(e) => internal(e),
            })?;
            row.restored = true;
            self.db.archive_save(project, &row).map_err(internal)?;
            self.db.audit(actor, "task_archive_restore", id);
            self.queue_changed(project);
        }
        let active =
            tasks::parse(&file_rev::read(&self.project_dir(project)?.join("TASKS.md")).map_err(internal)?.text);
        let entries = self
            .db
            .archive_list(project)
            .map_err(internal)?
            .into_iter()
            .filter(|r| !r.restored && !active.iter().any(|t| t.id.as_deref() == Some(&r.task_id)))
            .collect();
        Ok(FrontResponse::TaskArchive { entries })
    }
}

#[cfg(test)]
mod tests;
