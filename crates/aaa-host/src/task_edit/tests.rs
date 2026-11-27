use super::*;
use crate::tasks::{parse, parse_board, sha256};

fn ok_owner(o: &str, person: bool) -> Result<(), String> {
    match (o, person) {
        ("claude" | "me" | "agent" | "codex@mac-b", _) => Ok(()),
        (_, true) => tasks::check_person(o),
        _ => Err(format!("`{o}`는 사람 이름으로 읽힌다")),
    }
}

fn ok_repo(r: &str) -> Result<String, String> {
    match r {
        "aaa" | tasks::REPO_DIR => Ok(r.to_string()),
        "/work/aaa" => Ok("aaa".into()),
        _ => Err(format!("project.toml repos에 없는 저장소다: {r}")),
    }
}

fn no_log(_: i64) -> Result<Removed, String> {
    Err("기록 없음".into())
}

fn run(text: &str, ops: Vec<TaskOp>) -> Result<Applied, Fail> {
    apply(text, &ops, &Ctx { owner_ok: &ok_owner, profiles: &[], repo_ok: &ok_repo, removed: &no_log })
}

fn anchor(text: &str, title: &str) -> TaskAnchor {
    let t = parse(text).into_iter().find(|t| t.title == title).unwrap_or_else(|| panic!("{title}"));
    TaskAnchor { list: t.list, title: t.title, block_sha256: t.block_sha256, line: t.line }
}

fn update(at: TaskAnchor) -> TaskOp {
    TaskOp::Update {
        at,
        title: None,
        owner: None,
        person: false,
        profile: None,
        machine: None,
        review: None,
        notes: None,
        thread: None,
        done: None,
        repo: None,
    }
}

#[test]
fn review_patch_round_trips_and_retains_other_fields() {
    let before = "- [ ] 확인 (claude) [aaa] · t-0001 <!-- aaa-task:AAAAAAAAAAAAAAAAAAAAAAAAAA -->\r\n  원래 메모\r\n";
    assert!(parse(before)[0].review.is_none());
    let edit = |text: &str, profile, account| {
        let mut op = update(anchor(text, "확인"));
        let TaskOp::Update { review, .. } = &mut op else { unreachable!() };
        *review = Some(aaa_proto::TaskReview { profile, account });
        run(text, vec![op]).unwrap()
    };
    let added = edit(before, Some("review-a".into()), Some("review-account".into()));
    assert_eq!(
        added.text,
        "- [ ] 확인 (claude) [aaa] · t-0001 <!-- aaa-review:{\"profile\":\"review-a\",\"account\":\"review-account\"} --> <!-- aaa-task:AAAAAAAAAAAAAAAAAAAAAAAAAA -->\r\n  원래 메모\r\n",
    );
    assert_ne!(parse(before)[0].block_sha256, parse(&added.text)[0].block_sha256);
    assert!(added.records[0].after.as_ref().unwrap().contains("\"account\":\"review-account\""));
    let account_reset = edit(&added.text, None, Some(String::new()));
    let review = parse(&account_reset.text)[0].review.clone().unwrap();
    assert_eq!(review.profile.as_deref(), Some("review-a"));
    assert_eq!(review.account, None);
    assert_eq!(edit(&account_reset.text, Some(String::new()), None).text, before);
}

