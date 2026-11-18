//! 기획 저장(t-1894 2단계, t-2002 main 직접 저장): 실제 host·agent·Git으로 CLI `aaa plan new|save|show|log|list|discard`를 본다.
//! 저장은 바로 origin main에 들어가고(저장소 문서 검사를 지나야), main checkout은 끝까지 그대로다.
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

/// origin main 판의 파일 글(없으면 None)
fn origin_file(w: &World, path: &str) -> Option<String> {
    let o = std::process::Command::new("git")
        .args(["show", &format!("refs/heads/main:{path}")])
        .current_dir(w.lab.root.join("origin.git"))
        .output()
        .unwrap();
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn origin_log(w: &World, rev: &str) -> String {
    let o = std::process::Command::new("git")
        .args(["log", "-1", "--format=%s", rev])
        .current_dir(w.lab.root.join("origin.git"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// 저장은 한 commit으로 origin main에 바로 들어간다(main checkout은 그대로). 판이 다르면 거절, 저장소 문서 검사를 통과하지 못하면
/// 올리지 않고 까닭을 보인다. 함께 저장(`--also`)은 한 commit, 이력과 옛 판 읽기, 대화 끝내기, 없어진 「main에 반영」
#[test]
fn design_saves_go_straight_to_main_with_checks_and_history() {
    // 검사: docs/generated.md를 바꾸면 거절(생성 문서 꼴), 인자의 {base}·{head}는 agent가 바꾼다. git만 써서 세 OS에서 돈다
    let w = design_world(
        "tick_secs=3600\n",
        r#"["git", "diff", "--quiet", "{base}", "{head}", "--", "docs/generated.md"]"#,
    );
    let main = origin_ref(&w, "refs/heads/main").unwrap();
    let checkout = git(&w.repo, &["rev-parse", "HEAD"]);
    let opened = w.h.ok(&["plan", "new", "repo", "boss-dex", "--title", "보스 도감"]);
    assert_eq!(
        (opened["branch"]["slug"].as_str(), opened["branch"]["title"].as_str()),
        (Some("boss-dex"), Some("보스 도감"))
    );
    assert_eq!(opened["branch"]["base"], main, "대화를 연 main 판");
    assert!(w.lab.root.join("workA/repo/demo-design-main").exists(), "main 기획 자리");
    assert_eq!(origin_ref(&w, "refs/heads/design/boss-dex"), None, "branch를 만들지 않는다");

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
    let commit = first["commit"].as_str().unwrap().to_string();
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), commit, "저장은 곧 origin main");
    assert_eq!(origin_ref(&w, &format!("{commit}^")).unwrap(), main, "한 commit");
    assert_eq!(origin_log(&w, &commit), "기획: 보스 도감 시작 (사람)");
    let e = save(&["--new"]).unwrap_err();
    assert!(e.contains("다른 판"), "새 문서인데 이미 있다: {e}");
    let e = save(&["--base", "0123456789abcdef0123456789abcdef01234567"]).unwrap_err();
    assert!(e.contains("다른 판"), "{e}");
    assert!(save(&[]).unwrap_err().contains("--base"), "기준 판 없이는 쓰지 않는다");
    std::fs::write(&body, "# 보스 도감\n\n## 해골 왕\n").unwrap();
    let second = save(&["--base", &blob]).unwrap();
    let head = second["commit"].as_str().unwrap().to_string();

    // 곧바로 읽어도 저장한 글이다(host가 트리를 새 판으로 다시 읽는다). 표지 없는 문서는 저장할 때 ID 표지를 넣는다(t-1957)
    let shown = w.h.ok(&["plan", "show", "repo", doc]);
    assert_eq!(shown["doc"]["id"], "k-0001");
    assert_eq!(shown["doc"]["text"], "# 보스 도감\n<!-- aaa-id: k-0001 -->\n\n## 해골 왕\n");
    assert_eq!(shown["doc"]["commit"], head.as_str());
    assert_eq!(git(&w.repo, &["rev-parse", "HEAD"]), checkout, "main checkout은 그대로");
    assert!(!w.repo.join("docs/bosses").exists(), "main checkout의 작업 파일은 그대로");

    // 저장소 문서 검사를 통과하지 못하면 올리지 않는다
    let generated = w.lab.root.join("generated.md");
    std::fs::write(&generated, "# 생성\n").unwrap();
    let e =
        w.h.json(&["plan", "save", "repo", "docs/generated.md", "--new", "--file", path_str(&generated)]).unwrap_err();
    assert!(e.contains("검사를 통과하지 못해") && e.contains("종료 코드 Some(1)"), "{e}");
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), head, "올리지 않았다");

    // 서로 링크하는 두 문서를 한 commit으로
    let a = w.lab.root.join("a.md");
    let b = w.lab.root.join("b.md");
    std::fs::write(&a, "# 가\n\n[나](b.md)\n").unwrap();
    std::fs::write(&b, "# 나\n\n[가](a.md)\n").unwrap();
    let both = w.h.ok(&[
        "plan",
        "save",
        "repo",
        "docs/a.md",
        "--new",
        "--file",
        path_str(&a),
        "--also",
        &format!("docs/b.md={}", path_str(&b)),
    ]);
    let both = both["commit"].as_str().unwrap().to_string();
    assert_eq!(origin_ref(&w, &format!("{both}^")).unwrap(), head, "한 commit");
    assert!(origin_file(&w, "docs/b.md").unwrap().starts_with("# 나\n<!-- aaa-id: k-"), "함께 저장한 문서에도 ID 표지");

    // 이력(main이 가졌던 판, 새것부터)과 옛 판 읽기
    let log = w.h.ok(&["plan", "log", "repo", doc]);
    let entries = log["entries"].as_array().unwrap();
    let got: Vec<(&str, &str)> =
        entries.iter().map(|e| (e["commit"].as_str().unwrap(), e["by"].as_str().unwrap())).collect();
    assert_eq!(got, [(head.as_str(), "사람"), (commit.as_str(), "사람")], "{log}");
    let old = w.h.ok(&["plan", "show", "repo", doc, "--at", &commit]);
    assert_eq!(old["doc"]["text"], "# 보스 도감\n<!-- aaa-id: k-0001 -->\n");
    assert!(w.h.json(&["plan", "show", "repo", doc, "--at", "0123456789abcdef0123456789abcdef01234567"]).is_err());

    // 대화 사건: 연 것·사람 고침(다음 말 앞에 알림)
    let list = branches(&w);
    let kinds: Vec<&str> = list[0]["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["open", "edit", "edit"], "{list}");
    assert_eq!(list[0]["edited"], serde_json::json!([doc]));
    assert_eq!(list[0]["head"], head.as_str(), "그 대화의 마지막 저장");

    // 「main에 반영」은 없어졌다. 끝내기는 기록만 지우고 main은 그대로다
    assert!(w.h.json(&["plan", "apply", "repo", "boss-dex"]).unwrap_err().contains("없어졌다"));
    w.h.ok(&["plan", "discard", "repo", "boss-dex"]);
    assert_eq!(branches(&w), serde_json::json!([]));
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), both);
    assert!(origin_file(&w, doc).is_some());
}

