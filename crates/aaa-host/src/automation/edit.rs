//! 자동화 파일 읽기·검사·저장·삭제·되살리기·지금 실행(관리 GUI W2, plan §8.342).
//! 원본은 프로젝트의 TOML 파일이다. 저장은 사람이 본 판(sha256)일 때만 쓰고, 다르면 아무것도 쓰지 않고 `conflict`로 거부한다.
//! 쓰기는 공용 저장 helper(`file_rev`, plan §8.311)로 한다: 파일마다 lock, rename 직전 다시 읽어 그사이 바뀌었으면 판 대조를 다시 한다.
//! 폼 값은 정본 타입(`File`)으로 읽은 뒤 바뀐 칸만 원문에 고친다: 주석·순서·나머지 칸은 toml_edit로 그대로 둔다.
use super::{File, decode, field_error, parse_with, project_text};
use crate::{Host, err};
use aaa_proto::{AutomationCheck, AutomationDoc, AutomationReview, AutomationTrashEntry, ErrorBody, ErrorCode};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use toml_edit::{DocumentMut, Item, TableLike};

const TRASH: &str = "automations/.trash";
/// 본문 초안 한도(`project_text`와 같다)
const BODY_MAX: usize = 1024 * 1024;

fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
fn bad(message: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, message)
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}
fn conflict(message: impl Into<String>) -> ErrorBody {
    err(ErrorCode::Conflict, message)
}
fn rule_file(name: &str) -> String {
    format!("automations/{name}.toml")
}

/// 본 판(`expected`, 없으면 「아직 없는 파일」)일 때만 `text`를 쓴다. 이미 같은 글이면 쓰지 않는다.
fn save_seen(
    host: &Host,
    path: &Path,
    expected: Option<&str>,
    text: &str,
    gone: impl Fn() -> ErrorBody,
    changed: impl Fn() -> ErrorBody,
) -> Result<(), ErrorBody> {
    let lock = host.file_locks.of(path);
    let _held = lock.lock().unwrap();
    crate::file_rev::save(path, |rev| match (expected, rev.exists) {
        (None, true) | (Some(_), true) if expected != Some(rev.sha256.as_str()) => Err(changed()),
        (Some(_), false) => Err(gone()),
        _ if rev.exists && rev.text == text => Ok(crate::file_rev::Edit::Keep(())),
        _ => Ok(crate::file_rev::Edit::Write(text.to_string(), ())),
    })
    .map(|_| ())
    .map_err(|e| match e {
        crate::file_rev::SaveError::Io(e) => internal(e),
        crate::file_rev::SaveError::Edit(e) => e,
    })
}

/// 프로젝트 안 일반 파일의 지금 글. 없으면 `None`, symlink·폴더면 오류다.
fn current(root: &Path, relative: &str) -> Result<Option<String>, ErrorBody> {
    match std::fs::symlink_metadata(root.join(relative)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(internal(e)),
        Ok(_) => project_text(root, relative).map(Some).map_err(|e| bad(format!("{relative}: {e}"))),
    }
}

/// 새로 쓸 본문 파일 경로: 프로젝트 안 상대 경로, 있는 중간 폴더는 일반 폴더(없으면 쓸 때 만든다), 끝은 없거나 일반 파일.
fn writable_body(root: &Path, relative: &str) -> Result<(), ErrorBody> {
    use std::path::Component;
    let path = Path::new(relative);
    let parts: Vec<_> = path.components().collect();
    if relative.is_empty() || !parts.iter().all(|c| matches!(c, Component::Normal(_))) {
        return Err(bad("본문 파일은 프로젝트 폴더 안 상대 경로여야 한다"));
    }
    let mut at = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        at.push(part.as_os_str());
        let last = index + 1 == parts.len();
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return Err(bad("본문 파일 경로에 symlink가 있다")),
            Ok(m) if !last && !m.is_dir() => return Err(bad("본문 파일의 폴더가 일반 폴더가 아니다")),
            Ok(m) if last && !m.is_file() => return Err(bad("본문 파일 자리가 일반 파일이 아니다")),
            Ok(_) => {}
            // 여기부터 없다: 아래는 쓸 때 새로 만든다
            Err(_) => break,
        }
    }
    Ok(())
}

/// 폼 값(TOML과 같은 모양)을 정본 타입으로 읽는다. 오류는 칸 경로와 함께 낸다.
fn form_file(form: Value) -> Result<File, String> {
    serde_path_to_error::deserialize::<_, File>(form).map_err(|e| format!("폼 {}: {}", e.path(), e.inner()))
}

