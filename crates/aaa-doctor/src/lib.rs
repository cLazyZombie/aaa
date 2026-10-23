//! 자가 점검(기획서 §10.4 6, §8.10)과 P0 측정 도구.
//!
//! `aaa doctor probe`는 지금 프로세스 계보에서 다음을 잰다.
//! - 책임 프로세스(responsible pid·경로), 보안 세션 속성, TCC 개발자 도구 preflight(비공개 API, 참고값)
//! - 키체인 잠금 bit와 실제 항목 읽기, 외장 볼륨 읽기·쓰기
//! - 새 실행 파일 n개의 첫 실행 − warm 지연(서명만 바꾼 「처음 보는」 Mach-O), warm 절대값
//! - 처리량(warm spawn 반복, CPU 해시), nice·QoS
//!
//! 판정은 도구가 한다(손으로 계산하지 않는다). gate는 기획서 §10.6의 제안값이다.

#[cfg(target_os = "macos")]
pub mod macos;

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// 첫 실행 지연 gate(§10.6): median ≤10ms, p95 ≤20ms, 100ms 초과 0
pub const GATE_MEDIAN_MS: f64 = 10.0;
pub const GATE_P95_MS: f64 = 20.0;
pub const GATE_MAX_MS: f64 = 100.0;
/// warm·처리량 gate: 기준선 대비 +10% 이내(제안)
pub const GATE_THROUGHPUT_RATIO: f64 = 1.10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    pub first_ms: f64,
    pub warm_ms: f64,
    pub extra_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FirstExecSummary {
    pub n: usize,
    pub median_extra_ms: f64,
    pub p95_extra_ms: f64,
    pub max_extra_ms: f64,
    pub over_100ms: usize,
    pub median_warm_ms: f64,
    pub pass: bool,
}

/// 중앙값(짝수 개면 가운데 둘의 평균)
pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let m = s.len() / 2;
    if s.len().is_multiple_of(2) { (s[m - 1] + s[m]) / 2.0 } else { s[m] }
}

/// nearest-rank p분위수
pub fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let rank = ((p / 100.0) * s.len() as f64).ceil().max(1.0) as usize;
    s[rank.min(s.len()) - 1]
}

