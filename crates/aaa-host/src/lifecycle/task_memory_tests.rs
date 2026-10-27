//! m13의 준비 경쟁을 실제 Host/DB/파일의 관측 순서로 나눈다. 배경 ticker·agent·Git은 필요 없다.
use super::*;
use serde_json::{Value, json};

struct Lab {
    h: Arc<Host>,
    dir: std::path::PathBuf,
    // 선언 순서로 Host/SQLite를 먼저 닫고 Windows에서도 파일을 지운다.
    _state: State,
}

struct State(std::path::PathBuf);

impl Drop for State {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

impl Lab {
    fn new() -> Self {
        let state = std::env::temp_dir().join(format!("aaa-task-memory-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        let dir = h.project_dir("demo").unwrap();
        Self { h, dir, _state: State(state) }
    }

    fn write(&self, title: &str) {
        std::fs::write(self.dir.join("TASKS.md"), format!("# Tasks\n\n## Backlog\n- [ ] {title}\n")).unwrap();
    }

    fn pass(&self) {
        self.h.tasks_pass("demo", &self.dir);
    }

    fn log(&self) -> Value {
        let FrontResponse::TaskLog { entries, .. } = self.h.task_log("demo", None, None, None, None).unwrap() else {
            panic!("할 일 기록 응답");
        };
        serde_json::to_value(entries).unwrap()
    }

    async fn delete(&self) {
        self.pass();
        self.write("옛 일");
        self.pass();
        assert!(!self.log().as_array().unwrap().is_empty(), "직접 편집은 기록된다");
        self.h.project_delete("demo", false, "test").await.unwrap();
    }
}

/// 기존 m13 계약(plan §8.311): 재생성한 파일의 첫 관측은 옛 내용과 비교하지 않는다.
#[tokio::test]
async fn task_memory_forgets_deleted_project() {
    let lab = Lab::new();
    lab.delete().await;
    lab.h.project_create("demo", Default::default(), "test").unwrap();
    lab.write("새 일");
    lab.pass();
    assert_eq!(lab.log(), json!([]));
    // 첫 관측 뒤의 편집은 여전히 기록한다.
    lab.write("바뀐 일");
    lab.pass();
    assert!(!lab.log().as_array().unwrap().is_empty());
}

/// 새 빈 파일을 먼저 관측하면 뒤의 추가는 정상 기록이다. 옛 m13은 이 순서를 배제하지 못했다.
#[tokio::test]
async fn task_memory_records_edit_after_recreated_empty_project_was_seen() {
    let lab = Lab::new();
    lab.delete().await;
    lab.h.project_create("demo", Default::default(), "test").unwrap();
    lab.pass();
    assert_eq!(lab.log(), json!([]));
    lab.write("새 일");
    lab.pass();
    let log = lab.log();
    let entries = log.as_array().unwrap();
    assert_eq!(entries.len(), 1, "{log}");
    assert_eq!(entries[0]["action"], "add");
    assert_eq!(entries[0]["actor"], "file");
    assert_eq!(entries[0]["title"], "새 일");
}

/// 삭제 전에 목록을 받은 tick이 삭제 뒤 도착해도 없는 프로젝트의 빈 기억을 다시 심지 않는다.
#[tokio::test]
async fn task_memory_does_not_reinsert_a_deleted_project_from_a_stale_tick() {
    let lab = Lab::new();
    lab.delete().await;
    assert!(!lab.dir.exists());
    lab.pass();
    lab.h.project_create("demo", Default::default(), "test").unwrap();
    lab.write("새 일");
    lab.pass();
    assert_eq!(lab.log(), json!([]), "삭제 뒤 늦은 tick이 재생성한 프로젝트의 첫 관측을 바꾸면 안 된다");
}
