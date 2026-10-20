//! thread worktree 비교(diff.v1, plan §8.305)의 인수·출력 해석·상한. 명령 실행은 thread 머신 agent만 한다.
//!
//! 인수와 상한은 t3code 4b1024da `apps/server/src/vcs/GitVcsDriverCore.ts`를 따른다: `REVIEW_DIFF_ARGS`(:70-78),
//! 전체 patch 120,000바이트(:59-65), 새 파일 5,000개(:61-64). 파일 하나의 patch는 t3code가 1 MiB지만 aaa는 agent link
//! frame 상한(1 MiB, JSON)에 들어가야 해 JSON으로 적은 길이로 자른다.
use aaa_proto::{DiffFile, DiffStatus};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// 한 번에 그리는 비교 전체 patch의 상한(t3code `REVIEW_DIFF_PATCH_MAX_OUTPUT_BYTES`). 넘으면 웹이 파일마다 읽는다
pub const PATCH_MAX_BYTES: usize = 120_000;
/// 파일 하나의 patch를 JSON 글로 적은 길이 상한(frame 1 MiB 안에 다른 칸과 함께)
pub const FILE_PATCH_MAX_JSON: usize = 768 * 1024;
/// 무시하지 않은 새 파일 상한(t3code `REVIEW_UNTRACKED_MAX_FILES`). 넘으면 새 파일을 빼고 알린다
pub const UNTRACKED_MAX_FILES: usize = 5_000;
/// 답에 싣는 파일 목록 상한(frame 안). 넘으면 목록 없이 잘린 patch만 준다
pub const MAX_FILES: usize = 3_000;
/// git 출력(numstat·patch) 읽기 상한
pub const OUTPUT_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(20);
/// 첫 커밋 전 저장소의 비교 기준(git의 빈 tree)
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// 모든 git 호출 앞에 붙는다. 저장소 설정의 외부 명령·잠금·색·경로 따옴표가 끼지 않게 한다
/// (`aaa_proto::mission_git_read_commands`와 같은 경계, t3code는 `--no-ext-diff --no-textconv`).
pub fn git_prefix() -> Vec<String> {
    [
        "--no-pager",
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.quotePath=false",
        "-c",
        "gc.auto=0",
        "-c",
        "maintenance.auto=false",
        "-c",
        "color.ui=never",
    ]
    .iter()
    .map(|s| (*s).into())
    .collect()
}

/// t3code `REVIEW_DIFF_ARGS`(+ `--ignore-all-space`). `what`은 `--numstat`·`--name-status`·`--patch`
pub fn diff_args(what: &str, ignore_whitespace: bool, base_sha: &str, paths: &[String]) -> Vec<String> {
    let mut a: Vec<String> = [
        "diff",
        what,
        "-z",
        "--find-renames",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--minimal",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ]
    .iter()
    .filter(|s| !(what == "--patch" && **s == "-z"))
    .map(|s| (*s).into())
    .collect();
    if ignore_whitespace {
        a.push("--ignore-all-space".into());
    }
    a.push(base_sha.into());
    a.push("--".into());
    a.extend(paths.iter().map(|p| literal(p)));
    a
}

/// 경로를 저장소 맨 위 기준 글자 그대로의 pathspec으로(`*`·`:` 해석 없음)
pub fn literal(path: &str) -> String {
    format!(":(top,literal){path}")
}

/// branch 범위의 기준 후보: thread가 잡은 base, 그다음 t3code 차례(원격 기본 branch, `main`, `master`)
pub fn base_candidates(base: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    // `HEAD`는 thread worktree에서 자기 HEAD라 기준이 되지 못한다(분리된 checkout에서 시작한 thread)
    if let Some(b) = base.map(str::trim).filter(|b| valid_ref(b) && *b != "HEAD") {
        v.push(b.into());
    }
    for b in ["origin/HEAD", "origin/main", "origin/master", "main", "master"] {
        if !v.iter().any(|x| x == b) {
            v.push(b.into());
        }
    }
    v
}

/// rev-parse에 넘길 ref 이름(옵션·범위·reflog 식이 아니다)
pub fn valid_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 1024
        && !r.starts_with('-')
        && !r.contains("..")
        && !r.contains("@{")
        && !r.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
}

/// numstat 한 줄: (경로, 옛 경로, 추가, 삭제). 바이너리는 숫자가 없다
pub type NumstatRow = (String, Option<String>, Option<u64>, Option<u64>);

/// `git diff --numstat -z`: `추가\t삭제\t경로\0` 또는 이름 바뀜 `추가\t삭제\t\0옛\0새\0`. 바이너리는 `-\t-`
pub fn parse_numstat(out: &[u8]) -> Vec<NumstatRow> {
    let mut parts = out.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned());
    let mut v = Vec::new();
    while let Some(head) = parts.next() {
        if head.is_empty() {
            continue;
        }
        let mut f = head.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (f.next(), f.next(), f.next()) else { continue };
        let (path, old) = if path.is_empty() {
            let (Some(old), Some(new)) = (parts.next(), parts.next()) else { break };
            (new, Some(old))
        } else {
            (path.to_string(), None)
        };
        v.push((path, old, a.parse().ok(), d.parse().ok()));
    }
    v
}

