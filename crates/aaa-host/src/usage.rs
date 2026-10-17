//! 구독 사용량(plan 한도). herdr-web-ui `server/usage.ts`(aa624cb)를 옮겼다.
//!
//! host 머신의 CLI가 로그인한 AI 구독의 한도를 그 CLI 자신의 로그인으로 읽는다: 5시간 세션·주·결제 달을 얼마나
//! 썼고 언제 다시 시작하는지. 로그인이 있는 곳과 한도를 말하는 endpoint는 OpenUsage(github.com/robinebers/openusage,
//! MIT, 2d2eabe)를 따른다(참고와 같다).
//!
//! 일부러 읽기만 한다: OAuth token을 여기서 refresh하지 않는다. Claude·Codex·Cursor·Grok은 refresh token을 돌려쓰므로
//! CLI가 하지 않은 refresh는 그 CLI를 로그아웃시킨다. 끝난 token은 `expired`로 알리고, CLI가 다음에 돌 때 제 파일을 고친다.
//!
//! 뒤에서 혼자 돌지 않는다: client가 물을 때만, FRESH_MS에 한 번까지 묻는다. client가 다시 읽기를 원해도
//! MIN_REFRESH_MS에 한 번이다. 429로 답한 구독은 그가 말한 때까지 다시 묻지 않는다.
//!
//! 참고와 다른 곳(plan §8.45):
//! - keychain을 로그인(Aqua) 세션 밖에서 못 읽을 때 참고는 launchd 일회 job으로 다시 읽는다. aaa는 하지 않는다:
//!   launchd job을 만드는 일은 사용자 승인이 필요한 시스템 동작이고, aaa host는 로그인 세션의 LaunchAgent로 돈다.
//!   그때는 `locked`로 알린다.
//! - 시험 고리: `AAA_USAGE_HOME`이 있으면 그 폴더를 집으로 보고 keychain·명령(gh)·환경 변수를 쓰지 않는다.
//!   `AAA_USAGE_ORIGIN`(`http://127.0.0.1:<port>`)이 있으면 모든 구독 주소의 scheme·host를 그것으로 바꾼다(가짜 서버).
//!   웹 E2E가 개발자의 실제 로그인·keychain을 건드리지 않게 한다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aaa_common::util::{parse_iso, utc_iso};
use aaa_proto::{ProviderUsage, UsageProblem, UsageProviderId, UsageReport, UsageWindow, UsageWindowKind};
use base64::Engine as _;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const FRESH_MS: u64 = 5 * 60_000;
pub const MIN_REFRESH_MS: u64 = 30_000;
/// 실패나 끝난 로그인 뒤: CLI의 다음 refresh를 곧 알아챌 만큼
pub const RETRY_MS: u64 = 60_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const USER_AGENT: &str = "aaa";
/// 로그인 파일·명령 출력·구독 답의 상한. 넘으면 읽을 수 없는 것으로 본다
pub const MAX_READ_BYTES: usize = 1024 * 1024;

pub const PROVIDERS: [UsageProviderId; 7] = [
    UsageProviderId::Claude,
    UsageProviderId::Codex,
    UsageProviderId::Cursor,
    UsageProviderId::Copilot,
    UsageProviderId::Grok,
    UsageProviderId::Antigravity,
    UsageProviderId::Opencode,
];

