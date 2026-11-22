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
        let supplied = hex::decode(sig)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let message = Self::message(id);
        // HMAC의 앞 128bit를 쓴다. 뒤 128bit는 계산한 값으로 채워 ring의 상수 시간 verify로 비교한다.
        let expected = hmac::sign(&self.0, message.as_bytes());
        let mut tag = expected.as_ref().to_vec();
        tag[..16].copy_from_slice(&supplied);
        hmac::verify(&self.0, message.as_bytes(), &tag)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        Some(id)
    }
}

impl Host {
    pub(crate) fn link_view(&self, r: LibraryLinkRow) -> LinkView {
        let size = self
            .project_dir(&r.project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
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
        let report_id = self.report_id(&r.project, &r.thread);
        self.link_view_summary(r, size, report_id)
    }

    pub(crate) fn link_view_summary(&self, r: LibraryLinkRow, size: u64, report_id: Option<String>) -> LinkView {
        let base = format!("/v/{}/", self.library_signing.read().unwrap().cap(&r.id));
        LinkView {
            report_id,
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
            // 열린 보고서 목록·thread 화면의 링크 줄이 다시 읽는다(reports 사건)
            self.reports_bump();
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
        self.db
            .library_links(Some(project))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?
            .into_iter()
            .find(|r| r.thread == thread)
            .map(|r| self.link_view(r))
    }

    pub(crate) fn library_link_revoke(&self, id: &str, actor: &str) -> Result<LinkView, ErrorBody> {
        if !self.db.library_link_revoke(id).map_err(super::orch::internal)? {
            return Err(err(ErrorCode::NotFound, "보고서 링크가 없다"));
        }
        self.db.audit(actor, "library_link_revoke", &format!("id={id}"));
        self.reports_bump();
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
        if let Some(v) = self
            .db
            .report_target(project, name)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            && self.report_id(project, &v.source).as_deref() == Some(name)
        {
            self.library_opened(project, &v.source);
        }
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
        self.reports_bump();
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
mod tests;
