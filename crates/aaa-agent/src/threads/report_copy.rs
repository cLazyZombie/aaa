//! 보고 연결·작은 근거부터 고른다. 내용은 선택 뒤에만 읽어 큰 중간 산출물의 I/O를 피한다.
use super::*;
use std::collections::HashSet;

pub(super) struct Candidate {
    pub path: String,
    pub size: u64,
    pub absolute: PathBuf,
}

fn small_record(path: &str, size: u64) -> bool {
    let ext = Path::new(path).extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    size <= 1 << 20
        && matches!(
            ext.as_str(),
            "md" | "txt"
                | "log"
                | "json"
                | "jsonl"
                | "ndjson"
                | "csv"
                | "tsv"
                | "toml"
                | "yaml"
                | "yml"
                | "html"
                | "htm"
                | "xml"
        )
}

// 선택의 힌트만 읽는다. Markdown 링크·참조 정의·HTML 속성·본문의 library 경로를 받으며
// 파일 열기 권한이나 경로 검증을 대신하지 않는다. 원문 보고·hash는 바꾸지 않는다.
fn references(report: &[u8]) -> HashSet<String> {
    use std::sync::LazyLock;
    static PATHS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"<(?:\./)?library/([^<>\r\n]+)>|(?:^|[\s(\"'`=])(?:\./)?library/([^\s<>\"'`]+)"#).unwrap()
    });
    let text = String::from_utf8_lossy(report);
    let mut paths = HashSet::new();
    for capture in PATHS.captures_iter(&text) {
        let raw = capture.get(2).or_else(|| capture.get(1)).unwrap().as_str();
        let raw = raw.split(['?', '#']).next().unwrap();
        let Some(path) = crate::page::percent_decode(raw) else { continue };
        // 경로와 문장 끝 구두점을 뺀 꼴만 둔다. 긴 구두점도 메모리를 제곱으로 늘리지 않는다.
        paths.insert(path.trim_end_matches([')', ']', ',', ';', '.']).to_string());
        paths.insert(path);
    }
    paths
}

fn priority<'a>(file: &'a Candidate, linked: &HashSet<String>) -> (u8, u64, &'a str) {
    let referenced = linked.contains(&file.path);
    let tier = if referenced && file.size <= 1 << 20 {
        0
    } else if small_record(&file.path, file.size) {
        1
    } else if referenced {
        2
    } else {
        3
    };
    (tier, file.size, &file.path)
}

pub(super) fn select(lib: &Path, report: &[u8], max_bytes: u64) -> Result<(Vec<Candidate>, Vec<Skipped>)> {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    if std::fs::symlink_metadata(lib).is_ok_and(|m| m.is_dir()) {
        walk(lib, lib, &mut |path, meta, absolute| {
            if meta.is_file() {
                files.push(Candidate { path, size: meta.len(), absolute: absolute.into() });
            } else {
                let reason = if meta.file_type().is_symlink() {
                    "symlink는 따라가지 않는다"
                } else {
                    "일반 파일이 아니다"
                };
                skipped.push(Skipped { path, reason: reason.into(), lazy_size: None });
            }
        })?;
    }
    let linked = references(report);
    files.sort_by(|a, b| priority(a, &linked).cmp(&priority(b, &linked)));
    let mut total = 0u64;
    files.retain(|file| {
        if file.size <= max_bytes.saturating_sub(total) {
            total += file.size;
            true
        } else {
            skipped.push(Skipped {
                path: file.path.clone(),
                reason: format!("복사 상한({} MiB)을 넘는다. agent에 남는다", max_bytes >> 20),
                lazy_size: Some(file.size),
            });
            false
        }
    });
    Ok((files, skipped))
}

#[cfg(test)]
#[path = "report_copy/tests.rs"]
mod tests;
