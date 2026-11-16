//! 기획 branch(t-1894 2단계): 실제 host·agent·Git으로 CLI `aaa plan new|save|show|list|discard`와 쉼 뒤 올리기·쉼·다시 열기를 본다.
//! main checkout과 origin main은 끝까지 그대로다.
use super::*;

/// 저장소 기획 폴더 `docs`를 origin main에 올리고 프로젝트 저장소 항목에 `design = "docs"`를 단다. 올리기 전 검사 `check`는
/// host.toml `[projects.design_check]`에 둔다(프로젝트 폴더가 아니다)
fn design_world(extra: &str, check: &str) -> World {
    let w = world_with(&format!("{extra}[projects.design_check]\nrepo = {check}\n"));
    git(&w.repo, &["config", "user.name", "t"]);
    git(&w.repo, &["config", "user.email", "t@example.com"]);
    std::fs::create_dir_all(w.repo.join("docs")).unwrap();
    std::fs::write(w.repo.join("docs/README.md"), "# 개요\n").unwrap();
    git(&w.repo, &["add", "."]);
    git(&w.repo, &["commit", "-qm", "기획 폴더"]);
    git(&w.repo, &["push", "-q", "origin", "main"]);
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labA", path_str(&w.repo))]);
    let file = w.projects.join("demo/project.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    let entry = format!("path = {:?}\nmachine = \"labA\"\n", path_str(&w.repo));
    assert!(text.contains(&entry), "{text}");
    std::fs::write(&file, text.replace(&entry, &format!("{entry}design = \"docs\"\n"))).unwrap();
    w
}

fn origin_ref(w: &World, name: &str) -> Option<String> {
    let o = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", name])
        .current_dir(w.lab.root.join("origin.git"))
        .output()
        .unwrap();
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn branches(w: &World) -> Value {
    let tree = w.h.ok(&["plan", "list"]);
    tree["repos"].as_array().unwrap().iter().find(|r| r["repo"] == "repo").unwrap()["branches"].clone()
}

#[test]
fn design_branch_saves_on_its_own_branch_pushes_after_rest_and_discards() {
    let w = design_world("tick_secs=3600\ndesign_push_secs=1\n", r#"["git", "--version"]"#);
    let main = origin_ref(&w, "refs/heads/main").unwrap();
    let checkout = git(&w.repo, &["rev-parse", "HEAD"]);
    let opened = w.h.ok(&["plan", "new", "repo", "boss-dex", "--title", "보스 도감"]);
    assert_eq!(opened["branch"]["slug"], "boss-dex", "{opened}");
    assert_eq!(opened["branch"]["title"], "보스 도감");
    assert_eq!(opened["branch"]["head"], main, "origin main에서 갈라진다");

    let doc = "docs/bosses/README.md";
    let body = w.lab.root.join("boss.md");
    std::fs::write(&body, "# 보스 도감\n").unwrap();
    let save = |extra: &[&str]| {
        let mut args = vec!["plan", "save", "repo", doc, "--branch", "boss-dex", "--file", path_str(&body)];
        args.extend(extra);
        w.h.json(&args)
    };
    let first = save(&["--new", "--summary", "보스 도감 시작"]).unwrap();
    let blob = first["blob"].as_str().unwrap().to_string();
    let e = save(&["--new"]).unwrap_err();
    assert!(e.contains("다른 판"), "새 문서인데 이미 있다: {e}");
    let e = save(&["--base", "0123456789abcdef0123456789abcdef01234567"]).unwrap_err();
    assert!(e.contains("다른 판"), "{e}");
    assert!(save(&[]).unwrap_err().contains("--base"), "기준 판 없이는 쓰지 않는다");
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n").unwrap();
    let second = save(&["--base", &blob]).unwrap();
    let head = second["commit"].as_str().unwrap().to_string();

    let shown = w.h.ok(&["plan", "show", "repo", doc, "--branch", "boss-dex"]);
    assert_eq!(shown["doc"]["text"], "# 보스 도감\n\n## 해골 왕\n");
    assert_eq!(
        (shown["doc"]["commit"].as_str(), shown["doc"]["branch"].as_str()),
        (Some(head.as_str()), Some("boss-dex"))
    );
    assert!(w.h.json(&["plan", "show", "repo", doc]).is_err(), "main에는 없다");
    let list = branches(&w);
    assert_eq!(list[0]["ahead"], 2, "{list}");
    assert_eq!(list[0]["changed"], serde_json::json!([{ "path": doc, "status": "A" }]));

    // 쉼(1초) 뒤 한 번 올린다. main과 main checkout은 그대로다
    let pushed =
        wait_for("기획 branch 올림", T, || origin_ref(&w, "refs/heads/design/boss-dex").filter(|h| *h == head));
    assert_eq!(pushed, head);
    wait_for("올린 판 기록", T, || (branches(&w)[0]["pushed"] == head.as_str()).then_some(()));
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), main);
    assert_eq!(git(&w.repo, &["rev-parse", "HEAD"]), checkout);
    assert!(!w.repo.join("docs/bosses").exists(), "main checkout의 작업 파일은 그대로");
    let msg = git(&w.repo, &["log", "-1", "--format=%s", "refs/heads/design/boss-dex"]);
    assert_eq!(msg, "기획: docs/bosses/README.md 고침 (사람)");

    // 올린 뒤 더 쓰고 올리기 전(쉼 창 안)에 버려도 host가 올린 origin branch를 지운다(올린 판에 lease)
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n\n## 서리 거인\n").unwrap();
    save(&["--base", second["blob"].as_str().unwrap()]).unwrap();
    w.h.ok(&["plan", "discard", "repo", "boss-dex"]);
    assert_eq!(origin_ref(&w, "refs/heads/design/boss-dex"), None, "host가 올린 origin branch도 지운다");
    assert!(git(&w.repo, &["branch", "--list", "design/boss-dex"]).is_empty());
    assert_eq!(branches(&w), serde_json::json!([]));
    assert!(!w.lab.root.join("workA/repo/demo-design-boss-dex").exists());
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), main);

    // 사람이 먼저 만든 로컬 branch를 꺼내 썼다 버려도 그 branch는 남긴다
    git(&w.repo, &["branch", "design/private", "main"]);
    let own = git(&w.repo, &["rev-parse", "design/private"]);
    w.h.ok(&["plan", "new", "repo", "private"]);
    w.h.ok(&["plan", "discard", "repo", "private"]);
    assert_eq!(git(&w.repo, &["rev-parse", "design/private"]), own, "있던 로컬 branch는 남는다");
    assert!(!w.lab.root.join("workA/repo/demo-design-private").exists());
    assert_eq!(origin_ref(&w, "refs/heads/design/private"), None);
}

