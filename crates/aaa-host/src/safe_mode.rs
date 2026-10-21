//! 안전 모드(관리 GUI W6b, plan §8.368): host가 설정을 읽지 못하면 끝내지 않고, 같은 주소·같은 기기 인증으로 최소 웹을 연다.
//! `Host`를 열지 않는다: host DB(기기·공개 주소)만 연다. 웹은 오류 줄·가린 원문과 「마지막 정상 설정으로 되돌리기」·「다시 읽기」를
//! 보인다. 마지막 정상 설정은 정상 시작마다 상태 폴더에 보관한다. JS 없이 서버가 그린 화면과 form POST만 쓴다.
//!
//! - 화면과 API는 503으로 답한다: 웹 service worker가 이 화면을 셸로 두지 않고, 배포 감시(W6c)가 `/api/v1/me`를 ready로 보지 않는다.
//! - 원문은 env 표·인라인 env·비밀처럼 보이는 key·그런 낱말이 든 값을 가린다. 오류 글에서도 원문 조각과 가린 값을 뺀다.
//! - 조작 기기만 원문을 보고 되돌린다. 보기 전용 기기는 안내만, 짝짓지 않은 기기는 host Mac 터미널 안내만 본다.
use crate::db::Db;
use aaa_config::{ConfigErrors, Diag};
use aaa_proto::{ErrorBody, ErrorCode, Outcome};
use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 정상 host가 실제로 연 웹 주소(DB meta). 안전 모드는 이 주소에 뜬다
pub(crate) const LISTEN_KEY: &str = "web_listen_last";
/// 보관하는 설정 파일 이름(설정 폴더 기준). 다른 파일은 손대지 않는다
const FILES: [&str; 2] = ["host.toml", "host.local.toml"];
const HIDDEN: &str = "«가림»";
const MARK: &str = "safe_mode";

/// 안전 모드가 끝난 까닭
#[derive(Debug, PartialEq, Eq)]
pub enum SafeExit {
    /// 설정을 다시 읽을 수 있다(되돌리기·다시 읽기): 부른 쪽이 정상 host로 다시 시작한다
    Reload,
    /// 종료 신호
    Shutdown,
}

/// 마지막 정상 설정의 폴더(설정 폴더 아래, 0700). 상태 폴더에 두지 않는다: 평문 env 값·인증 값은 원래 설정 폴더에만 있고,
/// 상태 폴더·DB·로그·내보내기에는 남기지 않는다(plan §8.307 K0·계정 시험이 지키는 계약)
pub fn last_good_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("last-good")
}

