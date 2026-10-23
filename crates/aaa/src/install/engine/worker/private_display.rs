//! 상호 OS peer/고정 image 확인 뒤만 여는 일시 private callback. 공개 Progress/Result와 분리한다.
use super::*;
use std::io::Write;
use tokio::{
    io::{ReadHalf, WriteHalf},
    sync::{mpsc, watch},
};
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::install) enum Request {
    CanDisplay,
    Sas { code: String, host_fingerprint: String },
    Pairing,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Answer {
    Ready,
    Declined,
}
pub(in crate::install::engine) struct Subscriber {
    writer: WriteHalf<aaa_common::ipc::LocalStream>,
    answers: mpsc::Receiver<Answer>,
    alive: watch::Receiver<bool>,
    reader: tokio::task::JoinHandle<()>,
    private_peer: bool,
}
impl Drop for Subscriber {
    fn drop(&mut self) {
        self.reader.abort();
    }
}
impl Subscriber {
    pub(super) async fn connect(state: &Path, id: &str) -> Result<Self> {
        let (stream, pid) = aaa_common::ipc::connect_peer(&socket(state, id)?).await.map_err(Fault::io)?;
        let parent = aaa_common::process::parent(std::process::id());
        let private_peer = pid.is_some()
            && pid == parent
            && pid.is_some_and(|pid| {
                parent_image(pid)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
            });
        let (reader, writer) = tokio::io::split(stream);
        let (answers, rx) = mpsc::channel(4);
        let (alive, watch) = watch::channel(true);
        let reader = tokio::spawn(read_answers(reader, answers, alive));
        Ok(Self { writer, answers: rx, alive: watch, reader, private_peer })
    }
    pub(super) async fn send(&mut self, reply: &Reply) -> bool {
        let Ok(bytes) = serde_json::to_vec(reply)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return false;
        };
        tokio::time::timeout(
            Duration::from_secs(2),
            write_frame(&mut self.writer, FrameKind::Control, &bytes, 128 * 1024),
        )
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|r| {
            r.as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok()
        })
    }
    pub(in crate::install::engine) async fn request(&mut self, request: Request) -> Result<()> {
        if !self.private_peer {
            return Err(failed("installation_private_display_required", 3));
        }
        if !self.send(&Reply::Private { request }).await {
            return Err(failed("installation_private_display_lost", 3));
        }
        match tokio::time::timeout(Duration::from_secs(10), self.answers.recv()).await {
            Ok(Some(Answer::Ready)) => Ok(()),
            Ok(Some(Answer::Declined)) => Err(failed("installation_private_display_required", 3)),
            _ => Err(failed("installation_private_display_lost", 3)),
        }
    }
    pub(in crate::install::engine) fn closed(&self) -> impl std::future::Future<Output = ()> + use<> {
        let mut alive = self.alive.clone();
        async move {
            loop {
                if !*alive.borrow_and_update() {
                    break;
                }
                if alive
                    .changed()
                    .await
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}
async fn read_answers(
    reader: ReadHalf<aaa_common::ipc::LocalStream>,
    answers: mpsc::Sender<Answer>,
    alive: watch::Sender<bool>,
) {
    let mut reader = FrameRx::new(reader, 4096);
    while let Ok(answer) = reader
        .control::<Answer>()
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "수신 루프 종료 원인"))
    {
        if answers
            .send(answer)
            .await
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            break;
        }
    }
    aaa_common::diagnostic::ignored(alive.send(false), "수신자가 끝난 뒤에도 현재 작업은 마친다");
}
fn parent_image(pid: u32) -> Result<()> {
    let image = aaa_common::process::image(pid).ok_or_else(|| failed("installation_private_display_required", 3))?;
    if crate::update::sha256(&image)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?
        != executor_sha()?
    {
        return Err(Fault::conflict());
    }
    #[cfg(windows)]
    if !supervisor::same_account(pid) {
        return Err(Fault::conflict());
    }
    Ok(())
}

pub(super) async fn answer(request: Request, state: &Path, private: bool) -> Answer {
    if !private
        || crate::install::review::outside_async(state)
            .await
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
    {
        return Answer::Declined;
    }
    match request {
        Request::CanDisplay => Answer::Ready,
        Request::Sas { code, host_fingerprint } => {
            if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) || !digest_ok(&host_fingerprint) {
                return Answer::Declined;
            }
            println!("확인 코드: {} {}", &code[..3], &code[3..]);
            println!("신뢰하는 host 화면의 같은 머신·코드를 대조해 승인하세요. 새 연결에서는 새 코드입니다.");
            if std::io::stdout()
                .flush()
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                Answer::Declined
            } else {
                Answer::Ready
            }
        }
        Request::Pairing => {
            let Ok(mut client) = aaa_common::front::FrontClient::connect(state).await.inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                return Answer::Declined;
            };
            match client.call(aaa_proto::FrontRequest::WebPair { role: aaa_proto::DeviceRole::Controller }).await {
                Ok(Ok(aaa_proto::FrontResponse::PairCode { code, .. })) => {
                    println!("첫 기기 Controller pairing 코드: {code}");
                    println!("기존 pairing의 10분·1회 사용·앞 코드 폐기 규칙을 따릅니다.");
                    if std::io::stdout()
                        .flush()
                        .as_ref()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_err()
                    {
                        Answer::Declined
                    } else {
                        Answer::Ready
                    }
                }
                _ => Answer::Declined,
            }
        }
    }
}
