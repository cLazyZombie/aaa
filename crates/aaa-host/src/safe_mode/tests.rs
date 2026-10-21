//! 안전 모드(관리 GUI W6b, plan §8.368): 원문 가림, 오류 글 정리, 마지막 정상 설정 보관·되돌리기, 뜰 주소, 기기별 화면과 POST 권한.
//! host 재시작 전체(설정을 깨고 다시 띄우기)는 웹 E2E 63이 실제 binary로 본다.
use super::*;
use axum::body::Body;
use tower::ServiceExt;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp() -> Tmp {
    let d = std::env::temp_dir().join(format!("aaa-safe-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(d.join("config")).unwrap();
    std::fs::create_dir_all(d.join("state").join("host")).unwrap();
    Tmp(d)
}

#[test]
fn mask_hides_env_secret_keys_and_secret_looking_values_but_keeps_the_rest() {
    let text = r#"[web]
listen = "127.0.0.1:7431"
[notify]
webhook = "https://hooks.example.com/abc123"
[mcp.auth]
value = "hdr-zzz-111"
[profiles.work.env]
PLAIN = "visible-under-env"
[profiles.home]
env = { HOME_TOKEN = "inline-env-secret" }
api_key = "sk-live-zzz"
note = "my password is hunter2x"
# old_token = "commented-secret"
private_note = """
first-line-secret
"""
tokens = [
  "array-secret-1",
  "array-secret-2",
]
after = "kept-value"
"#;
    let (lines, hidden) = mask(text);
    let shown = lines.join("\n");
    for secret in [
        "hooks.example.com",
        "visible-under-env",
        "inline-env-secret",
        "sk-live-zzz",
        "hunter2x",
        "commented-secret",
        "first-line-secret",
        "array-secret-1",
        "array-secret-2",
        "hdr-zzz-111",
    ] {
        assert!(!shown.contains(secret), "{secret}가 보인다:\n{shown}");
        assert!(hidden.iter().any(|h| h.contains(secret)), "{secret}를 오류 글에서 뺄 목록에 없다: {hidden:?}");
    }
    // 줄 수는 그대로다(오류 줄 표시가 맞는다), 비밀이 아닌 값과 표 머리는 남는다
    assert_eq!(lines.len(), text.lines().count());
    assert_eq!(lines[1], r#"listen = "127.0.0.1:7431""#);
    assert_eq!(lines[3], format!("webhook = {HIDDEN}"));
    assert_eq!(lines[12], format!("# old_token = {HIDDEN}"));
    assert_eq!(lines.last().unwrap(), r#"after = "kept-value""#);
    assert!(lines.contains(&"[profiles.work.env]".to_string()));
}

#[test]
fn every_continuation_line_of_a_multiline_value_is_hidden() {
    // 닫는 표시가 빠진 보통 여러 줄 문자열(파싱이 깨진 바로 그 경우): 뒤의 env 표까지 문자열로 보고 끝까지 가린다
    let unclosed = "[profiles.work]\ndescription = \"\"\"\n메모\n[profiles.work.env]\nOPENAI_API_KEY = \"sk-live-after-unclosed\"\n";
    let (lines, hidden) = mask(unclosed);
    assert_eq!(&lines[..2], ["[profiles.work]", "description = \"\"\""]);
    assert!(lines[2..].iter().all(|l| l == HIDDEN), "{lines:?}");
    assert!(hidden.iter().any(|h| h.contains("sk-live-after-unclosed")), "{hidden:?}");
    // 비밀이 아닌 key의 여러 줄 배열: 원소를 줄마다 하나씩 쓰면 `"--api-key",` 다음 줄의 값에는 비밀 낱말이 없다.
    // 배열 안의 한 원소짜리 배열(`["…"]`)도 표 머리가 아니다. `=` 없는 주석의 비밀
    let text = r#"[profiles.other]
args = [
  "--verbose",
  "--header=Authorization: Bearer bearer-zzz-111",
  "--api-key", "sk-live-array-zzz",
]
# token: sk-comment-zzz
[profiles.third]
args = [
  "--api-key",
  "sk-one-per-line-zzz",
  ["sk-live-nested"]
]
after = "kept-after-array"
"#;
    let (lines, hidden) = mask(text);
    let shown = lines.join("\n");
    for secret in ["bearer-zzz-111", "sk-live-array-zzz", "sk-comment-zzz", "sk-one-per-line-zzz", "sk-live-nested"] {
        assert!(!shown.contains(secret), "{secret}가 보인다:\n{shown}");
        assert!(hidden.iter().any(|h| h.contains(secret)), "{secret}를 오류 글에서 뺄 목록에 없다: {hidden:?}");
    }
    assert_eq!(lines.len(), text.lines().count());
    assert_eq!(lines[1], "args = [");
    assert!(lines[2..6].iter().all(|l| l == HIDDEN), "{lines:?}");
    assert_eq!(lines.last().unwrap(), r#"after = "kept-after-array""#, "배열이 닫힌 뒤는 다시 보인다");
}

#[test]
fn an_unclosed_or_commented_value_does_not_hide_or_leak_the_next_lines() {
    // 주석으로 꺼 둔 배열, `]`를 빠뜨린 배열 뒤에 env 표가 온다(공백 없이 주석이 붙은 머리 포함)
    let text = r#"[profiles.work]
# args = [
#   "--verbose",
# ]
[profiles.work.env]
ANTHROPIC_BASE_URL = "https://proxy.internal/x"
[profiles.other]
args = [
  "--verbose",
[profiles.other.env]# 메모
DATABASE_URL = "postgres://u:pw@db/x"
[web
"#;
    let (lines, hidden) = mask(text);
    let shown = lines.join("\n");
    for secret in ["proxy.internal", "u:pw@db"] {
        assert!(!shown.contains(secret), "{secret}가 보인다:\n{shown}");
        assert!(hidden.iter().any(|h| h.contains(secret)), "{secret}를 오류 글에서 뺄 목록에 없다: {hidden:?}");
    }
    assert_eq!(lines.len(), text.lines().count());
    assert_eq!(lines[4], "[profiles.work.env]");
    // 주석으로 꺼 둔 비밀 배열·여러 줄 문자열: 이어지는 주석 줄은 모두 가리고(주석 안의 `]`·`"""`는 닫는 표시로 셀 수 없다),
    // 주석이 끝나면 뒤의 표·값은 그대로 보인다
    let text = "[profiles.work]\n# api_keys = [\n#   \"sk-live-commented\",\n# ]\n# private_key = \"\"\"\n# MIIEvQIBADANBg\n# \"\"\"\n[web]\nlisten = \"127.0.0.1:7431\"\n";
    let (lines, hidden) = mask(text);
    let shown = lines.join("\n");
    for secret in ["sk-live-commented", "MIIEvQIBADANBg"] {
        assert!(!shown.contains(secret), "{secret}가 보인다:\n{shown}");
        assert!(hidden.iter().any(|h| h.contains(secret)), "{secret}를 오류 글에서 뺄 목록에 없다: {hidden:?}");
    }
    assert_eq!(lines[1], format!("# api_keys = {HIDDEN}"));
    assert_eq!(&lines[7..], ["[web]", "listen = \"127.0.0.1:7431\""]);
}

#[test]
fn error_text_drops_source_snippets_and_quoted_or_masked_values() {
    let file = PathBuf::from("/c/host.toml");
    let toml = Diag {
        severity: aaa_config::Severity::Error,
        file: file.clone(),
        line: 3,
        col: 9,
        message:
            "TOML parse error at line 3, column 9\n  |\n3 | token = sk-unquoted-zzz\n  |         ^\ninvalid string"
                .into(),
    };
    let shown = clean_message(&toml, &["sk-unquoted-zzz".into()]);
    assert_eq!(shown, "TOML parse error at line 3, column 9 · invalid string");
    let serde = Diag {
        severity: aaa_config::Severity::Error,
        file,
        line: 0,
        col: 0,
        message: "겹친 설정이 잘못됐다: invalid type: string \"pa\\\"ss\", expected u16 for `port`".into(),
    };
    assert_eq!(
        clean_message(&serde, &[]),
        format!("겹친 설정이 잘못됐다: invalid type: string \"{HIDDEN}\", expected u16 for `port`")
    );
}

#[test]
fn restore_backs_up_current_files_and_brings_back_exactly_the_kept_set() {
    let t = tmp();
    let config = t.0.join("config");
    // 정상 시작 때는 host.toml만 있었다
    std::fs::write(config.join("host.toml"), "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n").unwrap();
    keep_last_good(&config, &[config.join("host.toml")]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&last_good_dir(&config)), 0o700);
        assert_eq!(mode(&last_good_dir(&config).join("host.toml")), 0o600);
    }
    // 그 뒤 host.toml을 깨고 host.local.toml을 새로 만들었다
    std::fs::write(config.join("host.toml"), "[web\n").unwrap();
    std::fs::write(config.join("host.local.toml"), "x = 1\n").unwrap();
    let backups = restore(&config, 1_700_000_000_000).unwrap();
    let stamp = aaa_common::util::utc_compact(1_700_000_000_000);
    assert_eq!(
        backups,
        vec![config.join(format!("host.toml.broken-{stamp}")), config.join(format!("host.local.toml.broken-{stamp}"))]
    );
    assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), "[web\n");
    assert_eq!(std::fs::read_to_string(&backups[1]).unwrap(), "x = 1\n");
    assert_eq!(
        std::fs::read_to_string(config.join("host.toml")).unwrap(),
        "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n"
    );
    assert!(!config.join("host.local.toml").exists());
    // 같은 시각에 다시 되돌려도 남긴 파일을 덮지 않는다(바뀐 게 없으면 아무것도 남기지 않는다)
    assert!(restore(&config, 1_700_000_000_000).unwrap().is_empty());
    std::fs::write(config.join("host.toml"), "[web\n").unwrap();
    assert!(restore(&config, 1_700_000_000_000).is_err(), "같은 이름의 남긴 파일을 덮었다");
    // 정상 시작이 local을 읽었으면 local도 되돌린다, 정상 시작이 없었으면 되돌리지 못한다
    std::fs::write(config.join("host.toml"), "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n").unwrap();
    std::fs::write(config.join("host.local.toml"), "schema = 1\n").unwrap();
    keep_last_good(&config, &[config.join("host.toml"), config.join("host.local.toml")]).unwrap();
    std::fs::remove_file(config.join("host.local.toml")).unwrap();
    assert!(restore(&config, 1_700_000_001_000).unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(config.join("host.local.toml")).unwrap(), "schema = 1\n");
    assert!(restore(&t.0.join("other"), 0).is_err());
    // 둘째 파일을 남기지 못하면(같은 이름이 이미 있다) 첫째도 바꾸지 않는다
    std::fs::write(config.join("host.toml"), "[web\n").unwrap();
    std::fs::write(config.join("host.local.toml"), "[local\n").unwrap();
    let stamp = aaa_common::util::utc_compact(1_700_000_002_000);
    std::fs::write(config.join(format!("host.local.toml.broken-{stamp}")), "older").unwrap();
    let err = restore(&config, 1_700_000_002_000).unwrap_err();
    assert!(!format!("{err:#}").contains(&*t.0.to_string_lossy()), "오류 글에 경로가 있다: {err:#}");
    assert_eq!(std::fs::read_to_string(config.join("host.toml")).unwrap(), "[web\n");
    assert_eq!(std::fs::read_to_string(config.join("host.local.toml")).unwrap(), "[local\n");
    // 먼저 남긴 host.toml 사본도 치운다(다시 누르면 같은 이름에 막히지 않는다), 이미 있던 파일은 그대로다
    assert!(!config.join(format!("host.toml.broken-{stamp}")).exists());
    assert_eq!(std::fs::read_to_string(config.join(format!("host.local.toml.broken-{stamp}"))).unwrap(), "older");
}

#[test]
fn last_good_keeps_the_previous_copy_when_the_file_changed_into_a_broken_one() {
    let t = tmp();
    let config = t.0.join("config");
    std::fs::write(config.join("host.toml"), "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n").unwrap();
    keep_last_good(&config, &[config.join("host.toml")]).unwrap();
    // 시작한 뒤 보관하기 전에 사람이 고치다 깨진 채 저장했다
    std::fs::write(config.join("host.toml"), "schema = 1\n[web\n").unwrap();
    assert!(keep_last_good(&config, &[config.join("host.toml")]).is_err());
    assert_eq!(
        std::fs::read_to_string(last_good_dir(&config).join("host.toml")).unwrap(),
        "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n"
    );
    assert!(!config.join("last-good-next").exists());
}

#[tokio::test]
async fn listen_prefers_the_last_bound_address_then_last_good_and_never_guesses() {
    let t = tmp();
    let state = t.0.join("state");
    let db = Db::open(&state.join("host").join("host.db")).unwrap();
    let config = t.0.join("config");
    // 이 판으로 정상 시작한 적이 없다(업그레이드 직후): 기본 주소에 뜨지 않는다
    assert_eq!(listen_addr(&db, &config), None);
    std::fs::write(config.join("host.toml"), "schema = 1\n[web]\nlisten = \"127.0.0.1:7999\"\n").unwrap();
    keep_last_good(&config, &[config.join("host.toml")]).unwrap();
    assert_eq!(listen_addr(&db, &config).as_deref(), Some("127.0.0.1:7999"));
    // 정상 host가 실제로 연 주소(:0이면 받은 port)를 적고, 안전 모드는 그 주소를 먼저 쓴다
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    cfg.web.listen = "127.0.0.1:0".into();
    let h = crate::Host::open(cfg.clone(), state.join("host")).unwrap();
    crate::web::start(&h).await.unwrap();
    let bound = h.web_listen.lock().unwrap().clone().unwrap();
    assert_eq!(listen_addr(&db, &config), Some(bound));
    // 웹을 끈 설정으로 정상 시작하면 지난 주소를 지워, 다음 안전 모드도 웹을 열지 않는다(보관 설정에 listen이 있어도)
    cfg.web.listen = String::new();
    let h = crate::Host::open(cfg, state.join("host")).unwrap();
    crate::web::start(&h).await.unwrap();
    assert_eq!(listen_addr(&db, &config), None);
}

struct Lab {
    _t: Tmp,
    safe: Arc<Safe>,
    config: PathBuf,
    reload: tokio::sync::watch::Receiver<bool>,
}
const LOOP: &str = "127.0.0.1:7000";
const ORIGIN: &str = "http://127.0.0.1:7000";

fn lab() -> Lab {
    let t = tmp();
    let (config, state) = (t.0.join("config"), t.0.join("state"));
    std::fs::write(config.join("host.toml"), "schema = 1\n[web]\nlisten = \"127.0.0.1:7000\"\n").unwrap();
    keep_last_good(&config, &[config.join("host.toml")]).unwrap();
    std::fs::write(
        config.join("host.toml"),
        "schema = 1\n[web]\nlisten = \"127.0.0.1:7000\"\nbogus = \n[profiles.p.env]\nX = \"env-secret-value\"\n",
    )
    .unwrap();
    let errors = aaa_config::load_host(&config).unwrap_err();
    let db = Db::open(&state.join("host").join("host.db")).unwrap();
    db.add_device("ctl", "controller", &crate::token_hash("ctl-token"), "책상").unwrap();
    db.add_device("rd", "reader", &crate::token_hash("rd-token"), "벽").unwrap();
    let (done, reload) = tokio::sync::watch::channel(false);
    let safe = Arc::new(Safe { db, port: 7000, config_dir: config.clone(), errors: Mutex::new(errors), done });
    Lab { _t: t, safe, config, reload }
}

async fn ask(
    l: &Lab,
    method: &str,
    path: &str,
    host: &str,
    token: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, String) {
    let mut req = axum::http::Request::builder().method(method).uri(path).header(header::HOST, host);
    if let Some(t) = token {
        req = req.header(header::COOKIE, format!("{}={t}", crate::web::cookie_name(false)));
    }
    if let Some(o) = origin {
        req = req.header(header::ORIGIN, o);
    }
    let res = router(l.safe.clone()).oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn page_shows_errors_and_masked_source_only_to_a_controller() {
    let l = lab();
    let (status, body) = ask(&l, "GET", "/", LOOP, Some("ctl-token"), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("마지막 정상 설정으로 되돌리기"), "{body}");
    assert!(body.contains("<span class=err data-line=4>"), "오류 줄 표시가 없다:\n{body}");
    assert!(body.contains("host.toml:4:"), "{body}");
    assert!(!body.contains("env-secret-value"), "env 값이 보인다");
    for (token, words) in [
        (Some("rd-token"), "보기 전용"),
        (None, "짝지어진 기기가 아닙니다"),
        (Some("revoked"), "짝지어진 기기가 아닙니다"),
    ] {
        let (status, body) = ask(&l, "GET", "/", LOOP, token, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains(words), "{token:?}: {body}");
        assert!(!body.contains("bogus") && !body.contains("되돌리기</button>"), "{token:?}에 원문·단추가 보인다");
    }
    // 열린 웹 앱의 API는 503 unavailable(배포 감시가 ready로 보지 않는다), 다른 Host는 거부
    let (status, body) = ask(&l, "GET", "/api/v1/me", LOOP, Some("ctl-token"), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("\"unavailable\"") && body.contains("안전 모드"), "{body}");
    assert_eq!(ask(&l, "GET", "/api/v1/sessions", LOOP, None, None).await.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        ask(&l, "GET", "/", "evil.example:7000", Some("ctl-token"), None).await.0,
        StatusCode::MISDIRECTED_REQUEST
    );
}

#[tokio::test]
async fn restore_needs_a_controller_and_this_origin_then_ends_safe_mode() {
    let mut l = lab();
    let broken = std::fs::read_to_string(l.config.join("host.toml")).unwrap();
    // 다른 Origin·Origin 없음(CSRF), 보기 전용·짝 없는 기기는 파일을 건드리지 않는다
    for (token, origin, want) in [
        (Some("ctl-token"), Some("http://evil.example"), StatusCode::FORBIDDEN),
        (Some("ctl-token"), None, StatusCode::FORBIDDEN),
        (Some("rd-token"), Some(ORIGIN), StatusCode::FORBIDDEN),
        (None, Some(ORIGIN), StatusCode::FORBIDDEN),
    ] {
        for path in ["/api/v1/safe/restore", "/api/v1/safe/retry"] {
            assert_eq!(ask(&l, "POST", path, LOOP, token, origin).await.0, want, "{path} {token:?} {origin:?}");
        }
    }
    assert_eq!(std::fs::read_to_string(l.config.join("host.toml")).unwrap(), broken);
    assert!(!*l.reload.borrow());
    // 다시 읽기: 아직 깨져 있으면 안전 모드에 남는다
    let (status, body) = ask(&l, "POST", "/api/v1/safe/retry", LOOP, Some("ctl-token"), Some(ORIGIN)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("아직 설정을 읽지 못한다"), "{body}");
    assert!(!*l.reload.borrow());
    // 되돌리기: 지금 파일은 옆에 남기고, 정상 설정으로 바꿔 안전 모드를 끝낸다
    let (status, body) = ask(&l, "POST", "/api/v1/safe/restore", LOOP, Some("ctl-token"), Some(ORIGIN)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("safe-reloading"), "{body}");
    // 실패 한도: 끝을 알리지 않으면 기다림이 끝나지 않는다
    tokio::time::timeout(std::time::Duration::from_secs(5), l.reload.changed())
        .await
        .expect("안전 모드 끝을 알리지 않았다")
        .unwrap();
    assert!(*l.reload.borrow());
    assert!(aaa_config::load_host(&l.config).is_ok());
    let backup: Vec<_> = std::fs::read_dir(&l.config)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("host.toml.broken-"))
        .collect();
    assert_eq!(backup.len(), 1, "{backup:?}");
    assert_eq!(std::fs::read_to_string(l.config.join(&backup[0])).unwrap(), broken);
    assert_eq!(
        l.safe.db.audit_actions(),
        [("device:ctl", "safe_retry"), ("device:ctl", "safe_restore")].map(|(a, b)| (a.to_string(), b.to_string()))
    );
}
