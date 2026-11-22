//! 머신·pane 조회, pane 붙기와 앞 기록 읽기.

use super::*;

impl Host {
    // ───────────── 보기 ─────────────

    /// 연결된 agent가 hello에서 알린 실행 신원.
    pub(crate) fn machine_identity(&self, mid: &str) -> Option<aaa_proto::RuntimeIdentity> {
        let i = self.inner.lock().unwrap();
        i.machines.get(mid).filter(|m| m.conn.is_some()).and_then(|m| m.identity.clone())
    }

    pub(crate) fn machine_feature(&self, mid: &str, feature: &str) -> bool {
        self.inner.lock().unwrap().machines.get(mid).is_some_and(|m| m.features.iter().any(|f| f == feature))
    }

    pub fn machine_views(&self) -> Vec<MachineView> {
        let host_software = aaa_common::util::software();
        // DB·할 일 파일은 inner 잠금 밖에서 읽는다
        let (mut reservations, weights) = (self.machine_reservations(), self.machine_weights());
        let i = self.inner.lock().unwrap();
        let mut v: Vec<_> = i
            .machines
            .iter()
            .map(|(id, m)| MachineView {
                id: id.clone(),
                name: m.name.clone(),
                local: m.local,
                connection: if m.conn.is_some() { Connection::Connected } else { Connection::Disconnected },
                os: m.os.clone(),
                boot_id: m.boot_id.clone(),
                dev_mode: m.dev_mode,
                last_seen_ms: m.last_seen_ms,
                health: m.health.as_ref().map(|h| h.0.clone()),
                clock_skew_ms: m.health.as_ref().map(|h| h.1),
                problems: health::problems(m),
                arch: m.build.arch.clone(),
                software_differs: m.build.software.as_ref().is_some_and(|s| !util::same_software(s, &host_software)),
                software: m.build.software.as_deref().map(util::short_software),
                sha256: m.build.sha256.clone(),
                features: m.features.clone(),
                selfcheck: m.build.selfcheck.clone(),
                reservation: reservations.remove(id),
                weight: weights.get(id).copied(),
            })
            .collect();
        v.sort_by(|a, b| b.local.cmp(&a.local).then(a.name.cmp(&b.name)));
        v
    }

    /// 잠금 아래 실행 상태만 복사한다. 경로와 계정은 반환한 뒤 조회한다.
    pub(super) fn pane_snapshot(i: &Inner, id: &str, p: &PaneRt, role: DeviceRole) -> (PaneView, bool) {
        let m = i.machines.get(&p.machine_id);
        let connected = m.is_some_and(|m| m.conn.is_some());
        let local_agent = m.is_some_and(|m| m.local)
            && (p.info.role == Some(aaa_proto::Role::Coordinator)
                || p.agent.is_some()
                || crate::adopt::harness_of(p.info.harness.as_deref(), &p.info.argv).is_some());
        let view = PaneView {
            responsibility: p.info.responsibility.clone(),
            account: None,
            id: id.to_string(),
            machine_id: p.machine_id.clone(),
            machine: m.map(|m| m.name.clone()).unwrap_or_default(),
            name: p.info.name.clone(),
            argv: p.info.argv.clone(),
            cwd: p.info.cwd.clone(),
            state: p.info.state.clone(),
            connection: if connected { Connection::Connected } else { Connection::Disconnected },
            readonly: !connected || !p.info.state.is_running() || !role.can_control(),
            cols: p.info.cols,
            rows: p.info.rows,
            lease: p.lease.as_ref().map(|l| LeaseView { front: l.id.clone(), label: l.label.clone() }),
            role: p.info.role,
            coordinator_project: None,
            agent: p.agent.clone().map(Box::new),
            label: p.info.label.clone(),
            restore_error: p.restore_error.clone(),
            replaced: p.replaced.clone(),
        };
        (view, local_agent)
    }

