//! 상태줄에서 받은 구독별 마지막 버킷 관측.

use std::collections::BTreeMap;
use std::sync::Mutex;

use aaa_proto::{AgyQuotaBucket, AgyQuotaSample, AgySubscriptionUsage};

use crate::Host;

#[derive(Default)]
pub(crate) struct AgyQuota(Mutex<BTreeMap<String, Observation>>);

struct Observation {
    machine: String,
    sample: AgyQuotaSample,
    received_at_ms: u64,
}

/// T3의 구독 버킷 분류와 agy /usage 그룹의 확인된 모델 이름만 사용한다.
fn model_bucket(model: &str, bucket: &str) -> bool {
    let family = if model.starts_with("gemini-4-argon") {
        "gemini-argon"
    } else if model.starts_with("gemini-4-carbon") {
        "gemini-carbon"
    } else if model.starts_with("gemini-3.8-flash") || model.starts_with("gemini-4-neon") {
        "gemini"
    } else if model.starts_with("claude-") || model.starts_with("gpt-oss-") {
        "3p"
    } else {
        return false;
    };
    bucket.strip_prefix(family).is_some_and(|window| matches!(window, "-5h" | "-weekly"))
}

/// 소진한 창이 있으면 Some이다. 하나라도 리셋 미상이면 바깥 Some만 남긴다.
fn limited(model: &str, buckets: &[AgyQuotaBucket], now: u64) -> Option<Option<u64>> {
    let mut limit = None;
    for b in buckets {
        let observed = b.observed_at.as_deref().and_then(aaa_common::util::parse_iso);
        // 1.5.13 상태줄은 버킷 시각을 생략할 수 있다. 현재 pane에서 받은 잔량을 판정에 사용한다.
        if b.disabled || !model_bucket(model, &b.id) || observed.is_some_and(|ms| ms > now) {
            continue;
        }
        let exhausted = b.remaining_fraction.map_or(b.remaining_amount == Some(0.0), |n| n == 0.0);
        if !exhausted {
            continue;
        }
        let reset = b.resets_at.as_deref().and_then(aaa_common::util::parse_iso);
        match reset {
            None => return Some(None),
            Some(reset) if reset > now => limit = Some(limit.map_or(reset, |old: u64| old.max(reset))),
            _ => {}
        }
    }
    limit.map(Some)
}

impl AgyQuota {
    fn observe(&self, subscription: String, machine: &str, sample: AgyQuotaSample, received_at_ms: u64) {
        let mut slots = self.0.lock().unwrap();
        if slots.get(&subscription).is_some_and(|old| {
            old.machine == machine && old.sample.session == sample.session && old.sample.at_ms >= sample.at_ms
        }) {
            return;
        }
        slots.insert(subscription, Observation { machine: machine.into(), sample, received_at_ms });
    }
}

impl Host {
    pub(crate) fn agy_limit_for(&self, thread: &crate::orch::ThreadRec, now: u64) -> Option<Option<u64>> {
        let launch = thread.launch.as_ref().filter(|l| l.harness == "agy")?;
        let model = launch.argv.windows(2).find_map(|a| (a[0] == "--model").then_some(a[1].as_str()))?;
        let pane = thread.pane.as_deref()?;
        self.conn_of(&thread.machine_id)?;
        let subscription = self.db.agy_slot_subscription(&thread.machine_id, pane).ok()??;
        let observations = self.agy_quota.0.lock().unwrap();
        let o = observations.get(&subscription)?;
        if o.sample.session != pane || o.machine != thread.machine_id {
            return None;
        }
        limited(model, &o.sample.buckets, now)
    }

    pub(crate) fn on_agy_quota(&self, mid: &str, sample: AgyQuotaSample) {
        let running = self.inner.lock().unwrap().panes.get(&sample.session).is_some_and(|p| {
            p.machine_id == mid
                && p.info.state.is_running()
                && p.info.harness.as_deref() == Some("agy")
                && sample.at_ms >= p.info.started_at_ms
        });
        if !running {
            return;
        }
        match self.db.agy_slot_subscription(mid, &sample.session) {
            Ok(Some(subscription)) => {
                let pane = sample.session.clone();
                self.agy_quota.observe(subscription, mid, sample, aaa_common::util::now_ms());
                self.thread_changed(&pane);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!("agy 구독 자리 조회 실패: {error:#}"),
        }
    }

    pub(crate) fn agy_usage(&self) -> Vec<AgySubscriptionUsage> {
        let inner = self.inner.lock().unwrap();
        self.agy_quota
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(subscription, o)| {
                let machine = inner.machines.get(&o.machine);
                let running = inner.panes.get(&o.sample.session).is_some_and(|p| p.info.state.is_running());
                AgySubscriptionUsage {
                    subscription: subscription.clone(),
                    machine: machine.map(|m| m.name.clone()).unwrap_or_default(),
                    plan: o.sample.plan.clone(),
                    buckets: o.sample.buckets.clone(),
                    received_at: aaa_common::util::utc_iso(o.received_at_ms),
                    stale: !running || machine.is_none_or(|m| m.conn.is_none()),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(id: &str, remaining: f64, reset: Option<&str>) -> AgyQuotaBucket {
        AgyQuotaBucket {
            id: id.into(),
            remaining_fraction: Some(remaining),
            remaining_amount: None,
            resets_at: reset.map(str::to_owned),
            observed_at: None,
            disabled: false,
        }
    }

    #[test]
    fn three_machines_replace_one_subscription_observation() {
        let quota = AgyQuota::default();
        for (index, (machine, remaining)) in [("mac-a", 0.8), ("mac-b", 0.7), ("mac-c", 0.6)].into_iter().enumerate() {
            quota.observe(
                "starter".into(),
                machine,
                AgyQuotaSample {
                    session: format!("pane-{index}"),
                    at_ms: 100 + index as u64,
                    plan: Some("Antigravity Starter Quota".into()),
                    buckets: vec![bucket("gemini-weekly", remaining, None)],
                },
                200 + index as u64,
            );
        }
        let observations = quota.0.lock().unwrap();
        assert_eq!(observations.len(), 1);
        let observation = &observations["starter"];
        assert_eq!(observation.machine, "mac-c");
        assert_eq!(observation.sample.buckets, [bucket("gemini-weekly", 0.6, None)]);
    }

    #[test]
    fn an_applicable_empty_weekly_bucket_can_have_an_unknown_reset() {
        let flash = "gemini-3.8-flash-low";
        assert_eq!(limited(flash, &[bucket("gemini-weekly", 0.0, None)], 100), Some(None));
        assert_eq!(limited(flash, &[bucket("3p-weekly", 0.0, None)], 100), None);
        assert_eq!(limited(flash, &[bucket("gemini-weekly", 0.5, None)], 100), None);
        let reset = "1970-01-01T00:00:02Z";
        assert_eq!(limited(flash, &[bucket("gemini-weekly", 0.0, Some(reset))], 100), Some(Some(2000)));
        assert_eq!(limited(flash, &[bucket("gemini-weekly", 0.0, Some(reset))], 2000), None);
    }
}
