use super::*;
use std::time::Instant;
use tower::ServiceExt;

const PUBLIC: &str = "mac-a.example.ts.net:8443";
const LOOP: &str = "127.0.0.1:7000";

struct Lab(Arc<Host>, std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(
            std::fs::remove_dir_all(&self.1),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
    }
}

fn lab(public_origin: &str) -> Lab {
    let dir = std::env::temp_dir().join(format!("aaa-guard-test-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = dir.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, dir.clone()).unwrap();
    h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
    h.db.set_meta("publish_origin", public_origin).unwrap();
    Lab(h, dir)
}

#[tokio::test]
async fn terminal_grid_is_ready_while_metadata_db_is_locked() {
    let l = lab("https://mac-a.example.ts.net:8443");
    l.0.on_session(
        "machine",
        aaa_proto::SessionInfo {
            responsibility: None,
            id: "pane".into(),
            name: "pane".into(),
            argv: vec![],
            cwd: None,
            pid: None,
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 100,
            rows: 30,
            state: aaa_proto::SessionState::Running,
            role: None,
            label: None,
            harness: None,
        },
        false,
    );
    let (holding, started) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let host = l.0.clone();
    let holder = tokio::task::spawn_blocking(move || {
        host.db.image_transaction(|_| {
            holding.send(()).unwrap();
            released.recv()?;
            Ok(())
        })
    });
    started.await.unwrap();
    let host = l.0.clone();
    let mut reader = tokio::task::spawn_blocking(move || terminal_state(&host, "pane", DeviceRole::Controller, true));
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut reader).await;
    release.send(()).unwrap();
    holder.await.unwrap().unwrap();
    let Ok(result) = result else {
        drop(reader.await.unwrap());
        panic!("터미널 상태가 DB 잠금에 막혔다");
    };
    let (access, state) = result.unwrap().unwrap();
    assert_eq!(state, TermServerMsg::Grid { cols: 100, rows: 30, lease: None });
    assert!(access.readonly(DeviceRole::Controller, false));
}

fn token(h: &Host) -> String {
    let (code, _) = h.mint_pairing(DeviceRole::Controller, Instant::now());
    h.redeem_pairing(&code, "").unwrap().0
}

#[tokio::test]
async fn answer_timing_consumes_only_the_issuing_devices_receipt() {
    let lab = lab("https://mac-a.example.ts.net:8443");
    let owner = token(&lab.0);
    let other = token(&lab.0);
    let post = |token: &str, path: &'static str, body: serde_json::Value| {
        let host = lab.0.clone();
        let cookie = format!("{}={token}", cookie_name(false));
        async move {
            router(host)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header(header::HOST, LOOP)
                        .header(header::ORIGIN, "http://127.0.0.1:7000")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::COOKIE, cookie)
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
        }
    };
    let response = post(
        &owner,
        "/api/v1/call",
        serde_json::json!({"op":"thread_answer", "project":"demo", "thread":"t-5001", "report_hash":"old",
            "picks": [0], "text":"private answer specimen"}),
    )
    .await;
    let trace = response.headers()[operation_log::ANSWER_TRACE].to_str().unwrap().to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["err"].is_object());
    let timing = serde_json::json!({"trace":trace, "response_ms":501, "render_ms":10});
    let wrong = post(&other, "/api/v1/diag/answer", timing.clone()).await;
    assert_eq!(wrong.status(), StatusCode::NO_CONTENT);
    let invalid =
        post(&owner, "/api/v1/diag/answer", serde_json::json!({"trace":trace, "response_ms":120001, "render_ms":10}))
            .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let unknown = post(
        &owner,
        "/api/v1/diag/answer",
        serde_json::json!({"trace":trace, "response_ms":501, "render_ms":10, "text":"private"}),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let device = lab.0.device_role(&owner).unwrap().0;
    assert!(lab.0.answer_receipts.lock().unwrap().contains(&device, &trace));
    assert_eq!(post(&owner, "/api/v1/diag/answer", timing.clone()).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(post(&owner, "/api/v1/diag/answer", timing).await.status(), StatusCode::NO_CONTENT);
    assert!(!lab.0.answer_receipts.lock().unwrap().contains(&device, &trace));
}

async fn ask(h: &Arc<Host>, method: &str, path: &str, host: &str, more: &[(&str, &str)]) -> Response {
    let mut req = Request::builder().method(method).uri(path).header(header::HOST, host);
    for (k, v) in more {
        req = req.header(*k, *v);
    }
    router(h.clone()).oneshot(req.body(Body::empty()).unwrap()).await.unwrap()
}

/// 조작 기기 passkey(관리 GUI W7): 공개 주소의 조작 기기는 passkey 없이 승인하지 못하고, serve를 거친 요청은 Host를
/// loopback으로 적어도 host Mac 화면으로 보지 않는다(serve는 받은 Host를 넘긴다: 훔친 cookie로 UV를 건너뛰는 길)
#[tokio::test]
async fn only_a_direct_loopback_screen_approves_without_a_passkey() {
    let l = lab("https://mac-a.example.ts.net:8443");
    let t = token(&l.0);
    let call = |host: &'static str, more: Vec<(&'static str, String)>| {
        let h = l.0.clone();
        async move {
            let origin = if host == LOOP { "http://127.0.0.1:7000" } else { "https://mac-a.example.ts.net:8443" };
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/v1/call")
                .header(header::HOST, host)
                .header(header::ORIGIN, origin)
                .header(header::CONTENT_TYPE, "application/json");
            for (k, v) in more {
                req = req.header(k, v);
            }
            let body = Body::from(r#"{"op":"web_pair","role":"controller"}"#);
            let res = router(h).oneshot(req.body(body).unwrap()).await.unwrap();
            let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        }
    };
    let direct = call(LOOP, vec![("cookie", format!("{}={t}", cookie_name(false)))]).await;
    assert_eq!(direct["ok"]["kind"], "pair_code", "{direct}");
    let public = call(PUBLIC, vec![("cookie", format!("{}={t}", cookie_name(true)))]).await;
    assert_eq!(public["err"]["code"], "passkey_required", "{public}");
    for (k, v, secure) in [("x-forwarded-proto", "https", true), ("x-forwarded-for", "203.0.113.9", false)] {
        let cookie = format!("{}={t}", cookie_name(secure));
        let bent = call(LOOP, vec![("cookie", cookie), (k, v.to_string())]).await;
        assert_eq!(bent["err"]["code"], "passkey_required", "{k}: {bent}");
    }
}

async fn code(h: &Arc<Host>, method: &str, path: &str, host: &str, more: &[(&str, &str)]) -> StatusCode {
    ask(h, method, path, host, more).await.status()
}

#[tokio::test]
async fn attachment_api_errors_are_utf8_and_caps_expire_with_the_device() {
    let l = lab("https://mac-a.example.ts.net:8443");
    let t = token(&l.0);
    let (id, _) = l.0.device_role(&t).unwrap();
    for path in [
        "/api/v1/projects/demo/questions/t-0001/hash/library/a.mp4",
        "/api/v1/projects/demo/requests/item/hash/library/a.mp4",
        "/api/v1/projects/demo/asks/item/library/a.mp4",
    ] {
        let response = ask(&l.0, "GET", path, LOOP, &[]).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json; charset=utf-8");
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["err"]["message"], "pairing이 필요하다");
    }
    let original = Some("/api/v1/projects/demo/asks/item/".to_string());
    let mut base = original.clone();
    attachment_base(&l.0, &id, &mut base);
    assert!(base.as_deref().unwrap().starts_with("/v/"));
    let mut same = original;
    attachment_base(&l.0, &id, &mut same);
    assert_eq!(base, same, "목록을 갱신해도 같은 기기·사본은 같은 경로다");
    let cap = base.unwrap().trim_start_matches("/v/").trim_end_matches('/').to_string();
    assert_eq!(l.0.library_resolve("demo", &cap).unwrap_err().code, ErrorCode::BadRequest);
    // 유효 cap은 대상 파일 확인까지 가고, 만료되면 대상보다 먼저 권한 오류다.
    assert_ne!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
    l.0.view_caps.lock().unwrap().get_mut(&cap).unwrap().until = Instant::now();
    assert_eq!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
    let mut base = Some("/api/v1/projects/demo/asks/item/".to_string());
    attachment_base(&l.0, &id, &mut base);
    let cap = base.unwrap().trim_start_matches("/v/").trim_end_matches('/').to_string();
    l.0.revoke_device(&id).unwrap();
    assert_eq!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn host_must_be_this_host_with_its_port() {
    let l = lab("https://mac-a.example.ts.net:8443");
    assert_ne!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    assert_ne!(code(&l.0, "GET", "/", "MAC-A.example.ts.net:8443", &[]).await, StatusCode::MISDIRECTED_REQUEST);
    assert_ne!(code(&l.0, "GET", "/", LOOP, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    // 같은 이름의 다른 포트·포트 없음(443의 다른 서비스), localhost, 다른 loopback 포트, 다른 이름
    for other in
        ["mac-a.example.ts.net:9999", "mac-a.example.ts.net", "localhost:7000", "127.0.0.1:7001", "evil.example:8443"]
    {
        assert_eq!(code(&l.0, "GET", "/", other, &[]).await, StatusCode::MISDIRECTED_REQUEST, "{other}");
    }
}

#[tokio::test]
async fn origin_must_match_the_whole_origin() {
    let l = lab("https://mac-a.example.ts.net:8443");
    let ok = [("origin", "https://mac-a.example.ts.net:8443")];
    assert_eq!(code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &ok).await, StatusCode::NO_CONTENT);
    assert_eq!(
        code(&l.0, "DELETE", "/api/v1/me", LOOP, &[("origin", "http://127.0.0.1:7000")]).await,
        StatusCode::NO_CONTENT
    );
    for bad in [
        "https://mac-a.example.ts.net:9999",
        "https://mac-a.example.ts.net",
        "http://mac-a.example.ts.net:8443",
        "http://127.0.0.1:7000",
        "http://localhost:7000",
        "null",
        "",
    ] {
        assert_eq!(
            code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &[("origin", bad)]).await,
            StatusCode::FORBIDDEN,
            "{bad:?}"
        );
    }
    assert_eq!(code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &[]).await, StatusCode::FORBIDDEN);
    // loopback 주소에는 그 주소의 origin만
    let other = [("origin", "https://mac-a.example.ts.net:8443")];
    assert_eq!(code(&l.0, "DELETE", "/api/v1/me", LOOP, &other).await, StatusCode::FORBIDDEN);
    // WebSocket upgrade도 GET이지만 Origin을 본다(cookie가 있어도)
    let t = format!("__Host-aaa={}", token(&l.0));
    let ws = |origin| {
        [
            ("origin", origin),
            ("connection", "upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-version", "13"),
            ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ("cookie", t.as_str()),
        ]
    };
    // 다른 기기 TUI의 Front API(`/api/v1/front`, plan §6.2)도 같은 규칙이다
    for path in ["/api/v1/events", "/api/v1/front"] {
        for bad in ["https://mac-a.example.ts.net:9999", "https://mac-a.example.ts.net"] {
            assert_eq!(code(&l.0, "GET", path, PUBLIC, &ws(bad)).await, StatusCode::FORBIDDEN, "{path} {bad}");
        }
        assert_ne!(
            code(&l.0, "GET", path, PUBLIC, &ws("https://mac-a.example.ts.net:8443")).await,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_default_https_port_has_no_port_in_host_or_origin() {
    let l = lab("https://mac-a.example.ts.net");
    for host in ["mac-a.example.ts.net", "mac-a.example.ts.net:443"] {
        let ok = [("origin", "https://mac-a.example.ts.net")];
        assert_eq!(code(&l.0, "DELETE", "/api/v1/me", host, &ok).await, StatusCode::NO_CONTENT, "{host}");
        let bad = [("origin", "https://mac-a.example.ts.net:8443")];
        assert_eq!(code(&l.0, "DELETE", "/api/v1/me", host, &bad).await, StatusCode::FORBIDDEN, "{host}");
    }
    assert_eq!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn not_published_means_loopback_only() {
    let l = lab("https://mac-a.example.ts.net:8443");
    l.0.db.del_meta("publish_origin").unwrap();
    // 이름만 알던 옛 기록(publish_dns)으로는 열리지 않는다
    l.0.db.set_meta("publish_dns", "mac-a.example.ts.net").unwrap();
    assert_eq!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    assert_ne!(code(&l.0, "GET", "/", LOOP, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    // serve 상태를 아직 못 읽었어도 aaa가 만든 공개(포트를 안다)는 그 포트로만 열린다
    l.0.db.set_meta("publish_port", "8443").unwrap();
    assert_ne!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(code(&l.0, "GET", "/", "mac-a.example.ts.net", &[]).await, StatusCode::MISDIRECTED_REQUEST);
}

/// 같은 hostname의 두 인스턴스가 서로의 로그인 cookie를 덮지 않는다. 기본 인스턴스는 지금 이름 그대로다
#[test]
fn instance_cookie_names() {
    assert_eq!(cookie_name_in(None, true), "__Host-aaa");
    assert_eq!(cookie_name_in(None, false), "aaa");
    assert_eq!(cookie_name_in(Some("work"), true), "__Host-aaa-work");
    assert_eq!(cookie_name_in(Some("work"), false), "aaa-work");
}

#[tokio::test]
async fn cookies_skip_junk_and_https_takes_only_the_host_prefix() {
    let l = lab("https://mac-a.example.ts.net:8443");
    let t = token(&l.0);
    let me = |cookie: String| {
        let h = l.0.clone();
        let host = if cookie.contains("__Host-") { PUBLIC } else { LOOP };
        async move { code(&h, "GET", "/api/v1/me", host, &[("cookie", cookie.as_str())]).await }
    };
    // 같은 host의 다른 서비스가 심은 `=` 없는 항목·빈 이름이 앞에 있어도
    assert_eq!(me(format!("foo; =x; __Host-aaa={t}")).await, StatusCode::OK);
    assert_eq!(me(format!("junk; aaa={t}")).await, StatusCode::OK);
    // HTTPS에서 prefix 없는 cookie는 장치 cookie가 아니다(다른 포트·하위 경로가 심을 수 있다)
    assert_eq!(
        code(&l.0, "GET", "/api/v1/me", PUBLIC, &[("cookie", &format!("aaa={t}"))]).await,
        StatusCode::UNAUTHORIZED
    );
    let planted = format!("aaa=junk; __Host-aaa={t}");
    assert_eq!(code(&l.0, "GET", "/api/v1/me", PUBLIC, &[("cookie", planted.as_str())]).await, StatusCode::OK);
}

/// 큰 API 답만, 받는다고 한 브라우저에만 gzip으로 보낸다. 푼 글은 원문과 같고 저장 금지·UTF-8 표지는 그대로다(t-1767)
#[tokio::test]
async fn large_api_answers_are_gzipped_only_when_accepted() {
    use std::io::Read as _;
    let l = lab("https://mac-a.example.ts.net:8443");
    let t = token(&l.0);
    // 고정 40개(51바이트씩)로 `preferences` 답을 문턱(1024) 위인 2000바이트 남짓으로 올린다
    let pins: Vec<String> = (0..40).map(|n| format!("mac-a:{n:02}-{}", "가".repeat(14))).collect();
    let call = |body: String, accept: Option<&'static str>| {
        let h = l.0.clone();
        let cookie = format!("{}={t}", cookie_name(false));
        async move {
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/v1/call")
                .header(header::HOST, LOOP)
                .header(header::ORIGIN, "http://127.0.0.1:7000")
                .header(header::CONTENT_TYPE, "application/json")
                .header("cookie", cookie);
            if let Some(accept) = accept {
                req = req.header(header::ACCEPT_ENCODING, accept);
            }
            let res = router(h).oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
            let headers = res.headers().clone();
            (headers, axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap())
        }
    };
    let set = serde_json::json!({ "op": "preferences_set", "settings": {}, "pins": pins }).to_string();
    let (_, saved) = call(set, None).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&saved).unwrap()["ok"]["kind"], "preferences");
    let read = || r#"{"op":"preferences"}"#.to_string();
    let (plain_headers, plain) = call(read(), None).await;
    assert!(plain.len() >= 2000 && !plain_headers.contains_key(header::CONTENT_ENCODING), "{}", plain.len());
    assert_eq!(plain_headers[header::VARY], "Accept-Encoding", "큰 답은 원문으로 보내도 협상 결과다");
    let (refused_headers, refused) = call(read(), Some("gzip;q=0, br")).await;
    assert!(!refused_headers.contains_key(header::CONTENT_ENCODING));
    assert_eq!(refused, plain);
    let (headers, packed) = call(read(), Some("gzip, deflate, br")).await;
    assert_eq!(headers[header::CONTENT_ENCODING], "gzip");
    assert_eq!(headers[header::VARY], "Accept-Encoding");
    assert_eq!(headers[header::CONTENT_TYPE], "application/json; charset=utf-8");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(packed.len() < plain.len() / 4, "{} / {}", packed.len(), plain.len());
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(&packed[..]).read_to_end(&mut text).unwrap();
    assert_eq!(text, plain);
    // 작은 답은 받는다고 해도 그대로 보낸다
    let (small_headers, small) = call(r#"{"op":"questions","all":false}"#.to_string(), Some("gzip")).await;
    assert!(small.len() < 1024 && !small_headers.contains_key(header::CONTENT_ENCODING));
    assert!(!small_headers.contains_key(header::VARY));
    assert_eq!(small_headers[header::CONTENT_TYPE], "application/json; charset=utf-8");
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&small).unwrap()["ok"]["kind"], "questions");
}

#[tokio::test]
async fn api_answers_are_not_stored() {
    let l = lab("https://mac-a.example.ts.net:8443");
    for path in ["/api/v1/me", "/api/v1/usage", "/api/v1/push", "/api/v1/app-badge"] {
        let res = ask(&l.0, "GET", path, LOOP, &[]).await;
        assert_eq!(res.headers().get(header::CACHE_CONTROL).map(|v| v.to_str().unwrap()), Some("no-store"), "{path}");
    }
}