fn scalar(v: &Value) -> Option<toml_edit::Value> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => (*b).into(),
        Value::String(s) => s.as_str().into(),
        // 정수인 실수(사용량 문턱 5.0)는 `5`로 쓴다. 정본 타입은 정수도 실수로 읽는다
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.into(),
            (None, Some(f)) if f.fract() == 0.0 && f.abs() < 1e15 => (f as i64).into(),
            (None, f) => f?.into(),
        },
        Value::Array(a) => toml_edit::Value::Array(a.iter().filter_map(scalar).collect()),
        Value::Object(o) => {
            toml_edit::Value::InlineTable(o.iter().filter_map(|(k, v)| Some((k, scalar(v)?))).collect())
        }
    })
}
/// 새 칸: 표는 `[on.git]` 같은 표로, 나머지는 값으로
fn item(v: &Value) -> Option<Item> {
    let Value::Object(o) = v else { return scalar(v).map(Item::Value) };
    let mut table = toml_edit::Table::new();
    for (k, c) in o {
        if let Some(i) = item(c) {
            table.insert(k, i);
        }
    }
    if !table.is_empty() && table.iter().all(|(_, i)| i.is_table()) {
        table.set_implicit(true);
    }
    Some(Item::Table(table))
}
/// 정수인 실수(`5.0`)를 `5`로 쓴다(폼 합치기의 `scalar`와 같은 모양). 정본 타입은 정수도 실수로 읽는다
fn integral_floats(table: &mut dyn TableLike) {
    for (_, item) in table.iter_mut() {
        if let Some(child) = item.as_table_like_mut() {
            integral_floats(child);
        } else if let Some(f) = item.as_float()
            && f.fract() == 0.0
            && f.abs() < 1e15
        {
            *item = toml_edit::value(f as i64);
        }
    }
}
/// 새로 넣은 칸의 경로(안쪽 칸까지). 나중에 기본값과 같은 칸을 지운다.
fn leaves(prefix: &[String], v: &Value, out: &mut Vec<Vec<String>>) {
    out.push(prefix.to_vec());
    if let Value::Object(o) = v {
        for (k, c) in o {
            let mut p = prefix.to_vec();
            p.push(k.clone());
            leaves(&p, c, out);
        }
    }
}

/// `before`(원문을 읽은 값) → `after`(폼 값)에서 바뀐 칸만 `table`에 고친다.
fn apply(
    table: &mut dyn TableLike,
    inline: bool,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    path: &mut Vec<String>,
    inserted: &mut Vec<Vec<String>>,
) {
    for k in before.keys() {
        if after.get(k).is_none_or(Value::is_null) {
            table.remove(k);
        }
    }
    for (k, a) in after {
        if a.is_null() {
            table.remove(k);
            continue;
        }
        let b = before.get(k).filter(|b| !b.is_null());
        if b == Some(a) && table.contains_key(k) {
            continue;
        }
        path.push(k.clone());
        let existing = table.get_mut(k);
        let child_inline = existing.as_ref().is_some_and(|i| i.is_value());
        match (b, a, existing) {
            (Some(Value::Object(bo)), Value::Object(ao), Some(slot)) if slot.is_table_like() => {
                apply(slot.as_table_like_mut().unwrap(), child_inline, bo, ao, path, inserted)
            }
            (_, _, Some(slot)) => {
                let new = if inline || child_inline { scalar(a).map(Item::Value) } else { item(a) };
                if let Some(mut new) = new {
                    // 값 뒤 주석(` # …`)과 칸 앞 주석(키의 decor)은 그 자리에 남긴다
                    if let (Item::Value(old), Item::Value(nv)) = (&*slot, &mut new) {
                        *nv.decor_mut() = old.decor().clone();
                    }
                    *slot = new;
                }
            }
            (_, _, None) => {
                let new = if inline { scalar(a).map(Item::Value) } else { item(a) };
                if let Some(new) = new {
                    table.insert(k, new);
                    leaves(path, a, inserted);
                }
            }
        }
        path.pop();
    }
}

fn remove_path(table: &mut dyn TableLike, path: &[String]) -> bool {
    match path {
        [] => false,
        [last] => table.remove(last).is_some(),
        [head, rest @ ..] => {
            table.get_mut(head).and_then(Item::as_table_like_mut).is_some_and(|child| remove_path(child, rest))
        }
    }
}

