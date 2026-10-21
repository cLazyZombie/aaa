//! A2 등록 목록 기반 수집. 자동 배정·새 모델 작업은 만들지 않는다.
use aaa_common::{quota, util};
use aaa_proto::*;
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone)]
struct Reading {
    snapshot: QuotaSnapshot,
    machine: String,
    received_at_ms: u64,
}
#[derive(Clone, Default)]
struct Entry {
    generation: String,
    attempted_ms: u64,
    next_ms: u64,
    readings: HashMap<String, Reading>,
}
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<String, Entry>,
}

fn merge(entry: &Entry, account: &str, harness: &str, now: u64) -> AccountQuota {
    let mut out = AccountQuota {
        account: account.into(),
        harness: harness.into(),
        windows: vec![],
        problem: None,
        retry_at_ms: None,
    };
    for r in entry.readings.values() {
        let s = &r.snapshot;
        if s.generation != entry.generation {
            continue;
        }
        let skew = s.sampled_at_ms.abs_diff(r.received_at_ms) > quota::CLOCK_SKEW_MS;
        let problem = if skew { Some(QuotaProblem::ClockSkew) } else { s.problem };
        out.retry_at_ms = out.retry_at_ms.max(s.retry_at_ms);
        // 에러 출처의 옛 수치가 성공 출처를 덮지 않는다. 창별 문제는 아래에 따로 남긴다.
        for w in &s.windows {
            if !quota::valid(w) {
                continue;
            }
            let mut freshness = quota::freshness(w, now);
            if problem.is_some() {
                freshness = QuotaFreshness::Unknown;
            }
            let item = AccountQuotaWindow {
                window: w.clone(),
                machine: r.machine.clone(),
                source: s.source,
                received_at_ms: r.received_at_ms,
                freshness,
                problem,
                conflict: false,
            };
            let old = out.windows.iter_mut().find(|x| {
                x.window.limit_id == w.limit_id
                    && x.window.scope == w.scope
                    && x.window.duration_mins == w.duration_mins
                    && (w.duration_mins.is_some() || x.window.window_id == w.window_id)
            });
            match old {
                None => out.windows.push(item),
                Some(old) => {
                    let equal_time = old.window.observed_at_ms == w.observed_at_ms;
                    let conflict = equal_time && old.window.used_percent != w.used_percent;
                    let replace = (old.problem.is_some() && problem.is_none())
                        || (old.problem.is_some() == problem.is_some()
                            && (w.observed_at_ms > old.window.observed_at_ms
                                || (equal_time
                                    && (w.used_percent > old.window.used_percent
                                        || (w.used_percent == old.window.used_percent && r.machine < old.machine)))));
                    if replace {
                        let previous_conflict = old.conflict;
                        *old = item;
                        old.conflict = equal_time && (previous_conflict || conflict);
                    } else if conflict {
                        old.conflict = true;
                    }
                }
            }
        }
        // 창이 하나도 없을 때 표시할 실패. 결과 순서가 HashMap 순서에 좌우되지 않는다.
        if out.problem.is_none() || problem_rank(problem) > problem_rank(out.problem) {
            out.problem = problem;
        }
    }
    out.windows.sort_by(|a, b| {
        (&a.window.limit_id, &a.window.scope, a.window.duration_mins, &a.window.window_id).cmp(&(
            &b.window.limit_id,
            &b.window.scope,
            b.window.duration_mins,
            &b.window.window_id,
        ))
    });
    if out.windows.iter().any(|w| w.problem.is_none() && w.freshness != QuotaFreshness::Unknown) {
        out.problem = None;
    } else if out.problem.is_none() {
        out.problem = Some(QuotaProblem::Unknown);
    }
    out
}
fn problem_rank(p: Option<QuotaProblem>) -> u8 {
    match p {
        None => 0,
        Some(QuotaProblem::Unknown) => 1,
        Some(QuotaProblem::Unsupported) => 2,
        Some(QuotaProblem::Failed) => 3,
        Some(QuotaProblem::Disconnected) => 4,
        Some(QuotaProblem::ClockSkew) => 5,
        Some(QuotaProblem::RateLimited) => 6,
        Some(QuotaProblem::Expired) => 7,
        Some(QuotaProblem::Locked) => 8,
    }
}
fn accept(entry: &mut Entry, key: String, machine: String, mut s: QuotaSnapshot, now: u64) {
    if s.generation != entry.generation || s.windows.len() > 64 {
        return;
    }
    s.windows.retain(quota::valid);
    if s.problem.is_some()
        && s.windows.is_empty()
        && let Some(old) = entry.readings.get(&key)
    {
        s.windows = old.snapshot.windows.clone();
    }
    entry.readings.insert(key, Reading { snapshot: s, machine, received_at_ms: now });
}

