//! 구독 사용량 시험. 실제 로그인·keychain·명령·네트워크에 닿지 않는다: 집은 임시 폴더, keychain·gh는 가짜,
//! 구독 주소는 loopback 가짜 서버(origin)로 바꾼다. token·계정은 모두 지어낸 값이다(me@example.com).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// 길: (상태, 덧붙일 머리, 본문)
type Route = (u16, Vec<(String, String)>, String);

/// 길마다 마지막 요청의 머리
type Seen = HashMap<String, Vec<(String, String)>>;
type Pause = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

#[derive(Clone, Default)]
struct Fake {
    routes: Arc<Mutex<HashMap<String, Route>>>,
    /// 길마다 받은 요청 수
    hits: Arc<Mutex<HashMap<String, usize>>>,
    /// 길마다 마지막 요청의 머리(소문자 이름)
    seen: Arc<Mutex<Seen>>,
    bodies: Arc<Mutex<HashMap<String, Value>>>,
    pauses: Arc<Mutex<HashMap<String, Pause>>>,
}

impl Fake {
    async fn start() -> (Fake, String) {
        let fake = Fake::default();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        let f = fake.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let f = f.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, n)| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    while buf.len() < end + length {
                        let n = s.read(&mut chunk).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let mut lines = head.split("\r\n");
                    let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
                    let headers: Vec<(String, String)> = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        .collect();
                    f.bodies
                        .lock()
                        .unwrap()
                        .insert(path.clone(), serde_json::from_slice(&buf[end..end + length]).unwrap_or(Value::Null));
                    *f.hits.lock().unwrap().entry(path.clone()).or_default() += 1;
                    f.seen.lock().unwrap().insert(path.clone(), headers.clone());
                    // 「길|Authorization」 길이 먼저(token마다 다른 답)
                    let auth =
                        headers.iter().find(|(k, _)| k == "authorization").map(|(_, v)| v.clone()).unwrap_or_default();
                    let (status, extra, body) = {
                        let routes = f.routes.lock().unwrap();
                        routes.get(&format!("{path}|{auth}")).or_else(|| routes.get(&path)).cloned().unwrap_or((
                            404,
                            vec![],
                            "{}".into(),
                        ))
                    };
                    let pause = f.pauses.lock().unwrap().get(&path).cloned();
                    if let Some((entered, release)) = pause {
                        entered.notify_one();
                        release.notified().await;
                    }
                    let mut out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n");
                    for (k, v) in extra {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    // chunked로 답한다(Content-Length가 없는 실제 서버처럼)
                    out.push_str(&format!("Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len()));
                    let _ = s.write_all(out.as_bytes()).await;
                });
            }
        });
        (fake, origin)
    }
    fn route(&self, path: &str, status: u16, body: serde_json::Value) {
        self.route_with(path, status, vec![], body);
    }
    fn route_with(&self, path: &str, status: u16, headers: Vec<(&str, &str)>, body: serde_json::Value) {
        let h = headers.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.routes.lock().unwrap().insert(path.into(), (status, h, body.to_string()));
    }
    fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
    fn header(&self, path: &str, name: &str) -> Option<String> {
        self.seen.lock().unwrap().get(path)?.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
}

