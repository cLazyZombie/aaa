//! `aaa migrate`: 지금 운영(herdr·herdr-projects 0.2)에서 옮기기(plan §15, docs/migration.md). 일은 `aaa_host::migrate`가 한다.
//! host가 떠 있지 않아도 도는 파일 도구다(import는 오히려 host가 떠 있으면 거부한다).

use std::path::PathBuf;
use std::process::ExitCode;

use aaa_host::migrate::{convert, inventory, shadow};
use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum MigrateCmd {
    /// 0.2 프로젝트 폴더와 herdr 세션 스냅샷을 읽기만 해서 inventory.json·inventory.md를 낸다
    Inventory {
        /// herdr-projects root(예: ~/.herdr-projects)
        #[arg(long)]
        from: PathBuf,
        /// 0.2 프로젝트 slug
        #[arg(long)]
        project: String,
        /// 결과 폴더(원본 밖)
        #[arg(long)]
        out: PathBuf,
        /// projects root가 있는 머신 이름(기록에 머신이 없는 thread가 여기서 돈다)
        #[arg(long)]
        home_machine: String,
        /// `<머신>=<herdr api snapshot 출력 파일>`(여러 번)
        #[arg(long)]
        snapshot: Vec<String>,
        /// 이보다 큰 파일은 hash하지 않는다(MB). 주지 않으면 모두 hash한다
        #[arg(long)]
        hash_max_mb: Option<u64>,
        /// `herdr-projects --version` 출력(기록용)
        #[arg(long)]
        source_version: Option<String>,
        /// home 머신 `herdr-projects profile resolve <name>` 출력을 한 줄에 하나씩 모은 파일
        #[arg(long)]
        home_profiles: Option<PathBuf>,
    },
    /// inventory를 확인하며 aaa 상태 폴더(--state-dir)·projects root(host.toml)에 프로젝트를 만든다. 원본은 바꾸지 않는다
    Import {
        /// `aaa migrate inventory`가 낸 inventory.json
        #[arg(long)]
        inventory: PathBuf,
        /// 만들 aaa 프로젝트 이름
        #[arg(long)]
        to_project: String,
        /// 끝난 thread의 library도 복사한다(같은 APFS volume이면 clone)
        #[arg(long)]
        library: bool,
        /// 결과(convert.md·convert.json)를 둘 폴더. 주지 않으면 프로젝트 폴더 imported/에만 둔다
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// `scripts/m4/shadow-capture.sh`가 모은 tick들을 두 규칙으로 다시 판정해 shadow.md·shadow.jsonl을 낸다
    Shadow {
        #[arg(long)]
        ticks: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        home_machine: String,
    },
}

fn write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, bytes).with_context(|| format!("{}", path.display()))
}

