//! 보고서 하나의 host 사본만 읽는 30일 링크. 기기 token과 별개이며 비밀 경로를 DB·로그에 쓰지 않는다.
use std::path::Path;

use aaa_proto::{ErrorBody, ErrorCode, InboxItem, LinkView};
use anyhow::{Context, Result, bail};
use ring::hmac;

use crate::db::LibraryLinkRow;
use crate::{Host, err};

const DAY_MS: u64 = 86_400_000;
pub(crate) const DEFAULT_DAYS: u32 = 30;

pub(crate) struct SigningKey(hmac::Key);

impl SigningKey {
    pub(crate) fn open(state: &Path) -> Result<Self> {
        let path = state.join("library-link.key");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let b = rand::random::<[u8; 32]>();
                aaa_common::util::write_private(&path, &b)?;
                b.to_vec()
            }
            Err(e) => return Err(e).context("보고서 서명 키 읽기"),
        };
        if bytes.len() != 32 {
            bail!("보고서 서명 키 길이가 틀렸다");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&path)?.permissions().mode() & 0o777 != 0o600 {
                bail!("보고서 서명 키 권한은 0600이어야 한다");
            }
        }
        Ok(Self(hmac::Key::new(hmac::HMAC_SHA256, &bytes)))
    }

    fn message(id: &str) -> String {
        format!("aaa-library-link:v1:{id}")
    }

    fn cap(&self, id: &str) -> String {
        let tag = hmac::sign(&self.0, Self::message(id).as_bytes());
        format!("{id}{}", hex::encode(&tag.as_ref()[..16]))
    }

    fn verify<'a>(&self, cap: &'a str) -> Option<&'a str> {
        if cap.len() != 48 || !cap.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
            return None;
        }
        let (id, sig) = cap.split_at(16);
        let supplied = hex::decode(sig).ok()?;
        let message = Self::message(id);
        // HMAC의 앞 128bit를 쓴다. 뒤 128bit는 계산한 값으로 채워 ring의 상수 시간 verify로 비교한다.
        let expected = hmac::sign(&self.0, message.as_bytes());
        let mut tag = expected.as_ref().to_vec();
        tag[..16].copy_from_slice(&supplied);
        hmac::verify(&self.0, message.as_bytes(), &tag).ok()?;
        Some(id)
    }
}

impl Host {
    pub(crate) fn link_view(&self, r: LibraryLinkRow) -> LinkView {
        let base = format!("/v/{}/", self.library_signing.cap(&r.id));
        let size = self
            .project_dir(&r.project)
            .ok()
            .map(|d| {
                self.manifest(&d, &r.thread)
                    .iter()
                    .filter(|f| !f.on_agent && crate::library::safe_rel(&f.path))
                    .map(|f| f.size)
                    .sum()
            })
            .unwrap_or(0);
        LinkView {
            id: r.id,
            project: r.project,
            thread: r.thread,
            url: self.public_origin().map(|o| format!("{o}{base}")),
            base,
            created_at_ms: r.created,
            expires_at_ms: r.expires,
            last_opened_at_ms: r.opened,
            revoked: r.revoked,
            size,
        }
    }

    pub(crate) fn library_link(
        &self,
        project: &str,
        thread: &str,
        days: Option<u32>,
        actor: &str,
    ) -> Result<LinkView, ErrorBody> {
        let dir = self.project_dir(project)?;
        self.load(project, thread)?;
        if !crate::library::manifest_path(&dir, thread).is_file() {
            return Err(err(ErrorCode::NotFound, "아직 회수한 결과물이 없다"));
        }
        let days = days.unwrap_or(DEFAULT_DAYS);
        if !(1..=3650).contains(&days) {
            return Err(err(ErrorCode::BadRequest, "수명은 1~3650일이다"));
        }
        let now = aaa_common::util::now_ms();
        let (row, created) = self
            .db
            .library_link_issue(LibraryLinkRow {
                id: hex::encode(rand::random::<[u8; 8]>()),
                project: project.into(),
                thread: thread.into(),
                created: now,
                expires: now + days as u64 * DAY_MS,
                opened: None,
                revoked: false,
            })
            .map_err(super::orch::internal)?;
        if created {
            self.db.audit(actor, "library_link", &format!("{project}/{thread} id={}", row.id));
        }
        Ok(self.link_view(row))
    }