/// 모델·머신 칸(plan §8.335): 칸마다 고치고(다른 칸은 그대로), 바뀌지 않은 옛 줄은 바이트 그대로, owner(me)와 함께 주면 거부
#[test]
fn model_machine_edits() {
    fn ok(o: &str, _: bool) -> Result<(), String> {
        if o == "nobody@mac-b" { Err("mac-b에서 쓸 수 있는 profile이 아니다: nobody".into()) } else { Ok(()) }
    }
    let profiles = vec!["claude".to_string(), "codex-fast".to_string()];
    let go = |text: &str, ops: Vec<TaskOp>| {
        apply(text, &ops, &Ctx { owner_ok: &ok, profiles: &profiles, repo_ok: &ok_repo, removed: &no_log })
    };
    let md = "## A\n- [ ] 일 (codex-fast@mac-a) [aaa] · t-0001\n  메모\n- [ ] 옛 줄 (codex-fast)  \n";
    let set = |title: &str, p: Option<&str>, m: Option<&str>| {
        let mut op = update(anchor(md, title));
        if let TaskOp::Update { profile, machine, .. } = &mut op {
            *profile = p.map(Into::into);
            *machine = m.map(Into::into);
        }
        op
    };
    let line = |out: Applied| out.text.lines().nth(1).unwrap().to_string();
    assert_eq!(line(go(md, vec![set("일", None, Some(""))]).unwrap()), "- [ ] 일 (codex-fast) [aaa] · t-0001");
    assert_eq!(line(go(md, vec![set("일", Some("claude"), None)]).unwrap()), "- [ ] 일 (claude@mac-a) [aaa] · t-0001");
    assert_eq!(line(go(md, vec![set("일", Some(""), None)]).unwrap()), "- [ ] 일 (@mac-a) [aaa] · t-0001");
    assert_eq!(line(go(md, vec![set("일", Some(""), Some(""))]).unwrap()), "- [ ] 일 [aaa] · t-0001");
    // 메모만 고치면 옛 줄은 바이트 그대로다(끝 공백까지)
    let mut notes = update(anchor(md, "옛 줄"));
    if let TaskOp::Update { notes: n, .. } = &mut notes {
        *n = Some(vec!["새 메모".into()]);
    }
    assert!(go(md, vec![notes]).unwrap().text.contains("- [ ] 옛 줄 (codex-fast)  \n  새 메모\n"));
    // 모델만 적힌 줄에 머신을 더한다
    assert!(go(md, vec![set("옛 줄", None, Some("mac-b"))]).unwrap().text.contains("- [ ] 옛 줄 (codex-fast@mac-b)\n"));
    // 더하기: 두 칸, 판정 거부, me와 함께 주면 거부
    let add = |owner: Option<&str>, p: Option<&str>, m: Option<&str>| TaskOp::Add {
        list: "A".into(),
        title: "새 일".into(),
        owner: owner.map(Into::into),
        person: false,
        profile: p.map(Into::into),
        machine: m.map(Into::into),
        review: None,
        notes: vec![],
        parent: None,
        before: None,
        thread: None,
        repo: None,
    };
    let out = go(md, vec![add(None, Some("claude"), Some("mac-b"))]).unwrap();
    assert!(out.text.contains("- [ ] 새 일 (claude@mac-b)\n"), "{}", out.text);
    let e = go(md, vec![add(None, Some("nobody"), Some("mac-b"))]).unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("쓸 수 있는 profile이 아니다")), "{e:?}");
    let e = go(md, vec![add(Some("me"), Some("claude"), None)]).unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("함께 주지 않는다")), "{e:?}");
}

/// 저장소 칸(plan §8.330): 더하기·바꾸기(`None` 그대로, 빈 글 비움)·경로는 이름으로·모르는 이름 거부
#[test]
fn repo_field_edits() {
    let md = "## A\n- [ ] 하나 (claude) · t-1\n";
    let add = |repo: &str| TaskOp::Add {
        list: "A".into(),
        title: "둘".into(),
        owner: Some("me".into()),
        person: false,
        profile: None,
        machine: None,
        review: None,
        notes: vec![],
        parent: None,
        before: None,
        thread: None,
        repo: Some(repo.into()),
    };
    assert_eq!(run(md, vec![add("/work/aaa")]).unwrap().text, format!("{md}- [ ] 둘 (me) [aaa]\n"));
    let e = run(md, vec![add("bbb")]).unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("없는 저장소")), "{e:?}");
    let set = |text: &str, repo: Option<&str>, owner: Option<&str>| {
        run(
            text,
            vec![TaskOp::Update {
                at: anchor(text, "하나"),
                title: None,
                owner: owner.map(Into::into),
                person: false,
                profile: None,
                machine: None,
                review: None,
                notes: None,
                thread: None,
                done: None,
                repo: repo.map(Into::into),
            }],
        )
        .unwrap()
        .text
    };
    let with = set(md, Some(tasks::REPO_DIR), None);
    assert_eq!(with, "## A\n- [ ] 하나 (claude) [작업 폴더] · t-1\n");
    assert_eq!(set(&with, None, Some("me")), "## A\n- [ ] 하나 (me) [작업 폴더] · t-1\n", "None은 그대로");
    assert_eq!(set(&with, Some(""), None), md, "빈 글은 비움");
    // 끝 대괄호 제목 뒤에 owner가 있는 옛 줄은 그대로 고친다(thread 잇기)
    let old = "## A\n- [ ] 배포 [v2] (claude)\n";
    let link = TaskOp::Update {
        at: anchor(old, "배포 [v2]"),
        title: None,
        owner: None,
        person: false,
        profile: None,
        machine: None,
        review: None,
        notes: None,
        thread: Some("t-2".into()),
        done: None,
        repo: None,
    };
    assert_eq!(run(old, vec![link]).unwrap().text, "## A\n- [ ] 배포 [v2] (claude) · t-2\n");
    let bare = TaskOp::Add {
        list: "A".into(),
        title: "릴리스 [v2]".into(),
        owner: None,
        person: false,
        profile: None,
        machine: None,
        review: None,
        notes: vec![],
        parent: None,
        before: None,
        thread: None,
        repo: None,
    };
    let e = run(md, vec![bare]).unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("저장소 칸으로 읽힌다")), "{e:?}");
}

