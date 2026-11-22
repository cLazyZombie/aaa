//! workspace·tab 배치(layout)와 닫기.

use super::*;

impl Host {
    // ───────────── workspace layout(host 정본) ─────────────

    pub fn workspace(&self, name: &str) -> Workspace {
        self.workspaces
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Workspace { name: name.to_string(), tabs: vec![] })
    }

    pub(super) fn save_workspace(&self, w: &Workspace) {
        if let Err(e) = self.db.save_workspace(&w.name, &serde_json::to_string(w).unwrap_or_default()) {
            tracing::warn!(workspace = %w.name, "workspace 저장 실패: {e}");
        }
        self.workspaces.lock().unwrap().insert(w.name.clone(), w.clone());
        aaa_common::diagnostic::ignored(self.layout_events.send(w.clone()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
    }

    pub(super) async fn layout_apply(
        self: &Arc<Self>,
        name: &str,
        change: LayoutChange,
        actor: &str,
    ) -> Result<Workspace, ErrorBody> {
        let pane_machine = |id: &str| self.inner.lock().unwrap().panes.get(id).map(|p| p.machine_id.clone());
        let mut w = self.workspace(name);
        match change {
            LayoutChange::NewTab { machine, name: pname, argv, cwd, cols, rows, label, from } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let cwd = self.new_pane_cwd(cwd, from.as_deref(), &mid).await;
                let spec = SpawnSpec {
                    name: pname.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
                    harness: None,
                    profile: None,
                    resolved_profile: None,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                // spawn 사이에 다른 변경이 있었을 수 있다: 최신을 다시 읽어 붙인다
                w = self.workspace(name);
                let label = label.map(|l| l.trim().to_string()).unwrap_or_default();
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label,
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Split { pane, dir, machine, argv, cwd, ratio } => {
                let (target, target_mid) = self.resolve_pane(&pane)?;
                if !aaa_proto::layout::ws_contains(&w, &target) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
                let mid = match machine {
                    Some(m) => self.resolve_machine(Some(&m))?,
                    None => pane_machine(&target).unwrap_or(target_mid),
                };
                let cwd = self.new_pane_cwd(cwd, Some(&target), &mid).await;
                let spec = SpawnSpec {
                    name: String::new(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: 80,
                    rows: 24,
                    role: None,
                    harness: None,
                    profile: None,
                    resolved_profile: None,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                w = self.workspace(name);
                let done = w.tabs.iter_mut().any(|t| {
                    // herdr: 나누면 그 탭의 zoom을 푼다
                    let hit = aaa_proto::layout::split(&mut t.root, &target, &id, dir);
                    t.zoomed &= !hit;
                    hit
                });
                if let Some(r) = ratio {
                    // herdr `pane split --ratio`: 새로 생긴 나눔의 a 쪽 비율
                    for t in &mut w.tabs {
                        aaa_proto::layout::set_split_ratio(&mut t.root, &id, r.clamp(100, 900));
                    }
                }
                if !done {
                    return Err(err(ErrorCode::NotFound, "나누는 사이에 pane이 layout에서 빠졌다"));
                }
            }
            LayoutChange::AddTab { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                if aaa_proto::layout::ws_contains(&w, &id) {
                    return Err(err(ErrorCode::BadRequest, "이미 이 workspace에 있다"));
                }
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: String::new(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Close { pane, kill } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                if kill && self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
                    let conn = self.live_conn(&mid)?;
                    self.db.audit(actor, "pane_kill", &id);
                    self.kill_session(&conn, &id).await?;
                }
                if !aaa_proto::layout::ws_remove(&mut w, &id) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::Resize { pane, direction, amount } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // herdr `handle_pane_resize`: 기본 0.05, 최대 0.5
                let amount = amount.unwrap_or(50).min(500);
                let mut found = false;
                for t in &mut w.tabs {
                    if aaa_proto::layout::leaves(&t.root).contains(&id) {
                        found = true;
                        aaa_proto::layout::resize(&mut t.root, &id, direction, amount);
                    }
                }
                if !found {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::RenameTab { tab, name: label } => {
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // 앞뒤 빈칸은 뺀다. 빈 이름은 이름 없음(탭 번호로 보인다)
                t.name = label.trim().to_string();
            }
            LayoutChange::CloseTab { tab } => {
                let at = w
                    .tabs
                    .iter()
                    .position(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                let t = w.tabs.remove(at);
                // 탭을 뺀 layout을 먼저 저장한다(front에서 바로 사라진다). pane을 끝내는 동안 기다리므로, 그 사이 다른
                // 변경(새 탭·이름 바꾸기·나누기)이 있을 수 있다: 끝낸 뒤 최신을 다시 읽어 그것을 저장한다(보안 감사 t-0340:
                // 맨 위에서 복사한 것을 저장하면 그 사이 변경이 사라졌다)
                self.save_workspace(&w);
                self.close_panes(&aaa_proto::layout::leaves(&t.root), name, actor).await;
                w = self.workspace(name);
            }
            LayoutChange::MoveToNewTab { pane, name: label } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // 옮긴다: 다른 workspace에 있던 자리도 뺀다(herdr `pane move`는 pane이 한 곳에만 있다)
                let others: Vec<Workspace> = {
                    let mut ws = self.workspaces.lock().unwrap();
                    ws.values_mut()
                        .filter(|o| o.name != name)
                        .filter_map(|o| aaa_proto::layout::ws_remove(o, &id).then(|| o.clone()))
                        .collect()
                };
                for o in others {
                    self.save_workspace(&o);
                }
                aaa_proto::layout::ws_remove(&mut w, &id);
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label.unwrap_or_default(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Zoom { pane, zoomed } => {
                let (id, _) = self.resolve_pane(&pane)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| aaa_proto::layout::leaves(&t.root).contains(&id))
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")))?;
                // herdr `apply_pane_zoom`: pane이 하나뿐인 탭은 그대로
                if aaa_proto::layout::leaves(&t.root).len() > 1 {
                    t.zoomed = zoomed.unwrap_or(!t.zoomed);
                }
            }
            LayoutChange::Swap { pane, with } => {
                let (a, _) = self.resolve_pane(&pane)?;
                let (b, _) = self.resolve_pane(&with)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| {
                        let l = aaa_proto::layout::leaves(&t.root);
                        l.contains(&a) && l.contains(&b)
                    })
                    .ok_or_else(|| {
                        err(ErrorCode::BadRequest, format!("workspace {name}의 한 탭에 {pane}·{with}가 함께 없다"))
                    })?;
                aaa_proto::layout::swap(&mut t.root, &a, &b);
            }
            LayoutChange::SplitRatio { tab, path, ratio } => {
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // herdr `handle_layout_set_split_ratio`: 그 자리에 나눔이 없으면 split_not_found
                if !aaa_proto::layout::set_ratio_at(&mut t.root, &path, ratio) {
                    return Err(err(ErrorCode::NotFound, format!("탭 {tab}에 그 나눔({path:?})이 없다")));
                }
            }
            LayoutChange::MoveTab { tab, index } => {
                let from = w
                    .tabs
                    .iter()
                    .position(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // herdr `handle_tab_move`: insert_index는 0..=탭 수
                if usize::from(index) > w.tabs.len() {
                    return Err(err(ErrorCode::BadRequest, format!("탭 자리 {index}가 0..={} 밖이다", w.tabs.len())));
                }
                aaa_proto::layout::move_tab(&mut w.tabs, from, usize::from(index));
            }
        }
        self.db.audit(actor, "layout", name);
        self.save_workspace(&w);
        Ok(w)
    }

    /// 탭·workspace를 닫을 때 그 안 pane을 끝내고 기록에서 지운다(herdr `tab close`·`workspace close`: pane이 사라진다).
    /// 역할이 있는 pane(thread·coordinator·평가자)은 끝내지 않는다: 그 수명은 thread 명령(resolve·stop)이 정한다.
    /// `closing` 밖 다른 workspace에도 있는 pane은 자리만 빠진다. agent가 끊긴 pane은 끝낼 수 없어 기록을 둔다.
    pub(super) async fn close_panes(self: &Arc<Self>, panes: &[String], closing: &str, actor: &str) {
        for id in panes {
            let target = {
                let i = self.inner.lock().unwrap();
                i.panes.get(id).map(|p| (p.machine_id.clone(), p.info.state.is_running(), p.info.role.is_some()))
            };
            let Some((mid, running, has_role)) = target else { continue };
            let elsewhere = self
                .workspaces
                .lock()
                .unwrap()
                .values()
                .any(|w| w.name != closing && aaa_proto::layout::ws_contains(w, id));
            if has_role || elsewhere {
                continue;
            }
            let Ok(conn) = self.live_conn(&mid).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if running {
                let Ok(rx) = self.pane_events(id).inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
                }) else {
                    continue;
                };
                self.db.audit(actor, "pane_kill", id);
                if let Err(e) = self.kill_session(&conn, id).await {
                    tracing::warn!(pane = %id, "닫으며 끝내지 못했다: {}", e.message);
                    continue;
                }
                if let Err(e) = self
                    .wait_pane(id, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다")
                    .await
                {
                    tracing::warn!(pane = %id, "{}", e.message);
                    continue;
                }
            }
            if let Err(e) = self.forget(id, &mid, actor).await {
                tracing::warn!(pane = %id, "닫으며 기록을 지우지 못했다: {}", e.message);
            }
        }
    }

    /// pane이 들어 있는 workspace(main을 먼저). layout 요청에 workspace를 주지 않으면 이것을 쓴다(herdr는 pane ID가 workspace를 정한다).
    pub fn workspace_of_pane(&self, pane: &str) -> Option<String> {
        let (id, _) = self
            .resolve_pane(pane)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let ws = self.workspaces.lock().unwrap();
        if ws.get(DEFAULT_WORKSPACE).is_some_and(|w| aaa_proto::layout::ws_contains(w, &id)) {
            return Some(DEFAULT_WORKSPACE.to_string());
        }
        let mut names: Vec<_> =
            ws.values().filter(|w| aaa_proto::layout::ws_contains(w, &id)).map(|w| w.name.clone()).collect();
        names.sort();
        names.into_iter().next()
    }

    /// 탭이 들어 있는 workspace
    pub fn workspace_of_tab(&self, tab: &str) -> Option<String> {
        let ws = self.workspaces.lock().unwrap();
        ws.values().find(|w| w.tabs.iter().any(|t| t.id == tab)).map(|w| w.name.clone())
    }

    pub(super) fn workspace_summaries(&self) -> Vec<aaa_proto::WorkspaceSummary> {
        let ws = self.workspaces.lock().unwrap();
        let mut out: Vec<_> = ws
            .values()
            .map(|w| aaa_proto::WorkspaceSummary {
                name: w.name.clone(),
                tabs: w.tabs.len() as u32,
                panes: w.tabs.iter().map(|t| aaa_proto::layout::leaves(&t.root).len() as u32).sum(),
            })
            .collect();
        if !ws.contains_key(DEFAULT_WORKSPACE) {
            out.push(aaa_proto::WorkspaceSummary { name: DEFAULT_WORKSPACE.to_string(), tabs: 0, panes: 0 });
        }
        out.sort_by(|a, b| (a.name != DEFAULT_WORKSPACE).cmp(&(b.name != DEFAULT_WORKSPACE)).then(a.name.cmp(&b.name)));
        out
    }

    /// workspace를 닫는다: 그 안 pane을 끝내고(역할 있는 pane은 두고) layout을 지운다. front에는 빈 workspace로 알린다.
    pub(super) async fn workspace_close(self: &Arc<Self>, name: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let w = self
            .workspaces
            .lock()
            .unwrap()
            .remove(name)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?;
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        let panes: Vec<String> = w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root)).collect();
        self.close_panes(&panes, name, actor).await;
        self.db.audit(actor, "workspace_close", name);
        aaa_common::diagnostic::ignored(
            self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] }),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() })
    }

    pub(super) fn workspace_rename(&self, name: &str, to: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let to = to.trim();
        if to.is_empty() {
            return Err(err(ErrorCode::BadRequest, "새 이름이 비었다"));
        }
        let mut w = {
            let mut ws = self.workspaces.lock().unwrap();
            if ws.contains_key(to) {
                return Err(err(ErrorCode::NameTaken, format!("이미 있는 workspace: {to}")));
            }
            ws.remove(name).ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?
        };
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        aaa_common::diagnostic::ignored(
            self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] }),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        w.name = to.to_string();
        self.save_workspace(&w);
        self.db.audit(actor, "workspace_rename", &format!("{name} -> {to}"));
        Ok(FrontResponse::Workspace(w))
    }

    /// 새 pane의 cwd(herdr `resolve_new_terminal_cwd`): 준 값이 먼저, 없으면 `[terminal] new_cwd`.
    /// follow면 원래 pane(`from`)이 같은 머신에 있을 때 그 pane이 지금 있는 폴더(agent `cwd.v1`),
    /// 읽지 못하면 그 pane을 띄운 폴더(herdr `launch_cwd_for_terminal`). 다른 머신이면 그 머신 기본 위치.
    pub(super) async fn new_pane_cwd(&self, given: Option<String>, from: Option<&str>, mid: &str) -> Option<String> {
        if given.is_some() {
            return given;
        }
        match self.cfg.terminal.new_cwd.as_str() {
            "follow" => {}
            "home" => return None,
            path => return Some(path.to_string()),
        }
        let (src, _) = self
            .resolve_pane(from?)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let (conn, launch) = {
            let i = self.inner.lock().unwrap();
            let p = i.panes.get(&src)?;
            if p.machine_id != mid {
                return None;
            }
            let m = i.machines.get(mid);
            let can_ask = p.info.state.is_running() && m.is_some_and(|m| m.features.iter().any(|f| f == "cwd.v1"));
            (m.and_then(|m| m.conn.clone()).filter(|_| can_ask), p.info.cwd.clone())
        };
        if let Some(conn) = conn
            && let Ok(AgentResponse::Cwd { cwd: Some(cwd) }) = conn.request(AgentRequest::Cwd { session: src }).await
        {
            return Some(cwd);
        }
        launch
    }

    pub(super) fn check_running(&self, id: &str) -> Result<(), ErrorBody> {
        let i = self.inner.lock().unwrap();
        match i.panes.get(id).map(|p| &p.info.state) {
            Some(SessionState::Running) => Ok(()),
            Some(_) => Err(err(ErrorCode::SessionExited, "세션이 끝났다")),
            None => Err(err(ErrorCode::NotFound, "pane이 없다")),
        }
    }
}
