//! 작업 경로는 CLI 설정 → 실제 agent 시작 경계에서 검사한다.
use super::*;

#[test]
fn agent_refuses_an_unmounted_work_volume_before_creating_a_workspace() {
    let mut lab = Lab::new();
    let node = lab.node("agent", None);
    // 빠진 볼륨 자리에 남은 일반 폴더: exists 검사만 하면 통과하는 입력이다.
    let mount = lab.root.join("volume");
    let root = mount.join("work");
    std::fs::create_dir(&mount).unwrap();
    std::fs::write(
        node.cfg.join("agent.toml"),
        format!("schema = 1\n[agent]\nwork_root = {root:?}\nwork_mount = {mount:?}\n"),
    )
    .unwrap();
    lab.spawn("agent-start", &node, &["agent"]);
    assert!(!lab.wait_exit("agent-start", T).success());
    assert!(lab.log("agent-start").contains("작업 볼륨이 마운트되지 않았다"), "{}", lab.log("agent-start"));
    assert!(!root.exists(), "내장에 대체 작업 폴더를 만들면 안 된다");
}
