use super::*;

struct Lab(PathBuf);

impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-finish-{}", aaa_common::util::new_id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("isolated")).unwrap();
        Self(root)
    }

    fn write_source(&self, rel: &str, text: &str) {
        let path = self.0.join("local").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn refresh(&self, parent: &str, name: &str) -> Result<()> {
        refresh_entry(&self.0.join("isolated"), &self.0.join("local"), parent, name)
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn replaces_readonly_skill_and_reviewer_and_removes_previous_copies_if_source_disappears() {
    let lab = Lab::new();
    for (parent, name, file) in
        [("skills", "finish", "skills/finish/SKILL.md"), ("agents", "finish-reviewer.md", "agents/finish-reviewer.md")]
    {
        let path = lab.0.join("isolated").join(file);
        lab.write_source(file, "first");
        lab.refresh(parent, name).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");
        assert!(path.metadata().unwrap().permissions().readonly());

        // 중단된 준비 사본이 남아도 다음 실행에서 새 원본만 배치한다.
        let pending = lab.0.join("isolated").join(format!(".aaa-finish-{parent}/pending"));
        fs::create_dir(&pending).unwrap();
        fs::write(pending.join("partial"), "unfinished").unwrap();
        lab.write_source(file, "next");
        lab.refresh(parent, name).unwrap();
        assert!(!pending.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "next");
        assert!(path.metadata().unwrap().permissions().readonly());

        fs::remove_file(lab.0.join("local").join(file)).unwrap();
        assert!(lab.refresh(parent, name).is_err());
        assert!(!lab.0.join("isolated").join(parent).join(name).exists());
        assert!(!lab.0.join("isolated").join(format!(".aaa-finish-{parent}")).exists());
    }
}

#[test]
fn preserves_preexisting_unmanaged_destination() {
    let lab = Lab::new();
    lab.write_source("skills/finish/SKILL.md", "local");
    let skills = lab.0.join("isolated/skills");
    fs::create_dir_all(skills.join("finish")).unwrap();
    let existing = skills.join("finish/SKILL.md");
    fs::write(&existing, "existing").unwrap();
    assert!(lab.refresh("skills", "finish").unwrap_err().to_string().contains("별도 항목"));
    assert_eq!(fs::read_to_string(&existing).unwrap(), "existing");
}

#[cfg(unix)]
#[test]
fn linked_files_and_destination_directories_do_not_expose_unrelated_data() {
    use std::os::unix::fs::symlink;

    let lab = Lab::new();
    lab.write_source("skills/finish/SKILL.md", "local");
    lab.write_source("unrelated.json", "unrelated");
    let unrelated = lab.0.join("local/unrelated.json");
    let outside = lab.0.join("local/skills");
    symlink(&outside, lab.0.join("isolated/skills")).unwrap();
    assert!(lab.refresh("skills", "finish").unwrap_err().to_string().contains("일반 폴더"));
    assert_eq!(fs::read_to_string(outside.join("finish/SKILL.md")).unwrap(), "local");

    fs::remove_file(lab.0.join("isolated/skills")).unwrap();
    lab.refresh("skills", "finish").unwrap();
    symlink(&unrelated, outside.join("finish/private.md")).unwrap();
    assert!(lab.refresh("skills", "finish").is_err());
    assert!(!lab.0.join("isolated/skills/finish").exists());
    assert!(!lab.0.join("isolated/.aaa-finish-skills/pending").exists());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "unrelated");
    assert!(!unrelated.metadata().unwrap().permissions().readonly());
}