impl crate::Host {
    pub(crate) async fn quota_report(self: &Arc<Self>, refresh: bool) -> Result<AccountQuotaReport, ErrorBody> {
        let mut cache = self.account_quota.lock().await;
        let accounts = self.accounts.list()?;
        cache.entries.retain(|k, _| accounts.iter().any(|a| a.name == *k));
        let now = util::now_ms();
        // 호스트에 등록되고 pane과 결속된 참조만 쓴다. 로컬 홈·형제 폴더 탐색은 하지 않는다.
        let panes: Vec<_> = self.inner.lock().unwrap().panes.keys().cloned().collect();
        let bindings: Vec<_> = panes.iter().filter_map(|p| self.pane_account(p).ok().flatten()).collect();
        let jobs = accounts.iter().map(|a| {
            let name = a.name.clone();
            let harness = a.harness.clone();
            let generation = self.accounts.generation(&name).unwrap_or_default();
            let mut entry = cache.entries.get(&name).cloned().unwrap_or_default();
            if entry.generation != generation {
                entry = Entry { generation: generation.clone(), ..Default::default() };
            }
            let mine: Vec<_> = bindings.iter().filter(|b| b.account == name && b.harness == harness).cloned().collect();
            let forced = refresh && now.saturating_sub(entry.attempted_ms) >= crate::usage::min_refresh_ms();
            let due = !generation.is_empty() && (entry.next_ms <= now || forced);
            if due {
                entry.attempted_ms = now;
                entry.next_ms = now.saturating_add(60_000);
            }
            // HTTP 요청이 취소돼도 시도 하한·이미 아는 backoff를 잊지 않는다.
            cache.entries.insert(name.clone(), entry.clone());
            async move {
                if due {
                    if harness == "codex" {
                        let retry = entry.readings.get("host").and_then(|r| r.snapshot.retry_at_ms);
                        if retry.is_none_or(|t| now >= t) {
                            let snapshot = match self.accounts.credential(&name) {
                                Ok(c) if c.auth_generation.as_deref() == Some(&generation) => {
                                    crate::quota_rpc::read(&self.state, generation.clone(), &c.secret.0, now).await
                                }
                                _ => empty(&generation, QuotaSource::CodexRpc, QuotaProblem::Locked, now),
                            };
                            accept(&mut entry, "host".into(), "host".into(), snapshot, util::now_ms());
                        }
                    }
                    let reads =
                        mine.into_iter().filter_map(|b| b.pane.map(|pane| (b.machine, pane))).map(|(machine, pane)| {
                            let generation = generation.clone();
                            async move {
                                let label = self
                                    .inner
                                    .lock()
                                    .unwrap()
                                    .machines
                                    .get(&machine)
                                    .map(|m| m.name.clone())
                                    .unwrap_or_else(|| machine.clone());
                                let snapshot = if !self.machine_feature(&machine, "account_quota.v1") {
                                    empty(
                                        &generation,
                                        QuotaSource::ClaudeStatusline,
                                        QuotaProblem::Unsupported,
                                        util::now_ms(),
                                    )
                                } else if let Ok(conn) = self.live_conn(&machine) {
                                    match conn
                                        .request_timeout(
                                            AgentRequest::AccountQuota {
                                                session: pane.clone(),
                                                generation: generation.clone(),
                                            },
                                            Duration::from_secs(5),
                                        )
                                        .await
                                    {
                                        Ok(AgentResponse::AccountQuota { snapshot: Some(s) }) => s,
                                        Ok(AgentResponse::AccountQuota { snapshot: None }) => empty(
                                            &generation,
                                            QuotaSource::ClaudeStatusline,
                                            QuotaProblem::Unknown,
                                            util::now_ms(),
                                        ),
                                        _ => empty(
                                            &generation,
                                            QuotaSource::ClaudeStatusline,
                                            QuotaProblem::Disconnected,
                                            util::now_ms(),
                                        ),
                                    }
                                } else {
                                    empty(
                                        &generation,
                                        QuotaSource::ClaudeStatusline,
                                        QuotaProblem::Disconnected,
                                        util::now_ms(),
                                    )
                                };
                                (pane, label, snapshot)
                            }
                        });
                    for (pane, machine, snapshot) in futures_util::future::join_all(reads).await {
                        accept(&mut entry, pane, machine, snapshot, util::now_ms());
                    }
                }
                (name, entry)
            }
        });
        let finished = futures_util::future::join_all(jobs).await;
        for (name, entry) in finished {
            // 느린 조회 중 다시 로그인·삭제·재등록했다면 그 결과를 새 신원에 붙이지 않는다.
            match self.accounts.generation(&name).ok() {
                Some(g) if g == entry.generation => {
                    cache.entries.insert(name, entry);
                }
                Some(generation) => {
                    cache.entries.insert(name, Entry { generation, ..Default::default() });
                }
                None => {
                    cache.entries.remove(&name);
                }
            }
        }
        let now = util::now_ms();
        let accounts = self.accounts.list()?;
        Ok(AccountQuotaReport {
            accounts: accounts
                .iter()
                .filter_map(|a| {
                    let e = cache.entries.get(&a.name)?;
                    Some(merge(e, &a.name, &a.harness, now))
                })
                .collect(),
        })
    }
}
fn empty(generation: &str, source: QuotaSource, problem: QuotaProblem, now: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        generation: generation.into(),
        source,
        sampled_at_ms: now,
        windows: vec![],
        problem: Some(problem),
        retry_at_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn good(at: u64, used: f64) -> QuotaSnapshot {
        QuotaSnapshot {
            generation: "login-a".into(),
            source: QuotaSource::ClaudeStatusline,
            sampled_at_ms: at,
            windows: vec![QuotaWindow {
                limit_id: "claude".into(),
                window_id: "five_hour".into(),
                scope: None,
                duration_mins: Some(300),
                used_percent: used,
                observed_at_ms: at,
                resets_at_ms: Some(9_999_999),
            }],
            problem: None,
            retry_at_ms: None,
        }
    }
    #[test]
    fn merges_same_account_conservatively_and_drops_old_login_generation() {
        let mut e = Entry { generation: "login-a".into(), ..Default::default() };
        accept(&mut e, "pane-a".into(), "mac-a".into(), good(1000, 30.), 1000);
        accept(&mut e, "pane-b".into(), "mac-b".into(), good(1000, 40.), 1000);
        let q = merge(&e, "work", "claude", 2000);
        assert_eq!(q.windows.len(), 1);
        assert_eq!(q.windows[0].window.used_percent, 40.);
        assert!(q.windows[0].conflict);
        assert_eq!(q.windows[0].machine, "mac-b");
        let mut later = good(3000, 20.);
        later.windows[0].scope = Some("Sonnet".into());
        accept(&mut e, "pane-c".into(), "mac-c".into(), later, 3000);
        assert_eq!(merge(&e, "work", "claude", 3000).windows.len(), 2);
        let other = Entry { generation: "login-b".into(), ..Default::default() };
        assert!(merge(&other, "private", "claude", 3000).windows.is_empty(), "다른 계정은 합치지 않는다");
        e.generation = "login-b".into();
        accept(&mut e, "pane-new".into(), "mac-a".into(), good(4000, 1.), 4000);
        assert!(merge(&e, "work", "claude", 4000).windows.is_empty(), "늦은 옛 로그인 응답도 버린다");
    }
    #[test]
    fn failure_keeps_original_observation_and_clock_skew_is_unknown() {
        let mut e = Entry { generation: "login-a".into(), ..Default::default() };
        accept(&mut e, "pane-a".into(), "mac-a".into(), good(1000, 80.), 1000);
        let mut failed = empty("login-a", QuotaSource::ClaudeStatusline, QuotaProblem::Disconnected, 2000);
        failed.retry_at_ms = Some(3000);
        accept(&mut e, "pane-a".into(), "mac-a".into(), failed, 2000);
        let q = merge(&e, "work", "claude", 2000);
        assert_eq!(q.windows[0].window.observed_at_ms, 1000);
        assert_eq!(q.windows[0].freshness, QuotaFreshness::Unknown);
        assert_eq!(q.problem, Some(QuotaProblem::Disconnected));
        assert_eq!(q.retry_at_ms, Some(3000));
        accept(&mut e, "pane-a".into(), "mac-a".into(), good(40_000, 5.), 3000);
        let q = merge(&e, "work", "claude", 40_000);
        assert_eq!(q.windows[0].problem, Some(QuotaProblem::ClockSkew));
        assert_eq!(q.windows[0].freshness, QuotaFreshness::Unknown);
        assert_eq!(merge(&e, "work", "claude", 9_999_999).windows[0].window.used_percent, 5.);
    }
}
