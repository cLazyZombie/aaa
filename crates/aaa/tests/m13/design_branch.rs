//! 기획서 열람과 일반 편집 작업의 경계.
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

#[test]
fn editing_commands_leave_main_for_regular_landing() {
    let w = design_world("tick_secs=3600\n", "[]");
    let head = origin_ref(&w, "refs/heads/main");
    assert!(head.is_some());
    assert_eq!(w.h.ok(&["plan", "show", "repo", "docs/README.md"])["doc"]["text"], "# 개요\n");
    assert!(!w.h.ok(&["plan", "log", "repo", "docs/README.md"])["entries"].as_array().unwrap().is_empty());
    for op in ["new", "save", "attach", "ids", "say", "discard"] {
        assert!(w.h.json(&["plan", op, "repo"]).is_err());
    }
    assert_eq!(origin_ref(&w, "refs/heads/main"), head);
    assert!(!w.lab.root.join("workA/repo/demo-design-main").exists());
}

#[test]
fn preview_and_landing_check_use_committed_document_and_media() {
    let w = design_world("tick_secs=3600\n", "[]");
    let main = git(&w.repo, &["rev-parse", "HEAD"]);
    git(&w.repo, &["switch", "-qc", "review"]);
    let docs = w.repo.join("docs");
    std::fs::create_dir(docs.join("art")).unwrap();
    let gif =
        hex::decode("47494638396101000100800000000000ffffff21f90401000000002c00000000010001000002024401003b").unwrap();
    std::fs::write(docs.join("art/move.gif"), &gif).unwrap();
    std::fs::write(docs.join("README.md"), "# 개요\n\n## 이동\n\n![회피](art/move.gif)\n").unwrap();
    git(&w.repo, &["add", "docs"]);
    git(&w.repo, &["commit", "-qm", "문서 편집"]);
    let head = git(&w.repo, &["rev-parse", "HEAD"]);
    let dest = w.lab.root.join("page");
    let run = |args: &[&str]| w.h.cmd(args).current_dir(&w.repo).capture_output().unwrap();
    let page = run(&["plan", "preview", "docs/README.md", "--base", &main, "--output", path_str(&dest)]);
    assert!(page.status.success(), "{}", String::from_utf8_lossy(&page.stderr));
    let html = std::fs::read_to_string(dest.join("index.html")).unwrap();
    assert!(html.contains("이동") && html.contains("추가됨") && html.contains(".gif"));
    let img =
        std::fs::read_dir(&dest).unwrap().flatten().find(|e| e.path().extension().is_some_and(|s| s == "gif")).unwrap();
    assert_eq!(std::fs::read(img.path()).unwrap(), gif);
    let check = |media, command| {
        run(&[
            "plan",
            "check",
            "docs/README.md",
            "--base",
            &main,
            "--head",
            &head,
            "--media",
            media,
            "--command-json",
            command,
        ])
    };
    assert!(check("art", r#"["git","merge-base","--is-ancestor","{base}","{head}"]"#).status.success());
    let misplaced = check("wrong", "[]");
    assert!(!misplaced.status.success() && String::from_utf8_lossy(&misplaced.stderr).contains("design_media"));
    let failed = check("art", r#"["git","diff","--quiet","{base}","{head}","--","docs/README.md"]"#);
    assert!(!failed.status.success() && String::from_utf8_lossy(&failed.stderr).contains("검사 실패"));
    assert_eq!(origin_ref(&w, "refs/heads/main").as_deref(), Some(main.as_str()));
}
