//! host 전체의 기존 [projects] 보존 키를 웹 설정에 연결한다.
use aaa_proto::{ErrorBody, LibraryRetentionView};

use super::{base, conflict, forbidden, local_defines, save_error, toml_ok};
use crate::Host;
use crate::file_rev::{self, Edit};
use crate::orch::bad;

impl Host {
    pub(crate) fn library_retention_settings(&self) -> Result<LibraryRetentionView, ErrorBody> {
        let dir = self.settings_dir()?;
        let rev = self.host_toml_rev(dir)?.sha256;
        let (max_days, bytes) = self.library_limits()?;
        Ok(LibraryRetentionView {
            max_days,
            max_mb: bytes / (1024 * 1024),
            rev,
            editable: !["library_max_days", "library_max_mb"].iter().any(|key| local_defines(dir, "projects", key)),
        })
    }

    pub(crate) fn library_retention_set(
        &self,
        max_days: u64,
        max_mb: u64,
        expected: &str,
        actor: &str,
    ) -> Result<LibraryRetentionView, ErrorBody> {
        if max_days.checked_mul(86_400_000).is_none() || max_mb.checked_mul(1024 * 1024).is_none() {
            return Err(bad("보존 값이 바이트·밀리초로 바꾸기에 너무 크다"));
        }
        // 정리가 도는 도중에는 한도를 바꾸지 않는다. 저장 자체로 정리를 부르거나 회차를 앞당기지 않는다.
        let _ops = self.library_ops.lock().unwrap();
        let dir = self.settings_dir()?;
        if ["library_max_days", "library_max_mb"].iter().any(|key| local_defines(dir, "projects", key)) {
            return Err(forbidden("host.local.toml이 보존 설정을 덮고 있다: 그 파일에서 고친다"));
        }
        let path = dir.join("host.toml");
        let lock = self.file_locks.of(&path);
        let _guard = lock.lock().unwrap();
        file_rev::save(&path, |rev| {
            if rev.sha256 != expected {
                return Err(conflict());
            }
            toml_ok(&rev.text)?;
            let mut doc =
                base(&rev.text).parse::<toml_edit::DocumentMut>().map_err(|_| bad("host.toml을 읽지 못했다"))?;
            if !doc.contains_key("projects") {
                doc["projects"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let table = doc["projects"].as_table_like_mut().ok_or_else(|| bad("projects가 표가 아니다"))?;
            for (key, number) in [("library_max_days", max_days), ("library_max_mb", max_mb)] {
                let mut value = toml_edit::Value::from(number as i64);
                if let Some(previous) = table.get(key).and_then(toml_edit::Item::as_value) {
                    *value.decor_mut() = previous.decor().clone();
                }
                table.insert(key, toml_edit::Item::Value(value));
            }
            let text = doc.to_string();
            // 다른 설정·주석은 남기고 유효한 전체 문서만 저장한다.
            toml_edit::de::from_str::<aaa_config::HostConfig>(&text)
                .map_err(|_| bad("host.toml의 설정 형식이 맞지 않다: aaa config check로 확인한다"))?;
            Ok(if text == rev.text { Edit::Keep(()) } else { Edit::Write(text, ()) })
        })
        .map_err(save_error)?;
        self.db.audit(actor, "library_retention_set", &format!("days={max_days} mb={max_mb}"));
        self.reports_bump();
        self.library_retention_settings()
    }
}
