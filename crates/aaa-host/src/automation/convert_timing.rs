use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Default, serde::Serialize)]
struct Cost {
    calls: u64,
    total_us: u64,
    max_us: u64,
}

pub(crate) struct Timing {
    scope: &'static str,
    started: Instant,
    costs: BTreeMap<&'static str, Cost>,
}

impl Timing {
    pub(crate) fn new(scope: &'static str) -> Self {
        Self { scope, started: Instant::now(), costs: BTreeMap::new() }
    }

    pub(crate) fn record(&mut self, step: &'static str, started: Instant) {
        self.duration(step, "", started.elapsed());
    }

    pub(crate) fn duration(&mut self, step: &'static str, file: &str, elapsed: Duration) {
        let us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        let cost = self.costs.entry(step).or_default();
        cost.calls += 1;
        cost.total_us += us;
        cost.max_us = cost.max_us.max(us);
        tracing::debug!(target: "aaa_conversion", scope = self.scope, step, file, elapsed_us = us,
            "자동화 변환 파일 비용");
    }

    pub(crate) fn measure<T>(&mut self, step: &'static str, file: &str, op: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = op();
        self.duration(step, file, started.elapsed());
        result
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        tracing::info!(target: "aaa_conversion", scope = self.scope,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            costs = %serde_json::to_string(&self.costs).unwrap_or_else(|e| e.to_string()),
            "자동화 변환 단계 비용");
    }
}
