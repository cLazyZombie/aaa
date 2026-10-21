//! 실제 TLS를 해독하지 않는 시험 소유 TCP 중계. host 승인 전에 응답 쪽을 멈춰 commit 뒤 응답 유실을 만든다.
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot},
};
enum Control {
    Pause(oneshot::Sender<()>),
    Disconnect,
}
pub(super) struct ReplyLoss {
    pub address: String,
    control: mpsc::Sender<Control>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ReplyLoss {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl ReplyLoss {
    pub async fn start(target: String) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (control, receiver) = mpsc::channel(2);
        let task = tokio::spawn(async move {
            let mut first = Some(receiver);
            let mut bridges = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=> {
                        let Ok((client,_))=accepted else{break};let target=target.clone();let control=first.take();
                        bridges.spawn(async move {
                            let server=tokio::net::TcpStream::connect(target).await.unwrap();
                            let (mut cr,mut cw)=client.into_split();let (mut sr,mut sw)=server.into_split();
                            let up=tokio::io::copy(&mut cr,&mut sw);
                            let down=async {
                                if let Some(mut control)=control {
                                    let mut bytes=[0u8;16384];
                                    loop {
                                        tokio::select! {biased;
                                            command=control.recv()=>match command {
                                                Some(Control::Pause(ready))=> {let _=ready.send(());let _=control.recv().await;break;}
                                                _=>break,
                                            },
                                            size=sr.read(&mut bytes)=>match size {Ok(0)|Err(_)=>break,Ok(n)=>if cw.write_all(&bytes[..n]).await.is_err(){break;}},
                                        }
                                    }
                                } else {let _=tokio::io::copy(&mut sr,&mut cw).await;}
                            };
                            tokio::select! {_=up=>{},_=down=>{}}
                        });
                    }
                    _=bridges.join_next(),if !bridges.is_empty()=>{},
                }
            }
        });
        Self { address, control, task }
    }
    pub async fn pause(&self) {
        let (ready, wait) = oneshot::channel();
        self.control.send(Control::Pause(ready)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), wait).await.unwrap().unwrap();
    }
    pub async fn disconnect(&self) {
        self.control.send(Control::Disconnect).await.unwrap();
    }
}
