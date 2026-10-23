//! 입력칸 「/」 목록(plan §8.323, herdr-web-ui `server/commands.ts`를 옮김): harness 버전에 묶인 내장 표 + 그 머신의 명령·skill 파일.
//!
//! - 내장 표는 Claude Code 2.1.304·Codex 0.163 기준이다(Claude 문서 commands.md와 실행 파일, Codex `tui/src/slash_command.rs`·
//!   `chatwidget/slash_dispatch.rs`). 버전이 바뀌면 낡으므로 `builtin_version`으로 함께 알린다.
//! - 분류(`CommandKind`): 맥락을 비우는 `/clear`(Claude 다른 이름 `/reset`·`/new`)·Codex `/new`는 확인창, harness를 끝내거나 되돌릴 수
//!   없는 `/exit`·`/quit`·`/logout`·Codex `/delete`는 숨기고 확인창(사용자 결정 10-20). 인자 없이 보내면 터미널 선택창이 뜨는 것은
//!   `Picker`, 결과가 화면에만 남는 Codex `/status` 등은 `Screen`(실측: rollout에 남지 않는다).
//! - 파일: Claude 사용자·프로젝트 명령(`commands/**/*.md`, 하위 폴더는 `a:b`)과 skill(`skills/*/SKILL.md`, `user-invocable: false`는
//!   뺀다), Codex skill(`$이름`). 프로젝트는 세션 폴더에서 git root까지 올라가며 본다. plugin·MCP prompt·Codex `prompts/`(deprecated)는
//!   이 머신에 실례가 없어 뺐다. 파일은 앞부분(frontmatter·첫 줄)만 읽고, 깊이·개수에 상한을 둔다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aaa_proto::{CommandKind, CommandList, CommandSource, SlashCommand};

const CLAUDE_VERSION: &str = "2.1.304";
const CODEX_VERSION: &str = "0.163";
/// 폴더마다 읽는 파일 수, 명령 폴더 깊이, 파일에서 읽는 앞부분, 설명 글자 수
const MAX_FILES: usize = 300;
const MAX_DEPTH: usize = 4;
const HEAD_BYTES: usize = 16 << 10;
const DESCRIPTION_CHARS: usize = 160;
/// 프로젝트 폴더를 찾아 올라가는 상한
const MAX_ANCESTORS: usize = 12;

use CommandKind::{Confirm, Danger, Picker, Screen, Send};

/// 내장 명령 한 줄: 이름, 분류, 작업 중 못 씀, 인자 안내, 다른 이름, 설명
type Builtin = (&'static str, CommandKind, bool, Option<&'static str>, &'static [&'static str], &'static str);

const CLAUDE: &[Builtin] = &[
    ("compact", Send, true, Some("[요약 지시]"), &[], "대화를 요약해 컨텍스트를 비운다"),
    ("context", Send, false, None, &[], "컨텍스트 사용량을 범주별로 보인다"),
    ("init", Send, false, None, &[], "이 저장소의 CLAUDE.md를 만든다"),
    ("review", Send, false, Some("[지시]"), &[], "변경을 리뷰한다"),
    ("add-dir", Send, false, Some("<경로>"), &[], "작업 폴더를 더한다"),
    ("agents", Send, false, None, &[], "하위 agent 안내를 보인다"),
    ("model", Picker, false, Some("/model sonnet"), &[], "모델을 바꾼다"),
    ("effort", Picker, false, Some("/effort high"), &[], "생각 수준을 바꾼다"),
    ("config", Picker, false, Some("/config key=value"), &["settings"], "설정을 연다"),
    ("resume", Picker, false, Some("/resume <id|이름>"), &[], "이전 대화를 잇는다"),
    ("rename", Picker, false, Some("/rename <이름>"), &[], "대화 이름을 바꾼다"),
    ("mcp", Picker, false, Some("/mcp reconnect <서버>"), &[], "MCP 서버를 관리한다"),
    ("permissions", Picker, false, None, &[], "도구 권한 규칙을 고친다"),
    ("usage", Picker, false, None, &["cost", "stats"], "비용과 사용 한도를 보인다"),
    ("status", Picker, false, None, &[], "버전·모델·계정 상태를 보인다"),
    ("memory", Picker, false, None, &[], "CLAUDE.md 기억 파일을 고친다"),
    ("hooks", Picker, false, None, &[], "hook 설정을 보인다"),
    ("help", Picker, false, None, &[], "도움말과 명령 목록"),
    ("rewind", Picker, false, None, &["checkpoint", "undo"], "대화·코드를 앞 지점으로 되돌린다"),
    ("tasks", Picker, false, None, &[], "배경 작업을 보인다"),
    ("export", Picker, false, Some("/export <파일>"), &[], "대화를 파일·클립보드로 내보낸다"),
    ("login", Picker, false, None, &[], "로그인한다"),
    ("clear", Confirm, false, None, &["reset", "new"], "대화를 비우고 새로 시작한다(이전 대화는 /resume)"),
    ("exit", Danger, false, None, &["quit"], "Claude Code를 끝낸다"),
    ("logout", Danger, false, None, &[], "로그아웃하고 끝낸다"),
];

