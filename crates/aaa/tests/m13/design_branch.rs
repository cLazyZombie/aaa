//! 기획 branch(t-1894 2단계): 실제 host·agent·Git으로 CLI `aaa plan new|save|show|list|discard`와 쉼 뒤 올리기·쉼·다시 열기를 본다.
//! main checkout과 origin main은 끝까지 그대로다.
use super::*;

/// 저장소 기획 폴더 `docs`를 origin main에 올리고 프로젝트 저장소 항목에 `design = "docs"`를 단다. 올리기 전 검사 `check`는
/// host.toml `[projects.design_check]`에 둔다(프로젝트 폴더가 아니다)
fn design_world(extra: &str, check: &str) -> World {
    let w = world_with(&format!("{extra}[projects.design_check]\nrepo = {check}\n"));
    git(&w.repo, &["config", "user.name", "t"]);
    git(&w.repo, &["config", "user.email", "t@example.com"]);
    std::fs::create_dir_all(w.repo.join("docs")).unwrap();
    std::fs::write(w.repo.join("docs/README.md"), "# 개요\n").unwrap();
    git(&w.repo, &["add", "."]);
    git(&w.repo, &["commit", "-qm", "기획 폴더"]);
    git(&w.repo, &["push", "-q", "origin", "main"]);
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labA", path_str(&w.repo))]);
    let file = w.projects.join("demo/project.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    let entry = format!("path = {:?}\nmachine = \"labA\"\n", path_str(&w.repo));
    assert!(text.contains(&entry), "{text}");
    std::fs::write(&file, text.replace(&entry, &format!("{entry}design = \"docs\"\n"))).unwrap();
    w
}

fn origin_ref(w: &World, name: &str) -> Option<String> {
    let o = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", name])
        .current_dir(w.lab.root.join("origin.git"))
        .output()
        .unwrap();
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn branches(w: &World) -> Value {
    let tree = w.h.ok(&["plan", "list"]);
    tree["repos"].as_array().unwrap().iter().find(|r| r["repo"] == "repo").unwrap()["branches"].clone()
}

#[test]
fn design_branch_saves_on_its_own_branch_pushes_after_rest_and_discards() {
    let w = design_world("tick_secs=3600\ndesign_push_secs=1\n", r#"["git", "--version"]"#);
    let main = origin_ref(&w, "refs/heads/main").unwrap();
    let checkout = git(&w.repo, &["rev-parse", "HEAD"]);
    let opened = w.h.ok(&["plan", "new", "repo", "boss-dex", "--title", "보스 도감"]);
    assert_eq!(opened["branch"]["slug"], "boss-dex", "{opened}");
    assert_eq!(opened["branch"]["title"], "보스 도감");
    assert_eq!(opened["branch"]["head"], main, "origin main에서 갈라진다");

    let doc = "docs/bosses/README.md";
    let body = w.lab.root.join("boss.md");
    std::fs::write(&body, "# 보스 도감\n").unwrap();
    let save = |extra: &[&str]| {
        let mut args = vec!["plan", "save", "repo", doc, "--branch", "boss-dex", "--file", path_str(&body)];
        args.extend(extra);
        w.h.json(&args)
    };
    let first = save(&["--new", "--summary", "보스 도감 시작"]).unwrap();
    let blob = first["blob"].as_str().unwrap().to_string();
    let e = save(&["--new"]).unwrap_err();
    assert!(e.contains("다른 판"), "새 문서인데 이미 있다: {e}");
    let e = save(&["--base", "0123456789abcdef0123456789abcdef01234567"]).unwrap_err();
    assert!(e.contains("다른 판"), "{e}");
    assert!(save(&[]).unwrap_err().contains("--base"), "기준 판 없이는 쓰지 않는다");
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n").unwrap();
    let second = save(&["--base", &blob]).unwrap();
    let head = second["commit"].as_str().unwrap().to_string();

    let shown = w.h.ok(&["plan", "show", "repo", doc, "--branch", "boss-dex"]);
    // 표지 없는 문서는 저장할 때 host가 원장 번호의 ID 표지를 제목 아래 넣는다(t-1957)
    assert_eq!(shown["doc"]["id"], "k-0001");
    assert_eq!(shown["doc"]["text"], "# 보스 도감\n<!-- aaa-id: k-0001 -->\n\n## 해골 왕\n");
    assert_eq!(
        (shown["doc"]["commit"].as_str(), shown["doc"]["branch"].as_str()),
        (Some(head.as_str()), Some("boss-dex"))
    );
    assert!(w.h.json(&["plan", "show", "repo", doc]).is_err(), "main에는 없다");
    let list = branches(&w);
    assert_eq!(list[0]["ahead"], 2, "{list}");
    // 2-3: 고친 파일마다 더하고 뺀 줄 수(웹 반영·버리기 창). ID 표지 줄도 센다
    assert_eq!(list[0]["changed"], serde_json::json!([{ "path": doc, "status": "A", "added": 4, "removed": 0 }]));

    // 쉼(1초) 뒤 한 번 올린다. main과 main checkout은 그대로다
    let pushed =
        wait_for("기획 branch 올림", T, || origin_ref(&w, "refs/heads/design/boss-dex").filter(|h| *h == head));
    assert_eq!(pushed, head);
    wait_for("올린 판 기록", T, || (branches(&w)[0]["pushed"] == head.as_str()).then_some(()));
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), main);
    assert_eq!(git(&w.repo, &["rev-parse", "HEAD"]), checkout);
    assert!(!w.repo.join("docs/bosses").exists(), "main checkout의 작업 파일은 그대로");
    let msg = git(&w.repo, &["log", "-1", "--format=%s", "refs/heads/design/boss-dex"]);
    assert_eq!(msg, "기획: docs/bosses/README.md 고침 (사람)");

    // 올린 뒤 더 쓰고 올리기 전(쉼 창 안)에 버려도 host가 올린 origin branch를 지운다(올린 판에 lease)
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n\n## 서리 거인\n").unwrap();
    save(&["--base", second["blob"].as_str().unwrap()]).unwrap();
    w.h.ok(&["plan", "discard", "repo", "boss-dex"]);
    assert_eq!(origin_ref(&w, "refs/heads/design/boss-dex"), None, "host가 올린 origin branch도 지운다");
    assert!(git(&w.repo, &["branch", "--list", "design/boss-dex"]).is_empty());
    assert_eq!(branches(&w), serde_json::json!([]));
    assert!(!w.lab.root.join("workA/repo/demo-design-boss-dex").exists());
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), main);

    // 사람이 먼저 만든 로컬 branch를 꺼내 썼다 버려도 그 branch는 남긴다
    git(&w.repo, &["branch", "design/private", "main"]);
    let own = git(&w.repo, &["rev-parse", "design/private"]);
    w.h.ok(&["plan", "new", "repo", "private"]);
    w.h.ok(&["plan", "discard", "repo", "private"]);
    assert_eq!(git(&w.repo, &["rev-parse", "design/private"]), own, "있던 로컬 branch는 남는다");
    assert!(!w.lab.root.join("workA/repo/demo-design-private").exists());
    assert_eq!(origin_ref(&w, "refs/heads/design/private"), None);
}