/// 새로 넣은 칸 가운데 지워도 같은 값으로 읽히는 것(기본값)을 지운다. 안쪽부터 본다.
fn prune(doc: &mut DocumentMut, file: &str, target: &File, mut inserted: Vec<Vec<String>>) {
    inserted.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for p in inserted {
        let mut trial = doc.clone();
        if remove_path(trial.as_table_mut(), &p)
            && decode::<File>(file, &trial.to_string()).ok().as_ref() == Some(target)
        {
            *doc = trial;
        }
    }
}

/// 폼 값을 원문에 합친다. 원문이 비었으면 새 파일 글을 만든다.
pub(crate) fn merge_form(file: &str, text: &str, form: Value) -> Result<String, String> {
    let target = form_file(form)?;
    let after = serde_json::to_value(&target).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        // 새 파일: 정본 타입의 칸 순서(제목·켜짐·조건·동작·한도)로 쓰고 기본값 칸은 뺀다
        let canonical = toml::to_string_pretty(&target).map_err(|e| e.to_string())?;
        let mut doc: DocumentMut = canonical.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
        integral_floats(doc.as_table_mut());
        let mut inserted = Vec::new();
        if let Value::Object(o) = &after {
            for (k, v) in o {
                leaves(std::slice::from_ref(k), v, &mut inserted);
            }
        }
        prune(&mut doc, file, &target, inserted);
        let out = format!("#:schema ./automation.schema.json\n{doc}");
        return match decode::<File>(file, &out) {
            Ok(read) if read == target => Ok(out),
            Ok(_) => Err("새 원문이 폼 값과 다르게 읽힌다".into()),
            Err(e) => Err(format!("새 원문을 읽지 못한다: {e}")),
        };
    }
    // 맨 위 주석 줄(`#:schema …` 등)은 따로 쥐었다 붙인다: toml_edit는 그것을 첫 칸의 decor로 붙이는데, 그 칸이 지워지거나
    // 숨은 표(`[on.git]`의 `on`)이면 함께 사라진다
    let lead_len: usize = text
        .split_inclusive('\n')
        .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
        .map(str::len)
        .sum();
    let (lead, text) = text.split_at(lead_len);
    let (mut doc, before) = {
        let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            format!("원문 TOML을 읽지 못해 폼을 합칠 수 없다. TOML에서 먼저 고친다: {}", e.message())
        })?;
        let before = decode::<File>(file, text)
            .map_err(|e| format!("원문을 정본 타입으로 읽지 못해 폼을 합칠 수 없다. TOML에서 먼저 고친다: {e}"))?;
        (doc, serde_json::to_value(&before).map_err(|e| e.to_string())?)
    };
    let (Value::Object(before), Value::Object(after)) = (before, after) else {
        return Err("폼 값이 표가 아니다".into());
    };
    let mut inserted = Vec::new();
    apply(doc.as_table_mut(), false, &before, &after, &mut vec![], &mut inserted);
    prune(&mut doc, file, &target, inserted);
    let out = format!("{lead}{doc}");
    // 합친 글을 다시 읽어 폼 값과 같은지 본다(합치기 결함이 엉뚱한 칸을 쓰지 않게)
    match decode::<File>(file, &out) {
        Ok(read) if read == target => Ok(out),
        Ok(_) => Err("합친 원문이 폼 값과 다르게 읽힌다. TOML에서 고친다".into()),
        Err(e) => Err(format!("합친 원문을 읽지 못한다: {e}")),
    }
}

/// 이름 `<규칙>-<지운 시각 ms>.toml`
fn trash_id(id: &str) -> Option<(&str, u64)> {
    let stem = id.strip_suffix(".toml")?;
    let (name, at) = stem.rsplit_once('-')?;
    (aaa_proto::valid_project(name) && !at.is_empty() && at.bytes().all(|b| b.is_ascii_digit()))
        .then(|| Some((name, at.parse().ok()?)))
        .flatten()
}