struct Lab {
    home: PathBuf,
    clock: Arc<AtomicU64>,
    keychain: Arc<Mutex<HashMap<String, KeychainRead>>>,
    commands: Arc<Mutex<HashMap<String, String>>>,
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

const NOW: u64 = 1_792_200_000_000;

impl Lab {
    fn new(name: &str) -> Lab {
        let home = std::env::temp_dir().join(format!("aaa-usage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        Lab { home, clock: Arc::new(AtomicU64::new(NOW)), keychain: Arc::default(), commands: Arc::default() }
    }
    fn write(&self, rel: &str, body: &str) {
        let p = self.home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    fn ctx(&self, origin: &str, darwin: bool) -> UsageContext {
        let (kc, cmds, clock) = (self.keychain.clone(), self.commands.clone(), self.clock.clone());
        UsageContext {
            home: self.home.clone(),
            env: HashMap::from([("USER".to_string(), "me".to_string())]),
            darwin,
            origin: Some(origin.into()),
            keychain: Arc::new(move |svc, acct| {
                let key = format!("{svc}|{}", acct.unwrap_or(""));
                kc.lock().unwrap().get(&key).cloned().unwrap_or(KeychainRead::Missing)
            }),
            run: Arc::new(move |argv| cmds.lock().unwrap().get(&argv.join(" ")).cloned()),
            now: Arc::new(move || clock.load(Ordering::SeqCst)),
        }
    }
    fn advance(&self, ms: u64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }
}

/// 서명 없는 JWT(claims만 본다)
fn jwt(claims: serde_json::Value) -> String {
    let e = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    format!("{}.{}.sig", e(br#"{"alg":"none"}"#), e(claims.to_string().as_bytes()))
}

fn claude_lab(lab: &Lab, expires: u64) {
    lab.write(".claude/.credentials.json", &serde_json::json!({"claudeAiOauth": {"accessToken": &aaa_test_support::token("tok-claude"), "expiresAt": expires, "subscriptionType": "max"}}).to_string());
    lab.write(
        ".claude.json",
        &serde_json::json!({"oauthAccount": {"accountUuid": "acct-1", "emailAddress": "me@example.com"}}).to_string(),
    );
}

fn claude_answer(fake: &Fake) {
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({
            "five_hour": {"utilization": 12.34, "resets_at": "2026-10-17T05:00:00.000000+00:00"},
            "seven_day": {"utilization": 77, "resets_at": "2026-10-20T00:00:00Z"},
            "seven_day_opus": {"utilization": null},
            "seven_day_sonnet": {"utilization": 5, "resets_at": null},
            "limits": [
                {"kind": "weekly_scoped", "percent": 40, "resets_at": "2026-10-20T00:00:00Z", "scope": {"model": {"display_name": "Fable"}}},
                {"kind": "weekly_scoped", "percent": 99, "scope": {"model": {"display_name": "Sonnet"}}}
            ]
        }),
    );
}

#[tokio::test]
async fn claude_reads_the_file_sign_in_and_keeps_numbers_fresh_for_five_minutes() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("claude");
    claude_lab(&lab, NOW + 3_600_000);
    claude_answer(&fake);
    let svc = UsageService::new(lab.ctx(&origin, false));
    let r = svc.report(false).await;
    assert_eq!(r.providers.len(), 1, "{r:?}");
    let u = &r.providers[0];
    assert_eq!(
        (u.id, u.key.as_str(), u.account.as_deref(), u.plan.as_deref(), u.problem),
        (UsageProviderId::Claude, "claude:acct-1", Some("me@example.com"), Some("max"), None)
    );
    assert_eq!(
        fake.header("/api/oauth/usage", "authorization").as_deref(),
        Some(format!("Bearer {}", aaa_test_support::token("tok-claude")).as_str())
    );
    let w: Vec<(UsageWindowKind, Option<&str>, f64)> =
        u.windows.iter().map(|w| (w.kind, w.scope.as_deref(), w.used_percent)).collect();
    // 평평한 Sonnet이 limits의 Sonnet을 이긴다. 값이 없는 Opus는 빠진다
    assert_eq!(
        w,
        vec![
            (UsageWindowKind::Session, None, 12.3),
            (UsageWindowKind::Week, None, 77.0),
            (UsageWindowKind::Week, Some("Sonnet"), 5.0),
            (UsageWindowKind::Week, Some("Fable"), 40.0)
        ]
    );
    // 5시간 창은 다시 시작하는 때에서 5시간 앞에 시작했다
    assert_eq!(u.windows[0].resets_at.as_deref(), Some("2026-10-17T05:00:00.000Z"));
    assert_eq!(u.windows[0].starts_at.as_deref(), Some("2026-10-17T00:00:00.000Z"));
    assert_eq!(u.checked_at.as_deref(), Some(utc_iso(NOW).as_str()));
    // 5분 안에는 다시 묻지 않는다. 다시 읽기도 30초 안에는 묻지 않는다
    lab.advance(10_000);
    svc.report(false).await;
    svc.report(true).await;
    assert_eq!(fake.hits("/api/oauth/usage"), 1);
    lab.advance(MIN_REFRESH_MS);
    svc.report(true).await;
    assert_eq!(fake.hits("/api/oauth/usage"), 2);
    lab.advance(FRESH_MS);
    svc.report(false).await;
    assert_eq!(fake.hits("/api/oauth/usage"), 3);
}

#[tokio::test]
async fn a_429_keeps_the_last_numbers_and_waits_as_told() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("429");
    claude_lab(&lab, NOW + 86_400_000);
    claude_answer(&fake);
    let svc = UsageService::new(lab.ctx(&origin, false));
    assert_eq!(svc.report(false).await.providers[0].problem, None);
    fake.route_with("/api/oauth/usage", 429, vec![("Retry-After", "600")], serde_json::json!({}));
    lab.advance(FRESH_MS);
    let r = svc.report(false).await;
    let u = &r.providers[0];
    assert_eq!(u.problem, Some(UsageProblem::RateLimited));
    assert_eq!(u.windows.len(), 4, "마지막 숫자가 남는다");
    assert_eq!(u.checked_at.as_deref(), Some(utc_iso(NOW).as_str()));
    assert_eq!(fake.hits("/api/oauth/usage"), 2);
    // 말한 때(10분)까지는 다시 읽기로도 묻지 않는다
    for _ in 0..3 {
        lab.advance(MIN_REFRESH_MS + 1);
        assert_eq!(svc.report(true).await.providers[0].problem, Some(UsageProblem::RateLimited));
    }
    assert_eq!(fake.hits("/api/oauth/usage"), 2);
    claude_answer(&fake);
    lab.advance(600_000);
    let r = svc.report(false).await;
    assert_eq!((r.providers[0].problem, fake.hits("/api/oauth/usage")), (None, 3));
}

#[tokio::test]
async fn an_expired_or_refused_sign_in_is_expired_and_never_refreshed() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("expired");
    claude_lab(&lab, NOW - 1);
    claude_answer(&fake);
    let svc = UsageService::new(lab.ctx(&origin, false));
    let r = svc.report(false).await;
    assert_eq!((r.providers[0].problem, r.providers[0].windows.len()), (Some(UsageProblem::Expired), 0));
    assert_eq!(fake.hits("/api/oauth/usage"), 0, "끝난 token으로 묻지 않는다");
    // CLI가 파일을 고친 뒤(1분 안에 다시 본다). 서버가 거절하면 expired
    claude_lab(&lab, NOW + 3_600_000);
    fake.route("/api/oauth/usage", 401, serde_json::json!({}));
    lab.advance(RETRY_MS);
    let r = svc.report(false).await;
    assert_eq!((r.providers[0].problem, fake.hits("/api/oauth/usage")), (Some(UsageProblem::Expired), 1));
    // 다른 실패는 failed
    fake.route("/api/oauth/usage", 500, serde_json::json!({}));
    lab.advance(RETRY_MS);
    assert_eq!(svc.report(false).await.providers[0].problem, Some(UsageProblem::Failed));
    fake.route("/api/oauth/usage", 403, serde_json::json!({}));
    lab.advance(RETRY_MS);
    assert_eq!(svc.report(false).await.providers[0].problem, Some(UsageProblem::Denied), "403은 만료 증거가 아니다");
}

#[tokio::test]
async fn sidebar_uses_registered_claude_statusline_instead_of_unrelated_expired_oauth() {
    use aaa_proto::{QuotaSnapshot, QuotaSource};
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("registered-sidebar");
    claude_lab(&lab, NOW - 1);
    let mut host = crate::Host::open(aaa_config::HostConfig::default(), lab.home.join("host")).unwrap();
    Arc::get_mut(&mut host).unwrap().usage = UsageService::new(lab.ctx(&origin, false));
    host.accounts.import("work", "claude", &aaa_test_support::token("fixture-claude-oauth-token-0123456789")).unwrap();
    let now = aaa_common::util::now_ms();
    let snapshot = QuotaSnapshot {
        input_missing: None,
        generation: host.accounts.generation("work").unwrap(),
        source: QuotaSource::ClaudeStatusline,
        sampled_at_ms: now,
        windows: aaa_common::quota::claude(
            &serde_json::json!({"rate_limits": {
                "five_hour": {"used_percentage": 2, "resets_at": now / 1000 + 3600},
                "seven_day": {"used_percentage": 94, "resets_at": now / 1000 + 86400}
            }}),
            now,
            &[],
        ),
        problem: None,
        retry_at_ms: None,
    };
    host.db
        .set_meta(
            "account-quota-v1/work",
            &serde_json::json!({
                "generation": snapshot.generation, "attempted_ms": now, "next_ms": now + 60000,
                "readings": {"pane": {"snapshot": snapshot, "machine": "mac-a", "received_at_ms": now}}
            })
            .to_string(),
        )
        .unwrap();
    let local = host.usage.report(false).await;
    assert_eq!(local.providers[0].problem, Some(UsageProblem::Expired));
    let report = host.sidebar_usage(false).await;
    assert_eq!(report.providers.len(), 1, "무관한 기본 Claude 로그인은 중복 표시하지 않는다");
    let usage = &report.providers[0];
    assert_eq!(usage.account.as_deref(), Some("work"));
    assert_eq!(usage.key, "claude:registered:work");
    assert_eq!(usage.problem, None);
    assert_eq!(usage.windows.iter().map(|w| 100. - w.used_percent).collect::<Vec<_>>(), vec![98., 6.]);
    assert_eq!(usage.checked_at, Some(utc_iso(now)));
    assert_eq!(fake.hits("/api/oauth/usage"), 0, "기본 인증을 갱신하거나 모델 작업을 만들지 않는다");
    assert_eq!(host.usage.cached().await, local, "자동화의 로컬 캐시 의미는 바꾸지 않는다");
    // 같은 별칭에 다시 로그인하면 새 인증으로 조회하며 이전 세대의 숫자가 붙지 않는다.
    fake.route("/api/oauth/usage", 403, serde_json::json!({}));
    let replacement = aaa_test_support::token("fixture-new-claude-token-0123456789");
    host.accounts.import("work", "claude", &replacement).unwrap();
    let disconnected = host.sidebar_usage(false).await;
    assert_eq!(disconnected.providers[0].query_status, Some(UsageProblem::Unobserved));
    assert!(disconnected.providers[0].windows.is_empty());
    let query_replacement = aaa_test_support::token("replacement-query-access");
    let generation = host.accounts.generation("work").unwrap();
    host.accounts.store_usage(&generation, &query_auth(&query_replacement, now + 3_600_000), None).unwrap();
    let report = host.sidebar_usage(false).await;
    assert_eq!(report.providers[0].problem, Some(UsageProblem::Unsupported));
    assert!(report.providers[0].windows.is_empty());
    assert_eq!(fake.header("/api/oauth/usage", "authorization"), Some(format!("Bearer {query_replacement}")));
}

#[test]
fn registered_sidebar_keeps_observation_age_and_failure_without_borrowing_another_account() {
    use aaa_proto::*;
    let window = AccountQuotaWindow {
        window: QuotaWindow {
            limit_id: "claude".into(),
            window_id: "five_hour".into(),
            scope: None,
            duration_mins: Some(300),
            used_percent: 2.,
            observed_at_ms: NOW,
            resets_at_ms: Some(NOW + 3600000),
        },
        machine: "mac-a".into(),
        source: QuotaSource::ClaudeStatusline,
        received_at_ms: NOW + 600000,
        freshness: QuotaFreshness::Unknown,
        problem: None,
        conflict: false,
    };
    let mut quota = AccountQuota {
        input_missing: None,
        query_status: None,
        account: "work".into(),
        harness: "claude".into(),
        windows: vec![window],
        problem: Some(QuotaProblem::Unknown),
        retry_at_ms: None,
    };
    let usage = registered_claude_usage("work", None, Some(&quota));
    assert_eq!(usage.problem, Some(UsageProblem::Stale));
    assert_eq!(usage.checked_at, Some(utc_iso(NOW)), "수신 시각으로 관측을 새것으로 만들지 않는다");
    assert_eq!(usage.windows[0].used_percent, 2.);
    quota.problem = Some(QuotaProblem::Disconnected);
    assert_eq!(registered_claude_usage("work", None, Some(&quota)).problem, Some(UsageProblem::Disconnected));
    assert!(registered_claude_usage("spare", None, Some(&quota)).windows.is_empty());
    quota.windows[0].conflict = true;
    assert!(registered_claude_usage("work", None, Some(&quota)).windows.is_empty());
    quota.windows[0].conflict = false;
    quota.windows[0].window.scope = Some("Sonnet".into());
    assert!(registered_claude_usage("work", None, Some(&quota)).windows.is_empty());
}

// 필요: coordinator의 주간 관측이 계속 와도 오래된 세션 창이 계정 전체를 낡게 보이던 회귀.
#[test]
fn registered_sidebar_keeps_each_window_age_when_only_weekly_observations_arrive() {
    use aaa_proto::*;
    let now = NOW + 2908 * 60_000;
    let window = |duration, observed, used, freshness| AccountQuotaWindow {
        window: QuotaWindow {
            limit_id: "claude".into(),
            window_id: format!("window-{duration}"),
            scope: None,
            duration_mins: Some(duration),
            used_percent: used,
            observed_at_ms: observed,
            resets_at_ms: Some(observed + 3_600_000),
        },
        machine: "mac-a".into(),
        source: QuotaSource::ClaudeStatusline,
        received_at_ms: now,
        freshness,
        problem: None,
        conflict: false,
    };
    let mut quota = AccountQuota {
        input_missing: None,
        query_status: None,
        account: "work".into(),
        harness: "claude".into(),
        problem: None,
        retry_at_ms: None,
        windows: vec![window(300, NOW, 0., QuotaFreshness::Unknown), window(10080, now, 9., QuotaFreshness::Fresh)],
    };
    for _ in 0..2 {
        let usage = registered_claude_usage("work", None, Some(&quota));
        assert_eq!(usage.checked_at, Some(utc_iso(now)), "계정은 최근 관측 시각");
        assert_eq!(usage.problem, None, "오래된 세션 창이 최신 주간 창을 가리지 않는다");
        assert_eq!(usage.windows[0].observed_at, Some(utc_iso(NOW)));
        assert_eq!(usage.windows[0].problem, Some(UsageProblem::Stale));
        assert_eq!(usage.windows[0].used_percent, 0., "초기화 뒤에도 새 숫자를 합성하지 않는다");
        assert_eq!(usage.windows[1].observed_at, Some(utc_iso(now)));
        assert_eq!(usage.windows[1].problem, None);
        assert_eq!(usage.windows[1].used_percent, 9.);
        quota.windows.reverse();
    }
}

#[tokio::test]
async fn keychain_item_wins_unless_the_file_is_later_and_a_locked_item_says_locked() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("keychain");
    claude_answer(&fake);
    let item =
        |tok: &str, exp: u64| serde_json::json!({"claudeAiOauth": {"accessToken": tok, "expiresAt": exp}}).to_string();
    lab.keychain
        .lock()
        .unwrap()
        .insert("Claude Code-credentials|me".into(), KeychainRead::Found(item("tok-keychain", NOW + 10_000)));
    lab.write(".claude/.credentials.json", &item("tok-file", NOW + 5_000));
    let svc = UsageService::new(lab.ctx(&origin, true));
    svc.report(false).await;
    assert_eq!(fake.header("/api/oauth/usage", "authorization").as_deref(), Some("Bearer tok-keychain"));
    // CLI가 keychain에 못 쓰고 파일만 고쳤다: 늦은 파일이 이긴다
    lab.write(".claude/.credentials.json", &item("tok-file", NOW + 99_000));
    let svc = UsageService::new(lab.ctx(&origin, true));
    svc.report(false).await;
    assert_eq!(fake.header("/api/oauth/usage", "authorization").as_deref(), Some("Bearer tok-file"));
    // 못 읽는 keychain 항목만 있으면 locked(묻지 않는다)
    std::fs::remove_file(lab.home.join(".claude/.credentials.json")).unwrap();
    lab.keychain.lock().unwrap().insert("Claude Code-credentials|me".into(), KeychainRead::Locked);
    let svc = UsageService::new(lab.ctx(&origin, true));
    let r = svc.report(false).await;
    assert_eq!(r.providers[0].problem, Some(UsageProblem::Locked));
    assert!(!r.providers[0].key.contains('/'), "key에 경로가 없다: {}", r.providers[0].key);
}

#[tokio::test]
async fn codex_reads_the_workspace_and_window_lengths_from_its_token() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("codex");
    let token = jwt(serde_json::json!({
        "exp": (NOW / 1000) + 3600,
        "https://api.openai.com/auth": {"chatgpt_plan_type": "pro", "chatgpt_user_id": "user-1"},
        "https://api.openai.com/profile": {"email": "me@example.com"}
    }));
    lab.write(
        ".codex/auth.json",
        &serde_json::json!({"tokens": {"access_token": token, "account_id": "ws-1"}}).to_string(),
    );
    fake.route(
        "/backend-api/wham/usage",
        200,
        serde_json::json!({"plan_type": "pro", "rate_limit": {
            "primary_window": {"used_percent": 30, "limit_window_seconds": 18000, "reset_after_seconds": 3600},
            "secondary_window": {"used_percent": "73", "limit_window_seconds": 604800, "reset_at": 1792500000}
        }}),
    );
    let svc = UsageService::new(lab.ctx(&origin, false));
    let r = svc.report(false).await;
    let u = &r.providers[0];
    assert_eq!(
        (u.key.as_str(), u.account.as_deref(), u.plan.as_deref()),
        ("codex:user-1__ws-1", Some("me@example.com"), Some("pro"))
    );
    assert_eq!(fake.header("/backend-api/wham/usage", "chatgpt-account-id").as_deref(), Some("ws-1"));
    let w: Vec<(UsageWindowKind, f64, Option<&str>)> =
        u.windows.iter().map(|w| (w.kind, w.used_percent, w.resets_at.as_deref())).collect();
    let in_hour = utc_iso(NOW + 3_600_000);
    assert_eq!(
        w,
        vec![
            (UsageWindowKind::Session, 30.0, Some(in_hour.as_str())),
            (UsageWindowKind::Week, 73.0, Some("2026-10-20T12:40:00.000Z"))
        ]
    );
}

