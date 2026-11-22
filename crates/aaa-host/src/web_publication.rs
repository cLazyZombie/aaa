//! 사람이 요청한 공개 포트 변경. 잠시 끊어도 되며 옛 포트는 비운다. 실패 뒤에는 같은 명령으로 이어 한다.
use crate::{Host, err, tailscale};
use aaa_proto::{ErrorBody, ErrorCode, PublishState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PortChange {
    pub id: String,
    pub old_port: u16,
    pub new_port: u16,
    pub old_origin: String,
    pub new_origin: String,
    pub web_port: u16,
}

trait Serve {
    fn read(&self) -> impl Future<Output = Option<tailscale::TsOutput>> + Send;
    fn on(&self, port: u16, target: u16) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn off(&self, port: u16) -> impl Future<Output = anyhow::Result<()>> + Send;
}
struct Native;
impl Serve for Native {
    async fn read(&self) -> Option<tailscale::TsOutput> {
        tailscale::read().await
    }
    async fn on(&self, port: u16, target: u16) -> anyhow::Result<()> {
        tailscale::serve(port, target).await.map(|_| ())
    }
    async fn off(&self, port: u16) -> anyhow::Result<()> {
        tailscale::serve_off(port).await
    }
}
fn unavailable(error: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Unavailable, format!("{error}. 전환이 남아 있으면 같은 aaa web publish --port 명령으로 이어 한다"))
}
fn exact(out: Option<&tailscale::TsOutput>, web: u16, port: u16) -> Result<tailscale::Exact, ErrorBody> {
    tailscale::exact(out, web, port)
        .map_err(|e| err(if e.conflict { ErrorCode::Conflict } else { ErrorCode::Unavailable }, e.code))
}

impl Host {
    fn pending_publication(&self) -> Result<Option<PortChange>, ErrorBody> {
        self.db
            .meta("publish_change")
            .map_err(unavailable)?
            .map(|s| serde_json::from_str(&s).map_err(unavailable))
            .transpose()
    }
    pub(crate) fn no_pending_publication(&self) -> Result<(), ErrorBody> {
        if let Some(pending) = self.pending_publication()? {
            return Err(err(
                ErrorCode::Conflict,
                format!("공개 포트 전환이 남아 있다: aaa web publish --port {}", pending.new_port),
            ));
        }
        Ok(())
    }

    pub(crate) async fn publish_status(&self) -> PublishState {
        let _gate = self.publish_gate.lock().await;
        self.publication_status(Native.read().await.as_ref())
    }

    fn publication_status(&self, out: Option<&tailscale::TsOutput>) -> PublishState {
        let selected = self
            .db
            .meta("publish_selected_port")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .and_then(|s| {
                s.parse::<u16>()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            });
        let plan = tailscale::installation_plan(out, self.web_port(), &self.cfg.web.publish_ports, selected);
        let known = plan.state == "running"
            && out
                .and_then(|o| o.serve.as_deref())
                .and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(s)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .is_some_and(|v| v.is_object() || v.is_null());
        // 없음도 관측한 상태다. origin 삭제와 관측 표지는 한 transaction으로 기록한다.
        let observed = !known
            || self
                .db
                .observe_publication(
                    plan.serving.as_ref().map(|(url, _)| url.to_ascii_lowercase()).as_deref(),
                    plan.dns_name.as_deref(),
                )
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok();
        let pending = self
            .pending_publication()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten();
        PublishState {
            tailscale: plan.state.into(),
            dns_name: plan.dns_name,
            url: plan.serving.map(|(url, _)| url),
            owned_port: self
                .db
                .meta("publish_port")
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .flatten()
                .and_then(|p| {
                    p.parse()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                }),
            web_port: self.web_port(),
            message: if let Some(p) = pending {
                Some(format!("공개 포트 전환이 남아 있다: aaa web publish --port {}", p.new_port))
            } else {
                match plan.state {
                    "missing" => Some("tailscale CLI가 없다. 웹은 127.0.0.1에서만 열린다".into()),
                    "stopped" => Some("Tailscale이 꺼졌거나 로그인 전이다".into()),
                    _ if !known => Some("Tailscale Serve 상태를 읽지 못했다. 저장한 공개 주소를 유지한다".into()),
                    _ if !observed => Some("공개 주소 상태를 저장하지 못했다".into()),
                    _ if self.public_origin().is_none() => Some("아직 공개하지 않았다: aaa web publish".into()),
                    _ => None,
                }
            },
        }
    }