impl Host {
    /// 검사: 정본 타입·일정·이름·파일 경계(`parse_with`), 그리고 저장하면 일어날 일(검토). `body`는 본문 파일 초안이다.
    pub(crate) fn automation_check(
        &self,
        p: &str,
        root: &Path,
        name: &str,
        file: &str,
        text: &str,
        body: Option<&str>,
    ) -> AutomationCheck {
        let mut check = AutomationCheck::default();
        let parsed = if file.ends_with(".md") {
            crate::routine::parse(name, text)
        } else {
            match decode::<File>(file, text) {
                Ok(f) => check.form = serde_json::to_value(&f).ok(),
                Err(e) => {
                    check.errors.push(e);
                    return check;
                }
            }
            let body_file = decode::<File>(file, text).ok().and_then(|f| f.action.texts().1.map(str::to_owned));
            if body.is_some_and(|b| b.len() > BODY_MAX) {
                check.errors.push(format!("{file}:1:1 본문: 본문은 1MiB 이하여야 한다"));
                return check;
            }
            parse_with(name, text, |path| match body {
                Some(b) if body_file.as_deref() == Some(path) => Ok(b.to_string()),
                _ => project_text(root, path),
            })
        };
        let r = match parsed {
            Ok(r) => r,
            Err(e) => {
                check.errors.push(e);
                return check;
            }
        };
        let mut review = AutomationReview {
            enabled: r.enabled,
            trigger: crate::routine::trigger_label(&r),
            action: r.action.label(),
            start_threads: self.safety(p).start_threads,
            command: r.command.clone(),
            machine: r.machine.clone(),
            command_sha256: if r.command.is_empty() { String::new() } else { r.command_sha256() },
            command_approved: !r.command.is_empty() && self.routine_approved(p, &r),
            commands_allowed: self.routine_commands_allowed(p),
            ..Default::default()
        };
        match self.automation_start_approval(p, &r) {
            Ok(Some((sha, approved))) => {
                review.start_approval_sha256 = Some(sha);
                review.start_approved = Some(approved);
                review.start_scope = self.automation_start_scope_text(p, &r).ok().flatten();
            }
            Ok(None) => {}
            Err(e) => check.warnings.push(format!("자동 시작 대상을 지금 풀지 못한다: {}", e.message)),
        }
        check.review = Some(review);
        check
    }

    pub(crate) fn automation_read(&self, p: &str, name: &str) -> Result<AutomationDoc, ErrorBody> {
        if !aaa_proto::valid_project(name) {
            return Err(bad("자동화 이름 형식이 아니다"));
        }
        let root = self.project_dir(p)?;
        let (file, text) = match current(&root, &rule_file(name))? {
            Some(text) => (rule_file(name), text),
            None => match current(&root, &format!("routines/{name}.md"))? {
                Some(text) => (format!("routines/{name}.md"), text),
                None => {
                    return Err(err(ErrorCode::NotFound, format!("자동화가 없다(지웠거나 이름이 바뀌었다): {name}")));
                }
            },
        };
        let legacy = file.ends_with(".md");
        let body_file = (!legacy)
            .then(|| decode::<File>(&file, &text).ok())
            .flatten()
            .and_then(|f| f.action.texts().1.map(str::to_owned));
        let body = match &body_file {
            Some(path) => current(&root, path).unwrap_or(None),
            None => None,
        };
        Ok(AutomationDoc {
            check: self.automation_check(p, &root, name, &file, &text, None),
            name: name.into(),
            sha256: sha256(&text),
            legacy,
            body_sha256: body.as_deref().map(sha256),
            body,
            body_file,
            file,
            text,
        })
    }

    /// 쓰지 않는다. `form`이 있으면 원문에 합친 글을 검사한다.
    pub(crate) fn automation_validate(
        &self,
        p: &str,
        name: &str,
        text: String,
        form: Option<Value>,
        body: Option<String>,
    ) -> Result<(String, AutomationCheck), ErrorBody> {
        if !aaa_proto::valid_project(name) {
            return Err(bad("자동화 이름 형식이 아니다(영문 소문자·숫자·-·_)"));
        }
        let root = self.project_dir(p)?;
        let file = rule_file(name);
        let text = match form {
            Some(form) => match merge_form(&file, &text, form) {
                Ok(text) => text,
                Err(e) => return Ok((text, AutomationCheck { errors: vec![e], ..Default::default() })),
            },
            None => text,
        };
        let check = self.automation_check(p, &root, name, &file, &text, body.as_deref());
        Ok((text, check))
    }

