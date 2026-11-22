//! Web Push 구독·시험과 사용량·진단 HTTP 경로.

use super::*;

#[derive(Serialize)]
pub(super) struct PushInfo {
    /// host 설정에서 Web Push가 켜져 있다
    pub(super) enabled: bool,
    /// `applicationServerKey`(VAPID 공개키, base64url)
    pub(super) key: Option<String>,
    /// 이 장치의 구독 수
    pub(super) subscribed: usize,
    /// 지금 쓰는 웹 화면 수(보이고 focus)
    pub(super) in_use: usize,
    /// 쓰는 기기(앞 7자): 마지막으로 누르거나 치거나 화면을 켠 기기. 그 기기가 쓰는 화면인 동안 Web Push를 보내지 않는다(plan §8.105)
    pub(super) user: Option<String>,
    /// 이 장치가 쓰는 기기다
    pub(super) you: bool,
    /// 알리는 inbox 종류(`notify.kinds`). 웹의 앱 안 알림·소리·탭 알림도 이 종류에만 띄운다(push와 같은 사건)
    pub(super) kinds: Vec<String>,
    /// 이 장치 구독이 고른 알림 종류(차이 목록 N3, 구독이 없으면 없음)
    pub(super) alerts: Option<crate::db::Alerts>,
    /// 이 장치에 최근 push로 보낸 항목(오래된 것부터). 깨어난 페이지는 이것으로 이미 받은 알림을 다시 띄우지 않는다(plan §8.141)
    pub(super) pushed: Vec<crate::push::PushedItem>,
}

/// Web Push(§8.26): 키와 이 장치의 구독 상태. 보기 권한 장치도 알림은 받는다.
pub(super) async fn push_info(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let mine = host.db.push_list(Some(&id)).unwrap_or_default();
    let subscribed = mine.len();
    let alerts = mine.first().and_then(|s| s.alerts);
    let key = host.vapid.as_ref().map(|v| v.public_b64());
    let in_use = host.in_use.load(std::sync::atomic::Ordering::SeqCst);
    let (user, you) = {
        let v = host.viewing.lock().unwrap();
        (v.user(), v.is_user(&id))
    };
    let kinds = host.cfg.notify.item_kinds();
    let pushed = host.pushed.lock().unwrap().of(&id);
    Json(Outcome::Ok(PushInfo { enabled: key.is_some(), key, subscribed, in_use, user, you, kinds, alerts, pushed }))
        .into_response()
}

#[derive(Deserialize)]
pub(super) struct PushKeys {
    pub(super) p256dh: String,
    pub(super) auth: String,
}

/// 브라우저 `PushSubscription.toJSON()` 모양
#[derive(Deserialize)]
pub(super) struct PushSubBody {
    pub(super) endpoint: String,
    pub(super) keys: PushKeys,
    /// 이 기기가 받을 알림(차이 목록 N3). 없으면 전에 고른 것을 둔다
    #[serde(default)]
    pub(super) alerts: Option<crate::db::Alerts>,
}

pub(super) async fn push_subscribe(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(b): Json<PushSubBody>,
) -> Response {
    let _gate = host.publish_gate.lock().await;
    // 입구 검사를 통과한 직후 포트가 바뀐 요청도 옛 origin의 구독을 다시 넣지 못한다.
    let Some(origin) = host.web_origin(&host_header(&headers)) else {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    };
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    if host.vapid.is_none() {
        return bad_request("host 설정에서 Web Push가 꺼져 있다(notify.web_push)");
    }
    if let Err(e) = crate::push::check_endpoint(&b.endpoint, &host.cfg.notify.push_hosts)
        .and_then(|_| crate::push::check_keys(&b.keys.p256dh, &b.keys.auth))
    {
        return bad_request(e);
    }
    let sub = crate::db::PushSub {
        endpoint: b.endpoint,
        p256dh: b.keys.p256dh,
        auth: b.keys.auth,
        device: id.clone(),
        alerts: b.alerts,
    };
    match host.db.push_put_at(&sub, Some(&origin)) {
        Ok(true) => {}
        Ok(false) => {
            host.db.audit(&format!("device:{id}"), "push_subscribe", "다른 기기의 구독이라 거절");
            return (
                StatusCode::FORBIDDEN,
                Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "다른 기기의 구독이다"))),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Internal, e.to_string()))),
            )
                .into_response();
        }
    }
    // endpoint는 남기지 않는다(그 기기에 보내는 열쇠다)
    host.db.audit(&format!("device:{id}"), "push_subscribe", "");
    Json(Outcome::Ok(serde_json::json!({ "subscribed": true }))).into_response()
}

