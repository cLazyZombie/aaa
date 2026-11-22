//! 웹 기기 짝짓기 코드와 기기 권한.

use super::*;

#[derive(Default)]
pub(super) struct Pairing {
    pub(super) pending: Option<PendingCode>,
    /// 코드와 상관없이 틀린 때, `PAIR_FAIL_WINDOW` 안의 것만(§6.4). 창이 지나면 저절로 줄어든다
    pub(super) failures: VecDeque<Instant>,
}

impl Pairing {
    pub(super) fn forget_old(&mut self, now: Instant) {
        while self.failures.front().is_some_and(|t| now.saturating_duration_since(*t) >= PAIR_FAIL_WINDOW) {
            self.failures.pop_front();
        }
    }

    /// 다음 시도를 받을 때. 창 안에서 `PAIR_FAIL_BUDGET`번까지는 기다리지 않고, 그 뒤로는 마지막 실패부터
    /// 1초·2초·4초… 60초까지(herdr-web-ui `recordAuthFailure`)
    pub(super) fn open_at(&self) -> Option<Instant> {
        let n = self.failures.len();
        if n < PAIR_FAIL_BUDGET {
            return None;
        }
        let wait = PAIR_BACKOFF.saturating_mul(1u32 << (n - PAIR_FAIL_BUDGET).min(16)).min(PAIR_BACKOFF_MAX);
        self.failures.back().map(|t| *t + wait)
    }
}

/// pairing 코드 수명(herdr-web-ui `CODE_TTL_MS` 10분)
pub const PAIR_CODE_TTL: Duration = Duration::from_secs(10 * 60);

/// 코드 하나로 틀릴 수 있는 수(참고 `CODE_ATTEMPTS`)
pub(super) const PAIR_CODE_ATTEMPTS: u32 = 5;

/// 틀린 시도를 세는 창(참고 `SHARED_WINDOW_MS` 10분): 실패가 이보다 오래되면 잊는다
pub(super) const PAIR_FAIL_WINDOW: Duration = Duration::from_secs(10 * 60);

/// 기다리지 않고 틀릴 수 있는 수(참고 `AUTH_FAILURE_BUDGET`)
pub(super) const PAIR_FAIL_BUDGET: usize = 5;

/// 그 뒤 기다림의 처음과 상한(참고 `AUTH_BACKOFF_MS` 1초·`AUTH_BACKOFF_MAX_MS` 60초)
pub(super) const PAIR_BACKOFF: Duration = Duration::from_secs(1);

pub(super) const PAIR_BACKOFF_MAX: Duration = Duration::from_secs(60);

impl Host {
    // ───────────── pairing ─────────────

    /// 새 pairing 코드. 아직 쓰지 않은 앞 코드는 버린다(herdr-web-ui `startPairing`: 「새 코드」·CLI가 같은 자리를 쓴다).
    /// 코드를 만드는 것은 주인(이 머신의 CLI나 controller 기기)이라, 앞서 틀린 시도의 잠금도 푼다: 남이 틀려 둔
    /// 잠금이 주인의 새 코드를 막지 않는다.
    pub(super) fn mint_pairing(&self, role: DeviceRole, now: Instant) -> (String, u64) {
        let code = format!("{:06}", rand::random_range(0..1_000_000u32));
        let mut p = self.pairing.lock().unwrap();
        p.pending = Some(PendingCode { code: code.clone(), role, expires: now + PAIR_CODE_TTL, attempts: 0 });
        p.failures.clear();
        (code, PAIR_CODE_TTL.as_secs())
    }

    /// 웹 pairing 코드를 장치 token으로 바꾼다. 코드는 한 번만 쓰고, 한 코드로 5번 틀리면 그 코드를 버린다.
    /// 코드와 상관없이 10분 안에 5번 넘게 틀리면 1초부터 두 배씩 60초까지 기다리게 한다.
    pub fn redeem_pairing(&self, code: &str, label: &str) -> Result<(String, DeviceRole), ErrorBody> {
        self.redeem_pairing_at(code, label, Instant::now())
    }

    pub(super) fn redeem_pairing_at(
        &self,
        code: &str,
        label: &str,
        now: Instant,
    ) -> Result<(String, DeviceRole), ErrorBody> {
        let mut p = self.pairing.lock().unwrap();
        p.forget_old(now);
        if let Some(t) = p.open_at()
            && t > now
        {
            return Err(err(
                ErrorCode::Unavailable,
                format!("시도가 너무 많다. {}초 뒤 다시 한다", (t - now).as_secs() + 1),
            ));
        }
        if p.pending.as_ref().is_some_and(|c| c.expires <= now) {
            p.pending = None;
        }
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        // 걸린 시간으로 자리를 알 수 없게 끝까지 비교한다(참고 `timingSafeEqual`)
        let right = p.pending.as_ref().is_some_and(|c| {
            c.code.len() == code.len() && c.code.bytes().zip(code.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
        });
        if !right {
            if let Some(c) = p.pending.as_mut() {
                c.attempts += 1;
                if c.attempts >= PAIR_CODE_ATTEMPTS {
                    p.pending = None;
                }
            }
            p.failures.push_back(now);
            return Err(err(ErrorCode::Unauthorized, "코드가 틀렸거나 이미 썼거나 만료됐다"));
        }
        let role = p.pending.take().map(|c| c.role).unwrap_or(DeviceRole::Reader);
        p.failures.clear();
        drop(p);
        let token =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, rand::random::<[u8; 32]>());
        let id = util::new_id();
        let role_s = if role == DeviceRole::Reader { "reader" } else { "controller" };
        let label = normalize_device_label(label, db::DEVICE_LABEL_FALLBACK);
        self.db
            .add_device(&id, role_s, &token_hash(&token), &label)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.db.audit(&format!("device:{id}"), "web_paired", role_s);
        self.bump();
        Ok((token, role))
    }

    /// cookie token의 기기. 들어올 때마다 「마지막으로 본 때」를 1분에 한 번까지 적는다(참고 `match`)
    pub fn device_role(&self, token: &str) -> Option<(String, DeviceRole)> {
        let d = self
            .db
            .device_by_token_hash(&token_hash(token))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()??;
        let now = util::now_ms();
        if d.last_seen_ms.is_none_or(|seen| now.saturating_sub(seen) >= DEVICE_SEEN_WRITE_MS) {
            // 기록은 입장을 막지 않는다
            aaa_common::diagnostic::ignored(self.db.device_seen(&d.id, now), "보조 작업 실패 뒤 기존 처리를 계속한다");
        }
        let role = if d.role == "reader" { DeviceRole::Reader } else { DeviceRole::Controller };
        Some((d.id, role))
    }

    /// 기기를 취소하고 그 기기의 열린 연결에 알린다. 없으면(이미 취소) false
    pub fn revoke_device(&self, id: &str) -> Result<bool, ErrorBody> {
        let done = self.db.revoke_device(id).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        if done {
            self.revoked_devices.lock().unwrap().insert(id.to_string());
            self.revoke_account_logins(id);
            aaa_common::diagnostic::ignored(
                self.device_revoked.send(id.to_string()),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
            self.bump();
        }
        Ok(done)
    }

    /// 열린 연결의 기기가 이 host가 돈 뒤 취소됐나(그 전에 취소된 기기는 cookie 검사에서 이미 막힌다)
    pub fn is_revoked(&self, id: &str) -> bool {
        self.revoked_devices.lock().unwrap().contains(id)
    }
}
