//! Agent link 보안(기획서 §6.1): TLS 1.3 + 양쪽 자체 서명 인증서의 fingerprint 고정(pinning).
//!
//! - host·agent가 각자 장치 키(ECDSA P-256)와 자체 서명 인증서를 만든다(상태 폴더, 0600).
//! - TLS 단계에서는 서명만 검증하고, 상대가 「누구인지」는 인증서 SHA-256 fingerprint로 앱 단계에서 판정한다.
//! - enrollment 때 양쪽이 TLS exporter에서 6자리 확인 코드(SAS)를 유도한다. 중간자가 끼면 양쪽 코드가 달라진다.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use sha2::Digest;

use crate::util;

#[derive(Clone)]
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: String,
}

pub fn fingerprint(der: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(der))
}

/// `<dir>/tls/cert.der`·`key.der`를 읽거나 처음이면 만든다.
pub fn load_or_create(dir: &Path, common_name: &str) -> Result<Identity> {
    let tls = dir.join("tls");
    let (cert_p, key_p) = (tls.join("cert.der"), tls.join("key.der"));
    if cert_p.exists() && key_p.exists() {
        let cert_der = std::fs::read(&cert_p)?;
        let key_der = std::fs::read(&key_p)?;
        let fingerprint = fingerprint(&cert_der);
        return Ok(Identity { cert_der, key_der, fingerprint });
    }
    util::ensure_private_dir(&tls)?;
    let key = rcgen::KeyPair::generate().context("장치 키 생성")?;
    let mut params = rcgen::CertificateParams::new(vec![common_name.to_string()])?;
    params.distinguished_name.push(rcgen::DnType::CommonName, common_name);
    let cert = params.self_signed(&key)?;
    let cert_der = cert.der().to_vec();
    let key_der = key.serialize_der();
    util::write_private(&key_p, &key_der)?;
    util::write_private(&cert_p, &cert_der)?;
    let fingerprint = fingerprint(&cert_der);
    Ok(Identity { cert_der, key_der, fingerprint })
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn algs() -> WebPkiSupportedAlgorithms {
    rustls::crypto::ring::default_provider().signature_verification_algorithms
}

/// agent 쪽: host 인증서가 고정한 fingerprint와 같아야 한다. 고정값이 없으면(등록 중) 받아들이고 기록한다.
#[derive(Debug)]
struct PinnedServer {
    pin: Option<String>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match &self.pin {
            Some(pin) if *pin != fingerprint(end_entity) => {
                Err(rustls::Error::General("host 인증서 fingerprint가 등록 때와 다르다".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2는 쓰지 않는다".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// host 쪽: 클라이언트 인증서는 반드시 요구하고 서명을 검증한다. 등록된 장치인지는 연결 뒤 fingerprint로 본다.
#[derive(Debug)]
struct AnyClientCert {
    algs: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for AnyClientCert {
    fn offer_client_auth(&self) -> bool {
        true
    }
    fn client_auth_mandatory(&self) -> bool {
        true
    }
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        _e: &CertificateDer<'_>,
        _i: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2는 쓰지 않는다".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

fn key(id: &Identity) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_der.clone()))
}

pub fn client_config(id: &Identity, pin: Option<String>) -> Result<Arc<rustls::ClientConfig>> {
    let cfg = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServer { pin, algs: algs() }))
        .with_client_auth_cert(vec![CertificateDer::from(id.cert_der.clone())], key(id))?;
    Ok(Arc::new(cfg))
}

pub fn server_config(id: &Identity) -> Result<Arc<rustls::ServerConfig>> {
    let cfg = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(AnyClientCert { algs: algs() }))
        .with_single_cert(vec![CertificateDer::from(id.cert_der.clone())], key(id))?;
    Ok(Arc::new(cfg))
}

pub fn server_name() -> ServerName<'static> {
    ServerName::try_from("aaa-host").unwrap()
}

/// 확인 코드(SAS): TLS exporter + 양쪽 fingerprint에서 6자리 숫자.
pub fn sas<D>(conn: &rustls::ConnectionCommon<D>, host_fp: &str, agent_fp: &str) -> Result<String> {
    let ctx = format!("{host_fp}:{agent_fp}");
    let out = conn
        .export_keying_material([0u8; 8], b"EXPERIMENTAL-aaa-enroll-sas", Some(ctx.as_bytes()))
        .context("TLS exporter")?;
    let n = u64::from_be_bytes(out) % 1_000_000;
    Ok(format!("{n:06}"))
}

pub fn peer_fingerprint<D>(conn: &rustls::ConnectionCommon<D>) -> Option<String> {
    conn.peer_certificates().and_then(|c| c.first()).map(|c| fingerprint(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("aaa-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    async fn handshake(
        host: &Identity,
        agent: &Identity,
        pin: Option<String>,
    ) -> Result<(String, String, Option<String>, Option<String>)> {
        let (a, b) = tokio::io::duplex(64 << 10);
        let acceptor = tokio_rustls::TlsAcceptor::from(server_config(host)?);
        let connector = tokio_rustls::TlsConnector::from(client_config(agent, pin)?);
        let (hfp, afp) = (host.fingerprint.clone(), agent.fingerprint.clone());
        let server = tokio::spawn(async move {
            let mut s = acceptor.accept(a).await?;
            let mut buf = [0u8; 2];
            s.read_exact(&mut buf).await?;
            let (_, c) = s.get_ref();
            anyhow::Ok((sas(c, &hfp, &afp)?, peer_fingerprint(c)))
        });
        let mut c = connector.connect(server_name(), b).await?;
        c.write_all(b"hi").await?;
        let (_, cc) = c.get_ref();
        let client_sas = sas(cc, &host.fingerprint, &agent.fingerprint)?;
        let client_saw = peer_fingerprint(cc);
        let (server_sas, server_saw) = server.await??;
        Ok((client_sas, server_sas, client_saw, server_saw))
    }

    #[tokio::test]
    async fn pinning_and_sas() {
        let dir = tmp("pin");
        let host = load_or_create(&dir.join("h"), "aaa-host").unwrap();
        let agent = load_or_create(&dir.join("a"), "aaa-agent").unwrap();
        // 다시 읽어도 같은 identity
        assert_eq!(load_or_create(&dir.join("h"), "aaa-host").unwrap().fingerprint, host.fingerprint);

        // 등록 전(pin 없음): 연결되고 양쪽 SAS가 같다. 서로 상대 fingerprint를 본다.
        let (cs, ss, c_saw, s_saw) = handshake(&host, &agent, None).await.unwrap();
        assert_eq!(cs, ss);
        assert_eq!(cs.len(), 6);
        assert_eq!(c_saw.as_deref(), Some(host.fingerprint.as_str()));
        assert_eq!(s_saw.as_deref(), Some(agent.fingerprint.as_str()));

        // 올바른 pin
        assert!(handshake(&host, &agent, Some(host.fingerprint.clone())).await.is_ok());
        // 다른 host(중간자)가 나서면 pin 불일치로 실패
        let other = load_or_create(&dir.join("x"), "aaa-host").unwrap();
        assert!(handshake(&other, &agent, Some(host.fingerprint.clone())).await.is_err());

        // 다른 연결은 SAS가 다르다(재사용 불가)
        let (cs2, _, _, _) = handshake(&host, &agent, None).await.unwrap();
        assert_ne!(cs, cs2, "같은 코드가 나올 확률은 1/10^6");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