/// 기획 대화(2-2): 저장소가 다른 머신(labB)에 있으면 `aaa plan say`가 그 머신에 대화 thread를 띄우고 말을 brief 뒤 보낸다.
/// thread 세션(front.sock 없음)은 agent 중계로 자기 기획 branch에만 문서·그림을 저장하고, 열기·말하기·다른 branch는 거부된다.
/// 올린 기획 branch에서 갈라진 구현 thread(`--base origin/design/<주제>`)는 그 기획 commit을 받는다. 버리면 대화 thread도 끝난다
#[test]
fn design_chat_thread_on_the_repo_machine_saves_only_its_own_branch() {
    let (mut w, env) = world_files("tick_secs=3600\n", false, Some("0"), true, |_| Vec::new());
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
    assert!(brief.contains("저장은 바로 origin main에 들어간다"), "{brief}");
    assert!(brief.contains("기획 손 그림") && brief.contains("`--name`"), "{brief}");
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
call('plan', 'save', 'game', 'docs/big.md', '--new', '--file', big_path, '--summary', '큰 문서')
# 저장할 때 host가 제목 아래 ID 표지를 넣는다(다음 줄과 빈 줄로 띄운다, t-1957)
got = call('plan', 'show', 'game', 'docs/big.md')['doc']
assert got['text'] == big.replace('\n', '\n<!-- aaa-id: ' + got['id'] + ' -->\n\n', 1), got['id']
open(big_path, 'w').write(big + line * 1000)
assert '너무 크다' in call('plan', 'save', 'game', 'docs/big2.md', '--new', '--file', big_path, ok=False)
# 실제 PNG를 한 조각으로 보낼 수 없는 크기로 만들어 CLI 원격 중계에서 원본 바이트까지 본다.
def png_chunk(tag, data):
    return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))
