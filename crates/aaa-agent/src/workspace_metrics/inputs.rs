//! 실행 ID는 종료 입력으로 고정하고, 재해석 판에는 부속 입력의 추가·변경·삭제도 넣는다.
use super::*;

const PARSER: &str = "parser-5";

pub(super) fn fingerprint(root: &Path, primary: &str) -> Result<String> {
    let mut files = vec![
        "workspace-summary.tsv".into(),
        "metrics.json".into(),
        "e2e.json".into(),
        "e2e.log".into(),
        "e2e-binary.log".into(),
        "rc-e2e/report.json".into(),
        "no2-e2e/all-headless.json".into(),
        "verification/gates/.started".into(),
    ];
    for (directory, limit, reports) in
        [("rust", 4096, false), ("verification", 64, true), ("verification/gates", 64, false)]
    {
        let path = root.join(directory);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => (),
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for (index, entry) in fs::read_dir(path)?.enumerate() {
            anyhow::ensure!(index < limit, "부속 결과 입력 목록 상한: {directory}");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if reports && entry.file_type()?.is_dir() {
                files.push(format!("{directory}/{name}/report.json"));
            } else if !reports && entry.file_type()?.is_file() {
                files.push(format!("{directory}/{name}"));
            }
        }
    }
    files.sort();
    files.dedup();
    let mut hash = Sha256::new();
    hash.update(PARSER);
    hash.update(primary);
    for file in files {
        hash.update(file.as_bytes());
        hash.update([0]);
        if let Some((v, _)) = version(&root.join(file)) {
            hash.update(v);
        }
        hash.update([0]);
    }
    Ok(hex::encode(hash.finalize()))
}