/// 목록 설명·빈 줄·모르는 줄·하위 task·CRLF·마지막 줄바꿈 없음이 든 파일
const MD: &str = "# Tasks\r\n\r\n## Backlog\r\n나중 일은 later.md에 모은다\r\n\r\n- [ ] 상위 (me)\r\n  메모 A\r\n  - [ ] 하위 하나\r\n    하위 메모\r\n  하위 뒤 상위 들여쓰기 줄\r\n- [ ] 정리 (t-0101 S4 뒤, 사용자 지시 시)\r\n<!-- 모르는 줄 -->\r\n\r\n## Later\r\n- [x] 끝 (claude) · t-0007";

/// 한 task만 고치면 나머지 바이트는 그대로다
#[test]
fn update_keeps_other_bytes() {
    let a = anchor(MD, "하위 하나");
    let notes = Some(vec!["새 메모".into(), "".into(), "  들여쓴 줄".into()]);
    let out = run(
        MD,
        vec![TaskOp::Update {
            at: a,
            title: None,
            owner: None,
            person: false,
            profile: None,
            machine: None,
            review: None,
            notes,
            thread: None,
            done: None,
            repo: None,
        }],
    )
    .unwrap();
    assert_eq!(
        out.text,
        MD.replace("    하위 메모\r\n", "    새 메모\r\n\r\n      들여쓴 줄\r\n"),
        "바뀐 블록만 다르다"
    );
    let r = &out.records[0];
    assert_eq!((r.action, r.list.as_str(), r.title.as_str()), (TaskAction::Update, "Backlog", "하위 하나"));
    assert_eq!(r.before.as_deref(), Some("  - [ ] 하위 하나\n    하위 메모"));
    // 칸이 하나도 안 바뀌면 글이 그대로이고 기록도 없다
    let same = run(
        MD,
        vec![TaskOp::Update {
            at: anchor(MD, "상위"),
            title: Some("상위".into()),
            owner: Some("me".into()),
            person: false,
            profile: None,
            machine: None,
            review: None,
            notes: Some(vec!["메모 A".into()]),
            thread: None,
            done: Some(false),
            repo: None,
        }],
    )
    .unwrap();
    assert_eq!(same.text, MD);
    assert!(same.records.is_empty());
    // 제목만 바꾸면 task 줄만 정규형으로 새로 쓴다(메모·줄 끝 문자 그대로)
    let t = run(
        MD,
        vec![TaskOp::Update {
            at: anchor(MD, "끝"),
            title: Some("끝난 일".into()),
            owner: None,
            person: false,
            profile: None,
            machine: None,
            review: None,
            notes: None,
            thread: None,
            done: None,
            repo: None,
        }],
    )
    .unwrap();
    assert_eq!(t.text, MD.replace("- [x] 끝 (claude) · t-0007", "- [x] 끝난 일 (claude) · t-0007"));
}

/// 다른 task가 그사이 바뀌어도 성공하고, 대상이 바뀌거나 지워지면 충돌이다(되살아나지 않는다)
#[test]
fn block_anchor_conflicts_only_on_the_target() {
    let a = anchor(MD, "끝");
    let other = MD.replace("메모 A", "메모 A 고침");
    let out = run(&other, vec![TaskOp::Delete { at: a.clone() }]).unwrap();
    assert!(!out.text.contains("끝 (claude)") && out.text.contains("메모 A 고침"));
    let changed = MD.replace("t-0007", "t-0008");
    let e = run(&changed, vec![TaskOp::Delete { at: a.clone() }]).unwrap_err();
    assert!(matches!(&e, Fail::Conflict { op: 0, title, .. } if title == "끝"), "{e:?}");
    let gone = MD.replace("\r\n## Later\r\n- [x] 끝 (claude) · t-0007", "\r\n## Later\r\n");
    let e = run(&gone, vec![update(a)]).unwrap_err();
    assert!(matches!(e, Fail::Conflict { .. }), "지운 task 수정은 충돌: {e:?}");
    // 제목으로 찾기(CLI)
    let by_title = TaskAnchor { title: "상위".into(), ..Default::default() };
    assert!(run(MD, vec![TaskOp::Delete { at: by_title }]).is_ok());
}