#[derive(Deserialize)]
pub(super) struct PushEndpoint {
    pub(super) endpoint: String,
}

pub(super) async fn push_unsubscribe(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(b): Json<PushEndpoint>,
) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let removed = host.db.push_del(&b.endpoint, Some(&id)).unwrap_or(false);
    if removed {
        host.db.audit(&format!("device:{id}"), "push_unsubscribe", "");
    }
    Json(Outcome::Ok(serde_json::json!({ "removed": removed }))).into_response()
}

#[derive(Deserialize, Default)]
pub(super) struct PushTestBody {
    /// 이 구독 하나에만(herdr-web-ui `sendTestPush(endpoint)`). 없으면 이 장치의 모든 구독
    pub(super) endpoint: Option<String>,
}

/// 이 장치의 구독에 시험 알림을 보낸다. `endpoint`를 주면 그 구독 하나에 보내고, 없거나(`subscription_not_found`)
/// push 서비스가 버렸으면(`subscription_gone`, 지운다) 오류로 답한다. 웹은 그 말로 「알림 다시 켜기」를 권한다.
pub(super) async fn push_test(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    body: Option<Json<PushTestBody>>,
) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let only = body.and_then(|Json(b)| b.endpoint);
    let mut subs = host.db.push_list(Some(&id)).unwrap_or_default();
    if let Some(e) = &only {
        subs.retain(|s| &s.endpoint == e);
        if subs.is_empty() {
            let err = ErrorBody::new(ErrorCode::SubscriptionNotFound, "이 기기의 push 구독이 없다");
            return (StatusCode::NOT_FOUND, Json(Outcome::<()>::Err(err))).into_response();
        }
    }
    let body =
        serde_json::json!({ "title": "aaa", "body": "알림이 켜졌다", "url": "/", "tag": "aaa-test" }).to_string();
    let (mut sent, mut failed) = (0, Vec::new());
    for s in &subs {
        match host.push_one(s, body.as_bytes()).await {
            crate::push::Sent::Ok => sent += 1,
            crate::push::Sent::Gone => failed.push("브라우저가 구독을 버렸다(지웠다)".to_string()),
            crate::push::Sent::Failed(e) => failed.push(e),
        }
    }
    // 시험 알림은 사람이 켜거나 「테스트 보내기」를 누를 때만 간다(쓰는 화면이 있어도 보낸다, 참고 server/push.ts sendTest).
    // 휴대폰에 뜬 배너가 어디서 왔는지 가릴 수 있게 남긴다(사용자 화면 10-18 00:27, plan §8.82)
    host.db.audit(&format!("device:{id}"), "push_test", &format!("sent {sent} failed {}", failed.len()));
    if only.is_some() && sent == 0 {
        let gone = failed.first().is_some_and(|f| f.starts_with("브라우저가 구독을 버렸다"));
        let err = if gone {
            ErrorBody::new(ErrorCode::SubscriptionGone, "push 서비스가 이 기기의 구독을 버렸다(지웠다)")
        } else {
            ErrorBody::new(ErrorCode::Unavailable, failed.join("; "))
        };
        return (StatusCode::BAD_GATEWAY, Json(Outcome::<()>::Err(err))).into_response();
    }
    Json(Outcome::Ok(serde_json::json!({ "sent": sent, "failed": failed }))).into_response()
}

