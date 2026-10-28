use super::*;
use aaa_proto::{DeviceRole, FrontRequest, PostType, Role, ThreadKind};
use std::{path::PathBuf, sync::Arc};

struct Lab {
    root: PathBuf,
    h: Arc<Host>,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-reserve-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        for (id, name) in [("m-a", "mac-a"), ("m-b", "mac-b")] {
            h.db.upsert_machine(id, name, None, id == "m-a").unwrap();
        }
        drop(h);
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        for m in h.inner.lock().unwrap().machines.values_mut() {
            m.conn = Some(crate::AgentConn::new(aaa_common::conn::frame_channel().0, 1));
        }
        Self { root, h }
    }
    fn tasks(&self) -> Vec<aaa_proto::TaskView> {
        self.h.task_board("demo").unwrap().tasks
    }
    fn thread(&self, machine: &str) -> ThreadRec {
        let id = self
            .h
            .db
            .thread_insert_next("demo", |id| {
                let mut r = ThreadRec::starting("demo", "측정", machine, "claude", Role::Worker, ThreadKind::Dir, 1);
                r.id = id.into();
                r.status = ThreadStatus::Open;
                serde_json::to_string(&r).unwrap()
            })
            .unwrap();
        self.h.load("demo", &id).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn args(machine: &str, task: Option<aaa_proto::TaskView>) -> crate::orch::StartArgs {
    crate::orch::StartArgs {
        automatic_workspace: None,
        start_task: task,
        pool: None,
        over_limit: None,
        reservation: None,
        machine_override: None,
        account: None,
        holds: vec![],
        project: "demo".into(),
        title: "작업".into(),
        machine: Some(machine.into()),
        profile: "claude".into(),
        kind: ThreadKind::Dir,
        repo: None,
        base: None,
        task: "작업".into(),
        role: Role::Worker,
        attachments: vec![],
    }
}

/// 예약 계약(2A): 주인 할 일이 아닌 시작은 자동 배정에서 빠지고 지정 시작은 기다리며, 주인 할 일이 끝나면 저절로 풀린다
#[test]
fn reservation_keeps_others_out_until_the_holder_task_leaves() {
    let lab = Lab::new();
    let file = lab.h.project_dir("demo").unwrap().join("TASKS.md");
    std::fs::write(&file, "## Backlog\n- [ ] 빌드 측정\n- [ ] 다른 일\n").unwrap();
    let tasks = lab.tasks();
    let (measure, other) = (tasks[0].clone(), tasks[1].clone());
    lab.h.machine_reserve("mac-a", "demo", "d-0001", Some("빌드 측정".into()), None, None, "local:cli").unwrap();
    let view = lab.h.machine_views().into_iter().find(|m| m.name == "mac-a").unwrap();
    assert_eq!(view.reservation.as_ref().map(|r| (r.holder.as_str(), r.by.as_str())), Some(("d-0001", "사용자")));

    let (order, blocked) = lab.h.auto_machine_order("demo", other.id.as_deref());
    assert_eq!(order, ["mac-b"]);
    assert!(blocked[0].contains("mac-a 독점 예약(d-0001 · 빌드 측정)"), "{blocked:?}");
    let (order, _) = lab.h.auto_machine_order("demo", measure.id.as_deref());
    assert!(order.contains(&"mac-a".to_string()), "주인 할 일은 그 머신에 들어간다");

    let mut fixed = args("mac-a", Some(other.clone()));
    let e = lab.h.check_machine_reservation(&fixed, "m-a").unwrap_err();
    assert!(e.message.starts_with("start_wait: mac-a 독점 예약"), "{}", e.message);
    fixed.machine_override = Some("사용자".into());
    lab.h.check_machine_reservation(&fixed, "m-a").unwrap();
    lab.h.check_machine_reservation(&args("mac-a", Some(measure)), "m-a").unwrap();
    assert_eq!(
        lab.h.machine_reserve("mac-a", "demo", "d-0002", None, None, None, "local:cli").unwrap_err().code,
        ErrorCode::Conflict,
        "다른 주인은 먼저 풀어야 건다"
    );

    // 주인 할 일이 활성 목록에서 빠지면 읽을 때 푼다
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.lines().filter(|l| !l.contains("빌드 측정")).collect::<Vec<_>>().join("\n")).unwrap();
    assert!(lab.h.machine_reservation("m-a").is_none());
    assert_eq!(lab.h.auto_machine_order("demo", other.id.as_deref()).0.len(), 2);
}

