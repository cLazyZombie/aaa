//! 실제 pane → peer+token agent → host → CLI 영수증의 묶음 완료 계약.
use super::*;

/// 밀린 입력 74개를 음소거 중 CLI 한 번으로 끝낸다. 단건·중복 ID와 실제 수신 목록도 확인한다.
#[test]
fn automation_completion_batch_from_muted_pane_records_all_ids() {
    let (mut w, env) = world_files("", false, Some("0"), false, |_| Vec::new());
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    let w = world_start(w, env);
    let t = w.start(&[
        "--title",
        "입력 완료",
        "--profile",
        "fake",
        "--machine",
        "labA",
        "--kind",
        "dir",
        "--task-file",
        &w.task("complete-task.md", "noreport"),
    ]);
    let id = t["id"].as_str().unwrap();
    let pane = t["pane"].as_str().unwrap();
    w.wait_show("완료 등록 worker 준비", id, |t| t["brief"] == "delivered");
    // 큐/자동 기동은 기존 owner가 검사한다. 이 fixture는 전달이 끝난 74개 입력만 준비한다.
    let db = rusqlite::Connection::open(w.h.state.join("host/host.db")).unwrap();
    db.execute("UPDATE threads SET data=json_set(data,'$.automation','watch') WHERE project='demo' AND id=?1", [id])
        .unwrap();
    db.execute("INSERT INTO meta(key,value) VALUES(?1,'muted')", [format!("post_muted:demo:{id}")]).unwrap();
    let ids: Vec<_> = (0..74).map(|n| format!("ar-test-{n:03}")).collect();
    for run in &ids {
        let data = serde_json::json!({
            "id":run,"name":"watch","input_id":run,"revision":"rev","source":"ci",
            "created_at_ms":1,"updated_at_ms":1,"state":"succeeded","stage":"action",
            "reason":"입력 전달","receipt":"thread_started","thread":id,"task":null,"report":null,
            "input_file":null,"processed":false,"outcome":null,"legacy":false,"events":[],
        });
        db.execute(
            "INSERT INTO automation_runs(project,id,name,input_id,updated,data) VALUES('demo',?1,'watch',?1,1,?2)",
            rusqlite::params![run, data.to_string()],
        )
        .unwrap();
    }
    let out = w.lab.root.join("completion.out");
    let script = w.task("complete.sh", &format!(
        "'{bin}' automation input complete {first} --outcome needs-decision > '{out}' 2>&1\nprintf 'single-rc=%s\\n' \"$?\" >> '{out}'\n\
         '{bin}' automation input complete {runs} {first} --outcome needs-decision >> '{out}' 2>&1\nprintf 'batch-rc=%s\\n' \"$?\" >> '{out}'\n",
        bin=lab::BIN, first=ids[0], runs=ids.join(" "), out=out.display(),
    ));
    w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
    w.h.wait_output(pane, "post-probe-done", T);
    let text = std::fs::read_to_string(out).unwrap();
    assert!(text.contains("single-rc=0") && text.contains("batch-rc=0"), "{text}");
    let receipts: Vec<Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    assert_eq!(receipts.len(), 2, "{text}");
    assert_eq!(receipts[0]["run"], ids[0]);
    assert_eq!(receipts[1]["state"], "automation_completed");
    assert_eq!(receipts[1]["runs"], serde_json::json!(ids));
    let finished: u64 = db.query_row(
        "SELECT COUNT(*) FROM automation_runs WHERE project='demo' AND json_extract(data,'$.processed')=1 AND json_extract(data,'$.outcome')='needs_decision'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(finished, 74);
    assert!(w.h.ok(&["post", "list", "demo"])["posts"].as_array().unwrap().is_empty());
    assert_eq!(w.h.ok(&["automation", "run", "show", &ids[73]])["run"]["processed"], true);
}
