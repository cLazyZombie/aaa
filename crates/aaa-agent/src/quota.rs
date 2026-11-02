//! 등록 계정 pane의 Claude 상태줄 관측. 인증·대화·기본 홈은 읽지 않는다.
use aaa_common::{quota, util};
use aaa_proto::{QuotaSnapshot, QuotaSource};
use aaa_session::files::{SessionPaths, SpecFile};
use anyhow::{Result, ensure};

// 로컬 파일의 마지막 입력 하나만 남긴다. QuotaSnapshot 독자는 추가 필드를 버린다.
#[derive(serde::Serialize, serde::Deserialize)]
struct RecordedQuota {
    #[serde(flatten)]
    snapshot: QuotaSnapshot,
    input_diagnostic: quota::claude::Diagnostic,
}

impl crate::Agent {
    pub(crate) fn record_quota(&self, spec: &SpecFile, v: &serde_json::Value) -> Result<String> {
        let generation = spec.account_generation.as_ref().ok_or_else(|| anyhow::anyhow!("계정 귀속 미확인"))?;
        let old = self.quota_snapshot(&spec.id, generation)?;
        let now = util::now_ms();
        let reading = quota::claude::read(v, now, old.as_ref().map(|s| s.windows.as_slice()).unwrap_or(&[]));
        let windows = reading.windows;
        let text = windows.iter().map(|w| statusline(w, now)).collect::<Vec<_>>().join(" · ");
        let snapshot = QuotaSnapshot {
            input_missing: Some(
                [(300, reading.diagnostic.five_hour.decision), (10080, reading.diagnostic.seven_day.decision)]
                    .into_iter()
                    .filter_map(|(duration, decision)| {
                        (decision == quota::claude::Decision::Missing).then_some(duration)
                    })
                    .collect(),
            ),
            generation: generation.clone(),
            source: QuotaSource::ClaudeStatusline,
            sampled_at_ms: now,
            windows,
            problem: None,
            retry_at_ms: None,
        };
        let recorded = RecordedQuota { snapshot, input_diagnostic: reading.diagnostic };
        util::write_json(&SessionPaths::new(&self.state, &spec.id).dir.join("quota.json"), &recorded)?;
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

fn statusline(w: &aaa_proto::QuotaWindow, now: u64) -> String {
    let minutes = now.saturating_sub(w.observed_at_ms) / 60_000;
    let age = if minutes == 0 { "방금".into() } else { format!("{minutes}분 전") };
    format!(
        "{} {}% 남음 ({age}{})",
        if w.duration_mins == Some(300) { "5h" } else { "주간" },
        (100. - w.used_percent).round(),
        if quota::freshness(w, now) == aaa_proto::QuotaFreshness::Unknown { " · 마지막 관측" } else { "" }
    )
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
        assert_eq!(first.input_missing, Some(vec![]));
        a.record_quota(&spec, &json!({"rate_limits":{"five_hour":{"used_percentage":70,"resets_at":9999999999_u64}}}))
            .unwrap();
        let observed = first.windows.iter().find(|w| w.duration_mins == Some(10080)).unwrap();
        assert_eq!(statusline(observed, observed.observed_at_ms + 45 * 60_000), "주간 80% 남음 (45분 전)");
        let reset = observed.resets_at_ms.unwrap();
        assert!(statusline(observed, reset).ends_with(" · 마지막 관측)"));
        let next = a.quota_snapshot("pane1", "generation-a").unwrap().unwrap();
        assert_eq!(
            first.windows.iter().find(|w| w.duration_mins == Some(10080)),
            next.windows.iter().find(|w| w.duration_mins == Some(10080))
        );
        assert_eq!(next.windows.iter().find(|w| w.duration_mins == Some(300)).unwrap().used_percent, 70.);
        assert!(a.quota_snapshot("pane1", "generation-b").unwrap().is_none());
        assert!(a.quota_snapshot("../pane1", "generation-a").is_err());
        assert!(!std::fs::read_to_string(paths.dir.join("quota.json")).unwrap().contains("discard-untrusted-token"));
        // 진단은 이 pane의 마지막 수신 하나만 저장하고 임의 키·인증·대화를 복제하지 않는다.
        let diagnostic_path = paths.dir.join("quota.json");
        let diagnostic: RecordedQuota = util::read_json(&diagnostic_path).unwrap();
        assert_eq!(diagnostic.snapshot.generation, "generation-a");
        assert_eq!(diagnostic.input_diagnostic.five_hour.decision, quota::claude::Decision::Accepted);
        assert_eq!(diagnostic.input_diagnostic.seven_day.decision, quota::claude::Decision::Missing);
        assert_eq!(diagnostic.snapshot.input_missing, Some(vec![10080]));
        let private = aaa_test_support::token("discard-untrusted-diagnostic");
        let mut input = json!({"rate_limits":{"five_hour":{"used_percentage":private,"resets_at":private}},
            "token":private,"transcript_path":private,"messages":[private]});
        input["rate_limits"].as_object_mut().unwrap().insert(private.clone(), json!(private));
        a.record_quota(&spec, &input).unwrap();
        let diagnostic_text = std::fs::read_to_string(&diagnostic_path).unwrap();
        assert!(!diagnostic_text.contains(&private));
        assert!(!diagnostic_text.contains("messages") && !diagnostic_text.contains("transcript"));
        let diagnostic: RecordedQuota = util::read_json(&diagnostic_path).unwrap();
        assert!(
            serde_json::to_string(&diagnostic.input_diagnostic).unwrap().len() < 1024,
            "입력이 커져도 마지막 판정만 저장한다"
        );
        assert_eq!(diagnostic.input_diagnostic.five_hour.decision, quota::claude::Decision::InvalidPercentage);
        assert_eq!(diagnostic.input_diagnostic.five_hour.reset, quota::claude::Reset::Invalid);
        let preserved = a.quota_snapshot("pane1", "generation-a").unwrap().unwrap();
        assert_eq!(preserved.windows, next.windows);
        assert!(
            serde_json::to_value(&preserved).unwrap().get("input_diagnostic").is_none(),
            "로컬 진단은 기존 agent link 응답 형식을 바꾸지 않는다"
        );
        let stored: QuotaSnapshot = util::read_json(&paths.dir.join("quota.json")).unwrap();
        assert_eq!(diagnostic.input_diagnostic.received_at_ms, stored.sampled_at_ms);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(paths.dir.join("quota.json").metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
