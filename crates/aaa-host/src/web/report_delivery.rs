//! 보고서 텍스트만 협상 압축한다. 권한·Range·원본 내려받기는 기존 보기 경로가 결정한다.
use std::io::Write;

use futures_util::StreamExt;

pub(super) fn compressible(kind: &str) -> bool {
    kind.starts_with("text/") || matches!(kind, "application/json" | "application/javascript" | "image/svg+xml")
}

/// 원본의 한 조각만 처리하고 압축 출력을 바로 비운다. 전체 파일을 메모리에 모으지 않는다.
pub(super) fn gzip(body: crate::library::ByteStream) -> crate::library::ByteStream {
    let encoder = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::fast());
    futures_util::stream::unfold((body, Some(encoder)), |(mut body, encoder)| async move {
        let mut encoder = encoder?;
        let (result, next) = match body.next().await {
            Some(Ok(chunk)) => match encoder.write_all(&chunk).and_then(|()| encoder.flush()) {
                Ok(()) => (Ok(std::mem::take(encoder.get_mut())), Some(encoder)),
                Err(error) => (Err(error), None),
            },
            Some(Err(error)) => (Err(error), None),
            None => (encoder.finish(), None),
        };
        if let Err(error) = &result {
            aaa_common::diagnostic::observe(error, "보고서 gzip stream 읽기·압축 실패");
        }
        Some((result, (body, next)))
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gzip_emits_before_upstream_finishes_and_keeps_read_failure() {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let input = futures_util::stream::unfold(rx, |mut rx| async { rx.recv().await.map(|b| (b, rx)) }).boxed();
        let mut output = gzip(input);
        tx.send(Ok(b"first report chunk".to_vec())).await.unwrap();
        let first =
            tokio::time::timeout(std::time::Duration::from_secs(5), output.next()).await.unwrap().unwrap().unwrap();
        assert!(first.starts_with(&[0x1f, 0x8b]));
        tx.send(Err(std::io::Error::other("report read failure"))).await.unwrap();
        assert_eq!(output.next().await.unwrap().unwrap_err().to_string(), "report read failure");
        assert!(output.next().await.is_none());
    }
}