fn pid(id: UsageProviderId) -> &'static str {
    match id {
        UsageProviderId::Claude => "claude",
        UsageProviderId::Codex => "codex",
        UsageProviderId::Cursor => "cursor",
        UsageProviderId::Copilot => "copilot",
        UsageProviderId::Grok => "grok",
        UsageProviderId::Antigravity => "antigravity",
        UsageProviderId::Opencode => "opencode",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainRead {
    Found(String),
    Missing,
    Locked,
}

type KeychainFn = Arc<dyn Fn(&str, Option<&str>) -> KeychainRead + Send + Sync>;
type RunFn = Arc<dyn Fn(&[&str]) -> Option<String> + Send + Sync>;
type NowFn = Arc<dyn Fn() -> u64 + Send + Sync>;

/// 이 파일 밖에서 닿는 모든 것. 시험은 이것을 바꿔 끼운다.
#[derive(Clone)]
pub struct UsageContext {
    pub home: PathBuf,
    pub env: HashMap<String, String>,
    pub darwin: bool,
    /// 구독 주소의 scheme·host를 바꾼다(시험의 가짜 서버)
    pub origin: Option<String>,
    pub keychain: KeychainFn,
    /// 명령의 다듬은 stdout. 없거나 실패하거나 멈추면 None
    pub run: RunFn,
    pub now: NowFn,
}

impl UsageContext {
    fn env(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// 이 host 머신의 실제 환경. `AAA_USAGE_HOME`이면 시험 고리(모듈 설명).
    pub fn system() -> UsageContext {
        let now: NowFn = Arc::new(aaa_common::util::now_ms);
        let origin = std::env::var("AAA_USAGE_ORIGIN").ok().filter(|v| !v.is_empty());
        if let Some(home) = std::env::var_os("AAA_USAGE_HOME").filter(|v| !v.is_empty()) {
            return UsageContext {
                home: PathBuf::from(home),
                env: HashMap::new(),
                darwin: cfg!(target_os = "macos"),
                origin,
                keychain: Arc::new(|_, _| KeychainRead::Missing),
                run: Arc::new(|_| None),
                now,
            };
        }
        let prompted: Arc<Mutex<HashSet<String>>> = Arc::default();
        UsageContext {
            home: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            env: std::env::vars().collect(),
            darwin: cfg!(target_os = "macos"),
            origin,
            keychain: Arc::new(move |service, account| read_keychain(&prompted, service, account)),
            run: Arc::new(run_command),
            now,
        }
    }
}

/// 명령을 돌려 끝나기를 COMMAND_TIMEOUT까지 기다린다. (종료 코드, stdout, 시간 넘김). 넘기면 자기 자식만 끝낸다.
fn spawn_capped(argv: &[&str]) -> Option<(Option<i32>, Option<String>, bool)> {
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        use std::io::Read as _;
        let mut buf = Vec::new();
        let n = (&mut out).take(MAX_READ_BYTES as u64 + 1).read_to_end(&mut buf).ok()?;
        (n <= MAX_READ_BYTES).then(|| String::from_utf8_lossy(&buf).to_string())
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= COMMAND_TIMEOUT => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let output = reader.join().ok().flatten();
    Some((status.and_then(|s| s.code()), output, timed_out))
}

/// `security find-generic-password`. 응답 없이 멈춘 읽기(아무도 답하지 않은 접근 창)의 service는 다시 묻지 않는다.
fn read_keychain(prompted: &Mutex<HashSet<String>>, service: &str, account: Option<&str>) -> KeychainRead {
    if prompted.lock().unwrap().contains(service) {
        return KeychainRead::Locked;
    }
    let mut argv = vec!["security", "find-generic-password", "-s", service];
    if let Some(a) = account {
        argv.extend(["-a", a]);
    }
    argv.push("-w");
    let Some((code, output, timed_out)) = spawn_capped(&argv) else {
        return KeychainRead::Missing;
    };
    if timed_out {
        prompted.lock().unwrap().insert(service.to_string());
        return KeychainRead::Locked;
    }
    match (code, output) {
        (Some(0), Some(v)) => KeychainRead::Found(v.trim().to_string()),
        (_, None) => KeychainRead::Missing,
        // 44: errSecItemNotFound. 그 밖(36: 잠긴 keychain 등)은 있는 항목을 읽지 못한 것이다
        (Some(44), _) => KeychainRead::Missing,
        _ => KeychainRead::Locked,
    }
}

fn run_command(argv: &[&str]) -> Option<String> {
    match spawn_capped(argv)? {
        (Some(0), Some(out), false) => Some(out.trim().to_string()),
        _ => None,
    }
}

/// 로그인의 주인: `id`가 계정을 가르고 `label`은 주인이 아는 이름(email, login)
#[derive(Debug, Clone, PartialEq, Eq)]
struct Account {
    id: String,
    label: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SignIn {
    token: String,
    /// epoch ms. 로그인이 말하지 않으면 None
    expires_at: Option<u64>,
    plan: Option<String>,
    /// Codex가 묻는 ChatGPT workspace
    chatgpt_account: Option<String>,
    /// 같은 계정의 다른 token. 앞 것이 거절되면 차례로 쓴다
    fallbacks: Vec<String>,
    account: Option<Account>,
    /// Copilot: 편집기에서 Copilot에 로그인한 적 없이 GitHub CLI로만 찾았다
    cli_only: bool,
    /// Copilot: 옛 CLI가 token 주인을 모를 때 견줄 편집기 계정들
    editor_accounts: Vec<String>,
}

/// 찾은 곳의 로그인. `source`는 그 곳(폴더, keychain 항목)의 이름이다. `sign_in`이 None이면 이 세션이 못 읽는 keychain 항목.
#[derive(Debug, Clone)]
struct Found {
    source: String,
    sign_in: Option<SignIn>,
    account: Option<Account>,
}

impl Found {
    fn new(source: impl Into<String>, mut sign_in: SignIn, fallback: Option<Account>) -> Found {
        let account = sign_in.account.take().or(fallback);
        Found { source: source.into(), sign_in: Some(sign_in), account }
    }
}

struct Reading {
    plan: Option<String>,
    windows: Vec<UsageWindow>,
    /// 로그인이 말하지 않았는데 답이 말한 주인
    account: Option<Account>,
}

#[derive(Debug)]
enum ReadError {
    Http { status: u16, retry_after_ms: Option<u64> },
    Other(String),
}

type Json = Map<String, Value>;

fn record(v: Option<&Value>) -> Json {
    match v {
        Some(Value::Object(m)) => m.clone(),
        _ => Json::new(),
    }
}
fn rec(m: &Json, key: &str) -> Json {
    record(m.get(key))
}
fn text(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}
/// 숫자, 또는 proto-JSON int64(글로 온다)
fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}
fn percent(v: f64) -> f64 {
    (v.clamp(0.0, 100.0) * 10.0).round() / 10.0
}
fn parse_json(source: Option<String>) -> Option<Value> {
    serde_json::from_str(&source?).ok()
}
fn json_record(source: Option<String>) -> Json {
    record(parse_json(source).as_ref())
}

/// 파일 글. 없거나 MAX_READ_BYTES를 넘으면 None
fn read_text(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(MAX_READ_BYTES as u64 + 1).read_to_end(&mut buf).ok()?;
    (buf.len() <= MAX_READ_BYTES).then(|| String::from_utf8_lossy(&buf).to_string())
}

/// `home` 안 이름이 `prefix`(".codex-")로 시작하는 폴더들(이름 순)
fn sibling_dirs(home: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(home) else { return vec![] };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// ISO 글이나 epoch 초·ms → ISO 8601
fn iso_time(v: Option<&Value>) -> Option<String> {
    if let Some(n) = number(v)
        && n > 0.0
    {
        let ms = if n < 1e12 { n * 1000.0 } else { n };
        return Some(utc_iso(ms as u64));
    }
    parse_iso(&text(v)?).map(utc_iso)
}
fn epoch_ms(v: Option<&Value>) -> Option<u64> {
    parse_iso(&iso_time(v)?)
}

fn jwt_claims(token: &str) -> Json {
    let Some(payload) = token.split('.').nth(1) else { return Json::new() };
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('='));
    raw.ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| record(Some(&v))).unwrap_or_default()
}
fn jwt_expiry(token: &str) -> Option<u64> {
    number(jwt_claims(token).get("exp")).map(|s| (s * 1000.0) as u64)
}

/// 한도가 세는 기간: 길이(초)로 정한다
fn kind_of(seconds: Option<f64>, fallback: UsageWindowKind) -> UsageWindowKind {
    match seconds {
        Some(s) if s > 0.0 => {
            if s <= 6.0 * 3600.0 {
                UsageWindowKind::Session
            } else if s <= 36.0 * 3600.0 {
                UsageWindowKind::Day
            } else if s <= 8.0 * 86400.0 {
                UsageWindowKind::Week
            } else {
                UsageWindowKind::Month
            }
        }
        _ => fallback,
    }
}

const FIVE_HOURS: f64 = 5.0 * 3600.0;
const SEVEN_DAYS: f64 = 7.0 * 86400.0;

/// `seconds` 길이의 창이 `resets_at`에 다시 시작하면 언제 시작했나
fn start_of(resets_at: &Option<String>, seconds: Option<f64>) -> Option<String> {
    let r = parse_iso(resets_at.as_deref()?)?;
    let s = seconds.filter(|s| *s > 0.0)?;
    r.checked_sub((s * 1000.0) as u64).map(utc_iso)
}

fn retry_after(header: Option<&str>, now: u64) -> Option<u64> {
    let h = header?.trim();
    if let Ok(secs) = h.parse::<f64>() {
        return Some((secs.max(0.0) * 1000.0) as u64);
    }
    let at = httpdate::parse_http_date(h).ok()?;
    let at = at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some(at.saturating_sub(now))
}

/// 주소의 scheme·host를 시험 고리 origin으로 바꾼다
fn target(ctx: &UsageContext, url: &str) -> String {
    match (&ctx.origin, url.strip_prefix("https://")) {
        (Some(origin), Some(rest)) => {
            let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
            format!("{}{path}", origin.trim_end_matches('/'))
        }
        _ => url.to_string(),
    }
}

async fn request_json(
    ctx: &UsageContext,
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Result<Json, ReadError> {
    let host = url.trim_start_matches("https://").split('/').next().unwrap_or("").to_string();
    let answer = tokio::time::timeout(
        REQUEST_TIMEOUT,
        crate::http::request(method, &target(ctx, url), headers, body, MAX_READ_BYTES),
    )
    .await
    .map_err(|_| ReadError::Other(format!("{host}: {}초 안에 답이 없다", REQUEST_TIMEOUT.as_secs())))?
    .map_err(|e| ReadError::Other(format!("{host}: {e}")))?;
    if !(200..300).contains(&answer.status) {
        return Err(ReadError::Http {
            status: answer.status,
            retry_after_ms: retry_after(answer.header("retry-after"), (ctx.now)()),
        });
    }
    let v: Value =
        serde_json::from_slice(&answer.body).map_err(|_| ReadError::Other(format!("{host}: JSON이 아니다")))?;
    Ok(record(Some(&v)))
}

/// keychain 항목에서 로그인을 처음 내는 것. 있는데 못 읽으면 Err(())(locked).
async fn from_keychain(
    ctx: &UsageContext,
    service: &str,
    accounts: &[Option<String>],
    parse: fn(&str) -> Option<SignIn>,
) -> Result<Option<SignIn>, ()> {
    if !ctx.darwin {
        return Ok(None);
    }
    for account in accounts {
        let (kc, svc, acct) = (ctx.keychain.clone(), service.to_string(), account.clone());
        let found =
            tokio::task::spawn_blocking(move || kc(&svc, acct.as_deref())).await.unwrap_or(KeychainRead::Missing);
        match found {
            KeychainRead::Locked => return Err(()),
            KeychainRead::Found(v) => {
                if let Some(s) = parse(&v) {
                    return Ok(Some(s));
                }
            }
            KeychainRead::Missing => {}
        }
    }
    Ok(None)
}

fn keychain_found(result: Result<Option<SignIn>, ()>, source: &str, account: Option<Account>) -> Vec<Found> {
    match result {
        Ok(None) => vec![],
        Err(()) => vec![Found { source: source.into(), sign_in: None, account }],
        Ok(Some(s)) => vec![Found::new(source, s, account)],
    }
}

/// 같은 로그인의 keychain 항목과 로그인 파일. CLI가 keychain에 못 쓰면(데스크톱 세션 밖) 파일만 고친다:
/// 늦은 것이 이긴다. 끝나는 때를 말하지 않는 항목은 더 오래됐다고 알 수 없으니 그대로 둔다.
fn keychain_or_file(
    keychain: Result<Option<SignIn>, ()>,
    file: Option<SignIn>,
    source: &str,
    account: Option<Account>,
) -> Vec<Found> {
    let item = keychain.as_ref().ok().and_then(|o| o.clone());
    let file_is_later = matches!((&file, &item), (Some(f), Some(i)) if matches!((f.expires_at, i.expires_at), (Some(a), Some(b)) if a > b));
    if let Some(i) = item
        && !file_is_later
    {
        return vec![Found::new(source, i, account)];
    }
    match file {
        Some(f) => vec![Found::new(source, f, account)],
        None => keychain_found(keychain, source, account),
    }
}

// ---- Claude Code: macOS keychain `Claude Code-credentials`, 아니면 ~/.claude/.credentials.json ----

/// Claude Code 설정 파일이 말하는 로그인 계정: .claude.json의 `oauthAccount`
fn claude_account(path: &Path) -> Option<Account> {
    let oauth = rec(&json_record(read_text(path)), "oauthAccount");
    let id = text(oauth.get("accountUuid"))?;
    Some(Account { id, label: text(oauth.get("emailAddress")) })
}

struct ClaudeHome {
    source: String,
    service: String,
    dir: PathBuf,
    config: PathBuf,
}

/// Claude Code가 로그인마다 두는 곳: 기본, 그리고 가리킨 설정 폴더마다(CLAUDE_CONFIG_DIR, 둘째 계정에 흔한 ~/.claude-*).
/// 그런 폴더는 keychain 항목 이름을 폴더 hash로 짓고 .claude.json을 그 안에 둔다.
fn claude_homes(ctx: &UsageContext) -> Vec<ClaudeHome> {
    let mut homes = vec![ClaudeHome {
        source: "default".into(),
        service: "Claude Code-credentials".into(),
        dir: ctx.home.join(".claude"),
        config: ctx.home.join(".claude.json"),
    }];
    let mut dirs: Vec<PathBuf> = ctx.env("CLAUDE_CONFIG_DIR").map(PathBuf::from).into_iter().collect();
    dirs.extend(sibling_dirs(&ctx.home, ".claude-"));
    let mut seen = HashSet::new();
    for dir in dirs {
        let s = dir.to_string_lossy().to_string();
        if !seen.insert(s.clone()) {
            continue;
        }
        let hash = hex::encode(Sha256::digest(s.as_bytes()));
        homes.push(ClaudeHome {
            source: s,
            service: format!("Claude Code-credentials-{}", &hash[..8]),
            config: dir.join(".claude.json"),
            dir,
        });
    }
    homes
}

fn claude_sign_in(source: &str) -> Option<SignIn> {
    let oauth = rec(&json_record(Some(source.to_string())), "claudeAiOauth");
    let token = text(oauth.get("accessToken"))?;
    Some(SignIn {
        token,
        expires_at: number(oauth.get("expiresAt")).map(|n| n as u64),
        plan: text(oauth.get("subscriptionType")),
        ..Default::default()
    })
}

/// `seconds`: 창의 길이. Claude는 열쇠 이름에 담는다(`five_hour`, `seven_day`)
fn claude_window(v: Option<&Value>, kind: UsageWindowKind, scope: Option<&str>, seconds: f64) -> Option<UsageWindow> {
    let w = record(v);
    let used = number(w.get("utilization"))?;
    let resets_at = iso_time(w.get("resets_at"));
    Some(UsageWindow {
        kind,
        scope: scope.map(String::from),
        used_percent: percent(used),
        starts_at: start_of(&resets_at, Some(seconds)),
        resets_at,
    })
}

async fn claude_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    let user = ctx.env("USER").map(String::from);
    let accounts: Vec<Option<String>> = match user {
        Some(u) => vec![Some(u), None],
        None => vec![None],
    };
    let mut out = vec![];
    for home in claude_homes(ctx) {
        let account = claude_account(&home.config);
        let keychain = from_keychain(ctx, &home.service, &accounts, claude_sign_in).await;
        let file = read_text(&home.dir.join(".credentials.json")).and_then(|s| claude_sign_in(&s));
        out.extend(keychain_or_file(keychain, file, &home.source, account));
    }
    out
}

async fn claude_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let body = request_json(
        ctx,
        "GET",
        "https://api.anthropic.com/api/oauth/usage",
        &[
            ("Authorization", format!("Bearer {}", s.token)),
            ("Accept", "application/json".into()),
            ("anthropic-beta", "oauth-2025-04-20".into()),
            ("User-Agent", USER_AGENT.into()),
        ],
        b"",
    )
    .await?;
    let mut windows: Vec<UsageWindow> = [
        claude_window(body.get("five_hour"), UsageWindowKind::Session, None, FIVE_HOURS),
        claude_window(body.get("seven_day"), UsageWindowKind::Week, None, SEVEN_DAYS),
        claude_window(body.get("seven_day_opus"), UsageWindowKind::Week, Some("Opus"), SEVEN_DAYS),
        claude_window(body.get("seven_day_sonnet"), UsageWindowKind::Week, Some("Sonnet"), SEVEN_DAYS),
    ]
    .into_iter()
    .flatten()
    .collect();
    // 제 주간 한도가 있는 모델(Fable)은 `limits`에만 있다. 평평한 필드가 이긴다
    for limit in body.get("limits").and_then(Value::as_array).into_iter().flatten() {
        let limit = record(Some(limit));
        let model = text(rec(&rec(&limit, "scope"), "model").get("display_name"));
        let used = number(limit.get("percent"));
        let (Some(model), Some(used)) = (model, used) else { continue };
        if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped")
            || windows.iter().any(|w| w.scope.as_deref() == Some(&model))
        {
            continue;
        }
        windows.push(UsageWindow {
            kind: UsageWindowKind::Week,
            scope: Some(model),
            used_percent: percent(used),
            resets_at: iso_time(limit.get("resets_at")),
            starts_at: None,
        });
    }
    Ok(Some(Reading { plan: None, windows, account: None }))
}

