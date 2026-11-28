//! pane 띄우기, 조작 lease, 이름·재개·교체·닫기.

use super::*;

impl Host {
    /// agent에 세션을 띄우고 pane 목록에 넣는다. 새 pane ID를 돌려준다.
    pub(super) async fn spawn_on(
        self: &Arc<Self>,
        mid: &str,
        spec: SpawnSpec,
        actor: &str,
    ) -> Result<String, ErrorBody> {
        let conn = self.live_conn(mid)?;
        self.check_health(mid)?;
        self.db.audit(actor, "pane_run", &db::pane_run_detail(&spec));
        match conn.request(AgentRequest::Spawn(spec)).await? {
            AgentResponse::Session(info) => {
                let id = info.id.clone();
                self.on_session(mid, info, false);
                Ok(id)
            }
            other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    // ───────────── 조작 lease(§4.5) ─────────────

    pub fn new_front(&self, label: String) -> FrontRef {
        FrontRef { id: format!("f{}", self.front_seq.fetch_add(1, Ordering::Relaxed) + 1), label, peer_pid: None }
    }

    pub(super) fn lease_set(&self, id: &str, to: Option<FrontRef>) {
        {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(id) else { return };
            if p.lease == to {
                return;
            }
            p.lease = to;
            aaa_common::diagnostic::ignored(p.events.send(PaneEvent::State), "수신자가 끝난 뒤에도 현재 작업은 마친다");
        }
        self.bump();
        // 보는 웹 화면이 그 pane을 쓰기 시작하면 봤다(seen.rs).
        if self.inner.lock().unwrap().panes.contains_key(id) && self.someone_sees(id) {
            self.mark_seen(&[id.to_string()]);
        }
    }

    /// 이 front가 지금 쓰는 화면이 된다(herdr-web-ui처럼 명시적 가져오기 없이, 마지막 입력이 이긴다).
    /// 바뀌면 pane 상태 사건이 나가고, 각 front는 자기 규칙으로 크기를 다시 건다.
    pub(crate) fn lease_use(&self, id: &str, f: &FrontRef) {
        self.lease_set(id, Some(f.clone()));
        // 그 화면이 썼다: 그 화면이 알린 색이 지금 색이다(herdr `promote_client_to_foreground`, plan §8.187)
        let i = &mut *self.inner.lock().unwrap();
        if i.colors.touch(&f.id, None) {
            colors::push_all(i, None);
        }
    }

    pub(super) fn lease_free(&self, id: &str) -> Result<(), ErrorBody> {
        match self.inner.lock().unwrap().panes.get(id).and_then(|p| p.lease.clone()) {
            Some(h) => Err(err(ErrorCode::LeaseHeld, format!("{}이 쓰는 중이라 크기를 바꾸지 않았다", h.label))),
            None => Ok(()),
        }
    }

    /// front 연결이 끝났다: 그 연결의 lease를 모두 푼다.
    pub fn front_gone(&self, front_id: &str) {
        let freed: Vec<String> = {
            let mut i = self.inner.lock().unwrap();
            if i.colors.gone(front_id) {
                colors::push_all(&mut i, None);
            }
            i.panes
                .iter()
                .filter(|(_, p)| p.lease.as_ref().is_some_and(|l| l.id == front_id))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in freed {
            self.lease_set(&id, None);
        }
    }

    // ───────────── 재개·정리 ─────────────

    /// 끝난 pane을 같은 spec(이름·argv·cwd·크기)으로 다시 띄우고 layout 자리를 새 pane으로 바꾼 뒤 옛 기록을 지운다.
    /// pane에 사람이 붙인 이름(herdr `pane rename`). 이름의 정본은 agent의 세션 기록이라 agent까지 간다(plan §8.109).
    pub(super) async fn rename(
        self: &Arc<Self>,
        pane: &str,
        label: Option<String>,
        role: DeviceRole,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        if label.as_ref().is_some_and(|l| l.chars().count() > aaa_proto::MAX_PANE_LABEL_CHARS) {
            return Err(err(
                ErrorCode::BadRequest,
                format!("이름은 {}자 안이어야 한다", aaa_proto::MAX_PANE_LABEL_CHARS),
            ));
        }
        let conn = self.live_conn(&mid)?;
        // 옛 agent는 모르는 요청에 연결을 끊는다: 보내기 전에 거른다
        let knows =
            self.inner.lock().unwrap().machines.get(&mid).is_some_and(|m| m.features.iter().any(|f| f == "rename.v1"));
        if !knows {
            return Err(err(
                ErrorCode::BadRequest,
                "이 머신의 agent가 pane 이름 바꾸기를 모른다(agent binary 반영 필요)",
            ));
        }
        self.db.audit(actor, "pane_rename", &format!("{id} {}", label.as_deref().unwrap_or("-")));
        match conn.request(AgentRequest::Rename { session: id.clone(), label }).await? {
            AgentResponse::Session(info) => self.on_session(&mid, info, false),
            other => return Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
        self.pane_view_of(&id, role).map(FrontResponse::Pane).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))
    }

    pub(super) async fn resume(
        self: &Arc<Self>,
        pane: &str,
        role: DeviceRole,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let info = self
            .inner
            .lock()
            .unwrap()
            .panes
            .get(&id)
            .map(|p| p.info.clone())
            .ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
        if info.state.is_running() {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 재개할 수 없다"));
        }
        let spec = SpawnSpec {
            name: respawn_name(&info),
            argv: info.argv.clone(),
            cwd: info.cwd.clone(),
            env: vec![],
            cols: info.cols,
            rows: info.rows,
            role: info.role,
            harness: info.harness.clone(),
            profile: None,
            resolved_profile: None,
        };
        let new_id = self.respawn(&id, &mid, spec, actor).await?;
        self.pane_view_of(&new_id, role)
            .map(FrontResponse::Pane)
            .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
    }

    /// 끝난 pane 자리에 새 세션을 띄운다(`pane resume`·재부팅 복원): layout 자리를 바꾸고 옛 기록을 지운다.
    /// spec 이름은 [`respawn_name`]으로 짓는다
    pub(crate) async fn respawn(
        self: &Arc<Self>,
        id: &str,
        mid: &str,
        spec: SpawnSpec,
        actor: &str,
    ) -> Result<String, ErrorBody> {
        // managed thread는 인증 유무와 관계없이 launch·identity·감시를 함께 잇는다.
        // thread_restart가 자리 교체와 옛 기록 정리도 맡으므로 여기서 두 번 교체하지 않는다.
        if let Some(rec) = self.thread_of_pane(id).filter(|r| r.kind != aaa_proto::ThreadKind::Adopted) {
            let resumed = self.thread_restart(&rec.project, &rec.id, true, actor).await?;
            let new_id = resumed.pane.ok_or_else(|| err(ErrorCode::Internal, "thread 세션이 없다"))?;
            self.db.audit(actor, "pane_resume", &format!("{id} -> {new_id}"));
            return Ok(new_id);
        }
        let new_id = self.respawn_with_account(id, mid, spec, actor).await?;
        self.db.audit(actor, "pane_resume", &format!("{id} -> {new_id}"));
        self.replace_pane(id, &new_id, mid, actor).await;
        Ok(new_id)
    }

    /// layout에서 `old` 자리를 `new`로 바꾸고 `old` 기록을 지운다. 사람이 붙인 이름은 `new`가 잇는다(herdr 복원
    /// `set_manual_label`). 이름의 정본은 agent 세션 기록이라 agent에 붙인다(plan §8.109)
    pub(crate) async fn replace_pane(self: &Arc<Self>, old: &str, new: &str, mid: &str, actor: &str) {
        let label = self.inner.lock().unwrap().panes.get(old).and_then(|p| p.info.label.clone());
        if let Some(label) = label.filter(|_| self.machine_feature(mid, "rename.v1")) {
            let renamed = match self.live_conn(mid) {
                Ok(conn) => conn.request(AgentRequest::Rename { session: new.to_string(), label: Some(label) }).await,
                Err(e) => Err(e),
            };
            match renamed {
                Ok(AgentResponse::Session(info)) => self.on_session(mid, info, false),
                Ok(other) => tracing::warn!(pane = %new, "이름 잇기: 예상하지 못한 응답 {other:?}"),
                Err(e) => tracing::warn!(pane = %new, "이름 잇기 실패: {}", e.message),
            }
        }
        // 옛 ID를 보던 화면이 새 pane을 따라가게 남긴다(QA Q19): 옛 pane이 대신했던 것까지 잇는다
        {
            let mut i = self.inner.lock().unwrap();
            let mut chain = i.panes.get(old).map(|p| p.replaced.clone()).unwrap_or_default();
            chain.push(old.to_string());
            let skip = chain.len().saturating_sub(REPLACED_KEEP);
            if let Some(p) = i.panes.get_mut(new) {
                p.replaced = chain.split_off(skip);
            }
        }
        // 데려온 thread는 그 pane이 정본이다(다시 띄울 launch가 없다): 새 pane이 대신하면 thread도 따라간다
        if let Some(r) = self.thread_of_pane(old).filter(|r| r.kind == aaa_proto::ThreadKind::Adopted) {
            aaa_common::diagnostic::ignored(
                self.update_rec(&r.project, &r.id, |x| x.pane = Some(new.to_string())),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        {
            let mut ws = self.workspaces.lock().unwrap();
            let changed: Vec<Workspace> =
                ws.values_mut().filter_map(|w| aaa_proto::layout::ws_replace(w, old, new).then(|| w.clone())).collect();
            drop(ws);
            for w in changed {
                self.save_workspace(&w);
            }
        }
        if let Err(e) = self.forget(old, mid, actor).await {
            tracing::warn!(pane = %old, "재개 뒤 옛 기록 정리 실패: {}", e.message);
        }
    }

    /// 세션에 끝내기를 보낸다. host가 끝낸 pane은 스스로 끝난 셸이 아니다(`exited_by_itself`).
    pub(super) async fn kill_session(&self, conn: &AgentConn, id: &str) -> Result<AgentResponse, ErrorBody> {
        if let Some(p) = self.inner.lock().unwrap().panes.get_mut(id) {
            p.kill_sent = true;
        }
        conn.request(AgentRequest::Kill { session: id.to_string() }).await
    }

    /// 스스로 끝난 셸 pane을 닫는다(herdr 0.9.5 `handle_pane_died`·`Workspace::remove_pane`): 기록을 지우고 모든 layout에서
    /// 뺀다(`forget`, 빈 탭은 없어진다). 그래서 탭이 하나도 남지 않은 workspace도 지운다(herdr는 마지막 pane이 끝난
    /// workspace를 지운다).
    pub(super) fn spawn_close_exited(self: &Arc<Self>, id: String, mid: String) {
        let me = self.clone();
        tokio::spawn(async move { me.close_record(&id, &mid, "exit").await });
    }

    /// 끝난 pane을 닫는다(셸 exit, 기록 편집기 overlay): 기록을 지우고 모든 layout에서 빼고, 탭이 하나도 남지 않은 workspace도 지운다
    pub(super) async fn close_record(self: &Arc<Self>, id: &str, mid: &str, actor: &str) {
        let had: Vec<String> = {
            let ws = self.workspaces.lock().unwrap();
            ws.values().filter(|w| aaa_proto::layout::ws_contains(w, id)).map(|w| w.name.clone()).collect()
        };
        if let Err(e) = self.forget(id, mid, actor).await {
            tracing::warn!(pane = %id, actor, "끝난 pane을 닫지 못했다: {}", e.message);
            return;
        }
        for name in had {
            let emptied = {
                let mut ws = self.workspaces.lock().unwrap();
                let empty = ws.get(&name).is_some_and(|w| w.tabs.is_empty());
                empty && ws.remove(&name).is_some()
            };
            if !emptied {
                continue;
            }
            if let Err(e) = self.db.delete_workspace(&name) {
                tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
            }
            self.db.audit(actor, "workspace_close", &name);
            aaa_common::diagnostic::ignored(
                self.layout_events.send(Workspace { name, tabs: vec![] }),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    pub(super) async fn forget(self: &Arc<Self>, id: &str, mid: &str, actor: &str) -> Result<(), ErrorBody> {
        if self.inner.lock().unwrap().panes.get(id).is_some_and(|p| p.info.state.is_running()) {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 지울 수 없다"));
        }
        let conn = self.live_conn(mid)?;
        conn.request(AgentRequest::Forget { session: id.to_string() }).await?;
        self.inner.lock().unwrap().panes.remove(id);
        let changed: Vec<Workspace> = {
            let mut ws = self.workspaces.lock().unwrap();
            ws.values_mut().filter_map(|w| aaa_proto::layout::ws_remove(w, id).then(|| w.clone())).collect()
        };
        for w in changed {
            self.save_workspace(&w);
        }
        self.db.audit(actor, "pane_forget", id);
        self.bump();
        Ok(())
    }
}