const CODEX: &[Builtin] = &[
    ("compact", Send, true, None, &[], "대화를 요약해 컨텍스트를 비운다"),
    ("init", Send, true, None, &[], "AGENTS.md를 만든다"),
    ("fork", Send, true, Some("[이름]"), &[], "지금 대화를 복제해 새 채팅으로 간다"),
    ("plan", Send, true, Some("/plan <지시>"), &[], "계획 모드로 지시한다"),
    ("goal", Send, false, Some("/goal <목표>"), &[], "목표를 정한다"),
    ("stop", Send, false, None, &["clean"], "배경 터미널을 모두 끝낸다"),
    ("status", Screen, false, None, &[], "설정·토큰·한도"),
    ("diff", Screen, false, None, &[], "git diff"),
    ("mcp", Screen, false, None, &[], "MCP 도구 목록"),
    ("recap", Screen, true, None, &[], "지금까지를 요약한다"),
    ("model", Picker, false, None, &[], "모델·생각 수준을 고른다"),
    ("permissions", Picker, false, None, &[], "승인 방식을 고른다"),
    ("review", Picker, true, Some("/review <지시>"), &[], "변경을 리뷰한다"),
    ("resume", Picker, false, Some("/resume <id|이름>"), &[], "이전 세션을 잇는다"),
    ("rename", Picker, false, Some("/rename <이름>"), &[], "세션 이름을 바꾼다"),
    ("skills", Picker, false, None, &[], "skill 메뉴"),
    ("archive", Picker, false, None, &[], "세션을 보관한다"),
    ("new", Confirm, true, Some("[이름]"), &[], "새 채팅을 시작한다"),
    ("clear", Confirm, true, None, &[], "화면을 지우고 새 채팅을 시작한다"),
    ("quit", Danger, false, None, &["exit"], "Codex를 끝낸다"),
    ("logout", Danger, true, None, &[], "로그아웃하고 끝낸다"),
    ("delete", Danger, true, None, &[], "세션을 영구히 지운다"),
];

/// 그 harness의 목록. `project`는 세션이 지금 있는 폴더, 홈 폴더는 Claude·Codex 설정 폴더다
pub fn list(harness: &str, project: Option<&Path>, claude_home: &Path, codex_home: &Path) -> CommandList {
    let (table, version) = match harness {
        "codex" => (CODEX, CODEX_VERSION),
        _ => (CLAUDE, CLAUDE_VERSION),
    };
    let mut commands: Vec<SlashCommand> = table
        .iter()
        .map(|&(name, kind, idle_only, hint, aliases, description)| SlashCommand {
            name: name.into(),
            description: description.into(),
            source: CommandSource::Builtin,
            kind,
            hint: hint.map(Into::into),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            idle_only,
            insert: format!("/{name} "),
        })
        .collect();
    let mut taken: HashSet<String> =
        commands.iter().flat_map(|c| std::iter::once(c.name.clone()).chain(c.aliases.iter().cloned())).collect();
    let projects = project.map(project_dirs).unwrap_or_default();
    let mut files = Vec::new();
    if harness == "codex" {
        files.extend(skills(&codex_home.join("skills"), CommandSource::Skill, "$"));
        for dir in &projects {
            files.extend(skills(&dir.join(".codex/skills"), CommandSource::Skill, "$"));
        }
    } else {
        // 같은 이름이면 skill이 명령보다 먼저다(Claude 문서). 사용자 것이 프로젝트 것보다 먼저 읽힌다
        files.extend(skills(&claude_home.join("skills"), CommandSource::Skill, "/"));
        for dir in &projects {
            files.extend(skills(&dir.join(".claude/skills"), CommandSource::Skill, "/"));
        }
        files.extend(command_files(&claude_home.join("commands"), CommandSource::User));
        for dir in &projects {
            files.extend(command_files(&dir.join(".claude/commands"), CommandSource::Project));
        }
    }
    for c in files {
        // 내장 명령 이름을 파일이 가리지 않는다(확인창 분류가 바뀌지 않게)
        if taken.insert(c.name.clone()) {
            commands.push(c);
        }
    }
    CommandList { harness: harness.into(), builtin_version: version.into(), commands }
}