// ---- Codex: CODEX_HOME, ~/.config/codex, ~/.codex의 auth.json. keychain `Codex Auth` ----

fn codex_sign_in(source: &str) -> Option<SignIn> {
    let tokens = rec(&json_record(Some(source.to_string())), "tokens");
    let token = text(tokens.get("access_token"))?;
    let claims = jwt_claims(&token);
    let auth = rec(&claims, "https://api.openai.com/auth");
    let workspace = text(tokens.get("account_id")).or_else(|| text(auth.get("chatgpt_account_id")));
    let email = text(rec(&claims, "https://api.openai.com/profile").get("email"))
        .or_else(|| text(jwt_claims(&text(tokens.get("id_token")).unwrap_or_default()).get("email")));
    // 한도는 workspace 안 사람마다다: 같은 email도 팀 workspace면 다른 plan이다
    let user = text(auth.get("chatgpt_user_id")).or_else(|| text(auth.get("user_id")));
    let id = text(auth.get("chatgpt_account_user_id")).or_else(|| {
        if user.is_some() || workspace.is_some() {
            Some([user.clone(), workspace.clone()].into_iter().flatten().collect::<Vec<_>>().join("__"))
        } else {
            email.clone()
        }
    });
    Some(SignIn {
        expires_at: jwt_expiry(&token),
        plan: text(auth.get("chatgpt_plan_type")),
        chatgpt_account: workspace,
        account: id.map(|id| Account { id, label: email }),
        token,
        ..Default::default()
    })
}