    pub(crate) async fn publish(&self) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.no_pending_publication()?;
        if self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "host.toml의 web.listen이 꺼져 있다"));
        }
        self.db.del_meta("publish_selected_port").map_err(unavailable)?;
        let out = Native.read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        if plan.state != "running" {
            return Ok(self.publication_status(out.as_ref()));
        }
        if plan.serving.is_none() {
            let port = plan.free_port.ok_or_else(|| {
                err(ErrorCode::Unavailable, format!("후보 포트가 모두 쓰이고 있다: {:?}", plan.taken))
            })?;
            let before = exact(out.as_ref(), self.web_port(), port)?;
            Native.on(port, self.web_port()).await.map_err(unavailable)?;
            let after = exact(Native.read().await.as_ref(), self.web_port(), port)?;
            if !after.reused || !before.preserves(&after) {
                return Err(err(ErrorCode::Conflict, "Serve 공개 결과가 계획과 다르다"));
            }
            self.db.set_meta("publish_port", &port.to_string()).map_err(unavailable)?;
        }
        Ok(self.publication_status(Native.read().await.as_ref()))
    }

    pub(crate) async fn publish_off(&self) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.no_pending_publication()?;
        let port = self
            .db
            .meta("publish_port")
            .map_err(unavailable)?
            .and_then(|p| {
                p.parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .ok_or_else(|| err(ErrorCode::NotFound, "aaa가 만든 공개가 없다(다른 서비스의 매핑은 건드리지 않는다)"))?;
        let before = exact(Native.read().await.as_ref(), self.web_port(), port)?;
        if before.reused {
            Native.off(port).await.map_err(unavailable)?;
        }
        let out = Native.read().await;
        let after = exact(out.as_ref(), self.web_port(), port)?;
        if after.reused || !before.preserves(&after) {
            return Err(err(ErrorCode::Conflict, "Serve 공개 해제 결과가 계획과 다르다"));
        }
        self.db.del_meta("publish_port").map_err(unavailable)?;
        Ok(self.publication_status(out.as_ref()))
    }

    pub(crate) async fn publish_port_change(&self, port: u16, actor: &str) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.change_port(&Native, port, actor).await
    }

    async fn change_port(&self, io: &impl Serve, port: u16, actor: &str) -> Result<PublishState, ErrorBody> {
        if port == 0 || self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "공개 포트와 웹 listen은 1~65535여야 한다"));
        }
        let out = io.read().await;
        let target = exact(out.as_ref(), self.web_port(), port)?;
        let change = if let Some(pending) = self.pending_publication()? {
            if pending.new_port != port || pending.web_port != self.web_port() || pending.new_origin != target.url() {
                return Err(err(
                    ErrorCode::Conflict,
                    format!("남은 전환의 주소·listen을 유지한다: aaa web publish --port {}", pending.new_port),
                ));
            }
            pending
        } else {
            let old_port = self
                .db
                .meta("publish_port")
                .map_err(unavailable)?
                .and_then(|p| {
                    p.parse()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .ok_or_else(|| err(ErrorCode::NotFound, "aaa가 만든 공개가 없다. 먼저 aaa web publish로 공개한다"))?;
            let old = exact(out.as_ref(), self.web_port(), old_port)?;
            if old_port == port && target.reused {
                return Ok(self.publication_status(out.as_ref()));
            }
            if target.reused {
                return Err(err(
                    ErrorCode::Conflict,
                    "새 포트에 이미 매핑이 있다. aaa가 소유하지 않은 공개는 바꾸지 않는다",
                ));
            }
            if let Some(origin) = self.public_origin()
                && origin != old.url()
            {
                return Err(err(
                    ErrorCode::Conflict,
                    "저장된 공개 주소와 aaa가 만든 포트가 다르다. aaa web status로 확인한다",
                ));
            }
            let change = PortChange {
                id: aaa_common::util::new_id(),
                old_port,
                new_port: port,
                old_origin: old.url(),
                new_origin: target.url(),
                web_port: self.web_port(),
            };
            self.db
                .set_meta("publish_change", &serde_json::to_string(&change).map_err(unavailable)?)
                .map_err(unavailable)?;
            change
        };
        // 옛 포트를 먼저 비운다. 실패/종료 뒤에도 journal은 남고 각 단계의 실제 상태부터 확인한다.
        if change.old_port != change.new_port {
            let before = exact(io.read().await.as_ref(), self.web_port(), change.old_port)?;
            if before.url() != change.old_origin {
                return Err(err(ErrorCode::Conflict, "옛 공개 주소가 바뀌었다"));
            }
            if before.reused {
                io.off(change.old_port).await.map_err(unavailable)?;
            }
            let after = exact(io.read().await.as_ref(), self.web_port(), change.old_port)?;
            if after.reused || !before.preserves(&after) {
                return Err(err(ErrorCode::Conflict, "옛 포트 해제 결과가 계획과 다르다"));
            }
        }
        let before = exact(io.read().await.as_ref(), self.web_port(), port)?;
        if !before.reused {
            io.on(port, self.web_port()).await.map_err(unavailable)?;
        }
        let out = io.read().await;
        let after = exact(out.as_ref(), self.web_port(), port)?;
        if !after.reused || !before.preserves(&after) || after.url() != change.new_origin {
            return Err(err(ErrorCode::Conflict, "새 포트 공개 결과가 계획과 다르다"));
        }
        let (moved, subscriptions) = self.db.finish_port_change(&change, actor).map_err(unavailable)?;
        for item in self.db.inbox_list(None, true).map_err(unavailable)? {
            self.inbox_publish(&item);
        }
        self.asks_bump();
        self.bump();
        let mut status = self.publication_status(out.as_ref());
        status.message = Some(format!(
            "저장 기록 {moved}건 이전, 옛 푸시 구독 {subscriptions}건 정리. 옛 외부 링크는 끊겼다. 새 주소에서 알림을 다시 켠다"
        ));
        Ok(status)
    }
}

#[cfg(test)]
mod tests;