/// 권한 계약(3A): thread는 post 길로 자기 머신만 자기 이름으로 잡고 풀며, 세션 안·보기 권한은 Front로 못 한다.
/// 주인 thread가 끝나면 풀린다
#[tokio::test]
async fn thread_holds_only_its_own_machine_and_release_follows_the_thread() {
    let lab = Lab::new();
    let rec = lab.thread("m-a");
    let post = |topic: &str, machine: &str| PostInput {
        post_id: "0123456789abcdef0123456789abcdef".into(),
        to: "coordinator".into(),
        post_type: PostType::Request,
        topic: topic.into(),
        title: "머신 예약".into(),
        body: serde_json::json!({"machine": machine, "note": "빌드 시간 측정"}).to_string(),
        needs_user: false,
        refs: vec![],
    };
    assert!(lab.h.thread_machine_post(&rec, &post("machine-hold", "mac-b")).unwrap_err().contains("자기 머신만"));
    lab.h.thread_machine_post(&rec, &post("machine-hold", "mac-a")).unwrap();
    let r = lab.h.machine_reservation("m-a").unwrap();
    assert_eq!((r.holder.as_str(), r.by.as_str()), (rec.id.as_str(), rec.id.as_str()));
    assert!(lab.h.reservation_block("m-a", "demo", None).is_some(), "thread 주인은 새 시작을 받지 않는다");

    let release = FrontRequest::MachineRelease { machine: "mac-a".into(), user_request: None };
    for (role, actor) in [
        (DeviceRole::Reader, "device:viewer"),
        (DeviceRole::Local, "worker:p"),
        (DeviceRole::Local, "local:cli@other-pane"),
    ] {
        assert_eq!(lab.h.call(role, actor, release.clone()).await.unwrap_err().code, ErrorCode::Forbidden, "{actor}");
    }
    let weight = FrontRequest::MachineWeightSet { machine: "mac-a".into(), weight: Some(0) };
    assert_eq!(lab.h.call(DeviceRole::Local, "worker:p", weight.clone()).await.unwrap_err().code, ErrorCode::Forbidden);
    lab.h.call(DeviceRole::Controller, "device:owner", weight).await.unwrap();
    assert_eq!(lab.h.machine_views().into_iter().find(|m| m.name == "mac-a").unwrap().weight, Some(0));

    lab.h.update_rec("demo", &rec.id, |r| r.status = ThreadStatus::Resolved).unwrap();
    assert!(lab.h.machine_reservation("m-a").is_none(), "주인 thread가 끝나면 풀린다");
}

/// 선점 트랜잭션 계약: 앞 판정 뒤에 걸린 예약도 DB 선점에서 막고, 시한이 지난 기록은 막지 않는다
#[test]
fn transaction_recheck_sees_a_reservation_made_after_the_precheck() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)", []).unwrap();
    check_tx(&c, "m-a", "demo", None, 10).unwrap();
    let r = MachineReservation {
        project: "demo".into(),
        holder: "d-0001".into(),
        task_id: Some("T1".into()),
        note: None,
        until_ms: Some(100),
        by: "사용자".into(),
        user_request: None,
        at_ms: 1,
    };
    c.execute("INSERT INTO meta VALUES(?1, ?2)", [key("m-a"), serde_json::to_string(&r).unwrap()]).unwrap();
    assert!(check_tx(&c, "m-a", "demo", None, 10).unwrap_err().to_string().starts_with("start_wait:"));
    check_tx(&c, "m-a", "demo", Some("T1"), 10).unwrap();
    check_tx(&c, "m-a", "demo", None, 100).unwrap();
}
