use super::*;

struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-report-copy-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(root.join(".aaa/demo-t-0001/library")).unwrap();
        Self(root)
    }
    fn work(&self) -> PathBuf {
        self.0.join(".aaa/demo-t-0001")
    }
    fn put(&self, path: &str, size: u64) {
        let path = self.work().join("library").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path).unwrap().set_len(size).unwrap();
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

/// 수백 MiB 그림이 이름순 앞을 채워도 핵심 근거를 남기는 실제 크기/개수 회귀다.
/// sparse 파일의 metadata만 읽어 선택 계약을 검증하고 큰 본문 해시는 아래 작은 회수 시험으로 지킨다.
#[test]
fn real_size_budget_preserves_report_links_and_small_evidence_before_images() {
    let mut phases = aaa_test_support::phases::Phases::new(
        "real_size_budget_preserves_report_links_and_small_evidence_before_images",
    );
    let lab = Lab::new();
    // 같은 부모의 준비를 2390번 반복하지 않는다. 각 파일은 원래처럼
    // 서로 다른 실제 inode와 224 KiB sparse 크기를 가진다.
    let frames = lab.work().join("library/a-frames");
    std::fs::create_dir(&frames).unwrap();
    for i in 0..2390 {
        std::fs::File::create(frames.join(format!("{i:04}.png"))).unwrap().set_len(224 * 1024).unwrap();
    }
    for name in ["summary.md", "metrics.csv", "ledger.json", "run.log", "events.jsonl", "table.tsv"] {
        lab.put(&format!("z-evidence/{name}"), 1024);
    }
    lab.put("z-linked/final.png", 2 << 20);
    lab.put("z-linked/too-large.mp4", 201 << 20);
    lab.put("z-linked/판정 자료.json", 2048);
    let report = "[결과](library/z-linked/final.png)\n[원본](library/z-linked/too-large.mp4)\n[측정](<library/z-linked/판정 자료.json>)";
    phases.mark("contract", "select-real-library");
    let (files, skipped) = select(&lab.work().join("library"), report.as_bytes(), 200 << 20).unwrap();
    assert_eq!(files.first().unwrap().path, "z-linked/판정 자료.json");
    assert!(files[1..7].iter().all(|f| f.path.starts_with("z-evidence/")));
    assert_eq!(files[7].path, "z-linked/final.png");
    assert!(files[8..].iter().all(|f| f.path.starts_with("a-frames/")));
    let bytes: u64 = files.iter().map(|f| f.size).sum();
    assert!(bytes <= 200 << 20 && bytes > (200 << 20) - 224 * 1024);
    assert_eq!(files.len() + skipped.len(), 2399);
    assert!(
        skipped.iter().any(|s| s.path == "z-linked/too-large.mp4"
            && s.lazy_size == Some(201 << 20)
            && s.reason.contains("200 MiB"))
    );
    assert!(skipped.iter().all(|s| s.lazy_size.is_some()));
    phases.mark("cleanup", "real-library");
}

/// 실제 collect의 순서·상한·해시·누락과 링크 표기별 선택을 함께 검증한다.
#[test]
fn collect_keeps_priority_hashes_and_skips_without_following_symlinks() {
    let lab = Lab::new();
    for (name, size) in [
        ("a-frame.png", 700),
        ("z-big.png", 600),
        ("z-small.json", 50),
        ("z space.json", 40),
        ("z-ref.json", 30),
        ("z-raw.json", 20),
        ("z-html.json", 10),
    ] {
        lab.put(name, size);
    }
    let report = "![완료](./library/z-big.png)\n[측정](library/z%2Dsmall.json?raw=1#data)\n[공백](<library/z space.json>)\n[참조][ref]\n[ref]: library/z-ref.json\n`library/z-raw.json`\n<a href=\"library/z-html.json\">HTML</a>";
    std::fs::write(lab.work().join("report.md"), report).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(lab.work().join("library/z-small.json"), lab.work().join("library/link")).unwrap();
    let snap = collect(lab.work().to_str().unwrap(), 800).unwrap();
    let paths: Vec<_> = snap.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["z-html.json", "z-raw.json", "z-ref.json", "z space.json", "z-small.json", "z-big.png"]);
    assert_eq!(snap.report.0, report.as_bytes());
    assert_eq!(snap.hash, hex::encode(Sha256::digest(report.as_bytes())));
    for file in &snap.files {
        assert_eq!(
            file.sha256,
            hex::encode(Sha256::digest(std::fs::read(lab.work().join("library").join(&file.path)).unwrap()))
        );
    }
    assert!(snap.skipped.iter().any(|s| s.path == "a-frame.png" && s.lazy_size == Some(700)));
    #[cfg(unix)]
    assert!(snap.skipped.iter().any(|s| s.path == "link" && s.lazy_size.is_none()));
    let probe = collect(lab.work().to_str().unwrap(), 0).unwrap();
    assert!(probe.files.is_empty());
    assert_eq!(probe.hash, snap.hash);
}