png = (b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 512, 512, 8, 2, 0, 0, 0))
       + png_chunk(b'IDAT', zlib.compress(b'\0' * (512 * 3 + 1) * 512, 0)) + png_chunk(b'IEND', b''))
assert len(png) > 512 * 1024
image = os.path.join(os.path.dirname({body:?}), 'sketch.png')
with open(image, 'wb') as f:
    f.write(png)
attach = ('plan', 'attach', 'game', '--file', image)
name = hashlib.sha256(png).hexdigest()[:16] + '.png'
default = subprocess.check_output([AA, *attach], text=True).strip()
assert default == 'docs/' + name, default
nested = call(*attach, '--dir', 'docs/bestiary')
assert nested['path'] == 'docs/bestiary/' + name, nested
assert call(*attach, '--dir', 'docs/bestiary')['commit'] == nested['commit']
# 다른 대화로는 쓰지 못한다(주지 않거나 `main`이면 자기 대화)
assert '자기 대화' in call('plan', 'attach', 'game', '--branch', 'other', '--file', image, ok=False)
named = call(*attach, '--dir', 'docs/mockups', '--name', 'boss-hud-mockup.png')
assert named['path'] == 'docs/mockups/boss-hud-mockup.png', named
assert '확장자' in call(*attach, '--name', 'boss.gif', ok=False)
for outside in ['outside', 'docs/../outside', '/docs']:
    assert '경로' in call(*attach, '--dir', outside, ok=False)
assert 'PNG·JPEG·GIF·WebP' in call('plan', 'attach', 'game', '--file', {body:?}, ok=False)
oversized = image + '.large'
with open(oversized, 'wb') as f:
    f.truncate(16 * 1024 * 1024 + 1)
assert '16MiB' in call('plan', 'attach', 'game', '--file', oversized, ok=False)
saved = call('plan', 'save', 'game', 'docs/boss.md', '--new', '--file', {body:?}, '--summary', '해골 왕 정리')
doc = call('plan', 'show', 'game', 'docs/boss.md')['doc']
assert doc['text'] == '# 보스 도감\n<!-- aaa-id: ' + doc['id'] + ' -->\n\n## 해골 왕\n' and doc['commit'] == saved['commit'], doc
assert any(r['repo'] == 'game' for r in call('plan', 'list')['repos'])
assert '자기 대화' in call('plan', 'save', 'game', 'docs/x.md', '--branch', 'other', '--new', '--file', {body:?}, ok=False)
# 같은 그림을 옮길 폴더에 먼저 첨부했고, 기존 위치를 참조하는 문서는 삭제 응답에서 찾을 수 있다.
refs = image + '.md'
with open(refs, 'w') as f:
    f.write(f'# 그림 링크\n\n![옛 위치][old]\n\n[old]: ./{{name}}?v=1#preview\n')
call('plan', 'save', 'game', 'docs/refs.md', '--new', '--file', refs)
def detach(branch, path):
    return ('plan', 'detach', 'game', '--branch', branch, path)