#[test]
fn idle_design_branch_rests_without_losing_commits_and_reopens() {
    // 올리기 전 검사가 실패하는 저장소: 쉬기 전에 올리려다 까닭을 남기고, 올리지 않아도 로컬 branch는 남긴다
    let w =
        design_world("tick_secs=3600\ndesign_push_secs=3600\ndesign_rest_secs=1\n", r#"["git", "no-such-subcommand"]"#);
    w.h.ok(&["plan", "new", "repo", "shop"]);
    let body = w.lab.root.join("shop.md");
    std::fs::write(&body, "# 상점\n").unwrap();
    let saved =
        w.h.ok(&["plan", "save", "repo", "docs/shop.md", "--branch", "shop", "--new", "--file", path_str(&body)]);
    let head = saved["commit"].as_str().unwrap().to_string();
    let place = w.lab.root.join("workA/repo/demo-design-shop");
    assert!(place.exists());
    // fixed wait: 쉼 창(design_rest_secs=1)이 지난 뒤 ticker가 판정해야 쉼이다
    std::thread::sleep(Duration::from_millis(1100));
    w.h.ticks(1);
    // ticker는 기획 branch 일을 따로 띄운다: 쉼이 기록될 때까지 기다린다
    let list = wait_for("기획 branch 쉼", T, || {
        let list = branches(&w);
        (list[0]["state"] == "rest").then_some(list)
    });
    assert!(!place.exists(), "worktree를 돌려준다");
    assert_eq!(git(&w.repo, &["rev-parse", "refs/heads/design/shop"]), head, "branch와 commit은 남는다");
    assert_eq!(origin_ref(&w, "refs/heads/design/shop"), None, "검사를 통과하지 못하면 올리지 않는다");
    assert!(list[0]["push_error"].as_str().is_some_and(|e| e.contains("기획 검사를 통과하지 못해")), "{list}");
    let e =
        w.h.json(&["plan", "save", "repo", "docs/shop.md", "--branch", "shop", "--new", "--file", path_str(&body)])
            .unwrap_err();
    assert!(e.contains("쉬는 기획 branch"), "{e}");
    // 쉬는 branch에 말하면(2-2) 배경 일이 branch를 다시 열고 대화 thread를 띄워 말을 보낸다. 쉼 창이 1초라 ticker가 언제
    // 다시 쉬게 할지 모르므로, 다시 연 것은 말이 thread에 닿은 것과 그 뒤의 열림으로 본다
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    w.h.ok(&["plan", "say", "repo", "shop", "쉼말고유"]);
    let thread = wait_for("쉼 뒤 대화 thread", T, || branches(&w)[0]["thread"].as_str().map(str::to_string));
    let pane =
        w.wait_show("대화 thread brief", &thread, |v| v["brief"] == "delivered")["pane"].as_str().unwrap().to_string();
    w.h.wait_output(&pane, "쉼말고유", T);
    assert_eq!(git(&w.repo, &["rev-parse", "refs/heads/design/shop"]), head, "다시 열어도 그 branch의 commit 그대로");
    let reopened = w.h.ok(&["plan", "new", "repo", "shop"]);
    assert_eq!(
        (reopened["branch"]["state"].as_str(), reopened["branch"]["head"].as_str()),
        (Some("open"), Some(head.as_str()))
    );
    assert!(place.exists());
}

/// 기획 대화(2-2): 저장소가 다른 머신(labB)에 있으면 `aaa plan say`가 그 머신에 대화 thread를 띄우고 말을 brief 뒤 보낸다.
/// thread 세션(front.sock 없음)은 agent 중계로 자기 기획 branch에만 문서·그림을 저장하고, 열기·말하기·다른 branch는 거부된다.
/// 올린 기획 branch에서 갈라진 구현 thread(`--base origin/design/<주제>`)는 그 기획 commit을 받는다. 버리면 대화 thread도 끝난다
#[test]
fn design_chat_thread_on_the_repo_machine_saves_only_its_own_branch() {
    // 쉼 창은 기본(14일): ticker는 git·nudge 차례로도 일찍 깨므로 짧은 쉼 창은 이 시험의 어느 단계에서나 branch를 쉬게 한다.
    // 쉼 뒤 말하기는 `idle_design_branch_rests_without_losing_commits_and_reopens`가 본다
    let (mut w, env) = world_files("tick_secs=3600\ndesign_push_secs=1\n", false, Some("0"), true, |_| Vec::new());
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    let mut w = world_start(w, env);
    w.add_remote();
    git(&w.repo, &["config", "user.name", "t"]);
    git(&w.repo, &["config", "user.email", "t@example.com"]);
    std::fs::create_dir_all(w.repo.join("docs")).unwrap();
    std::fs::write(w.repo.join("docs/README.md"), "# 개요\n").unwrap();
    std::fs::write(w.repo.join("docs/fake.png"), "그림이 아닌 글").unwrap();
    for path in ["docs/old[1].gif", "docs/old1.gif"] {
        std::fs::write(w.repo.join(path), b"GIF89a").unwrap();
    }
    git(&w.repo, &["add", "."]);
    git(&w.repo, &["commit", "-qm", "기획 폴더"]);
    git(&w.repo, &["push", "-q", "origin", "main"]);
    let other = w.lab.root.join("b-src");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["clone", "-q", path_str(&w.lab.root.join("origin.git")), "game"]);
    let game = other.join("game");
    git(&game, &["config", "user.name", "t"]);
    git(&game, &["config", "user.email", "t@example.com"]);
    let game_head = git(&game, &["rev-parse", "HEAD"]);
    std::fs::write(
        w.projects.join("demo/project.toml"),
        format!(
            "schema = 1\nthread_profile = \"fake\"\n\n[[repos]]\npath = {:?}\nmachine = \"labA\"\n\n[[repos]]\npath = {:?}\nmachine = \"labB\"\ndesign = \"docs\"\n",
            path_str(&w.repo),
            path_str(&game)
        ),
    )
    .unwrap();
    let opened = w.h.ok(&["plan", "new", "game", "boss-dex", "--title", "보스 도감"]);
    assert_eq!(opened["branch"]["slug"], "boss-dex", "{opened}");
    let said = w.h.ok(&["plan", "say", "game", "boss-dex", "첫말고유", "보스부터"]);
    assert_eq!(said["branch"]["pending"], 1, "{said}");
    let thread = wait_for("기획 대화 thread", T, || {
        let b = &branches_of(&w, "game")[0];
        b["thread"].as_str().map(str::to_string)
    });
    let shown = w.wait_show("대화 thread brief", &thread, |v| v["brief"] == "delivered");
    assert_eq!(shown["design"], "game/boss-dex", "{shown}");
    assert_eq!(shown["machine"], "labB", "저장소 머신에서 돈다: {shown}");
    let brief = std::fs::read_to_string(Path::new(shown["work_dir"].as_str().unwrap()).join("brief.md")).unwrap();
    assert!(brief.contains("기존 그림 폴더(예: 문서 옆 `assets/`)를 `--dir`로 준다"), "{brief}");
    let pane = shown["pane"].as_str().unwrap().to_string();
    w.h.wait_output(&pane, "첫말고유 보스부터", T);
    wait_for("보낸 말 비움", T, || (branches_of(&w, "game")[0]["pending"] == 0).then_some(()));
    assert!(
        !w.h.ok(&["task", "list"])["tasks"].as_array().unwrap().iter().any(|t| t["thread"] == thread.as_str()),
        "대화 thread는 할 일을 남기지 않는다"
    );

    let body = w.lab.root.join("boss-from-thread.md");
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n").unwrap();
    let py = w.task(
        "design-probe.py",
        &format!(
            r#"
import hashlib, json, os, struct, subprocess, zlib
AA = {bin:?}
def call(*args, ok=True):
    p = subprocess.run([AA, '--json', *args], capture_output=True)
    assert (p.returncode == 0) == ok, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if ok else p.stderr.decode()
assert not os.path.exists(os.path.join(os.environ['AAA_STATE_DIR'], 'host/front.sock'))
# 한도 가까운 문서(500KiB 안팎)도 중계 전체(CLI → hook → agent → host → agent 저장)를 지나 저장·읽기가 되고, 넘으면 agent가 거절한다
big_path = os.path.join(os.path.dirname({body:?}), 'big-from-thread.md')
line = '해골 왕은 얼음 성의 주인이다. 서리 거인을 부린다.\n'
big = '# 큰 문서\n' + line * ((500 * 1024) // len(line.encode()))
assert 480 * 1024 < len(big.encode()) <= 512 * 1024, len(big.encode())
open(big_path, 'w').write(big)
call('plan', 'save', 'game', 'docs/big.md', '--branch', 'boss-dex', '--new', '--file', big_path, '--summary', '큰 문서')
# 저장할 때 host가 제목 아래 ID 표지를 넣는다(다음 줄과 빈 줄로 띄운다, t-1957)
got = call('plan', 'show', 'game', 'docs/big.md', '--branch', 'boss-dex')['doc']
assert got['text'] == big.replace('\n', '\n<!-- aaa-id: ' + got['id'] + ' -->\n\n', 1), got['id']
open(big_path, 'w').write(big + line * 1000)
assert '너무 크다' in call('plan', 'save', 'game', 'docs/big2.md', '--branch', 'boss-dex', '--new', '--file', big_path, ok=False)
# 실제 PNG를 한 조각으로 보낼 수 없는 크기로 만들어 CLI 원격 중계에서 원본 바이트까지 본다.
def png_chunk(tag, data):
    return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))
png = (b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 512, 512, 8, 2, 0, 0, 0))
       + png_chunk(b'IDAT', zlib.compress(b'\0' * (512 * 3 + 1) * 512, 0)) + png_chunk(b'IEND', b''))
