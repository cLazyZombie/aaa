//! 「직접 설치」 안내(웹 머신 추가 M1, plan §8.383): 새 머신에서 칠 `aaa agent enroll <주소>`의 주소 후보와 host 지문·판.
//! host는 자기가 연 agent 연결 주소(`agent_listen`)만 안다. 모든 주소(0.0.0.0·[::])로 열었으면 tailnet 이름과 이 머신 이름을,
//! tailnet 주소(IPv4 100.64/10·IPv6 fd7a:115c:a1e0::/48)로 열었으면 tailnet 이름을 앞에 둔다. loopback은 같은 머신 안에서만 닿는다.
use std::net::{IpAddr, SocketAddr};

use aaa_proto::{EnrollAddress, EnrollGuide};

use crate::Host;

fn tailnet_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.octets()[0] == 100 && (64..128).contains(&v.octets()[1]),
        IpAddr::V6(v) => v.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// 연 주소에서 새 머신이 칠 주소 후보를 만든다(앞이 추천)
pub(crate) fn addresses(listen: &str, tailnet: Option<&str>, hostname: &str) -> Vec<EnrollAddress> {
    let at = |address: String, source: &str| EnrollAddress { address, source: source.into() };
    let Ok(addr) = listen.parse::<SocketAddr>() else { return vec![at(listen.into(), "listen")] };
    let port = addr.port();
    let ip = addr.ip();
    let mut out = Vec::new();
    if ip.is_loopback() {
        return vec![at(listen.into(), "loopback")];
    }
    if let Some(name) = tailnet.filter(|_| ip.is_unspecified() || tailnet_ip(ip)) {
        out.push(at(format!("{name}:{port}"), "tailnet"));
    }
    if ip.is_unspecified() {
        if !hostname.is_empty() {
            out.push(at(format!("{hostname}:{port}"), "hostname"));
        }
    } else {
        out.push(at(listen.into(), "listen"));
    }
    out
}

impl Host {
    pub(crate) async fn enroll_guide(&self) -> EnrollGuide {
        let listen = self.agent_listen.lock().unwrap().clone().filter(|l| !l.is_empty());
        let addresses = match &listen {
            None => vec![],
            Some(l) => {
                // tailnet 이름은 모든 주소나 tailnet 주소로 열었을 때만 쓴다(그 밖이면 묻지 않는다)
                let wide = l.parse::<SocketAddr>().is_ok_and(|a| a.ip().is_unspecified() || tailnet_ip(a.ip()));
                let dns = if wide {
                    let out = crate::tailscale::read().await;
                    crate::tailscale::plan(out.as_ref(), 0, &[]).dns_name
                } else {
                    None
                };
                addresses(l, dns.as_deref(), &aaa_common::util::hostname())
            }
        };
        EnrollGuide {
            listen,
            addresses,
            host_fingerprint: self.identity.fingerprint.clone(),
            software: aaa_common::util::software(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn got(listen: &str, tailnet: Option<&str>) -> Vec<(String, String)> {
        addresses(listen, tailnet, "mac-a").into_iter().map(|a| (a.address, a.source)).collect()
    }
    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, s)| (a.to_string(), s.to_string())).collect()
    }

    #[test]
    fn candidates_follow_the_listen_address() {
        let ts = Some("mac-a.example.ts.net");
        // 모든 주소: tailnet 이름 → 머신 이름
        assert_eq!(
            got("0.0.0.0:7430", ts),
            pairs(&[("mac-a.example.ts.net:7430", "tailnet"), ("mac-a:7430", "hostname")])
        );
        assert_eq!(got("[::]:7430", None), pairs(&[("mac-a:7430", "hostname")]));
        // tailnet 주소로 열면 tailnet 이름이 그 주소에 닿는다
        let tailnet = format!("{}:7430", std::net::Ipv4Addr::new(100, 101, 102, 103));
        assert_eq!(got(&tailnet, ts), pairs(&[("mac-a.example.ts.net:7430", "tailnet"), (&tailnet, "listen")]));
        // 다른 주소(LAN)로 열면 tailnet 이름은 그 주소가 아니므로 쓰지 않는다
        assert_eq!(got("192.0.2.10:7430", ts), pairs(&[("192.0.2.10:7430", "listen")]));
        // loopback은 같은 머신 안에서만
        assert_eq!(got("127.0.0.1:65001", ts), pairs(&[("127.0.0.1:65001", "loopback")]));
        // 이름으로 연 설정은 그대로
        assert_eq!(got("mac-a.local:7430", ts), pairs(&[("mac-a.local:7430", "listen")]));
    }
}