#[tokio::test]
async fn sign_ins_that_name_no_account_are_numbered_and_keyed_without_their_path() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("unnamed");
    for (dir, tok) in [(".codex", "a.e30.x"), (".codex-work", "b.e30.x")] {
        lab.write(&format!("{dir}/auth.json"), &serde_json::json!({"tokens": {"access_token": tok}}).to_string());
    }
    fake.route(
        "/backend-api/wham/usage",
        200,
        serde_json::json!({"rate_limit": {"primary_window": {"used_percent": 1}}}),
    );
    let r = UsageService::new(lab.ctx(&origin, false)).report(false).await;
    let mut accounts: Vec<&str> = r.providers.iter().map(|u| u.account.as_deref().unwrap()).collect();
    accounts.sort();
    assert_eq!(accounts, vec!["#1", "#2"]);
    for u in &r.providers {
        assert!(u.key.starts_with("codex@") && !u.key.contains("codex-work") && !u.key.contains('/'), "{}", u.key);
    }
}

// 필요: pane이 없는 등록 계정도 자기 인증으로 조회하고, 실패·재시작 뒤 같은 기록으로 배정해야 한다.
#[tokio::test]
async fn registered_claude_oauth_persists_history_and_backs_off_without_blocking_start() {
    use aaa_proto::*;
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("registered-oauth");
    let mut host = crate::Host::open(aaa_config::HostConfig::default(), lab.home.join("host")).unwrap();
    Arc::get_mut(&mut host).unwrap().usage = UsageService::new(lab.ctx(&origin, false));
    let token = aaa_test_support::token("registered-only-quota");
    host.register_account("work", "claude", &token).unwrap();
    let now = aaa_common::util::now_ms();
    let generation = host.accounts.generation("work").unwrap();
    let execution = host.accounts.credential("work").unwrap();
    let missing = host.quota_report(true).await.unwrap();
    assert_eq!(missing.accounts[0].query_status, Some(UsageProblem::Unobserved));
    assert_eq!(fake.hits("/api/oauth/usage"), 0, "실행 인증으로 조회하지 않는다");
    let query_token = aaa_test_support::token("query-only-access");
    let query_raw = query_auth(&query_token, now + 3_600_000);
    host.accounts.store_usage(&generation, &query_raw, None).unwrap();
    let reset = now / 1000 * 1000 + 86_400_000;
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({
            "five_hour": {"utilization": 20, "resets_at": utc_iso(reset)},
            "seven_day": {"utilization": 35, "resets_at": utc_iso(reset)},
            "ignored": token
        }),
    );
    let report = host.quota_report(false).await.unwrap();
    assert_eq!(report.accounts[0].problem, None);
    assert_eq!(report.accounts[0].windows.len(), 2);
    assert!(report.accounts[0].windows.iter().all(|w| w.source == QuotaSource::ClaudeOauth));
    assert_eq!(fake.header("/api/oauth/usage", "authorization"), Some(format!("Bearer {query_token}")));
    assert_eq!(fake.header("/api/oauth/usage", "anthropic-beta").as_deref(), Some("oauth-2025-04-20"));
    assert!(!serde_json::to_string(&report).unwrap().contains(&token));
    // 같은 DB에서 수집기를 새로 만들고 시계를 기다리지 않고 다음 조회를 시작한다.
    async fn due(host: &crate::Host, age: bool) {
        let mut entry: Value = serde_json::from_str(&host.db.meta("account-quota-v1/work").unwrap().unwrap()).unwrap();
        entry["attempted_ms"] = 0.into();
        entry["next_ms"] = 0.into();
        if age {
            for key in ["readings", "last_known"] {
                let r = &mut entry[key]["host"];
                r["received_at_ms"] = (r["received_at_ms"].as_u64().unwrap() - 600_001).into();
                r["snapshot"]["sampled_at_ms"] = (r["snapshot"]["sampled_at_ms"].as_u64().unwrap() - 600_001).into();
                for w in r["snapshot"]["windows"].as_array_mut().unwrap() {
                    w["observed_at_ms"] = (w["observed_at_ms"].as_u64().unwrap() - 600_001).into();
                }
            }
        }
        host.db.set_meta("account-quota-v1/work", &entry.to_string()).unwrap();
        *host.account_quota.lock().await = Default::default();
    }
    due(&host, true).await;
    fake.route("/api/oauth/usage", 403, serde_json::json!({"error":"user:profile required"}));
    let report = host.quota_report(false).await.unwrap();
    assert_eq!(report.accounts[0].problem, Some(QuotaProblem::Unsupported));
    let saved: Value = serde_json::from_str(&host.db.meta("account-quota-v1/work").unwrap().unwrap()).unwrap();
    assert_eq!(saved["readings"]["host"]["http_status"], 403);
    assert_eq!(fake.hits("/api/oauth/usage"), 2);
    assert_eq!(report.accounts[0].query_status, Some(UsageProblem::Denied));
    due(&host, false).await;
    host.quota_report(true).await.unwrap();
    assert_eq!(fake.hits("/api/oauth/usage"), 2, "같은 거부 인증은 강제 새로고침에도 다시 묻지 않는다");
    assert_eq!(host.accounts.credential("work").unwrap().secret.0, execution.secret.0);
    assert_eq!(host.accounts.generation("work").unwrap(), generation);
    let (_, history) = host.cached_allocation_quota("work", "claude").unwrap();
    assert_eq!(history.windows[1].window.used_percent, 35.);
    assert_eq!(history.windows[1].window.resets_at_ms, Some(reset));
    let observed = history.windows[1].window.observed_at_ms;
    assert!(observed < now);
    due(&host, false).await;
    host.accounts.store_usage(&generation, &query_raw, None).unwrap();
    fake.route_with("/api/oauth/usage", 429, vec![("Retry-After", "1200")], serde_json::json!({}));
    let report = host.quota_report(false).await.unwrap();
    assert_eq!(report.accounts[0].problem, Some(QuotaProblem::Failed), "조회 한도는 모델 사용 한도가 아니다");
    let saved: Value = serde_json::from_str(&host.db.meta("account-quota-v1/work").unwrap().unwrap()).unwrap();
    assert_eq!(saved["readings"]["host"]["http_status"], 429);
    assert!(report.accounts[0].retry_at_ms.unwrap() >= now + 1_200_000);
    due(&host, false).await;
    host.quota_report(false).await.unwrap();
    assert_eq!(fake.hits("/api/oauth/usage"), 3, "host 캐시를 다시 읽어도 Retry-After 전에는 묻지 않는다");
    host.accounts.store_usage(&generation, &query_raw, None).unwrap();
    host.quota_report(true).await.unwrap();
    assert_eq!(fake.hits("/api/oauth/usage"), 3, "조회 재로그인도 Retry-After를 건너뛰지 않는다");
    let encrypted =
        std::fs::read_to_string(lab.home.join("host/usage-accounts").join(format!("{generation}.json"))).unwrap();
    assert!(!encrypted.contains(&query_token));
    assert!(!encrypted.contains(&aaa_test_support::token("query-owned-refresh")));
    // Host 자체도 다시 열어 마지막 정상 관측이 메모리에만 있는 것이 아님을 확인한다.
    drop(host);
    let host = crate::Host::open(aaa_config::HostConfig::default(), lab.home.join("host")).unwrap();
    let (_, history) = host.cached_allocation_quota("work", "claude").unwrap();
    assert_eq!(history.windows[1].window.observed_at_ms, observed);
    let profile = ResolvedProfile {
        harness: "claude".into(),
        model: "any".into(),
        effort: "high".into(),
        args: vec![],
        command: vec![],
        env: Default::default(),
    };
    let input = host.account_selection_input("demo", &profile, None, AccountTaskSize::Medium).await.unwrap().unwrap();
    let (name, allocation) = input.select(&host.db.conn_for_test(), now).unwrap();
    assert_eq!(name, "work");
    assert!(allocation.selection.reason.contains("마지막 관측으로 판단 잔량 65.0%"));
    host.accounts.import("work", "claude", &aaa_test_support::token("replacement-quota")).unwrap();
    assert!(host.accounts.usage_credential(&generation).is_err(), "실행 재등록 때 옛 조회 인증을 정리한다");
    assert!(
        host.cached_allocation_quota("work", "claude").unwrap().1.windows.is_empty(),
        "재로그인 때 옛 기록을 섞지 않는다"
    );
}