/// 세션 폴더부터 git root(`.git`이 있는 폴더)까지. git 밖이면 세션 폴더 하나
fn project_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in cwd.ancestors().take(MAX_ANCESTORS) {
        out.push(dir.to_path_buf());
        if dir.join(".git").exists() {
            return out;
        }
    }
    out.truncate(1);
    out
}

/// `commands/**/*.md`: 이름은 폴더 안 경로(하위 폴더는 `:`로 잇는다)
fn command_files(root: &Path, source: CommandSource) -> Vec<SlashCommand> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if out.len() >= MAX_FILES {
                return out;
            }
            if path.is_dir() {
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(rel) = path.with_extension("").strip_prefix(root).map(Path::to_path_buf).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let name = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join(":");
            let head = front(&path);
            out.push(SlashCommand {
                insert: format!("/{name} "),
                name,
                description: head.description,
                source,
                kind: CommandKind::Send,
                hint: head.hint,
                aliases: Vec::new(),
                idle_only: false,
            });
        }
    }
    out
}

/// `skills/<폴더>/SKILL.md`: 이름은 frontmatter `name`, 없으면 폴더 이름. `user-invocable: false`는 사람이 부르는 것이 아니다
fn skills(root: &Path, source: CommandSource, trigger: &str) -> Vec<SlashCommand> {
    let Ok(entries) = std::fs::read_dir(root)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.join("SKILL.md").is_file()).collect();
    dirs.sort();
    dirs.truncate(MAX_FILES);
    dirs.into_iter()
        .filter_map(|dir| {
            let head = front(&dir.join("SKILL.md"));
            if head.hidden {
                return None;
            }
            let name = head.name.unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
            Some(SlashCommand {
                insert: format!("{trigger}{name} "),
                name,
                description: head.description,
                source,
                kind: CommandKind::Send,
                hint: head.hint,
                aliases: Vec::new(),
                idle_only: false,
            })
        })
        .collect()
}

#[derive(Default)]
struct Head {
    name: Option<String>,
    description: String,
    hint: Option<String>,
    hidden: bool,
}