/// 정상 시작 뒤 그때 읽은 설정 파일을 보관한다. 없던 파일은 없는 채로 기록한다(되돌릴 때 지금 것을 치운다).
/// 시작한 뒤 다시 읽으므로, 그사이 사람이 고치다 깨진 파일을 보관하지 않게 옆 폴더에 먼저 써서 읽히는지 본 뒤에만 바꾼다.
/// 평문 env 값이 든 설정이면 같은 설정 폴더 안에 사본이 하나 더 생긴다: 폴더는 0700, 파일은 0600이다
pub fn keep_last_good(config_dir: &Path, files: &[PathBuf]) -> Result<()> {
    let dir = last_good_dir(config_dir);
    let next = config_dir.join("last-good-next");
    match std::fs::remove_dir_all(&next) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    aaa_common::util::ensure_private_dir(&next)?;
    let mut texts = Vec::new();
    for name in FILES {
        if let Some(src) = files.iter().find(|f| f.file_name().is_some_and(|n| n == name)) {
            let text = std::fs::read(src).with_context(|| format!("{name} 읽기"))?;
            aaa_common::util::write_private(&next.join(name), &text)?;
            texts.push((name, text));
        }
    }
    let readable = aaa_config::load_host(&next).is_ok();
    std::fs::remove_dir_all(&next)?;
    if !readable {
        anyhow::bail!("시작한 뒤 설정 파일이 바뀌어 읽히지 않는다: 지난 정상 설정을 그대로 둔다");
    }
    aaa_common::util::ensure_private_dir(&dir)?;
    for name in FILES {
        let target = dir.join(name);
        match texts.iter().find(|(n, _)| *n == name) {
            Some((_, text)) => {
                if std::fs::read(&target).ok().as_deref() != Some(text.as_slice()) {
                    aaa_common::util::write_private(&target, text)?;
                }
            }
            None => match std::fs::remove_file(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
        }
    }
    // 목록이 마지막에 바뀐다: 파일을 쓰는 중에 끊기면 옛 목록이 남아 되돌리기는 옛 판(이미 쓴 같은 이름 파일)을 쓴다
    let kept: Vec<&str> = texts.iter().map(|(n, _)| *n).collect();
    aaa_common::util::write_private(&dir.join("files.json"), &serde_json::to_vec(&kept)?)?;
    Ok(())
}

/// 보관한 정상 설정의 파일 이름들. 보관한 적이 없으면 None
fn kept(config_dir: &Path) -> Option<Vec<String>> {
    serde_json::from_slice(&std::fs::read(last_good_dir(config_dir).join("files.json")).ok()?).ok()
}

/// 지금 설정 파일을 옆에 `<이름>.broken-<UTC>`로 남기고 마지막 정상 설정으로 바꾼다. 정상 설정에 없던 파일은 남긴 뒤 치운다.
/// 남긴 파일 경로를 돌려준다
pub(crate) fn restore(config_dir: &Path, now_ms: u64) -> Result<Vec<PathBuf>> {
    let kept = kept(config_dir).context("보관한 정상 설정이 없다")?;
    let dir = last_good_dir(config_dir);
    // 읽을 수 있는지 먼저 다 본다: 반쯤 바꾸지 않는다
    let mut good = Vec::new();
    for name in FILES {
        let text = if kept.iter().any(|k| k == name) {
            Some(std::fs::read(dir.join(name)).with_context(|| format!("보관한 {name}"))?)
        } else {
            None
        };
        good.push((name, text));
    }
    let mut changes = Vec::new();
    for (name, text) in good {
        let current = config_dir.join(name);
        let now = match std::fs::read(&current) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("지금 {name}")),
        };
        if now != text {
            changes.push((name, current, now, text));
        }
    }
    // 남길 파일을 모두 만든 뒤에 바꾼다: 둘째 파일의 남기기가 실패해도 첫째만 바뀐 채로 두지 않는다
    let stamp = aaa_common::util::utc_compact(now_ms);
    let mut backups = Vec::new();
    for (name, _, now, _) in &changes {
        let Some(bytes) = now else { continue };
        let backup_name = format!("{name}.broken-{stamp}");
        let backup = config_dir.join(&backup_name);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        // 화면에 가는 오류 글에는 파일 이름만 쓴다(홈 경로를 싣지 않는다)
        let made = opts.open(&backup).and_then(|mut f| std::io::Write::write_all(&mut f, bytes));
        if let Err(e) = made {
            // 이 파일을 남기지 못했다: 앞서 남긴 것도 치우고 아무 파일도 바꾸지 않는다(같은 초에 다시 눌러도 막히지 않게)
            for b in &backups {
                let _ = std::fs::remove_file(b);
            }
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                let _ = std::fs::remove_file(&backup);
            }
            return Err(anyhow::Error::from(e).context(format!("{backup_name} 만들기")));
        }
        backups.push(backup);
    }
    // 바꾸다 실패하면 이미 바꾼 파일을 지금 것으로 되돌린다: 검증한 적 없는 조합(정상 host.toml + 깨진 local)을 남기지 않는다
    let put = |path: &Path, text: &Option<Vec<u8>>| match text {
        Some(t) => aaa_common::util::write_private(path, t),
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    };
    for (i, (name, current, _, text)) in changes.iter().enumerate() {
        if let Err(e) = put(current, text) {
            let restored = changes[..i].iter().all(|(_, done, now, _)| put(done, now).is_ok());
            let e = anyhow::Error::from(e).context(format!("{name} 바꾸기"));
            if !restored {
                // 원래 내용으로도 돌리지 못했다: 남긴 사본이 고치던 원문의 유일한 사본이라 지우지 않는다
                let names: Vec<String> = backups.iter().map(|b| file_name(b)).collect();
                return Err(
                    e.context(format!("원래 내용으로 돌리지 못했다. 지금 파일은 {}에 남아 있다", names.join(", ")))
                );
            }
            for b in &backups {
                let _ = std::fs::remove_file(b);
            }
            return Err(e);
        }
    }
    Ok(backups)
}

