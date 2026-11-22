//! 웹 기기, passkey, Web Push 구독.

use super::*;

/// Web Push 구독. endpoint는 비밀값처럼 다룬다(로그에 남기지 않는다).
#[derive(Debug, Clone)]
pub struct PushSub {
    pub endpoint: String,
    /// 브라우저 ECDH 공개키(base64url, 65바이트)
    pub p256dh: String,
    /// 브라우저 인증 비밀(base64url, 16바이트)
    pub auth: String,
    pub device: String,
    /// 이 기기가 받을 알림(차이 목록 N3). 넣을 때 None이면 전에 고른 것을 둔다(처음이면 둘 다 켬). 읽으면 늘 Some
    pub alerts: Option<Alerts>,
}

/// 기기마다 고르는 알림(herdr-web-ui `AlertPrefs`, 차이 목록 N3). 참고는 「작업 완료」가 끄기·긴 작업·매번이지만 aaa의
/// 「작업 완료」는 thread 새 보고(일 한 단위)라 길이로 거르지 않고 켜기·끄기만 둔다(plan §8.119).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Alerts {
    /// 확인 필요(질문·확인창·thread가 기다림·시작 실패)
    pub input: bool,
    /// 새 보고(검토 대기)
    pub done: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Alerts { input: true, done: true }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub id: String,
    pub role: String,
    pub label: String,
    pub created_ms: u64,
    pub last_seen_ms: Option<u64>,
}

/// 이름이 없는 옛 기기(CLI pairing만 있던 때)
pub const DEVICE_LABEL_FALLBACK: &str = "기기";