    pub(crate) fn library_links(&self, project: Option<&str>) -> Result<Vec<LinkView>, ErrorBody> {
        if let Some(p) = project {
            self.project_dir(p)?;
        }
        Ok(self
            .db
            .library_links(project)
            .map_err(super::orch::internal)?
            .into_iter()
            .map(|r| self.link_view(r))
            .collect())
    }

    pub(crate) fn latest_library_link(&self, project: &str, thread: &str) -> Option<LinkView> {
        self.db.library_links(Some(project)).ok()?.into_iter().find(|r| r.thread == thread).map(|r| self.link_view(r))
    }

    pub(crate) fn library_link_revoke(&self, id: &str, actor: &str) -> Result<LinkView, ErrorBody> {
        if !self.db.library_link_revoke(id).map_err(super::orch::internal)? {
            return Err(err(ErrorCode::NotFound, "보고서 링크가 없다"));
        }
        self.db.audit(actor, "library_link_revoke", &format!("id={id}"));
        Ok(self.link_view(self.db.library_link_get(id).map_err(super::orch::internal)?.unwrap()))
    }

    /// 요청마다 DB를 읽으므로 폐기가 바로 반영된다. 수명 판정은 호출자가 준 시각으로 한다.
    pub(crate) fn signed_library_at(&self, cap: &str, now: u64) -> Result<(String, String), ErrorBody> {
        let fail = || err(ErrorCode::Unauthorized, "보고서 링크가 없거나 만료·폐기됐다");
        let id = self.library_signing.verify(cap).ok_or_else(fail)?;
        let r = self.db.library_link_get(id).map_err(super::orch::internal)?.ok_or_else(fail)?;
        if r.revoked || now >= r.expires {
            return Err(fail());
        }
        self.db.library_link_opened(id, now).map_err(super::orch::internal)?;
        Ok((r.project, r.thread))
    }