#[derive(Deserialize)]
pub(super) struct UsageQuery {
    pub(super) refresh: Option<String>,
}

/// 이 host 머신의 CLI가 로그인한 구독의 한도(`usage.rs`). 보기 권한(reader) 기기도 본다(참고: 들어온 client 누구나).
/// 계정 이름(email)이 들어 있으니 저장하지 않게 한다.
pub(super) async fn usage_report(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<UsageQuery>,
) -> Response {
    let Some((_, role)) = device(&host, &headers) else { return unauthorized() };
    let mut report = host.sidebar_usage(q.refresh.as_deref() == Some("1")).await;
    if role == DeviceRole::Reader {
        for usage in &mut report.providers {
            if usage.key.starts_with("claude:registered:") || usage.key.starts_with("codex:registered:") {
                usage.plan = None;
            }
        }
    }
    let mut res = Json(Outcome::Ok(report)).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// 등록 계정의 창별 사용량. 메일·원문 응답·인증 경로는 이 응답에 없다.
pub(super) async fn account_quota_report(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<UsageQuery>,
) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    let mut res = Json(Outcome::from(host.quota_report(q.refresh.as_deref() == Some("1")).await)).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// 입력 사건 기록 하나의 상한과 남기는 개수
pub(super) const DIAG_MAX: usize = 1 << 20;

pub(super) const DIAG_KEEP: usize = 20;

/// 실기 입력 사건 기록(web/src/inputTrace.ts)을 host 상태 폴더 `diag/`에 남긴다. iPad 화면 키보드 같은 OS 입력기의
/// 사건 순서는 headless로 만들 수 없어 실기에서 모아 E2E 재생으로 옮긴다(herdr-web-ui terminal-ime-diagnostic과 같은 목적).
/// 친 글 원문이 들어 있어 조작 권한 기기만 쓰고, 파일은 사용자 전용(0600)이다. `application/json`만 받는다: 다른 사이트의
/// 폼이 보낼 수 있는 `text/plain` 단순 요청으로는 쓸 수 없다(보안 감사 t-0340 A6·B8, herdr-web-ui는 전용 헤더를 요구한다)
pub(super) async fn diag_input(State(host): State<Arc<Host>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    if !role.can_control() {
        return (
            StatusCode::FORBIDDEN,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 조작할 수 없다"))),
        )
            .into_response();
    }
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| {
            v.to_str()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .is_some_and(|v| v.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json")));
    if !json {
        return bad_request("content-type이 application/json이 아니다");
    }
    if body.len() > DIAG_MAX {
        return bad_request(format!("기록은 {}KiB까지", DIAG_MAX >> 10));
    }
    if serde_json::from_slice::<serde_json::Value>(&body)
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_err()
    {
        return bad_request("JSON이 아니다");
    }
    let dir = host.state.join("diag");
    let name = format!("input-{}.json", aaa_common::util::now_ms());
    let saved = std::fs::create_dir_all(&dir).and_then(|_| aaa_common::util::write_private(&dir.join(&name), &body));
    if let Err(e) = saved {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Internal, e.to_string()))),
        )
            .into_response();
    }
    // 오래된 것부터 지워 DIAG_KEEP개만 둔다(이름이 시각 순이다)
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("input-") && n.ends_with(".json"))
            .collect();
        names.sort();
        let extra = names.len().saturating_sub(DIAG_KEEP);
        for n in &names[..extra] {
            aaa_common::diagnostic::ignored(
                std::fs::remove_file(dir.join(n)),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
        }
    }
    host.db.audit(&format!("device:{id}"), "diag_input", &name);
    Json(Outcome::Ok(serde_json::json!({ "saved": name }))).into_response()
}