impl Db {
    pub fn add_device(&self, id: &str, role: &str, token_hash: &str, label: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO devices(id,role,token_hash,created_at,label) VALUES(?1,?2,?3,?4,?5)",
            params![id, role, token_hash, aaa_common::util::now_ms() as i64, label],
        )?;
        Ok(())
    }

    pub(super) fn device_from(r: &rusqlite::Row) -> rusqlite::Result<DeviceRow> {
        Ok(DeviceRow {
            id: r.get(0)?,
            role: r.get(1)?,
            label: r.get::<_, Option<String>>(2)?.unwrap_or_else(|| DEVICE_LABEL_FALLBACK.to_string()),
            created_ms: r.get::<_, i64>(3)? as u64,
            last_seen_ms: r.get::<_, Option<i64>>(4)?.map(|v| v as u64),
        })
    }

    pub fn device_by_token_hash(&self, h: &str) -> Result<Option<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE token_hash=?1 AND revoked=0",
            [h],
            Self::device_from,
        )
        .optional()?)
    }

    /// 취소하지 않은 기기를 연결한 순서대로
    pub fn devices(&self) -> Result<Vec<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE revoked=0 ORDER BY created_at, id",
        )?;
        let rows = st.query_map([], Self::device_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 마지막으로 본 때. 취소한 기기는 고치지 않는다
    pub fn device_seen(&self, id: &str, now_ms: u64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE devices SET last_seen=?2 WHERE id=?1 AND revoked=0", params![id, now_ms as i64])?;
        Ok(())
    }

    /// 이름을 바꾼다. 그런 기기가 없으면(취소됐으면) false
    pub fn rename_device(&self, id: &str, label: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("UPDATE devices SET label=?2 WHERE id=?1 AND revoked=0", params![id, label])? > 0)
    }

    /// 취소한다(token hash는 남겨 다시 쓰지 못하게 한다). 그 기기의 push 구독도 지운다. 없으면(이미 취소) false
    pub fn revoke_device(&self, id: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = c.execute("UPDATE devices SET revoked=1 WHERE id=?1 AND revoked=0", [id])?;
        if n > 0 {
            c.execute("DELETE FROM push_subs WHERE device=?1", [id])?;
            c.execute("DELETE FROM passkeys WHERE device=?1", [id])?;
        }
        Ok(n > 0)
    }

    /// 시험: 짝지은 때를 옮긴다(passkey 등록 시한)
    #[cfg(test)]
    pub fn set_device_created(&self, id: &str, ms: u64) {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE devices SET created_at=?2 WHERE id=?1", params![id, ms as i64])
            .unwrap();
    }

    /// 취소하지 않은 기기 하나
    pub fn device(&self, id: &str) -> Result<Option<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE id=?1 AND revoked=0",
            [id],
            Self::device_from,
        )
        .optional()?)
    }

    // ───────────── 조작 기기 passkey(관리 GUI W7) ─────────────

    /// 이 기기의 이 RP passkey를 새 credential로 바꾼다(기기마다 하나). 다른 기기가 쓰는 credential이면 false
    pub fn passkey_put(
        &self,
        credential: &str,
        device: &str,
        rp_id: &str,
        cose: &[u8],
        sign_count: u32,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let owner: Option<String> =
            tx.query_row("SELECT device FROM passkeys WHERE credential=?1", [credential], |r| r.get(0)).optional()?;
        if owner.is_some_and(|o| o != device) {
            return Ok(false);
        }
        tx.execute("DELETE FROM passkeys WHERE device=?1 AND rp_id=?2", params![device, rp_id])?;
        tx.execute(
            "INSERT OR REPLACE INTO passkeys(credential,device,rp_id,cose,sign_count,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![credential, device, rp_id, cose, sign_count as i64, aaa_common::util::now_ms() as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// 이 기기의 이 RP passkey: (credential, COSE 공개키, 서명 횟수)
    pub fn passkeys(&self, device: &str, rp_id: &str) -> Result<Vec<(String, Vec<u8>, u32)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT credential,cose,sign_count FROM passkeys WHERE device=?1 AND rp_id=?2")?;
        let rows = st
            .query_map(params![device, rp_id], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u32)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 이 RP passkey가 있는 기기
    pub fn passkey_devices(&self, rp_id: &str) -> Result<std::collections::HashSet<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT DISTINCT device FROM passkeys WHERE rp_id=?1")?;
        let rows = st.query_map([rp_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn passkey_used(&self, credential: &str, sign_count: u32, now_ms: u64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE passkeys SET sign_count=?2, last_used=?3 WHERE credential=?1",
            params![credential, sign_count as i64, now_ms as i64],
        )?;
        Ok(())
    }

    // ───────────── Web Push 구독(§8.26) ─────────────

    /// 구독을 넣는다. 같은 장치의 같은 endpoint면 키를 바꾼다(브라우저가 구독을 새로 만든 경우).
    /// 다른 장치의 endpoint는 키까지 같을 때만 옮긴다: 같은 브라우저가 다시 pairing한 경우다. endpoint만 아는 요청이
    /// 남의 구독을 가져가지 못한다(보안 감사 t-0340 B10. 참고 herdr-web-ui server/push.ts는 늘 옮긴다). 옮기지 않았으면 false.
    /// 알림 종류(`alerts`)를 주면 바꾸고, None이면 전에 고른 것을 둔다(참고 `subscribe`: 「alerts absent keeps what the device chose before」).
    pub(crate) fn push_put_at(&self, s: &PushSub, origin: Option<&str>) -> Result<bool> {
        let given = s.alerts.is_some();
        let a = s.alerts.unwrap_or_default();
        let n = self.conn.lock().unwrap().execute(
            "INSERT INTO push_subs(endpoint,p256dh,auth,device,created_at,alert_input,alert_done,origin) VALUES(?1,?2,?3,?4,?5,?6,?7,?9)
             ON CONFLICT(endpoint) DO UPDATE SET p256dh=excluded.p256dh, auth=excluded.auth, device=excluded.device,
               alert_input=CASE WHEN ?8 THEN excluded.alert_input ELSE push_subs.alert_input END,
               alert_done=CASE WHEN ?8 THEN excluded.alert_done ELSE push_subs.alert_done END,
               origin=COALESCE(excluded.origin,push_subs.origin)
             WHERE push_subs.device=excluded.device OR (push_subs.p256dh=excluded.p256dh AND push_subs.auth=excluded.auth)",
            params![s.endpoint, s.p256dh, s.auth, s.device, aaa_common::util::now_ms() as i64, a.input, a.done, given, origin],
        )?;
        Ok(n > 0)
    }

    /// endpoint를 지운다. `device`를 주면 그 장치의 구독만 지운다.
    pub fn push_del(&self, endpoint: &str, device: Option<&str>) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = match device {
            Some(d) => c.execute("DELETE FROM push_subs WHERE endpoint=?1 AND device=?2", [endpoint, d])?,
            None => c.execute("DELETE FROM push_subs WHERE endpoint=?1", [endpoint])?,
        };
        Ok(n > 0)
    }

    /// 보낼 구독(취소되지 않은 장치의 것만). `device`를 주면 그 장치 것만.
    pub fn push_list(&self, device: Option<&str>) -> Result<Vec<PushSub>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT s.endpoint,s.p256dh,s.auth,s.device,s.alert_input,s.alert_done FROM push_subs s JOIN devices d ON d.id=s.device
             WHERE d.revoked=0 AND (?1 IS NULL OR s.device=?1) ORDER BY s.created_at",
        )?;
        let rows = st
            .query_map([device], |r| {
                Ok(PushSub {
                    endpoint: r.get(0)?,
                    p256dh: r.get(1)?,
                    auth: r.get(2)?,
                    device: r.get(3)?,
                    alerts: Some(Alerts { input: r.get(4)?, done: r.get(5)? }),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