/// 추가는 원문·줄 끝·하위를 보존하고, 앞 동작 뒤에도 같은 블록을 찾는다.
#[test]
fn append_notes_keeps_raw_block_and_conflicts() {
    let md = "## A\r\n- [ ] 상위 (me)  \r\n  원래  \r\n\r\n    들여씀\r\n  - [ ] 하위\r\n    자식 메모\r\n- [ ] 마지막";
    let at = anchor(md, "상위");
    let append = |at| TaskOp::AppendNotes { at, notes: vec!["새 줄".into(), "둘째".into()] };
    let mut rename = update(at.clone());
    if let TaskOp::Update { title, .. } = &mut rename {
        *title = Some("바뀐 제목".into());
    }
    let a = run(md, vec![append(at.clone()), rename]).unwrap();
    assert_eq!(
        a.text,
        md.replace("상위 (me)", "바뀐 제목 (me)")
            .replace("  - [ ] 하위", "  새 줄\r\n  둘째\r\n  - [ ] 하위")
            .replace("(me)  ", "(me)")
    );
    assert_eq!(a.records[0].before.as_deref(), Some("- [ ] 상위 (me)  \n  원래  \n\n    들여씀"));
    assert_eq!(a.records[0].after.as_deref(), Some("- [ ] 상위 (me)  \n  원래  \n\n    들여씀\n  새 줄\n  둘째"));
    assert!(matches!(run(&md.replace("원래", "경합"), vec![append(at)]), Err(Fail::Conflict { .. })));
    let tail = run(md, vec![append(anchor(md, "마지막"))]).unwrap();
    assert!(tail.text.ends_with("- [ ] 마지막\r\n  새 줄\r\n  둘째"));
    let invalid = TaskOp::AppendNotes { at: anchor(md, "상위"), notes: vec!["- [ ] 오독".into()] };
    assert!(matches!(run(md, vec![invalid]), Err(Fail::Bad(_))));
}

/// 교체 경고는 같은 줄 수의 유실·중복 감소도 잡고 정상 추가엔 뜨지 않는다.
#[test]
fn note_loss_detects_removed_and_replaced_lines() {
    let block = |notes: &str| format!("- [ ] 일\n{notes}");
    let before = block("  첫째\n  둘째\n  둘째");
    assert_eq!(note_loss(&before, &block("  첫째\n  둘째\n  둘째\n  추가")), None);
    assert_eq!(note_loss(&before, &block("  둘째\n  첫째\n  둘째")), None);
    assert_eq!(note_loss(&before, &block("  첫째\n  둘째")), Some((3, 2)));
    assert_eq!(note_loss(&before, &block("  첫째\n  둘째\n  교체")), Some((3, 3)));
    assert_eq!(note_loss(&before, &block("")), Some((3, 0)));
}

