//! 보고서·library 회수와 사본 갱신.

use super::*;

impl Host {
    pub(crate) fn report_collect_lock(&self, project: &str, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.collect_locks.lock().unwrap().entry(format!("{project}/{id}")).or_default().clone()
    }

    pub(super) fn report_transfer_supported(&self, machine: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .machines
            .get(machine)
            .is_some_and(|m| m.features.iter().any(|f| f == "report_copy.v2"))
    }

    /// agent가 report.md hash 변화를 알렸다
    pub(crate) fn on_report_changed(self: &Arc<Self>, mid: &str, key: &str, hash: Option<String>) {
        let Some((project, id)) = key.split_once('/') else { return };
        let Ok(rec) = self
            .load(project, id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        if rec.machine_id != mid || rec.status != ThreadStatus::Open || hash.is_none() || hash == rec.report_hash {
            return;
        }
        self.refresh_report(project, id);
    }

    /// 보고를 다시 확인하고(바뀌었으면 사본을 받는다) 묶음을 다시 센다. harness 사건(턴 끝·진행 보고)과 세션 끝남은 agent의
    /// report.md 감시(1초마다)보다 먼저 온다. 옛 보고로 세면 쉼 다음에 새 보고가 따로 오고, 끝낸 보고 대신 중간 보고가 검토
    /// 대기가 된다. herdr-projects ticker는 같은 tick 안에서 파일 hash를 읽는다(`fresh_hash`). 확인하는 동안 묶음 셈은 이
    /// thread를 건너뛰고(transitions.rs), 그동안 또 부탁받으면 한 번 더 확인한다.
    pub(crate) fn refresh_report(self: &Arc<Self>, project: &str, id: &str) {
        let key = format!("{project}/{id}");
        {
            let mut pending = self.report_refresh.lock().unwrap();
            if let Some(again) = pending.get_mut(&key) {
                *again = true;
                return;
            }
            pending.insert(key.clone(), false);
        }
        let me = self.clone();
        let (p, t) = (project.to_string(), id.to_string());
        tokio::spawn(async move {
            loop {
                me.collect_if_changed(&p, &t).await;
                let mut pending = me.report_refresh.lock().unwrap();
                match pending.get_mut(&key) {
                    Some(again) if *again => *again = false,
                    _ => {
                        pending.remove(&key);
                        break;
                    }
                }
            }
            me.tick_wake.notify_one();
        });
    }

    /// 실패한 회수는 report hash가 더 바뀌지 않아도 다음 ticker에서 다시 한다. 연결이 없으면 건너뛴다.
    pub(crate) fn retry_report_copies(self: &Arc<Self>, project: &str, now: u64) {
        for r in self.open_threads(project) {
            if r.copy.as_deref() == Some("failed")
                && r.copy_retry_at_ms <= now
                && self
                    .live_conn(&r.machine_id)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
            {
                self.refresh_report(project, &r.id);
            }
        }
    }

    /// agent에 지금 report.md hash를 묻고(library는 읽지 않는다) 받은 것과 다르거나 앞 회수가 실패했으면 받는다
    pub(crate) async fn current_report_hash(
        &self,
        rec: &ThreadRec,
        conn: &crate::AgentConn,
    ) -> Result<String, ErrorBody> {
        let work_dir = rec.work_dir.clone().ok_or_else(|| bad("작업 폴더가 없다"))?;
        let response = if self.machine_feature(&rec.machine_id, "report_hash.v1") {
            conn.request(AgentRequest::ReportHash { work_dir }).await?
        } else {
            conn.request(AgentRequest::ReportCollect { work_dir, max_library_bytes: 0 }).await?
        };
        match response {
            AgentResponse::ReportHash { hash } => Ok(hash),
            AgentResponse::Report(s) if s.report.0.is_empty() => {
                Err(err(ErrorCode::Conflict, "보고가 비어 있어 다음 본문을 기다린다"))
            }
            AgentResponse::Report(s) => Ok(s.hash),
            _ => Err(internal("보고 hash 응답이 아니다")),
        }
    }

    pub(super) async fn collect_if_changed(self: &Arc<Self>, project: &str, id: &str) {
        // 종료·보류의 마지막 사본과 상태 전이 뒤에는 대기하던 요청도 다시 상태를 읽는다.
        let lock = self.report_collect_lock(project, id);
        let _guard = lock.lock().await;
        let Ok(rec) = self
            .load(project, id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        if rec.status != ThreadStatus::Open {
            return;
        }
        let (Some(_), Ok(conn)) = (rec.work_dir.as_ref(), self.live_conn(&rec.machine_id)) else { return };
        self.warn_question_format(&rec).await;
        let probe = self.current_report_hash(&rec, &conn).await;
        if rec.copy.as_deref() != Some("failed")
            && !matches!(&probe, Ok(hash) if Some(hash) != rec.report_hash.as_ref())
        {
            return;
        }
        // 한 번의 일시 오류는 바로 새 manifest로 다시 받는다. 계속 실패하면 ticker 간격으로 재수집한다.
        for _ in 0..2 {
            match self.collect(project, id, false).await {
                Ok(()) => break,
                Err(e) => tracing::warn!(thread = %id, "보고 사본 실패: {}", e.message),
            }
        }
    }

    /// report.md와 library를 받아 host 프로젝트 폴더에 둔다. 실패하면 hash·마지막 사본을 보존하고 자동 재수집한다.
    /// `last`(resolve의 마지막 사본)는 보고가 그대로여도 library를 다시 받는다. 보고 뒤에 바뀐 결과물이 옛 사본으로
    /// 남지 않게 한다(herdr-projects threads.rs `final_copy`가 늘 `copy_home_*`로 library까지 다시 복사하는 것과 같다).
    /// 호출자는 thread별 회수 잠금을 잡는다. 종료·보류는 상태를 저장할 때까지 놓지 않는다.
    pub(crate) async fn collect(self: &Arc<Self>, project: &str, id: &str, last: bool) -> Result<(), ErrorBody> {
        // FileRead뿐 아니라 manifest·보고 사본을 host에 저장하다 실패해도 같은 재수집 경로를 쓴다.
        match self.collect_snapshot(project, id, last).await {
            Ok(()) => {
                // 지표 회수의 저장소 수가 보고 사본·resolve 응답 시간을 늘리지 않는다.
                let host = self.clone();
                let project = project.to_owned();
                let id = id.to_owned();
                tokio::spawn(async move { host.collect_workspace_metrics(&project, &id).await });
                Ok(())
            }
            // 빈 보고는 쓰기 중이다. 마지막 회수에서는 종료를 멈추되 앞 사본의 상태를 그대로 둔다.
            Err(e) if last && e.code == ErrorCode::Conflict => Err(e),
            Err(e) => {
                // 전송 프로토콜은 io 오류를 문자열로 보존한다. host 저장·agent 응답의 EMFILE도 경고한다.
                #[cfg(unix)]
                let nofile = e.message.contains("Too many open files")
                    && (e.message.contains(&format!("os error {}", libc::EMFILE))
                        || e.message.contains(&format!("os error {}", libc::ENFILE)));
                #[cfg(not(unix))]
                let nofile = false;
                if nofile {
                    self.mark_resource_exhausted("report copy");
                }
                let rec = self.update_rec(project, id, |r| {
                    r.copy = Some("failed".into());
                    r.copy_failures = r.copy_failures.saturating_add(1);
                    r.copy_retry_at_ms = aaa_common::util::now_ms().saturating_add(self.cfg.projects.tick_secs * 1000);
                    r.copy_notes = vec![e.message.clone()];
                })?;
                self.flow(&rec, "copy_failed", None, e.message.clone());
                if rec.copy_failures == 2 && !nofile {
                    self.inbox_add_body(
                        project,
                        "report-copy-failed",
                        id,
                        "copy failed",
                        &format!(
                            "{}: 보고·결과물 사본을 받지 못했다. 마지막 상태를 유지하고 자동으로 다시 받는다",
                            crate::transitions::label(&rec)
                        ),
                        &e.message,
                    );
                }
                Err(e)
            }
        }
    }

    pub(super) async fn collect_snapshot(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        last: bool,
    ) -> Result<(), ErrorBody> {
        let rec = self.load(project, id)?;
        let pending_materials =
            if rec.question_answer.is_none() && rec.question_warning.is_none() && !self.default_review_for(&rec) {
                self.pending_question_materials(&rec)?
            } else {
                Vec::new()
            };
        let dir = self.project_dir(project)?;
        let work_dir = rec.work_dir.clone().ok_or_else(|| bad("작업 폴더가 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        let summary_seen = self.summary_report_seen(&dir, id);
        // Ok(None): 이미 받은 보고와 같거나 빈 본문이라 다음 쓰기를 기다린다
        // (보고 hash, 건너뛴 것, 사본 상태, 보고, library가 앞 사본과 달라졌나)
        type Got = Option<(String, Vec<String>, String, Vec<u8>, bool, Vec<String>, Vec<String>, Vec<String>)>;
        let result: Result<Got, ErrorBody> = async {
            let transfer = self.report_transfer_supported(&rec.machine_id);
            let request = if transfer {
                AgentRequest::ReportTransfer {
                    work_dir: work_dir.clone(),
                    max_library_bytes: self.cfg.projects.library_copy_mb << 20,
                }
            } else {
                AgentRequest::ReportCollect {
                    work_dir: work_dir.clone(),
                    max_library_bytes: self.cfg.projects.library_copy_mb << 20,
                }
            };
            let mut snap = match conn.request(request).await {
                Ok(AgentResponse::ReportTransfer(s)) if transfer => s,
                Ok(AgentResponse::Report(s)) if !transfer => s,
                // 한 번의 요청으로 마지막 본문과 첨부 목록을 읽는다. 읽기 실패를 보고 없음으로 처리하지 않는다.
                Err(e) if last && final_report_absent(&rec, &work_dir, &e) => return Ok(None),
                Err(e) if !last && e.code == ErrorCode::Conflict => return Ok(None),
                Err(e) => return Err(e),
                _ => return Err(internal("보고 사본 응답이 아니다")),
            };
            // 구판 agent의 빈 snapshot도 파일·manifest·r-ID·질문·Next를 바꾸기 전에 보류한다.
            if snap.report.0.is_empty() {
                return if last {
                    Err(err(ErrorCode::Conflict, "보고가 비어 있어 다음 본문을 기다린다"))
                } else {
                    Ok(None)
                };
            }
            if transfer && hex::encode(sha2::Sha256::digest(&snap.report.0)) != snap.hash {
                return Err(internal("보고 본문 hash가 다르다"));
            }
            // 원본 해시 대조 뒤, 파일·DB·보고 판에 쓰기 전에 가린다. 조각 경계도 함께 검사한다.
            snap.report.0 = aaa_common::redact::bytes(&snap.report.0);
            if !last && Some(&snap.hash) == rec.report_hash.as_ref() && rec.copy.as_deref() != Some("failed") {
                return Ok(None);
            }
            // library: 임시 폴더에 받고 hash를 확인한 뒤 바꿔 넣는다
            // 옛 agent는 숨은 폴더까지 manifest에 넣는다. 정책상 열지 않는 경로는
            // 받거나 host 경로에 붙이지 않고 건너뛰며, 정상 첨부·본문은 계속 갱신한다.
            snap.files.retain(|f| {
                if crate::library::safe_rel(&f.path) {
                    return true;
                }
                snap.skipped.push(aaa_proto::Skipped {
                    path: f.path.clone(),
                    reason: "host에서 허용하지 않는 library 상대 경로".into(),
                    lazy_size: None,
                });
                false
            });
            let lib_root = dir.join("library");
            let tmp = lib_root.join(format!(".{id}.tmp-{}", aaa_common::util::new_id()));
            let mut sandbox_notes = Vec::new();
            let mut secret_notes = Vec::new();
            let mut failed_files = Vec::new();
            let fetch = async {
                std::fs::create_dir_all(&tmp).map_err(internal)?;
                for f in &snap.files {
                    let target = tmp.join(&f.path);
                    let received = async {
                        std::fs::create_dir_all(target.parent().unwrap()).map_err(internal)?;
                        let mut out = Vec::with_capacity(f.size as usize);
                        // manifest가 해시한 길이만 받는다. 실행 중인 로그의 뒤쪽 append를 섞지 않는다.
                        while (out.len() as u64) < f.size {
                            let ask = (f.size - out.len() as u64).min(512 * 1024) as u32;
                            let request = if transfer {
                                AgentRequest::ReportFileRead {
                                    work_dir: work_dir.clone(),
                                    path: f.path.clone(),
                                    offset: out.len() as u64,
                                    len: ask,
                                }
                            } else {
                                AgentRequest::FileRead {
                                    work_dir: work_dir.clone(),
                                    path: f.path.clone(),
                                    offset: out.len() as u64,
                                    len: ask,
                                }
                            };
                            let (data, eof) = match conn.request(request).await? {
                                AgentResponse::ReportFileChunk { data, eof } if transfer => (data, eof),
                                AgentResponse::FileChunk { data, eof } if !transfer => (data, eof),
                                _ => return Err(internal("library 전송 응답이 아니다")),
                            };
                            if data.0.len() > ask as usize {
                                return Err(internal("요청 범위를 넘는 library 응답"));
                            }
                            out.extend_from_slice(&data.0);
                            if eof || data.0.is_empty() {
                                break;
                            }
                        }
                        if out.len() as u64 != f.size || hex::encode(sha2::Sha256::digest(&out)) != f.sha256 {
                            return Err(internal(format!("{}: 받는 동안 바뀌었다(hash 다름)", f.path)));
                        }
                        // 원본 hash 대조가 끝난 파일 전체로 종류를 판정한다.
                        let (out, warning) = crate::report_copy::prepare(&f.path, &out);
                        std::fs::write(&target, &out).map_err(internal)?;
                        if let Some(warning) = warning {
                            secret_notes.push(warning);
                        }
                        if let Some(note) = crate::report_sandbox::warning(&f.path, &out) {
                            sandbox_notes.push(note);
                        }
                        Ok::<(), ErrorBody>(())
                    }
                    .await;
                    if let Err(e) = received {
                        failed_files.push((f.clone(), e.message));
                        // 이전 검증 사본은 보존하되 새 manifest·불변 판에는 넣지 않는다.
                        aaa_common::diagnostic::ignored(
                            std::fs::remove_file(&target),
                            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                        );
                        if let Ok((previous, _)) = crate::library::local_file(&dir, id, &f.path) {
                            std::fs::create_dir_all(target.parent().unwrap()).map_err(internal)?;
                            std::fs::copy(previous, &target).map_err(internal)?;
                        }
                    }
                }
                Ok(())
            }
            .await;
            if let Err(e) = fetch {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_dir_all(&tmp),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
                return Err(e);
            }
            let mut failures: Vec<String> = failed_files.iter().map(|(_, reason)| reason.clone()).collect();
            snap.files.retain(|f| !failed_files.iter().any(|(failed, _)| failed.path == f.path));
            snap.skipped.extend(failed_files.into_iter().map(|(f, reason)| aaa_proto::Skipped {
                path: f.path,
                reason,
                lazy_size: Some(f.size),
            }));
            let dest = lib_root.join(id);
            let old_manifest = std::fs::read(crate::library::manifest_path(&dir, id))
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok();
            let same_files = same_tree(&tmp, &dest);
            if dest.exists() {
                std::fs::remove_dir_all(&dest).map_err(internal)?;
            }
            std::fs::rename(&tmp, &dest).map_err(internal)?;
            crate::library::write_manifest(&dir, id, &snap).map_err(internal)?;
            let changed = !same_files
                || std::fs::read(crate::library::manifest_path(&dir, id))
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    != old_manifest;
            let mut version_created = false;
            if rec.report_hash.as_ref() != Some(&snap.hash)
                || changed
                || self.thread_report_id(project, id, Some(&snap.hash)).is_none()
            {
                let files = self.manifest(&dir, id);
                let meta = crate::db::LibraryReportRow {
                    project: project.into(),
                    name: id.into(),
                    updated: aaa_common::util::now_ms(),
                    from_thread: Some(id.into()),
                    ..self.db.library_report(project, id).map_err(internal)?.unwrap_or_default()
                };
                match self.snapshot_report_version(project, id, &snap.hash, &dest, &files, Some(&snap.report.0), &meta)
                {
                    Ok(_) => version_created = true,
                    Err(e) => {
                        // 판 보존의 실패도 첨부 전송 실패처럼 기록한다. 최신 본문·단계·ack 처리는
                        // 계속하고, 불완전한 r 판은 snapshot_report_version이 공개하지 않는다.
                        failures.push(format!("보고 판 보존 실패: {}", e.message));
                    }
                }
            }

            if changed || old_manifest.is_none() || version_created {
                // 보고서 화면 「작업 보고」(plan §8.256)
                self.reports_bump();
            }
            let skipped: Vec<String> = snap.skipped.iter().map(|s| format!("{}: {}", s.path, s.reason)).collect();
            // 기억의 실패도 새 본문·완료 단계는 보존한다. 사본 실패로 남겨 마지막
            // 회수·resolve의 거부와 기존 재수집 경로는 그대로 유지한다.
            if let Err(error) = self.automation_copy_memory(&rec, last).await {
                failures.push(format!("자동화 기억 회수 실패: {}", error.message));
            }
            let copy = if !failures.is_empty() {
                "failed"
            } else if skipped.is_empty() {
                "complete"
            } else {
                "partial"
            };
            Ok(Some((
                snap.hash,
                skipped,
                copy.to_string(),
                snap.report.0,
                changed,
                sandbox_notes,
                secret_notes,
                failures,
            )))
        }
        .await;
        match result {
            Ok(None) => Ok(()),
            Ok(Some((hash, skipped, copy, report, changed, sandbox_notes, secret_notes, failures))) => {
                if !secret_notes.is_empty() && (rec.report_hash.as_ref() != Some(&hash) || changed) {
                    self.inbox_add_body(
                        project,
                        "report-secret-warning",
                        id,
                        "바이너리 첨부의 비밀값 확인",
                        &format!("{id}: 원본 보존한 바이너리 첨부에 비밀값 조각 일치가 있다(오탐 가능)"),
                        &secret_notes.join("\n"),
                    );
                }
                if !sandbox_notes.is_empty() && (rec.report_hash.as_ref() != Some(&hash) || changed) {
                    self.inbox_add_body(
                        project,
                        "report-sandbox-warning",
                        id,
                        "보고서 sandbox 확인",
                        &format!("{id}: 보고서에서 제한되거나 확인이 필요한 API를 찾았다"),
                        &sandbox_notes.join("\n"),
                    );
                }
                // 새 보고 알림보다 먼저 만든다. 같은 보고서의 유효한 링크는 재사용한다.
                self.library_link(project, id, None, "host")?;
                // 같은 원본 판의 재수집도 저장 사본을 복구한다. 옛 host의 가린 본문에는 별도 hash가 없을 수 있다.
                write_atomic(&dir.join("threads").join(format!("{id}.md")), &report).map_err(internal)?;
                self.summary_report_received(project, id, &String::from_utf8_lossy(&report), &hash, summary_seen)?;
                self.complete_thread_report(project, id, &String::from_utf8_lossy(&report), &hash)?;
                // 마지막 사본인데 보고가 그대로면 library만 새로 받은 것이다: 보고 사건·평가 수집은 다시 하지 않고,
                // library가 앞 사본과 달라졌을 때만 알린다
                if rec.report_hash.as_ref() == Some(&hash) {
                    // 본문 hash가 같아도 같은 경로의 그림이 달라질 수 있다.
                    if changed && rec.question_answer.is_none() {
                        let mut questions = rec.questions.clone();
                        for q in &mut questions {
                            q.id = None;
                        }
                        self.carry_pending_questions(
                            &rec,
                            if failures.is_empty() { &pending_materials } else { &[] },
                            &hash,
                            &mut questions,
                        )?;
                        let questions = self.identify_questions(project, "thread", id, &hash, &questions)?;
                        self.update_rec(project, id, |r| r.questions = questions)?;
                    }
                    let missing = self.report_missing_files(project, id, &String::from_utf8_lossy(&report), &failures);
                    let rec = self.update_rec(project, id, |r| {
                        if let Some(missing) = missing {
                            r.question_missing = missing;
                        }
                        r.report_copy_hash = Some(hex::encode(sha2::Sha256::digest(&report)));
                        r.report_stage = crate::report_mark::stage(&String::from_utf8_lossy(&report));
                        crate::followups::capture(r, &hash, &String::from_utf8_lossy(&report));
                        r.copy = Some(copy.clone());
                        if failures.is_empty() {
                            r.copy_failures = 0;
                            r.copy_retry_at_ms = 0;
                        }
                        r.copy_notes = skipped.clone();
                    })?;
                    self.followup_notice(&rec);
                    if !changed {
                        return failures.first().map_or(Ok(()), |e| Err(internal(e.clone())));
                    }
                    let mut detail = format!("마지막 사본 {copy}");
                    if !skipped.is_empty() {
                        detail.push_str(&format!(", 건너뜀: {}", copy_notes_summary(&skipped)));
                    }
                    self.flow(&rec, "library_copied", Some(format!("library/{id}")), detail);
                    return failures.first().map_or(Ok(()), |e| Err(internal(e.clone())));
                }
                let text = String::from_utf8_lossy(&report);
                let next = next_lines(&text);
                let question_warning = crate::report_mark::question_warning(&text);
                let question_evidence = crate::report_mark::question_evidence(&text);
                let question_missing = self.report_missing_files(project, id, &text, &failures).unwrap_or_default();
                let questions = crate::report_mark::questions(&text);
                let repeated = if question_warning.is_none() && !crate::report_mark::default_review(&text) {
                    self.repeated_answer(&rec, &questions)?
                } else {
                    None
                };
                let repeated_ids = repeated.as_ref().map(|r| r.ids.clone()).unwrap_or_default();
                let (mut questions, answer) = match repeated {
                    Some(r) => (r.items, r.answer),
                    None => (questions, None),
                };
                self.carry_pending_questions(
                    &rec,
                    if failures.is_empty() && question_warning.is_none() && !crate::report_mark::default_review(&text) {
                        &pending_materials
                    } else {
                        &[]
                    },
                    &hash,
                    &mut questions,
                )?;
                let questions = self.identify_questions(project, "thread", id, &hash, &questions)?;
                let closed_question = questions.is_empty()
                    && question_warning.is_none()
                    && !rec.questions.is_empty()
                    && rec.question_answer.is_none();
                let previous = rec;
                let rec = self.update_rec(project, id, |r| {
                    r.report_stage = crate::report_mark::stage(&text);
                    crate::followups::capture(r, &hash, &text);
                    r.report_hash = Some(hash);
                    r.report_copy_hash = Some(hex::encode(sha2::Sha256::digest(&report)));
                    r.report_changed_ms = Some(aaa_common::util::now_ms());
                    r.next = next;
                    r.questions = questions;
                    r.question_warning = question_warning;
                    r.question_evidence = question_evidence;
                    r.question_missing = question_missing;
                    r.question_warning_sent = None;
                    r.question_answer = answer;
                    r.copy = Some(copy.clone());
                    if failures.is_empty() {
                        r.copy_failures = 0;
                        r.copy_retry_at_ms = 0;
                    }
                    r.copy_notes = skipped.clone();
                })?;
                self.followup_notice(&rec);
                if !repeated_ids.is_empty() {
                    self.warn_answered_questions(&rec, &repeated_ids).await;
                }
                self.warn_question_format(&rec).await;
                if closed_question {
                    self.record_question_closed(&previous);
                }
                self.reconcile_question_requests(project)?;
                let mut detail = format!("사본 {copy}");
                if !skipped.is_empty() {
                    detail.push_str(&format!(", 건너뜀: {}", copy_notes_summary(&skipped)));
                }
                if rec.automation.is_some() && crate::report_mark::done(crate::report_mark::marker(&text)) {
                    self.thread_ack(project, id, "automation")?;
                    self.new_report_item(project, &self.load(project, id)?);
                }
                self.flow(&rec, "report_ready", Some(format!("threads/{id}.md")), detail);
                // 새 보고는 이번 묶음 셈에 바로 든다(herdr-projects ticker `fresh_hash`)
                self.tick_wake.notify_one();
                failures.first().map_or(Ok(()), |e| Err(internal(e.clone())))
            }
            Err(e) => Err(e),
        }
    }
}

fn final_report_absent(rec: &ThreadRec, work_dir: &str, error: &ErrorBody) -> bool {
    if error.code != ErrorCode::NotFound {
        return false;
    }
    // 작업 폴더를 사람이 먼저 없앤 경우 Git 기록을 정리할 수 있다. 구판 agent도 이 IO 문맥을 보낸다.
    let missing_path = error.message.ends_with("(os error 2)") || error.message.ends_with("(os error 3)");
    if missing_path && error.message.starts_with(&format!("작업 폴더가 없다: {work_dir}: ")) {
        return true;
    }
    // 보류 뒤 새 자리에서는 이전 보고가 남아 있어도 새 본문이 없을 수 있다. 구판은 첨부 유무를 알려 주지 않으므로
    // 지워질 수 있는 worktree에서는 보고·첨부가 모두 없다는 응답을 요구한다.
    error.message == "report.md와 첨부가 없다"
        || (rec.kind != ThreadKind::Worktree && missing_path && error.message.starts_with("report.md가 없다: "))
}