    pub(super) fn finish_pane_view(
        &self,
        (mut view, local_agent): (PaneView, bool),
        projects: &[(String, PathBuf)],
        resolve: impl FnOnce(&str) -> std::io::Result<PathBuf>,
    ) -> PaneView {
        if local_agent
            && let Some(cwd) = view.cwd.as_deref()
            && let Ok(dir) = resolve(cwd).inspect_err(|e| aaa_common::diagnostic::observe(e, "pane 경로 정규화 실패"))
        {
            view.coordinator_project = projects.iter().find(|(_, path)| *path == dir).map(|(name, _)| name.clone());
        }
        view.account = self
            .pane_account(&view.id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .map(|a| a.account_label.unwrap_or(a.account));
        view
    }

    pub fn pane_views(&self, role: DeviceRole) -> Vec<PaneView> {
        self.pane_views_resolving(role, |cwd| std::fs::canonicalize(cwd))
    }

    pub(super) fn pane_views_resolving(
        &self,
        role: DeviceRole,
        resolve: impl Fn(&str) -> std::io::Result<PathBuf>,
    ) -> Vec<PaneView> {
        let projects = self.coordinator_projects();
        let snapshots: Vec<_> = {
            let i = self.inner.lock().unwrap();
            i.panes.iter().map(|(id, p)| Self::pane_snapshot(&i, id, p, role)).collect()
        };
        let mut v: Vec<_> = snapshots.into_iter().map(|p| self.finish_pane_view(p, &projects, &resolve)).collect();
        // 처음 띄운 차례(ULID): 재개·재부팅 복원으로 새 ID가 되어도 처음 ID 자리에 남는다.
        let first = |p: &PaneView| p.replaced.first().cloned().unwrap_or_else(|| p.id.clone());
        v.sort_by(|a, b| a.machine.cmp(&b.machine).then_with(|| first(a).cmp(&first(b))));
        v
    }

    pub fn pane_view_of(&self, id: &str, role: DeviceRole) -> Option<PaneView> {
        let projects = self.coordinator_projects();
        let snapshot = {
            let i = self.inner.lock().unwrap();
            i.panes.get(id).map(|p| Self::pane_snapshot(&i, id, p, role))
        }?;
        Some(self.finish_pane_view(snapshot, &projects, |cwd| std::fs::canonicalize(cwd)))
    }

    pub(super) fn coordinator_projects(&self) -> Vec<(String, std::path::PathBuf)> {
        self.project_directories()
            .into_iter()
            .filter(|(name, _)| self.project_status(name) != aaa_proto::ProjectStatus::Archived)
            .filter_map(|(name, dir)| {
                std::fs::canonicalize(dir)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .map(|dir| (name, dir))
            })
            .collect()
    }

    /// 웹 터미널 attach: 사건 구독과 snapshot을 같은 잠금 안에서 잡아 빈틈이 없게 한다.
    pub fn attach_pane(&self, id: &str) -> Option<Attach> {
        let i = self.inner.lock().unwrap();
        let p = i.panes.get(id)?;
        let rx = p.events.subscribe();
        let (replay, more, offset, rows, cols) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                let (replay, more) = m.vt.replay_more();
                (replay, more || m.source_more, m.offset, rows, cols)
            }
            None => (Vec::new(), false, 0, p.info.rows, p.info.cols),
        };
        Some(Attach { rx, replay, more, offset, cols, rows })
    }

    /// 웹이 위로 굴려 받은 기록 끝에 닿았다: host mirror 기록에서 anchor 바로 앞 덩어리(plan §8.67).
    /// mirror는 처음 구독 snapshot(최근 2000줄)부터 채워진다. 그보다 앞은 session host에만 있으므로, mirror에서 더 줄 것이
    /// 없으면 session host에 묻는다(plan §8.118, QA t-0337 Q8). 옛 agent·session host면 묻지 않는다.
    pub fn older_history(&self, id: &str, anchor: &[String], depth: usize) -> OlderHistory {
        let i = self.inner.lock().unwrap();
        let Some(p) = i.panes.get(id) else { return OlderHistory::Ready(None) };
        let Some(m) = p.mirror.as_ref() else { return OlderHistory::Ready(None) };
        let found = m.vt.older(anchor, depth, aaa_vt::REPLAY_HISTORY);
        match found {
            Some(mut o) if o.lines > 0 => {
                o.more |= m.source_more;
                OlderHistory::Ready(Some(o))
            }
            found if m.source_more => {
                let conn = i
                    .machines
                    .get(&p.machine_id)
                    .filter(|mc| mc.features.iter().any(|f| f == "history.v1"))
                    .and_then(|mc| mc.conn.clone());
                match conn {
                    Some(c) => OlderHistory::Ask(c),
                    None => OlderHistory::Ready(found),
                }
            }
            found => OlderHistory::Ready(found),
        }
    }

    /// session host에 앞 기록을 묻는다(plan §8.118). 실패하면 None(웹은 더 받지 않는다).
    pub async fn older_from_agent(
        conn: &AgentConn,
        session: String,
        anchor: Vec<String>,
        depth: usize,
    ) -> Option<aaa_vt::Older> {
        let depth = u32::try_from(depth).unwrap_or(u32::MAX);
        match conn.request(AgentRequest::History { session, anchor, depth }).await {
            Ok(AgentResponse::History(c)) if c.found => {
                Some(aaa_vt::Older { data: c.data.0, lines: c.lines as usize, more: c.more })
            }
            _ => None,
        }
    }

    pub(super) fn resolve_pane(&self, prefix: &str) -> Result<(String, String), ErrorBody> {
        let i = self.inner.lock().unwrap();
        if let Some(p) = i.panes.get(prefix) {
            return Ok((prefix.to_string(), p.machine_id.clone()));
        }
        let up = prefix.to_ascii_uppercase();
        let mut hits: Vec<_> = i.panes.iter().filter(|(id, _)| prefix.len() >= 4 && id.starts_with(&up)).collect();
        if hits.is_empty() {
            // ID가 아니면 이름: 실행 중인 것을 먼저, 없으면 끝난 것까지
            let named: Vec<_> = i.panes.iter().filter(|(_, p)| p.info.name == prefix).collect();
            let running: Vec<_> = named.iter().copied().filter(|(_, p)| p.info.state.is_running()).collect();
            hits = if running.is_empty() { named } else { running };
        }
        match hits.as_slice() {
            [(id, p)] => Ok(((*id).clone(), p.machine_id.clone())),
            [] => Err(err(ErrorCode::NotFound, format!("pane이 없다: {prefix}"))),
            _ => Err(err(ErrorCode::BadRequest, format!("pane ID가 모호하다: {prefix}"))),
        }
    }

    pub(super) fn resolve_machine(&self, name: Option<&str>) -> Result<String, ErrorBody> {
        let i = self.inner.lock().unwrap();
        let found = match name {
            Some(n) => i.machines.iter().find(|(id, m)| m.name == n || id.as_str() == n),
            None => i.machines.iter().find(|(_, m)| m.local),
        };
        found.map(|(id, _)| id.clone()).ok_or_else(|| {
            err(
                ErrorCode::NotFound,
                match name {
                    Some(n) => format!("머신이 없다: {n}"),
                    None => "로컬 agent가 없다. --machine으로 머신을 고른다".into(),
                },
            )
        })
    }

    pub(super) fn live_conn(&self, machine_id: &str) -> Result<AgentConn, ErrorBody> {
        self.conn_of(machine_id).ok_or_else(|| err(ErrorCode::AgentDisconnected, "agent와 연결이 끊겨 있다(읽기 전용)"))
    }

    pub(super) fn mirror_text(&self, id: &str) -> Option<ScreenText> {
        let i = self.inner.lock().unwrap();
        i.panes.get(id).and_then(|p| p.mirror.as_ref()).map(|m| m.vt.text(id))
    }
}
