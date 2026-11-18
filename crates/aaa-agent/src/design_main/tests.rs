use super::*;
use crate::design::tests::{lab, out, sh};
use aaa_proto::{DesignMainChange, DesignMainFile, ErrorCode};

fn cfg(root: &Path) -> ThreadsConfig {
    ThreadsConfig { work_root: root.join("work-root"), ..Default::default() }
}

fn text(path: &str, base: Option<&str>, body: &str) -> DesignMainFile {
    DesignMainFile {
        path: path.into(),
        base: base.map(str::to_string),
        change: DesignMainChange::Text { text: body.into() },
    }
}

/// origin main 끝(clone이 아니라 bare에서 직접)
async fn origin_main(origin: &Path) -> String {
    out(origin, &["rev-parse", "refs/heads/main"], b"").await
}

async fn blob_of(dir: &Path, rev: &str, path: &str) -> String {
    out(dir, &["rev-parse", &format!("{rev}:{path}")], b"").await
}

/// 다른 사람이 origin main에 한 commit을 올린다(따로 clone해서)
async fn someone_pushes(lab: &Path, origin: &Path, path: &str, body: &str) {
    let other = lab.join(format!("other-{}", aaa_common::util::new_id()));
    sh(lab, &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]).await;
    std::fs::create_dir_all(other.join(path).parent().unwrap()).unwrap();
    std::fs::write(other.join(path), body).unwrap();
    sh(&other, &["add", "."]).await;
    sh(&other, &["commit", "-q", "-m", "남의 commit"]).await;
    sh(&other, &["push", "-q", "origin", "HEAD:main"]).await;
}

async fn setup() -> (crate::design::tests::Lab, PathBuf, PathBuf, ThreadsConfig) {
    let (t, clone) = lab().await;
    sh(&clone, &["config", "user.name", "t"]).await;
    sh(&clone, &["config", "user.email", "t@example.com"]).await;
    let origin = t.path().join("origin.git");
    let cfg = cfg(t.path());
    (t, clone, origin, cfg)
}

/// 열면 자리가 origin main 끝(detached)이고, 저장은 한 commit으로 origin main에 들어간다. 사람 checkout·로컬 branch는 그대로다
#[tokio::test]
async fn open_and_write_push_to_main_without_touching_the_checkout() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let clone_head = out(&clone, &["rev-parse", "HEAD"], b"").await;
    let Ok(AgentResponse::DesignBranch { cwd, head, base, opened }) = open(&cfg, "demo", repo).await else {
        panic!("열기")
    };
    assert_eq!((base.as_str(), opened.as_str()), ("origin/main", "main"));
    assert_eq!(head, origin_main(&origin).await, "열면 origin 끝으로 맞춘다");
    let cwd = PathBuf::from(cwd);
    assert!(cwd.ends_with("clone/demo-design-main"), "{}", cwd.display());
    assert_eq!(out(&cwd, &["rev-parse", "--abbrev-ref", "HEAD"], b"").await, "HEAD", "detached");
    let overview = blob_of(&origin, "main", "docs/overview.md").await;
    let files = [
        text("docs/overview.md", Some(&overview), "# 개요\n\n고침\n"),
        text("docs/new.md", None, "# 새 문서\n\n[개요](overview.md)\n"),
    ];
    let Ok(AgentResponse::DesignMainSaved { commit, base, blobs, warnings }) =
        write(&cfg, t.path(), ("demo", repo, "docs"), &files, "기획: 개요 고침 (사람)", &[]).await
    else {
        panic!("저장")
    };
    assert_eq!(commit, origin_main(&origin).await, "origin main에 올라갔다");
    assert_eq!(out(&origin, &["rev-parse", "main^"], b"").await, base, "한 commit");
    assert_eq!(out(&origin, &["log", "-1", "--format=%s", "main"], b"").await, "기획: 개요 고침 (사람)");
    assert!(warnings.is_empty());
    assert_eq!(blobs.len(), 2);
    assert_eq!(blobs[1].blob.as_deref(), Some(blob_of(&origin, "main", "docs/new.md").await.as_str()));
    assert_eq!(out(&clone, &["rev-parse", "HEAD"], b"").await, clone_head, "사람 checkout은 그대로");
    assert_eq!(
        out(&clone, &["rev-parse", "refs/remotes/origin/main"], b"").await,
        commit,
        "다음 읽기가 새 판을 보게 추적 ref를 옮겼다"
    );
    // 읽기(트리)도 곧바로 새 판이다
    let Ok(AgentResponse::DesignTree(tree)) = crate::design::tree(repo, "docs", None).await else { panic!("트리") };
    assert_eq!(tree.commit, commit);
    // 기획 폴더 밖은 쓰지 않는다
    let e = write(&cfg, t.path(), ("demo", repo, "docs"), &[text("README.md", None, "x")], "기획: x (사람)", &[])
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
}

