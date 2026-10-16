//! 역할별 crate 의존 규칙(결정 13)을 `cargo metadata`로 검사한다. 일반·dev·build 의존을 모두 보고, workspace 안에서는
//! 간접 의존까지 따라간다.
//!
//! - front(`aa-tui`)는 host와 aa-proto 계약으로만 말한다: `aa-host`·`aa-agent`·`aa-session`에 의존하지 않는다.
//! - `aa-host`는 agent·session 내부에 의존하지 않고 Agent link(aa-proto)로만 다룬다.
//! - `aa-agent`는 `aa-session`을 쓴다. host·front에는 의존하지 않는다.
//! - 공통 crate는 어떤 역할 crate에도 의존하지 않는다(역할 경계를 넘는 통로가 되지 않게).
//!
//! 실행: `cargo test -p aa --test deps`

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

const ROLES: &[&str] = &["aa-session", "aa-agent", "aa-host", "aa-tui", "aa"];

/// (crate, 의존하면 안 되는 crate)
fn forbidden() -> Vec<(&'static str, Vec<&'static str>)> {
    let common = ROLES.to_vec();
    vec![
        ("aa-tui", vec!["aa-host", "aa-agent", "aa-session", "aa-doctor", "aa-vt", "aa"]),
        ("aa-host", vec!["aa-agent", "aa-session", "aa-tui", "aa"]),
        ("aa-agent", vec!["aa-host", "aa-tui", "aa"]),
        ("aa-session", vec!["aa-agent", "aa-host", "aa-tui", "aa"]),
        ("aa-proto", common.clone()),
        ("aa-config", common.clone()),
        ("aa-common", common.clone()),
        ("aa-vt", common.clone()),
        ("aa-doctor", common),
    ]
}

/// workspace crate → 직접 의존하는 workspace crate(종류 무관)
fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata 실행");
    assert!(out.status.success(), "cargo metadata 실패: {}", String::from_utf8_lossy(&out.stderr));
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let packages = meta["packages"].as_array().unwrap();
    let names: BTreeSet<String> = packages.iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
    packages
        .iter()
        .map(|p| {
            let deps = p["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["name"].as_str().unwrap().to_string())
                .filter(|d| names.contains(d))
                .collect();
            (p["name"].as_str().unwrap().to_string(), deps)
        })
        .collect()
}

/// `from`에서 닿는 workspace crate와 그 경로
fn reachable(graph: &BTreeMap<String, BTreeSet<String>>, from: &str) -> BTreeMap<String, Vec<String>> {
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stack = vec![(from.to_string(), vec![from.to_string()])];
    while let Some((n, path)) = stack.pop() {
        for d in graph.get(&n).into_iter().flatten() {
            if !seen.contains_key(d) {
                let mut p = path.clone();
                p.push(d.clone());
                seen.insert(d.clone(), p.clone());
                stack.push((d.clone(), p));
            }
        }
    }
    seen
}

#[test]
fn role_crates_keep_dependency_rules() {
    let graph = workspace_graph();
    let mut broken = Vec::new();
    for (krate, banned) in forbidden() {
        assert!(graph.contains_key(krate), "workspace에 {krate}가 없다(규칙 표를 고친다)");
        let reach = reachable(&graph, krate);
        for b in banned {
            if let Some(path) = reach.get(b) {
                broken.push(format!("{krate} → {b} (경로: {})", path.join(" → ")));
            }
        }
    }
    assert!(broken.is_empty(), "금지된 crate 의존:\n{}", broken.join("\n"));
    // 새 crate가 생기면 규칙 표에 넣게 한다
    let known: BTreeSet<&str> = forbidden().iter().map(|(k, _)| *k).chain(["aa"]).collect();
    let unknown: Vec<&String> = graph.keys().filter(|k| !known.contains(k.as_str())).collect();
    assert!(unknown.is_empty(), "의존 규칙이 없는 crate: {unknown:?}");
}

/// 검사가 실제로 잡는지: 규칙을 어기는 가짜 그래프
#[test]
fn reachable_follows_indirect_edges() {
    let g: BTreeMap<String, BTreeSet<String>> =
        [("aa-tui", vec!["aa-common"]), ("aa-common", vec!["aa-session"]), ("aa-session", vec![])]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
            .collect();
    assert_eq!(reachable(&g, "aa-tui")["aa-session"], ["aa-tui", "aa-common", "aa-session"]);
}
