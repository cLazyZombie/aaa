//! 파일 보기(웹 터미널의 파일 경로 링크, 결정 20 D7, herdr-web-ui `server/file-view.ts`·`server/files.ts`).
//!
//! - 경로는 사람과 agent가 쓰는 대로 받는다: 절대, `~/…`, pane이 지금 있는 폴더 기준 상대.
//! - 상대 경로가 그대로 없으면 그 폴더 아래에서 끝이 같은 파일을 찾는다(참고 `filesNamed`: 깊이 6, 항목 5000,
//!   `.git`·`node_modules`·`dist`·`target`·`.venv`는 건너뜀, 10개까지). 하나면 그것, 여럿이면 고를 후보다.
//! - 참고는 서버 사용자가 읽을 수 있는 것을 다 연다. aaa는 §6.5대로 허용 폴더(`[agent] file_roots`) 안만 연다.
//!   기본은 집·작업 root·기본 cwd·임시 폴더다. symlink는 따라간 실제 경로로 본다.
//! - 일반 파일만 연다. 종류는 확장자로 정하고, 모르는 확장자는 앞 8KiB에 NUL이 있으면 binary다.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use aaa_proto::{FileInfo, FileKind};

/// 찾는 동안 내려가는 깊이·보는 항목 수·돌려주는 후보 수(참고 files.ts)
const WALK_DEPTH: usize = 6;
const WALK_ENTRIES: usize = 5000;
const CANDIDATES: usize = 10;
const SKIP_DIRS: [&str; 5] = [".git", "node_modules", "dist", "target", ".venv"];
/// 종류를 모르는 파일에서 NUL을 찾는 앞부분
const SNIFF_BYTES: usize = 8192;
/// 한 번에 읽는 조각 상한
pub const MAX_CHUNK: u32 = 512 * 1024;

/// 열어도 되는 폴더들(실제 경로)
#[derive(Debug, Clone, Default)]
pub struct Roots(Vec<PathBuf>);

impl Roots {
    /// 있는 폴더만 실제 경로로 둔다(macOS `/tmp` → `/private/tmp`)
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut v: Vec<PathBuf> = paths.into_iter().filter_map(|p| p.canonicalize().ok()).collect();
        v.sort();
        v.dedup();
        Roots(v)
    }

    fn allows(&self, real: &Path) -> bool {
        self.0.iter().any(|r| real.starts_with(r))
    }
}

/// 경로를 찾은 결과
#[derive(Debug, PartialEq)]
pub enum Located {
    Found(FileInfo),
    /// 끝이 같은 파일이 여럿이다(실제 경로)
    Candidates(Vec<String>),
    /// 읽을 수 있는 일반 파일이 없다
    Missing,
    /// 있지만 허용 폴더 밖이다
    Outside,
}

/// 사람이 쓴 경로를 절대 경로로(참고 `resolveFilePath`). 상대 경로인데 폴더를 모르면 None.
pub fn resolve(input: &str, cwd: Option<&Path>, home: &Path) -> Option<PathBuf> {
    let path = input.trim();
    if path.is_empty() {
        return None;
    }
    if path == "~" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(home.join(rest));
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    cwd.map(|c| c.join(p))
}

/// 경로가 뜻하는 파일(참고 `locateFile`): 그대로, 아니면 상대 경로면 폴더 아래 끝이 같은 파일
pub fn locate(input: &str, cwd: Option<&Path>, home: &Path, roots: &Roots) -> Located {
    let mut outside = false;
    if let Some(path) = resolve(input, cwd, home) {
        match checked(&path, roots) {
            Some(Ok(info)) => return Located::Found(info),
            Some(Err(())) => outside = true,
            None => {}
        }
    }
    let rel = input.trim();
    let relative = !rel.is_empty() && !Path::new(rel).is_absolute() && !rel.starts_with('~') && !rel.starts_with("../");
    if let (Some(cwd), true, false) = (cwd, relative, outside) {
        let found: Vec<PathBuf> = files_named(cwd, rel)
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .filter(|p| roots.allows(p))
            .collect();
        match found.as_slice() {
            [one] => {
                if let Some(Ok(info)) = checked(one, roots) {
                    return Located::Found(info);
                }
            }
            [] => {}
            many => return Located::Candidates(many.iter().map(|p| p.to_string_lossy().into_owned()).collect()),
        }
    }
    if outside { Located::Outside } else { Located::Missing }
}