/// 그 사이 같은 문서가 바뀌었으면 Stale, 다른 파일만 바뀌었으면 새 끝 위에 저장한다
#[tokio::test]
async fn stale_base_is_refused_and_other_changes_are_kept() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let old = blob_of(&origin, "main", "docs/overview.md").await;
    someone_pushes(t.path(), &origin, "docs/overview.md", "# 개요\n\n남이 고침\n").await;
    let e = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/overview.md", Some(&old), "# 내 글\n")],
        "기획: x (사람)",
        &[],
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "{e}");
    assert!(e.message.contains("다른 판"), "{e}");
    someone_pushes(t.path(), &origin, "src/code.rs", "fn main() {}\n").await;
    let now = blob_of(&origin, "main", "docs/overview.md").await;
    let before = origin_main(&origin).await;
    let Ok(AgentResponse::DesignMainSaved { base, .. }) = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/overview.md", Some(&now), "# 내 글\n")],
        "기획: 내 글 (사람)",
        &[],
    )
    .await
    else {
        panic!("저장")
    };
    assert_eq!(base, before, "기획 폴더 밖 남의 commit 위에 얹었다");
    assert_eq!(out(&origin, &["show", "main:src/code.rs"], b"").await, "fn main() {}");
}

/// 검사는 새 commit에서 `{base}`·`{head}`를 받아 돌고, 실패하면 stderr 까닭을 싣고 올리지 않는다. 다음 저장은 그 자리를 다시 맞춘다
#[cfg(unix)]
#[tokio::test]
async fn failed_check_tells_why_and_pushes_nothing() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let before = origin_main(&origin).await;
    let seen = t.path().join("seen.txt");
    let check: Vec<String> = [
        "sh",
        "-c",
        r#"echo "$1 $2" > "$3"; git diff --quiet "$1" "$2" -- docs/bad.md && exit 0; echo "docs/bad.md: 깨진 링크 nowhere.md" >&2; exit 3"#,
        "check",
        "{base}",
        "{head}",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([seen.to_string_lossy().into_owned()])
    .collect();
    let e = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/bad.md", None, "[x](nowhere.md)\n")],
        "기획: x (사람)",
        &check,
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("깨진 링크 nowhere.md"), "까닭이 보인다: {e}");
    assert!(e.message.contains("종료 코드 Some(3)"), "{e}");
    assert_eq!(origin_main(&origin).await, before, "올리지 않았다");
    let (b, h) = std::fs::read_to_string(&seen)
        .unwrap()
        .trim()
        .split_once(' ')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap();
    assert_eq!(b, before);
    assert_ne!(h, before, "검사는 새 commit을 받았다");
    // 다음 저장은 실패한 commit을 버리고 origin 끝에서 한다
    let Ok(AgentResponse::DesignMainSaved { base, .. }) = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/good.md", None, "# 좋음\n")],
        "기획: 좋음 (사람)",
        &check,
    )
    .await
    else {
        panic!("다음 저장")
    };
    assert_eq!(base, before);
    assert_eq!(
        out(&origin, &["ls-tree", "--name-only", "main", "docs/"], b"")
            .await
            .lines()
            .filter(|l| l.contains("bad"))
            .count(),
        0
    );
}

/// push가 non-fast-forward로 거절되면 새 끝에서 판 비교·검사부터 다시 한다. 계속 밀리면 횟수 뒤 멈춘다
#[cfg(unix)]
#[tokio::test]
async fn rejected_push_retries_on_the_new_main_then_gives_up() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    // 검사가 처음 한 번만 남의 commit을 origin에 올린다(받은 뒤 올리기 전에 누가 main에 올린 것과 같다)
    let other = t.path().join("racer");
    sh(t.path(), &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]).await;
    let race = |marker: Option<PathBuf>| -> Vec<String> {
        let once =
            marker.map(|m| format!(r#"[ -e "{m}" ] && exit 0; touch "{m}"; "#, m = m.display())).unwrap_or_default();
        let script = format!(
            r#"{once}cd "{o}" && git pull -q --rebase origin main && echo $$ >> race.txt && git add race.txt && git -c user.name=r -c user.email=r@example.com commit -qm race && git push -q origin HEAD:main"#,
            o = other.display()
        );
        vec!["sh".into(), "-c".into(), script]
    };
    let Ok(AgentResponse::DesignMainSaved { commit, base, .. }) = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/a.md", None, "# a\n")],
        "기획: a (사람)",
        &race(Some(t.path().join("once"))),
    )
    .await
    else {
        panic!("다시 해서 저장")
    };
    assert_eq!(commit, origin_main(&origin).await);
    assert_eq!(out(&origin, &["log", "-1", "--format=%s", &base], b"").await, "race", "남의 commit 위에 얹었다");
    // 늘 밀리면(검사마다 남이 올린다) 횟수 뒤 오류다
    let always = race(None);
    let e =
        write(&cfg, t.path(), ("demo", repo, "docs"), &[text("docs/b.md", None, "# b\n")], "기획: b (사람)", &always)
            .await
            .unwrap_err();
    assert!(e.message.contains("3번 모두"), "{e}");
}

