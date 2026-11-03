//! A2 등록 목록 기반 수집. 자동 배정·새 모델 작업은 만들지 않는다.
use aaa_common::{quota, util};
use aaa_proto::*;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
struct Reading {
    snapshot: QuotaSnapshot,
    machine: String,
    received_at_ms: u64,
    /// 사용량 endpoint의 HTTP 실패. 모델 한도 판정과 분리하며 오류 본문은 저장하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Entry {
    generation: String,
    #[serde(default)]
    query_generation: Option<String>,
    attempted_ms: u64,
    next_ms: u64,
    readings: HashMap<String, Reading>,
    /// 마지막 정상 관측. 이후 빈 응답·실패와 별도로 보존하며 재로그인 때 함께 버린다.
    #[serde(default)]
    last_known: HashMap<String, Reading>,
}
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<String, Entry>,
}

fn merge(entry: &Entry, account: &str, harness: &str, now: u64) -> AccountQuota {
    let mut out = AccountQuota {
        session_limit: None,
        input_missing: None,
        query_status: None,
        account: account.into(),
        harness: harness.into(),
        windows: vec![],
        problem: None,
        retry_at_ms: None,
    };
    if harness == "claude" {
        let inputs: Vec<_> = entry
            .readings
            .values()
            .filter(|r| {
                r.snapshot.generation == entry.generation
                    && r.snapshot.sampled_at_ms.abs_diff(r.received_at_ms) <= quota::CLOCK_SKEW_MS
            })
            .filter_map(|r| r.snapshot.input_missing.as_ref().map(|missing| (r, missing)))
            .collect();
        let oauth = inputs
            .iter()
            .filter(|(r, _)| r.snapshot.source == QuotaSource::ClaudeOauth && r.snapshot.problem.is_none())
            .max_by_key(|(r, _)| r.snapshot.sampled_at_ms);
        out.input_missing = (!inputs.is_empty()).then(|| {
            [300, 10080]
                .into_iter()
                .filter(|duration| {
                    if let Some((reading, missing)) = oauth {
                        // sampled_at은 agent의 재조회 때도 갱신된다. 실제 창 관측만 OAuth의 null을 대체한다.
                        missing.contains(duration)
                            && !entry.readings.values().any(|r| {
                                r.snapshot.generation == entry.generation
                                    && r.snapshot.problem.is_none()
                                    && r.snapshot.sampled_at_ms.abs_diff(r.received_at_ms) <= quota::CLOCK_SKEW_MS
                                    && !r
                                        .snapshot
                                        .input_missing
                                        .as_ref()
                                        .is_some_and(|missing| missing.contains(duration))
                                    && r.snapshot.windows.iter().any(|w| {
                                        w.duration_mins == Some(*duration)
                                            && w.scope.is_none()
                                            && w.observed_at_ms >= reading.snapshot.sampled_at_ms
                                    })
                            })
                    } else {
                        inputs.iter().all(|(_, missing)| missing.contains(duration))
                    }
                })
                .collect()
        });
        out.query_status = if entry.query_generation.is_none() {
            Some(UsageProblem::Unobserved)
        } else {
            entry.readings.get("host").and_then(|r| match r.http_status {
                Some(403) => Some(UsageProblem::Denied),
                Some(400 | 401) => Some(UsageProblem::Expired),
                Some(429) => Some(UsageProblem::RateLimited),
                Some(_) => Some(UsageProblem::Failed),
                None => r.snapshot.problem.map(|_| UsageProblem::Failed),
            })
        };
    }
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
fn accept(entry: &mut Entry, key: String, machine: String, s: QuotaSnapshot, now: u64) {
    accept_reading(entry, key, machine, s, None, now);
}

fn accept_reading(
    entry: &mut Entry,
    key: String,
    machine: String,
    mut s: QuotaSnapshot,
    http_status: Option<u16>,
    now: u64,
) {
    if s.generation != entry.generation || s.windows.len() > 64 {
        return;
    }
    s.windows.retain(quota::valid);
    if s.problem.is_none() && s.sampled_at_ms.abs_diff(now) <= quota::CLOCK_SKEW_MS {
        let mut saved = s.clone();
        if let Some(old) = entry.last_known.get(&key).or_else(|| entry.readings.get(&key))
            && old.snapshot.generation == entry.generation
            && old.snapshot.sampled_at_ms.abs_diff(old.received_at_ms) <= quota::CLOCK_SKEW_MS
            && old.snapshot.problem != Some(QuotaProblem::ClockSkew)
        {
            for w in &old.snapshot.windows {
                if let Some(new) = saved.windows.iter_mut().find(|new| same_window(new, w)) {
                    if new.observed_at_ms < w.observed_at_ms {
                        *new = w.clone();
                    }
                } else {
                    saved.windows.push(w.clone());
                }
            }
        }
        entry.last_known.insert(
            key.clone(),
            Reading { snapshot: saved, machine: machine.clone(), received_at_ms: now, http_status },
        );
    }
    if s.problem.is_some()
        && s.windows.is_empty()
        && let Some(old) = entry.readings.get(&key)
    {
        s.windows = old.snapshot.windows.clone();
    }
    // null로 빠진 창의 과거 숫자는 접힌 기록용으로만 보존한다.
    if let Some(missing) = &s.input_missing
        && let Some(old) = entry.last_known.get(&key)
    {
        for window in &old.snapshot.windows {
            if window.duration_mins.is_some_and(|n| missing.contains(&n))
                && !s.windows.iter().any(|w| same_window(w, window))
            {
                s.windows.push(window.clone());
            }
        }
    }
    entry.readings.insert(key, Reading { snapshot: s, machine, received_at_ms: now, http_status });
}

fn same_window(a: &QuotaWindow, b: &QuotaWindow) -> bool {
    a.limit_id == b.limit_id
        && a.scope == b.scope
        && a.duration_mins == b.duration_mins
        && (a.duration_mins.is_some() || a.window_id == b.window_id)
}

fn last_known(entry: &Entry, account: &str, harness: &str, now: u64) -> AccountQuota {
    // 옛 저장 형식의 관측도 쓴다. 실패 때 보존한 수치의 관측 시각은 바꾸지 않는다.
    let mut history =
        Entry { generation: entry.generation.clone(), readings: entry.last_known.clone(), ..Default::default() };
    for (key, reading) in &entry.readings {
        if !history.readings.contains_key(key)
            && reading.snapshot.sampled_at_ms.abs_diff(reading.received_at_ms) <= quota::CLOCK_SKEW_MS
            && reading.snapshot.problem != Some(QuotaProblem::ClockSkew)
        {
            let mut reading = reading.clone();
            reading.snapshot.problem = None;
            history.readings.insert(key.clone(), reading);
        }
    }
    merge(&history, account, harness, now)
}

impl Cache {
    pub(crate) fn rename(&mut self, from: &str, to: &str) {
        self.entries.remove(to);
        if let Some(entry) = self.entries.remove(from) {
            self.entries.insert(to.into(), entry);
        }
    }
}

impl crate::Host {
    /// 마지막 완료된 웹 계정 사용량을 읽는다. 조회 중인 RPC의 잠금·응답을 기다리지 않는다.
    /// 재로그인 전 세대와 삭제된 계정은 판정에 쓰지 않는다.
    pub(crate) fn cached_account_quota(&self, name: &str, harness: &str) -> Result<AccountQuota, ErrorBody> {
        let entry = self.cached_quota_entry(name, harness)?;
        Ok(self.quota_plan(merge(&entry, name, harness, util::now_ms())))
    }

    pub(crate) fn cached_allocation_quota(
        &self,
        name: &str,
        harness: &str,
    ) -> Result<(AccountQuota, AccountQuota), ErrorBody> {
        let entry = self.cached_quota_entry(name, harness)?;
        let now = util::now_ms();
        let live = self.quota_plan(merge(&entry, name, harness, now));
        let mut last = self.quota_plan(last_known(&entry, name, harness, now));
        // 마지막 정상 숫자가 현재 제공 안 되는 창을 되살리지 않는다.
        last.input_missing = live.input_missing.clone();
        Ok((live, last))
    }

    fn quota_plan(&self, mut quota: AccountQuota) -> AccountQuota {
        quota.session_limit = self.account_session_limit(&quota.account, &quota.harness);
        quota
    }

    fn cached_quota_entry(&self, name: &str, harness: &str) -> Result<Entry, ErrorBody> {
        if !self.accounts.list()?.iter().any(|a| a.name == name && a.harness == harness) {
            return Err(crate::err(ErrorCode::BadRequest, "등록 계정과 profile의 harness가 다르거나 계정이 없다"));
        }
        let generation = self.accounts.generation(name)?;
        let entry: Option<Entry> = self
            .db
            .meta(&format!("account-quota-v1/{name}"))
            .map_err(crate::orch::internal)?
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(crate::orch::internal)?;
        let entry = entry.filter(|e| e.generation == generation).unwrap_or_default();
        Ok(entry)
    }

    pub(crate) async fn quota_report(self: &Arc<Self>, refresh: bool) -> Result<AccountQuotaReport, ErrorBody> {
        let mut cache = self.account_quota.lock().await;
        let account_read = self.account_rename_gate.read().await;
        let accounts = self.accounts.list()?;
        for a in &accounts {
            if !cache.entries.contains_key(&a.name)
                && let Some(text) =
                    self.db.meta(&format!("account-quota-v1/{}", a.name)).map_err(crate::orch::internal)?
            {
                let entry = serde_json::from_str(&text).map_err(crate::orch::internal)?;
                cache.entries.insert(a.name.clone(), entry);
            }
        }
        cache.entries.retain(|k, _| accounts.iter().any(|a| a.name == *k));
        let now = util::now_ms();
        // 호스트에 등록되고 pane과 결속된 참조만 쓴다. 로컬 홈·형제 폴더 탐색은 하지 않는다.
        let panes: Vec<_> = self.inner.lock().unwrap().panes.keys().cloned().collect();
        let bindings: Vec<_> = panes
            .iter()
            .filter_map(|p| {
                self.pane_account(p)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
            })
            .collect();
        let jobs: Vec<_> = accounts
            .iter()
            .map(|a| {
                let name = a.name.clone();
                let harness = a.harness.clone();
                let generation = self.accounts.generation(&name).unwrap_or_default();
                let mut entry = cache.entries.get(&name).cloned().unwrap_or_default();
                if entry.generation != generation {
                    entry = Entry { generation: generation.clone(), ..Default::default() };
                }
                let query = if harness == "claude" { self.accounts.usage_credential(&generation).ok() } else { None };
                let query_generation = query.as_ref().and_then(|c| c.auth_generation.clone());
                if harness == "claude" && entry.query_generation != query_generation {
                    entry.query_generation = query_generation;
                    entry.next_ms = 0;
                    entry.attempted_ms = 0;
                    if let Some(reading) = entry.readings.get_mut("host")
                        && reading.http_status != Some(429)
                    {
                        reading.http_status = None;
                        reading.snapshot.problem = None;
                        reading.snapshot.retry_at_ms = None;
                    }
                }
                let mine: Vec<_> =
                    bindings.iter().filter(|b| b.account == name && b.harness == harness).cloned().collect();
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
                                        tracing::Instrument::instrument(
                                            crate::quota_rpc::read(&self.state, generation.clone(), &c.secret.0, now),
                                            tracing::info_span!("계정 사용량 조회", account = %name),
                                        )
                                        .await
                                    }
                                    _ => empty(&generation, QuotaSource::CodexRpc, QuotaProblem::Locked, now),
                                };
                                accept(&mut entry, "host".into(), "host".into(), snapshot, util::now_ms());
                            }
                        }
                        let reads = mine.into_iter().filter_map(|b| b.pane.map(|pane| (b.machine, pane))).map(
                            |(machine, pane)| {
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
                            },
                        );
                        for (pane, machine, snapshot) in futures_util::future::join_all(reads).await {
                            accept(&mut entry, pane, machine, snapshot, util::now_ms());
                        }
                        if harness == "claude" {
                            let live = merge(&entry, &name, &harness, now);
                            let complete = [300, 10080].into_iter().all(|duration| {
                                live.windows.iter().any(|w| {
                                    w.window.scope.is_none()
                                        && w.window.duration_mins == Some(duration)
                                        && w.problem.is_none()
                                        && !w.conflict
                                        && quota::freshness(&w.window, now) == QuotaFreshness::Fresh
                                })
                            });
                            let retry = entry.readings.get("host").and_then(|r| r.snapshot.retry_at_ms);
                            let denied = entry
                                .readings
                                .get("host")
                                .is_some_and(|r| matches!(r.http_status, Some(400 | 401 | 403)));
                            if !complete
                                && !denied
                                && retry.is_none_or(|t| now >= t)
                                && let Some(c) = &query
                            {
                                let (snapshot, http_status) =
                                    self.usage.claude_usage_quota(&self.accounts, c, generation.clone(), now).await;
                                accept_reading(
                                    &mut entry,
                                    "host".into(),
                                    "host".into(),
                                    snapshot,
                                    http_status,
                                    util::now_ms(),
                                );
                            }
                        }
                    }
                    (name, entry)
                }
            })
            .collect();
        // 네트워크 응답을 기다리는 동안 별칭 변경을 허용한다. 옛 이름의 늦은 결과는 아래 세대 검사에서 버린다.
        drop(account_read);
        let finished = futures_util::future::join_all(jobs).await;
        let _account_read = self.account_rename_gate.read().await;
        for (name, entry) in finished {
            // 느린 조회 중 다시 로그인·삭제·재등록했다면 그 결과를 새 신원에 붙이지 않는다.
            match self
                .accounts
                .generation(&name)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            {
                Some(g)
                    if g == entry.generation
                        && (self.accounts.credential(&name).is_ok_and(|c| c.harness != "claude")
                            || self.accounts.usage_credential(&g).ok().and_then(|c| c.auth_generation)
                                == entry.query_generation) =>
                {
                    cache.entries.insert(name, entry);
                }
                // 조회 로그인만 바뀌었으면 늦은 응답을 버리되 직전 숫자·시각은 남긴다.
                Some(g) if g == entry.generation => {}
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
        cache.entries.retain(|name, _| accounts.iter().any(|a| &a.name == name));
        // 조회 중 바뀐 별칭은 원자 이전한 DB 관측에서 다시 읽는다. 옛 별칭으로 재저장하지 않는다.
        for a in &accounts {
            if !cache.entries.contains_key(&a.name)
                && let Some(text) =
                    self.db.meta(&format!("account-quota-v1/{}", a.name)).map_err(crate::orch::internal)?
            {
                cache.entries.insert(a.name.clone(), serde_json::from_str(&text).map_err(crate::orch::internal)?);
            }
        }
        for (name, entry) in &cache.entries {
            self.db
                .set_meta(
                    &format!("account-quota-v1/{name}"),
                    &serde_json::to_string(entry).map_err(crate::orch::internal)?,
                )
                .map_err(crate::orch::internal)?;
        }
        let report = AccountQuotaReport {
            accounts: accounts
                .iter()
                .filter_map(|a| {
                    let e = cache.entries.get(&a.name)?;
                    Some(self.quota_plan(merge(e, &a.name, &a.harness, now)))
                })
                .collect(),
        };
        self.image_reconcile(&report)?;
        Ok(report)
    }
}
fn empty(generation: &str, source: QuotaSource, problem: QuotaProblem, now: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        input_missing: None,
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
            input_missing: None,
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
    fn input_missing_and_query_denied_do_not_change_model_quota() {
        let now = util::now_ms();
        let mut entry =
            Entry { generation: "g".into(), query_generation: Some("query-g".into()), ..Default::default() };
        let mut pane = good(now, 20.);
        pane.generation = "g".into();
        pane.windows[0].resets_at_ms = Some(now + 3_600_000);
        pane.input_missing = Some(vec![300]);
        accept(&mut entry, "pane".into(), "mac-a".into(), pane, now);
        accept_reading(
            &mut entry,
            "host".into(),
            "host".into(),
            empty("g", QuotaSource::ClaudeOauth, QuotaProblem::Unsupported, now),
            Some(403),
            now,
        );
        let report = merge(&entry, "work", "claude", now);
        assert_eq!(report.query_status, Some(UsageProblem::Denied));
        assert_eq!(report.input_missing, Some(vec![300]));
        assert_eq!(report.problem, None, "조회 거부를 모델 한도/실행 인증 오류로 올리지 않는다");
        let mut other = good(now, 20.);
        other.generation = "g".into();
        other.input_missing = Some(vec![]);
        accept(&mut entry, "other".into(), "mac-b".into(), other, now);
        assert_eq!(merge(&entry, "work", "claude", now).input_missing, Some(vec![]));
    }

    // 필요: 정상 null 응답 뒤 과거 수치는 남기되 현재 입력으로 부활시키지 않는다.
    #[test]
    fn newer_null_oauth_window_keeps_history_and_overrides_older_presence() {
        let now = 1_000_000;
        let mut entry = Entry { generation: "g".into(), ..Default::default() };
        let mut old = good(now, 100.);
        old.source = QuotaSource::ClaudeOauth;
        old.generation = "g".into();
        old.input_missing = Some(vec![]);
        accept(&mut entry, "host".into(), "host".into(), old.clone(), now);
        old.source = QuotaSource::ClaudeStatusline;
        old.sampled_at_ms = now + 100;
        accept(&mut entry, "pane".into(), "mac-a".into(), old, now + 100);
        let missing = QuotaSnapshot {
            input_missing: Some(vec![300]),
            generation: "g".into(),
            source: QuotaSource::ClaudeOauth,
            sampled_at_ms: now + 1,
            windows: vec![],
            problem: None,
            retry_at_ms: None,
        };
        accept(&mut entry, "host".into(), "host".into(), missing, now + 1);
        let report = merge(&entry, "work", "claude", now + 1);
        assert_eq!(report.input_missing, Some(vec![300]));
        assert_eq!(report.windows.len(), 1);
        assert_eq!(report.windows[0].window.observed_at_ms, now);
        assert!(!crate::account_pool::below_usage_threshold(&report, [Some(50.), None], now + 1));
        let mut new = good(now + 102, 20.);
        new.generation = "g".into();
        new.input_missing = Some(vec![]);
        accept(&mut entry, "pane".into(), "mac-a".into(), new, now + 102);
        assert_eq!(merge(&entry, "work", "claude", now + 102).input_missing, Some(vec![]));
    }

    #[tokio::test]
    async fn rename_during_quota_collection_keeps_saved_observations_and_drops_old_alias_cache() {
        let root = std::env::temp_dir().join(format!("aaa-quota-rename-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = root.join("projects").display().to_string();
        let mut h = crate::Host::open(cfg, root.join("host")).unwrap();
        Arc::get_mut(&mut h).unwrap().usage = crate::usage::UsageService::new(crate::usage::UsageContext {
            origin: Some("http://127.0.0.1:0".into()),
            ..crate::usage::UsageContext::system()
        });
        h.accounts.import("work", "claude", &aaa_test_support::token("fixture-quota-rename-0123456789")).unwrap();
        let generation = h.accounts.generation("work").unwrap();
        let now = util::now_ms();
        let raw = serde_json::json!({"access_token":aaa_test_support::token("rename-query"), "refresh_token":aaa_test_support::token("rename-refresh"), "expires_ms":now+3_600_000,"scopes":["user:profile"]}).to_string();
        h.accounts.store_usage(&generation, &raw, None).unwrap();
        let query = h.accounts.usage_credential(&generation).unwrap();
        let mut snapshot = good(now, 37.);
        snapshot.generation = generation.clone();
        snapshot.windows[0].resets_at_ms = Some(now + 3_600_000);
        let mut entry = Entry {
            generation: generation.clone(),
            query_generation: query.auth_generation.clone(),
            attempted_ms: now,
            next_ms: now + 60_000,
            ..Default::default()
        };
        accept(&mut entry, "pane-a".into(), "mac-a".into(), snapshot, now);
        h.db.set_meta("account-quota-v1/work", &serde_json::to_string(&entry).unwrap()).unwrap();
        // collector가 cache를 잡고 외부 응답을 기다리는 경계. 이름 변경은 기다림 없이 끝나야 한다.
        let mut collecting = h.account_quota.lock().await;
        h.call(
            DeviceRole::Controller,
            "device:phone",
            FrontRequest::AccountRename { name: "work".into(), to: "renamed".into() },
        )
        .await
        .unwrap();
        // 이름 변경 전 요청의 늦은 cache가 남더라도 다음 보고는 DB의 새 별칭을 읽는다.
        collecting.entries.insert("work".into(), entry);
        drop(collecting);
        let report = h.quota_report(false).await.unwrap();
        assert_eq!(report.accounts.len(), 1);
        assert_eq!(report.accounts[0].account, "renamed");
        assert_eq!(h.accounts.usage_credential(&generation).unwrap().secret.0, query.secret.0);
        assert_eq!(h.accounts.generation("renamed").unwrap(), generation);
        assert_eq!(report.accounts[0].windows[0].window.used_percent, 37.);
        assert!(h.db.meta("account-quota-v1/work").unwrap().is_none());
        assert!(!h.account_quota.lock().await.entries.contains_key("work"));
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
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
    // 필요: 수집이 멎어도 host 응답이 창의 원래 숫자·시각을 리셋 전까지 유지한다.
    #[test]
    fn idle_pane_stays_stale_until_its_reset_without_rewriting_observation_time() {
        let mut e = Entry { generation: "login-a".into(), ..Default::default() };
        accept(&mut e, "pane-a".into(), "mac-a".into(), good(1000, 80.), 1000);
        let q = merge(&e, "work", "claude", 601_001);
        assert_eq!(q.problem, None);
        assert_eq!(q.windows[0].freshness, QuotaFreshness::Stale);
        assert_eq!(q.windows[0].window.observed_at_ms, 1000);
        assert_eq!(q.windows[0].window.used_percent, 80.);
        let reset = q.windows[0].window.resets_at_ms.unwrap();
        assert_eq!(merge(&e, "work", "claude", reset).problem, Some(QuotaProblem::Unknown));
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

    // 필요: 정상 빈 응답·부분 응답도 마지막 주간 잔량과 리셋을 지우면 안 된다.
    #[test]
    fn history_survives_empty_partial_failed_and_skewed_snapshots() {
        let mut entry = Entry { generation: "login-a".into(), ..Default::default() };
        let mut week = good(1000, 35.);
        week.windows[0].duration_mins = Some(10080);
        week.windows[0].window_id = "seven_day".into();
        accept(&mut entry, "pane".into(), "mac-a".into(), week, 1000);
        accept(&mut entry, "pane".into(), "mac-a".into(), good(2000, 80.), 2000);
        let blank = QuotaSnapshot { input_missing: None, windows: vec![], ..good(3000, 0.) };
        accept(&mut entry, "pane".into(), "mac-a".into(), blank, 3000);
        accept(
            &mut entry,
            "pane".into(),
            "mac-a".into(),
            empty("login-a", QuotaSource::ClaudeStatusline, QuotaProblem::Disconnected, 4000),
            4000,
        );
        accept(&mut entry, "pane".into(), "mac-a".into(), good(100_000, 1.), 5000);
        let entry: Entry = serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
        let history = last_known(&entry, "work", "claude", 700_000);
        assert_eq!(history.windows.len(), 2);
        let week = history.windows.iter().find(|w| w.window.duration_mins == Some(10080)).unwrap();
        assert_eq!(week.window.used_percent, 35.);
        assert_eq!(week.window.observed_at_ms, 1000);
        assert_eq!(week.window.resets_at_ms, Some(9_999_999));
        assert_eq!(
            history.windows.iter().find(|w| w.window.duration_mins == Some(300)).unwrap().window.used_percent,
            80.
        );
    }
}