/// 있는 일반 파일이면 허용 폴더 안인지까지 본다. 없으면 None, 밖이면 Err.
fn checked(path: &Path, roots: &Roots) -> Option<Result<FileInfo, ()>> {
    let real = path.canonicalize().ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    if !meta.is_file() {
        return None;
    }
    if !roots.allows(&real) {
        return Some(Err(()));
    }
    Some(Ok(info(&real, &meta)))
}

fn info(real: &Path, meta: &std::fs::Metadata) -> FileInfo {
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mime = mime_of(&name);
    let kind = match mime {
        Some(m) => kind_of(m),
        None if sniff_binary(real) => FileKind::Binary,
        None => FileKind::Text,
    };
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    FileInfo {
        path: real.to_string_lossy().into_owned(),
        name,
        size: meta.len(),
        modified_ms,
        mime: mime.unwrap_or(if kind == FileKind::Text { "text/plain" } else { "application/octet-stream" }).into(),
        kind,
    }
}

/// 확장자로 정하는 MIME(모르면 None: 내용으로 본다)
fn mime_of(name: &str) -> Option<&'static str> {
    let (_, ext) = name.rsplit_once('.')?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "js" | "mjs" | "cjs" => "text/javascript",
        "css" => "text/css",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "sh" => "application/x-sh",
        "sql" => "application/sql",
        "txt" | "log" | "rs" | "ts" | "tsx" | "jsx" | "py" | "go" | "rb" | "java" | "kt" | "swift" | "c" | "h"
        | "cc" | "cpp" | "hpp" | "cs" | "php" | "lua" | "zig" | "diff" | "patch" | "ini" | "conf" | "env" | "lock" => {
            "text/plain"
        }
        "zip" | "gz" | "tgz" | "tar" | "7z" | "bz2" | "xz" => "application/octet-stream",
        "wasm" | "so" | "dylib" | "dll" | "exe" | "bin" | "o" | "a" => "application/octet-stream",
        _ => return None,
    })
}

fn kind_of(mime: &str) -> FileKind {
    if mime.starts_with("image/") {
        FileKind::Image
    } else if mime.starts_with("video/") {
        FileKind::Video
    } else if mime.starts_with("audio/") {
        FileKind::Audio
    } else if mime == "application/pdf" {
        FileKind::Pdf
    } else if mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/toml"
                | "application/x-sh"
                | "application/sql"
        )
    {
        FileKind::Text
    } else {
        FileKind::Binary
    }
}

fn sniff_binary(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else { return true };
    let mut buf = vec![0u8; SNIFF_BYTES];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return true,
        }
    }
    buf[..n].contains(&0)
}

/// 폴더 아래에서 상대 경로가 같거나 `/<경로>`로 끝나는 파일(참고 `filesNamed`)
fn files_named(cwd: &Path, rel: &str) -> Vec<PathBuf> {
    let suffix: PathBuf = Path::new(rel.trim_start_matches("./")).components().collect();
    if suffix.as_os_str().is_empty() || suffix.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = 0usize;
    let mut stack = vec![(cwd.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            seen += 1;
            if seen > WALK_ENTRIES {
                out.sort();
                return out;
            }
            let Ok(t) = e.file_type() else { continue };
            let path = e.path();
            if t.is_dir() {
                let name = e.file_name();
                if depth + 1 < WALK_DEPTH && !SKIP_DIRS.iter().any(|s| name == *s) {
                    stack.push((path, depth + 1));
                }
            } else if t.is_file() && path.strip_prefix(cwd).is_ok_and(|r| r.ends_with(&suffix)) {
                out.push(path);
                if out.len() >= CANDIDATES {
                    break;
                }
            }
        }
        if out.len() >= CANDIDATES {
            break;
        }
    }
    out.sort();
    out
}