pub fn summarize(samples: &[Sample]) -> FirstExecSummary {
    let extra: Vec<f64> = samples.iter().map(|s| s.extra_ms).collect();
    let warm: Vec<f64> = samples.iter().map(|s| s.warm_ms).collect();
    let med = median(&extra);
    let p95 = percentile(&extra, 95.0);
    let max = extra.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let over = extra.iter().filter(|&&e| e > GATE_MAX_MS).count();
    FirstExecSummary {
        n: samples.len(),
        median_extra_ms: med,
        p95_extra_ms: p95,
        max_extra_ms: max,
        over_100ms: over,
        median_warm_ms: median(&warm),
        pass: !samples.is_empty() && med <= GATE_MEDIAN_MS && p95 <= GATE_P95_MS && over == 0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Throughput {
    /// 같은 실행 파일(/usr/bin/true)을 연달아 띄운 시간의 중앙값
    pub warm_spawn_median_ms: f64,
    pub warm_spawn_n: usize,
    /// SHA-256 32MiB 한 번(단일 thread)
    pub cpu_hash_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sched {
    pub nice: i32,
    /// macOS qos_class_self (user_interactive·user_initiated·default·utility·background·unspecified)
    pub qos: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lineage {
    pub pid: u32,
    pub ppid: u32,
    pub sid: i32,
    pub responsible_pid: Option<i32>,
    pub responsible_path: Option<String>,
    /// 보안 세션 속성(macOS SessionGetInfo)
    pub session_id: Option<u32>,
    pub session_attrs: Option<u32>,
    pub session_has_gui: Option<bool>,
    pub session_has_tty: Option<bool>,
    pub session_is_remote: Option<bool>,
    /// TCCAccessPreflight(kTCCServiceDeveloperTool) 원시값. 비공개 API라 참고만 한다.
    pub tcc_devtool_preflight_raw: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Keychain {
    pub default_unlocked: Option<bool>,
    pub status_error: Option<i32>,
    /// 실제 항목 읽기: ok | not_found | interaction_not_allowed | auth_failed | error(<code>) | skipped
    pub item: String,
    pub item_service: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Volume {
    pub dir: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Binary {
    pub exe: String,
    /// codesign -dv 요약(서명 종류·Authority·TeamIdentifier·Identifier·CDHash)
    pub signature: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProbeReport {
    pub created_at_ms: u64,
    pub host: String,
    pub os: String,
    pub os_version: Option<String>,
    pub label: Option<String>,
    pub binary: Binary,
    pub lineage: Lineage,
    pub sched: Sched,
    pub keychain: Keychain,
    pub volume: Option<Volume>,
    /// 전체 디스크 접근 확인: ok | denied | error(..) | skipped
    #[serde(default)]
    pub fda: String,
    /// 빌드 구분값(업그레이드 모의에서 바뀐다)
    #[serde(default)]
    pub software: String,
    pub samples: Vec<Sample>,
    pub first_exec: Option<FirstExecSummary>,
    pub throughput: Throughput,
    /// 이 프로세스가 App Nap 방지 활동을 잡고 있는가(agent backend 자신이 잴 때만 true)
    pub activity_held: bool,
    /// 전체 판정과 실패 이유
    pub pass: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProbeOptions {
    pub samples: u32,
    pub keychain_service: Option<String>,
    pub volume_dir: Option<String>,
    pub label: Option<String>,
    pub throughput: bool,
    pub activity_held: bool,
    pub fda_check: bool,
}

pub fn probe(opts: &ProbeOptions) -> ProbeReport {
    let mut r = ProbeReport {
        created_at_ms: aaa_common::util::now_ms(),
        host: aaa_common::util::hostname(),
        os: std::env::consts::OS.to_string(),
        label: opts.label.clone(),
        software: aaa_common::util::software(),
        activity_held: opts.activity_held,
        ..Default::default()
    };
    r.binary.exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    r.lineage = lineage_self();
    r.sched = sched();
    #[cfg(target_os = "macos")]
    {
        r.os_version = aaa_common::util::sysctl_string("kern.osproductversion");
        r.binary.signature = macos::codesign_summary(&r.binary.exe);
        r.keychain = macos::keychain(opts.keychain_service.as_deref());
        if opts.samples > 0 {
            match macos::first_exec_samples(opts.samples as usize) {
                Ok(s) => r.samples = s,
                Err(e) => r.reasons.push(format!("첫 실행 측정 실패: {e:#}")),
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        r.keychain.item = "skipped".into();
    }
    if let Some(dir) = &opts.volume_dir {
        r.volume = Some(volume_check(dir));
    }
    r.fda = if opts.fda_check { fda_check() } else { "skipped".into() };
    if opts.throughput {
        r.throughput = throughput();
    }
    if !r.samples.is_empty() {
        r.first_exec = Some(summarize(&r.samples));
    }
    judge(&mut r, opts);
    r
}

fn judge(r: &mut ProbeReport, opts: &ProbeOptions) {
    if let Some(fe) = &r.first_exec {
        if !fe.pass {
            r.reasons.push(format!(
                "첫 실행 지연 gate 실패: median {:.3}ms(≤{GATE_MEDIAN_MS}), p95 {:.3}ms(≤{GATE_P95_MS}), 100ms 초과 {}개(0)",
                fe.median_extra_ms, fe.p95_extra_ms, fe.over_100ms
            ));
        }
    } else if opts.samples > 0 && cfg!(target_os = "macos") {
        r.reasons.push("첫 실행 표본이 없다".into());
    }
    if r.keychain.default_unlocked == Some(false) {
        r.reasons.push("기본 키체인이 잠겨 있다".into());
    }
    if opts.keychain_service.is_some() && r.keychain.item != "ok" {
        r.reasons.push(format!("키체인 실제 항목 읽기 실패: {}", r.keychain.item));
    }
    if let Some(v) = &r.volume
        && !v.ok
    {
        r.reasons.push(format!("볼륨 읽기·쓰기 실패: {} {}", v.dir, v.error.clone().unwrap_or_default()));
    }
    if opts.fda_check && r.fda != "ok" {
        r.reasons.push(format!("전체 디스크 접근 확인 실패: {}", r.fda));
    }
    r.pass = r.reasons.is_empty();
}

/// 기준선 대비 warm·처리량 판정(E2). 비교할 값이 없으면 None.
pub fn compare_throughput(base: &ProbeReport, cur: &ProbeReport) -> Vec<String> {
    let mut out = Vec::new();
    let pairs = [
        (
            "첫 실행 warm 중앙값",
            base.first_exec.as_ref().map(|f| f.median_warm_ms),
            cur.first_exec.as_ref().map(|f| f.median_warm_ms),
        ),
        ("warm spawn 중앙값", Some(base.throughput.warm_spawn_median_ms), Some(cur.throughput.warm_spawn_median_ms)),
        ("CPU 해시", Some(base.throughput.cpu_hash_ms), Some(cur.throughput.cpu_hash_ms)),
    ];
    for (name, b, c) in pairs {
        if let (Some(b), Some(c)) = (b, c)
            && b > 0.0
            && c > 0.0
            && c > b * GATE_THROUGHPUT_RATIO
        {
            out.push(format!("{name}: {c:.3}ms > 기준선 {b:.3}ms ×{GATE_THROUGHPUT_RATIO}"));
        }
    }
    out
}

pub fn lineage_self() -> Lineage {
    #[allow(unused_mut)] // Windows에서는 채울 값이 없다
    let mut l = Lineage { pid: std::process::id(), ..Default::default() };
    #[cfg(unix)]
    unsafe {
        l.ppid = libc::getppid() as u32;
        l.sid = libc::getsid(0);
    }
    #[cfg(target_os = "macos")]
    macos::fill_lineage(&mut l);
    l
}

pub fn sched() -> Sched {
    #[allow(unused_mut)]
    let mut s = Sched::default();
    #[cfg(unix)]
    unsafe {
        s.nice = libc::getpriority(libc::PRIO_PROCESS, 0);
    }
    #[cfg(target_os = "macos")]
    {
        s.qos = Some(macos::qos_name());
    }
    s
}

fn volume_check(dir: &str) -> Volume {
    let path = std::path::Path::new(dir).join(format!(".aaa-probe-{}", std::process::id()));
    let payload = aaa_common::util::new_id();
    let res = (|| -> std::io::Result<()> {
        std::fs::write(&path, &payload)?;
        let back = std::fs::read_to_string(&path)?;
        std::fs::remove_file(&path)?;
        if back != payload {
            return Err(std::io::Error::other("읽은 내용이 다르다"));
        }
        Ok(())
    })();
    Volume {
        dir: dir.to_string(),
        ok: res
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok(),
        error: res.err().map(|e| e.to_string()),
    }
}

/// 전체 디스크 접근(FDA)이 있어야 열리는 시스템 TCC DB의 첫 16 byte를 읽어 본다(내용은 쓰지 않는다).
fn fda_check() -> String {
    #[cfg(target_os = "macos")]
    {
        use std::io::Read as _;
        // 시스템 TCC DB는 전체 디스크 접근이 있는 계보에서만 열린다.
        let db = std::path::Path::new("/Library/Application Support/com.apple.TCC/TCC.db");
        match std::fs::File::open(db).and_then(|mut f| {
            let mut b = [0u8; 16];
            f.read_exact(&mut b).map(|_| b)
        }) {
            Ok(b) if b.starts_with(b"SQLite format 3") => "ok".into(),
            Ok(_) => "error(형식이 다름)".into(),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "denied".into(),
            Err(e) => format!("error({e})"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "skipped".into()
    }
}

pub fn throughput() -> Throughput {
    use sha2::Digest;
    let mut t = Throughput::default();
    #[cfg(unix)]
    {
        let n = 40;
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let start = Instant::now();
            let ok = std::process::Command::new("/usr/bin/true").status().map(|s| s.success()).unwrap_or(false);
            if ok {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // 첫 회는 캐시가 덜 찬 값이라 뺀다
        if times.len() > 1 {
            times.remove(0);
        }
        t.warm_spawn_n = times.len();
        t.warm_spawn_median_ms = median(&times);
    }
    let buf = vec![0x5au8; 32 << 20];
    let start = Instant::now();
    let digest = sha2::Sha256::digest(&buf);
    std::hint::black_box(digest);
    t.cpu_hash_ms = start.elapsed().as_secs_f64() * 1000.0;
    t
}

/// 사람이 읽는 요약(한국어)
pub fn render(r: &ProbeReport) -> String {
    let mut s = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(s, "판정: {}", if r.pass { "통과" } else { "실패" });
    for reason in &r.reasons {
        let _ = writeln!(s, "  - {reason}");
    }
    let l = &r.lineage;
    let _ = writeln!(s, "실행 파일: {} ({})", r.binary.exe, r.software);
    if r.fda != "skipped" && !r.fda.is_empty() {
        let _ = writeln!(s, "전체 디스크 접근: {}", r.fda);
    }
    for line in &r.binary.signature {
        let _ = writeln!(s, "  {line}");
    }
    let _ = writeln!(
        s,
        "계보: pid {} ppid {} sid {} · 책임 프로세스 {} {}",
        l.pid,
        l.ppid,
        l.sid,
        l.responsible_pid.map(|p| p.to_string()).unwrap_or("?".into()),
        l.responsible_path.clone().unwrap_or_default()
    );
    if let Some(attrs) = l.session_attrs {
        let _ = writeln!(
            s,
            "보안 세션: id {} attrs 0x{attrs:x} (GUI {:?}, TTY {:?}, 원격 {:?})",
            l.session_id.unwrap_or(0),
            l.session_has_gui.unwrap_or(false),
            l.session_has_tty.unwrap_or(false),
            l.session_is_remote.unwrap_or(false)
        );
    }
    if let Some(p) = l.tcc_devtool_preflight_raw {
        let _ = writeln!(s, "TCC 개발자 도구 preflight(참고): {p}");
    }
    let _ = writeln!(s, "스케줄: nice {} · QoS {}", r.sched.nice, r.sched.qos.clone().unwrap_or("-".into()));
    let _ = writeln!(
        s,
        "키체인: 기본 키체인 {} · 실제 항목 {}{}",
        match r.keychain.default_unlocked {
            Some(true) => "풀림",
            Some(false) => "잠김",
            None => "모름",
        },
        r.keychain.item,
        r.keychain.item_service.as_ref().map(|x| format!("({x})")).unwrap_or_default()
    );
    if let Some(v) = &r.volume {
        let _ = writeln!(s, "볼륨: {} {}", v.dir, if v.ok { "읽기·쓰기 성공" } else { "실패" });
    }
    if let Some(fe) = &r.first_exec {
        let _ = writeln!(
            s,
            "첫 실행(n={}): 추가 지연 median {:.3}ms · p95 {:.3}ms · max {:.3}ms · 100ms 초과 {} · warm median {:.3}ms",
            fe.n, fe.median_extra_ms, fe.p95_extra_ms, fe.max_extra_ms, fe.over_100ms, fe.median_warm_ms
        );
    }
    if r.throughput.cpu_hash_ms > 0.0 {
        let _ = writeln!(
            s,
            "처리량: warm spawn median {:.3}ms(n={}) · CPU 해시 {:.1}ms",
            r.throughput.warm_spawn_median_ms, r.throughput.warm_spawn_n, r.throughput.cpu_hash_ms
        );
    }
    s
}

/// 여러 결과를 한 표로(마크다운). P0 결과 정리용.
pub fn summary_table(reports: &[(String, ProbeReport)]) -> String {
    let mut s = String::from(
        "| 결과 | 조건 | 판정 | 책임 프로세스 | 서명 | 추가 지연 median/p95/max ms | 100ms 초과 | warm median ms | spawn ms | 키체인 | FDA |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (file, r) in reports {
        let fe = r.first_exec.as_ref();
        let sig = r
            .binary
            .signature
            .iter()
            .find(|l| l.starts_with("Authority="))
            .or_else(|| r.binary.signature.iter().find(|l| l.starts_with("Signature=")))
            .cloned()
            .unwrap_or_default();
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.3} | {}/{} | {} |\n",
            file,
            r.label.clone().unwrap_or_default(),
            if r.pass { "통과" } else { "실패" },
            r.lineage.responsible_path.clone().unwrap_or_default().rsplit('/').next().unwrap_or(""),
            sig,
            fe.map(|f| format!("{:.3}/{:.3}/{:.3}", f.median_extra_ms, f.p95_extra_ms, f.max_extra_ms))
                .unwrap_or("-".into()),
            fe.map(|f| f.over_100ms.to_string()).unwrap_or("-".into()),
            fe.map(|f| format!("{:.3}", f.median_warm_ms)).unwrap_or("-".into()),
            r.throughput.warm_spawn_median_ms,
            match r.keychain.default_unlocked {
                Some(true) => "풀림",
                Some(false) => "잠김",
                None => "?",
            },
            r.keychain.item,
            r.fda,
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(extra: f64) -> Sample {
        Sample { first_ms: extra + 2.0, warm_ms: 2.0, extra_ms: extra }
    }

    #[test]
    fn gate_thresholds() {
        // 운영 기준(WezTerm 계보 +0.1ms 수준)은 통과
        let fast: Vec<_> = (0..30).map(|i| s(0.1 + i as f64 * 0.01)).collect();
        assert!(summarize(&fast).pass);
        // 미등록 계보(+220ms)는 실패
        let slow: Vec<_> = (0..30).map(|_| s(221.0)).collect();
        let r = summarize(&slow);
        assert!(!r.pass && r.over_100ms == 30);
        // median은 좋아도 p95가 20ms를 넘으면 실패(30개 중 2개가 25ms → p95 rank 29)
        let mut tail: Vec<_> = (0..28).map(|_| s(1.0)).collect();
        tail.extend([s(25.0), s(25.0)]);
        let r = summarize(&tail);
        assert!(r.median_extra_ms <= 10.0 && r.p95_extra_ms == 25.0 && !r.pass);
        // 한 표본이라도 100ms를 넘으면 실패
        let mut one: Vec<_> = (0..29).map(|_| s(0.5)).collect();
        one.push(s(150.0));
        let r = summarize(&one);
        assert!(r.p95_extra_ms < 20.0 && r.over_100ms == 1 && !r.pass);
        assert!(!summarize(&[]).pass);
    }

    #[test]
    fn percentile_nearest_rank() {
        let v: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 95.0), 19.0);
        assert_eq!(percentile(&v, 100.0), 20.0);
        assert_eq!(median(&[3.0, 1.0, 2.0, 4.0]), 2.5);
    }

    #[test]
    fn throughput_gate_uses_ten_percent() {
        let mut base = ProbeReport::default();
        base.throughput.warm_spawn_median_ms = 2.0;
        base.throughput.cpu_hash_ms = 100.0;
        let mut cur = base.clone();
        cur.throughput.warm_spawn_median_ms = 2.19;
        assert!(compare_throughput(&base, &cur).is_empty());
        cur.throughput.warm_spawn_median_ms = 6.0; // GUI 앱 계보에서 본 약 3배
        assert_eq!(compare_throughput(&base, &cur).len(), 1);
    }
}