/// 조각으로 넘긴 그림을 붙이고(같은 그림은 다시 붙여도 그대로), 지우면 남은 참조를 알린다
#[tokio::test]
async fn staged_images_attach_once_and_delete_warns() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let state = t.path().join("state");
    let png = b"\x89PNG\r\n\x1a\n0000IHDR-test-image".to_vec();
    use sha2::Digest as _;
    let sha = hex::encode(sha2::Sha256::digest(&png));
    let (a, b) = png.split_at(8);
    assert!(matches!(stage(&state, ("demo", repo), &sha, 0, a, false), Ok(AgentResponse::Done)));
    assert!(matches!(stage(&state, ("demo", repo), &sha, 8, b, true), Ok(AgentResponse::Done)));
    let path = format!("docs/game-loop/mockups/{}.png", &sha[..16]);
    let image =
        DesignMainFile { path: path.clone(), base: None, change: DesignMainChange::Staged { sha256: sha.clone() } };
    let doc = blob_of(&origin, "main", "docs/game-loop/README.md").await;
    let files = [
        image.clone(),
        text("docs/game-loop/README.md", Some(&doc), &format!("# 게임 루프\n\n![시안](mockups/{}.png)\n", &sha[..16])),
    ];
    let Ok(AgentResponse::DesignMainSaved { commit, .. }) =
        write(&cfg, &state, ("demo", repo, "docs"), &files, "기획: 시안 (사람)", &[]).await
    else {
        panic!("그림과 문서를 한 commit으로")
    };
    assert_eq!(out(&origin, &["rev-list", "--count", "main^..main"], b"").await, "1");
    assert!(!stage_file(&state, "demo", repo, &sha, true).exists(), "올린 조각은 지운다");
    // 같은 그림을 다시 붙이면 바뀐 것이 없다
    stage(&state, ("demo", repo), &sha, 0, &png, true).unwrap();
    let Ok(AgentResponse::DesignMainSaved { commit: again, base, .. }) =
        write(&cfg, &state, ("demo", repo, "docs"), &[image], "기획: 다시 (사람)", &[]).await
    else {
        panic!("같은 그림")
    };
    assert_eq!((again.as_str(), base.as_str()), (commit.as_str(), commit.as_str()), "새 commit 없음");
    // 참조가 남은 그림을 지우면 경고가 온다
    let blob = blob_of(&origin, "main", &path).await;
    let gone = DesignMainFile { path: path.clone(), base: Some(blob), change: DesignMainChange::Delete };
    let Ok(AgentResponse::DesignMainSaved { warnings, .. }) =
        write(&cfg, &state, ("demo", repo, "docs"), &[gone], "기획: 그림 지움 (사람)", &[]).await
    else {
        panic!("지우기")
    };
    assert!(warnings.iter().any(|w| w.contains("README.md")), "{warnings:?}");
    // 기준 판 없는 지우기·sha256이 다른 조각은 거절
    let e = write(
        &cfg,
        &state,
        ("demo", repo, "docs"),
        &[DesignMainFile { path: "docs/overview.md".into(), base: None, change: DesignMainChange::Delete }],
        "기획: x (사람)",
        &[],
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
    assert!(stage(&state, ("demo", repo), &"0".repeat(64), 0, &png, true).is_err());
}