pub fn run(json: bool, cmd: MigrateCmd) -> Result<ExitCode> {
    let now = aaa_common::util::now_ms();
    match cmd {
        MigrateCmd::Inventory {
            from,
            project,
            out,
            home_machine,
            snapshot,
            hash_max_mb,
            source_version,
            home_profiles,
        } => {
            let mut profiles = std::collections::BTreeMap::new();
            if let Some(f) = &home_profiles {
                for line in std::fs::read_to_string(f).with_context(|| format!("{}", f.display()))?.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let v: serde_json::Value = serde_json::from_str(line).context("profile resolve 줄")?;
                    let name = v["name"].as_str().context("profile resolve: name이 없다")?.to_string();
                    let args = v["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|a| a.as_str().map(str::to_string))
                        .collect();
                    profiles.insert(name, args);
                }
            }
            let snapshots = snapshot
                .iter()
                .map(|s| {
                    s.split_once('=')
                        .map(|(m, p)| (m.to_string(), PathBuf::from(p)))
                        .with_context(|| format!("<머신>=<파일> 꼴이 아니다: {s}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let opts = inventory::Opts {
                hash_max: hash_max_mb.map(|m| m << 20),
                snapshots,
                home_machine,
                source_version,
                home_profiles: profiles,
                now_ms: now,
            };
            if out.starts_with(from.join(&project)) {
                anyhow::bail!("결과 폴더가 원본 프로젝트 안이다: {}", out.display());
            }
            let inv = inventory::build(&from, &project, &opts)?;
            write(&out.join("inventory.json"), serde_json::to_string_pretty(&inv)?.as_bytes())?;
            write(&out.join("inventory.md"), inventory::render_md(&inv).as_bytes())?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "out": out, "threads": inv.threads.len(), "files": inv.files.len() })
                );
            } else {
                let bytes: u64 = inv.counts.values().map(|c| c.bytes).sum();
                println!(
                    "inventory: 파일 {}개 · {} · thread {}개 · inbox {}+{} · 뺀 곳 {}개 · 읽지 못함 {}개 → {}",
                    inv.files.len(),
                    aaa_host::migrate::human_bytes(bytes),
                    inv.threads.len(),
                    inv.inbox.open,
                    inv.inbox.done,
                    inv.excluded.len(),
                    inv.unreadable.len(),
                    out.join("inventory.md").display()
                );
            }
            Ok(if inv.unreadable.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(3) })
        }
        MigrateCmd::Import { inventory: file, to_project, library, out } => {
            let inv: inventory::Inventory =
                serde_json::from_str(&std::fs::read_to_string(&file).with_context(|| format!("{}", file.display()))?)
                    .context("inventory.json")?;
            let cfg = match aaa_config::load_host(&aaa_config::paths::config_dir()) {
                Ok(l) => l.config,
                Err(e) => anyhow::bail!("host 설정을 읽지 못했다:\n{}", e.render()),
            };
            let projects_root = if cfg.projects.root.is_empty() {
                aaa_config::paths::home().join(".aaa").join("projects")
            } else {
                PathBuf::from(&cfg.projects.root)
            };
            let dest = convert::Dest {
                state_root: aaa_config::paths::state_dir(),
                projects_root,
                project: to_project,
                library,
            };
            let o = convert::convert(&inv, &dest, now)?;
            if let Some(out) = &out {
                write(&out.join("convert.json"), serde_json::to_string_pretty(&o)?.as_bytes())?;
                write(&out.join("convert.md"), convert::render_md(&o).as_bytes())?;
            }
            if json {
                println!("{}", serde_json::to_string(&o)?);
            } else {
                println!(
                    "import: {} · 파일 {}개(같음 {} · 바뀜 {} · 생김 {} · 실패 {}) · thread {}개 · inbox {}+{}",
                    o.project_dir,
                    o.copied_files,
                    o.verified,
                    o.changed.len(),
                    o.added.len(),
                    o.failed.len(),
                    o.threads_imported.len(),
                    o.inbox_open,
                    o.inbox_done
                );
            }
            Ok(if o.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(3) })
        }
        MigrateCmd::Shadow { ticks, out, home_machine } => {
            let (rows, bad) = shadow::analyze(&ticks, &shadow::Opts { home_machine })?;
            shadow::write(&out, &rows, &bad)?;
            let judged = rows.iter().filter(|r| r.agree.is_some()).count();
            let differ = rows.iter().filter(|r| r.agree == Some(false)).count();
            // G4가 세는 것: 까닭을 정하지 못한 차이
            let unclassified = rows
                .iter()
                .filter(|r| r.agree == Some(false) && r.reason.as_deref() == Some(shadow::UNCLASSIFIED))
                .count();
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "rows": rows.len(), "judged": judged, "differ": differ, "unclassified": unclassified, "bad_ticks": bad.len() })
                );
            } else {
                println!(
                    "shadow: 줄 {}개 · 판정 {judged} · 다름 {differ}(분류 안 됨 {unclassified}) · 읽지 못한 tick {} → {}",
                    rows.len(),
                    bad.len(),
                    out.join("shadow.md").display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
