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
        let base = format!("/v/{}/", self.library_signing.read().unwrap().cap(&r.id));
        let size = self
            .project_dir(&r.project)
            .ok()
            .map(|d| {
                self.report_manifest(&d, &r.project, &r.thread)
                    .unwrap_or_default()
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
        if self.db.library_report(project, thread).map_err(super::orch::internal)?.is_none() {
            self.load(project, thread)?;
        }
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
        let id = self.library_signing.read().unwrap().verify(cap).ok_or_else(fail)?;
        let r = self.db.library_link_get(id).map_err(super::orch::internal)?.ok_or_else(fail)?;
        if r.revoked || now >= r.expires {
            return Err(fail());
        }
        Ok((r.project, r.thread))
    }

    /// 열람 기록 실패가 보고서 열기를 막지 않는다. API 열기와 실제 문서 응답이 같은 기록을 쓴다.
    pub(crate) fn library_opened(&self, project: &str, name: &str) {
        match self.db.library_opened_at(project, name, aaa_common::util::now_ms()) {
            Ok(true) => self.reports_bump(),
            Ok(false) => {}
            Err(e) => tracing::warn!("보고서 열람 기록 실패: {e}"),
        }
    }

    pub(crate) fn library_rotate(&self, actor: &str) -> Result<aaa_proto::FrontResponse, ErrorBody> {
        let _ops = self.library_ops.lock().unwrap();
        let mut key = self.library_signing.write().unwrap();
        let bytes = rand::random::<[u8; 32]>();
        aaa_common::util::write_private(&self.state.join("library-link.key"), &bytes).map_err(super::orch::internal)?;
        // 옛 id를 새 키로 다시 노출하지 않는다. 보고서·기기 pairing 키는 그대로다.
        *key = SigningKey(hmac::Key::new(hmac::HMAC_SHA256, &bytes));
        self.db.library_links_revoke_all().map_err(super::orch::internal)?;
        self.db.audit(actor, "library_rotate", "");
        Ok(aaa_proto::FrontResponse::LibraryRotated)
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
            Self::with_retention(30, 20 * 1024)
        }

        fn with_retention(days: u64, mb: u64) -> Self {
            let state = std::env::temp_dir().join(format!("aaa-library-links-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.notify.web_push = false;
            cfg.projects.library_max_days = days;
            cfg.projects.library_max_mb = mb;
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

    async fn publish(lab: &Lab, name: &str, source: &Path, live: bool) -> LinkView {
        let req = FrontRequest::LibraryPublish {
            project: "demo".into(),
            name: name.into(),
            source: source.to_str().unwrap().into(),
            live,
            to_user: false,
            title: None,
            from_thread: None,
        };
        let FrontResponse::LibraryLink(link) = lab.h.call(DeviceRole::Local, "test", req).await.unwrap() else {
            panic!("발급 응답")
        };
        link
    }

    #[tokio::test]
    async fn publish_replaces_the_snapshot_and_preserves_the_link() {
        let lab = Lab::new();
        let source = lab.state.join("input");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("index.html"), b"old").unwrap();
        std::fs::write(source.join("gone.txt"), b"gone").unwrap();
        std::fs::write(source.join(".env"), b"secret").unwrap();
        let old = publish(&lab, "page", &source, false).await;
        assert_eq!(get(&lab.h, &old.base, None).await.2, b"old");
        std::fs::write(source.join("index.html"), b"latest").unwrap();
        std::fs::remove_file(source.join("gone.txt")).unwrap();
        let new = publish(&lab, "page", &source, false).await;
        assert_eq!(old.base, new.base, "다시 올려도 유효한 링크는 최신을 가리킨다");
        assert_eq!(get(&lab.h, &old.base, None).await.2, b"latest");
        assert_eq!(get(&lab.h, &format!("{}gone.txt", old.base), None).await.0, StatusCode::NOT_FOUND);
        assert!(!lab.h.project_dir("demo").unwrap().join("library/page/.env").exists());
        std::fs::File::create(source.join("large.bin")).unwrap().set_len(201 * 1024 * 1024).unwrap();
        assert_eq!(
            lab.h
                .library_publish("demo", "page", source.to_str().unwrap(), false, None, None, "test")
                .unwrap_err()
                .code,
            ErrorCode::TooLarge
        );
        assert_eq!(
            get(&lab.h, &old.base, None).await.2,
            b"latest",
            "거부된 올리기는 이전 사본을 지키며 일부를 덮지 않는다"
        );
        let file = lab.state.join("image.png");
        std::fs::write(&file, b"image").unwrap();
        let link = publish(&lab, "single", &file, false).await;
        assert!(String::from_utf8(get(&lab.h, &link.base, None).await.2).unwrap().contains("image.png"));
        assert_eq!(get(&lab.h, &format!("{}image.png", link.base), None).await.2, b"image");
        for name in ["t-0001", "../escape", ".secret", "a/b"] {
            assert!(
                lab.h.library_publish("demo", name, file.to_str().unwrap(), false, None, None, "test").is_err(),
                "{name}"
            );
        }
    }

    // HTTP 경계가 주인이다. 서명 확인·파일 탐색만으로는 열람이 아니며, 첫 문서 응답이 배지와 마지막 열람을 함께 바꾼다.
    #[tokio::test]
    async fn report_navigation_records_seen_and_opened_but_not_previews() {
        let lab = Lab::new();
        let source = lab.state.join("input");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("index.html"), b"<h1>report</h1>").unwrap();
        std::fs::write(source.join("image.png"), b"image").unwrap();
        let cases = [
            ("head", "HEAD", "", "navigate", "document", None, false, StatusCode::OK),
            ("fetch", "GET", "", "cors", "empty", None, false, StatusCode::OK),
            ("unfurl", "GET", "", "", "", Some(("User-Agent", "TelegramBot")), false, StatusCode::OK),
            ("thumbnail", "GET", "image.png", "no-cors", "image", None, false, StatusCode::OK),
            (
                "prefetch",
                "GET",
                "",
                "navigate",
                "document",
                Some(("Sec-Purpose", "prefetch;prerender")),
                false,
                StatusCode::OK,
            ),
            ("preview", "GET", "", "navigate", "document", Some(("Purpose", "preview")), false, StatusCode::OK),
            ("download", "GET", "?download=1", "navigate", "document", None, false, StatusCode::OK),
            ("missing", "GET", "missing.html", "navigate", "document", None, false, StatusCode::NOT_FOUND),
            (
                "range",
                "GET",
                "",
                "navigate",
                "document",
                Some(("Range", "bytes=9999-")),
                false,
                StatusCode::RANGE_NOT_SATISFIABLE,
            ),
            ("direct", "GET", "", "navigate", "document", None, true, StatusCode::OK),
            ("iframe", "GET", "index.html", "navigate", "iframe", None, true, StatusCode::OK),
            ("app", "GET", "index.html", "navigate", "iframe", None, true, StatusCode::OK),
        ];
        for (name, method, path, mode, dest, extra, opened, status) in cases {
            let FrontResponse::LibraryLink(link) =
                lab.h.library_publish("demo", name, source.to_str().unwrap(), false, Some(None), None, "test").unwrap()
            else {
                panic!("링크 응답")
            };
            let base =
                if name == "app" { lab.h.report_library("demo", name, None, false).unwrap().base } else { link.base };
            let mut changed = lab.h.reports_changed.subscribe();
            changed.borrow_and_update();
            let mut req =
                Request::builder().method(method).uri(format!("{base}{path}")).header("Host", "127.0.0.1:7000");
            if !mode.is_empty() {
                req = req.header("Sec-Fetch-Mode", mode).header("Sec-Fetch-Dest", dest);
            }
            if let Some((key, value)) = extra {
                req = req.header(key, value);
            }
            let res = crate::web::router(lab.h.clone()).oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(res.status(), status, "{name}");
            let _body = to_bytes(res.into_body(), 1024).await.unwrap();
            let report = lab.h.db.library_report("demo", name).unwrap().unwrap();
            let link = lab.h.latest_library_link("demo", name).unwrap();
            assert_eq!(report.unseen(), !opened, "안 봄: {name}");
            assert_eq!(link.last_opened_at_ms.is_some(), opened, "마지막 열람: {name}");
            assert_eq!(report.seen_at, link.last_opened_at_ms, "같은 열람: {name}");
            assert_eq!(changed.has_changed().unwrap(), opened, "배지 사건: {name}");
        }
    }

    /// 보고서를 낸 thread(사용자 요청 10-20): 준 번호 > 이름 > 원본 경로의 뒤 성분부터, 그 프로젝트에 있는 thread만 근원이다.
    /// 다시 올리면 다시 정하고, thread 사본·import는 이름에서 찾는다. 열람(`LibraryView`)도 같은 번호를 싣는다.
    #[test]
    fn reports_name_the_thread_they_came_from() {
        let lab = Lab::new();
        lab.report("t-0001", &[("index.html", b"one", false)]);
        lab.report("t-0002", &[("index.html", b"two", false)]);
        let page = |rel: &str| {
            let d = lab.state.join(rel);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("index.html"), b"<h1>page</h1>").unwrap();
            d
        };
        let publish = |name: &str, src: &Path, given: Option<&str>| {
            lab.h.library_publish("demo", name, src.to_str().unwrap(), false, None, given, "test")
        };
        let from = |name: &str| {
            let r = lab.h.library_reports(Some("demo"), false).unwrap().into_iter().find(|r| r.name == name).unwrap();
            r.from_thread
        };
        // coordinator가 thread library를 옮겨 둔 폴더(뒤 성분)가 thread worktree 폴더보다 먼저다
        let moved = page("projects-t-0002/.aaa/projects-t-0002/library/t-0001");
        publish("weekly", &moved, None).unwrap();
        assert_eq!(from("weekly").as_deref(), Some("t-0001"));
        publish("t0002-review", &moved, None).unwrap();
        assert_eq!(from("t0002-review").as_deref(), Some("t-0002"), "이름이 경로보다 먼저, t0002 꼴도");
        // 없는 thread·낱말 안의 번호는 근원이 아니다
        publish("art-0001", &page("work/t-0009/part-0001"), None).unwrap();
        assert_eq!(from("art-0001"), None);
        // 준 번호가 먼저이고 그 프로젝트에 있어야 한다
        publish("weekly", &moved, Some("t-0002")).unwrap();
        assert_eq!(from("weekly").as_deref(), Some("t-0002"));
        assert_eq!(publish("weekly", &moved, Some("t-0009")).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(publish("weekly", &moved, Some("0001")).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(from("weekly").as_deref(), Some("t-0002"), "거절하면 전 기록 그대로");
        publish("weekly", &page("work/plain"), None).unwrap();
        assert_eq!(from("weekly"), None, "다시 올리면 다시 정한다");
        // thread 사본(DB 행 없음)과 접두사 붙인 import
        assert_eq!(from("t-0001").as_deref(), Some("t-0001"));
        page("batch/t-0002");
        lab.h.library_import("demo", lab.state.join("batch").to_str().unwrap(), "old-", "test").unwrap();
        assert_eq!(from("old-t-0002").as_deref(), Some("t-0002"));
        assert_eq!(
            lab.h.report_library("demo", "t0002-review", None, false).unwrap().from_thread.as_deref(),
            Some("t-0002")
        );
        assert_eq!(lab.h.report_library("demo", "weekly", None, false).unwrap().from_thread, None);
    }

    /// 사용자 보고서(plan §8.256): index.html이 있어야 보내고, 보내면 안 봄·알림 post, 열면 봄, 다시 보내면 다시 안 봄,
    /// --to-user 없이 다시 올리면 작업 보고. 지우기는 끝나지 않은 thread가 하나라도 있으면 아무것도 지우지 않고, 아니면 사본·목록·
    /// thread 보고 사본·공유 링크를 모두 끝낸다. 보기 권한은 지우지 못한다.
    #[tokio::test]
    async fn user_reports_are_unseen_until_opened_and_delete_removes_everything() {
        let lab = Lab::new();
        let dir = lab.h.project_dir("demo").unwrap();
        let view = |name: &str| lab.h.library_reports(None, true).unwrap().into_iter().find(|r| r.name == name);
        let page = lab.state.join("page");
        std::fs::create_dir(&page).unwrap();
        std::fs::write(page.join("index.html"), b"<h1>week</h1>").unwrap();
        let bare = lab.state.join("bare");
        std::fs::create_dir(&bare).unwrap();
        std::fs::write(bare.join("notes.md"), b"notes").unwrap();
        let send = |name: &str, src: &Path, title: Option<&str>| {
            lab.h.library_publish("demo", name, src.to_str().unwrap(), false, Some(title), None, "test")
        };
        assert_eq!(send("bare", &bare, None).unwrap_err().code, ErrorCode::BadRequest, "index.html 없는 사용자 보고서");
        assert!(view("bare").is_none() && !dir.join("library/bare").exists(), "거절하면 아무것도 남기지 않는다");
        assert_eq!(send("week", &page, Some("a\nb")).unwrap_err().code, ErrorCode::BadRequest, "줄바꿈 제목");

        let mut changed = lab.h.reports_changed.subscribe();
        changed.borrow_and_update();
        send("week", &page, Some("주간 보고")).unwrap();
        assert!(changed.has_changed().unwrap(), "보내면 웹에 알린다");
        let w = view("week").unwrap();
        assert!(w.to_user && w.unseen && !w.busy && w.title.as_deref() == Some("주간 보고"), "{w:?}");
        assert_eq!(lab.h.db.library_unseen_count().unwrap(), 1);
        let posts = lab.h.db.post_list("demo").unwrap();
        let post = posts.iter().find(|e| e.input.topic == "report").expect("사용자 알림 post");
        assert!(post.input.to == "user" && post.input.title.contains("주간 보고"), "{post:?}");
        assert_eq!(post.input.refs, vec!["library/demo/week".to_string()]);

        // 보기 권한 기기에서 열어도 봤다(사람 하나의 보고서다)
        changed.borrow_and_update();
        let open = FrontRequest::LibraryOpen { project: "demo".into(), name: "week".into() };
        lab.h.call(DeviceRole::Reader, "device:r", open.clone()).await.unwrap();
        assert!(!view("week").unwrap().unseen && lab.h.db.library_unseen_count().unwrap() == 0);
        assert!(changed.has_changed().unwrap(), "봤으면 배지를 다시 보낸다");
        changed.borrow_and_update();
        lab.h.call(DeviceRole::Reader, "device:r", open).await.unwrap();
        assert!(!changed.has_changed().unwrap(), "이미 본 보고서를 다시 열면 바뀐 것이 없다");

        send("week", &page, None).unwrap();
        let w = view("week").unwrap();
        assert!(w.unseen && w.title.is_none(), "다시 보내면 다시 안 봄: {w:?}");
        lab.h.library_publish("demo", "week", page.to_str().unwrap(), false, None, None, "test").unwrap();
        let w = view("week").unwrap();
        assert!(!w.to_user && !w.unseen && lab.h.db.library_unseen_count().unwrap() == 0, "작업 보고로: {w:?}");

        // 지우기: 끝난 thread 사본(+보고 사본·링크)과 끝나지 않은 thread 사본
        let done = lab.report("t-0001", &[("index.html", b"done", false)]);
        std::fs::write(dir.join("threads/t-0001.md"), b"[\xec\x99\x84\xeb\xa3\x8c]").unwrap();
        let link = lab.issue("t-0001");
        let running = lab.report("t-0002", &[("index.html", b"run", false)]);
        let mut t = lab.h.load("demo", "t-0002").unwrap();
        t.status = aaa_proto::ThreadStatus::Open;
        lab.h.db.thread_save("demo", "t-0002", "open", &serde_json::to_string(&t).unwrap()).unwrap();
        assert!(view("t-0002").unwrap().busy && !view("t-0001").unwrap().busy);
        let refs = |names: &[&str]| {
            names
                .iter()
                .map(|n| aaa_proto::LibraryRef { project: "demo".into(), name: (*n).into() })
                .collect::<Vec<_>>()
        };
        let req = |names: &[&str]| FrontRequest::LibraryDelete { reports: refs(names) };
        assert!(
            lab.h.call(DeviceRole::Reader, "device:r", req(&["t-0001"])).await.is_err(),
            "보기 권한은 지우지 못한다"
        );
        let e = lab.h.call(DeviceRole::Controller, "device:c", req(&["t-0001", "t-0002"])).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(done.exists() && running.exists(), "하나라도 막히면 아무것도 지우지 않는다");
        let e = lab.h.call(DeviceRole::Controller, "device:c", req(&["nope"])).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);

        let FrontResponse::LibraryPruned { removed } =
            lab.h.call(DeviceRole::Controller, "device:c", req(&["t-0001", "week"])).await.unwrap()
        else {
            panic!("지운 목록")
        };
        assert_eq!(removed, vec!["demo/t-0001".to_string(), "demo/week".to_string()]);
        assert!(!done.exists() && !dir.join("library/week").exists());
        for gone in ["threads/t-0001.files.json", "threads/t-0001.md", "threads/week.files.json"] {
            assert!(!dir.join(gone).exists(), "{gone}");
        }
        assert!(view("t-0001").is_none() && view("week").is_none() && view("t-0002").is_some());
        assert!(lab.h.load("demo", "t-0001").is_ok(), "thread 기록은 남긴다");
        assert_eq!(get(&lab.h, &link.base, None).await.0, StatusCode::UNAUTHORIZED, "공유 링크도 끝난다");
        assert!(page.join("index.html").exists(), "원본은 건드리지 않는다");
    }

    #[tokio::test]
    async fn registered_folder_is_live_and_confined() {
        let mut lab = Lab::new();
        let source = lab.state.join("live");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("index.html"), b"before").unwrap();
        let link = publish(&lab, "design", &source, true).await;
        assert_eq!(get(&lab.h, &link.base, None).await.2, b"before");
        std::fs::write(source.join("index.html"), b"after").unwrap();
        std::fs::write(source.join("added.txt"), b"new").unwrap();
        assert_eq!(get(&lab.h, &link.base, None).await.2, b"after");
        assert_eq!(get(&lab.h, &format!("{}added.txt", link.base), None).await.2, b"new");
        std::fs::remove_file(source.join("added.txt")).unwrap();
        assert_eq!(get(&lab.h, &format!("{}added.txt", link.base), None).await.0, StatusCode::NOT_FOUND);
        for path in ["../outside.txt", "%2e%2e%2foutside.txt", "sub/../../outside.txt", "C:%5cprivate.txt"] {
            assert_eq!(get(&lab.h, &format!("{}{path}", link.base), None).await.0, StatusCode::BAD_REQUEST, "{path}");
        }
        std::fs::write(source.join(".env"), b"secret").unwrap();
        assert_eq!(get(&lab.h, &format!("{}.env", link.base), None).await.0, StatusCode::NOT_FOUND);
        assert!(
            !lab.h.project_dir("demo").unwrap().join("library/design/index.html").exists(),
            "실시간 폴더는 복사하지 않는다"
        );
        let rows = lab.h.library_reports(None, false).unwrap();
        assert_eq!((rows[0].name.as_str(), rows[0].size, rows[0].live), ("design", 5, true));
        assert!(rows[0].link.is_none(), "보기 권한에는 서명 링크를 싣지 않는다");
        assert!(
            !serde_json::to_string(&rows).unwrap().contains(source.to_str().unwrap()),
            "등록 원본 경로는 front에 내보내지 않는다"
        );
        lab.h = Host::open(lab.cfg.clone(), lab.state.clone()).unwrap();
        lab.h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(get(&lab.h, &link.base, None).await.2, b"after", "등록과 링크는 host 재시작 뒤에도 남는다");
        #[cfg(unix)]
        {
            std::fs::write(lab.state.join("outside.txt"), b"private").unwrap();
            std::os::unix::fs::symlink(lab.state.join("outside.txt"), source.join("escape.txt")).unwrap();
            std::os::unix::fs::symlink(&lab.state, source.join("sub")).unwrap();
            for path in ["escape.txt", "sub/outside.txt"] {
                assert_eq!(get(&lab.h, &format!("{}{path}", link.base), None).await.0, StatusCode::NOT_FOUND, "{path}");
            }
            std::fs::rename(&source, lab.state.join("old-live")).unwrap();
            std::os::unix::fs::symlink(lab.state.join("old-live"), &source).unwrap();
            assert_ne!(
                get(&lab.h, &link.base, None).await.0,
                StatusCode::OK,
                "등록 root 자체의 symlink 교체도 거부한다"
            );
        }
    }

    /// task 첨부(plan §8.327): 붙인 보고서 주소를 그 보고서로 알아보고, live 공개 문서도 thread로 넘길 사본을 읽는다
    #[tokio::test]
    async fn task_link_resolves_report_urls_and_reads_live_docs() {
        let lab = Lab::new();
        let source = lab.state.join("live");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("index.html"), b"page").unwrap();
        std::fs::write(source.join("plan.md"), b"# one").unwrap();
        let link = publish(&lab, "design", &source, true).await;
        let cap = |base: &str| base.trim_start_matches("/v/").trim_end_matches('/').to_string();
        let resolve = |role, cap: String| {
            lab.h.call(role, "device:c", FrontRequest::LibraryResolve { project: "demo".into(), cap })
        };
        let resolved = FrontResponse::LibraryResolved { project: "demo".into(), name: "design".into() };
        let signed = resolve(DeviceRole::Controller, cap(&link.base)).await.unwrap();
        assert_eq!(serde_json::to_value(signed).unwrap(), serde_json::to_value(&resolved).unwrap(), "공유 링크");
        let app = lab.h.report_library("demo", "design", Some("device:c"), false).unwrap();
        let short = resolve(DeviceRole::Controller, cap(&app.base)).await.unwrap();
        assert_eq!(serde_json::to_value(short).unwrap(), serde_json::to_value(&resolved).unwrap(), "앱 보기 경로");
        assert!(resolve(DeviceRole::Reader, cap(&link.base)).await.is_err(), "첨부는 조작 권한만 한다");
        assert_eq!(lab.h.library_resolve("other", &cap(&link.base)).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(resolve(DeviceRole::Controller, "0".repeat(48)).await.unwrap_err().code, ErrorCode::Unauthorized);
        lab.h.library_link_revoke(&link.id, "test").unwrap();
        assert_eq!(resolve(DeviceRole::Controller, cap(&link.base)).await.unwrap_err().code, ErrorCode::Unauthorized);

        let mut task = "고칠 것\n[계획](library/design/plan.md)\n[없음](library/design/gone.md)".to_string();
        let (files, warnings) = lab.h.task_attachments("demo", &mut task);
        assert_eq!(files.len(), 1, "live 공개는 원본 폴더에서 읽는다: {warnings:?}");
        assert!(
            task.contains(&format!("[계획](attachments/{})", files[0].name)) && files[0].name.ends_with(".md"),
            "{task}"
        );
        assert!(warnings.len() == 1 && warnings[0].contains("library/design/gone.md"), "{warnings:?}");
        std::fs::write(source.join("plan.md"), b"# two").unwrap();
        assert_eq!(lab.h.report_doc_bytes("demo", "design", "plan.md", 1024).unwrap(), b"# two");
        assert_eq!(lab.h.report_doc_bytes("demo", "design", "plan.md", 3).unwrap_err().code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn import_reports_every_skip_and_does_not_overwrite() {
        let lab = Lab::new();
        let input = lab.state.join("import");
        std::fs::create_dir(&input).unwrap();
        for name in ["ok", "duplicate", "large", ".hidden", "t-0001"] {
            std::fs::create_dir(input.join(name)).unwrap();
            std::fs::write(input.join(name).join("index.html"), name).unwrap();
        }
        let old = publish(&lab, "duplicate", &input.join("ok"), false).await;
        std::fs::File::create(input.join("large/big.bin")).unwrap().set_len(201 * 1024 * 1024).unwrap();
        std::fs::write(input.join("loose.png"), b"one").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(input.join("ok"), input.join("linked")).unwrap();
        let FrontResponse::LibraryImported { reports, skipped } =
            lab.h.library_import("demo", input.to_str().unwrap(), "", "test").unwrap()
        else {
            panic!("import 응답")
        };
        assert_eq!(reports.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["ok"]);
        for (name, reason) in [
            ("duplicate", "같은 이름"),
            ("large", "상한"),
            (".hidden", "이름"),
            ("loose.png", "하위 폴더"),
            ("t-0001", "이름"),
        ] {
            assert!(skipped.iter().any(|s| s.name == name && s.reason.contains(reason)), "{name}: {skipped:?}");
        }
        #[cfg(unix)]
        assert!(skipped.iter().any(|s| s.name == "linked" && s.reason.contains("symlink")));
        let FrontResponse::LibraryImported { reports, skipped } =
            lab.h.library_import("demo", input.to_str().unwrap(), "archive-", "test").unwrap()
        else {
            panic!("접두사 import 응답")
        };
        assert_eq!(
            reports.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["archive-duplicate", "archive-ok", "archive-t-0001"]
        );
        assert!(skipped.iter().any(|s| s.name == "large" && s.reason.contains("상한")));
        assert!(skipped.iter().any(|s| s.name == ".hidden"));
        let FrontResponse::LibraryImported { reports, skipped } =
            lab.h.library_import("demo", input.to_str().unwrap(), "archive-", "test").unwrap()
        else {
            panic!("충돌 응답")
        };
        assert!(reports.is_empty());
        assert!(skipped.iter().any(|s| s.name == "t-0001" && s.reason.contains("같은 이름")));
        assert_eq!(get(&lab.h, &old.base, None).await.2, b"ok");
    }

    #[tokio::test]
    async fn prune_uses_publication_age_and_protects_live_sources() {
        let lab = Lab::new();
        let input = lab.state.join("input");
        std::fs::create_dir(&input).unwrap();
        std::fs::write(input.join("index.html"), b"data").unwrap();
        let old = publish(&lab, "old", &input, false).await;
        publish(&lab, "recent", &input, false).await;
        publish(&lab, "live", &input, true).await;
        for name in ["old", "live"] {
            let mut r = lab.h.db.library_report("demo", name).unwrap().unwrap();
            r.updated = 1;
            lab.h.db.library_report_put(&r).unwrap();
        }
        lab.report("t-0001", &[("index.html", b"done", false)]);
        lab.report("t-0002", &[("index.html", b"working", false)]);
        let dir = lab.h.project_dir("demo").unwrap();
        for id in ["t-0001", "t-0002"] {
            std::fs::File::options()
                .write(true)
                .open(crate::library::manifest_path(&dir, id))
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(1))
                .unwrap();
        }
        let mut active = lab.h.load("demo", "t-0002").unwrap();
        active.status = aaa_proto::ThreadStatus::Open;
        lab.h.db.thread_save("demo", "t-0002", "open", &serde_json::to_string(&active).unwrap()).unwrap();
        std::fs::write(dir.join("threads/t-0001.md"), b"keep report").unwrap();
        let app = lab.h.report_library("demo", "old", None, false).unwrap();
        let FrontResponse::LibraryPruned { removed } =
            lab.h.library_prune_at(Some("demo"), 30, aaa_common::util::now_ms(), "test").unwrap()
        else {
            panic!("prune 응답")
        };
        assert_eq!(removed, vec!["demo/old", "demo/t-0001"]);
        assert!(dir.join("library/t-0002/index.html").is_file());
        assert_eq!(std::fs::read(dir.join("threads/t-0001.md")).unwrap(), b"keep report");
        assert_eq!(lab.h.load("demo", "t-0001").unwrap().status, aaa_proto::ThreadStatus::Resolved);
        assert_eq!(get(&lab.h, &old.base, None).await.0, StatusCode::UNAUTHORIZED);
        assert!(
            !lab.h.view_caps.lock().unwrap().contains_key(app.base.trim_start_matches("/v/").trim_end_matches('/')),
            "지운 보고서의 앱 안 cap도 끝난다"
        );
        assert!(input.join("index.html").is_file());
        assert!(lab.h.project_dir("demo").unwrap().join("library/recent/index.html").is_file());
        assert_eq!(lab.h.library_reports(None, true).unwrap().len(), 3);
        assert!(lab.h.library_prune_at(None, 0, u64::MAX, "test").is_err());
    }

    // 실제 사본·DB 상태·서명 HTTP 경계로 정리 계약을 본다. 시각과 상한만 주입한다.
    fn retention_report(lab: &Lab, name: &str, size: usize, updated: u64, active: bool) -> PathBuf {
        let root = lab.report(name, &[("index.html", &vec![b'x'; size], false)]);
        lab.h
            .db
            .library_report_put(&crate::db::LibraryReportRow {
                project: "demo".into(),
                name: name.into(),
                live: None,
                updated,
                ..Default::default()
            })
            .unwrap();
        if active {
            let mut t = lab.h.load("demo", name).unwrap();
            t.status = aaa_proto::ThreadStatus::Open;
            lab.h.db.thread_save("demo", name, "open", &serde_json::to_string(&t).unwrap()).unwrap();
        }
        root
    }

    const RETENTION_NOW: u64 = 40 * 86_400_000;

    /// 웹 「보고서 정리」 실행(관리 GUI W4): 미리 본 후보 그대로일 때만 지운다. 그 사이 후보가 늘면 하나도 지우지 않는다
    #[tokio::test]
    async fn prune_apply_deletes_only_the_previewed_candidates() {
        let lab = Lab::with_retention(1, 0);
        let day = 86_400_000;
        let old = retention_report(&lab, "t-0001", 4, RETENTION_NOW - 2 * day, false);
        let recent = retention_report(&lab, "t-0002", 4, RETENTION_NOW - day, false);
        let plan = lab.h.library_prune_preview(None, None, RETENTION_NOW).unwrap();
        let seen: Vec<String> = plan.candidates.iter().map(|c| format!("{}/{}", c.project, c.name)).collect();
        assert_eq!(seen, vec!["demo/t-0001"]);
        let e = lab.h.library_prune_apply(None, None, &seen, RETENTION_NOW + day, "test").unwrap_err();
        assert_eq!(e.code, aaa_proto::ErrorCode::Conflict, "{}", e.message);
        assert!(old.exists() && recent.exists(), "후보가 바뀌면 하나도 지우지 않는다");
        let FrontResponse::LibraryPruned { removed } =
            lab.h.library_prune_apply(None, None, &seen, RETENTION_NOW, "test").unwrap()
        else {
            panic!("정리 응답")
        };
        assert_eq!(removed, seen);
        assert!(!old.exists() && recent.exists());
    }

    #[tokio::test]
    async fn retention_expires_copies_and_ends_links() {
        let lab = Lab::with_retention(1, 0);
        let old = retention_report(&lab, "t-0001", 4, RETENTION_NOW - 2 * 86_400_000, false);
        let recent = retention_report(&lab, "t-0002", 4, RETENTION_NOW - 86_400_000, false);
        let link = lab.issue("t-0001");
        lab.h.library_retention_pass_at(RETENTION_NOW).unwrap();
        assert!(!old.exists(), "최대 기한을 넘은 사본을 지운다");
        assert!(recent.exists(), "기한 경계는 남긴다");
        assert_eq!(get(&lab.h, &link.base, None).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn retention_capacity_removes_oldest_first() {
        let lab = Lab::with_retention(0, 1);
        let oldest = retention_report(&lab, "t-0003", 400_000, 1, false);
        let middle = retention_report(&lab, "t-0001", 400_000, 2, false);
        let newest = retention_report(&lab, "t-0002", 700_000, 3, false);
        lab.h.library_retention_pass_at(RETENTION_NOW).unwrap();
        assert!(!oldest.exists() && !middle.exists());
        assert!(newest.exists(), "갱신 시각 순으로 상한 아래에 이를 때까지만 지운다");
        let later = retention_report(&lab, "t-0003", 400_000, 4, false);
        lab.h.library_retention_pass_at(RETENTION_NOW + 6 * 3_600_000 - 1).unwrap();
        assert!(newest.exists() && later.exists(), "6시간 전 tick은 다시 정리하지 않는다");
        lab.h.library_retention_pass_at(RETENTION_NOW + 6 * 3_600_000).unwrap();
        assert!(!newest.exists() && later.exists(), "다음 회차에도 갱신 순으로 정리한다");
    }

    #[tokio::test]
    async fn retention_protects_active_and_live_and_excludes_live_bytes() {
        let lab = Lab::with_retention(0, 1);
        let active = retention_report(&lab, "t-0001", 700_000, 1, true);
        let gone = retention_report(&lab, "t-0002", 400_000, 2, false);
        let newest = retention_report(&lab, "t-0003", 200_000, 3, false);
        let live = lab.state.join("live-source");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("index.html"), vec![b'x'; 900_000]).unwrap();
        lab.h.library_publish("demo", "live", live.to_str().unwrap(), true, None, None, "test").unwrap();
        lab.h.library_retention_pass_at(RETENTION_NOW).unwrap();
        assert!(!gone.exists());
        assert!(active.exists() && newest.exists() && live.join("index.html").exists());
        assert!(lab.h.db.library_report("demo", "live").unwrap().is_some());
    }

    #[tokio::test]
    async fn retention_protected_overflow_only_notifies_once() {
        let lab = Lab::with_retention(1, 1);
        let active = retention_report(&lab, "t-0001", 1_100_000, 1, true);
        let other = retention_report(&lab, "t-0002", 4, 1, false);
        lab.h.library_retention_pass_at(RETENTION_NOW).unwrap();
        lab.h.library_retention_pass_at(RETENTION_NOW + 6 * 3_600_000).unwrap();
        assert!(active.exists() && other.exists(), "보호분만으로 상한을 넘으면 삭제 없이 알린다");
        let notices = lab.h.db.inbox_list(None, true).unwrap();
        assert_eq!(notices.iter().filter(|i| i.item_kind == "library-retention").count(), 1);
        // 재시작에도 같은 초과를 다시 알리지 않는다.
        let reopened = Host::open(lab.cfg.clone(), lab.state.clone()).unwrap();
        reopened.library_retention_pass_at(RETENTION_NOW + 12 * 3_600_000).unwrap();
        assert_eq!(
            reopened.db.inbox_list(None, true).unwrap().iter().filter(|i| i.item_kind == "library-retention").count(),
            1
        );
    }

    #[tokio::test]
    async fn retention_preview_lists_age_then_capacity_without_writes() {
        let lab = Lab::with_retention(1, 1);
        let old = retention_report(&lab, "t-0003", 4, 1, false);
        let middle = retention_report(&lab, "t-0001", 700_000, RETENTION_NOW - 1, false);
        let newest = retention_report(&lab, "t-0002", 700_000, RETENTION_NOW, false);
        let before = lab.h.db.project_export("demo").unwrap();
        let plan = lab.h.library_prune_preview(None, None, RETENTION_NOW).unwrap();
        assert_eq!(
            plan.candidates.iter().map(|r| (r.name.as_str(), r.reason.as_str())).collect::<Vec<_>>(),
            vec![("t-0003", "기한"), ("t-0001", "용량")]
        );
        assert_eq!((plan.size, plan.remaining_size, plan.max_size, plan.max_days), (1_400_004, 700_000, 1_048_576, 1));
        assert!(old.exists() && middle.exists() && newest.exists());
        assert_eq!(lab.h.db.project_export("demo").unwrap(), before);
        assert!(lab.h.db.meta("library_retention_last_pass").unwrap().is_none());
        let response = lab
            .h
            .call(
                DeviceRole::Reader,
                "device:test",
                FrontRequest::LibraryPrunePreview { project: Some("demo".into()), older_than_days: Some(0) },
            )
            .await
            .unwrap();
        assert!(matches!(response, FrontResponse::LibraryPrunePreview(_)));
        assert_eq!(lab.h.db.project_export("demo").unwrap(), before);
    }

    #[tokio::test]
    async fn retention_zero_disables_both_limits() {
        let lab = Lab::with_retention(0, 0);
        let old = retention_report(&lab, "t-0001", 1_100_000, 1, false);
        lab.h.library_retention_pass_at(RETENTION_NOW).unwrap();
        assert!(old.exists());
        assert!(lab.h.db.inbox_list(None, true).unwrap().is_empty());
    }

    #[tokio::test]
    async fn rotate_command_ends_old_links_but_keeps_pairing_and_app_views() {
        let mut lab = Lab::new();
        let source = lab.state.join("input");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("index.html"), b"report").unwrap();
        let old = publish(&lab, "page", &source, false).await;
        let (code, _) = lab.h.mint_pairing(DeviceRole::Controller, std::time::Instant::now());
        let (token, _) = lab.h.redeem_pairing(&code, "시험 기기").unwrap();
        let device = lab.h.device_role(&token).unwrap();
        let app = lab.h.report_library("demo", "page", None, false).unwrap();
        let tls = std::fs::read(lab.state.join("tls/key.der")).unwrap();
        let signing = std::fs::read(lab.state.join("library-link.key")).unwrap();
        lab.h.call(DeviceRole::Local, "test", FrontRequest::LibraryRotate).await.unwrap();
        assert!(std::fs::read(lab.state.join("library-link.key")).unwrap() != signing, "서명 키 자체가 교체돼야 한다");
        assert_eq!(get(&lab.h, &old.base, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(lab.h.device_role(&token), Some(device.clone()));
        assert_eq!(get(&lab.h, &app.base, None).await.2, b"report");
        assert_eq!(std::fs::read(lab.state.join("tls/key.der")).unwrap(), tls);
        let new = lab.h.library_link("demo", "page", None, "test").unwrap();
        assert_ne!(new.id, old.id);
        assert_eq!(get(&lab.h, &new.base, None).await.2, b"report");
        lab.h = Host::open(lab.cfg.clone(), lab.state.clone()).unwrap();
        lab.h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(get(&lab.h, &old.base, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(get(&lab.h, &new.base, None).await.2, b"report");
        assert_eq!(lab.h.device_role(&token), Some(device));
    }

    #[tokio::test]
    async fn publication_is_local_only_and_list_respects_reader_permission() {
        let lab = Lab::new();
        let source = lab.state.join("file.txt");
        std::fs::write(&source, b"content").unwrap();
        let req = FrontRequest::LibraryPublish {
            project: "demo".into(),
            name: "page".into(),
            source: source.to_str().unwrap().into(),
            live: false,
            to_user: false,
            title: None,
            from_thread: None,
        };
        for role in [DeviceRole::Controller, DeviceRole::Reader] {
            for req in [
                req.clone(),
                FrontRequest::LibraryImport {
                    project: "demo".into(),
                    source: source.to_str().unwrap().into(),
                    prefix: String::new(),
                },
                FrontRequest::LibraryPrune { project: None, older_than_days: 30 },
                FrontRequest::LibraryRotate,
            ] {
                assert_eq!(lab.h.call(role, "device:test", req).await.unwrap_err().code, ErrorCode::Forbidden);
            }
        }
        publish(&lab, "page", &source, false).await;
        let FrontResponse::LibraryReports { reports } = lab
            .h
            .call(DeviceRole::Reader, "device:test", FrontRequest::LibraryReports { project: None })
            .await
            .unwrap()
        else {
            panic!("목록 응답")
        };
        assert_eq!(reports.len(), 1);
        assert!(reports[0].link.is_none());
        let FrontResponse::Library(v) = lab
            .h
            .call(
                DeviceRole::Reader,
                "device:test",
                FrontRequest::LibraryOpen { project: "demo".into(), name: "page".into() },
            )
            .await
            .unwrap()
        else {
            panic!("앱 안 보기 응답")
        };
        assert_eq!(get(&lab.h, &format!("{}file.txt", v.base), None).await.2, b"content");
        assert_eq!(
            lab.h
                .call(
                    DeviceRole::Reader,
                    "device:test",
                    FrontRequest::LibraryOpen { project: "demo".into(), name: "../private".into() }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
        let usage = lab.h.library_usage().unwrap();
        assert!(matches!(usage, FrontResponse::LibraryUsage { size: 7, reports: 1, live: 0, problems: 0, .. }));
        let live = lab.state.join("usage-live");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("index.html"), b"live").unwrap();
        publish(&lab, "live", &live, true).await;
        assert!(
            matches!(
                lab.h.library_usage().unwrap(),
                FrontResponse::LibraryUsage { size: 7, reports: 2, live: 1, problems: 0, .. }
            ),
            "실시간 원본은 host 사본 용량에 더하지 않는다"
        );
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
        let telegram = crate::notify::notify_body(
            "telegram",
            "example",
            &crate::notify::Place::Project("demo".into()),
            &items,
            &Default::default(),
        );
        assert!(telegram["text"].as_str().unwrap().contains("\n보기: https://"));
        assert_eq!(telegram["disable_web_page_preview"], true);
        assert!(!lab.h.db.inbox_list(Some("demo"), false).unwrap()[0].body.contains("/v/"));
    }
}