fn codex_window(v: Option<&Value>, now: u64) -> Option<UsageWindow> {
    let w = record(v);
    let used = number(w.get("used_percent"))?;
    let after = number(w.get("reset_after_seconds"));
    let seconds = number(w.get("limit_window_seconds"));
    let resets_at = iso_time(w.get("reset_at")).or_else(|| after.map(|a| utc_iso(now + (a * 1000.0) as u64)));
    Some(UsageWindow {
        kind: kind_of(seconds, UsageWindowKind::Session),
        scope: None,
        used_percent: percent(used),
        starts_at: start_of(&resets_at, seconds),
        resets_at,
    })
}

async fn codex_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    // CODEX_HOME은 Codex를 다른 곳으로 보낸다. ~/.codex-*는 둘째 계정이 흔히 사는 곳이다
    let mut homes: Vec<PathBuf> = ctx.env("CODEX_HOME").map(PathBuf::from).into_iter().collect();
    homes.extend([ctx.home.join(".config").join("codex"), ctx.home.join(".codex")]);
    homes.extend(sibling_dirs(&ctx.home, ".codex-"));
    let mut seen = HashSet::new();
    let mut found = vec![];
    for home in homes {
        if !seen.insert(home.clone()) {
            continue;
        }
        if let Some(s) = read_text(&home.join("auth.json")).and_then(|t| codex_sign_in(&t)) {
            found.push(Found::new(home.to_string_lossy(), s, None));
        }
    }
    if !found.is_empty() {
        return found;
    }
    keychain_found(from_keychain(ctx, "Codex Auth", &[None], codex_sign_in).await, "keychain", None)
}

async fn codex_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let mut headers = vec![
        ("Authorization", format!("Bearer {}", s.token)),
        ("Accept", "application/json".into()),
        ("User-Agent", USER_AGENT.into()),
    ];
    if let Some(a) = &s.chatgpt_account {
        headers.push(("chatgpt-account-id", a.clone()));
    }
    let body = request_json(ctx, "GET", "https://chatgpt.com/backend-api/wham/usage", &headers, b"").await?;
    let limits = rec(&body, "rate_limit");
    let now = (ctx.now)();
    let windows = [codex_window(limits.get("primary_window"), now), codex_window(limits.get("secondary_window"), now)]
        .into_iter()
        .flatten()
        .collect();
    Ok(Some(Reading { plan: text(body.get("plan_type")), windows, account: None }))
}

// ---- Cursor: 앱의 state.vscdb, 또는 cursor-agent CLI의 keychain 항목 ----

fn cursor_database(ctx: &UsageContext) -> PathBuf {
    if ctx.darwin {
        ctx.home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    } else {
        ctx.env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.home.join(".config"))
            .join("Cursor/User/globalStorage/state.vscdb")
    }
}

/// token의 subject로 가르는 Cursor 계정. 이름은 Cursor가 곁에 둔 email
fn cursor_account(token: &str, email: Option<String>) -> Option<Account> {
    Some(Account { id: text(jwt_claims(token).get("sub"))?, label: email })
}

fn cursor_app_sign_in(path: &Path) -> Option<SignIn> {
    if !path.is_file() {
        return None;
    }
    let db = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let value = |key: &str| -> Option<String> {
        let v: rusqlite::types::Value = db
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1 AND length(value) <= ?2",
                rusqlite::params![key, MAX_READ_BYTES as i64],
                |r| r.get(0),
            )
            .ok()?;
        match v {
            rusqlite::types::Value::Text(t) => text(Some(&Value::String(t))),
            rusqlite::types::Value::Blob(b) => text(Some(&Value::String(String::from_utf8_lossy(&b).to_string()))),
            _ => None,
        }
    };
    let token = value("cursorAuth/accessToken")?;
    Some(SignIn {
        expires_at: jwt_expiry(&token),
        plan: value("cursorAuth/stripeMembershipType"),
        account: cursor_account(&token, value("cursorAuth/cachedEmail")),
        token,
        ..Default::default()
    })
}

async fn cursor_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    let db = cursor_database(ctx);
    let app = tokio::task::spawn_blocking(move || cursor_app_sign_in(&db)).await.ok().flatten();
    let cli_email =
        text(rec(&json_record(read_text(&ctx.home.join(".cursor/cli-config.json"))), "authInfo").get("email"));
    fn cli_parse(v: &str) -> Option<SignIn> {
        let token = text(Some(&Value::String(v.to_string())))?;
        Some(SignIn { expires_at: jwt_expiry(&token), token, ..Default::default() })
    }
    let mut cli = from_keychain(ctx, "cursor-access-token", &[None], cli_parse).await;
    if let Ok(Some(c)) = &mut cli {
        c.account = cursor_account(&c.token, cli_email);
    }
    let Some(app) = app else { return keychain_found(cli, "cli", None) };
    let cli = match cli {
        Ok(Some(c)) => c,
        _ => return vec![Found::new("app", app, None)],
    };
    // 두 계정은 두 plan이다. 한 계정이 둘 다에 로그인했으면 더 오래 사는 token으로 쓴다
    if app.account.as_ref().map(|a| &a.id) != cli.account.as_ref().map(|a| &a.id) {
        return vec![Found::new("app", app, None), Found::new("cli", cli, None)];
    }
    if cli.expires_at.unwrap_or(0) > app.expires_at.unwrap_or(0) {
        vec![Found::new("cli", SignIn { plan: app.plan.clone(), account: app.account.clone(), ..cli }, None)]
    } else {
        vec![Found::new("app", app, None)]
    }
}

async fn cursor_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let body = request_json(
        ctx,
        "POST",
        "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
        &[
            ("Authorization", format!("Bearer {}", s.token)),
            ("Content-Type", "application/json".into()),
            ("connect-protocol-version", "1".into()),
            ("User-Agent", USER_AGENT.into()),
        ],
        b"{}",
    )
    .await?;
    let plan = rec(&body, "planUsage");
    let limit = number(plan.get("limit"));
    let remaining = number(plan.get("remaining"));
    let used = number(plan.get("totalPercentUsed")).or(match (limit, remaining) {
        (Some(l), Some(r)) if l != 0.0 => Some((l - r) / l * 100.0),
        _ => None,
    });
    let resets_at = iso_time(body.get("billingCycleEnd"));
    // plan 전체 몫, 그리고 그 안의 두 몫: Cursor 자체 모델(Auto)과 나머지(API)
    let windows = [
        (used, None),
        (number(plan.get("autoPercentUsed")), Some("Cursor models")),
        (number(plan.get("apiPercentUsed")), Some("Other models")),
    ]
    .into_iter()
    .filter_map(|(share, scope)| {
        Some(UsageWindow {
            kind: UsageWindowKind::Month,
            scope: scope.map(String::from),
            used_percent: percent(share?),
            resets_at: resets_at.clone(),
            starts_at: None,
        })
    })
    .collect();
    Ok(Some(Reading { plan: None, windows, account: None }))
}