assert len(png) > 512 * 1024
image = os.path.join(os.path.dirname({body:?}), 'sketch.png')
with open(image, 'wb') as f:
    f.write(png)
attach = ('plan', 'attach', 'game', '--branch', 'boss-dex', '--file', image)
name = hashlib.sha256(png).hexdigest()[:16] + '.png'
default = subprocess.check_output([AA, *attach], text=True).strip()
assert default == 'docs/' + name, default
nested = call(*attach, '--dir', 'docs/bestiary')
assert nested['path'] == 'docs/bestiary/' + name, nested
assert call(*attach, '--dir', 'docs/bestiary')['commit'] == nested['commit']
for branch in ['other', 'main']:
    assert '자기 branch' in call('plan', 'attach', 'game', '--branch', branch, '--file', image, ok=False)
for outside in ['outside', 'docs/../outside', '/docs']:
    assert '경로' in call(*attach, '--dir', outside, ok=False)
assert 'PNG·JPEG·GIF·WebP' in call('plan', 'attach', 'game', '--branch', 'boss-dex', '--file', {body:?}, ok=False)
oversized = image + '.large'
with open(oversized, 'wb') as f:
    f.truncate(16 * 1024 * 1024 + 1)
assert '16MiB' in call('plan', 'attach', 'game', '--branch', 'boss-dex', '--file', oversized, ok=False)
saved = call('plan', 'save', 'game', 'docs/boss.md', '--branch', 'boss-dex', '--new', '--file', {body:?}, '--summary', '해골 왕 정리')
doc = call('plan', 'show', 'game', 'docs/boss.md', '--branch', 'boss-dex')['doc']
assert doc['text'] == '# 보스 도감\n<!-- aaa-id: ' + doc['id'] + ' -->\n\n## 해골 왕\n' and doc['commit'] == saved['commit'], doc
assert any(r['repo'] == 'game' for r in call('plan', 'list')['repos'])
assert '자기 branch' in call('plan', 'save', 'game', 'docs/x.md', '--branch', 'other', '--new', '--file', {body:?}, ok=False)
# 같은 그림을 옮길 폴더에 먼저 첨부했고, 기존 위치를 참조하는 문서는 삭제 응답에서 찾을 수 있다.
refs = image + '.md'
with open(refs, 'w') as f:
    f.write(f'# 그림 링크\n\n![옛 위치][old]\n\n[old]: ./{{name}}?v=1#preview\n')
