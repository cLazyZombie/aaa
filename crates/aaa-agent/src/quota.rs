//! 등록 계정 pane의 Claude 상태줄 관측. 인증·대화·기본 홈은 읽지 않는다.
use aaa_common::{quota, util};
use aaa_proto::{QuotaSnapshot, QuotaSource};
use aaa_session::files::{SessionPaths, SpecFile};
use anyhow::{Result, ensure};

impl crate::Agent {
    pub(crate) fn record_quota(&self, spec: &SpecFile, v: &serde_json::Value) -> Result<String> {
        let generation = spec.account_generation.as_ref().ok_or_else(|| anyhow::anyhow!("계정 귀속 미확인"))?;
        let old = self.quota_snapshot(&spec.id, generation)?;
        let now = util::now_ms();
        let windows = quota::claude(v, now, old.as_ref().map(|s| s.windows.as_slice()).unwrap_or(&[]));
        let text = windows
            .iter()
            .map(|w| {
                format!(
                    "{} {}% 남음",
                    if w.duration_mins == Some(300) { "5h" } else { "주간" },
                    (100. - w.used_percent).round()
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        let snapshot = QuotaSnapshot {
            generation: generation.clone(),
            source: QuotaSource::ClaudeStatusline,
            sampled_at_ms: now,
            windows,
            problem: None,
            retry_at_ms: None,
        };
        util::write_json(&SessionPaths::new(&self.state, &spec.id).dir.join("quota.json"), &snapshot)?;
        Ok(text)
    }

    pub(crate) fn quota_snapshot(&self, session: &str, generation: &str) -> Result<Option<QuotaSnapshot>> {
        ensure!(
            !session.is_empty() && session.len() <= 64 && session.bytes().all(|b| b.is_ascii_alphanumeric()),
            "계정 pane 참조 오류"
        );
        let paths = SessionPaths::new(&self.state, session);
        ensure!(!paths.dir.symlink_metadata()?.file_type().is_symlink(), "계정 pane 폴더 오류");
        let spec: SpecFile = util::read_json(&paths.spec())?;
        if spec.account_run.is_none() || spec.account_generation.as_deref() != Some(generation) {
            return Ok(None);
        }
        let path = paths.dir.join("quota.json");
        let m = match path.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        ensure!(m.is_file() && m.len() <= 64 * 1024, "사용량 관측 파일 오류");
        let mut s: QuotaSnapshot = util::read_json(&path)?;
        ensure!(
            s.source == QuotaSource::ClaudeStatusline && s.windows.len() <= 32 && s.windows.iter().all(quota::valid),
            "사용량 관측 형식 오류"
        );
        s.sampled_at_ms = util::now_ms();
        Ok((s.generation == generation).then_some(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn per_pane_files_keep_window_times_and_reject_a_different_login() {
        let temp = Temp(std::env::temp_dir().join(format!("aaa-quota-agent-{}", util::new_id())));
        let a = crate::Agent::new(temp.0.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let spec: SpecFile = serde_json::from_value(
            json!({"id":"pane1","token":aaa_test_support::token("test-token"),"boot_id":"b","created_at_ms":1,
            "spec":{"name":"test","argv":[],"cwd":null,"env":[],"cols":80,"rows":24,"harness":"claude"},
            "account_run":"demo/t-0001","account_generation":"generation-a"}),
        )
        .unwrap();
        let paths = SessionPaths::new(&temp.0, "pane1");
        util::ensure_private_dir(&paths.dir).unwrap();
        util::write_json(&paths.spec(), &spec).unwrap();
        assert!(a.quota_snapshot("pane1", "generation-a").unwrap().is_none());
        a.record_quota(&spec, &json!({"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":9999999999_u64},"seven_day":{"used_percentage":20,"resets_at":9999999999_u64}},"token":aaa_test_support::token("discard-untrusted-token")})).unwrap();
        let first = a.quota_snapshot("pane1", "generation-a").unwrap().unwrap();
        a.record_quota(&spec, &json!({"rate_limits":{"five_hour":{"used_percentage":70,"resets_at":9999999999_u64}}}))
            .unwrap();
        let next = a.quota_snapshot("pane1", "generation-a").unwrap().unwrap();
        assert_eq!(
            first.windows.iter().find(|w| w.duration_mins == Some(10080)),
            next.windows.iter().find(|w| w.duration_mins == Some(10080))
        );
        assert_eq!(next.windows.iter().find(|w| w.duration_mins == Some(300)).unwrap().used_percent, 70.);
        assert!(a.quota_snapshot("pane1", "generation-b").unwrap().is_none());
        assert!(a.quota_snapshot("../pane1", "generation-a").is_err());
        assert!(!std::fs::read_to_string(paths.dir.join("quota.json")).unwrap().contains("discard-untrusted-token"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(paths.dir.join("quota.json").metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