// ---- GitHub Copilot: 편집기 plugin의 로그인, 아니면 GitHub CLI의 로그인 ----

struct GitHubToken {
    token: String,
    login: Option<String>,
    /// GitHub CLI가 아니라 편집기의 Copilot 로그인에서
    editor: bool,
}

fn copilot_file_tokens(source: Option<String>) -> Vec<GitHubToken> {
    json_record(source)
        .iter()
        .filter_map(|(host, entry)| {
            let e = record(Some(entry));
            let token = if host.starts_with("github.com") { text(e.get("oauth_token")) } else { None }?;
            Some(GitHubToken { token, login: text(e.get("user")), editor: true })
        })
        .collect()
}

async fn run(ctx: &UsageContext, argv: Vec<String>) -> Option<String> {
    let r = ctx.run.clone();
    tokio::task::spawn_blocking(move || r(&argv.iter().map(String::as_str).collect::<Vec<_>>())).await.ok().flatten()
}

/// GitHub CLI가 쓸 수 있는 로그인을 가진 github.com 계정들. 옛 CLI면 지금 계정 하나
async fn gh_tokens(ctx: &UsageContext) -> Vec<GitHubToken> {
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // github.com만: status는 늘어놓은 host마다 그 host의 로그인으로 확인한다
    let status =
        parse_json(run(ctx, argv(&["gh", "auth", "status", "--hostname", "github.com", "--json", "hosts"])).await);
    let accounts =
        record(status.as_ref()).get("hosts").map(|h| record(Some(h))).and_then(|h| h.get("github.com").cloned());
    let Some(Value::Array(accounts)) = accounts else {
        let token = text(
            run(ctx, argv(&["gh", "auth", "token", "--hostname", "github.com"])).await.map(Value::String).as_ref(),
        );
        return token.map(|token| GitHubToken { token, login: None, editor: false }).into_iter().collect();
    };
    let mut out = vec![];
    for entry in accounts {
        let a = record(Some(&entry));
        // gh가 이미 오류로 본 로그인(취소된 token)은 뺀다
        let Some(login) = text(a.get("login")) else { continue };
        if a.get("state").and_then(Value::as_str) != Some("success") {
            continue;
        }
        // GH_TOKEN·GITHUB_TOKEN 로그인에는 저장된 token이 없다: 사용자별로 찾으면 아무것도 없다
        let from_env = matches!(text(a.get("tokenSource")).as_deref(), Some("GH_TOKEN" | "GITHUB_TOKEN"));
        let mut cmd = argv(&["gh", "auth", "token", "--hostname", "github.com"]);
        if !from_env {
            cmd.extend(["--user".to_string(), login.clone()]);
        }
        if let Some(token) = text(run(ctx, cmd).await.map(Value::String).as_ref()) {
            out.push(GitHubToken { token, login: Some(login), editor: false });
        }
    }
    out
}

fn copilot_window(v: Option<&Value>, scope: &str, resets_at: &Option<String>) -> Option<UsageWindow> {
    let q = record(v);
    let entitlement = number(q.get("entitlement"))?;
    let left = number(q.get("percent_remaining"))?;
    if q.get("unlimited") == Some(&Value::Bool(true)) || entitlement <= 0.0 {
        return None;
    }
    Some(UsageWindow {
        kind: UsageWindowKind::Month,
        scope: Some(scope.into()),
        used_percent: percent(100.0 - left),
        resets_at: resets_at.clone(),
        starts_at: None,
    })
}

async fn copilot_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    let dir = ctx
        .env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.home.join(".config"))
        .join("github-copilot");
    let mut tokens = copilot_file_tokens(read_text(&dir.join("apps.json")));
    tokens.extend(copilot_file_tokens(read_text(&dir.join("hosts.json"))));
    tokens.extend(gh_tokens(ctx).await);
    let editor_accounts: Vec<String> =
        tokens.iter().filter(|t| t.editor).filter_map(|t| t.login.as_ref().map(|l| l.to_lowercase())).collect();
    // GitHub 계정마다 로그인 하나. 편집기 로그인은 token보다 몇 해 오래 남는다: 같은 계정의 다음 token(GitHub CLI)을 뒤에
    // 시도한다. 주인을 모르는 token은 따로 하나다: 누구 것인지 모르고, 먼저 답한 것이 다른 것을 가린다.
    let mut order: Vec<String> = vec![];
    let mut accounts: HashMap<String, (Option<String>, Vec<String>, bool)> = HashMap::new();
    let mut seen = HashSet::new();
    let mut unknown = 0;
    for t in tokens {
        if !seen.insert(t.token.clone()) {
            continue;
        }
        let id = match &t.login {
            Some(l) => l.to_lowercase(),
            None => {
                unknown += 1;
                format!("#{unknown}")
            }
        };
        let a = accounts.entry(id.clone()).or_insert_with(|| {
            order.push(id.clone());
            (t.login.clone(), vec![], false)
        });
        a.1.push(t.token);
        a.2 |= t.editor;
    }
    order
        .into_iter()
        .map(|id| {
            let (login, toks, editor) = accounts.remove(&id).unwrap();
            let mut toks = toks.into_iter();
            let token = toks.next().unwrap_or_default();
            let source = if login.is_some() { format!("github:{id}") } else { format!("github{id}") };
            let account = login.map(|l| Account { id: id.clone(), label: Some(l) });
            Found::new(
                source,
                SignIn {
                    token,
                    fallbacks: toks.collect(),
                    account,
                    cli_only: !editor,
                    editor_accounts: editor_accounts.clone(),
                    ..Default::default()
                },
                None,
            )
        })
        .collect()
}