call('plan', 'save', 'game', 'docs/refs.md', '--branch', 'boss-dex', '--new', '--file', refs)
def detach(branch, path):
    return ('plan', 'detach', 'game', '--branch', branch, path)
for branch in ['other', 'main']:
    assert '자기 branch' in call(*detach(branch, default), ok=False)
for outside in ['outside.png', 'docs/../outside.png', '/docs/image.png']:
    assert '경로' in call(*detach('boss-dex', outside), ok=False)
assert '그림만 지운다' in call(*detach('boss-dex', 'docs/boss.md'), ok=False)
assert '그림 형식' in call(*detach('boss-dex', 'docs/fake.png'), ok=False)
removed_literal = call(*detach('boss-dex', 'docs/old[1].gif'))
assert removed_literal['path'] == 'docs/old[1].gif' and removed_literal['warnings'] == [], removed_literal
p = subprocess.run([AA, *detach('boss-dex', default)], capture_output=True, text=True, check=True)
assert p.stdout.startswith(default + ' 삭제 · commit '), p.stdout
assert '경고:' in p.stderr and 'docs/refs.md' in p.stderr, p.stderr
assert '기획 폴더에 없다' in call(*detach('boss-dex', default), ok=False)
doc = call('plan', 'show', 'game', 'docs/refs.md', '--branch', 'boss-dex')['doc']
with open(refs, 'w') as f:
    f.write(f'# 그림 링크\n\n![새 위치]({{nested["path"].removeprefix("docs/")}})\n')