    /// `expected`: 본 판. 없으면 새로 만든다(이미 있으면 거부). 본문 파일은 먼저, 규칙 파일은 마지막에 쓴다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn automation_save(
        &self,
        p: &str,
        name: &str,
        text: String,
        expected: Option<String>,
        body: Option<String>,
        body_expected: Option<String>,
        actor: &str,
    ) -> Result<AutomationDoc, ErrorBody> {
        if !aaa_proto::valid_project(name) {
            return Err(bad("자동화 이름 형식이 아니다(영문 소문자·숫자·-·_)"));
        }
        let _guard = self.automation_sources.lock().await;
        let root = self.project_dir(p)?;
        let file = rule_file(name);
        if current(&root, &format!("routines/{name}.md"))?.is_some() {
            return Err(bad("같은 이름의 이전 형식 routine이 있다. 먼저 「새 형식으로 바꾸기」를 한다"));
        }
        let now = current(&root, &file)?;
        match (&expected, &now) {
            (None, Some(_)) => return Err(conflict(format!("같은 이름의 자동화가 이미 있다: {name}. 다시 읽는다"))),
            (Some(_), None) => {
                return Err(err(ErrorCode::NotFound, format!("이 자동화는 그 사이 지워졌다: {name}. 저장하지 않았다")));
            }
            (Some(want), Some(have)) if *want != sha256(have) => {
                return Err(conflict(
                    "본 뒤에 파일이 바뀌었다(다른 화면이나 파일 직접 편집). 저장하지 않았다. 다시 읽고 차이를 본다",
                ));
            }
            _ => {}
        }
        let check = self.automation_check(p, &root, name, &file, &text, body.as_deref());
        if !check.errors.is_empty() {
            return Err(bad(check.errors.join("\n")));
        }
        let body_write = match body {
            None => None,
            Some(body) => {
                let decoded = decode::<File>(&file, &text).map_err(bad)?;
                let path =
                    decoded.action.texts().1.map(str::to_owned).ok_or_else(|| {
                        bad(field_error(&file, &text, "do", "본문 파일 칸(text_file·brief_file)이 없다"))
                    })?;
                // 웹이 새로 쓰는 본문은 automations/ 안에만 둔다. 이 규칙이 이미 가리키던 파일은 그 자리에 고친다
                let before = now.as_deref().and_then(|t| decode::<File>(&file, t).ok());
                let known = before.as_ref().and_then(|f| f.action.texts().1) == Some(path.as_str());
                if !known && !(path.starts_with("automations/") && path.ends_with(".md")) {
                    return Err(bad(format!("새 본문 파일은 automations/ 안의 .md로 둔다: {path}")));
                }
                // 규칙 파일·보존 폴더를 본문으로 덮지 않는다
                if path.ends_with(".toml") || path.split('/').any(|part| part == ".trash" || part == ".converted") {
                    return Err(bad(format!("규칙 파일이나 보존 폴더는 본문 파일이 될 수 없다: {path}")));
                }
                writable_body(&root, &path)?;
                let have = current(&root, &path)?;
                if have.as_deref().map(sha256) != body_expected {
                    return Err(conflict(format!(
                        "본 뒤에 본문 파일이 바뀌었다: {path}. 저장하지 않았다. 다시 읽는다"
                    )));
                }
                Some((path, body, body_expected))
            }
        };
        if let Some((path, body, seen)) = &body_write {
            save_seen(
                self,
                &root.join(path),
                seen.as_deref(),
                body,
                || conflict(format!("본 뒤에 본문 파일이 지워졌다: {path}. 저장하지 않았다. 다시 읽는다")),
                || conflict(format!("본 뒤에 본문 파일이 바뀌었다: {path}. 저장하지 않았다. 다시 읽는다")),
            )?;
        }
        save_seen(
            self,
            &root.join(&file),
            expected.as_deref(),
            &text,
            || err(ErrorCode::NotFound, format!("이 자동화는 그 사이 지워졌다: {name}. 저장하지 않았다")),
            || {
                if expected.is_none() {
                    conflict(format!("같은 이름의 자동화가 이미 있다: {name}. 다시 읽는다"))
                } else {
                    conflict(
                        "본 뒤에 파일이 바뀌었다(다른 화면이나 파일 직접 편집). 저장하지 않았다. 다시 읽고 차이를 본다",
                    )
                }
            },
        )?;
        let word = if expected.is_none() { "만듦" } else { "고침" };
        self.db.audit(actor, "automation_save", &format!("{p}/{name} {word} {}", sha256(&text)));
        self.flow_project(p, "automation_saved", Some(file), format!("{name}: {word}"));
        self.tick_wake.notify_one();
        drop(_guard);
        self.automation_read(p, name)
    }

    /// 끈 뒤 `.trash/`로 옮긴다. 대기 입력은 취소하고 처리 중 묶음·기록·본문 파일은 둔다.
    pub(crate) async fn automation_delete(
        &self,
        p: &str,
        name: &str,
        expected: &str,
        actor: &str,
    ) -> Result<String, ErrorBody> {
        if !aaa_proto::valid_project(name) {
            return Err(bad("자동화 이름 형식이 아니다"));
        }
        let _guard = self.automation_sources.lock().await;
        let root = self.project_dir(p)?;
        let file = rule_file(name);
        let Some(text) = current(&root, &file)? else {
            if current(&root, &format!("routines/{name}.md"))?.is_some() {
                return Err(bad("이전 형식 routine은 웹에서 지우지 않는다. 먼저 「새 형식으로 바꾸기」를 한다"));
            }
            return Err(err(ErrorCode::NotFound, format!("자동화가 없다(이미 지웠다): {name}")));
        };
        if sha256(&text) != expected {
            return Err(conflict("본 뒤에 파일이 바뀌었다. 지우지 않았다. 다시 읽는다"));
        }
        let (off, _) = crate::routine::edit_enabled(&text, Some(false))?;
        let path = root.join(&file);
        // 먼저 끈다: 옮기기 전에 끊겨도 규칙은 꺼진 채 남는다
        save_seen(
            self,
            &path,
            Some(expected),
            &off,
            || err(ErrorCode::NotFound, format!("자동화가 없다(이미 지웠다): {name}")),
            || conflict("본 뒤에 파일이 바뀌었다. 지우지 않았다. 다시 읽는다"),
        )?;
        let at = aaa_common::util::now_ms();
        let id = format!("{name}-{at}.toml");
        std::fs::create_dir_all(root.join(TRASH)).map_err(internal)?;
        std::fs::rename(&path, root.join(TRASH).join(&id)).map_err(internal)?;
        // 지금 실행(같은 lock)과 겹쳐 지운 규칙의 입력이 남지 않게 lock 안에서 닫는다
        self.automation_cancel(p, name, "자동화를 지워 보내지 않음", |_| true)?;
        drop(_guard);
        self.db.audit(actor, "automation_delete", &format!("{p}/{name} → {TRASH}/{id}"));
        self.flow_project(p, "automation_deleted", Some(format!("{TRASH}/{id}")), name.to_string());
        Ok(id)
    }

    pub(crate) fn automation_trash(&self, p: &str) -> Result<Vec<AutomationTrashEntry>, ErrorBody> {
        let root = self.project_dir(p)?;
        let mut out: Vec<AutomationTrashEntry> = std::fs::read_dir(root.join(TRASH))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                let (name, at) = trash_id(&id)?;
                let title = project_text(&root, &format!("{TRASH}/{id}"))
                    .ok()
                    .and_then(|t| t.parse::<DocumentMut>().ok())
                    .and_then(|d| d.get("title").and_then(|t| t.as_str()).map(str::to_owned))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| name.to_string());
                Some(AutomationTrashEntry { name: name.into(), title, deleted_at_ms: at, id })
            })
            .collect();
        out.sort_by(|a, b| b.deleted_at_ms.cmp(&a.deleted_at_ms).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// 꺼진 채로 되살린다. 같은 이름이 있으면 거부한다.
    pub(crate) async fn automation_restore(&self, p: &str, id: &str, actor: &str) -> Result<String, ErrorBody> {
        let (name, _) = trash_id(id).ok_or_else(|| bad("지운 자동화 id 형식이 아니다"))?;
        let _guard = self.automation_sources.lock().await;
        let root = self.project_dir(p)?;
        let from = format!("{TRASH}/{id}");
        let text =
            current(&root, &from)?.ok_or_else(|| err(ErrorCode::NotFound, "지운 자동화가 없다(이미 되살렸다)"))?;
        let file = rule_file(name);
        if current(&root, &file)?.is_some() || current(&root, &format!("routines/{name}.md"))?.is_some() {
            return Err(conflict(format!("같은 이름의 자동화가 있다: {name}. 그것을 먼저 지우거나 이름을 바꾼다")));
        }
        let (off, _) = crate::routine::edit_enabled(&text, Some(false))?;
        save_seen(
            self,
            &root.join(&file),
            None,
            &off,
            || internal("되살리기"),
            || conflict(format!("같은 이름의 자동화가 있다: {name}. 그것을 먼저 지우거나 이름을 바꾼다")),
        )?;
        std::fs::remove_file(root.join(&from)).map_err(internal)?;
        self.db.audit(actor, "automation_restore", &format!("{p}/{name} ← {from}"));
        self.flow_project(p, "automation_restored", Some(file), format!("{name}: 꺼진 채로"));
        Ok(name.to_string())
    }

    /// 조건을 기다리지 않고 동작을 한 번 돌린다. 같은 `key`는 한 번만 받는다(참이면 새로 받았다).
    pub(crate) async fn automation_run(
        self: &Arc<Self>,
        p: &str,
        name: &str,
        key: &str,
        actor: &str,
    ) -> Result<(bool, String), ErrorBody> {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(bad("실행 key는 영문·숫자·-·_ 128자 이하다"));
        }
        let root = self.project_dir(p)?;
        // 지우기·저장과 줄 세운다: 찾은 규칙이 넣기 전에 지워지지 않는다
        let _guard = self.automation_sources.lock().await;
        let (rules, broken) = crate::routine::load_all(&root);
        let Some(r) = rules.into_iter().find(|r| r.name == name) else {
            if broken.iter().any(|(f, _, _)| Path::new(f).file_stem().is_some_and(|s| s == name)) {
                return Err(bad("파일 오류가 있는 자동화는 돌리지 않는다. 먼저 고친다"));
            }
            return Err(err(ErrorCode::NotFound, format!("자동화가 없다: {name}")));
        };
        if !r.enabled {
            return Err(bad("꺼진 자동화는 돌리지 않는다. 먼저 켠다"));
        }
        match &r.action {
            super::Action::Deploy => {
                return Err(bad("배포 제안은 지금 실행하지 않는다(main push 사건에만 돈다)"));
            }
            super::Action::Message(a) if a.thread == "owner" => {
                return Err(bad("주인 thread는 사건이 정한다. 지금 실행은 고정 thread로 보내는 규칙만 한다"));
            }
            _ => {}
        }
        if let Some((_, false)) = self.automation_start_approval(p, &r)? {
            return Err(err(ErrorCode::Forbidden, "자동 시작 범위가 승인되지 않았다. 먼저 「승인 검토」에서 승인한다"));
        }
        let once = format!("automation_manual:{p}:{name}:{}", sha256(key));
        if !r.command.is_empty() {
            if !self.routine_commands_allowed(p) {
                return Err(err(ErrorCode::Forbidden, "safety routine_commands가 꺼져 있어 명령을 돌리지 않는다"));
            }
            if !self.routine_approved(p, &r) {
                return Err(err(ErrorCode::Forbidden, "명령이 승인되지 않았다. 먼저 명령을 승인한다"));
            }
            // 돌고 있으면 key를 쓰지 않고 거부한다(같은 key로 다시 누를 수 있다)
            let running = format!("{p}/{name}");
            if !self.routine_running.lock().unwrap().insert(running.clone()) {
                return Err(bad("지난 명령이 아직 돈다. 끝난 뒤 다시 한다"));
            }
            match self.db.update_meta_checked(&once, &[&once], |_| Ok((actor.into(), vec![]))) {
                Ok(true) => {}
                other => {
                    self.routine_running.lock().unwrap().remove(&running);
                    other.map_err(internal)?;
                    return Ok((false, "이미 받은 실행이다".into()));
                }
            }
            self.db.audit(actor, "automation_run", &format!("{p}/{name} 명령"));
            let me = self.clone();
            let project = p.to_string();
            tokio::spawn(async move {
                me.routine_run(&project, &r).await;
                me.routine_running.lock().unwrap().remove(&running);
            });
            return Ok((true, "명령을 지금 돌린다. 출력이 바뀌면 동작이 돈다".into()));
        }
        let id = format!("{}:{key}", super::runtime::MANUAL);
        let event = super::runtime::Event {
            id: id.clone(),
            at_ms: aaa_common::util::now_ms(),
            owner: None,
            body: String::new(),
            action: r.action.clone(),
            source: super::runtime::MANUAL.into(),
            start_scope: None,
        };
        let accepted = self.automation_accept(p, &r, event)?;
        if !accepted {
            return Ok((false, "이미 받은 실행이다".into()));
        }
        self.record_routine(p, name, &id, "waiting", "지금 실행 · 차례 기다림", None);
        self.db.audit(actor, "automation_run", &format!("{p}/{name}"));
        self.flow_project(p, "automation_run", Some(r.file.clone()), name.to_string());
        self.tick_wake.notify_one();
        Ok((true, "큐에 넣었다. 다음 판정에서 돈다".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(text: &str) -> Value {
        serde_json::to_value(decode::<File>("automations/x.toml", text).unwrap()).unwrap()
    }

    #[test]
    fn merge_changes_only_edited_fields_and_keeps_comments() {
        let text = "#:schema ./automation.schema.json\n# 맨 위 주석\ntitle = \"점검\" # 제목 뒤 주석\n\n[on.schedule]\nevery = \"30m\"\n\n# 동작 앞 주석\n[do.inbox]\ntext = \"본다\"\n";
        let mut f = form(text);
        f["on"]["schedule"]["every"] = "1h".into();
        let out = merge_form("automations/x.toml", text, f).unwrap();
        assert_eq!(out, text.replace("\"30m\"", "\"1h\""));
        // 바꾼 것 없음 → 글자 그대로
        assert_eq!(merge_form("automations/x.toml", text, form(text)).unwrap(), text);
    }

    #[test]
    fn merge_switches_condition_and_drops_defaults() {
        let text = "title = \"점검\"\n[on.schedule]\nevery = \"30m\"\n[do.inbox]\ntext = \"본다\"\n";
        let mut f = form(text);
        f["on"] = serde_json::json!({"git": {"repo": "app", "branches": ["main"]}});
        f["limits"] = serde_json::json!({"usage_week_below": 10});
        let out = merge_form("automations/x.toml", text, f.clone()).unwrap();
        assert!(!out.contains("schedule"), "{out}");
        assert!(out.contains("[on.git]") && out.contains("repo = \"app\""), "{out}");
        // 기본값 칸(events = [], machine 없음)을 새로 쓰지 않는다
        assert!(!out.contains("events"), "{out}");
        assert!(out.contains("usage_week_below = 10\n"), "{out}");
        assert_eq!(decode::<File>("automations/x.toml", &out).unwrap(), form_file(f).unwrap());
    }

    #[test]
    fn new_file_is_canonical_and_minimal() {
        let f = serde_json::json!({"title":"새 규칙","on":{"schedule":{"daily":"09:00"}},"do":{"inbox":{"text":"본다"}},"limits":{"usage_session_below":5,"usage_week_below":10}});
        let out = merge_form("automations/x.toml", "", f.clone()).unwrap();
        assert!(out.starts_with("#:schema ./automation.schema.json\n"), "{out}");
        assert!(!out.contains("catch_up") && !out.contains("notify") && !out.contains("enabled"), "{out}");
        assert!(out.contains("usage_session_below = 5\n") && out.contains("usage_week_below = 10\n"), "{out}");
        let on = out.find("[on.schedule]").unwrap();
        let act = out.find("[do.inbox]").unwrap();
        assert!(out.find("title").unwrap() < on && on < act, "{out}");
        assert_eq!(decode::<File>("automations/x.toml", &out).unwrap(), form_file(f).unwrap());
    }

    #[test]
    fn leading_comments_survive_when_the_first_field_goes_away() {
        let text = "#:schema ./automation.schema.json\n# 머리 주석\n[on.git]\nrepo = \"app\"\nbranches = [\"main\"]\n[do.inbox]\ntext = \"본다\"\n";
        let mut f = form(text);
        f["title"] = "새 제목".into();
        f["on"] = serde_json::json!({"schedule": {"every": "1h"}});
        let out = merge_form("automations/x.toml", text, f).unwrap();
        assert!(out.starts_with("#:schema ./automation.schema.json\n# 머리 주석\n"), "{out}");
        assert!(out.contains("title = \"새 제목\""), "{out}");
    }

    #[test]
    fn merge_into_inline_tables_and_deploy_string() {
        let text = "on = { git = { repo = \"aaa\", branches = [\"main\"] } }\ndo = \"deploy\"\n";
        let mut f = form(text);
        f["on"]["git"]["repo"] = "app".into();
        let out = merge_form("automations/x.toml", text, f).unwrap();
        assert_eq!(out, text.replace("\"aaa\"", "\"app\""));
    }

    #[test]
    fn bad_form_and_broken_text_are_reported_not_written() {
        let e = merge_form("automations/x.toml", "", serde_json::json!({"on":{"nope":{}},"do":"deploy"})).unwrap_err();
        assert!(e.starts_with("폼 on"), "{e}");
        let ok = serde_json::json!({"on":{"schedule":{"every":"1m"}},"do":{"inbox":{"text":"본다"}}});
        let e = merge_form("automations/x.toml", "title = \n", ok).unwrap_err();
        assert!(e.contains("TOML에서 먼저 고친다"), "{e}");
    }

    #[test]
    fn trash_ids() {
        assert_eq!(trash_id("ci-main-1700000000000.toml"), Some(("ci-main", 1_700_000_000_000)));
        assert_eq!(trash_id("../x-1.toml"), None);
        assert_eq!(trash_id("x-.toml"), None);
        assert_eq!(trash_id("x-1.md"), None);
    }
}