async fn copilot_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    // token들이 다른 GitHub 계정의 것일 수 있다: Copilot이 있는 첫 계정이 답한다. 404는 Copilot이 없는 계정이다.
    // 모든 token이 그렇다고 할 때만 Copilot을 뺀다.
    let mut body = None;
    let mut refusal = None;
    for token in std::iter::once(&s.token).chain(s.fallbacks.iter()) {
        let r = request_json(
            ctx,
            "GET",
            "https://api.github.com/copilot_internal/user",
            &[
                ("Authorization", format!("token {token}")),
                ("Accept", "application/json".into()),
                ("User-Agent", "GitHubCopilotChat/0.26.7".into()),
                ("editor-version", "vscode/1.96.2".into()),
                ("editor-plugin-version", "copilot-chat/0.26.7".into()),
                ("x-github-api-version", "2025-04-01".into()),
            ],
            b"",
        )
        .await;
        match r {
            Ok(b) => {
                body = Some(b);
                break;
            }
            Err(ReadError::Http { status, retry_after_ms }) if [401, 403, 404].contains(&status) => {
                if status != 404 {
                    refusal = Some(ReadError::Http { status, retry_after_ms });
                }
            }
            Err(e) => return Err(e),
        }
    }
    let Some(body) = body else {
        return match refusal {
            Some(e) => Err(e),
            None => Ok(None),
        };
    };
    let resets_at = iso_time(body.get("quota_reset_date"));
    let snapshots = rec(&body, "quota_snapshots");
    let mut windows: Vec<UsageWindow> = [
        copilot_window(snapshots.get("premium_interactions"), "Premium", &resets_at),
        copilot_window(snapshots.get("chat"), "Chat", &resets_at),
        copilot_window(snapshots.get("completions"), "Completions", &resets_at),
    ]
    .into_iter()
    .flatten()
    .collect();
    // Copilot Free는 달마다 받은 몫 가운데 남은 것을 말한다
    let left = rec(&body, "limited_user_quotas");
    let monthly = rec(&body, "monthly_quotas");
    for (key, scope) in [("chat", "Chat"), ("completions", "Completions")] {
        let total = number(monthly.get(key));
        let remaining = number(left.get(key));
        if let (Some(total), Some(remaining)) = (total, remaining)
            && total > 0.0
            && !windows.iter().any(|w| w.scope.as_deref() == Some(scope))
        {
            windows.push(UsageWindow {
                kind: UsageWindowKind::Month,
                scope: Some(scope.into()),
                used_percent: percent((total - remaining) / total * 100.0),
                resets_at: iso_time(body.get("limited_user_reset_date")).or(resets_at.clone()),
                starts_at: None,
            });
        }
    }
    // Copilot Free는 plan을 "individual"로 말한다. SKU만 둘을 가른다
    let plan = if text(body.get("access_type_sku")).is_some_and(|s| s.contains("free")) {
        Some("free".to_string())
    } else {
        text(body.get("copilot_plan"))
    };
    let login = text(body.get("login"));
    // GitHub는 모든 계정에 Copilot Free를 준다: GitHub CLI 로그인만으로는 Copilot을 쓴다는 표시가 아니다.
    // 주인을 모르는 CLI token도 알려진 편집기 계정의 답일 수 있다.
    let editor_account = login.as_ref().is_some_and(|l| s.editor_accounts.contains(&l.to_lowercase()));
    if plan.as_deref() == Some("free") && s.cli_only && !editor_account {
        return Ok(None);
    }
    Ok(Some(Reading { plan, windows, account: login.map(|l| Account { id: l.to_lowercase(), label: Some(l) }) }))
}

// ---- Grok CLI: ~/.grok/auth.json ----

async fn grok_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    // 로그인한 계정마다 한 항목
    json_record(read_text(&ctx.home.join(".grok/auth.json")))
        .iter()
        .filter_map(|(source, entry)| {
            let v = record(Some(entry));
            let token = text(v.get("key"))?;
            let id = text(v.get("user_id")).or_else(|| text(v.get("principal_id"))).unwrap_or_else(|| source.clone());
            let expires = epoch_ms(v.get("expires_at").or(v.get("expires")));
            Some(Found::new(
                source.clone(),
                SignIn {
                    token,
                    expires_at: expires,
                    account: Some(Account { id, label: text(v.get("email")) }),
                    ..Default::default()
                },
                None,
            ))
        })
        .collect()
}

async fn grok_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let body = request_json(
        ctx,
        "GET",
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        &[
            ("Authorization", format!("Bearer {}", s.token)),
            ("x-xai-token-auth", "xai-grok-cli".into()),
            ("Accept", "application/json".into()),
            ("User-Agent", USER_AGENT.into()),
        ],
        b"",
    )
    .await?;
    let config = rec(&body, "config");
    let period = rec(&config, "currentPeriod");
    let stated = number(config.get("creditUsagePercent"));
    let known = match text(period.get("type")).as_deref() {
        Some("USAGE_PERIOD_TYPE_DAILY") => Some(UsageWindowKind::Day),
        Some("USAGE_PERIOD_TYPE_WEEKLY") => Some(UsageWindowKind::Week),
        Some("USAGE_PERIOD_TYPE_MONTHLY") => Some(UsageWindowKind::Month),
        _ => None,
    };
    // 둘 다 말하지 않는 답은 plan 사용량이 아니다
    if stated.is_none() && known.is_none() {
        return Ok(Some(Reading { plan: None, windows: vec![], account: None }));
    }
    let start = epoch_ms(period.get("start"));
    let end = epoch_ms(period.get("end"));
    let span = match (start, end) {
        (Some(a), Some(b)) => Some((b as f64 - a as f64) / 1000.0),
        _ => None,
    };
    let kind = known.unwrap_or_else(|| kind_of(span, UsageWindowKind::Month));
    // proto-JSON은 0을 뺀다: 기간은 말했는데 몫이 없으면 쓴 것이 없다
    Ok(Some(Reading {
        plan: None,
        windows: vec![UsageWindow {
            kind,
            scope: None,
            used_percent: percent(stated.unwrap_or(0.0)),
            resets_at: iso_time(period.get("end")),
            starts_at: iso_time(period.get("start")),
        }],
        account: None,
    }))
}

// ---- Antigravity(Google Gemini·다른 회사 모델 몫): keychain `gemini`/`antigravity`, keychain이 없으면(Linux)
// CLI의 `antigravity-oauth-token` 파일 ----

fn antigravity_sign_in(value: &str) -> Option<SignIn> {
    let decoded = match value.strip_prefix("go-keyring-base64:") {
        Some(b) => String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b.trim()).ok()?).ok()?,
        None => value.to_string(),
    };
    let root = json_record(Some(decoded));
    let token = rec(&root, "token");
    let access = text(token.get("access_token"))?;
    let claims = text(root.get("id_token")).map(|t| jwt_claims(&t)).unwrap_or_default();
    let email = text(claims.get("email"));
    let sub = text(claims.get("sub"));
    let account = sub.or(email.clone()).map(|id| Account { id, label: email });
    Some(SignIn { token: access, expires_at: epoch_ms(token.get("expiry")), account, ..Default::default() })
}

async fn antigravity_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    let dir = ctx
        .env("ANTIGRAVITY_APP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.home.join(".gemini/antigravity-cli"));
    let file = read_text(&dir.join("antigravity-oauth-token")).and_then(|t| antigravity_sign_in(&t));
    let keychain = from_keychain(ctx, "gemini", &[Some("antigravity".into())], antigravity_sign_in).await;
    keychain_or_file(keychain, file, "keychain", None)
}

async fn antigravity_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let headers = [
        ("Authorization", format!("Bearer {}", s.token)),
        ("Accept", "application/json".into()),
        ("Content-Type", "application/json".into()),
        ("User-Agent", "antigravity".into()),
    ];
    let body = match request_json(
        ctx,
        "POST",
        "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
        &headers,
        b"{}",
    )
    .await
    {
        Ok(b) => b,
        Err(ReadError::Http { status, retry_after_ms }) if [401, 403, 429].contains(&status) => {
            return Err(ReadError::Http { status, retry_after_ms });
        }
        Err(_) => {
            request_json(
                ctx,
                "POST",
                "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
                &headers,
                b"{}",
            )
            .await?
        }
    };
    let groups = body.get("groups").cloned().or_else(|| rec(&body, "response").get("groups").cloned());
    let mut windows = vec![];
    for group in groups.as_ref().and_then(Value::as_array).into_iter().flatten() {
        for bucket in record(Some(group)).get("buckets").and_then(Value::as_array).into_iter().flatten() {
            let v = record(Some(bucket));
            let shape = match text(v.get("bucketId")).as_deref() {
                Some("gemini-5h") => Some((UsageWindowKind::Session, None)),
                Some("gemini-weekly") => Some((UsageWindowKind::Week, None)),
                Some("3p-5h") => Some((UsageWindowKind::Session, Some("Other models"))),
                Some("3p-weekly") => Some((UsageWindowKind::Week, Some("Other models"))),
                _ => None,
            };
            // 몫이 없으면 0%·100%로 보이지 않고 뺀다(OpenUsage와 같다). 시작은 모른다
            if let (Some((kind, scope)), Some(remaining)) = (shape, number(v.get("remainingFraction"))) {
                windows.push(UsageWindow {
                    kind,
                    scope: scope.map(String::from),
                    used_percent: percent((1.0 - remaining) * 100.0),
                    resets_at: iso_time(v.get("resetTime")),
                    starts_at: None,
                });
            }
        }
    }
    Ok(Some(Reading { plan: None, windows, account: None }))
}

