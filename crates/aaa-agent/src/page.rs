//! 파일 보기의 HTML 페이지(agent가 쓴 보고·차트, 참고 herdr-web-ui `server/file-view.ts` `pageWithLocalFiles`·`pageFile`).
//!
//! - 페이지는 origin 없는 sandbox에서 제 script로 돈다. 아무 데도 묻지 못한다(host `PAGE_CSP`: 이 서버에도 웹에도).
//! - 그래서 페이지가 제 폴더에서 가져오는 그림·동영상·소리·stylesheet·script를 보내기 전에 data: URL로 넣는다.
//!   요소는 제 종류의 파일만 받는다(그림 자리에 `.env`·열쇠를 적어도 넣지 않는다). 폴더 밖·숨은 이름(`.ssh/`)은 읽지 않는다.
//! - 참고는 lol-html(HTMLRewriter)로 고친다. aaa는 시작 태그만 훑는 작은 tokenizer다: 주석·`<!…>`·script·style 안은
//!   건너뛰고, 고칠 속성 값 자리만 바꾸며 나머지 bytes는 그대로 둔다.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use base64::Engine;

use crate::files::{Roots, mime_of};

/// 넣는 파일 하나·모두의 크기 상한, 이보다 큰 페이지는 넣지 않고 그대로, 찾아보는 서로 다른 주소 수(참고와 같다)
const EMBED_FILE_BYTES: u64 = 8 * 1024 * 1024;
const EMBED_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub const EMBED_PAGE_BYTES: u64 = 8 * 1024 * 1024;
const EMBED_REFS: usize = 1000;

/// 요소가 받는 파일 종류
#[derive(Clone, Copy)]
enum Accepts {
    Media,
    Script,
    Style,
}

impl Accepts {
    fn takes(self, mime: &str) -> bool {
        match self {
            Accepts::Media => mime.starts_with("image/") || mime.starts_with("video/") || mime.starts_with("audio/"),
            Accepts::Script => mime == "text/javascript" || mime == "application/javascript",
            Accepts::Style => mime == "text/css",
        }
    }
}

/// 확장자의 MIME(모르면 octet-stream: 어느 요소도 받지 않는다)
fn mime(path: &Path) -> &'static str {
    path.file_name().and_then(|n| n.to_str()).and_then(mime_of).unwrap_or("application/octet-stream")
}

/// 페이지가 상대 주소로 적은 파일: 페이지 폴더 아래(symlink를 따라간 뒤에도)이고 가는 길에 숨은 이름이 없는 일반 파일.
/// scheme·host·뿌리 경로·`#`로 시작하는 주소, 폴더 밖은 None(참고 `pageFile`).
fn page_file(reference: &str, folder: &Path, roots: &Roots) -> Option<PathBuf> {
    let value = reference.trim();
    let scheme = value.split_once(':').is_some_and(|(s, _)| {
        let mut c = s.chars();
        c.next().is_some_and(|f| f.is_ascii_alphabetic())
            && c.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-'))
    });
    if value.is_empty() || scheme || value.starts_with(['/', '\\', '#']) {
        return None;
    }
    let relative = percent_decode(value.split(['?', '#']).next()?)?;
    let drive = relative.len() >= 2 && relative.as_bytes()[0].is_ascii_alphabetic() && relative.as_bytes()[1] == b':';
    if relative.is_empty() || relative.starts_with(['/', '\\']) || drive || Path::new(&relative).is_absolute() {
        return None;
    }
    // 글자로 먼저 접는다(`a/../b`): 폴더 위로 나가면 그 자리에서 끝
    let mut target = folder.to_path_buf();
    for part in Path::new(&relative).components() {
        match part {
            Component::Normal(p) => target.push(p),
            Component::CurDir => {}
            Component::ParentDir if target != folder => {
                target.pop();
            }
            _ => return None,
        }
    }
    if target == folder {
        return None;
    }
    let file = target
        .canonicalize()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    let inside = file
        .strip_prefix(folder)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    if inside.as_os_str().is_empty() || inside.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
        return None;
    }
    (roots.allows(&file)
        && std::fs::metadata(&file)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?
            .is_file())
    .then_some(file)
}

/// `decodeURIComponent`: 잘못된 `%`나 UTF-8이 아니면 None
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?;
            out.push(
                u8::from_str_radix(hex, 16)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()?,
            );
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
}

/// 넣을 파일을 고르고 읽는다: 주소마다 한 번 찾고, 크기 상한을 센다
struct Embedder<'a> {
    folder: &'a Path,
    roots: &'a Roots,
    left: u64,
    resolved: HashMap<String, Option<PathBuf>>,
}