/// 저장 검사: `)` 끝 제목, ` · t-` 제목, 중복 제목(대소문자 무시), 사람 owner 표시, task 꼴 메모
#[test]
fn save_checks() {
    let add = |title: &str, owner: Option<&str>, person: bool, notes: Vec<&str>| TaskOp::Add {
        list: "Backlog".into(),
        title: title.into(),
        owner: owner.map(str::to_string),
        person,
        profile: None,
        machine: None,
        review: None,
        notes: notes.into_iter().map(str::to_string).collect(),
        parent: None,
        before: None,
        thread: None,
        repo: None,
    };
    for (op, why) in [
        (add("상태 메모 (S4 뒤)", None, false, vec![]), "`)`로 끝나면"),
        (add("웹 화면 · t-0002", None, false, vec![]), "thread로 읽힌다"),
        (add("번호 t-1182가 묻힌 일", None, false, vec![]), "연결 칸"),
        (add(&"긴".repeat(121), None, false, vec![]), "메모로 옮긴다"),
        (add("상위", None, false, vec![]), "같은 제목"),
        (add("새 일", Some("Priya"), false, vec![]), "사람 이름으로 읽힌다"),
        (add("새 일", Some("t-0001 뒤, 지시 시"), true, vec![]), "owner 꼴"),
        (add("새 일", None, false, vec!["- [ ] 하위"]), "task 꼴"),
        (add("새 일", None, false, vec!["# 머리"]), "`#`"),
        (add("", None, false, vec![]), "비었다"),
    ] {
        match run(MD, vec![op]) {
            Err(Fail::Bad(e)) => assert!(e.contains(why), "{why}: {e}"),
            other => panic!("{why}: {other:?}"),
        }
    }
    // 대소문자만 다른 제목도 중복이다
    assert!(matches!(run(MD, vec![add("하위 하나", None, false, vec![])]), Err(Fail::Bad(_))));
    let ok = run(MD, vec![add("새 일", Some("Priya Rao"), true, vec!["메모", "", "둘째"])]).unwrap();
    assert!(ok.text.ends_with("- [x] 끝 (claude) · t-0007"), "다른 목록은 그대로");
    assert!(
        ok.text.contains("<!-- 모르는 줄 -->\r\n- [ ] 새 일 (Priya Rao)\r\n  메모\r\n\r\n  둘째\r\n\r\n## Later"),
        "{}",
        ok.text
    );
    // 형식 어긋남 task의 제목을 그대로 둔 채 owner만 바꾸면 다시 읽히지 않으므로 거부한다
    let e = run(
        MD,
        vec![TaskOp::Update {
            at: anchor(MD, "정리 (t-0101 S4 뒤, 사용자 지시 시)"),
            title: None,
            owner: Some("me".into()),
            person: false,
            profile: None,
            machine: None,
            review: None,
            notes: None,
            thread: None,
            done: None,
            repo: None,
        }],
    )
    .unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("정리 제안")), "{e:?}");
}

/// 지우기·옮기기는 하위 묶음과 함께, 들여쓰기를 맞추고, 빈 줄이 겹치지 않는다. 기록은 묶음 전체다
#[test]
fn delete_and_move_carry_the_subtree() {
    let md = "## A\n- [ ] 하나\n\n- [ ] 둘\n  메모\n  - [ ] 둘의 하위\n\n- [ ] 셋\n## B\n- [ ] 넷\n";
    let out = run(md, vec![TaskOp::Delete { at: anchor(md, "둘") }]).unwrap();
    assert_eq!(out.text, "## A\n- [ ] 하나\n\n- [ ] 셋\n## B\n- [ ] 넷\n");
    assert_eq!(out.records[0].before.as_deref(), Some("- [ ] 둘\n  메모\n  - [ ] 둘의 하위"));
    // 다른 목록 끝으로
    let out =
        run(md, vec![TaskOp::Move { at: anchor(md, "둘"), to_list: "B".into(), before: None, parent: None }]).unwrap();
    assert_eq!(out.text, "## A\n- [ ] 하나\n\n- [ ] 셋\n## B\n- [ ] 넷\n- [ ] 둘\n  메모\n  - [ ] 둘의 하위\n");
    assert_eq!(out.records[0].from_list.as_deref(), Some("A"));
    // 다른 task의 하위로(들여쓰기 +2)
    let out = run(
        md,
        vec![TaskOp::Move {
            at: anchor(md, "둘"),
            to_list: String::new(),
            before: None,
            parent: Some(anchor(md, "넷")),
        }],
    )
    .unwrap();
    assert!(out.text.ends_with("## B\n- [ ] 넷\n  - [ ] 둘\n    메모\n    - [ ] 둘의 하위\n"), "{}", out.text);
    let (_, t) = parse_board(&out.text);
    assert_eq!(t.iter().find(|t| t.title == "둘").unwrap().parent, t.iter().find(|t| t.title == "넷").map(|t| t.line));
    // 하위를 위로(맨 위로 = 첫 task 앞)
    let out = run(
        md,
        vec![TaskOp::Move {
            at: anchor(md, "둘의 하위"),
            to_list: String::new(),
            before: Some(anchor(md, "하나")),
            parent: None,
        }],
    )
    .unwrap();
    assert!(out.text.starts_with("## A\n- [ ] 둘의 하위\n- [ ] 하나\n"), "{}", out.text);
    // 자기 하위로는 못 옮긴다
    assert!(matches!(
        run(
            md,
            vec![TaskOp::Move {
                at: anchor(md, "둘"),
                to_list: String::new(),
                before: None,
                parent: Some(anchor(md, "둘의 하위"))
            }]
        ),
        Err(Fail::Bad(_))
    ));
    // 마지막 줄바꿈 없는 파일 끝 task를 옮겨도 줄이 붙지 않는다
    let tail = "## A\n- [ ] 하나\n## B\n- [ ] 끝";
    let out = run(tail, vec![TaskOp::Move { at: anchor(tail, "끝"), to_list: "A".into(), before: None, parent: None }])
        .unwrap();
    assert_eq!(out.text, "## A\n- [ ] 하나\n- [ ] 끝\n## B\n");
}