#[test]
fn idle_design_branch_rests_without_losing_commits_and_reopens() {
    // 올리기 전 검사가 실패하는 저장소: 쉬기 전에 올리려다 까닭을 남기고, 올리지 않아도 로컬 branch는 남긴다
    let w =
        design_world("tick_secs=3600\ndesign_push_secs=3600\ndesign_rest_secs=1\n", r#"["git", "no-such-subcommand"]"#);
    w.h.ok(&["plan", "new", "repo", "shop"]);
    let body = w.lab.root.join("shop.md");
    std::fs::write(&body, "# 상점\n").unwrap();
    let saved =
        w.h.ok(&["plan", "save", "repo", "docs/shop.md", "--branch", "shop", "--new", "--file", path_str(&body)]);
    let head = saved["commit"].as_str().unwrap().to_string();
    let place = w.lab.root.join("workA/repo/demo-design-shop");
    assert!(place.exists());
    // fixed wait: 쉼 창(design_rest_secs=1)이 지난 뒤 ticker가 판정해야 쉼이다
    std::thread::sleep(Duration::from_millis(1100));
    w.h.ticks(1);
    // ticker는 기획 branch 일을 따로 띄운다: 쉼이 기록될 때까지 기다린다
    let list = wait_for("기획 branch 쉼", T, || {
        let list = branches(&w);
        (list[0]["state"] == "rest").then_some(list)
    });
    assert!(!place.exists(), "worktree를 돌려준다");
    assert_eq!(git(&w.repo, &["rev-parse", "refs/heads/design/shop"]), head, "branch와 commit은 남는다");
    assert_eq!(origin_ref(&w, "refs/heads/design/shop"), None, "검사를 통과하지 못하면 올리지 않는다");
    assert!(list[0]["push_error"].as_str().is_some_and(|e| e.contains("기획 검사를 통과하지 못해")), "{list}");
    let e =
        w.h.json(&["plan", "save", "repo", "docs/shop.md", "--branch", "shop", "--new", "--file", path_str(&body)])
            .unwrap_err();
    assert!(e.contains("쉬는 기획 branch"), "{e}");
    let reopened = w.h.ok(&["plan", "new", "repo", "shop"]);
    assert_eq!(
        (reopened["branch"]["state"].as_str(), reopened["branch"]["head"].as_str()),
        (Some("open"), Some(head.as_str()))
    );
    assert!(place.exists());
}
