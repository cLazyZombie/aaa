//! 실제 CLI→host→Serve 명령→DB→HTTP 경계. Tailscale 프로세스만 격리 파일 fixture로 대체한다.
use super::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

const TAILSCALE: &str = r#"#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(__file__).parent
args = sys.argv[1:]
config_file = root / 'serve.json'
if args == ['status', '--json']:
    print(json.dumps({'BackendState':'Running','Self':{'DNSName':'mac-a.example.ts.net.'}}))
elif args == ['serve', 'status', '--json']:
    print(config_file.read_text())
else:
    assert args[0] == 'serve', args
    port = next(a.split('=',1)[1] for a in args if a.startswith('--https='))
    config = json.loads(config_file.read_text())
    key = 'mac-a.example.ts.net:' + port
    if args[-1] == 'off':
        config['TCP'].pop(port, None)
        config['Web'].pop(key, None)
    else:
        assert args[1:3] == ['--bg','--yes'], args
        assert args[-1].startswith('http://127.0.0.1:'), args
        config['TCP'][port] = {'HTTPS':True}
        config['Web'][key] = {'Handlers':{'/':{'Proxy':args[-1]}}}
    tmp = root / ('serve-' + str(os.getpid()) + '.json')
    tmp.write_text(json.dumps(config))
    tmp.replace(config_file)
    with (root / 'calls.jsonl').open('a') as log:
        log.write(json.dumps(args) + '\n')
    print('fixture applied')
"#;

fn public_get(addr: &str, authority: &str, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(T)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").unwrap();
    let mut data = String::new();
    stream.read_to_string(&mut data).unwrap();
    (data.split_whitespace().nth(1).unwrap().parse().unwrap(), data)
}

#[test]
fn public_port_change_migrates_links_and_share_tokens_and_survives_host_restart() {
    let mut lab = Lab::new();
    let h = lab.node(
        "host",
        Some("schema=1\n[host]\nagent_listen='127.0.0.1:0'\n[web]\nlisten='127.0.0.1:0'\n[notify]\nweb_push=false\n"),
    );
    let fixture = lab.root.join("tailscale-fixture");
    std::fs::create_dir(&fixture).unwrap();
    let script = fixture.join("tailscale");
    std::fs::write(&script, TAILSCALE).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let foreign = json!({"TCP":{"443":{"HTTPS":true}},"Web":{"mac-a.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:9000"}}}}});
    std::fs::write(fixture.join("serve.json"), foreign.to_string()).unwrap();
    let path = std::env::join_paths(
        std::iter::once(fixture.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let path = path.to_str().unwrap();
    lab.spawn_env("host", &h, &["host"], &[("PATH", path)]);
    wait_for("host front 준비", T, || h.json(&["status"]).ok());
    h.ok(&["project", "create", "demo"]);
    let published = h.ok(&["web", "publish"]);
    assert_eq!(published["url"], "https://mac-a.example.ts.net:8443");
    let report = lab.root.join("report");
    std::fs::create_dir(&report).unwrap();
    std::fs::write(report.join("index.html"), "<!doctype html><p>포트 이전 보고</p>").unwrap();
    h.ok(&["library", "publish", path_str(&report), "--project", "demo", "--name", "port-report"]);
    let old_link = h.ok(&["library", "link", "demo", "port-report"]);
    let old_url = old_link["url"].as_str().unwrap();
    assert!(old_url.starts_with("https://mac-a.example.ts.net:8443/v/"), "{old_link}");
    let body = lab.root.join("post.txt");
    std::fs::write(&body, format!("[보고]({old_url}) https://mac-a.example.ts.net:8443/#question=q-0001")).unwrap();
    h.ok(&[
        "post",
        "user",
        "--notice",
        "--topic",
        "report",
        "--title",
        "이전 시험",
        "--body-file",
        path_str(&body),
        "--ref",
        old_url,
    ]);
    let changed = h.ok(&["web", "publish", "--port", "7443"]);
    assert_eq!(changed["url"], "https://mac-a.example.ts.net:7443");
    assert_eq!(changed["owned_port"], 7443);
    let new_link = h.ok(&["library", "link", "demo", "port-report"]);
    let new_url = new_link["url"].as_str().unwrap();
    assert_eq!(new_url, old_url.replace(":8443/", ":7443/"));
    let posts = h.ok(&["post", "list", "demo"]);
    assert!(posts["posts"][0]["body"].as_str().unwrap().contains(new_url), "{posts}");
    assert_eq!(posts["posts"][0]["refs"][0], new_url);
    let web = h.ok(&["status"])["web_listen"].as_str().unwrap().to_owned();
    assert_eq!(public_get(&web, "mac-a.example.ts.net:8443", "/").0, 421);
    let file = &new_url[new_url.find("/v/").unwrap()..];
    let response = public_get(&web, "mac-a.example.ts.net:7443", file);
    assert_eq!(response.0, 200, "{}", response.1);
    assert!(response.1.contains("포트 이전 보고"));
    let after: Value = serde_json::from_slice(&std::fs::read(fixture.join("serve.json")).unwrap()).unwrap();
    assert_eq!(after["TCP"]["443"], foreign["TCP"]["443"]);
    assert_eq!(after["Web"]["mac-a.example.ts.net:443"], foreign["Web"]["mac-a.example.ts.net:443"]);
    assert!(after["TCP"].get("8443").is_none());
    assert_eq!(after["TCP"]["7443"]["HTTPS"], true);
    // 같은 listen으로 다시 띄운다. public 포트·서명 cap·링크 이전이 메모리에만 남으면 안 된다.
    let host_toml = std::fs::read_to_string(h.cfg.join("host.toml"))
        .unwrap()
        .replace("\nlisten='127.0.0.1:0'", &format!("\nlisten='{web}'"));
    std::fs::write(h.cfg.join("host.toml"), host_toml).unwrap();
    lab.kill("host");
    lab.spawn_env("host-restarted", &h, &["host"], &[("PATH", path)]);
    wait_for("다시 띄운 host front 준비", T, || h.json(&["status"]).ok());
    assert_eq!(h.ok(&["web", "status"])["url"], changed["url"]);
    assert_eq!(h.ok(&["library", "link", "demo", "port-report"])["url"], new_url);
    assert_eq!(public_get(&web, "mac-a.example.ts.net:7443", file).0, 200);
}