moved = call('plan', 'save', 'game', 'docs/refs.md', '--branch', 'boss-dex', '--base', doc['blob'], '--file', refs, '--summary', '그림 경로 고침')
# 열기·말하기·버리기는 중계 목록 밖이라 CLI가 보내지 않는다(host·agent 중계 검사는 단위 시험)
for args in [('plan', 'new', 'game', 'other'), ('plan', 'say', 'game', 'boss-dex', 'x'), ('plan', 'discard', 'game', 'boss-dex')]:
    call(*args, ok=False)
print('design-relay-ok', moved['commit'])
print('design-image', nested['commit'], nested['path'], default)
print('design-delete', p.stdout.split()[-1])
"#,
            bin = lab::BIN,
        ),
    );
    let out = w.lab.root.join("design-probe.out");
    let done = w.lab.root.join("design-probe.done");
    let script = w.task(
        "design-probe.sh",
        &format!(
            "python3 {py:?} > {out:?} 2>&1\nprintf '%s' \"$?\" > {done:?}.tmp\nmv {done:?}.tmp {done:?}\necho design-probe-done\n"
        ),
    );
    w.h.ok(&["pane", "run", &pane, &format!("post-probe {script}")]);
    w.h.wait_output(&pane, "design-probe-done", T);
    let output = std::fs::read_to_string(out).unwrap();
    assert_eq!(std::fs::read_to_string(done).unwrap(), "0", "{output}");
    let commit = output.split_whitespace().skip_while(|s| *s != "design-relay-ok").nth(1).unwrap().to_string();
    assert_eq!(
        git(&game, &["log", "-1", "--format=%s", "refs/heads/design/boss-dex"]),
        format!("기획: 그림 경로 고침 ({thread})"),
        "저장한 thread가 commit 메시지에 남는다"
    );
    assert_eq!(git(&game, &["rev-parse", "HEAD"]), game_head, "저장소 checkout은 그대로");
    // pane 밖 프로세스가 대화 thread의 세션을 대도(coordinator pane·떼어 낸 프로세스와 같은 꼴) 그 thread로 쓰지 못한다
    let claimed =
        w.h.cmd(&[
            "--json",
            "plan",
            "save",
            "game",
            "docs/claim.md",
            "--branch",
            "boss-dex",
            "--new",
            "--file",
            path_str(&body),
        ])
        .env("AAA_SESSION_ID", &pane)
        .capture_output()
        .unwrap();
    assert!(!claimed.status.success(), "{}", String::from_utf8_lossy(&claimed.stdout));
    assert!(
        String::from_utf8_lossy(&claimed.stderr).contains("확인하지 못했다"),
        "{}",
        String::from_utf8_lossy(&claimed.stderr)
    );
    for actor in [None, Some(pane.as_str())] {
        let mut cmd = w.h.cmd(&["plan", "detach", "game", "--branch", "boss-dex", "docs/old1.gif"]);
        if let Some(session) = actor {
            cmd.env("AAA_SESSION_ID", session);
        } else {
            cmd.env_remove("AAA_SESSION_ID");
        }
        let result = cmd.capture_output().unwrap();
        assert!(!result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    }
    assert!(!game.join("docs/boss.md").exists());

    // 올린 기획 branch에서 갈라진 구현 thread는 그 기획 commit을 받는다(2-3 「main에 반영」·3단계 구현 맡기기의 바탕)
    wait_for("기획 branch 올림", T, || origin_ref(&w, "refs/heads/design/boss-dex").filter(|h| *h == commit));
    let built = w.start(&[
        "--title",
        "기획 위 구현",
        "--machine",
        "labA",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--base",
        "origin/design/boss-dex",
        "--task-file",
        &w.task("on-design.md", "noreport"),
    ]);
    let cwd = PathBuf::from(built["cwd"].as_str().unwrap());
    git(&cwd, &["merge-base", "--is-ancestor", &commit, "HEAD"]);
    let boss = std::fs::read_to_string(cwd.join("docs/boss.md")).unwrap();
    let id = aaa_proto::design_text_id(&boss).expect("저장한 문서의 ID 표지");
    assert_eq!(boss, format!("# 보스 도감\n<!-- aaa-id: {id} -->\n\n## 해골 왕\n"));
    let mut image = output.lines().find_map(|line| line.strip_prefix("design-image ")).unwrap().split_whitespace();
    let image_commit = image.next().unwrap();
    assert!(
        git(&game, &["show", "-s", "--format=%s", image_commit]).ends_with(&format!("({thread})")),
        "그림 commit에 작성한 thread가 남는다"
    );
    let source = std::fs::read(w.lab.root.join("sketch.png")).unwrap();
    let new_path = image.next().unwrap();
    let old_path = image.next().unwrap();
    let deletion = output.lines().find_map(|line| line.strip_prefix("design-delete ")).unwrap();
    assert_eq!(
        git(&game, &["show", "--format=%s", "--name-status", deletion]),
        format!("기획: 그림 {} 삭제 ({thread})\n\nD\t{old_path}", old_path.rsplit('/').next().unwrap())
    );
    assert_eq!(std::fs::read(cwd.join(new_path)).unwrap(), source, "origin에서 받은 그림의 원본 바이트");
    assert!(!cwd.join(old_path).exists(), "origin 기획 branch에서 옛 그림이 없어졌다");
    assert!(!cwd.join("docs/old[1].gif").exists());
    assert!(cwd.join("docs/old1.gif").exists(), "비슷한 이름의 그림은 남는다");
    assert!(game.join("docs/old[1].gif").exists(), "main에 있는 그림은 남는다");
    assert!(!game.join(new_path).exists() && !game.join(old_path).exists(), "main checkout은 그대로다");
    assert!(
        std::fs::read_to_string(cwd.join("docs/refs.md")).unwrap().contains(new_path.strip_prefix("docs/").unwrap())
    );

    // 2-3: 사람이 고친 문서는 다음 말 앞에 알리고, 대화 사건(새 대화·저장·올림·사람 고침)과 고친 줄 수가 branch 보기에 실린다
    let doc = w.h.ok(&["plan", "show", "game", "docs/boss.md", "--branch", "boss-dex"])["doc"].clone();
    let human = w.lab.root.join("human.md");
    std::fs::write(&human, "# 보스 도감\n\n## 해골 왕\n\n## 서리 거인\n").unwrap();
    w.h.ok(&[
        "plan",
        "save",
        "game",
        "docs/boss.md",
        "--branch",
        "boss-dex",
        "--base",
        doc["blob"].as_str().unwrap(),
        "--file",
        path_str(&human),
    ]);
    let b = branches_of(&w, "game")[0].clone();
    assert_eq!(b["edited"], serde_json::json!(["docs/boss.md"]), "{b}");
    let kinds: Vec<&str> = b["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    for want in ["open", "create", "push", "edit"] {
        assert!(kinds.contains(&want), "{want}: {kinds:?}");
    }
    let changed = b["changed"].as_array().unwrap().iter().find(|c| c["path"] == "docs/boss.md").unwrap().clone();
    // 사람 글에 표지 줄이 없어도 같은 경로의 번호로 다시 넣는다(ID 표지 줄 하나가 더해진 6줄, t-1957)
    assert_eq!((changed["status"].as_str(), changed["added"].as_u64()), (Some("A"), Some(6)), "{changed}");
    let again = w.h.ok(&["plan", "show", "game", "docs/boss.md", "--branch", "boss-dex"])["doc"].clone();
    assert_eq!(again["id"].as_str(), Some(id.as_str()), "같은 번호");
    w.h.ok(&["plan", "say", "game", "boss-dex", "넷째말고유"]);
    w.h.wait_output(&pane, "(문서 고침: docs/boss.md)", T);
    wait_for("알린 고침 비움", T, || {
        (branches_of(&w, "game")[0]["edited"] == serde_json::json!([])).then_some(())
    });

    // 보류된 대화 thread는 다음 말에 같은 thread로 잇는다(이야기가 없어 쉬어 내린 것과 같은 길)
    let parked = w.h.ok(&["thread", "park", "demo", &thread, "--now"]);
    assert_eq!(parked["status"], "parked", "{parked}");
    w.h.ok(&["plan", "say", "game", "boss-dex", "둘째말고유"]);
    let back = w.wait_show("보류에서 이음", &thread, |v| v["status"] == "open" && v["brief"] == "delivered");
    let pane = back["pane"].as_str().unwrap().to_string();
    w.h.wait_output(&pane, "둘째말고유", T);
    wait_for("둘째 말 보냄", T, || {
        let b = &branches_of(&w, "game")[0];
        (b["pending"] == 0 && b["thread"] == thread.as_str()).then_some(())
    });

    // main에 반영: 올린 기획 branch에서 갈라진 worktree thread를 저장소 머신에 띄운다(그 thread가 main을 합치고 착지 창을 받는다)
    let applied = w.h.ok(&["plan", "apply", "game", "boss-dex"]);
    let apply = applied["branch"]["apply"].as_str().unwrap().to_string();
    let head = applied["branch"]["head"].as_str().unwrap().to_string();
    assert_eq!(applied["branch"]["pushed"], head.as_str(), "올린 뒤 띄운다: {applied}");
    let shown = w.wait_show("반영 thread brief", &apply, |v| v["brief"] == "delivered");
    assert_eq!((shown["thread_kind"].as_str(), shown["machine"].as_str()), (Some("worktree"), Some("labB")), "{shown}");
    let brief = std::fs::read_to_string(Path::new(shown["work_dir"].as_str().unwrap()).join("brief.md")).unwrap();
    assert!(brief.contains("합치기 전에 `git diff --name-status origin/main...HEAD`"), "{brief}");
    assert!(brief.contains("문서별 상대 링크도 고쳐"), "{brief}");
    assert!(brief.contains("옮긴 그림의 이전 경로와 새 경로를 보고에 적는다"), "{brief}");
    let applied_cwd = PathBuf::from(shown["cwd"].as_str().unwrap());
    git(&applied_cwd, &["merge-base", "--is-ancestor", &head, "HEAD"]);
    assert!(applied_cwd.join("docs/boss.md").exists());
    let e = w.h.json(&["plan", "apply", "game", "boss-dex"]).unwrap_err();
    assert!(e.contains("이미 반영 중"), "{e}");

    // 띄우는 중에 버려도 그 thread는 뜬 뒤 끝난다(버린 branch에 남는 thread가 없다)
    w.h.ok(&["plan", "new", "game", "race"]);
    w.h.ok(&["plan", "say", "game", "race", "경합말"]);
    let race = wait_for("경합 대화 thread 생김", T, || {
        let list = w.h.ok(&["thread", "list", "demo"]);
        list["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["design"] == "game/race")
            .map(|t| t["id"].as_str().unwrap().to_string())
    });
    w.h.ok(&["plan", "discard", "game", "race"]);
    w.wait_show("경합 thread 끝남", &race, |v| v["status"] == "resolved");
    assert!(branches_of(&w, "game").as_array().unwrap().iter().all(|b| b["slug"] != "race"));

    w.h.ok(&["plan", "discard", "game", "boss-dex"]);
    assert_eq!(w.show(&thread)["status"], "resolved", "버리면 대화 thread도 끝난다");
    assert_eq!(branches_of(&w, "game"), serde_json::json!([]));
    assert_eq!(origin_ref(&w, "refs/heads/design/boss-dex"), None);
}

fn branches_of(w: &World, repo: &str) -> Value {
    let tree = w.h.ok(&["plan", "list"]);
    tree["repos"].as_array().unwrap().iter().find(|r| r["repo"] == repo).unwrap()["branches"].clone()
}

/// coordinator의 열기·말하기(t-1928): 사용자의 「아이디어」 요청을 할 일 없이 기획 대화로 넘긴다. coordinator pane 안의 실제
/// CLI가 `plan new`·`plan say`로 branch를 열고 대화 thread를 띄워 말을 보낸다(만든 이 coordinator). 같은 pane의 저장·버리기·
/// 반영은 거부되고, 버리기는 사람이 한다
#[test]
fn coordinator_opens_a_plan_chat_and_says_but_does_not_write() {
    let w = design_world("tick_secs=3600\ndesign_push_secs=3600\n", r#"["git", "--version"]"#);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let coord = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let cpane = coord["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&cpane, "fake>");
    let said = w.lab.root.join("coord-say.md");
    std::fs::write(&said, "사용자: 보스 도감을 만들자 코디말고유\n").unwrap();
    let py = w.task(
        "coord-plan-probe.py",
        &format!(
            r#"
import json, subprocess
AA = {bin:?}
def call(*args, ok=True):
    p = subprocess.run([AA, '--json', *args], capture_output=True)
    assert (p.returncode == 0) == ok, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if ok else p.stderr.decode()
opened = call('plan', 'new', 'repo', 'boss-idea', '--title', '보스 도감 아이디어')['branch']
assert (opened['slug'], opened['by']) == ('boss-idea', 'coordinator'), opened
assert call('plan', 'say', 'repo', 'boss-idea', '--file', {said:?})['branch']['pending'] == 1
# 이어지는 말: 목록에서 같은 주제를 찾아 그 주제에 말한다
repo = next(r for r in call('plan', 'list')['repos'] if r['repo'] == 'repo')
assert [b['slug'] for b in repo['branches']] == ['boss-idea'], repo
call('plan', 'say', 'repo', 'boss-idea', '사용자: 해골 왕부터 코디둘째말')
for args in [('plan', 'save', 'repo', 'docs/x.md', '--branch', 'boss-idea', '--new', '--file', {said:?}),
             ('plan', 'discard', 'repo', 'boss-idea'), ('plan', 'apply', 'repo', 'boss-idea')]:
    assert '기획 branch에 쓰지 않는다' in call(*args, ok=False), args
print('coord-plan-ok')
"#,
            bin = lab::BIN,
        ),
    );
    let out = w.lab.root.join("coord-plan-probe.out");
    let done = w.lab.root.join("coord-plan-probe.done");
    let script = w.task(
        "coord-plan-probe.sh",
        &format!(
            "python3 {py:?} > {out:?} 2>&1\nprintf '%s' \"$?\" > {done:?}.tmp\nmv {done:?}.tmp {done:?}\necho coord-plan-probe-done\n"
        ),
    );
    w.h.ok(&["pane", "run", &cpane, &format!("post-probe {script}")]);
    w.h.wait_output(&cpane, "coord-plan-probe-done", T);
    let output = std::fs::read_to_string(out).unwrap();
    assert_eq!(std::fs::read_to_string(done).unwrap(), "0", "{output}");
    assert!(output.contains("coord-plan-ok"), "{output}");

    let thread = wait_for("기획 대화 thread", T, || branches(&w)[0]["thread"].as_str().map(str::to_string));
    let shown = w.wait_show("대화 thread brief", &thread, |v| v["brief"] == "delivered");
    assert_eq!(shown["design"], "repo/boss-idea", "{shown}");
    let pane = shown["pane"].as_str().unwrap().to_string();
    w.h.wait_output(&pane, "코디말고유", T);
    w.h.wait_output(&pane, "코디둘째말", T);
    wait_for("보낸 말 비움", T, || (branches(&w)[0]["pending"] == 0).then_some(()));
    let list = branches(&w);
    assert_eq!((list[0]["slug"].as_str(), list[0]["by"].as_str()), (Some("boss-idea"), Some("coordinator")), "{list}");
    assert!(
        !w.h.ok(&["task", "list"])["tasks"].as_array().unwrap().iter().any(|t| t["thread"] == thread.as_str()),
        "대화 thread는 할 일을 남기지 않는다"
    );
    w.h.ok(&["plan", "discard", "repo", "boss-idea"]);
    assert_eq!(w.show(&thread)["status"], "resolved", "사람이 버리면 대화 thread도 끝난다");
    assert_eq!(branches(&w), serde_json::json!([]));
}

/// 「main에 반영」이 프로젝트 실행 한도에 걸리면(t-1955) 거절하지 않고 예약한다: 같은 branch 거듭 예약은 막고, 취소할 수 있고,
/// host가 다시 떠도 남고, 자리가 나면 host가 그때의 올린 판에서 반영 thread를 띄운다
#[test]
fn apply_over_the_thread_limit_waits_for_a_slot_and_starts_itself() {
    let mut w = design_world("tick_secs=3600\ndesign_push_secs=3600\n", r#"["git", "--version"]"#);
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let busy = w.start(&[
        "--title",
        "자리",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("busy.md", "noreport"),
    ]);
    let busy = busy["id"].as_str().unwrap().to_string();
    w.h.ok(&["plan", "new", "repo", "shop", "--title", "상점"]);
    let body = w.lab.root.join("shop.md");
    std::fs::write(&body, "# 상점\n").unwrap();
    let saved =
        w.h.ok(&["plan", "save", "repo", "docs/shop.md", "--branch", "shop", "--new", "--file", path_str(&body)]);

    let applied = w.h.json(&["plan", "apply", "repo", "shop"]).unwrap_or_else(|e| panic!("한도에서 거절했다: {e}"));
    let b = &applied["branch"];
    assert!(b["apply"].is_null(), "아직 띄우지 않는다: {applied}");
    assert_eq!(b["pushed"], saved["commit"], "올리기는 먼저 한다: {applied}");
    assert!(b["apply_wait"]["reason"].as_str().is_some_and(|r| r.contains("프로젝트 실행 한도 1")), "{applied}");
    let e = w.h.json(&["plan", "apply", "repo", "shop"]).unwrap_err();
    assert!(e.contains("이미 반영을 예약했다"), "같은 branch는 한 번만 예약한다: {e}");

    // 취소하면 예약이 사라지고, 다시 취소하면 없다고 알린다
    let cancelled = w.h.ok(&["plan", "apply", "repo", "shop", "--cancel"]);
    assert!(cancelled["branch"]["apply_wait"].is_null(), "{cancelled}");
    assert!(w.h.json(&["plan", "apply", "repo", "shop", "--cancel"]).unwrap_err().contains("반영 예약이 없다"));
    w.h.ticks(1);
    assert!(branches(&w)[0]["apply"].is_null(), "취소한 예약은 띄우지 않는다");

    // 다시 예약하고, 기다리는 사이 더 쓰고, host가 다시 떠도 예약은 남는다
    w.h.ok(&["plan", "apply", "repo", "shop"]);
    std::fs::write(&body, "# 상점\n\n## 물약\n").unwrap();
    let later = w.h.ok(&[
        "plan",
        "save",
        "repo",
        "docs/shop.md",
        "--branch",
        "shop",
        "--base",
        saved["blob"].as_str().unwrap(),
        "--file",
        path_str(&body),
    ]);
    let later = later["commit"].as_str().unwrap().to_string();
    w.lab.kill("host");
    w.spawn_host("host-apply-wait");
    wait_for("재연결", T, || w.h.connected("labA").then_some(()));
    w.h.ticks(1);
    let b = branches(&w)[0].clone();
    assert!(b["apply"].is_null() && b["apply_wait"]["reason"].is_string(), "자리가 없으면 계속 기다린다: {b}");
    let kinds: Vec<&str> = b["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"apply_wait") && kinds.contains(&"apply_cancel"), "{kinds:?}");

    // 예약한 branch는 쉼 창(3600초)을 기다리지 않고 배경 일이 올린다. 올리는 동안 ticker는 그 branch를 기다리지 않고 넘긴다
    wait_for("예약 중 쓴 판 올림", T, || (branches(&w)[0]["pushed"] == later.as_str()).then_some(()));
    // 자리가 나면 host가 띄운다: 기다리는 사이 쓴 것까지 올린 판에서 갈라진다
    w.h.ok(&["thread", "resolve", &busy, "--skip-copy"]);
    w.h.ticks(1);
    let apply = wait_for("예약한 반영 thread", T, || branches(&w)[0]["apply"].as_str().map(str::to_string));
    let b = branches(&w)[0].clone();
    assert!(b["apply_wait"].is_null(), "{b}");
    assert_eq!(b["pushed"], later.as_str(), "{b}");
    let shown = w.wait_show("반영 thread 준비", &apply, |v| v["cwd"].is_string() && v["status"] != "starting");
    assert_eq!(shown["thread_kind"], "worktree", "{shown}");
    let cwd = PathBuf::from(shown["cwd"].as_str().unwrap());
    git(&cwd, &["merge-base", "--is-ancestor", &later, "HEAD"]);
    // 저장할 때 host가 넣은 ID 표지(t-1957)와 함께
    let shop = std::fs::read_to_string(cwd.join("docs/shop.md")).unwrap();
    let id = aaa_proto::design_text_id(&shop).expect("저장한 문서의 ID 표지");
    assert_eq!(shop, format!("# 상점\n<!-- aaa-id: {id} -->\n\n## 물약\n"));
    w.h.ticks(1);
    assert_eq!(branches(&w)[0]["apply"], apply.as_str(), "한 번만 띄운다");
    assert_eq!(
        w.h.ok(&["thread", "list"])["threads"].as_array().unwrap().iter().filter(|t| t["status"] != "resolved").count(),
        1
    );
}