// ---- OpenCode Go: OpenCode가 <XDG data>/opencode/auth.json에 둔 열쇠, 아니면 OPENCODE_API_KEY ----

/// OpenCode는 모든 로그인을 그 파일 하나에 둔다. API 열쇠는 `{ type: "api", key }`이고 계정을 말하지 않는다.
/// Go 열쇠를 먼저, 그다음 Zen 열쇠: Go endpoint는 어느 console 열쇠든 받고, Go 구독이 없으면 403이다.
fn opencode_sign_in(source: Option<String>) -> Option<SignIn> {
    let root = json_record(source);
    ["opencode-go", "opencode"].iter().find_map(|p| {
        let e = rec(&root, p);
        if e.get("type").and_then(Value::as_str) != Some("api") {
            return None;
        }
        Some(SignIn { token: text(e.get("key"))?, ..Default::default() })
    })
}

async fn opencode_sign_ins(ctx: &UsageContext) -> Vec<Found> {
    // OpenCode가 찾는 대로(xdg-basedir): XDG_DATA_HOME, 아니면 ~/.local/share. macOS도 같다
    let dir =
        ctx.env("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| ctx.home.join(".local/share")).join("opencode");
    if let Some(s) = opencode_sign_in(read_text(&dir.join("auth.json"))) {
        return vec![Found::new(dir.to_string_lossy(), s, None)];
    }
    match ctx.env("OPENCODE_API_KEY") {
        Some(t) => vec![Found::new("env", SignIn { token: t.trim().to_string(), ..Default::default() }, None)],
        None => vec![],
    }
}

async fn opencode_read(ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    let body = match request_json(
        ctx,
        "GET",
        "https://opencode.ai/zen/go/v1/usage",
        &[
            ("Authorization", format!("Bearer {}", s.token)),
            ("Accept", "application/json".into()),
            ("User-Agent", USER_AGENT.into()),
        ],
        b"",
    )
    .await
    {
        Ok(b) => b,
        Err(ReadError::Http { status: 403, .. }) => return Ok(None),
        Err(e) => return Err(e),
    };
    let usage = rec(&body, "usage");
    let mut windows = vec![];
    for (key, kind) in
        [("rolling", UsageWindowKind::Session), ("weekly", UsageWindowKind::Week), ("monthly", UsageWindowKind::Month)]
    {
        let w = rec(&usage, key);
        if let Some(p) = number(w.get("percent")) {
            windows.push(UsageWindow {
                kind,
                scope: None,
                used_percent: percent(p),
                resets_at: iso_time(w.get("resetsAt")),
                starts_at: None,
            });
        }
    }
    Ok(Some(Reading { plan: text(body.get("plan")).or(Some("Go".into())), windows, account: None }))
}

async fn sign_ins(id: UsageProviderId, ctx: &UsageContext) -> Vec<Found> {
    match id {
        UsageProviderId::Claude => claude_sign_ins(ctx).await,
        UsageProviderId::Codex => codex_sign_ins(ctx).await,
        UsageProviderId::Cursor => cursor_sign_ins(ctx).await,
        UsageProviderId::Copilot => copilot_sign_ins(ctx).await,
        UsageProviderId::Grok => grok_sign_ins(ctx).await,
        UsageProviderId::Antigravity => antigravity_sign_ins(ctx).await,
        UsageProviderId::Opencode => opencode_sign_ins(ctx).await,
    }
}

async fn provider_read(id: UsageProviderId, ctx: &UsageContext, s: &SignIn) -> Result<Option<Reading>, ReadError> {
    match id {
        UsageProviderId::Claude => claude_read(ctx, s).await,
        UsageProviderId::Codex => codex_read(ctx, s).await,
        UsageProviderId::Cursor => cursor_read(ctx, s).await,
        UsageProviderId::Copilot => copilot_read(ctx, s).await,
        UsageProviderId::Grok => grok_read(ctx, s).await,
        UsageProviderId::Antigravity => antigravity_read(ctx, s).await,
        UsageProviderId::Opencode => opencode_read(ctx, s).await,
    }
}

/// 로그인 하나의 마지막 읽기
#[derive(Clone)]
struct Entry {
    /// None: 이 plan 없이 로그인했다
    usage: Option<ProviderUsage>,
    next_at: u64,
}

/// 구독의 로그인을 마지막으로 찾은 때와 그때 찾은 곳들
struct Search {
    read_at: u64,
    next_at: u64,
    sources: Vec<String>,
}

#[derive(Default)]
struct State {
    searches: HashMap<UsageProviderId, Search>,
    /// 구독과 찾은 곳으로
    entries: HashMap<String, Entry>,
    /// 429 뒤 계정마다 다시 물어도 되는 때(다음에 어디서 찾든)
    backoffs: HashMap<String, u64>,
}

enum Backoff {
    Keep,
    Remove(String),
    Set(String, u64),
}

/// 새로 읽은 곳마다 (곳, 기록, 429 기록 바꿈)과 다음에 찾을 때
type Fresh = (Vec<(String, Entry, Backoff)>, u64);

struct ProviderPass {
    id: UsageProviderId,
    /// None이면 저장된 것을 그대로 쓴다
    fresh: Option<Fresh>,
    cached_sources: Vec<String>,
}

pub struct UsageService {
    ctx: UsageContext,
    /// 한 번에 한 바퀴. 동시에 물은 client는 앞 바퀴가 끝난 뒤 그 결과(아직 새 것)를 받는다
    state: tokio::sync::Mutex<State>,
}

fn hash12(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))[..12].to_string()
}

impl UsageService {
    pub fn new(ctx: UsageContext) -> UsageService {
        UsageService { ctx, state: Default::default() }
    }