/// 목록: 더하기·이름 바꾸기·합치기(설명 줄도)·빈 목록만 지우기·옮기기
#[test]
fn list_ops() {
    let md = "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## 빈\n\n## B\n- [ ] 둘\n";
    let out = run(md, vec![TaskOp::ListAdd { name: "새".into(), after: Some("A".into()) }]).unwrap();
    assert_eq!(out.text, "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## 새\n\n## 빈\n\n## B\n- [ ] 둘\n");
    let out = run(md, vec![TaskOp::ListAdd { name: "끝".into(), after: None }]).unwrap();
    assert!(out.text.ends_with("- [ ] 둘\n\n## 끝\n"), "{}", out.text);
    assert!(matches!(run(md, vec![TaskOp::ListAdd { name: "B".into(), after: None }]), Err(Fail::Bad(_))));
    let out = run(md, vec![TaskOp::ListRename { name: "A".into(), to: "가".into() }]).unwrap();
    assert_eq!(out.text, md.replace("## A\n", "## 가\n"));
    assert!(
        matches!(run(md, vec![TaskOp::ListRename { name: "A".into(), to: "B".into() }]), Err(Fail::Bad(m)) if m.contains("합치기"))
    );
    let out = run(md, vec![TaskOp::ListMerge { name: "A".into(), into: "B".into() }]).unwrap();
    assert_eq!(out.text, "# Tasks\n\n## 빈\n\n## B\n- [ ] 둘\nA 설명\n- [ ] 하나\n");
    let (lists, _) = parse_board(&out.text);
    assert_eq!(lists.iter().find(|l| l.name == "B").unwrap().notes, ["A 설명"]);
    assert!(matches!(run(md, vec![TaskOp::ListDelete { name: "A".into() }]), Err(Fail::Bad(m)) if m.contains("1개")));
    let out = run(md, vec![TaskOp::ListDelete { name: "빈".into() }]).unwrap();
    assert_eq!(out.text, "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## B\n- [ ] 둘\n");
    let out = run(md, vec![TaskOp::ListMove { name: "B".into(), before: Some("A".into()) }]).unwrap();
    assert_eq!(out.text, "# Tasks\n\n## B\n- [ ] 둘\n\n## A\nA 설명\n- [ ] 하나\n\n## 빈\n");
    let out = run(md, vec![TaskOp::ListMove { name: "A".into(), before: None }]).unwrap();
    assert_eq!(out.text, "# Tasks\n\n## 빈\n\n## B\n- [ ] 둘\n\n## A\nA 설명\n- [ ] 하나\n");
    // 없는 목록에 더하면 파일 끝에 만든다(빈 파일 포함)
    let out = run(
        "",
        vec![TaskOp::Add {
            list: "Backlog".into(),
            title: "첫 일".into(),
            owner: None,
            person: false,
            profile: None,
            machine: None,
            review: None,
            notes: vec![],
            parent: None,
            before: None,
            thread: None,
            repo: None,
        }],
    )
    .unwrap();
    assert_eq!(out.text, "## Backlog\n- [ ] 첫 일\n");
    // 한 요청 안에서 새 목록을 만들고 옮긴다
    let out = run(
        md,
        vec![
            TaskOp::ListAdd { name: "C".into(), after: None },
            TaskOp::Move { at: anchor(md, "둘"), to_list: "C".into(), before: None, parent: None },
        ],
    )
    .unwrap();
    assert!(out.text.ends_with("## B\n\n## C\n- [ ] 둘\n"), "{}", out.text);
}

