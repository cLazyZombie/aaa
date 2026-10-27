use super::*;

const COUNT: usize = 17;
fn source(i: usize) -> String {
    format!("+++\nschedule='every 1h'\nenabled=false\n+++\n원문 {i} 한글\n")
}
fn seed(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("routines")).unwrap();
    for i in 0..COUNT {
        std::fs::write(root.join(format!("routines/batch-{i:04}.md")), source(i)).unwrap();
    }
}
fn originals(root: &std::path::Path) {
    for i in 0..COUNT {
        assert_eq!(std::fs::read_to_string(root.join(format!("routines/batch-{i:04}.md"))).unwrap(), source(i));
    }
}
fn backups(root: &std::path::Path) {
    for i in 0..COUNT {
        assert_eq!(
            std::fs::read_to_string(root.join(format!("routines/.converted/batch-{i:04}.md"))).unwrap(),
            source(i)
        );
    }
}
fn complete(root: &std::path::Path) {
    backups(root);
    let (rules, errors) = crate::routine::load_all(root);
    assert!(errors.is_empty(), "{errors:?}");
    for i in 0..COUNT {
        assert!(!root.join(format!("routines/batch-{i:04}.md")).exists());
        let name = format!("batch-{i:04}");
        assert_eq!(rules.iter().find(|r| r.name == name).unwrap().prompt, format!("원문 {i} 한글"));
    }
    assert!(snapshot(root).keys().all(|p| !p.contains(".aaa-convert-")), "임시 파일 잔여 없음");
}

#[test]
fn batch_migration_keeps_originals_until_backup_and_output_barriers_finish() {
    let p = Project::new();
    seed(&p.0);
    let plan = convert::prepare(&p.0, "mac-a").unwrap();
    let mut durable_backups = false;
    let mut durable_outputs = false;
    let mut outputs = 0;
    convert::resume_with(&p.0, &plan, |event, _| {
        match event {
            "backup_sync" => {
                originals(&p.0);
                backups(&p.0);
                assert!(!p.0.join("automations/batch-0000.toml").exists());
            }
            "backups_durable" => {
                durable_backups = true;
                originals(&p.0);
            }
            "output_published" => {
                assert!(durable_backups);
                originals(&p.0);
                outputs += 1;
            }
            "output_sync" => {
                assert!(outputs > COUNT);
                originals(&p.0);
            }
            "outputs_durable" => {
                durable_outputs = true;
            }
            "source_removed" => {
                assert!(durable_outputs);
                backups(&p.0);
            }
            _ => {}
        }
        Ok(())
    })
    .unwrap();
    assert!(durable_backups && durable_outputs);
    complete(&p.0);
    let before = snapshot(&p.0);
    convert::resume(&p.0, &plan).unwrap();
    assert_eq!(snapshot(&p.0), before, "완료한 journal의 재개는 같은 bytes");
}

fn check_batch_migration_fault(point: &str) {
    let p = Project::new();
    let h = demo_host(&p);
    let root = h.project_dir("demo").unwrap();
    seed(&root);
    let plan = convert::prepare(&root, "mac-a").unwrap();
    let raw = serde_json::to_string(&plan).unwrap();
    h.db.set_meta("automation_migration:demo", &raw).unwrap();
    let mut seen = 0;
    let error = convert::resume_with(&root, &plan, |event, _| {
        if event == point {
            seen += 1;
            if point != "output_published" || seen == 9 {
                return Err(format!("주입한 I/O 실패: {point}"));
            }
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains(point) && error.contains("journal 보존") && error.contains("출력 확인"), "{error}");
    assert_eq!(h.db.meta("automation_migration:demo").unwrap().as_deref(), Some(raw.as_str()));
    assert!(snapshot(&root).keys().all(|p| !p.contains(".aaa-convert-")), "{point}: 임시 파일 정리");
    backups(&root);
    if point != "source_removed" {
        originals(&root);
    }
    for i in 0..COUNT {
        let original = root.join(format!("routines/batch-{i:04}.md"));
        if original.exists() {
            assert_eq!(std::fs::read_to_string(original).unwrap(), source(i));
        }
    }
    h.automation_recover("demo").unwrap();
    assert!(h.db.meta("automation_migration:demo").unwrap().is_none());
    complete(&root);
}

#[test]
fn batch_migration_backup_sync_failure_keeps_journal_and_resumes() {
    check_batch_migration_fault("backup_sync");
}

#[test]
fn batch_migration_output_published_failure_keeps_journal_and_resumes() {
    check_batch_migration_fault("output_published");
}

#[test]
fn batch_migration_output_sync_failure_keeps_journal_and_resumes() {
    check_batch_migration_fault("output_sync");
}

#[test]
fn batch_migration_source_removed_failure_keeps_journal_and_resumes() {
    check_batch_migration_fault("source_removed");
}

#[test]
fn batch_migration_preserves_edits_between_validation_and_publish_or_remove() {
    for during_output in [true, false] {
        let p = Project::new();
        seed(&p.0);
        let plan = convert::prepare(&p.0, "mac-a").unwrap();
        let target = if during_output { "automations/batch-0000.toml" } else { "routines/batch-0000.md" };
        let event = if during_output { "backups_durable" } else { "outputs_durable" };
        let error = convert::resume_with(&p.0, &plan, |at, _| {
            if at == event {
                std::fs::create_dir_all(p.0.join(target).parent().unwrap()).unwrap();
                std::fs::write(p.0.join(target), "다른 사람이 쓴 글\n").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("다른 편집 발견"), "{error}");
        assert_eq!(std::fs::read_to_string(p.0.join(target)).unwrap(), "다른 사람이 쓴 글\n");
        backups(&p.0);
    }
}

#[test]
fn batch_migration_link_failure_keeps_existing_backup_and_cleans_staged_files() {
    let p = Project::new();
    seed(&p.0);
    let plan = convert::prepare(&p.0, "mac-a").unwrap();
    let occupied = p.0.join("routines/.converted/batch-0001.md");
    let error = convert::resume_with(&p.0, &plan, |event, file| {
        if event == "backup_published" && file.ends_with("batch-0000.md") {
            std::fs::write(&occupied, "이전부터 있던 다른 사본\n").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("원자적 공개") && error.contains("journal 보존"), "{error}");
    assert_eq!(std::fs::read_to_string(&occupied).unwrap(), "이전부터 있던 다른 사본\n");
    originals(&p.0);
    assert!(snapshot(&p.0).keys().all(|p| !p.contains(".aaa-convert-")));
    std::fs::remove_file(occupied).unwrap();
    convert::resume(&p.0, &plan).unwrap();
    complete(&p.0);
}
