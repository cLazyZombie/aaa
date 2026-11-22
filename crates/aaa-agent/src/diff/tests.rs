use super::*;
use aaa_proto::{DiffFileRef, DiffStatus};
use std::process::Command;

/// 비교가 건드리면 안 되는 것: index 바이트·HEAD·ref·작업 파일(.git 안 포함) 경로와 내용
#[derive(PartialEq)]
struct Snapshot {
    index: Vec<u8>,
    head: String,
    refs: String,
    files: Vec<(String, Vec<u8>)>,
}

impl Snapshot {
    /// 실패 원문에 남길 바뀐 항목(파일은 경로와 길이만)
    fn changes(&self, after: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        if self.index != after.index {
            out.push(format!("index {}→{}바이트", self.index.len(), after.index.len()));
        }
        if (&self.head, &self.refs) != (&after.head, &after.refs) {
            out.push(format!("HEAD·ref {:?}→{:?}", (&self.head, &self.refs), (&after.head, &after.refs)));
        }
        let len = |s: &Snapshot, p: &str| s.files.iter().find(|f| f.0 == p).map(|f| f.1.len());
        let mut paths: Vec<&str> = self.files.iter().chain(&after.files).map(|f| f.0.as_str()).collect();
        paths.sort();
        paths.dedup();
        for p in paths {
            let (a, b) = (self.files.iter().find(|f| f.0 == p), after.files.iter().find(|f| f.0 == p));
            if a != b {
                out.push(format!("{p}: {:?}→{:?}바이트", len(self, p), len(after, p)));
            }
        }
        out
    }
}

struct Repo {
    root: PathBuf,
    dir: PathBuf,
    empty: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let root = std::env::temp_dir().join(format!("aaa-diff-test-{}", aaa_common::util::new_id()));
        let dir = root.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = root.join("empty-config");
        std::fs::write(&empty, "").unwrap();
        let r = Repo { root, dir, empty };
        r.git(&["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(r.dir.join(".git/info")).unwrap();
        r
    }
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "init.templateDir=", "-c", "user.name=Test", "-c", "user.email=test@example.com"])
            // commit의 자동 maintenance는 분리된 자식이 `objects/maintenance.lock`을 쥔 채 남는다(Git 2.55).
            // 그 잠금이 비교 전 snapshot에만 잡히면 원본 불변 단언이 틀린다. 제품 `git_prefix`와 같이 끈다.
            .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", &self.empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn snapshot(&self) -> Snapshot {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
            entries.sort();
            for p in entries {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                if p.is_dir() {
                    walk(base, &p, out);
                } else if rel != ".git/index" {
                    out.push((rel, std::fs::read(&p).unwrap()));
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.dir, &self.dir, &mut files);
        let index = std::fs::read(self.dir.join(".git/index")).unwrap();
        Snapshot { index, head: self.git(&["rev-parse", "HEAD"]), refs: self.git(&["for-each-ref"]), files }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn q(scope: DiffScope, patch: bool) -> DiffQuery {
    DiffQuery { scope, patch, ..Default::default() }
}

#[tokio::test]
async fn full_context_is_file_only_bounded_and_keeps_default_patch_small() {
    let r = Repo::new();
    let original: String = (0..90).map(|i| format!("line {i}\n")).collect();
    r.write("old.txt", &original);
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "base"]);
    r.git(&["mv", "old.txt", "new.txt"]);
    r.write("new.txt", &original.replace("line 45\n", "changed\n"));
    let cwd = r.dir.to_str().unwrap();
    let before = r.snapshot();
    let mut ask = DiffQuery {
        file: Some(DiffFileRef { path: "new.txt".into(), old_path: Some("old.txt".into()) }),
        ..q(DiffScope::Uncommitted, true)
    };
    let compact = query(cwd, Some("main"), &ask).await.unwrap();
    assert!(!compact.full_context);
    assert!(!compact.patch.unwrap().contains("line 0\n"));
    ask.full_context = true;
    let full = query(cwd, Some("main"), &ask).await.unwrap();
    assert!(full.full_context && !full.truncated);
    let patch = full.patch.unwrap();
    assert!(patch.contains("rename from old.txt") && patch.contains("rename to new.txt"));
    assert!(patch.contains(" line 0\n") && patch.contains(" line 89\n") && patch.contains("+changed\n"));
    let after = r.snapshot();
    assert!(before == after, "원본 저장소를 바꾸면 안 된다: {:?}", before.changes(&after));

    r.write("new.txt", &format!("{}changed\n", "context\n".repeat(d::FILE_PATCH_MAX_JSON / 7)));
    let cut = query(cwd, Some("main"), &ask).await.unwrap();
    assert!(cut.truncated && !cut.full_context);
    ask.file = None;
    assert!(query(cwd, Some("main"), &ask).await.unwrap_err().to_string().contains("파일 하나"));
}

