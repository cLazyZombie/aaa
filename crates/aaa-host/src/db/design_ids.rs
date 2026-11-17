//! 기획서 ID(`k-NNNN`, t-1957) 원장. 번호는 host 전역 하나이고(`AUTOINCREMENT`) 지운 행의 번호도 다시 쓰지 않는다. 정본은 문서 안
//! 표지(`<!-- aaa-id: k-NNNN -->`)이고, 원장은 번호마다 마지막으로 본 자리(프로젝트·저장소·경로)·제목과 지금 main 트리에 있는지를
//! 기억한다. 표지가 아직 없는 문서는 경로로 번호를 잇는다(다음 저장·한 번에 달기가 그 번호를 적는다).
use super::*;
use std::collections::HashSet;

/// 트리 문서 하나(맞추기 입력)
pub(crate) struct DesignIdDoc<'a> {
    pub path: &'a str,
    pub title: &'a str,
    /// 문서 안 표지의 번호
    pub marker: Option<i64>,
}

/// 맞춘 결과: 번호와 그 번호가 문서 안 표지에 적혀 있는가
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignIdOf {
    pub number: i64,
    pub marked: bool,
}

pub(crate) fn design_id_text(number: i64) -> String {
    format!("{}-{number:04}", aaa_proto::DESIGN_ID_PREFIX)
}