/// 안전 모드가 뜰 주소: 정상 host가 마지막으로 연 주소(웹을 끈 채 시작했으면 None) → 보관한 정상 설정의 listen. 둘 다 없거나 빈 주소면 None
pub(crate) fn listen_addr(db: &Db, config_dir: &Path) -> Option<String> {
    // 빈 값은 지난 정상 시작이 웹을 끈 설정이었다는 기록이다
    if let Some(addr) = db.meta(LISTEN_KEY).ok().flatten() {
        return (!addr.is_empty()).then_some(addr);
    }
    // 이 판으로 정상 시작한 적이 없다(업그레이드 직후): 설정과 무관한 기본 주소에 뜨지 않는다(다른 aaa의 port일 수 있다)
    kept(config_dir)?;
    let listen = aaa_config::load_host(&last_good_dir(config_dir)).ok()?.config.web.listen;
    (!listen.is_empty()).then_some(listen)
}

// ───────────── 가림 ─────────────

/// env 밖에서도 비밀을 담는 이름(webhook 주소 등)
fn secret_word(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    aaa_config::secret_like(text)
        || ["AUTH", "CREDENTIAL", "COOKIE", "PRIVATE", "WEBHOOK", "PASSWD"].iter().any(|w| upper.contains(w))
}

/// `[a."b".c]`·`[[a]]` 머리의 표 이름
fn table_path(line: &str) -> Vec<String> {
    let inner = line.split('#').next().unwrap_or("").trim().trim_start_matches('[').trim_end_matches(']').trim();
    inner.split('.').map(|p| p.trim().trim_matches('"').trim_matches('\'').to_string()).collect()
}

/// 따옴표 밖의 첫 `=` 자리
fn assignment(line: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '=') => return Some(i),
            (None, '#') => return None,
            _ => {}
        }
    }
    None
}

/// 따옴표 밖 첫 `#` 앞(주석을 뗀 본문)
fn uncommented(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '#') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// 따옴표 없는 key 이름만 든 표 머리(`[a.b]`·`[[a]]`). 배열 원소 줄(`["sk-…"]`)은 머리로 보지 않는다
fn header_like(line: &str) -> bool {
    let body = uncommented(line.trim()).trim_end();
    let inner = body
        .strip_prefix("[[")
        .and_then(|b| b.strip_suffix("]]"))
        .or_else(|| body.strip_prefix('[').and_then(|b| b.strip_suffix(']')));
    inner.is_some_and(|i| !i.trim().is_empty() && i.chars().all(|c| c.is_alphanumeric() || "_-. ".contains(c)))
}

/// 따옴표 밖 괄호(`[`·`{`)의 깊이 변화
fn depth(text: &str) -> i32 {
    let (mut d, mut quote) = (0, None);
    for c in text.chars() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '[' | '{') => d += 1,
            (None, ']' | '}') => d -= 1,
            (None, '#') => break,
            _ => {}
        }
    }
    d
}