fn query_auth(token: &str, expires_ms: u64) -> String {
    serde_json::json!({"access_token": token, "refresh_token": aaa_test_support::token("query-owned-refresh"),
        "expires_ms":expires_ms, "scopes":["user:profile","user:inference"]})
    .to_string()
}

// 필요: 실행용 인증의 조회 거부·조회 제한을 진단하며 모델 한도·인증을 바꾸지 않는다.
#[tokio::test]
async fn registered_oauth_diagnostic_distinguishes_http_status_without_saving_error_bodies() {
    use aaa_proto::QuotaProblem;
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("registered-oauth-diagnostic");
    let service = UsageService::new(lab.ctx(&origin, false));
    let token = aaa_test_support::token("diagnostic-only-auth");
    let private = aaa_test_support::token("discard-private-error-body");
    for (status, problem) in [
        (401, QuotaProblem::Unsupported),
        (403, QuotaProblem::Unsupported),
        (429, QuotaProblem::Failed),
        (500, QuotaProblem::Failed),
    ] {
        fake.route("/api/oauth/usage", status, serde_json::json!({"error":private}));
        let (snapshot, http_status) = service.claude_account_quota("generation-a".into(), &token, NOW).await;
        assert_eq!(http_status, Some(status));
        assert_eq!(snapshot.problem, Some(problem), "진단 때문에 모델 배정 판단을 바꾸지 않는다");
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.retry_at_ms.is_some(), status == 429);
        let text = serde_json::to_string(&snapshot).unwrap();
        assert!(!text.contains(&private) && !text.contains(&token));
    }
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({"five_hour":{"utilization":0,"resets_at":utc_iso(NOW+3600000)}}),
    );
    let (snapshot, http_status) = service.claude_account_quota("generation-a".into(), &token, NOW).await;
    assert_eq!(http_status, None, "성공하면 앞 HTTP 실패 코드를 이어 쓰지 않는다");
    assert_eq!(snapshot.problem, None);
    assert_eq!(snapshot.windows[0].used_percent, 0.);
}

