//! 승인 알림은 ticker의 보고 수집·예약 판정과 독립적으로 빈 입력창을 확인한다.
use super::*;

impl Host {
    pub(crate) fn wake_nudge(&self) -> u64 {
        let mut requested = 0;
        self.nudge_requested.send_modify(|seq| {
            *seq += 1;
            requested = *seq;
        });
        requested
    }

    pub(super) fn start_nudges(self: &Arc<Self>) {
        let host = self.clone();
        self.supervise(crate::supervisor::Task::InboxNudge, move || {
            let host = host.clone();
            async move {
                let mut requests = host.nudge_requested.subscribe();
                let every = Duration::from_secs(host.cfg.projects.tick_secs.max(1));
                loop {
                    let seq = *requests.borrow_and_update();
                    let start = Instant::now();
                    let mut next = every;
                    for (project, dir) in host.project_directories() {
                        if host.project_status(&project) == aaa_proto::ProjectStatus::Active
                            && let Some(wait) = host.nudge(&project, &dir).await
                        {
                            next = next.min(start.elapsed().saturating_add(wait));
                        }
                    }
                    host.nudge_checked.send_replace(seq);
                    host.task_progress(crate::supervisor::Task::InboxNudge);
                    tokio::select! {
                        _ = requests.changed() => {}
                        _ = tokio::time::sleep(next.saturating_sub(start.elapsed())) => {}
                    }
                }
            }
        });
    }
}