/// branch 범위는 merge-base 대비 커밋·staged·미커밋·새 파일을 담고, 무시한 파일·`.aaa/`는 빼며, 저장소를 바꾸지 않는다.
#[tokio::test]
async fn branch_and_uncommitted_read_every_change_without_touching_the_repo() {
    let r = Repo::new();
    r.write("keep.txt", "one\ntwo\n");
    r.write("gone.txt", "bye\n");
    r.write("old name.txt", "same\nsame\nsame\n");
    r.write(".gitignore", "*.log\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-qm", "first"]);
    let main = r.git(&["rev-parse", "HEAD"]);
    r.git(&["switch", "-qc", "aaa/demo/t-0001-x"]);
    r.write("src/lib.rs", "fn a() {}\n");
    r.git(&["add", "src/lib.rs"]);
    r.git(&["commit", "-qm", "branch"]);
    // main이 앞으로 가도 merge-base 대비다(main 쪽 변경은 보이지 않는다)
    r.git(&["switch", "-q", "main"]);
    r.write("main-only.txt", "x\n");
    r.git(&["add", "main-only.txt"]);
    r.git(&["commit", "-qm", "main moves"]);
    r.git(&["switch", "-q", "aaa/demo/t-0001-x"]);
    r.git(&["rm", "-q", "gone.txt"]);
    r.git(&["mv", "old name.txt", "new name.txt"]);
    r.write("keep.txt", "one\nTWO\nthree\n");
    r.write("new dir/fresh.md", "# hi\n");
    r.write("debug.log", "ignored\n");
    r.write(".aaa/demo-t-0001/report.md", "work dir\n");
    std::fs::write(r.dir.join(".git/info/exclude"), ".aaa/\n").unwrap();
    let before = r.snapshot();
    let cwd = r.dir.to_string_lossy().into_owned();

    let v = query(&cwd, Some("main"), &q(DiffScope::Branch, true)).await.unwrap();
    assert_eq!((v.base.as_str(), v.base_sha.as_str(), v.head.as_str()), ("main", main.as_str(), "aaa/demo/t-0001-x"));
    let rows: Vec<_> = v
        .files
        .as_ref()
        .unwrap()
        .iter()
        .map(|f| (f.path.as_str(), f.old_path.as_deref(), f.status, f.additions, f.deletions))
        .collect();
    assert_eq!(
        rows,
        [
            ("gone.txt", None, DiffStatus::Deleted, Some(0), Some(1)),
            ("keep.txt", None, DiffStatus::Modified, Some(2), Some(1)),
            ("new dir/fresh.md", None, DiffStatus::Untracked, Some(1), Some(0)),
            ("new name.txt", Some("old name.txt"), DiffStatus::Renamed, Some(0), Some(0)),
            ("src/lib.rs", None, DiffStatus::Added, Some(1), Some(0)),
        ]
    );
    let patch = v.patch.unwrap();
    assert!(patch.contains("diff --git a/keep.txt b/keep.txt") && patch.contains("+TWO"), "{patch}");
    assert!(patch.contains("+# hi") && !patch.contains("main-only") && !patch.contains("ignored"));
    assert!(!v.truncated && !v.untracked_omitted);

    // 미커밋은 HEAD 대비: 브랜치 커밋(src/lib.rs)은 없다
    let u = query(&cwd, Some("main"), &q(DiffScope::Uncommitted, false)).await.unwrap();
    let paths: Vec<_> = u.files.unwrap().into_iter().map(|f| f.path).collect();
    assert_eq!(paths, ["gone.txt", "keep.txt", "new dir/fresh.md", "new name.txt"]);
    assert_eq!(u.base, "HEAD");
    assert!(u.patch.is_none());

    // 파일 하나(공백 무시): 그 파일만
    let one = DiffQuery {
        file: Some(DiffFileRef { path: "keep.txt".into(), old_path: None }),
        ignore_whitespace: true,
        ..q(DiffScope::Branch, true)
    };
    let f = query(&cwd, Some("main"), &one).await.unwrap();
    let fp = f.patch.unwrap();
    assert!(fp.starts_with("diff --git a/keep.txt b/keep.txt") && !fp.contains("fresh.md"));
    assert!(f.files.is_none());

    // 없는 base는 다음 후보로(main)
    let g = query(&cwd, Some("origin/nope"), &q(DiffScope::Branch, false)).await.unwrap();
    assert_eq!(g.base, "main");

    let after = r.snapshot();
    assert!(after.index == before.index, "index가 바뀌었다");
    assert_eq!((&after.head, &after.refs), (&before.head, &before.refs), "HEAD·ref가 바뀌었다");
    let names = |s: &Snapshot| s.files.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
    let extra: Vec<_> = names(&after).into_iter().filter(|n| !names(&before).contains(n)).collect();
    assert_eq!(names(&after), names(&before), "파일 목록이 바뀌었다: {extra:?}");
    assert!(after.files == before.files, "파일 내용이 바뀌었다");
}