// 필요: 조회 인증만 host가 갱신하고, 회전한 인증을 보관한 뒤 실제 조회에 쓴다.
#[tokio::test]
async fn usage_auth_refresh_rotates_only_query_credentials_and_rejects_late_save() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("query-refresh");
    let service = UsageService::new(lab.ctx(&origin, false));
    let accounts = crate::accounts::Accounts::new(&lab.home.join("host")).unwrap();
    let execution = aaa_test_support::token("execution-refresh-untouched");
    accounts.import("work", "claude", &execution).unwrap();
    let generation = accounts.generation("work").unwrap();
    let old = query_auth(&aaa_test_support::token("query-before-refresh"), NOW - 1);
    accounts.store_usage(&generation, &old, None).unwrap();
    let credential = accounts.usage_credential(&generation).unwrap();
    let access = aaa_test_support::token("query-refreshed-access");
    let refresh = aaa_test_support::token("query-refreshed-refresh");
    fake.route("/v1/oauth/token", 200, serde_json::json!({"access_token":access,"refresh_token":refresh,"expires_in":3600,"scope":"user:profile user:inference"}));
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({"five_hour":{"utilization":24,"resets_at":utc_iso(NOW+3_600_000)}}),
    );
    let (snapshot, status) = service.claude_usage_quota(&accounts, &credential, generation.clone(), NOW).await;
    assert_eq!(status, None);
    assert_eq!(snapshot.windows[0].used_percent, 24.);
    let saved = accounts.usage_credential(&generation).unwrap();
    let auth = crate::usage::claude_auth::Auth::parse(&saved.secret.0).unwrap();
    assert_eq!(auth.access_token, access);
    assert_eq!(auth.refresh_token, refresh);
    assert_eq!(saved.auth_generation, credential.auth_generation, "자동 갱신은 조회 연결 세대를 바꾸지 않는다");
    assert_eq!(accounts.credential("work").unwrap().secret.0, execution);
    assert_eq!(accounts.generation("work").unwrap(), generation);
    assert_eq!(fake.header("/api/oauth/usage", "authorization"), Some(format!("Bearer {access}")));
    let body = fake.bodies.lock().unwrap()["/v1/oauth/token"].clone();
    assert_eq!(body["refresh_token"], aaa_test_support::token("query-owned-refresh"));
    assert_eq!(body["grant_type"], "refresh_token");
    service.claude_usage_quota(&accounts, &saved, generation.clone(), NOW).await;
    assert_eq!(fake.hits("/v1/oauth/token"), 1, "새 인증은 다시 갱신하지 않는다");
    accounts
        .store_usage(&generation, &query_auth(&aaa_test_support::token("new-login-query"), NOW + 3_600_000), None)
        .unwrap();
    let newer = accounts.usage_credential(&generation).unwrap();
    service.claude_usage_quota(&accounts, &credential, generation.clone(), NOW).await;
    assert_eq!(fake.hits("/api/oauth/usage"), 2, "늦은 갱신의 저장 실패 뒤 조회하지 않는다");
    assert_eq!(accounts.usage_credential(&generation).unwrap().secret.0, newer.secret.0);
    for status in [400, 401, 403, 429, 500] {
        fake.route_with(
            "/v1/oauth/token",
            status,
            vec![("Retry-After", "1200")],
            serde_json::json!({"ignored":execution}),
        );
        let (snapshot, actual) = service.claude_usage_quota(&accounts, &credential, generation.clone(), NOW).await;
        assert_eq!(actual, Some(status));
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.retry_at_ms, (status == 429).then_some(NOW + 1_200_000));
        assert_eq!(accounts.usage_credential(&generation).unwrap().secret.0, newer.secret.0);
    }
}