/// 앞선 쓰기가 commit만 하고 죽었거나 자리에 파일을 남겨도 다음 저장은 origin 끝에서 하고 그것을 올리지 않는다
#[tokio::test]
async fn leftovers_in_the_slot_are_dropped() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let Ok(AgentResponse::DesignBranch { cwd, .. }) = open(&cfg, "demo", repo).await else { panic!("열기") };
    let cwd = PathBuf::from(cwd);
    std::fs::write(cwd.join("docs/half.md"), "올리지 못한 저장\n").unwrap();
    sh(&cwd, &["add", "docs/half.md"]).await;
    sh(&cwd, &["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "죽은 쓰기"]).await;
    std::fs::write(cwd.join("docs/overview.md"), "더러움\n").unwrap();
    std::fs::write(cwd.join("docs/stray.md"), "남은 파일\n").unwrap();
    let Ok(AgentResponse::DesignMainSaved { .. }) =
        write(&cfg, t.path(), ("demo", repo, "docs"), &[text("docs/ok.md", None, "# ok\n")], "기획: ok (사람)", &[])
            .await
    else {
        panic!("저장")
    };
    let names = out(&origin, &["ls-tree", "-r", "--name-only", "main", "docs/"], b"").await;
    assert!(names.contains("docs/ok.md"));
    assert!(!names.contains("half.md") && !names.contains("stray.md"), "{names}");
    assert_eq!(out(&origin, &["show", "main:docs/overview.md"], b"").await, "# 개요");
}

/// 이력은 main이 실제로 가졌던 판만(병합 안 branch commit 빼고) 새것부터, 기획 저장이면 누가를 commit 꼬리에서 읽는다
#[tokio::test]
async fn log_lists_first_parent_versions() {
    let (t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let b0 = blob_of(&origin, "main", "docs/overview.md").await;
    let Ok(AgentResponse::DesignMainSaved { .. }) = write(
        &cfg,
        t.path(),
        ("demo", repo, "docs"),
        &[text("docs/overview.md", Some(&b0), "# 개요\n\n하나\n")],
        "기획: 하나 (t-0001)",
        &[],
    )
    .await
    else {
        panic!("저장")
    };
    // 다른 사람이 branch에서 두 번 고치고 --no-ff로 병합한다
    let other = t.path().join("merger");
    sh(t.path(), &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]).await;
    sh(&other, &["checkout", "-q", "-b", "work"]).await;
    for body in ["# 개요\n\n둘\n", "# 개요\n\n셋\n"] {
        std::fs::write(other.join("docs/overview.md"), body).unwrap();
        sh(&other, &["commit", "-qam", "branch 안 commit"]).await;
    }
    sh(&other, &["checkout", "-q", "main"]).await;
    sh(&other, &["merge", "-q", "--no-ff", "-m", "병합: 개요 고침 (t-0002)", "work"]).await;
    sh(&other, &["push", "-q", "origin", "main"]).await;
    // 읽기 쪽 받기는 1분에 한 번이라 시험에서는 직접 받는다
    fetch_now(&crate::design::toplevel(repo).await.unwrap()).await.unwrap();
    let Ok(AgentResponse::DesignLog { entries }) = log(repo, "docs", "docs/overview.md", 10).await else {
        panic!("이력")
    };
    let got: Vec<_> = entries.iter().map(|e| (e.subject.as_str(), e.by.as_str(), e.other)).collect();
    assert_eq!(
        got,
        [("병합: 개요 고침 (t-0002)", "t", true), ("기획: 하나 (t-0001)", "t-0001", false), ("a", "t", true)],
        "branch 안 commit은 빠진다"
    );
    assert!(entries.iter().all(|e| e.committed_ms > 0));
    let Ok(AgentResponse::DesignLog { entries }) = log(repo, "docs", "docs/overview.md", 1).await else {
        panic!("이력")
    };
    assert_eq!(entries.len(), 1);
    assert!(log(repo, "docs", "README.md", 10).await.is_err(), "기획 폴더 밖");
}

/// ID 표지를 main에 한 commit으로 단다(이미 단 문서·vendor·없는 문서는 건너뛰고, 다시 해도 그대로)
#[tokio::test]
async fn mark_ids_on_main_in_one_commit() {
    let (_t, clone, origin, cfg) = setup().await;
    let repo = clone.to_str().unwrap();
    let mark = |path: &str, id: &str| DesignIdMark { path: path.into(), id: id.into() };
    let ids = [mark("docs/overview.md", "k-1"), mark("docs/missing.md", "k-3"), mark("docs/vendor/LICENSE.md", "k-4")];
    let before = origin_main(&origin).await;
    let Ok(AgentResponse::DesignIdsMarked { commit: Some(commit), marked }) =
        mark_ids(&cfg, ("demo", repo, "docs"), &ids, "기획: 기획서 ID 표지 (사람)", &[]).await
    else {
        panic!("달기")
    };
    assert_eq!(marked, ["docs/overview.md"]);
    assert_eq!(commit, origin_main(&origin).await);
    assert_eq!(out(&origin, &["rev-parse", "main^"], b"").await, before);
    assert_eq!(out(&origin, &["show", "main:docs/overview.md"], b"").await, "# 개요\n<!-- aaa-id: k-0001 -->");
    let Ok(AgentResponse::DesignIdsMarked { commit: None, marked }) =
        mark_ids(&cfg, ("demo", repo, "docs"), &ids, "기획: 다시 (사람)", &[]).await
    else {
        panic!("다시")
    };
    assert!(marked.is_empty());
    assert_eq!(origin_main(&origin).await, commit, "새 commit 없음");
}