/// 파일 앞부분의 frontmatter(`name`·`description`·`argument-hint`·`user-invocable`)와, 설명이 없으면 본문 첫 줄
fn front(path: &Path) -> Head {
    let Ok(bytes) = read_head(path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return Head::default();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut head = Head::default();
    let mut lines = text.lines().peekable();
    if lines.peek().map(|l| l.trim()) == Some("---") {
        lines.next();
        let mut fields: Vec<(String, String)> = Vec::new();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
            if line.starts_with([' ', '\t']) {
                // 접힌 값(`description: >`)의 이어지는 줄
                if let Some((_, v)) = fields.last_mut() {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(line.trim());
                }
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim();
                let v = if matches!(v, ">" | "|" | ">-" | "|-") { "" } else { v };
                fields.push((k.trim().to_string(), v.to_string()));
            }
        }
        for (k, v) in fields {
            let v = unquote(&v);
            match k.as_str() {
                "name" if !v.is_empty() => head.name = Some(v),
                "description" => head.description = v,
                "argument-hint" if !v.is_empty() => head.hint = Some(v),
                "user-invocable" => head.hidden = v.eq_ignore_ascii_case("false"),
                _ => {}
            }
        }
    }
    if head.description.is_empty() {
        head.description = lines
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| l.trim_start_matches('#').trim().to_string())
            .unwrap_or_default();
    }
    head.description = clip(&head.description);
    head
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(HEAD_BYTES as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= DESCRIPTION_CHARS {
        return s;
    }
    let mut out: String = s.chars().take(DESCRIPTION_CHARS - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaa-slash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn find<'a>(l: &'a CommandList, name: &str) -> &'a SlashCommand {
        l.commands.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name} 없음"))
    }

    #[test]
    fn claude_builtins_carry_the_confirm_and_hide_decisions() {
        let d = temp("builtin");
        let l = list("claude", None, &d.join("claude"), &d.join("codex"));
        assert_eq!(l.builtin_version, CLAUDE_VERSION);
        let kinds = |names: &[&str]| names.iter().map(|n| find(&l, n).kind).collect::<Vec<_>>();
        assert_eq!(
            kinds(&["clear", "exit", "logout", "model", "context", "compact"]),
            [Confirm, Danger, Danger, Picker, Send, Send]
        );
        assert_eq!(find(&l, "clear").aliases, ["reset", "new"]);
        assert_eq!(find(&l, "exit").aliases, ["quit"]);
        assert!(find(&l, "compact").idle_only);
        let c = list("codex", None, &d.join("claude"), &d.join("codex"));
        let kinds = |names: &[&str]| names.iter().map(|n| find(&c, n).kind).collect::<Vec<_>>();
        assert_eq!(
            kinds(&["new", "clear", "quit", "logout", "delete", "status"]),
            [Confirm, Confirm, Danger, Danger, Danger, Screen]
        );
        assert_eq!(c.commands.iter().filter(|x| x.source == CommandSource::Builtin).count(), CODEX.len());
    }

    #[test]
    fn claude_files_are_commands_and_skills_from_home_and_the_project_up_to_git_root() {
        let d = temp("claude");
        let home = d.join("claude");
        write(&home.join("commands/hello.md"), "---\ndescription: 인사한다\nargument-hint: \"[이름]\"\n---\n본문");
        write(&home.join("commands/frontend/review.md"), "# 화면 리뷰\n\n나머지");
        // 내장 이름은 파일이 가리지 않는다
        write(&home.join("commands/clear.md"), "가짜 clear");
        write(
            &home.join("skills/finish/SKILL.md"),
            "---\nname: finish\ndescription: >\n  리뷰하고\n  마무리한다\n---\n",
        );
        write(&home.join("skills/inner/SKILL.md"), "---\nname: inner\nuser-invocable: false\n---\n");
        write(&home.join("skills/empty/README.md"), "skill 아님");
        let repo = d.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        write(&repo.join(".claude/commands/deploy.md"), "---\ndescription: '배포'\n---\n");
        write(&repo.join(".claude/skills/lint/SKILL.md"), "---\ndescription: 검사\n---\n");
        // 같은 이름은 skill이 이긴다
        write(&repo.join(".claude/commands/finish.md"), "명령판 finish");
        let cwd = repo.join("crates/x");
        std::fs::create_dir_all(&cwd).unwrap();
        let l = list("claude", Some(&cwd), &home, &d.join("codex"));
        let files: Vec<_> = l
            .commands
            .iter()
            .filter(|c| c.source != CommandSource::Builtin)
            .map(|c| (c.name.as_str(), c.source, c.description.as_str(), c.hint.as_deref(), c.insert.as_str()))
            .collect();
        assert_eq!(
            files,
            [
                ("finish", CommandSource::Skill, "리뷰하고 마무리한다", None, "/finish "),
                ("lint", CommandSource::Skill, "검사", None, "/lint "),
                ("hello", CommandSource::User, "인사한다", Some("[이름]"), "/hello "),
                ("frontend:review", CommandSource::User, "화면 리뷰", None, "/frontend:review "),
                ("deploy", CommandSource::Project, "배포", None, "/deploy "),
            ]
        );
        assert_eq!(find(&l, "clear").source, CommandSource::Builtin);
    }

    #[test]
    fn codex_skills_insert_a_dollar_name() {
        let d = temp("codex");
        write(&d.join("codex/skills/finish/SKILL.md"), "---\nname: finish\ndescription: Review and fix\n---\n");
        write(&d.join("claude/commands/hello.md"), "Claude 것");
        let l = list("codex", Some(&d), &d.join("claude"), &d.join("codex"));
        let s = find(&l, "finish");
        assert_eq!(
            (s.source, s.insert.as_str(), s.description.as_str()),
            (CommandSource::Skill, "$finish ", "Review and fix")
        );
        assert!(l.commands.iter().all(|c| c.name != "hello"));
    }
}