/// `git diff --name-status -z`: `M\0경로\0`, `R100\0옛\0새\0`. 새 경로 → (상태, 옛 경로)
pub fn parse_name_status(out: &[u8]) -> BTreeMap<String, (DiffStatus, Option<String>)> {
    let mut parts = out.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned());
    let mut m = BTreeMap::new();
    while let Some(code) = parts.next() {
        let Some(letter) = code.chars().next() else { continue };
        let status = match letter {
            'A' => DiffStatus::Added,
            'D' => DiffStatus::Deleted,
            'R' => DiffStatus::Renamed,
            'C' => DiffStatus::Copied,
            'T' => DiffStatus::TypeChanged,
            'U' => DiffStatus::Unmerged,
            _ => DiffStatus::Modified,
        };
        if matches!(status, DiffStatus::Renamed | DiffStatus::Copied) {
            let (Some(old), Some(new)) = (parts.next(), parts.next()) else { break };
            m.insert(new, (status, Some(old)));
        } else {
            let Some(path) = parts.next() else { break };
            m.insert(path, (status, None));
        }
    }
    m
}

/// 숫자와 상태를 한 목록으로(경로 차례, t3code `Diffs.ts:35`). 새 파일(intent-to-add로 담은 것)은 `untracked`
pub fn files(
    numstat: Vec<NumstatRow>,
    status: &BTreeMap<String, (DiffStatus, Option<String>)>,
    untracked: &HashSet<String>,
) -> Vec<DiffFile> {
    let mut v: Vec<DiffFile> = numstat
        .into_iter()
        .map(|(path, old, additions, deletions)| {
            let (mut st, named_old) = status.get(&path).cloned().unwrap_or((DiffStatus::Modified, None));
            if st == DiffStatus::Added && untracked.contains(&path) {
                st = DiffStatus::Untracked;
            }
            DiffFile { old_path: old.or(named_old), path, status: st, additions, deletions }
        })
        .collect();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

/// `git ls-files -z` 출력의 경로들
pub fn nul_paths(out: &[u8]) -> Vec<String> {
    out.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

/// serde_json이 이 글을 JSON 문자열 안에 적는 길이(따옴표 제외)
pub fn json_len(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum()
}

/// patch를 줄 경계에서 자른다: 원래 길이(`max_bytes`)와 JSON 길이(`max_json`) 둘 다 안에. 잘렸으면 true
pub fn cut_patch(patch: String, max_bytes: usize, max_json: usize) -> (String, bool) {
    if patch.len() <= max_bytes && json_len(&patch) <= max_json {
        return (patch, false);
    }
    let (mut bytes, mut json, mut end) = (0usize, 0usize, 0usize);
    for line in patch.split_inclusive('\n') {
        bytes += line.len();
        json += json_len(line);
        if bytes > max_bytes || json > max_json {
            break;
        }
        end = bytes;
    }
    let mut patch = patch;
    patch.truncate(end);
    (patch, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_and_name_status_read_renames_binaries_and_odd_names() {
        let num = b"3\t1\tsrc/a.rs\0-\t-\timg.png\x000\t0\t\0old name.txt\0new\tname.txt\0";
        let st = b"M\0src/a.rs\0A\0img.png\0R100\0old name.txt\0new\tname.txt\0";
        let untracked: HashSet<String> = ["img.png".to_string()].into();
        let v = files(parse_numstat(num), &parse_name_status(st), &untracked);
        let rows: Vec<_> =
            v.iter().map(|f| (f.path.as_str(), f.old_path.as_deref(), f.status, f.additions, f.deletions)).collect();
        assert_eq!(
            rows,
            [
                ("img.png", None, DiffStatus::Untracked, None, None),
                ("new\tname.txt", Some("old name.txt"), DiffStatus::Renamed, Some(0), Some(0)),
                ("src/a.rs", None, DiffStatus::Modified, Some(3), Some(1)),
            ]
        );
    }

    #[test]
    fn patch_is_cut_at_a_line_inside_both_limits() {
        let p = "a\nbb\nccc\n".to_string();
        assert_eq!(cut_patch(p.clone(), 100, 100), (p.clone(), false));
        assert_eq!(cut_patch(p.clone(), 6, 100), ("a\nbb\n".into(), true));
        // 줄바꿈은 JSON에서 두 글자다: "a\n"(3) + "bb\n"(4) = 7
        assert_eq!(cut_patch(p.clone(), 100, 6), ("a\n".into(), true));
        assert_eq!(json_len("\u{1b}\"é"), 6 + 2 + 2);
        let long = "x".repeat(10);
        assert_eq!(cut_patch(long, 5, 100), (String::new(), true));
    }

    #[test]
    fn base_candidates_refuse_options_and_expressions() {
        assert_eq!(base_candidates(Some("origin/main"))[0], "origin/main");
        assert_eq!(base_candidates(Some("origin/main")).iter().filter(|b| *b == "origin/main").count(), 1);
        for bad in ["--output=/tmp/x", "main..HEAD", "HEAD@{1}", "a b", "HEAD~1", "HEAD"] {
            assert_eq!(base_candidates(Some(bad))[0], "origin/HEAD", "{bad}");
        }
        let a = diff_args("--patch", true, EMPTY_TREE, &["a*b.rs".into()]);
        assert!(!a.contains(&"-z".to_string()));
        assert!(a.contains(&"--ignore-all-space".to_string()));
        assert_eq!(a[a.len() - 3..], [EMPTY_TREE.to_string(), "--".into(), ":(top,literal)a*b.rs".into()]);
    }
}
