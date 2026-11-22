//! 등록한 머신과 머신 추가 작업.

use super::*;

#[derive(Debug, Clone)]
pub struct MachineRow {
    pub id: String,
    pub name: String,
    pub fingerprint: Option<String>,
    pub local: bool,
    pub os: Option<String>,
    pub boot_id: Option<String>,
    pub last_seen_ms: Option<u64>,
    /// 마지막 hello의 CPU 종류·판·실행 파일 SHA-256(웹 머신 상세, plan §8.372)
    pub arch: Option<String>,
    pub software: Option<String>,
    pub sha256: Option<String>,
}

impl Db {
    pub(super) fn machine_from(r: &rusqlite::Row) -> rusqlite::Result<MachineRow> {
        Ok(MachineRow {
            id: r.get(0)?,
            name: r.get(1)?,
            fingerprint: r.get(2)?,
            local: r.get::<_, i64>(3)? != 0,
            os: r.get(4)?,
            boot_id: r.get(5)?,
            last_seen_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
            arch: r.get(7)?,
            software: r.get(8)?,
            sha256: r.get(9)?,
        })
    }

    pub(super) const MACHINE_COLS: &str = "id,name,fingerprint,local,os,boot_id,last_seen,arch,software,sha256";

    pub fn machines(&self) -> Result<Vec<MachineRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!(
            "SELECT {} FROM machines WHERE revoked=0 ORDER BY local DESC, name",
            Self::MACHINE_COLS
        ))?;
        let rows = st.query_map([], Self::machine_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn machine_by_fingerprint(&self, fp: &str) -> Result<Option<MachineRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            &format!("SELECT {} FROM machines WHERE fingerprint=?1 AND revoked=0", Self::MACHINE_COLS),
            [fp],
            Self::machine_from,
        )
        .optional()?)
    }

    pub fn machine_by_name(&self, name: &str) -> Result<Option<MachineRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            &format!("SELECT {} FROM machines WHERE name=?1 AND revoked=0", Self::MACHINE_COLS),
            [name],
            Self::machine_from,
        )
        .optional()?)
    }

    /// 서명 검증을 마친 grant만 호출한다. nonce 소비와 새 머신 등록은 같은 commit이다.
    pub(crate) fn install_enroll(
        &self,
        installation: &str,
        plan: &str,
        nonce: &str,
        fingerprint: &str,
        id: &str,
        name: &str,
    ) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let previous: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT plan,fingerprint,machine,name FROM install_grants WHERE installation=?1",
                [installation],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some(previous) = previous {
            anyhow::ensure!(
                previous == (plan.into(), fingerprint.into(), id.into(), name.into()),
                "install grant target conflict"
            );
            let trusted: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM machines WHERE id=?1 AND name=?2 AND fingerprint=?3 AND revoked=0 AND local=0)",
                params![id,name,fingerprint], |r|r.get(0))?;
            anyhow::ensure!(trusted, "install grant revoked");
        } else {
            let conflict: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM machines WHERE id=?1 OR name=?2 OR fingerprint=?3)",
                params![id, name, fingerprint],
                |r| r.get(0),
            )?;
            // 기존/폐기된 신원을 되살리거나 다른 이름에 덮어쓰지 않는다.
            anyhow::ensure!(!conflict, "install grant target conflict");
            tx.execute("INSERT INTO install_grants(installation,plan,nonce,fingerprint,machine,name) VALUES(?1,?2,?3,?4,?5,?6)",
                params![installation,plan,nonce,fingerprint,id,name])?;
            tx.execute(
                "INSERT INTO machines(id,name,fingerprint,local,enrolled_at) VALUES(?1,?2,?3,0,?4)",
                params![id, name, fingerprint, aaa_common::util::now_ms() as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 응답 유실 뒤 조회에는 유효 기간을 연장하지 않고 실제 TLS peer와 아직 신뢰하는 머신을 대조한다.
    pub(crate) fn install_enrolled(&self, installation: &str, plan: &str, fingerprint: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT g.machine FROM install_grants g JOIN machines m ON m.id=g.machine
            WHERE g.installation=?1 AND g.plan=?2 AND g.fingerprint=?3 AND m.fingerprint=g.fingerprint AND m.name=g.name AND m.revoked=0 AND m.local=0",
            params![installation,plan,fingerprint], |r|r.get(0)).optional()?)
    }

    /// 등록(원격) 또는 로컬 agent 자동 등록. 같은 id면 이름·fingerprint를 갱신한다.
    pub fn upsert_machine(&self, id: &str, name: &str, fingerprint: Option<&str>, local: bool) -> Result<()> {
        let c = self.conn.lock().unwrap();
        // 폐기된 같은 이름 행은 지운다(다시 등록)
        c.execute("DELETE FROM machines WHERE name=?1 AND revoked=1", [name])?;
        c.execute(
            "INSERT INTO machines(id,name,fingerprint,local,enrolled_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, fingerprint=excluded.fingerprint, local=excluded.local, revoked=0",
            params![id, name, fingerprint, local as i64, aaa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    /// hello를 받을 때마다 그 머신의 OS·부팅·판을 남긴다(끊긴 뒤·host 재시작 뒤에도 마지막 값을 보인다)
    pub fn touch_machine(&self, hello: &aaa_proto::AgentHello) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE machines SET os=?2, boot_id=?3, last_seen=?4, arch=?5, software=?6, sha256=?7 WHERE id=?1",
            params![
                hello.machine_id,
                hello.os,
                hello.boot_id,
                aaa_common::util::now_ms() as i64,
                hello.arch,
                hello.software,
                hello.sha256
            ],
        )?;
        Ok(())
    }

    pub fn revoke_machine(&self, name: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        let id: Option<String> =
            c.query_row("SELECT id FROM machines WHERE name=?1 AND revoked=0", [name], |r| r.get(0)).optional()?;
        if let Some(id) = &id {
            c.execute("UPDATE machines SET revoked=1, fingerprint=NULL WHERE id=?1", [id])?;
        }
        Ok(id)
    }

    /// SSH 머신 추가 작업(plan §8.384): JSON 한 행. 최근 것부터
    pub(crate) fn machine_setups(&self) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM machine_setups ORDER BY updated DESC, id DESC")?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    pub(crate) fn machine_setup(&self, id: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT data FROM machine_setups WHERE id=?1", [id], |r| r.get(0)).optional()?)
    }

    /// 쓰고, 최근 `keep`개 밖의 오래된 작업은 지운다
    pub(crate) fn machine_setup_put(&self, id: &str, data: &str, updated_ms: u64, keep: usize) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO machine_setups(id,data,updated) VALUES(?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data, updated=excluded.updated",
            params![id, data, updated_ms as i64],
        )?;
        c.execute(
            "DELETE FROM machine_setups WHERE id NOT IN (SELECT id FROM machine_setups ORDER BY updated DESC, id DESC LIMIT ?1)",
            [keep as i64],
        )?;
        Ok(())
    }

    pub(crate) fn machine_setup_delete(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute("DELETE FROM machine_setups WHERE id=?1", [id])? != 0)
    }
}