impl Embedder<'_> {
    fn data_url(&mut self, reference: &str, accepts: Accepts) -> Option<String> {
        if !self.resolved.contains_key(reference) {
            let file =
                if self.resolved.len() < EMBED_REFS { page_file(reference, self.folder, self.roots) } else { None };
            self.resolved.insert(reference.to_string(), file);
        }
        let file = self.resolved.get(reference)?.clone()?;
        let mime = mime(&file);
        if !accepts.takes(mime) {
            return None;
        }
        let size = std::fs::metadata(&file)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?
            .len();
        if size > EMBED_FILE_BYTES || size > self.left {
            return None;
        }
        let bytes = std::fs::read(&file)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        self.left = self.left.saturating_sub(bytes.len() as u64);
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}

/// 시작 태그의 속성 하나: 소문자 이름과 값 자리(따옴표 안, 없으면 None)
struct Attr {
    name: String,
    value: Option<(usize, usize)>,
    quoted: bool,
}

/// 돌아가는 script인가: type이 없거나 JavaScript·module(참고 `RUNS`). 데이터 블록(`type="text/plain"`)은 파일을 받지 않는다
fn runs(kind: Option<&str>) -> bool {
    let Some(t) = kind else { return true };
    let t = t.trim().to_ascii_lowercase();
    t.is_empty()
        || t == "module"
        || ["text/", "application/"].iter().any(|p| {
            t.strip_prefix(p).is_some_and(|rest| {
                let rest = rest.strip_prefix("x-").unwrap_or(rest);
                rest == "javascript" || rest == "ecmascript"
            })
        })
}

/// 페이지 글에서 제 폴더 파일을 data: URL로 넣은 글
pub fn with_local_files(html: &str, folder: &Path, roots: &Roots) -> String {
    let mut embed = Embedder { folder, roots, left: EMBED_TOTAL_BYTES, resolved: HashMap::new() };
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(off) = html[i..].find('<') {
        let at = i + off;
        let rest = &html[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            i = comment.find("-->").map_or(html.len(), |e| at + 4 + e + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") || rest.starts_with("</") {
            i = rest.find('>').map_or(html.len(), |e| at + e + 1);
            continue;
        }
        if !b.get(at + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            i = at + 1;
            continue;
        }
        let Some((name, attrs, end)) = start_tag(html, at) else { break };
        let get = |n: &str| attrs.iter().find(|a| a.name == n);
        let value = |a: &Attr| a.value.map(|(s, e)| &html[s..e]);
        let targets: Vec<(&Attr, Accepts)> = match name.as_str() {
            "img" | "source" | "audio" => get("src").map(|a| (a, Accepts::Media)).into_iter().collect(),
            "video" => ["src", "poster"].iter().filter_map(|n| get(n).map(|a| (a, Accepts::Media))).collect(),
            "script" if runs(get("type").and_then(value)) => {
                get("src").map(|a| (a, Accepts::Script)).into_iter().collect()
            }
            "link"
                if get("rel")
                    .and_then(value)
                    .is_some_and(|r| r.split_ascii_whitespace().any(|t| t.eq_ignore_ascii_case("stylesheet"))) =>
            {
                get("href").map(|a| (a, Accepts::Style)).into_iter().collect()
            }
            _ => Vec::new(),
        };
        let mut edits: Vec<(usize, usize, String)> = targets
            .into_iter()
            .filter_map(|(a, kind)| {
                let (s, e) = a.value?;
                let url = embed.data_url(&html[s..e], kind)?;
                Some(if a.quoted { (s, e, url) } else { (s, e, format!("\"{url}\"")) })
            })
            .collect();
        edits.sort_by_key(|e| e.0);
        for (s, e, url) in edits {
            out.push_str(&html[copied..s]);
            out.push_str(&url);
            copied = e;
        }
        i = end;
        // script·style·textarea·title 안은 글이다(태그가 아니다): 같은 이름의 끝 태그까지 건너뛴다
        if matches!(name.as_str(), "script" | "style" | "textarea" | "title") {
            let close = format!("</{name}");
            i = find_ascii_ci(html, end, &close).unwrap_or(html.len());
        }
    }
    out.push_str(&html[copied..]);
    out
}

/// `from`부터 대소문자 없이 `needle`(ASCII)이 처음 나오는 자리
fn find_ascii_ci(hay: &str, from: usize, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    (from..h.len().saturating_sub(n.len() - 1)).find(|&k| h[k..k + n.len()].eq_ignore_ascii_case(n))
}

/// `<`에서 시작하는 시작 태그: 소문자 이름, 속성, `>` 다음 자리. 끝나지 않으면 None
fn start_tag(html: &str, at: usize) -> Option<(String, Vec<Attr>, usize)> {
    let b = html.as_bytes();
    let space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
    let mut k = at + 1;
    while k < b.len() && !space(b[k]) && b[k] != b'/' && b[k] != b'>' {
        k += 1;
    }
    let name = html[at + 1..k].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while k < b.len() && (space(b[k]) || b[k] == b'/') {
            k += 1;
        }
        if *b.get(k)? == b'>' {
            return Some((name, attrs, k + 1));
        }
        // 속성 이름(첫 글자는 `=`여도 이름이다)
        let s = k;
        k += 1;
        while k < b.len() && !space(b[k]) && !matches!(b[k], b'/' | b'>' | b'=') {
            k += 1;
        }
        let attr_name = html[s..k].to_ascii_lowercase();
        let mut j = k;
        while j < b.len() && space(b[j]) {
            j += 1;
        }
        let (value, quoted) = if b.get(j) == Some(&b'=') {
            j += 1;
            while j < b.len() && space(b[j]) {
                j += 1;
            }
            match b.get(j)? {
                q @ (b'"' | b'\'') => {
                    let e = j + 1 + html[j + 1..].find(*q as char)?;
                    k = e + 1;
                    (Some((j + 1, e)), true)
                }
                _ => {
                    let vs = j;
                    while j < b.len() && !space(b[j]) && b[j] != b'>' {
                        j += 1;
                    }
                    k = j;
                    (Some((vs, j)), false)
                }
            }
        } else {
            (None, false)
        };
        // 같은 이름이 또 나오면 앞의 것이 속성이다(HTML)
        if !attrs.iter().any(|a: &Attr| a.name == attr_name) {
            attrs.push(Attr { name: attr_name, value, quoted });
        }
    }
}

