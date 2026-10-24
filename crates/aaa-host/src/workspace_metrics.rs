//! 웹은 host 요약만 읽는다. agent 수집은 기존 보고 회수의 끝에서만 실행한다.
use crate::{Host, err};
use aaa_proto::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

struct Registration {
    project: String,
    path: String,
    registration: String,
    machine: String,
    view: RepoWorkspace,
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}
fn period_start(now: u64, days: u32, offset_minutes: i32) -> u64 {
    if days != 1 {
        return now.saturating_sub(days as u64 * 86_400_000);
    }
    let offset = offset_minutes.clamp(-720, 840) as i64 * 60_000;
    (((now as i64 + offset).div_euclid(86_400_000) * 86_400_000 - offset).max(0)) as u64
}
impl Host {
    fn metric_registrations(&self) -> Result<Vec<Registration>, ErrorBody> {
        let mut out = Vec::new();
        for project in self.projects() {
            for repo in self.settings(&project.name).repos {
                let Ok(machine) = self.resolve_machine(repo.machine.as_deref()).inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
                }) else {
                    continue;
                };
                let Some(view) = self.machine_views().into_iter().find(|m| m.id == machine) else { continue };
                let key = hex::encode(Sha256::digest(format!("{machine}:{}", repo.path).as_bytes()));
                let id = self.db.repo_workspace_id(&project.name, &key, None).map_err(internal)?;
                let collection =
                    self.db.meta(&format!("workspace-collection::{id}:{}", view.name)).map_err(internal)?.and_then(
                        |s| {
                            serde_json::from_str::<(u64, Vec<String>)>(&s)
                                .inspect_err(|error| {
                                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                                })
                                .ok()
                        },
                    );
                out.push(Registration {
                    project: project.name.clone(),
                    path: repo.path.clone(),
                    registration: key,
                    machine: machine.clone(),
                    view: RepoWorkspace {
                        id,
                        project: project.name.clone(),
                        name: crate::task_repo::repo_name(&repo.path).unwrap_or("저장소").into(),
                        machines: vec![WorkspaceMachine {
                            name: view.name,
                            connected: view.connection == Connection::Connected,
                            supported: self
                                .inner
                                .lock()
                                .unwrap()
                                .machines
                                .get(&machine)
                                .is_some_and(|m| m.features.iter().any(|f| f == "workspace_metrics.v2")),
                        }],
                        last_collected_ms: collection.as_ref().map(|c| c.0).filter(|t| *t > 0),
                        collection_notes: collection.map(|c| c.1).unwrap_or_default(),
                    },
                });
            }
        }
        Ok(out)
    }
    pub(crate) fn workspace_dashboard(&self, query: WorkspaceQuery) -> Result<WorkspaceDashboard, ErrorBody> {
        let now = aaa_common::util::now_ms();
        let mut groups: BTreeMap<String, RepoWorkspace> = BTreeMap::new();
        for r in self.metric_registrations()? {
            if r.view.id.is_empty() {
                continue;
            }
            let v =
                groups.entry(r.view.id.clone()).or_insert_with(|| RepoWorkspace { machines: vec![], ..r.view.clone() });
            for m in r.view.machines {
                if !v.machines.iter().any(|x| x.name == m.name) {
                    v.machines.push(m);
                }
            }
            v.last_collected_ms = v.last_collected_ms.max(r.view.last_collected_ms);
            v.collection_notes.extend(r.view.collection_notes);
            v.collection_notes.sort();
            v.collection_notes.dedup();
        }
        let mut workspaces: Vec<_> = groups.into_values().collect();
        workspaces.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let selected = query
            .workspace_id
            .as_ref()
            .and_then(|id| workspaces.iter().find(|w| &w.id == id))
            .or_else(|| if query.workspace_id.is_none() { workspaces.first() } else { None });
        let mut dashboard =
            WorkspaceDashboard { workspaces: workspaces.clone(), queried_at_ms: now, ..Default::default() };
        let Some(workspace) = selected else { return Ok(dashboard) };
        let days = query.days.unwrap_or(7).clamp(1, 30);
        let since = period_start(now, days, query.utc_offset_minutes.unwrap_or(0));
        let id = &workspace.id;
        let (mut runs, mut lines, truncated) = self.db.workspace_records("", id, since).map_err(internal)?;
        dashboard.profiles =
            runs.iter().filter_map(|r| r.profile.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        dashboard.counts_run = runs
            .iter()
            .filter(|r| r.tests.is_some() || r.e2e_files.is_some())
            .max_by_key(|r| r.observed_at_ms)
            .cloned()
            .map(Box::new);
        runs.retain(|r| {
            query.machine.as_ref().is_none_or(|m| &r.machine == m)
                && query.profile.as_ref().is_none_or(|p| r.profile.as_deref().unwrap_or("unknown") == p)
        });
        runs.sort_by_key(|r| r.observed_at_ms);
        lines.sort_by_key(|r| r.observed_at_ms);
        lines.retain(|l| l.branch == "main");
        if query.code_axis.as_deref() != Some("commit") {
            let offset = query.utc_offset_minutes.unwrap_or(0).clamp(-720, 840) as i64 * 60_000;
            let mut days = BTreeMap::new();
            for l in lines {
                let day = (l.observed_at_ms as i64 + offset).div_euclid(86_400_000);
                days.insert((day, l.rules_version), l);
            }
            lines = days.into_values().collect();
        }
        dashboard.verification_trend = points(&runs, &query, |r| r.verification_ms);
        dashboard.build_trend = points(&runs, &query, |r| r.build_ms);
        dashboard.artifact_trend = points(&runs, &query, |r| r.artifact_bytes);
        dashboard.workspace_id = Some(workspace.id.clone());
        dashboard.runs = runs;
        dashboard.code_lines = lines;
        dashboard.truncated = truncated;
        Ok(dashboard)
    }
    /// 실패해도 보고·검증의 성공 여부를 바꾸지 않는다.원문 로그·경로는 웹에 전달하지 않는다.
    pub(crate) async fn collect_workspace_metrics(self: &Arc<Self>, project: &str, thread: &str) {
        let _gate = self.metrics_gate.lock().await;
        let Ok(rec) = self
            .load(project, thread)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let Some(repo) = rec.repo.as_deref() else { return };
        let Ok(registrations) = self
            .metric_registrations()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let Some(reg) =
            registrations.into_iter().find(|r| r.project == project && r.machine == rec.machine_id && r.path == repo)
        else {
            return;
        };
        if !reg.view.machines[0].supported {
            return;
        }
        let Ok(conn) = self
            .live_conn(&rec.machine_id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let cwd = repo.to_string();
        let id = &reg.view.id;
        let mut value = match conn
            .request(AgentRequest::WorkspaceMetrics {
                cwd,
                workspace_key: reg.view.id.clone(),
                known_code_commits: self.db.workspace_code_commits("", id).unwrap_or_default(),
            })
            .await
        {
            Ok(AgentResponse::WorkspaceMetrics(value)) => value,
            other => {
                match other {
                    Err(error) => tracing::warn!(%error, machine=%reg.machine, "workspace 수집 요청 실패"),
                    Ok(_) => tracing::warn!(machine=%reg.machine, "workspace 수집 응답 종류 불일치"),
                }
                if self
                    .db
                    .workspace_collection_error("", id, &reg.view.machines[0].name)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
                {
                    self.workspace_metrics_changed.send_modify(|n| *n = n.wrapping_add(1));
                }
                return;
            }
        };
        let Some(remote) = value.remote_id.as_deref() else { return };
        let id = match self.db.repo_workspace_id(project, &reg.registration, Some(remote)) {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(%error, "workspace 원격 신원 저장 실패");
                return;
            }
        };
        // 응답은 고정 상한의 작은 요약만 보관한다.
        if value.runs.len() > 256 {
            value.runs.truncate(256);
            value.notes.push("실행 수집 상한".into());
        }
        for run in &mut value.runs {
            run.slow_tests.items.truncate(5);
            run.slow_e2e.items.truncate(5);
        }
        if let Err(e) = self.db.save_workspace_metrics("", &id, &reg.view.machines[0].name, &value) {
            tracing::warn!(error=%e,"workspace 지표 보존 실패");
        } else {
            self.workspace_metrics_changed.send_modify(|n| *n = n.wrapping_add(1));
        }
    }
}
fn points(runs: &[MetricRun], query: &WorkspaceQuery, value: impl Fn(&MetricRun) -> Option<u64>) -> Vec<MetricPoint> {
    let points: Vec<_> = runs
        .iter()
        .filter_map(|r| {
            Some(MetricPoint {
                at_ms: r.observed_at_ms,
                machine: r.machine.clone(),
                profile: r.profile.clone(),
                run_id: r.run_id.clone(),
                value: value(r)?,
                samples: 1,
                outcome: r.outcome.clone(),
            })
        })
        .collect();
    if query.granularity.as_deref() != Some("day") {
        return points;
    }
    let offset = query.utc_offset_minutes.unwrap_or(0).clamp(-720, 840) as i64 * 60_000;
    let mut groups: BTreeMap<(i64, String, Option<String>), Vec<MetricPoint>> = BTreeMap::new();
    for p in points.into_iter().filter(|p| p.outcome == "succeeded") {
        let day = (p.at_ms as i64 + offset).div_euclid(86_400_000);
        groups.entry((day, p.machine.clone(), p.profile.clone())).or_default().push(p);
    }
    groups
        .into_iter()
        .map(|((day, _, _), mut g)| {
            g.sort_by_key(|a| a.value);
            let mid = g.len() / 2;
            let n = if g.len().is_multiple_of(2) {
                g[mid - 1].value + (g[mid].value - g[mid - 1].value) / 2
            } else {
                g[mid].value
            };
            let mut point = g[mid].clone();
            point.value = n;
            point.samples = g.len() as u32;
            point.at_ms = (day * 86_400_000 - offset).max(0) as u64;
            point
        })
        .collect()
}
#[cfg(test)]
mod tests;
