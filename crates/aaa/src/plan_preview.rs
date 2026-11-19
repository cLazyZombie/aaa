//! 기획 편집 thread의 검토 사본. 본문과 모든 그림은 같은 Git 판에서 읽는다.
use anyhow::{Context, Result, bail, ensure};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn git(args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new("git").args(args).output().context("Git 실행")?;
    ensure!(result.status.success(), "git {} 실패: {}", args[0], String::from_utf8_lossy(&result.stderr).trim());
    Ok(result.stdout)
}

fn git_text(args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(args)?)?.trim_end().to_string())
}

fn commit(rev: &str) -> Result<String> {
    git_text(&["rev-parse", "--verify", "--end-of-options", &format!("{rev}^{{commit}}")])
}

fn escaped(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn asset_path(doc: &str, src: &str) -> Result<String> {
    ensure!(!src.starts_with('/') && !src.contains([':', '\\', '\0']), "저장소 상대 경로 그림만 담을 수 있다: {src}");
    let mut parts: Vec<_> = doc.split('/').collect();
    parts.pop();
    for p in src.split('/') {
        match p {
            "." | "" => {}
            ".." => {
                ensure!(parts.pop().is_some(), "저장소 밖 그림: {src}");
            }
            _ => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

fn blob(rev: &str, path: &str, limit: u64) -> Result<Vec<u8>> {
    let object = format!("{rev}:{path}");
    let size: u64 = git_text(&["cat-file", "-s", &object])?.parse()?;
    ensure!(size <= limit, "파일 크기 상한 초과: {path}");
    git(&["cat-file", "blob", &object])
}

/// 지정된 최종 커밋과 현재 worktree가 같은 내용이어야 그 자리에서 실행한 검사를 착지 근거로 쓸 수 있다.
fn clean_head(head: &str) -> Result<()> {
    ensure!(commit("HEAD")? == head, "착지 검사 HEAD와 지정한 최종 판이 다르다");
    ensure!(
        git(&["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
        "착지 검사 worktree에 미커밋 변경이 있다"
    );
    Ok(())
}

pub(crate) fn check(doc: &str, base: &str, head: &str, media: Option<&str>, command: &[String]) -> Result<()> {
    ensure!(aaa_proto::design_clean_path(doc).as_deref() == Some(doc), "문서 경로가 올바르지 않다");
    let base = commit(base)?;
    let head = commit(head)?;
    clean_head(&head)?;
    git(&["merge-base", "--is-ancestor", &base, &head])?;
    if let Some(media) = media {
        ensure!(aaa_proto::design_clean_path(media).as_deref() == Some(media), "design_media 경로가 올바르지 않다");
        let parent = Path::new(doc).parent().context("문서 폴더")?.join(media);
        if !git(&["--literal-pathspecs", "ls-tree", "--full-tree", &head, "--", doc])?.is_empty() {
            let text = String::from_utf8(blob(&head, doc, aaa_proto::DESIGN_MAX_TEXT)?)?;
            for event in Parser::new(&text) {
                if let Event::Start(Tag::Image { dest_url, .. }) = event {
                    let path = asset_path(doc, &dest_url)?;
                    let entry = |rev: &str| git(&["--literal-pathspecs", "ls-tree", "--full-tree", rev, "--", &path]);
                    ensure!(!entry(&head)?.is_empty(), "착지할 판에 그림이 없다: {path}");
                    if entry(&base)?.is_empty() {
                        ensure!(Path::new(&path).starts_with(&parent), "새 그림이 design_media 폴더 밖에 있다: {path}");
                    }
                }
            }
        }
    }
    if let Some((exe, args)) = command.split_first() {
        let status = Command::new(exe)
            .args(args.iter().map(|a| a.replace("{base}", &base).replace("{head}", &head)))
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .status()
            .with_context(|| format!("저장소 문서 검사 실행: {exe}"))?;
        ensure!(status.success(), "저장소 문서 검사 실패: {exe} ({status})");
    }
    clean_head(&head)
}

struct Section {
    title: String,
    body: String,
}

fn sections(rev: &str, doc: &str, output: &Path) -> Result<Vec<Section>> {
    let entry = git(&["--literal-pathspecs", "ls-tree", "-z", "--full-tree", rev, "--", doc])?;
    if entry.is_empty() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8(blob(rev, doc, aaa_proto::DESIGN_MAX_TEXT)?)?;
    let mut result = Vec::new();
    let mut events = Vec::new();
    let mut title = String::new();
    let mut heading = false;
    let mut video = false;
    let mut depth = 0;
    for event in Parser::new_ext(
        &text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES,
    ) {
        let mut event = event;
        match &mut event {
            Event::End(TagEnd::Image) if video => {
                events.push(Event::Html("</video>".into()));
                video = false;
                continue;
            }
            Event::Start(Tag::Heading { .. }) if depth == 0 => {
                if !events.is_empty() {
                    result.push(finish(&mut title, &mut events));
                }
                heading = true;
            }
            Event::End(TagEnd::Heading(_)) => heading = false,
            Event::Text(t) | Event::Code(t) if heading => title.push_str(t),
            Event::Html(t) | Event::InlineHtml(t) => {
                if t.trim().starts_with("<!--") {
                    continue;
                }
                event = Event::Text(t.clone());
            }
            Event::Start(Tag::Link { dest_url, .. }) if !dest_url.starts_with('#') => *dest_url = "#".into(),
            Event::Start(Tag::Image { dest_url, .. }) => {
                let path = asset_path(doc, dest_url)?;
                let extension =
                    Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                ensure!(
                    matches!(
                        extension.as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "webm" | "mp4"
                    ),
                    "지원하지 않는 그림: {path}"
                );
                let bytes = blob(rev, &path, aaa_proto::DESIGN_MAX_FILE)?;
                use sha2::{Digest, Sha256};
                let name = format!("{}.{}", hex::encode(Sha256::digest(&bytes)), extension);
                fs::write(output.join(&name), bytes)?;
                if matches!(extension.as_str(), "webm" | "mp4") {
                    events.push(Event::Html(format!("<video controls preload='metadata' src='{name}'>").into()));
                    video = true;
                    continue;
                }
                *dest_url = name.into();
            }
            _ => {}
        }
        match &event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        events.push(event);
    }
    if !events.is_empty() {
        result.push(finish(&mut title, &mut events));
    }
    Ok(result)
}

fn finish(title: &mut String, events: &mut Vec<Event<'_>>) -> Section {
    let mut body = String::new();
    html::push_html(&mut body, events.drain(..));
    Section { title: std::mem::take(title), body }
}

fn page(path: &str, base: &str, head: &str, dir: &Path) -> Result<String> {
    let before = sections(base, path, dir)?;
    let after = sections(head, path, dir)?;
    ensure!(!before.is_empty() || !after.is_empty(), "두 판에서 내용을 찾지 못했다: {path}");
    let mut old = BTreeMap::new();
    for (i, section) in before.iter().enumerate().rev() {
        old.entry(&section.title).or_insert_with(Vec::new).push(i);
    }
    let mut seen = vec![false; before.len()];
    let mut body = String::new();
    let mut links = Vec::new();
    for (index, section) in after.iter().enumerate() {
        let previous = old.get_mut(&section.title).and_then(Vec::pop).map(|i| {
            seen[i] = true;
            &before[i]
        });
        if previous.is_some_and(|p| p.body == section.body) {
            body.push_str(&section.body);
            continue;
        }
        let title = escaped(if section.title.is_empty() { "문서 첫머리" } else { &section.title });
        links.push(format!("<a href='#s{index}'>{title}</a>"));
        body.push_str(&format!(
            "<section id='s{index}'><small>{}</small>{}",
            if previous.is_some() { "수정됨" } else { "추가됨" },
            section.body
        ));
        if let Some(p) = previous {
            body.push_str(&format!("<details><summary>수정 전 보기</summary>{}</details>", p.body));
        }
        body.push_str("</section>");
    }
    for (i, section) in before.iter().enumerate().filter(|(i, _)| !seen[*i]) {
        let title = escaped(if section.title.is_empty() { "문서 첫머리" } else { &section.title });
        links.push(format!("<a href='#old{i}'>{title} · 삭제됨</a>"));
        body.push_str(&format!(
            "<section id='old{i}'><details><summary>{title} · 삭제됨</summary>{}</details></section>",
            section.body
        ));
    }
    let path = escaped(path);
    Ok(format!(
        "<!doctype html><html lang='ko'><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'><title>{path} · 수정본</title><style>{}</style><main><header>{path} · {} → {}</header><nav>{}</nav>{body}</main></html>",
        include_str!("plan_preview.css"),
        &base[..8],
        &head[..8],
        links.join(" · ")
    ))
}

pub(crate) fn write(path: &str, base: &str, output: &Path) -> Result<()> {
    ensure!(aaa_proto::design_clean_path(path).as_deref() == Some(path), "저장소 기준 문서 경로가 필요하다");
    let base = commit(base)?;
    let head = commit("HEAD")?;
    if output.exists() {
        bail!("결과 폴더가 이미 있다: {}", output.display());
    }
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".plan-{}", aaa_common::util::new_id()));
    fs::create_dir(&tmp)?;
    let written = (|| -> Result<()> {
        let html = page(path, &base, &head, &tmp)?;
        fs::write(tmp.join("index.html"), html)?;
        fs::rename(&tmp, output)?;
        Ok(())
    })();
    if written.is_err() {
        fs::remove_dir_all(tmp)?;
    }
    written
}