/// 여러 줄 값의 이어지는 줄을 가리는 중
enum Cont {
    /// 여러 줄 문자열: 닫는 표시가 나올 때까지(닫히지 않으면 파일 끝까지)
    Text(&'static str),
    /// 여러 줄 배열·인라인 표: 괄호가 닫히거나 표 머리가 나올 때까지(`]`를 빠뜨려도 뒤의 표는 표로 본다)
    Nested(i32),
    /// 주석으로 연 여러 줄 값: 이어지는 주석 줄 전부(주석 안의 `]`·`"""`는 닫는 표시로 셀 수 없다)
    Comment,
}

/// 원문을 줄마다 가린다(파싱이 안 되는 글에도 쓴다). 줄 수는 그대로다(오류 줄 표시).
/// - 한 줄 값: env 표 아래 값, `env`가 든 key·인라인 표, 표 머리·key에 비밀 낱말이 있거나 값에 그런 낱말이 들면 가린다.
/// - 여러 줄 값(배열·문자열·인라인 표)의 이어지는 줄은 비밀 여부와 상관없이 모두 가린다. 원소를 줄마다 하나씩 쓰면
///   `"--api-key",` 다음 줄의 값에는 비밀 낱말이 없고, 닫는 표시가 빠진 바로 그때 뒤의 표를 값으로 읽게 되기 때문이다.
/// - 대입이 아닌 줄(주석 등)은 env 표 아래거나 비밀 낱말이 있으면 가린다.
///
/// 가린 값들도 돌려준다(오류 글에서 뺀다)
pub(crate) fn mask(text: &str) -> (Vec<String>, Vec<String>) {
    let mut table: Vec<String> = Vec::new();
    let mut cont: Option<Cont> = None;
    let (mut out, mut hidden) = (Vec::new(), Vec::new());
    let hide = |line: &str, out: &mut Vec<String>, hidden: &mut Vec<String>| {
        hidden.extend(
            line.split(',').map(|v| v.trim().trim_matches(['"', '\'', '[', ']', '{', '}', '#']).trim().to_string()),
        );
        out.push(HIDDEN.to_string());
    };
    for line in text.lines() {
        let trimmed = line.trim_start();
        match &mut cont {
            Some(Cont::Text(delim)) => {
                if line.contains(*delim) {
                    cont = None;
                }
                hide(trimmed, &mut out, &mut hidden);
                continue;
            }
            Some(Cont::Nested(_)) if header_like(line) => cont = None,
            Some(Cont::Nested(d)) => {
                *d += depth(line);
                if *d <= 0 {
                    cont = None;
                }
                hide(trimmed, &mut out, &mut hidden);
                continue;
            }
            Some(Cont::Comment) if trimmed.starts_with('#') => {
                hide(trimmed, &mut out, &mut hidden);
                continue;
            }
            Some(Cont::Comment) => cont = None,
            None => {}
        }
        if trimmed.starts_with('[') {
            table = table_path(trimmed);
            out.push(line.to_string());
            continue;
        }
        let in_env_table = table.iter().any(|s| s == "env");
        // 주석으로 꺼 둔 `KEY = 값`도 같은 규칙으로 본다
        let commented = trimmed.starts_with('#');
        let body_at = line.len() - trimmed.len() + usize::from(commented);
        let body = &line[body_at..];
        let Some(eq) = assignment(body) else {
            if !trimmed.is_empty() && (in_env_table || secret_word(trimmed)) {
                hide(trimmed, &mut out, &mut hidden);
            } else {
                out.push(line.to_string());
            }
            continue;
        };
        let key = body[..eq].trim();
        let value = body[eq + 1..].trim();
        let segments: Vec<String> =
            key.split('.').map(|k| k.trim().trim_matches('"').trim_matches('\'').to_string()).collect();
        let in_env = in_env_table || segments.iter().any(|s| s == "env");
        let secret = in_env
            || table.iter().chain(segments.iter()).any(|s| secret_word(s))
            || secret_word(value)
            || (value.starts_with('{') && value.contains("env"));
        let text_open = ["\"\"\"", "'''"].into_iter().find(|d| value.starts_with(d) && value[3..].find(d).is_none());
        let nested = depth(value);
        if text_open.is_some() || nested > 0 {
            cont = Some(if commented {
                Cont::Comment
            } else if let Some(d) = text_open {
                Cont::Text(d)
            } else {
                Cont::Nested(nested)
            });
        }
        if secret && !value.is_empty() {
            let raw = value.trim_matches('"').trim_matches('\'').trim();
            if !raw.is_empty() {
                hidden.push(raw.to_string());
            }
            out.push(format!("{}{} = {HIDDEN}", &line[..body_at], body[..eq].trim_end()));
        } else {
            out.push(line.to_string());
        }
    }
    hidden.retain(|h| h.chars().count() >= 3);
    (out, hidden)
}

/// 화면에 보일 오류 글: TOML 오류의 원문 조각(`|` 줄)을 빼고, 가린 값과 등록된 비밀을 지운다
pub(crate) fn clean_message(d: &Diag, hidden: &[String]) -> String {
    let text = d
        .message
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('|')
                && !l.split_once('|').is_some_and(|(n, _)| n.trim().parse::<u32>().is_ok())
        })
        .collect::<Vec<_>>()
        .join(" · ");
    // 큰따옴표 안 값(serde·검증 오류가 값을 그대로 적는다)은 모두 가린다. 필드 이름은 backtick이라 남는다
    let mut text = quoted_hidden(&text);
    for h in hidden {
        text = text.replace(h.as_str(), HIDDEN);
    }
    aaa_common::redact::text(&text)
}