    /// 응답할 때만 붙인다. inbox 원본·흐름 사건에는 bearer 경로를 남기지 않는다.
    pub(crate) fn library_notice_links(&self, items: &mut [InboxItem]) {
        for i in items.iter_mut().filter(|i| i.item_kind == "thread-state" && i.event == "new report") {
            if let Some(url) = self
                .latest_library_link(&i.project, &i.subject)
                .filter(|l| !l.revoked && l.expires_at_ms > aaa_common::util::now_ms())
                .and_then(|l| l.url)
            {
                i.body.push_str(&format!("\n보기: {url}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, LibraryFile, Role, ThreadKind};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct Lab {
        h: Arc<Host>,
        state: PathBuf,
        cfg: aaa_config::HostConfig,
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.state);
        }
    }

    impl Lab {
        fn new() -> Self {
            let state = std::env::temp_dir().join(format!("aaa-library-links-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.notify.web_push = false;
            cfg.projects.root = state.join("projects").display().to_string();
            let h = Host::open(cfg.clone(), state.clone()).unwrap();
            h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
            h.project_create("demo", Default::default(), "test").unwrap();
            Self { h, state, cfg }
        }

        fn report(&self, id: &str, files: &[(&str, &[u8], bool)]) -> PathBuf {
            let dir = self.h.project_dir("demo").unwrap();
            let mut r =
                crate::orch::ThreadRec::starting("demo", "보고", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
            r.id = id.into();
            r.report_hash = Some("report".into());
            r.status = aaa_proto::ThreadStatus::Resolved;
            self.h.db.thread_import("demo", id, "resolved", &serde_json::to_string(&r).unwrap()).unwrap();
            let root = dir.join("library").join(id);
            std::fs::create_dir_all(&root).unwrap();
            let mut manifest = Vec::new();
            for (p, b, on_agent) in files {
                if !on_agent {
                    let path = root.join(p);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, b).unwrap();
                }
                manifest.push(LibraryFile {
                    path: (*p).into(),
                    size: b.len() as u64,
                    on_agent: *on_agent,
                    mime: String::new(),
                    kind: aaa_proto::FileKind::Binary,
                });
            }
            std::fs::write(crate::library::manifest_path(&dir, id), serde_json::to_vec(&manifest).unwrap()).unwrap();
            root
        }

        fn issue(&self, id: &str) -> LinkView {
            self.h.library_link("demo", id, None, "test").unwrap()
        }
    }

    async fn get(h: &Arc<Host>, path: &str, range: Option<&str>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut req = Request::builder().uri(path).header("Host", "127.0.0.1:7000");
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        let res = crate::web::router(h.clone()).oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let (parts, body) = res.into_parts();
        (parts.status, parts.headers, to_bytes(body, 1024 * 1024).await.unwrap().to_vec())
    }

    #[tokio::test]
    async fn signed_report_http_boundary() {
        let lab = Lab::new();
        lab.report(
            "t-0001",
            &[
                ("index.html", b"<p>report</p>", false),
                ("sub/a.txt", b"abcdef", false),
                (".env", b"hidden", false),
                ("big.bin", b"large", true),
            ],
        );
        lab.report("t-0002", &[("other.txt", b"other report", false)]);
        let link = lab.issue("t-0001");
        let (status, headers, body) = get(&lab.h, &link.base, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"<p>report</p>");
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(csp.split(';').any(|d| d.trim().starts_with("sandbox ")));
        assert!(!csp.contains("allow-same-origin"));
        assert!(csp.contains("connect-src 'none'") && csp.contains("form-action 'none'"));
        assert_eq!(headers["cache-control"], "private, no-store");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        let (status, head, body) = get(&lab.h, &format!("{}sub/a.txt", link.base), Some("bytes=1-3")).await;
        assert_eq!((status, body), (StatusCode::PARTIAL_CONTENT, b"bcd".to_vec()));
        assert_eq!(head["content-range"], "bytes 1-3/6");
        for (path, want) in [
            ("other.txt", StatusCode::NOT_FOUND),
            ("../t-0002/other.txt", StatusCode::BAD_REQUEST),
            ("%2e%2e%2ft-0002/other.txt", StatusCode::BAD_REQUEST),
            (".env", StatusCode::NOT_FOUND),
            ("sub/.env", StatusCode::NOT_FOUND),
        ] {
            let (st, head, _) = get(&lab.h, &format!("{}{path}", link.base), None).await;
            assert_eq!(st, want, "{path}");
            assert!(head["content-security-policy"].to_str().unwrap().contains("sandbox"));
        }
        // 앞 31개 서명 문자는 맞고 마지막만 틀린 위조도 거부한다.
        let mut forged = link.base.trim_end_matches('/').to_string();
        let last = forged.pop().unwrap();
        forged.push(if last == '0' { '1' } else { '0' });
        forged.push('/');
        assert_eq!(get(&lab.h, &forged, None).await.0, StatusCode::UNAUTHORIZED);
        let (st, head, body) = get(&lab.h, &format!("{}big.bin", link.base), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(head["content-type"], "text/html; charset=utf-8");
        assert!(String::from_utf8(body).unwrap().contains("앱의 보고서 화면"));
        #[cfg(unix)]
        {
            let root = lab.h.project_dir("demo").unwrap().join("library/t-0001");
            std::fs::remove_file(root.join("sub/a.txt")).unwrap();
            std::os::unix::fs::symlink(root.join("index.html"), root.join("sub/a.txt")).unwrap();
            assert_eq!(get(&lab.h, &format!("{}sub/a.txt", link.base), None).await.0, StatusCode::NOT_FOUND);
            std::fs::remove_dir_all(root.join("sub")).unwrap();
            std::os::unix::fs::symlink(lab.h.project_dir("demo").unwrap().join("library/t-0002"), root.join("sub"))
                .unwrap();
            // manifest 안에 있는 경로라도 중간 폴더 symlink는 따라가지 않는다.
            std::fs::write(lab.h.project_dir("demo").unwrap().join("library/t-0002/a.txt"), b"outside").unwrap();
            assert_eq!(get(&lab.h, &format!("{}sub/a.txt", link.base), None).await.0, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn signed_report_has_no_session() {
        let lab = Lab::new();
        lab.report("t-0001", &[("index.html", b"report", false)]);
        let l = lab.issue("t-0001");
        let (status, headers, _) = get(&lab.h, &l.base, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!headers.contains_key("set-cookie"), "링크는 장치 세션을 만들면 안 된다");
        assert!(lab.h.db.devices().unwrap().is_empty());
        assert!(lab.h.pairing.lock().unwrap().pending.is_none());
        assert!(lab.h.inner.lock().unwrap().panes.is_empty());
        assert!(lab.h.view_caps.lock().unwrap().is_empty());
        assert!(lab.h.file_caps.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn signed_report_cannot_read_a_recreated_project() {
        let lab = Lab::new();
        lab.report("t-0001", &[("index.html", b"old report", false)]);
        let old = lab.issue("t-0001");
        let app = lab.h.thread_library("demo", "t-0001", None, true).unwrap();
        lab.h.project_delete("demo", false, "test").await.unwrap();
        lab.h.project_create("demo", Default::default(), "test").unwrap();
        lab.report("t-0001", &[("index.html", b"new private report", false)]);
        assert_eq!(get(&lab.h, &old.base, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get(&lab.h, &app.base, None).await.0, StatusCode::UNAUTHORIZED);
        let new = lab.issue("t-0001");
        assert_ne!(new.id, old.id);
        assert_eq!(lab.h.db.project_export("demo").unwrap()["library_links"][0]["id"], new.id);
        lab.h.project_rename("demo", "renamed", false, "test").await.unwrap();
        assert_eq!(get(&lab.h, &new.base, None).await.2, b"new private report");
        assert_eq!(lab.h.db.library_link_get(&new.id).unwrap().unwrap().project, "renamed");
    }

    #[tokio::test]
    async fn signed_report_key_rotation_preserves_pairing() {
        let mut lab = Lab::new();
        lab.report("t-0001", &[("index.html", b"report", false)]);
        let old = lab.issue("t-0001");
        let (code, _) = lab.h.mint_pairing(DeviceRole::Controller, std::time::Instant::now());
        let (token, _) = lab.h.redeem_pairing(&code, "시험 기기").unwrap();
        let device = lab.h.device_role(&token).unwrap();
        let tls_key = std::fs::read(lab.state.join("tls/key.der")).unwrap();
        let signing = std::fs::read(lab.state.join("library-link.key")).unwrap();
        assert_eq!(signing.len(), 32);
        assert_ne!(signing, tls_key);
        aaa_common::util::write_private(&lab.state.join("library-link.key"), &[0xa5; 32]).unwrap();
        lab.h = Host::open(lab.cfg.clone(), lab.state.clone()).unwrap();
        lab.h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(get(&lab.h, &old.base, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(lab.h.device_role(&token), Some(device));
        assert_eq!(std::fs::read(lab.state.join("tls/key.der")).unwrap(), tls_key);
        let new = lab.issue("t-0001");
        assert_ne!(new.base, old.base);
        assert_eq!(get(&lab.h, &new.base, None).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn signed_report_lifetime_permissions_and_restart() {
        let mut lab = Lab::new();
        lab.report("t-0001", &[("index.html", b"report", false)]);
        let l = lab.issue("t-0001");
        assert_eq!(l.expires_at_ms - l.created_at_ms, 30 * DAY_MS);
        assert_eq!(lab.issue("t-0001").id, l.id);
        let cap = l.base.trim_matches('/').strip_prefix("v/").unwrap();
        assert!(lab.h.signed_library_at(cap, l.expires_at_ms - 1).is_ok());
        assert_eq!(lab.h.signed_library_at(cap, l.expires_at_ms).unwrap_err().code, ErrorCode::Unauthorized);
        assert_eq!(lab.h.signed_library_at(cap, l.expires_at_ms + 1).unwrap_err().code, ErrorCode::Unauthorized);
        assert_eq!(lab.h.db.meta("schema").unwrap().as_deref(), Some("1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(lab.state.join("library-link.key")).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        lab.h = Host::open(lab.cfg.clone(), lab.state.clone()).unwrap();
        lab.h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(get(&lab.h, &l.base, None).await.0, StatusCode::OK);
        for req in [
            FrontRequest::LibraryLink { project: "demo".into(), thread: "t-0001".into(), days: None },
            FrontRequest::LibraryLinks { project: None },
            FrontRequest::LibraryLinkRevoke { id: l.id.clone() },
        ] {
            assert_eq!(lab.h.call(DeviceRole::Reader, "reader", req).await.unwrap_err().code, ErrorCode::Forbidden);
        }
        let r = lab
            .h
            .call(
                DeviceRole::Reader,
                "reader",
                FrontRequest::ThreadLibrary { project: "demo".into(), thread: "t-0001".into() },
            )
            .await
            .unwrap();
        let FrontResponse::Library(view) = r else { panic!("library 응답이어야 한다") };
        assert!(view.link.is_none());
        lab.h.library_link_revoke(&l.id, "test").unwrap();
        assert_eq!(get(&lab.h, &l.base, None).await.0, StatusCode::UNAUTHORIZED);
        // 공유 링크를 폐기해도 pairing 기기가 받은 앱 안 보기 경로는 계속 된다.
        assert_eq!(get(&lab.h, &format!("{}index.html", view.base), None).await.0, StatusCode::OK);
        assert_ne!(lab.issue("t-0001").id, l.id);
        assert!(lab.h.library_link("demo", "t-0001", Some(0), "test").is_err());
    }

    #[tokio::test]
    async fn signed_report_listing_and_notice() {
        let lab = Lab::new();
        lab.report(
            "t-0001",
            &[
                // 다른 OS의 agent에서 온 이름은 manifest에 남을 수 있다. Windows에는 이 이름의 파일을 만들 수 없다.
                ("<script>.txt", b"safe", true),
                ("a&'b.txt", b"local", false),
                (".env", b"hidden", false),
                ("big.bin", b"large", true),
            ],
        );
        let l = lab.issue("t-0001");
        let (st, _, body) = get(&lab.h, &l.base, None).await;
        assert_eq!(st, StatusCode::OK);
        let page = String::from_utf8(body).unwrap();
        assert!(page.contains("&lt;script&gt;.txt") && page.contains("./%3Cscript%3E.txt"));
        assert!(page.contains("a&amp;&#39;b.txt") && page.contains("./a%26%27b.txt"));
        assert!(!page.contains("<script>") && !page.contains(".env"));
        let (st, _, body) = get(&lab.h, &format!("{}a%26%27b.txt", l.base), None).await;
        assert_eq!((st, body), (StatusCode::OK, b"local".to_vec()));
        assert_eq!(get(&lab.h, "/v/", None).await.0, StatusCode::NOT_FOUND);
        lab.h.inbox_add("demo", "thread-state", "t-0001", "new report", "보고 준비");
        let mut items = lab.h.db.inbox_list(Some("demo"), false).unwrap();
        lab.h.library_notice_links(&mut items);
        assert!(!items[0].body.contains("보기:"), "공개하지 않은 loopback은 밖 알림에 싣지 않는다");
        lab.h.db.set_meta("publish_origin", "https://mac-a.example.ts.net:8443").unwrap();
        lab.h.library_notice_links(&mut items);
        assert!(items[0].body.contains("보기: https://mac-a.example.ts.net:8443/v/"));
        let telegram = crate::notify::notify_body("telegram", "example", "demo", &items);
        assert!(telegram["text"].as_str().unwrap().contains("\n보기: https://"));
        assert_eq!(telegram["disable_web_page_preview"], true);
        assert!(!lab.h.db.inbox_list(Some("demo"), false).unwrap()[0].body.contains("/v/"));
    }
}