/// 실제 경로의 페이지를 보낼 bytes로: 크면 그대로, 아니면 제 폴더 파일을 넣는다
pub fn render(real: &Path, roots: &Roots) -> anyhow::Result<Vec<u8>> {
    let bytes = std::fs::read(real)?;
    if bytes.len() as u64 > EMBED_PAGE_BYTES {
        return Ok(bytes);
    }
    let folder = real.parent().ok_or_else(|| anyhow::anyhow!("폴더가 없다"))?.canonicalize()?;
    Ok(with_local_files(&String::from_utf8_lossy(&bytes), &folder, roots).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(tag: &str) -> (PathBuf, PathBuf, Roots) {
        let base = std::env::temp_dir().join(format!("aaa-page-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let site = base.join("site");
        for d in ["img", ".hidden", "deep/er"] {
            std::fs::create_dir_all(site.join(d)).unwrap();
        }
        std::fs::write(site.join("pic.png"), [137, 80, 78, 71]).unwrap();
        std::fs::write(site.join("img/a b.png"), [1, 2]).unwrap();
        std::fs::write(site.join("app.js"), "x=1").unwrap();
        std::fs::write(site.join("style.css"), "b{}").unwrap();
        std::fs::write(site.join(".env"), "KEY=1").unwrap();
        std::fs::write(site.join(".hidden/pic.png"), [9]).unwrap();
        std::fs::write(site.join("notes.txt"), "글").unwrap();
        std::fs::write(base.join("up.png"), [7]).unwrap();
        let roots = Roots::new([base.clone()]);
        (base.canonicalize().unwrap(), site.canonicalize().unwrap(), roots)
    }

    fn data(mime: &str, bytes: &[u8]) -> String {
        format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    #[test]
    fn puts_the_folders_own_files_into_the_page() {
        let (base, site, roots) = lab("own");
        let png = data("image/png", &[137, 80, 78, 71]);
        let page = |html: &str| with_local_files(html, &site, &roots);
        // 요소마다 제 종류: 그림·script·stylesheet, 따옴표 없는 값, 대소문자, 같은 주소 둘
        assert_eq!(
            page(r#"<IMG Src="pic.png"><img src=pic.png alt=x>"#),
            format!(r#"<IMG Src="{png}"><img src="{png}" alt=x>"#)
        );
        assert_eq!(
            page(r#"<script src="app.js"></script>"#),
            format!(r#"<script src="{}"></script>"#, data("text/javascript", b"x=1"))
        );
        assert_eq!(
            page(r#"<script type="module" src="./app.js">"#),
            format!(r#"<script type="module" src="{}">"#, data("text/javascript", b"x=1"))
        );
        assert_eq!(
            page(r#"<link rel="preload stylesheet" media="print" href="style.css">"#),
            format!(r#"<link rel="preload stylesheet" media="print" href="{}">"#, data("text/css", b"b{}"))
        );
        assert_eq!(page(r#"<video poster="pic.png" src="pic.png">"#), format!(r#"<video poster="{png}" src="{png}">"#));
        // 퍼센트 인코딩·질의·조각, 폴더 안 `..`
        assert_eq!(
            page(r#"<img src="img/a%20b.png?v=2#x">"#),
            format!(r#"<img src="{}">"#, data("image/png", &[1, 2]))
        );
        assert_eq!(page(r#"<img src="deep/er/../../pic.png">"#), format!(r#"<img src="{png}">"#));
        // 넣지 않는 것: 다른 종류(그림 자리의 .env·script 자리의 css·데이터 블록 script·stylesheet 아닌 link),
        // 숨은 이름, 폴더 밖, 뿌리·scheme·조각 주소, 없는 파일, 잘못된 퍼센트
        for kept in [
            r#"<img src=".env">"#,
            r#"<img src="style.css">"#,
            r#"<script src="style.css"></script>"#,
            r#"<script type="text/plain" src="app.js"></script>"#,
            r#"<link rel="icon" href="pic.png">"#,
            r#"<a href="pic.png">"#,
            r#"<img src=".hidden/pic.png">"#,
            r#"<img src="../up.png">"#,
            r#"<img src="deep/../../up.png">"#,
            r#"<img src="/etc/hosts">"#,
            r#"<img src="file:///etc/hosts">"#,
            r#"<img src="https://example.com/a.png">"#,
            r##"<img src="#pic.png">"##,
            r#"<img src="nope.png">"#,
            r#"<img src="%E0%A4.png">"#,
            r#"<img src="notes.txt">"#,
        ] {
            assert_eq!(page(kept), kept);
        }
        // 주석·script 안 글·끝 태그·`<` 글자는 태그가 아니다
        for kept in [
            r#"<!-- <img src="pic.png"> -->"#,
            r#"<script>let s = '<img src="pic.png">'</script>"#,
            r#"<style>/* <img src="pic.png"> */</style>"#,
            r#"<p>1 < 2 <img</p>"#,
        ] {
            assert_eq!(page(kept), kept);
        }
        assert_eq!(
            page(r#"<SCRIPT>x</Script ><img src="pic.png">"#),
            format!(r#"<SCRIPT>x</Script ><img src="{png}">"#)
        );
        // 첫 속성이 이긴다, 끝나지 않은 태그는 그대로
        assert_eq!(page(r#"<img src="nope.png" src="pic.png">"#), r#"<img src="nope.png" src="pic.png">"#);
        assert_eq!(page(r#"<img src="pic.png""#), r#"<img src="pic.png""#);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_folder_are_not_read() {
        let (base, site, roots) = lab("link");
        std::os::unix::fs::symlink(base.join("up.png"), site.join("out.png")).unwrap();
        std::os::unix::fs::symlink(site.join(".env"), site.join("env.png")).unwrap();
        assert_eq!(with_local_files(r#"<img src="out.png">"#, &site, &roots), r#"<img src="out.png">"#);
        assert_eq!(with_local_files(r#"<img src="env.png">"#, &site, &roots), r#"<img src="env.png">"#);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn sizes_are_bounded() {
        let (base, site, roots) = lab("size");
        std::fs::write(site.join("big.png"), vec![0u8; (EMBED_FILE_BYTES + 1) as usize]).unwrap();
        assert_eq!(with_local_files(r#"<img src="big.png">"#, &site, &roots), r#"<img src="big.png">"#);
        // 모두 합친 상한: 8MiB 넷이면 다섯째는 그대로
        std::fs::write(site.join("max.png"), vec![0u8; EMBED_FILE_BYTES as usize]).unwrap();
        let html = r#"<img src="max.png">"#.repeat(5);
        let out = with_local_files(&html, &site, &roots);
        assert_eq!(out.matches("data:image/png").count(), 4);
        assert!(out.ends_with(r#"<img src="max.png">"#));
        // 큰 페이지는 넣지 않고 그대로
        let big = format!(r#"<img src="pic.png">{}"#, " ".repeat(EMBED_PAGE_BYTES as usize));
        std::fs::write(site.join("big.html"), &big).unwrap();
        assert_eq!(render(&site.join("big.html"), &roots).unwrap(), big.as_bytes());
        let _ = std::fs::remove_dir_all(base);
    }
}