#[test]
fn usage_login_reads_only_its_isolated_store_and_requires_profile_scope() {
    let lab = Lab::new("query-isolation");
    let home = lab.home.join("owned-login");
    std::fs::create_dir_all(&home).unwrap();
    let mut ctx = lab.ctx("http://127.0.0.1:0", true);
    let expected_service =
        format!("Claude Code-credentials-{}", &hex::encode(Sha256::digest(home.to_string_lossy().as_bytes()))[..8]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let expected = expected_service.clone();
    ctx.keychain = Arc::new(move |service, account| {
        assert_eq!(service, expected);
        assert!(account.is_none());
        seen.lock().unwrap().push(service.to_string());
        KeychainRead::Found(serde_json::json!({"claudeAiOauth":{"accessToken":aaa_test_support::token("isolated-access"),"refreshToken":aaa_test_support::token("isolated-refresh"),"expiresAt":NOW+3_600_000,"scopes":["user:profile"]}}).to_string())
    });
    let expected = expected_service.clone();
    ctx.run = Arc::new(move |args| {
        assert_eq!(args, ["security", "delete-generic-password", "-s", expected.as_str()]);
        Some(String::new())
    });
    let service = UsageService::new(ctx);
    let raw = service.claude_login_secret(&home).unwrap();
    assert!(crate::usage::claude_auth::Auth::parse(&raw).is_ok());
    assert!(service.claude_login_cleanup(&home));
    assert_eq!(calls.lock().unwrap().len(), 2);
    let mut bad: Value = serde_json::from_str(&raw).unwrap();
    bad["scopes"] = serde_json::json!(["user:inference"]);
    assert!(crate::usage::claude_auth::Auth::parse(&bad.to_string()).is_err());
}

// 필요: 조회 중 새 로그인을 연결한 뒤 옛 403이 도착해도 정상 이력과 새 조회 권한을 덮지 않는다.
#[tokio::test]
async fn late_query_response_after_relogin_keeps_last_observations() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("late-query");
    let mut host = crate::Host::open(aaa_config::HostConfig::default(), lab.home.join("host")).unwrap();
    Arc::get_mut(&mut host).unwrap().usage = UsageService::new(lab.ctx(&origin, false));
    host.accounts.import("work", "claude", &aaa_test_support::token("late-query-execution")).unwrap();
    let generation = host.accounts.generation("work").unwrap();
    let now = aaa_common::util::now_ms();
    host.accounts
        .store_usage(&generation, &query_auth(&aaa_test_support::token("late-query-old"), now + 3_600_000), None)
        .unwrap();
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({"five_hour":{"utilization":27,"resets_at":utc_iso(now+3_600_000)}}),
    );
    host.quota_report(false).await.unwrap();
    let mut entry: Value = serde_json::from_str(&host.db.meta("account-quota-v1/work").unwrap().unwrap()).unwrap();
    entry["next_ms"] = 0.into();
    entry["attempted_ms"] = 0.into();
    host.db.set_meta("account-quota-v1/work", &entry.to_string()).unwrap();
    *host.account_quota.lock().await = Default::default();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    fake.pauses.lock().unwrap().insert("/api/oauth/usage".into(), (entered.clone(), release.clone()));
    fake.route("/api/oauth/usage", 403, serde_json::json!({}));
    let reading = tokio::spawn({
        let host = host.clone();
        async move { host.quota_report(false).await.unwrap() }
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await.unwrap();
    let query = query_auth(&aaa_test_support::token("late-query-new"), now + 3_600_000);
    host.accounts.store_usage(&generation, &query, None).unwrap();
    release.notify_one();
    let report = reading.await.unwrap();
    assert_eq!(report.accounts[0].windows[0].window.used_percent, 27.);
    assert_eq!(
        report.accounts[0].windows[0].window.observed_at_ms,
        entry["readings"]["host"]["snapshot"]["windows"][0]["observed_at_ms"].as_u64().unwrap()
    );
    assert_ne!(report.accounts[0].query_status, Some(UsageProblem::Denied));
    fake.pauses.lock().unwrap().clear();
    fake.route(
        "/api/oauth/usage",
        200,
        serde_json::json!({"five_hour":{"utilization":28,"resets_at":utc_iso(now+3_600_000)}}),
    );
    let report = host.quota_report(false).await.unwrap();
    assert_eq!(report.accounts[0].windows[0].window.used_percent, 28.);
    assert_eq!(
        fake.header("/api/oauth/usage", "authorization"),
        Some(format!("Bearer {}", aaa_test_support::token("late-query-new")))
    );
}
