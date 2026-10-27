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
pub(crate) fn identify(text: &str) -> Result<String, ErrorBody> {
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
    /// 명시한 사용자 지시 없는 coordinator 편집은 선택 보호를 따른다. API 원문 쓰기도 같은 정책이다.
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
                    "예약에서 승인한 모델·머신은 사용자가 웹이나 aaa 밖 CLI에서 직접 바꾸거나, coordinator가 --user-request로 사용자 지시 근거를 남겨 바꾼다",
                ));
            }
            let (op, om) = tasks::agent_parts(old.owner.as_deref(), &profiles).unwrap_or_default();
            let (np, nm) = tasks::agent_parts(new.owner.as_deref(), &profiles).unwrap_or_default();
            if (op.is_some() && op != np) || (om.is_some() && om != nm) {
                return Err(err(
                    ErrorCode::Forbidden,
                    "정해진 모델·머신은 사용자가 웹이나 aaa 밖 CLI에서 직접 바꾸거나, coordinator가 --user-request로 사용자 지시 근거를 남겨 바꾼다",
                ));
            }
        }
        Ok(())
    }

    /// 사용자 지시는 선택 칸만 바꾼다. 다른 내용이 이미 바뀐 예약을 함께 승인하지 않는다.
    pub(crate) fn check_requested_task_choices(
        &self,
        project: &str,
        before: &str,
        after: &str,
    ) -> Result<(), ErrorBody> {
        let new = tasks::parse(after);
        let rows = self.db.queue_list(project).map_err(internal)?;
        for old in tasks::parse(before) {
            let Some(t) = new.iter().find(|t| t.id == old.id) else { continue };
            if old.owner == t.owner {
                continue;
            }
            let Some(q) = rows.iter().find(|q| old.id.as_ref() == Some(&q.task_id)) else { continue };
            if matches!(q.state, S::Starting | S::Running) || (q.state == S::Check && q.thread.is_some()) {
                return Err(err(
                    ErrorCode::Conflict,
                    "이미 기동한 예약의 선택은 바꾸지 않는다. thread 상태를 먼저 확인한다",
                ));
            }
            if matches!(q.state, S::Queued | S::Check) && old.block_sha256 != q.approved_block {
                return Err(err(
                    ErrorCode::Conflict,
                    "예약의 다른 내용이 바뀌었다. 내용을 확인해 먼저 예약을 갱신한다",
                ));
            }
        }
        Ok(())
    }

    /// 명시한 사용자 지시를 선택 출처로 남기고, 미기동 예약의 순서·생성 시각을 유지해 실행값을 갱신한다.
    pub(crate) fn record_requested_task_choices(
        &self,
        project: &str,
        before: &str,
        after: &str,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        let mut old = tasks::parse(before);
        tasks::fill_agent(&mut old, &self.known_profiles(project));
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
                        let profile_changed = a.profile != b.profile;
                        // 머신만 바꿀 때는 예약 당시 해석한 기본 모델과 설정 판을 유지한다.
                        let pending = matches!(q.state, S::Queued | S::Check);
                        let profile = if profile_changed {
                            b.profile.clone().unwrap_or_else(|| self.default_thread_profile(project))
                        } else if pending {
                            q.profile.clone()
                        } else {
                            q.selection.profile.clone()
                        };
                        let now = aaa_common::util::now_ms();
                        q.selection = aaa_proto::TaskChoicePin {
                            owner: b.owner.clone(),
                            profile: profile.clone(),
                            machine: b.machine.clone(),
                            actor: actor.into(),
                            at_ms: now,
                        };
                        if pending {
                            if profile_changed {
                                let (model, effort, revision) =
                                    model_selection(self, &profile).map_err(|e| std::io::Error::other(e.message))?;
                                q.model = model;
                                q.effort = effort;
                                q.profile_revision = revision;
                            }
                            q.profile = profile;
                            q.machine = b.machine.clone();
                            q.task = b.clone();
                            q.approved_block = b.block_sha256.clone();
                            q.state = S::Queued;
                            q.reason = "사용자 지시로 모델·머신 변경 · 자리 대기".into();
                        }
                        q.updated_ms = now;
                    }
                }
                Ok(())
            })
            .map_err(internal)
    }

    /// 일반 사용자 API의 선택 변경을 새 출처로 인정한다. 파일 직접 수정·coordinator 일반 편집은 근거가 아니다.
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

    /// 내용은 그대로 두고 식별 주석만 이행한다. 파일 잠금 안의 최신 판에 적용해 동시 첫 조회도 안전하다.
    pub(crate) fn task_identify(&self, project: &str) -> Result<(), ErrorBody> {
        let dir = self.project_dir(project)?;
        let path = dir.join("TASKS.md");
        let lock = self.file_locks.of(&path);
        let _guard = lock.lock().unwrap();
        let saved = file_rev::save(&path, |rev| {
            let text = identify(&rev.text)?;
            if text == rev.text { Ok(file_rev::Edit::Keep(())) } else { Ok(file_rev::Edit::Write(text, ())) }
        })
        .map_err(|e| match e {
            file_rev::SaveError::Edit(e) => e,
            file_rev::SaveError::Io(e) => internal(e),
        })?;
        let mut tasks = tasks::parse(saved.after.as_ref().unwrap_or(&saved.before).text.as_str());
        self.fill_task_ids(project, &mut tasks)?;
        if let Some(after) = saved.after {
            self.task_identity_seen(project, &saved.before.sha256, &after);
            self.db.audit("host", "task_identity_migrate", project);
            aaa_common::diagnostic::ignored(
                self.task_events.send((project.to_string(), after.sha256)),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
            self.tick_wake.notify_one();
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
        self.task_identify(project)?;
        let action = match action {
            QueueOp::Cancel { task_id } => QueueOp::Cancel { task_id: self.task_selector(project, &task_id)? },
            QueueOp::Move { task_id, position } => {
                QueueOp::Move { task_id: self.task_selector(project, &task_id)?, position }
            }
            other => other,
        };
        self.check_active(project)?;
        let _gate = self.queue_gate.lock().await;
        match action {
            QueueOp::List => unreachable!(),
            QueueOp::Reserve { task, block, profile, profile_revision: approved_profile, after } => {
                let board = self.task_board(project)?;
                let old = tasks::find(&board.tasks, &task)
                    .or_else(|| board.tasks.iter().find(|t| t.id.as_deref() == Some(&task)))
                    .ok_or_else(|| err(ErrorCode::NotFound, "task가 없다"))?;
                let after = after.map(|a| self.after_id(project, &board.tasks, &a)).transpose()?;
                if let (Some(after), Some(id)) = (&after, &old.id) {
                    self.check_after_cycle(project, id, after)?;
                }
                if old.id.as_ref().is_some_and(|id| {
                    self.db
                        .queue_list(project)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok_and(|rows| {
                            rows.iter().any(|r| {
                                &r.task_id == id
                                    && r.approved_block == block
                                    && matches!(r.state, S::Starting | S::Running)
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
                    && self
                        .load(project, tid)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok_and(|r| {
                            matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting)
                                || r.resolve_pending.is_some()
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
                            after,
                            after_short_id: None,
                            stage: None,
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

    /// 앞 할 일을 ULID로 정한다. 활성 목록이 먼저이고, 이미 완료 보관된 할 일도 받는다(바로 차례에 든다).
    fn after_id(&self, project: &str, active: &[TaskView], value: &str) -> Result<String, ErrorBody> {
        let value = self.task_selector(project, value)?;
        if let Some(id) = tasks::find(active, &value).and_then(|t| t.id.clone()) {
            return Ok(id);
        }
        self.db
            .archive_list(project)
            .map_err(internal)?
            .into_iter()
            .rev()
            .find(|r| !r.restored && (r.task_id == value || r.task.title.eq_ignore_ascii_case(value.trim())))
            .map(|r| r.task_id)
            .ok_or_else(|| err(ErrorCode::NotFound, "앞 할 일이 활성 목록과 완료 보관에 없다"))
    }

    /// 앞 할 일을 따라가 자기에게 돌아오면 서로 기다려 영영 시작하지 않는다.
    fn check_after_cycle(&self, project: &str, id: &str, after: &str) -> Result<(), ErrorBody> {
        let rows = self.db.queue_list(project).map_err(internal)?;
        let mut seen = std::collections::HashSet::new();
        let mut cur = after.to_string();
        loop {
            if cur == id {
                return Err(err(ErrorCode::BadRequest, "앞 할 일이 이 할 일을 기다린다(순환). 앞뒤를 다시 정한다"));
            }
            if !seen.insert(cur.clone()) {
                return Ok(());
            }
            let next = rows.iter().find(|r| r.task_id == cur && matches!(r.state, S::Queued | S::Check));
            match next.and_then(|r| r.after.clone()) {
                Some(next) => cur = next,
                None => return Ok(()),
            }
        }
    }

    /// 앞 할 일이 끝났나: 활성 목록에 있으면 기다리고, 없으면 완료 보관(복원 안 됨)에 있어야 끝난 것이다.
    fn after_done(&self, project: &str, active: &[TaskView], after: &str) -> Result<Option<bool>, ErrorBody> {
        if active.iter().any(|t| t.id.as_deref() == Some(after)) {
            return Ok(Some(false));
        }
        let archived = self.db.archive_list(project).map_err(internal)?;
        Ok(archived.iter().any(|r| r.task_id == after && !r.restored).then_some(true))
    }

    fn queue_changed(&self, project: &str) {
        aaa_common::diagnostic::ignored(
            self.task_events.send((project.into(), "queue".into())),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
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
            tracing::info!(
                project,
                task = id,
                ?state,
                reason = %aaa_common::redact::text(reason),
                "예약 판정 변경"
            );
            self.queue_changed(project);
        }
        Ok(())
    }

    /// 예약 외 직접 시작도 같은 재연결 대조를 쓴다. 호출자가 thread 작업 잠금을 소유한다.
    pub(crate) fn recover_thread_start(
        &self,
        rec: &crate::orch::ThreadRec,
    ) -> Result<crate::orch::ThreadRec, ErrorBody> {
        // 목록을 읽은 뒤 실제 기동이 완료하고 잠금을 반환했을 수 있다.
        // 호출자가 얻은 claim 아래의 최신 상태로만 중단 여부를 판정한다.
        let rec = self.load(&rec.project, &rec.id)?;
        if rec.status != ThreadStatus::Starting {
            return Ok(rec);
        }
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
            let Ok(rec) = serde_json::from_str::<crate::orch::ThreadRec>(&text).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if rec.status != ThreadStatus::Starting {
                continue;
            }
            let Ok(_claim) = self.claim_thread(project, &rec.id).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
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
        let mut capacity_exhausted = false;
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
                let workspace = row.task.repo.as_deref().unwrap_or(tasks::REPO_DIR);
                let running_reason = if let Some(machine) = &row.machine {
                    match self.workspace_allowed(project, workspace, machine) {
                        Ok(false) => {
                            format!("시작한 thread · {machine}의 {workspace} 자동 허용은 꺼짐 · 고정 머신 지정 유지")
                        }
                        Ok(true) => "시작한 thread".into(),
                        Err(e) if e.code == ErrorCode::NotFound => "시작한 thread · 고정 머신 등록이 없다".into(),
                        Err(e) => return Err(e),
                    }
                } else {
                    "시작한 thread".into()
                };
                let (state, why) = match r.status {
                    ThreadStatus::Starting => (S::Starting, "기동 결과 대조 중 · 같은 기동을 다시 보내지 않음"),
                    ThreadStatus::Open if r.error.is_some() => (S::Check, r.error.as_deref().unwrap()),
                    ThreadStatus::Open => (S::Running, running_reason.as_str()),
                    ThreadStatus::Resolved if r.resolve_pending.is_some() => (S::Running, "resolve 정리 중"),
                    ThreadStatus::Resolved => (S::Finished, "resolve 완료"),
                    ThreadStatus::Parked => (S::Running, "보류 · 자리를 돌려주고 이어 하기를 기다린다"),
                    ThreadStatus::Failed => (S::Check, r.error.as_deref().unwrap_or("시작 실패 · 명시 재예약 필요")),
                };
                self.queue_status(project, &row.task_id, state, why)?;
                // 파일과 DB는 별개다. 기동 의도가 남아 있으면 같은 ID의 task에 연결을 복구한다.
                if matches!(state, S::Starting | S::Running)
                    && let Some(t) =
                        self.task_board(project)?.tasks.into_iter().find(|t| t.id.as_deref() == Some(&row.task_id))
                    && t.thread.is_none()
                    && let Some(reason) = self.link_started(project, &t, tid, "host").await
                {
                    tracing::warn!(error = %aaa_common::redact::text(&reason), "시작한 thread의 task 연결 실패");
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
            // 앞 할 일을 기다리는 예약은 자리 차례를 막지 않는다. 뒤의 예약은 그대로 자리를 얻는다.
            if let Some(after) = &row.after {
                let name = row.after_short_id.as_deref().unwrap_or(after);
                match self.after_done(project, &board.tasks, after)? {
                    Some(true) => {}
                    Some(false) => {
                        self.queue_status(project, &row.task_id, S::Queued, &format!("앞 할 일 {name} 완료 대기"))?;
                        continue;
                    }
                    None => {
                        let why = format!("앞 할 일 {name}이 활성 목록과 완료 보관에 없다 · 예약 갱신 필요");
                        self.queue_status(project, &row.task_id, S::Check, &why)?;
                        continue;
                    }
                }
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
            // 앞 예약이 자리 부족으로 멈춘 순회에서는 뒤 예약도 기다린다. resolve가 그사이
            // 자리를 돌려줘도 뒤쪽만 추월하지 않고, 다음 순회에서 차례대로 다시 판정한다.
            // 이미 기동한 예약의 상태 대조와 내용 검사는 뒤쪽도 계속한다.
            let open = self.open_worker_threads(project);
            let limit = self.settings(project).max_parallel_threads;
            tracing::debug!(project, task = %row.task_id, open, limit, capacity_exhausted, "예약 자리 판정");
            if capacity_exhausted || open >= limit {
                capacity_exhausted = true;
                self.queue_status(project, &row.task_id, S::Queued, "자리 대기")?;
                continue;
            }
            let mut a = StartArgs {
                automatic_workspace: None,
                start_task: None,
                pool: None,
                over_limit: None,
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
                let workspace = t.repo.as_deref().unwrap_or(tasks::REPO_DIR);
                a.automatic_workspace = Some(workspace.into());
                let mut allowed = Vec::new();
                for m in self.machine_order().0 {
                    if self.workspace_allowed(project, workspace, &m)? {
                        allowed.push(m);
                    }
                }
                if allowed.is_empty() {
                    self.queue_status(
                        project,
                        &row.task_id,
                        S::Queued,
                        &format!("자리 대기: {workspace} 자동 배정이 허용된 연결 머신이 없다"),
                    )?;
                    continue;
                }
                a.machine = allowed.into_iter().find(|m| {
                    owners.iter().any(|o| &o.machine == m && o.connected && o.profiles.contains(&row.profile))
                        && (t.repo.is_none()
                            || t.repo.as_deref() == Some(tasks::REPO_DIR)
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
                if let Some(warning) = self.apply_task_repo(&mut a, picked.repo.as_deref(), None)? {
                    tracing::warn!(project, task = %row.task_id, warning, "고정 머신 지정의 workspace 자동 배정 허용이 꺼져 있다");
                }
                self.thread_start_mode(a, "host", true).await
            }
            .await;
            match prep {
                Ok(v) => {
                    if let Some(reason) = self.link_started(project, t, &v.id, "host").await {
                        tracing::warn!(error = %aaa_common::redact::text(&reason), "시작한 thread의 task 연결 실패");
                    }
                    self.queue_changed(project);
                }
                Err(e) => {
                    // 사전 확인 뒤 직접 기동이 마지막 자리를 선점한 경우에도 같은 차례를 지킨다.
                    // 계정·머신 등 다른 start_wait는 뒤 예약의 실제 조건 판정을 막지 않는다.
                    capacity_exhausted |= e.message.starts_with("start_wait: 프로젝트 실행 한도 ");
                    let transient = matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Unavailable)
                        || e.message.starts_with("start_wait:");
                    let reason = e.message.strip_prefix("start_wait: ").unwrap_or(&e.message);
                    self.queue_status(project, &row.task_id, if transient { S::Queued } else { S::Check }, reason)?;
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
        view: Option<aaa_proto::TaskArchiveQuery>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        self.archive_recover(project)?;
        let restore = restore.map(|v| self.task_selector(project, v)).transpose()?;
        if let Some(id) = restore.as_deref() {
            let _by = self.task_writer(project, actor)?;
            let mut row = self
                .db
                .archive_list(project)
                .map_err(internal)?
                .into_iter()
                .find(|r| r.task_id == id || r.task.title.eq_ignore_ascii_case(id))
                .ok_or_else(|| err(ErrorCode::NotFound, "완료 보관에 없는 ID"))?;
            let id = row.task_id.as_str();
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
        let view = view.unwrap_or_default();
        let mut rows = self.db.archive_list(project).map_err(internal)?;
        let known = self.known_profiles(project);
        for row in &mut rows {
            if row.task.profile.is_none() && row.task.machine.is_none() {
                tasks::fill_agent(std::slice::from_mut(&mut row.task), &known);
            }
        }
        let profiles = rows
            .iter()
            .map(|r| r.task.profile.clone().unwrap_or_default())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let machines = rows
            .iter()
            .map(|r| r.task.machine.clone().unwrap_or_default())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let repos = history_repos(rows.iter().map(|r| &r.task));
        let limit = view.limit.unwrap_or(50).clamp(1, 100) as usize;
        let sort = view.sort.unwrap_or_default();
        for row in &mut rows {
            row.started_ms = archive_started_ms(row);
        }
        rows.sort_by_key(|row| std::cmp::Reverse(archive_sort_key(row, sort)));
        let after = view.after.as_deref().map(archive_cursor).transpose()?;
        let mut entries = Vec::new();
        let mut bytes = 0;
        let mut next = None;
        for mut row in rows {
            if after.is_some_and(|cursor| archive_sort_key(&row, sort) >= cursor) {
                continue;
            }
            let restored = row.restored || active.iter().any(|t| t.id.as_deref() == Some(&row.task_id));
            let matches = match view.filter {
                Some(aaa_proto::TaskArchiveFilter::Restored) => restored,
                Some(aaa_proto::TaskArchiveFilter::MainMerged) => !restored && row.reason == "origin/main 병합",
                Some(aaa_proto::TaskArchiveFilter::ReportDone) => !restored && row.reason == "보고 [완료]",
                None => !restored,
            };
            if !matches
                || !history_matches(&row.task, view.search.as_ref())
                || view.profile.as_deref().is_some_and(|v| row.task.profile.as_deref().unwrap_or("") != v)
                || view.machine.as_deref().is_some_and(|v| row.task.machine.as_deref().unwrap_or("") != v)
                || view.since_ms.is_some_and(|v| row.completed_ms < v)
                || view.until_ms.is_some_and(|v| row.completed_ms >= v)
            {
                continue;
            }
            row.restored = restored;
            // 기록의 원문은 DB에 보존한다. 목록에는 중복된 전체 TASKS.md 스냅샷을 실어 보내지 않는다.
            row.history.clear();
            let size = serde_json::to_vec(&row).map_err(internal)?.len();
            if !entries.is_empty() && (entries.len() >= limit || bytes + size > HISTORY_PAGE_BYTES) {
                next = entries.last().map(|r: &aaa_proto::ArchivedTask| {
                    let (at, number) = archive_sort_key(r, sort);
                    format!("{at}:{number}")
                });
                break;
            }
            if size > HISTORY_PAGE_BYTES {
                return Err(err(
                    ErrorCode::TooLarge,
                    format!(
                        "{} 보관 한 건이 페이지 상한을 넘는다",
                        row.task.short_id.as_deref().unwrap_or(&row.task_id)
                    ),
                ));
            }
            bytes += size;
            entries.push(row);
        }
        Ok(FrontResponse::TaskArchive { entries, next, repos, profiles, machines })
    }
}

/// 처음 thread를 이은 이력의 시각. 다시 맡긴 일도 처음 시작한 때가 시작 날짜다.
fn archive_started_ms(row: &aaa_proto::ArchivedTask) -> Option<u64> {
    row.history.iter().filter(|h| h.thread.as_deref().is_some_and(|t| !t.is_empty())).map(|h| h.at_ms).min()
}

/// 정렬 값과 할 일 번호. 번호가 프로젝트 안에서 유일하므로 이 쌍이 곧 페이지 커서다. 시작 기록이 없으면 0으로 맨 끝이다.
fn archive_sort_key(row: &aaa_proto::ArchivedTask, sort: aaa_proto::TaskArchiveSort) -> (u64, u64) {
    let number = row.task.short_id.as_deref().and_then(|s| aaa_proto::short_id_number(s, 'd')).unwrap_or(0) as u64;
    match sort {
        aaa_proto::TaskArchiveSort::Completed => (row.completed_ms, number),
        aaa_proto::TaskArchiveSort::Started => (row.started_ms.unwrap_or(0), number),
        aaa_proto::TaskArchiveSort::Number => (number, number),
    }
}

fn archive_cursor(text: &str) -> Result<(u64, u64), ErrorBody> {
    text.split_once(':')
        .and_then(|(at, number)| Some((at.parse().ok()?, number.parse().ok()?)))
        .ok_or_else(|| err(ErrorCode::BadRequest, format!("완료 목록 커서가 아니다: {text}. 처음부터 다시 읽는다")))
}

/// frame 봉투와 저장소 선택지를 위한 여백을 둔다. 개수뿐 아니라 직렬화 바이트로 자른다.
pub(crate) const HISTORY_PAGE_BYTES: usize = aaa_proto::MAX_FRAME * 3 / 4;

pub(crate) fn history_matches(task: &TaskView, search: Option<&aaa_proto::TaskHistorySearch>) -> bool {
    let Some(search) = search else { return true };
    if search.repo.as_deref().is_some_and(|repo| task.repo.as_deref().unwrap_or("") != repo) {
        return false;
    }
    let needle = search.query.trim().to_lowercase();
    needle.is_empty()
        || task
            .short_id
            .iter()
            .chain(std::iter::once(&task.title))
            .chain(task.owner.iter())
            .chain(task.notes.iter())
            .any(|text| text.to_lowercase().contains(&needle))
}

pub(crate) fn history_repos<'a>(tasks: impl Iterator<Item = &'a TaskView>) -> Vec<aaa_proto::TaskHistoryRepo> {
    let mut counts = std::collections::BTreeMap::<String, u32>::new();
    let mut seen = std::collections::HashSet::new();
    for task in tasks {
        let repo = task.repo.clone().unwrap_or_default();
        if seen.insert((task.id.as_deref().unwrap_or(&task.title).to_owned(), repo.clone())) {
            *counts.entry(repo).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(key, n)| aaa_proto::TaskHistoryRepo {
            label: if key.is_empty() { "정하지 않음".into() } else { key.clone() },
            key,
            n,
        })
        .collect()
}

#[cfg(test)]
mod tests;