/// 찾은 파일(실제 경로) 한 조각. 그 사이 바뀌었으면(실제 경로가 다르거나 허용 폴더 밖·일반 파일 아님) 읽지 않는다.
pub fn read(path: &str, offset: u64, len: u32, roots: &Roots) -> anyhow::Result<(Vec<u8>, bool)> {
    let p = Path::new(path);
    anyhow::ensure!(p.is_absolute(), "절대 경로가 아니다");
    let real = p.canonicalize()?;
    anyhow::ensure!(real == p, "실제 경로가 바뀌었다");
    anyhow::ensure!(roots.allows(&real), "허용 폴더 밖이다");
    let mut f = File::open(&real)?;
    let meta = f.metadata()?;
    anyhow::ensure!(meta.is_file(), "일반 파일이 아니다");
    f.seek(SeekFrom::Start(offset))?;
    let want = len.min(MAX_CHUNK) as usize;
    let mut buf = vec![0u8; want];
    let mut n = 0;
    while n < want {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    buf.truncate(n);
    Ok((buf, offset + n as u64 >= meta.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험 폴더: home(허용) 아래 프로젝트, 허용 밖 폴더 하나
    fn lab(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("aaa-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let proj = home.join("proj");
        let outside = base.join("outside");
        for d in [proj.join("docs/shots"), proj.join("src"), proj.join("node_modules/x/docs"), outside.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(proj.join("README.md"), "# 제목\n").unwrap();
        std::fs::write(proj.join("docs/demo.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(proj.join("docs/shots/a.log"), "plain\n").unwrap();
        std::fs::write(proj.join("src/a.log"), "other\n").unwrap();
        std::fs::write(proj.join("node_modules/x/docs/deep.txt"), "skip\n").unwrap();
        std::fs::write(proj.join("blob.dat"), [1u8, 0, 2]).unwrap();
        std::fs::write(outside.join("secret.txt"), "no\n").unwrap();
        (base, home, outside)
    }

    fn found(l: Located) -> FileInfo {
        match l {
            Located::Found(i) => i,
            other => panic!("찾지 못했다: {other:?}"),
        }
    }

    #[test]
    fn locates_like_the_reference() {
        let (base, home, outside) = lab("locate");
        let roots = Roots::new([home.clone()]);
        let proj = home.join("proj");
        let real = |p: &Path| p.canonicalize().unwrap().to_string_lossy().into_owned();
        // 상대(pane 폴더 기준)·절대·~/
        let i = found(locate("README.md", Some(&proj), &home, &roots));
        assert_eq!(
            (i.name.as_str(), i.kind, i.mime.as_str(), i.size),
            ("README.md", FileKind::Text, "text/markdown", 9)
        );
        assert_eq!(i.path, real(&proj.join("README.md")));
        assert_eq!(
            found(locate(&proj.join("docs/demo.png").to_string_lossy(), None, &home, &roots)).kind,
            FileKind::Image
        );
        assert_eq!(found(locate("~/proj/README.md", None, &home, &roots)).path, i.path);
        // 그대로 없는 상대 경로는 아래에서 끝이 같은 파일 하나(참고: agent는 docs/screenshots/를 빼고 쓰곤 한다)
        assert_eq!(found(locate("demo.png", Some(&proj), &home, &roots)).path, real(&proj.join("docs/demo.png")));
        // 여럿이면 후보(node_modules 아래는 보지 않는다)
        assert_eq!(
            locate("a.log", Some(&proj), &home, &roots),
            Located::Candidates(vec![real(&proj.join("docs/shots/a.log")), real(&proj.join("src/a.log"))])
        );
        assert_eq!(locate("deep.txt", Some(&proj), &home, &roots), Located::Missing);
        // 모르는 확장자는 내용으로: NUL이 있으면 binary
        assert_eq!(found(locate("blob.dat", Some(&proj), &home, &roots)).kind, FileKind::Binary);
        // 없는 것·폴더는 없다. `../`로 시작하는 상대 경로는 쓴 대로만 찾고 아래로 찾아 내려가지 않는다
        assert_eq!(locate("nope.md", Some(&proj), &home, &roots), Located::Missing);
        assert_eq!(locate("docs", Some(&proj), &home, &roots), Located::Missing);
        assert_eq!(found(locate("../README.md", Some(&proj.join("src")), &home, &roots)).path, i.path);
        assert_eq!(locate("../demo.png", Some(&proj.join("src")), &home, &roots), Located::Missing);
        // 허용 폴더 밖은 열지 않는다
        assert_eq!(locate(&outside.join("secret.txt").to_string_lossy(), None, &home, &roots), Located::Outside);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_judged_by_their_target() {
        let (base, home, outside) = lab("link");
        let roots = Roots::new([home.clone()]);
        let proj = home.join("proj");
        // 허용 폴더 안 symlink가 밖을 가리키면 밖이다. 찾아 내려갈 때도 걸러 낸다
        std::os::unix::fs::symlink(outside.join("secret.txt"), proj.join("leak.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, proj.join("out")).unwrap();
        assert_eq!(locate("leak.txt", Some(&proj), &home, &roots), Located::Outside);
        assert_eq!(locate("out/secret.txt", Some(&proj), &home, &roots), Located::Outside);
        assert_eq!(locate("secret.txt", Some(&proj), &home, &roots), Located::Missing);
        // 읽기도 실제 경로가 허용 폴더 안이고 찾을 때와 같아야 한다
        let readme = found(locate("README.md", Some(&proj), &home, &roots)).path;
        assert_eq!(read(&readme, 2, 100, &roots).unwrap(), ("제목\n".as_bytes().to_vec(), true));
        assert!(read(&outside.join("secret.txt").to_string_lossy(), 0, 10, &roots).is_err());
        assert!(
            read(&proj.join("leak.txt").to_string_lossy(), 0, 10, &roots).is_err(),
            "symlink 경로 그대로는 읽지 않는다"
        );
        assert!(read("README.md", 0, 10, &roots).is_err(), "상대 경로");
        // 찾은 뒤 바뀐 파일(symlink로 바꿔치기)은 읽지 않는다: 허용 폴더 안의 다른 파일을 가리켜도(읽기 경로는 그 파일 하나다)
        std::fs::write(home.join("private.txt"), "다른 파일").unwrap();
        std::fs::remove_file(proj.join("README.md")).unwrap();
        std::os::unix::fs::symlink(home.join("private.txt"), proj.join("README.md")).unwrap();
        assert!(read(&readme, 0, 10, &roots).is_err(), "허용 폴더 안으로 바꿔치기");
        std::fs::remove_file(proj.join("README.md")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), proj.join("README.md")).unwrap();
        assert!(read(&readme, 0, 10, &roots).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn reads_in_chunks() {
        let (base, home, _) = lab("read");
        let roots = Roots::new([home.clone()]);
        let p = home.join("proj/big.txt");
        std::fs::write(&p, vec![b'x'; (MAX_CHUNK as usize) + 10]).unwrap();
        let p = p.canonicalize().unwrap().to_string_lossy().into_owned();
        let (a, eof) = read(&p, 0, u32::MAX, &roots).unwrap();
        assert_eq!((a.len(), eof), (MAX_CHUNK as usize, false));
        let (b, eof) = read(&p, MAX_CHUNK as u64, MAX_CHUNK, &roots).unwrap();
        assert_eq!((b.len(), eof), (10, true));
        let _ = std::fs::remove_dir_all(base);
    }
}
