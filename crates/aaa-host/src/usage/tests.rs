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

#[derive(Clone, Default)]
struct Fake {
    routes: Arc<Mutex<HashMap<String, Route>>>,
    /// 길마다 받은 요청 수
    hits: Arc<Mutex<HashMap<String, usize>>>,
    /// 길마다 마지막 요청의 머리(소문자 이름)
    seen: Arc<Mutex<Seen>>,
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
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let mut lines = head.split("\r\n");
                    let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
                    let headers: Vec<(String, String)> = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        .collect();
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
    lab.write(".claude/.credentials.json", &serde_json::json!({"claudeAiOauth": {"accessToken": "tok-claude", "expiresAt": expires, "subscriptionType": "max"}}).to_string());
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
    assert_eq!(fake.header("/api/oauth/usage", "authorization").as_deref(), Some("Bearer tok-claude"));
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

#[tokio::test]
async fn cursor_reads_the_app_database() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("cursor");
    let db = lab.home.join(".config/Cursor/User/globalStorage/state.vscdb");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute("CREATE TABLE ItemTable (key TEXT, value BLOB)", []).unwrap();
    let token = jwt(serde_json::json!({"sub": "cur-1", "exp": (NOW / 1000) + 3600}));
    for (k, v) in [
        ("cursorAuth/accessToken", token.as_str()),
        ("cursorAuth/stripeMembershipType", "pro"),
        ("cursorAuth/cachedEmail", "me@example.com"),
    ] {
        c.execute("INSERT INTO ItemTable VALUES (?1, ?2)", rusqlite::params![k, v]).unwrap();
    }
    drop(c);
    fake.route(
        "/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
        200,
        serde_json::json!({"billingCycleEnd": "1792800000000", "planUsage": {"limit": 400, "remaining": 100, "autoPercentUsed": 10}}),
    );
    let r = UsageService::new(lab.ctx(&origin, false)).report(false).await;
    let u = &r.providers[0];
    assert_eq!(
        (u.key.as_str(), u.account.as_deref(), u.plan.as_deref()),
        ("cursor:cur-1", Some("me@example.com"), Some("pro"))
    );
    let w: Vec<(Option<&str>, f64)> = u.windows.iter().map(|w| (w.scope.as_deref(), w.used_percent)).collect();
    assert_eq!(w, vec![(None, 75.0), (Some("Cursor models"), 10.0)]);
    assert_eq!(
        fake.header("/aiserver.v1.DashboardService/GetCurrentPeriodUsage", "connect-protocol-version").as_deref(),
        Some("1")
    );
}

#[tokio::test]
async fn copilot_tries_the_accounts_next_token_and_leaves_out_free_cli_sign_ins() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("copilot");
    lab.write(
        ".config/github-copilot/apps.json",
        &serde_json::json!({"github.com:Iv1": {"oauth_token": "tok-old", "user": "Me"}}).to_string(),
    );
    lab.commands.lock().unwrap().insert(
        "gh auth status --hostname github.com --json hosts".into(),
        serde_json::json!({"hosts": {"github.com": [
            {"login": "me", "state": "success", "tokenSource": "keyring"},
            {"login": "other", "state": "success", "tokenSource": "keyring"}
        ]}})
        .to_string(),
    );
    lab.commands.lock().unwrap().insert("gh auth token --hostname github.com --user me".into(), "tok-new".into());
    lab.commands.lock().unwrap().insert("gh auth token --hostname github.com --user other".into(), "tok-other".into());
    // 편집기 token은 401(몇 해 묵음), 같은 계정의 CLI token이 답한다. 다른 계정(CLI만)은 Copilot Free라 뺀다
    fake.route("/copilot_internal/user|token tok-old", 401, serde_json::json!({}));
    fake.route(
        "/copilot_internal/user|token tok-new",
        200,
        serde_json::json!({"login": "Me", "copilot_plan": "individual", "quota_reset_date": "2026-11-01",
            "quota_snapshots": {"premium_interactions": {"entitlement": 300, "percent_remaining": 25}, "chat": {"unlimited": true, "entitlement": 0, "percent_remaining": 100}}}),
    );
    fake.route(
        "/copilot_internal/user|token tok-other",
        200,
        serde_json::json!({"login": "other", "access_type_sku": "free_limited_copilot", "copilot_plan": "individual"}),
    );
    let r = UsageService::new(lab.ctx(&origin, false)).report(false).await;
    let keys: Vec<&str> = r.providers.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, vec!["copilot:me"]);
    let me = &r.providers[0];
    assert_eq!(
        (me.account.as_deref(), me.plan.as_deref(), me.windows.len(), me.windows[0].used_percent),
        (Some("Me"), Some("individual"), 1, 75.0)
    );
    assert_eq!(me.windows[0].resets_at.as_deref(), Some("2026-11-01T00:00:00.000Z"));
    assert_eq!(fake.hits("/copilot_internal/user"), 3, "tok-old, tok-new, tok-other");
}

#[tokio::test]
async fn grok_antigravity_and_opencode() {
    let (fake, origin) = Fake::start().await;
    let lab = Lab::new("others");
    lab.write(
        ".grok/auth.json",
        &serde_json::json!({"xai": {"key": "tok-grok", "user_id": "g-1", "email": "me@example.com"}}).to_string(),
    );
    fake.route(
        "/v1/billing?format=credits",
        200,
        serde_json::json!({"config": {"currentPeriod": {"type": "USAGE_PERIOD_TYPE_WEEKLY", "start": "2026-10-13T00:00:00Z", "end": "2026-10-20T00:00:00Z"}}}),
    );
    // keychain 값은 go-keyring base64
    let ag = serde_json::json!({"token": {"access_token": "tok-ag", "expiry": "2026-10-18T00:00:00Z"}, "id_token": jwt(serde_json::json!({"sub": "ag-1", "email": "me@example.com"}))});
    let encoded = format!("go-keyring-base64:{}", base64::engine::general_purpose::STANDARD.encode(ag.to_string()));
    lab.keychain.lock().unwrap().insert("gemini|antigravity".into(), KeychainRead::Found(encoded));
    // 앞 주소가 실패하면 뒤 주소(같은 가짜 서버의 같은 길이라 답이 같다)
    fake.route(
        "/v1internal:retrieveUserQuotaSummary",
        200,
        serde_json::json!({"response": {"groups": [{"buckets": [{"bucketId": "gemini-weekly", "remainingFraction": 0.25}, {"bucketId": "3p-5h"}]}]}}),
    );
    lab.write(
        ".local/share/opencode/auth.json",
        &serde_json::json!({"opencode": {"type": "api", "key": "tok-oc"}}).to_string(),
    );
    fake.route("/zen/go/v1/usage", 403, serde_json::json!({}));
    let r = UsageService::new(lab.ctx(&origin, true)).report(false).await;
    let by = |id| r.providers.iter().find(|u| u.id == id);
    let grok = by(UsageProviderId::Grok).unwrap();
    assert_eq!(
        (grok.key.as_str(), grok.windows[0].kind, grok.windows[0].used_percent),
        ("grok:g-1", UsageWindowKind::Week, 0.0)
    );
    assert_eq!(grok.windows[0].starts_at.as_deref(), Some("2026-10-13T00:00:00.000Z"));
    let ag = by(UsageProviderId::Antigravity).unwrap();
    assert_eq!((ag.key.as_str(), ag.windows.len(), ag.windows[0].used_percent), ("antigravity:ag-1", 1, 75.0));
    // Go 구독이 없는 열쇠(403)는 뺀다
    assert!(by(UsageProviderId::Opencode).is_none());
}