fn quoted_hidden(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        out.push_str(&rest[..=start]);
        let after = &rest[start + 1..];
        // `\"`는 문자열 안 따옴표다
        let mut end = None;
        let mut escaped = false;
        for (i, c) in after.char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(i);
                    break;
                }
                _ => escaped = false,
            }
        }
        match end {
            Some(e) => {
                out.push_str(HIDDEN);
                out.push('"');
                rest = &after[e + 1..];
            }
            None => {
                out.push_str(HIDDEN);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

// ───────────── 웹 ─────────────

struct Safe {
    db: Db,
    port: u16,
    config_dir: PathBuf,
    errors: Mutex<ConfigErrors>,
    done: tokio::sync::watch::Sender<bool>,
}

/// 설정을 읽지 못한 host: 최소 웹을 열고 되돌리기·다시 읽기·종료를 기다린다. DB가 없거나(처음 설치) 웹을 끈 설정이면 원래 오류로 끝낸다
pub async fn run(
    errors: ConfigErrors,
    config_dir: PathBuf,
    state_root: PathBuf,
    ready: Option<tokio::sync::oneshot::Sender<()>>,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<SafeExit> {
    let db_path = state_root.join("host").join("host.db");
    if !db_path.is_file() {
        anyhow::bail!("설정을 읽지 못해 시작하지 않는다(처음 시작이라 안전 모드도 없다):\n{}", errors.render());
    }
    let db = Db::open(&db_path)?;
    let Some(addr) = listen_addr(&db, &config_dir) else {
        anyhow::bail!(
            "설정을 읽지 못해 시작하지 않는다(웹을 껐거나 이 판으로 정상 시작한 적이 없어 안전 모드가 열 주소가 없다):\n{}",
            errors.render()
        );
    };
    let listener = tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("안전 모드 웹 listen {addr}"))?;
    let port = listener.local_addr()?.port();
    tracing::error!(%addr, "설정을 읽지 못해 안전 모드로 웹만 연다:\n{}", errors.render());
    if let Some(ready) = ready {
        let _ = ready.send(());
    }
    let (done, mut reload) = tokio::sync::watch::channel(false);
    let safe = Arc::new(Safe { db, port, config_dir, errors: Mutex::new(errors), done });
    let serve = axum::serve(listener, router(safe.clone())).with_graceful_shutdown(async move {
        let _ = reload.wait_for(|v| *v).await;
    });
    tokio::select! {
        r = serve => {
            r?;
            Ok(if *safe.done.borrow() { SafeExit::Reload } else { SafeExit::Shutdown })
        }
        _ = shutdown => Ok(SafeExit::Shutdown),
    }
}

fn router(safe: Arc<Safe>) -> Router {
    Router::new()
        .route("/api/v1/safe/restore", post(restore_post))
        .route("/api/v1/safe/retry", post(retry_post))
        .route("/api/v1/me", get(unavailable))
        .fallback(page)
        .layer(middleware::from_fn_with_state(safe.clone(), guard))
        .with_state(safe)
}

/// `web::guard`와 같은 Host·Origin 판정(DNS rebinding·CSRF)과 보안 머리
async fn guard(State(safe): State<Arc<Safe>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let Some(expected) = crate::web_origin_in(safe.port, &safe.db, &crate::web::host_header(h)) else {
        return (StatusCode::MISDIRECTED_REQUEST, "알 수 없는 Host").into_response();
    };
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !origin.eq_ignore_ascii_case(&expected) {
            return (StatusCode::FORBIDDEN, "허용하지 않은 Origin").into_response();
        }
    }
    let mut res = next.run(req).await;
    let hs = res.headers_mut();
    hs.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"),
    );
    hs.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    hs.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // no-referrer면 브라우저가 form POST의 Origin을 `null`로 보낸다: 같은 origin에만 보낸다
    hs.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    hs.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    hs.insert("x-aaa-mode", HeaderValue::from_static(MARK));
    res
}