/// PID가 다시 배정돼도 이전 프로세스의 임시 index 폴더를 건드리지 않고 비교한다.
#[tokio::test]
async fn stale_pid_temp_index_never_blocks_a_new_process() {
    const CHILD: &str = "AAA_TEST_DIFF_PID_REUSE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // 새 시험 프로세스라 옛 실행기의 첫 순번은 0이다. 남의 잔여 폴더를 지우거나 재사용하면 안 된다.
        let legacy = std::env::temp_dir().join(format!("aaa-diff-{}-0", std::process::id()));
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("keep"), b"left by an earlier process").unwrap();
        let r = Repo::new();
        r.write("one.txt", "one\n");
        r.git(&["add", "-A"]);
        r.git(&["commit", "-qm", "first"]);
        r.write("one.txt", "two\n");
        let v = query(&r.dir.to_string_lossy(), None, &q(DiffScope::Uncommitted, true)).await.unwrap();
        assert!(v.patch.unwrap().contains("+two"));
        assert_eq!(std::fs::read(legacy.join("keep")).unwrap(), b"left by an earlier process");
        return;
    }
    // TMP 계열은 자식 환경에만 둔다. 다른 병렬 시험의 전역 환경·임시 폴더를 바꾸지 않는다.
    let root = std::env::temp_dir().join(format!("aaa-diff-pid-owner-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "diff::tests::stale_pid_temp_index_never_blocks_a_new_process", "--nocapture"])
        .env(CHILD, "1")
        .env("TMPDIR", &root)
        .env("TMP", &root)
        .env("TEMP", &root)
        .output()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// 큰 비교는 patch 없이 목록만 주고(웹이 파일마다 읽는다), 파일 하나도 frame 안으로 줄 경계에서 자른다.
#[tokio::test]
async fn large_changes_hand_over_to_per_file_reads_inside_the_frame() {
    let r = Repo::new();
    r.write("a.txt", "a\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-qm", "first"]);
    let line = format!("{}\n", "\"q\\".repeat(40));
    r.write("big.txt", &line.repeat(d::FILE_PATCH_MAX_JSON / line.len() + 10));
    r.write("small.txt", "s\n");
    let cwd = r.dir.to_string_lossy().into_owned();
    let v = query(&cwd, None, &q(DiffScope::Branch, true)).await.unwrap();
    assert!(v.truncated && v.patch.is_none());
    assert_eq!(v.files.unwrap().len(), 2);
    let one =
        DiffQuery { file: Some(DiffFileRef { path: "big.txt".into(), old_path: None }), ..q(DiffScope::Branch, true) };
    let f = query(&cwd, None, &one).await.unwrap();
    let p = f.patch.clone().unwrap();
    assert!(f.truncated && p.ends_with('\n') && d::json_len(&p) <= d::FILE_PATCH_MAX_JSON);
    let frame = serde_json::to_vec(&aaa_proto::AgentResponse::Diff(f)).unwrap();
    assert!(frame.len() < aaa_proto::MAX_FRAME, "{}", frame.len());
}

/// 저장소 설정의 외부 diff·textconv·fsmonitor를 부르지 않는다(`work_git_read_commands`와 같은 경계).
#[cfg(unix)]
#[tokio::test]
async fn repository_callbacks_never_run() {
    use std::os::unix::fs::PermissionsExt;
    let r = Repo::new();
    // Linux git의 초 단위 stat 비교를 OS·실행 속도와 무관하게 만든다.
    r.git(&["config", "core.checkStat", "minimal"]);
    r.git(&["config", "core.trustctime", "false"]);
    let stamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let set_mtime = |path: &Path| {
        std::fs::File::options().write(true).open(path).unwrap().set_modified(stamp).unwrap();
    };
    r.write("one.txt", "one\n");
    set_mtime(&r.dir.join("one.txt"));
    r.git(&["add", "-A"]);
    r.git(&["commit", "-qm", "first"]);
    // 같은 초의 add→덮어쓰기인 racy entry. 원본 git은 내용을 다시 검사한다.
    set_mtime(&r.dir.join(".git/index"));
    let marker = r.root.join("callback-ran");
    let callback = r.root.join("callback");
    std::fs::write(&callback, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&callback, std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["core.fsmonitor", "core.pager", "diff.external"] {
        r.git(&["config", name, callback.to_str().unwrap()]);
    }
    r.git(&["config", "diff.fixture.textconv", callback.to_str().unwrap()]);
    std::fs::write(r.dir.join(".git/info/attributes"), "*.txt diff=fixture\n").unwrap();
    r.write("one.txt", "two\n");
    set_mtime(&r.dir.join("one.txt"));
    r.write("new.txt", "new\n");
    let index = r.dir.join(".git/index");
    let before = std::fs::read(&index).unwrap();
    let v = query(&r.dir.to_string_lossy(), None, &q(DiffScope::Uncommitted, true)).await.unwrap();
    assert!(v.patch.unwrap().contains("+two"));
    assert!(!marker.exists(), "외부 callback을 실행하면 안 된다");
    assert_eq!(std::fs::read(&index).unwrap(), before);
    assert_eq!(std::fs::metadata(&index).unwrap().modified().unwrap(), stamp);
}

#[tokio::test]
async fn folders_outside_git_are_refused() {
    let root = std::env::temp_dir().join(format!("aaa-diff-plain-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let e = query(&root.to_string_lossy(), None, &q(DiffScope::Branch, false)).await.unwrap_err();
    assert!(format!("{e:#}").contains("git 저장소가 아니다"), "{e:#}");
    std::fs::remove_dir_all(root).unwrap();
    assert!(query("relative/dir", None, &q(DiffScope::Branch, false)).await.is_err());
}
