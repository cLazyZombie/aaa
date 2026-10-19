//! 파일 한도 소진은 thread 상태와 무관한 host 시스템 경고다.
use crate::Host;
use std::sync::atomic::Ordering;

impl Host {
    pub(crate) fn resource_error(&self, source: &str, error: &std::io::Error) {
        if aaa_common::resource::is_nofile_error(error) {
            self.mark_resource_exhausted(source);
        }
    }

    pub(crate) fn mark_resource_exhausted(&self, source: &str) {
        tracing::warn!(source, "host 파일 한도 소진");
        self.resource_exhausted.store(true, Ordering::Relaxed);
        self.tick_wake.notify_one();
    }

    pub(crate) fn resource_warning_pass(&self, projects: &[aaa_proto::ProjectView]) {
        if projects.is_empty() || !self.resource_exhausted.swap(false, Ordering::Relaxed) {
            return;
        }
        for p in projects {
            self.inbox_once(&p.name, "system", "host", "file descriptors exhausted",
                "host의 열린 파일 한도가 소진됐다. 연결·사본 수집을 자동으로 다시 시도한다. thread 작업 상태와는 별개인 시스템 경고다");
            if !self.db.inbox_open_exists(&p.name, "system", "host", "file descriptors exhausted").unwrap_or(false) {
                self.resource_exhausted.store(true, Ordering::Relaxed);
            }
        }
    }
}