/// 열린 웹 앱의 `/api/*`: 503 `unavailable`. 새로 고치면 안전 모드 화면이 열린다
async fn unavailable() -> Response {
    let body = ErrorBody::new(
        ErrorCode::Unavailable,
        "host가 설정을 읽지 못해 안전 모드로 떠 있다. 이 주소를 새로 고치면 안전 모드 화면(오류 줄·되돌리기)이 열린다",
    );
    (StatusCode::SERVICE_UNAVAILABLE, Json(Outcome::<()>::Err(body))).into_response()
}

enum Who {
    Controller(String),
    Reader,
    Nobody,
}
fn who(safe: &Safe, h: &axum::http::HeaderMap) -> Who {
    let Some(token) = crate::web::cookie_token(h) else { return Who::Nobody };
    match safe.db.device_by_token_hash(&crate::token_hash(&token)).ok().flatten() {
        Some(d) if d.role == "reader" => Who::Reader,
        Some(d) => Who::Controller(d.id),
        None => Who::Nobody,
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn shell(body: &str) -> String {
    format!(
        "<!doctype html><html lang=ko><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
<title>aaa 안전 모드</title><style>\
body{{font:15px/1.5 -apple-system,system-ui,sans-serif;word-break:keep-all;margin:0;padding:16px;background:#f6f6f4;color:#1c1c1a}}\
main{{max-width:960px;margin:0 auto}}h1{{font-size:20px}}h2{{font-size:16px;margin-top:24px}}\
.note{{background:#fff4d6;border:1px solid #e0c060;border-radius:8px;padding:10px 12px}}\
ul.errors li{{margin:4px 0}}pre{{background:#fff;border:1px solid #ddd;border-radius:8px;padding:8px 0;overflow-x:auto;font:13px/1.45 ui-monospace,Menlo,monospace}}\
pre span{{display:block;padding:0 12px;white-space:pre}}pre span.err{{background:#ffe1e1;box-shadow:inset 3px 0 #d33}}pre i{{color:#999;font-style:normal;user-select:none}}\
form{{display:inline-block;margin:8px 8px 0 0}}button{{font:inherit;padding:8px 14px;border-radius:8px;border:1px solid #888;background:#fff}}\
button.primary{{background:#1c1c1a;color:#fff;border-color:#1c1c1a}}.muted{{color:#666}}\
@media (prefers-color-scheme:dark){{body{{background:#1b1b1a;color:#e8e8e4}}pre{{background:#111;border-color:#333}}.note{{background:#3a3014;border-color:#6b5a20}}\
pre span.err{{background:#4a1d1d}}button{{background:#2a2a28;color:#e8e8e4;border-color:#555}}button.primary{{background:#e8e8e4;color:#1b1b1a}}.muted{{color:#999}}}}\
</style><main>{body}</main></html>"
    )
}

fn page_response(body: &str) -> Response {
    // 503: service worker가 셸로 두지 않는다(정상 host로 돌아온 뒤 오프라인 셸이 이 화면이 되지 않게)
    (StatusCode::SERVICE_UNAVAILABLE, Html(shell(body))).into_response()
}

async fn page(State(safe): State<Arc<Safe>>, req: Request) -> Response {
    if req.uri().path().starts_with("/api/") {
        return unavailable().await;
    }
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    page_response(&body_for(&safe, &who(&safe, req.headers()), None))
}

fn body_for(safe: &Safe, who: &Who, notice: Option<&str>) -> String {
    let mut b = String::from(
        "<h1>aaa 안전 모드</h1><p class=note>host가 설정을 읽지 못해 이 화면만 열었습니다. 세션·thread·agent 연결은 설정을 고친 뒤 다시 시작할 때 이어집니다.</p>",
    );
    if let Some(n) = notice {
        b.push_str(&format!("<p class=note role=status>{}</p>", esc(n)));
    }
    match who {
        Who::Nobody => {
            b.push_str("<p>이 브라우저는 짝지어진 기기가 아닙니다. 안전 모드에서는 새로 짝지을 수 없습니다. host Mac 터미널에서 <code>aaa config check</code>로 오류를 보고 설정을 고친 뒤 aaa를 다시 시작합니다.</p>");
        }
        Who::Reader => {
            b.push_str("<p>보기 전용 기기에는 설정 원문과 되돌리기를 보이지 않습니다. 조작 기기나 host Mac 터미널(<code>aaa config check</code>)에서 고칩니다.</p>");
        }
        Who::Controller(_) => {
            let errors = safe.errors.lock().unwrap();
            let mut hidden = Vec::new();
            let mut files = String::new();
            for name in FILES {
                let path = safe.config_dir.join(name);
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let (lines, h) = mask(&text);
                hidden.extend(h);
                let bad: Vec<usize> =
                    errors.0.iter().filter(|d| d.file == path && d.line > 0).map(|d| d.line).collect();
                // 이름만 보인다(홈 경로는 화면에 두지 않는다)
                files.push_str(&format!("<h2>{name}</h2><pre data-file=\"{name}\">"));
                for (i, line) in lines.iter().enumerate() {
                    let n = i + 1;
                    let class = if bad.contains(&n) { " class=err" } else { "" };
                    files.push_str(&format!("<span{class} data-line={n}><i>{n:>4} </i>{}</span>", esc(line)));
                }
                files.push_str("</pre>");
            }
            b.push_str("<h2>오류</h2><ul class=errors data-testid=safe-errors>");
            for d in errors.0.iter().filter(|d| d.severity == aaa_config::Severity::Error) {
                let at = if d.line > 0 {
                    format!("{}:{}:{}", file_name(&d.file), d.line, d.col)
                } else {
                    file_name(&d.file)
                };
                b.push_str(&format!("<li><code>{}</code> {}</li>", esc(&at), esc(&clean_message(d, &hidden))));
            }
            b.push_str("</ul>");
            match kept(&safe.config_dir) {
                Some(names) => b.push_str(&format!(
                    "<form method=post action=/api/v1/safe/restore><button class=primary type=submit>마지막 정상 설정으로 되돌리기</button></form>\
<p class=muted>지금 파일은 옆에 <code>&lt;이름&gt;.broken-&lt;시각&gt;</code>으로 남기고, 마지막으로 정상 시작한 설정({})으로 바꾼 뒤 다시 시작합니다.</p>",
                    esc(&if names.is_empty() { "설정 파일 없음(기본값)".to_string() } else { names.join(", ") })
                )),
                None => b.push_str("<p class=muted>보관한 정상 설정이 없어 되돌릴 수 없습니다. host Mac에서 파일을 고친 뒤 「다시 읽기」를 누릅니다.</p>"),
            }
            b.push_str("<form method=post action=/api/v1/safe/retry><button type=submit>다시 읽기</button></form>");
            b.push_str("<p class=muted>원문의 env 값, 비밀처럼 보이는 값, 여러 줄 값의 이어지는 줄은 가렸습니다(«가림»). 고친 파일은 host Mac에서 다시 읽기로 확인합니다.</p>");
            b.push_str(&files);
        }
    }
    b
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn forbidden(safe: &Safe, who: &Who) -> Response {
    (StatusCode::FORBIDDEN, Html(shell(&body_for(safe, who, Some("조작 기기만 되돌리거나 다시 읽습니다.")))))
        .into_response()
}

/// 설정을 다시 읽어 되면 안전 모드를 끝낸다(부른 쪽이 정상 host로 다시 시작한다). 안 되면 새 오류로 화면을 다시 그린다
fn reload(safe: &Safe, who: &Who, done_note: &str) -> Response {
    match aaa_config::load_host(&safe.config_dir) {
        Ok(_) => {
            let _ = safe.done.send(true);
            let body = format!(
                "<h1>aaa 안전 모드</h1><p class=note role=status data-testid=safe-reloading>{}</p><p><a href=/>다시 열기</a></p>",
                esc(done_note)
            );
            // 이 답 뒤 정상 host가 같은 주소에 뜬다. 화면은 3초 뒤 다시 연다(script 없이)
            let mut res = Html(shell(&body)).into_response();
            res.headers_mut().insert(header::REFRESH, HeaderValue::from_static("3; url=/"));
            res
        }
        Err(e) => {
            *safe.errors.lock().unwrap() = e;
            page_response(&body_for(safe, who, Some("아직 설정을 읽지 못한다. 오류를 다시 보인다.")))
        }
    }
}

async fn restore_post(State(safe): State<Arc<Safe>>, headers: axum::http::HeaderMap) -> Response {
    let who = who(&safe, &headers);
    let Who::Controller(id) = &who else { return forbidden(&safe, &who) };
    match restore(&safe.config_dir, aaa_common::util::now_ms()) {
        Ok(backups) => {
            let names: Vec<String> = backups.iter().map(|b| file_name(b)).collect();
            safe.db.audit(&format!("device:{id}"), "safe_restore", &names.join(","));
            reload(
                &safe,
                &who,
                &format!(
                    "마지막 정상 설정으로 되돌렸습니다{}. 정상 host로 다시 시작합니다.",
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!("(지금 파일은 {}로 남김)", names.join(", "))
                    }
                ),
            )
        }
        Err(e) => page_response(&body_for(&safe, &who, Some(&format!("되돌리지 못했다: {e:#}")))),
    }
}

async fn retry_post(State(safe): State<Arc<Safe>>, headers: axum::http::HeaderMap) -> Response {
    let who = who(&safe, &headers);
    let Who::Controller(id) = &who else { return forbidden(&safe, &who) };
    safe.db.audit(&format!("device:{id}"), "safe_retry", "");
    reload(&safe, &who, "설정을 읽었습니다. 정상 host로 다시 시작합니다.")
}

#[cfg(test)]
mod tests;