assert '자기 대화' in call(*detach('other', default), ok=False)
for outside in ['outside.png', 'docs/../outside.png', '/docs/image.png']:
    assert '경로' in call(*detach('boss-dex', outside), ok=False)
assert '그림만 지운다' in call(*detach('boss-dex', 'docs/boss.md'), ok=False)
removed_literal = call(*detach('boss-dex', 'docs/old[1].gif'))
assert removed_literal['path'] == 'docs/old[1].gif' and removed_literal['warnings'] == [], removed_literal
p = subprocess.run([AA, *detach('boss-dex', default)], capture_output=True, text=True, check=True)
assert p.stdout.startswith(default + ' 삭제 · commit '), p.stdout
assert '경고:' in p.stderr and 'docs/refs.md' in p.stderr, p.stderr
assert 'main에 없다' in call(*detach('boss-dex', default), ok=False)
doc = call('plan', 'show', 'game', 'docs/refs.md')['doc']
with open(refs, 'w') as f:
    f.write(f'# 그림 링크\n\n![새 위치]({{nested["path"].removeprefix("docs/")}})\n')
moved = call('plan', 'save', 'game', 'docs/refs.md', '--base', doc['blob'], '--file', refs, '--summary', '그림 경로 고침')
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
    assert_eq!(origin_ref(&w, "refs/heads/main").unwrap(), commit, "대화 thread의 저장도 곧 origin main");
    assert_eq!(
        origin_log(&w, &commit),
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

    // origin main에서 갈라진 구현 thread는 그 기획 commit을 받는다(구현 맡기기의 바탕)
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
        "origin/main",
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
    assert!(!cwd.join(old_path).exists(), "origin main에서 옛 그림이 없어졌다");
    assert!(!cwd.join("docs/old[1].gif").exists());
    assert!(cwd.join("docs/old1.gif").exists(), "비슷한 이름의 그림은 남는다");
    assert!(game.join("docs/old[1].gif").exists(), "저장소 checkout의 작업 파일은 그대로");
    assert!(!game.join(new_path).exists() && !game.join(old_path).exists(), "main checkout은 그대로다");
    assert!(
        std::fs::read_to_string(cwd.join("docs/refs.md")).unwrap().contains(new_path.strip_prefix("docs/").unwrap())
    );

    // 2-3: 사람이 고친 문서는 다음 말 앞에 알리고, 대화 사건(새 대화·저장·사람 고침)이 대화 보기에 실린다
    let doc = w.h.ok(&["plan", "show", "game", "docs/boss.md"])["doc"].clone();
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
    for want in ["open", "create", "save", "edit"] {
        assert!(kinds.contains(&want), "{want}: {kinds:?}");
    }
    assert!(!kinds.contains(&"push"), "올림 사건은 없다(저장이 곧 main): {kinds:?}");
    // 사람 글에 표지 줄이 없어도 같은 경로의 번호로 다시 넣는다(t-1957)
    let again = w.h.ok(&["plan", "show", "game", "docs/boss.md"])["doc"].clone();
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

    assert!(w.h.json(&["plan", "apply", "game", "boss-dex"]).unwrap_err().contains("없어졌다"));

    // 띄우는 중에 끝내도 그 thread는 뜬 뒤 끝난다(끝낸 대화에 남는 thread가 없다)
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
    assert_eq!(w.show(&thread)["status"], "resolved", "끝내면 대화 thread도 끝난다");
    assert_eq!(branches_of(&w, "game"), serde_json::json!([]));
    assert!(origin_file(&w, "docs/boss.md").is_some(), "끝내도 main에 저장한 글은 그대로");
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
    let w = design_world("tick_secs=3600\n", r#"["git", "--version"]"#);
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
             ('plan', 'discard', 'repo', 'boss-idea')]:
    assert '기획에 쓰지 않는다' in call(*args, ok=False), args
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
    assert_eq!(w.show(&thread)["status"], "resolved", "사람이 끝내면 대화 thread도 끝난다");
    assert_eq!(branches(&w), serde_json::json!([]));
}