/// 되살리기: 지운 묶음이 원래 목록 끝에 그대로 돌아오고, 같은 제목이 있으면 거부한다
#[test]
fn restore_brings_back_the_block() {
    let md = "## A\n- [ ] 하나\n  - [ ] 하위\n- [ ] 둘\n";
    let del = run(md, vec![TaskOp::Delete { at: anchor(md, "하나") }]).unwrap();
    let text = del.records[0].before.clone().unwrap();
    let removed = move |id: i64| {
        if id == 7 { Ok(Removed { list: "A".into(), text: text.clone() }) } else { Err("없는 기록".into()) }
    };
    let ctx = Ctx { owner_ok: &ok_owner, profiles: &[], repo_ok: &ok_repo, removed: &removed };
    let back = apply(&del.text, &[TaskOp::Restore { log_id: 7, list: None }], &ctx).unwrap();
    assert_eq!(back.text, "## A\n- [ ] 둘\n- [ ] 하나\n  - [ ] 하위\n");
    assert_eq!((back.records[0].title.as_str(), back.records[0].restored), ("하나", Some(7)));
    let e = apply(md, &[TaskOp::Restore { log_id: 7, list: None }], &ctx).unwrap_err();
    assert!(matches!(&e, Fail::Bad(m) if m.contains("같은 제목")), "{e:?}");
    // 고른 목록(없으면 만든다)
    let other = apply(&del.text, &[TaskOp::Restore { log_id: 7, list: Some("새".into()) }], &ctx).unwrap();
    assert!(other.text.ends_with("## 새\n- [ ] 하나\n  - [ ] 하위\n"), "{}", other.text);
}

/// 파일 직접 편집의 차이: 그대로·옮김·수정·삭제·더하기
#[test]
fn file_diff_records() {
    let old = "## A\n- [ ] 하나\n- [ ] 둘\n  메모\n- [ ] 셋\n## B\n";
    let new = "## A\n- [ ] 하나\n- [ ] 둘\n  메모 고침\n## B\n- [ ] 셋\n- [ ] 넷\n";
    let r: Vec<_> = diff(old, new).into_iter().map(|r| (r.action, r.title, r.list, r.from_list)).collect();
    assert_eq!(
        r,
        [
            (TaskAction::Move, "셋".into(), "B".into(), Some("A".into())),
            (TaskAction::Update, "둘".into(), "A".into(), None),
            (TaskAction::Add, "넷".into(), "B".into(), None),
        ]
    );
    let gone = diff(old, "## A\n- [ ] 하나\n## B\n");
    assert_eq!(gone.iter().filter(|r| r.action == TaskAction::Delete).count(), 2);
    assert_eq!(gone[0].before.as_deref(), Some("- [ ] 둘\n  메모"));
    assert!(diff(old, old).is_empty());
}

/// 한 요청에서 같은 task를 고친 뒤 옮긴다(둘째 동작은 바뀐 hash를 따라간다)
#[test]
fn update_then_move_in_one_request() {
    let md = "## A\n- [ ] 하나\n- [ ] 둘\n## B\n- [ ] 셋\n";
    let a = anchor(md, "하나");
    let out = run(
        md,
        vec![
            TaskOp::Update {
                at: a.clone(),
                title: Some("하나 고침".into()),
                owner: None,
                person: false,
                profile: None,
                machine: None,
                review: None,
                notes: Some(vec!["메모".into()]),
                thread: None,
                done: None,
                repo: None,
            },
            TaskOp::Move { at: a.clone(), to_list: String::new(), before: None, parent: Some(anchor(md, "셋")) },
            TaskOp::Update {
                at: a,
                title: None,
                owner: Some("me".into()),
                person: false,
                profile: None,
                machine: None,
                review: None,
                notes: None,
                thread: None,
                done: None,
                repo: None,
            },
        ],
    )
    .unwrap();
    assert_eq!(out.text, "## A\n- [ ] 둘\n## B\n- [ ] 셋\n  - [ ] 하나 고침 (me)\n    메모\n");
    assert_eq!(
        out.records.iter().map(|r| r.action).collect::<Vec<_>>(),
        [TaskAction::Update, TaskAction::Move, TaskAction::Update]
    );
}

/// 원문 편집은 홀로만
#[test]
fn raw_save_alone() {
    let raw = TaskOp::RawSave { expected_sha256: sha256(MD), text: "## X\n".into() };
    assert_eq!(run(MD, vec![raw.clone()]).unwrap().text, "## X\n");
    assert!(matches!(run(MD, vec![raw, TaskOp::ListAdd { name: "Y".into(), after: None }]), Err(Fail::Bad(_))));
}