impl Db {
    /// 한 저장소 main 트리의 문서들과 원장을 한 transaction에서 맞춘다(트리 읽기는 동시에 여러 번 온다). 결과는 `docs` 차례다.
    ///
    /// - 표지 N: 원장에 없으면 그 자리로 N을 둔다(다른 host·잃은 원장의 표지). 같은 저장소의 N이면 원장 경로에 있는 문서가,
    ///   없으면(옮김·이름 바꿈) 처음 문서가 N을 갖고 원장 경로를 그리로 옮긴다. 다른 프로젝트의 같은 이름 저장소 N은 그대로
    ///   함께 쓴다(같은 저장소). 같은 N을 단 나머지 문서(복사)와 다른 저장소의 N은 표지가 없는 것처럼 다룬다.
    /// - 표지 없음: 이 경로의 원장 번호 중 아직 아무도 갖지 않은 가장 큰 것, 없으면 새 번호.
    /// - 이 저장소 행은 이번 트리에 있는 번호만 `present`다(웹 링크 목록).
    pub(crate) fn design_ids_sync(&self, project: &str, repo: &str, docs: &[DesignIdDoc]) -> Result<Vec<DesignIdOf>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let mut out: Vec<Option<DesignIdOf>> = vec![None; docs.len()];
        let mut taken: HashSet<i64> = HashSet::new();
        let mut owned: Vec<(i64, usize)> = Vec::new();
        let mut numbers: Vec<i64> = docs.iter().filter_map(|d| d.marker).collect();
        numbers.sort_unstable();
        numbers.dedup();
        for n in numbers {
            let same: Vec<usize> = (0..docs.len()).filter(|i| docs[*i].marker == Some(n)).collect();
            let row: Option<(String, String, String)> = tx
                .query_row("SELECT project,repo,path FROM design_ids WHERE number=?1", [n], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            let owner = match &row {
                None => {
                    tx.execute(
                        "INSERT INTO design_ids(number,project,repo,path) VALUES(?1,?2,?3,?4)",
                        params![n, project, repo, docs[same[0]].path],
                    )?;
                    Some(same[0])
                }
                Some((p, r, path)) if p == project && r == repo => {
                    Some(same.iter().copied().find(|i| docs[*i].path == path).unwrap_or(same[0]))
                }
                // 다른 프로젝트에 든 같은 저장소: 번호를 함께 쓰고 그쪽 원장 자리는 건드리지 않는다
                Some((_, r, _)) if r == repo => {
                    let i = same[0];
                    out[i] = Some(DesignIdOf { number: n, marked: true });
                    taken.insert(n);
                    None
                }
                Some(_) => None,
            };
            if let Some(i) = owner {
                out[i] = Some(DesignIdOf { number: n, marked: true });
                taken.insert(n);
                owned.push((n, i));
            }
        }
        for i in 0..docs.len() {
            if out[i].is_some() {
                continue;
            }
            let have: Vec<i64> = tx
                .prepare("SELECT number FROM design_ids WHERE project=?1 AND repo=?2 AND path=?3 ORDER BY number DESC")?
                .query_map(params![project, repo, docs[i].path], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let n = match have.into_iter().find(|n| !taken.contains(n)) {
                Some(n) => n,
                None => {
                    tx.execute(
                        "INSERT INTO design_ids(project,repo,path) VALUES(?1,?2,?3)",
                        params![project, repo, docs[i].path],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            taken.insert(n);
            owned.push((n, i));
            out[i] = Some(DesignIdOf { number: n, marked: false });
        }
        tx.execute("UPDATE design_ids SET present=0 WHERE project=?1 AND repo=?2", params![project, repo])?;
        for (n, i) in owned {
            tx.execute(
                "UPDATE design_ids SET path=?2, title=?3, present=1 WHERE number=?1",
                params![n, docs[i].path, docs[i].title],
            )?;
        }
        tx.commit()?;
        Ok(out.into_iter().map(|o| o.expect("모든 문서에 번호를 줬다")).collect())
    }

    /// 표지가 없는 문서를 저장할 때 적을 번호: 그 경로의 원장 번호 중 가장 큰 것, 없으면 새 번호(기획 branch의 새 문서)
    pub(crate) fn design_id_for_path(&self, project: &str, repo: &str, path: &str) -> Result<i64> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let have: Option<i64> = tx
            .query_row(
                "SELECT MAX(number) FROM design_ids WHERE project=?1 AND repo=?2 AND path=?3",
                params![project, repo, path],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let n = match have {
            Some(n) => n,
            None => {
                tx.execute("INSERT INTO design_ids(project,repo,path) VALUES(?1,?2,?3)", params![project, repo, path])?;
                tx.last_insert_rowid()
            }
        };
        tx.commit()?;
        Ok(n)
    }

    /// 그 경로의 원장 번호(읽기만, 없으면 None)
    pub(crate) fn design_id_of_path(&self, project: &str, repo: &str, path: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT number FROM design_ids WHERE project=?1 AND repo=?2 AND path=?3 ORDER BY present DESC, number DESC",
                params![project, repo, path],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// 지금 main 트리에 있는 문서의 번호(번호 차례)
    pub(crate) fn design_ids_present(&self, project: &str) -> Result<Vec<aaa_proto::DesignIdView>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT number,repo,path,title FROM design_ids WHERE project=?1 AND present=1 ORDER BY number")?;
        let rows = st
            .query_map([project], |r| {
                Ok(aaa_proto::DesignIdView {
                    id: design_id_text(r.get(0)?),
                    repo: r.get(1)?,
                    path: r.get(2)?,
                    title: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc<'a>(path: &'a str, marker: Option<i64>) -> DesignIdDoc<'a> {
        DesignIdDoc { path, title: "제목", marker }
    }

    #[test]
    fn numbers_follow_markers_and_paths() {
        let root = std::env::temp_dir().join(format!("aaa-design-ids-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let db = Db::open(&root.join("host.db")).unwrap();
        // 처음: 표지가 없으면 경로 차례로 새 번호
        let got = db.design_ids_sync("demo", "app", &[doc("docs/a.md", None), doc("docs/b.md", None)]).unwrap();
        assert_eq!(got, [DesignIdOf { number: 1, marked: false }, DesignIdOf { number: 2, marked: false }]);
        // 다시 읽어도 같은 번호
        let again = db.design_ids_sync("demo", "app", &[doc("docs/a.md", None), doc("docs/b.md", None)]).unwrap();
        assert_eq!(again, got);
        // 표지를 단 뒤 a를 옮기면 번호가 따라가고, 옛 경로는 목록에서 빠진다
        let moved = db.design_ids_sync("demo", "app", &[doc("docs/b.md", Some(2)), doc("docs/x/a.md", Some(1))]).unwrap();
        assert_eq!(moved, [DesignIdOf { number: 2, marked: true }, DesignIdOf { number: 1, marked: true }]);
        let present: Vec<_> =
            db.design_ids_present("demo").unwrap().into_iter().map(|v| (v.id, v.path)).collect();
        assert_eq!(present, [("k-0001".into(), "docs/x/a.md".into()), ("k-0002".to_string(), "docs/b.md".to_string())]);
        // 복사: 같은 표지 둘이면 원장 경로 쪽이 갖고 다른 쪽은 새 번호(표지 없음)
        let copied =
            db.design_ids_sync("demo", "app", &[doc("docs/b.md", Some(2)), doc("docs/c.md", Some(2)), doc("docs/x/a.md", Some(1))]).unwrap();
        assert_eq!(copied[0], DesignIdOf { number: 2, marked: true });
        assert_eq!(copied[1], DesignIdOf { number: 3, marked: false });
        // 표지가 사라져도(자동 생성 문서를 다시 만듦) 같은 경로면 같은 번호
        let lost = db.design_ids_sync("demo", "app", &[doc("docs/b.md", None), doc("docs/x/a.md", Some(1))]).unwrap();
        assert_eq!(lost[0], DesignIdOf { number: 2, marked: false });
        // 다른 host·잃은 원장의 표지는 그 번호 그대로 받고, 다음 새 번호는 그 뒤다
        let foreign = db.design_ids_sync("demo", "lib", &[doc("docs/z.md", Some(40)), doc("docs/y.md", None)]).unwrap();
        assert_eq!(foreign, [DesignIdOf { number: 40, marked: true }, DesignIdOf { number: 41, marked: false }]);
        // 다른 저장소의 번호를 단 표지는 표지가 없는 것처럼(경로 번호)
        let clash = db.design_ids_sync("demo", "app", &[doc("docs/b.md", Some(40)), doc("docs/x/a.md", Some(1))]).unwrap();
        assert_eq!(clash[0], DesignIdOf { number: 2, marked: false });
        // 저장할 때: 경로 번호, 없으면 새 번호. 지운 프로젝트의 번호도 다시 쓰지 않는다
        assert_eq!(db.design_id_for_path("demo", "app", "docs/x/a.md").unwrap(), 1);
        assert_eq!(db.design_id_for_path("demo", "app", "docs/new.md").unwrap(), 42);
        db.project_forget("demo").unwrap();
        assert_eq!(db.design_id_for_path("demo", "app", "docs/a.md").unwrap(), 43);
        drop(db);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