    /// `refresh`는 FRESH_MS보다 일찍 다시 묻는다(MIN_REFRESH_MS에 한 번까지).
    pub async fn report(&self, refresh: bool) -> UsageReport {
        let mut st = self.state.lock().await;
        let now = (self.ctx.now)();
        let passes =
            futures_util::future::join_all(PROVIDERS.iter().map(|&id| self.provider_pass(id, &st, now, refresh))).await;
        let mut usages: Vec<ProviderUsage> = vec![];
        for p in passes {
            let sources = match p.fresh {
                None => p.cached_sources,
                Some((read, next_at)) => {
                    let old = st.searches.get(&p.id).map(|s| s.sources.clone()).unwrap_or_default();
                    for s in old {
                        if !read.iter().any(|(found, _, _)| *found == s) {
                            st.entries.remove(&s);
                        }
                    }
                    let mut sources = vec![];
                    for (source, entry, backoff) in read {
                        match backoff {
                            Backoff::Keep => {}
                            Backoff::Remove(k) => {
                                st.backoffs.remove(&k);
                            }
                            Backoff::Set(k, until) => {
                                st.backoffs.insert(k, until);
                            }
                        }
                        st.entries.insert(source.clone(), entry);
                        sources.push(source);
                    }
                    st.searches.insert(p.id, Search { read_at: now, next_at, sources: sources.clone() });
                    sources
                }
            };
            usages.extend(sources.iter().filter_map(|s| st.entries.get(s).and_then(|e| e.usage.clone())));
        }
        // 두 곳에서 찾은 한 계정은 한 번: 문제 없는 읽기, 아니면 처음 것
        let mut by_key: Vec<ProviderUsage> = vec![];
        for u in usages {
            match by_key.iter_mut().find(|h| h.key == u.key) {
                Some(held) => {
                    if held.problem.is_some() && u.problem.is_none() {
                        *held = u;
                    }
                }
                None => by_key.push(u),
            }
        }
        // 계정을 말하지 않는 한 구독의 로그인 둘 이상은 key 순서대로 번호로 가른다
        for id in PROVIDERS {
            let mut group: Vec<usize> =
                (0..by_key.len()).filter(|&i| by_key[i].id == id && by_key[i].account.is_none()).collect();
            if group.len() < 2 {
                continue;
            }
            group.sort_by(|&a, &b| by_key[a].key.cmp(&by_key[b].key));
            for (n, i) in group.into_iter().enumerate() {
                by_key[i].account = Some(format!("#{}", n + 1));
            }
        }
        UsageReport { providers: by_key }
    }

    async fn provider_pass(&self, id: UsageProviderId, st: &State, now: u64, refresh: bool) -> ProviderPass {
        let search = st.searches.get(&id);
        // 계정의 read_at은 구독의 것보다 늦지 않다: 억지 찾기는 계정마다 다시 물어도 된다
        let forced = refresh && search.is_some_and(|s| now.saturating_sub(s.read_at) >= MIN_REFRESH_MS);
        if let Some(s) = search
            && now < s.next_at
            && !forced
        {
            return ProviderPass { id, fresh: None, cached_sources: s.sources.clone() };
        }
        let mut found = sign_ins(id, &self.ctx).await;
        // 두 곳의 같은 token은 한 로그인이다
        let mut tokens = HashSet::new();
        found.retain(|f| f.sign_in.as_ref().is_none_or(|s| tokens.insert(s.token.clone())));
        let read = futures_util::future::join_all(found.into_iter().map(|f| async move {
            let source = format!("{}|{}", pid(id), f.source);
            let entry = st.entries.get(&source);
            let current = entry.is_some_and(|e| {
                now < e.next_at
                    && !(forced && e.usage.as_ref().and_then(|u| u.problem) != Some(UsageProblem::RateLimited))
            });
            if current {
                return (source, entry.unwrap().clone(), Backoff::Keep);
            }
            let previous = entry.and_then(|e| e.usage.clone());
            let (e, b) = self.read(id, &f, previous, now, &st.backoffs).await;
            (source, e, b)
        }))
        .await;
        let next_at = read.iter().map(|(_, e, _)| e.next_at).fold(now + FRESH_MS, u64::min);
        ProviderPass { id, fresh: Some((read, next_at)), cached_sources: vec![] }
    }

    async fn read(
        &self,
        id: UsageProviderId,
        found: &Found,
        previous: Option<ProviderUsage>,
        now: u64,
        backoffs: &HashMap<String, u64>,
    ) -> (Entry, Backoff) {
        let settle = |usage: Option<ProviderUsage>, wait: u64| Entry { usage, next_at: now + wait };
        // 계정은 id로 안다. 로그인이 말하지 않는 계정은 찾은 곳으로(hash: key는 브라우저로 가고, 로그인의 경로는 가면 안 된다)
        let identity = |account: Option<&Account>| -> (String, Option<String>) {
            match account.or(found.account.as_ref()) {
                Some(a) => (format!("{}:{}", pid(id), a.id), a.label.clone()),
                None => (format!("{}@{}", pid(id), hash12(&found.source)), None),
            }
        };
        // 마지막 숫자는 남고 그 뒤 잘못된 것이 이름으로 붙는다. 다만 그 곳이 이제 다른 계정의 로그인이면 그 숫자와 이름은 남의 것이었다
        let kept = previous.filter(|p| found.account.is_none() || p.key == identity(None).0);
        let keep = |problem: UsageProblem, plan: Option<String>, wait: u64| {
            let (key, account) = match &kept {
                Some(k) => (k.key.clone(), k.account.clone()),
                None => identity(None),
            };
            settle(
                Some(ProviderUsage {
                    id,
                    key,
                    account,
                    plan: kept.as_ref().and_then(|k| k.plan.clone()).or(plan),
                    windows: kept.as_ref().map(|k| k.windows.clone()).unwrap_or_default(),
                    problem: Some(problem),
                    checked_at: kept.as_ref().and_then(|k| k.checked_at.clone()),
                }),
                wait,
            )
        };
        let Some(sign_in) = &found.sign_in else {
            return (keep(UsageProblem::Locked, None, FRESH_MS), Backoff::Keep);
        };
        if sign_in.expires_at.is_some_and(|t| t <= now) {
            return (keep(UsageProblem::Expired, sign_in.plan.clone(), RETRY_MS), Backoff::Keep);
        }
        // 429는 곳이 아니라 계정에 걸린다: 다른 곳으로 옮기거나 베낀 로그인도 기다린다
        let backoff = match &found.account {
            Some(a) => format!("{}:{}", pid(id), a.id),
            None => format!("{}#{}", pid(id), hex::encode(Sha256::digest(sign_in.token.as_bytes()))),
        };
        if let Some(&until) = backoffs.get(&backoff)
            && now < until
        {
            return (keep(UsageProblem::RateLimited, sign_in.plan.clone(), until - now), Backoff::Keep);
        }
        let cleared = if backoffs.contains_key(&backoff) { Backoff::Remove(backoff.clone()) } else { Backoff::Keep };
        match provider_read(id, &self.ctx, sign_in).await {
            Ok(None) => (settle(None, FRESH_MS), cleared),
            Ok(Some(r)) => {
                let (key, account) = identity(r.account.as_ref());
                let usage = ProviderUsage {
                    id,
                    key,
                    account,
                    plan: r.plan.or(sign_in.plan.clone()),
                    windows: r.windows,
                    problem: None,
                    checked_at: Some(utc_iso(now)),
                };
                (settle(Some(usage), FRESH_MS), cleared)
            }
            Err(ReadError::Http { status: 401 | 403, .. }) => {
                (keep(UsageProblem::Expired, sign_in.plan.clone(), RETRY_MS), cleared)
            }
            Err(ReadError::Http { status: 429, retry_after_ms }) => {
                let wait = RETRY_MS.max(retry_after_ms.unwrap_or(FRESH_MS));
                (keep(UsageProblem::RateLimited, sign_in.plan.clone(), wait), Backoff::Set(backoff, now + wait))
            }
            Err(e) => {
                let why = match e {
                    ReadError::Http { status, .. } => format!("HTTP {status}"),
                    ReadError::Other(m) => m,
                };
                tracing::warn!("usage: {}를 읽지 못했다: {why}", pid(id));
                (keep(UsageProblem::Failed, sign_in.plan.clone(), RETRY_MS), cleared)
            }
        }
    }
}

#[cfg(test)]
mod tests;
