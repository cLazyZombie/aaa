//! 이미지 전용 CLI. 로컬·원격 모두 세션 agent를 거쳐 host에 요청한다.
use aaa_proto::{
    B64, IMAGE_CHUNK, IMAGE_MAX_BYTES, IMAGE_MAX_COUNT, ImageBegin, ImageFile, ImageRequest, ImageResponse, ImageState,
    ImageView,
};
use anyhow::{Context, Result, bail, ensure};
use clap::Subcommand;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Subcommand)]
pub enum Cmd {
    /// 자기 작업 목록. coordinator는 소속 프로젝트 전체를 본다.
    List,
    /// host Codex 계정으로 생성·편집하고 결과를 현재 머신에 받는다.
    Generate {
        #[arg(long)]
        prompt_file: PathBuf,
        /// 모델을 명시한 host의 Codex profile
        #[arg(long)]
        profile: String,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        account: Option<String>,
        #[arg(long = "image")]
        images: Vec<PathBuf>,
        #[arg(long)]
        transparent: bool,
        /// 응답 유실 뒤 같은 요청을 확인할 때 재사용한다.
        #[arg(long)]
        id: Option<String>,
    },
    Status {
        id: String,
    },
    /// host의 입력·결과 파일을 지운다. 중복 방지·사용량 원장은 보존한다.
    Forget {
        id: String,
    },
    /// 계정 배정 대기만 다시 판정한다. 이미 실행한 요청은 재생성하지 않는다.
    Start {
        id: String,
        #[arg(long)]
        output_dir: PathBuf,
    },
    Fetch {
        id: String,
        #[arg(long)]
        output_dir: PathBuf,
    },
}
fn descriptor(bytes: &[u8]) -> ImageFile {
    ImageFile { bytes: bytes.len() as u64, sha256: hex::encode(Sha256::digest(bytes)) }
}
fn read(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    std::fs::File::open(path)?.take(max + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= max, "입력 파일 크기 한도를 넘었다");
    Ok(bytes)
}
async fn send(request: ImageRequest) -> Result<ImageResponse> {
    aaa_agent::image::send(request).await
}
fn view(response: ImageResponse) -> Result<ImageView> {
    match response {
        ImageResponse::Job(v) => Ok(v),
        _ => bail!("이미지 작업 상태 응답이 아니다"),
    }
}
async fn status(id: &str, wait: bool) -> Result<ImageView> {
    view(send(ImageRequest::Status { id: id.into(), wait }).await?)
}
async fn wait(id: &str) -> Result<ImageView> {
    loop {
        let v = status(id, true).await?;
        if !matches!(v.state, ImageState::Queued | ImageState::Running) {
            return Ok(v);
        }
    }
}
async fn fetch(v: &ImageView, dir: &Path) -> Result<Vec<PathBuf>> {
    ensure!(v.state == ImageState::Succeeded, "{}: {:?} · {}", v.id, v.state, v.reason);
    std::fs::create_dir_all(dir)?;
    let mut paths = vec![];
    for (index, expected) in v.outputs.iter().enumerate() {
        ensure!(expected.bytes > 0 && expected.bytes <= IMAGE_MAX_BYTES, "출력 크기 한도를 넘었다");
        let mut bytes = vec![];
        loop {
            let ImageResponse::Chunk { data, eof } =
                send(ImageRequest::Read { id: v.id.clone(), index: index as u32, offset: bytes.len() as u64 })
                    .await
                    .with_context(|| {
                        format!("이미지 결과 수신: 작업 {}, 출력 {index}, offset {}", v.id, bytes.len())
                    })?
            else {
                bail!("이미지 조각 응답이 아니다")
            };
            ensure!(
                data.0.len() <= IMAGE_CHUNK
                    && (!data.0.is_empty() || eof)
                    && bytes.len() as u64 + data.0.len() as u64 <= expected.bytes,
                "잘못된 이미지 전송 조각이다"
            );
            bytes.extend_from_slice(&data.0);
            if eof {
                break;
            }
        }
        ensure!(&descriptor(&bytes) == expected, "출력 이미지의 길이·SHA가 다르다");
        let ext = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "png"
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            "jpg"
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            "webp"
        } else {
            bail!("출력 이미지 형식을 확인하지 못했다")
        };
        let path = dir.join(format!("{}-{}.{}", v.id, index + 1, ext));
        // 같은 결과를 다시 받으면 SHA가 같을 때만 재사용한다. 기존 사용자 파일을 덮어쓰지 않는다.
        if path.exists() {
            ensure!(
                !path.symlink_metadata()?.file_type().is_symlink()
                    && descriptor(&read(&path, IMAGE_MAX_BYTES)?) == *expected,
                "다른 파일이 출력 위치를 사용 중이다"
            );
        } else {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
            if let Err(e) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                drop(file);
                aaa_common::diagnostic::ignored(
                    std::fs::remove_file(&path),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
                return Err(e.into());
            }
        }
        paths.push(path);
    }
    Ok(paths)
}
pub fn run(json: bool, cmd: Cmd) -> Result<ExitCode> {
    ensure!(aaa_agent::hooks::in_session(), "aaa image는 aaa의 thread·coordinator 세션 안에서 실행한다");
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async {
        if matches!(cmd, Cmd::List) {
            let ImageResponse::Jobs { jobs } = send(ImageRequest::List).await? else {
                bail!("이미지 목록 응답이 아니다")
            };
            if json {
                println!("{}", serde_json::json!({"jobs":jobs}));
            } else {
                for job in jobs {
                    println!("{}\t{:?}\t{}\t{}", job.id, job.state, job.account.as_deref().unwrap_or("-"), job.reason);
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        let (job, output) = match cmd {
            Cmd::List => unreachable!(),
            Cmd::Status { id } => (status(&id, false).await?, None),
            Cmd::Forget { id } => (view(send(ImageRequest::Forget { id }).await?)?, None),
            Cmd::Fetch { id, output_dir } => (status(&id, false).await?, Some(output_dir)),
            Cmd::Start { id, output_dir } => {
                send(ImageRequest::Start { id: id.clone() }).await?;
                (wait(&id).await?, Some(output_dir))
            }
            Cmd::Generate { prompt_file, profile, output_dir, account, images, transparent, id } => {
                ensure!(images.len() <= IMAGE_MAX_COUNT, "참고 이미지는 5개까지다");
                let prompt = String::from_utf8(read(&prompt_file, 32000)?)?;
                ensure!(!prompt.trim().is_empty(), "이미지 설명이 비었다");
                let mut inputs = vec![];
                let mut total = 0;
                for path in images {
                    let bytes = read(&path, IMAGE_MAX_BYTES)?;
                    total += bytes.len() as u64;
                    ensure!(total <= IMAGE_MAX_BYTES, "참고 이미지 합계는 50MiB 이하다");
                    inputs.push(bytes);
                }
                let id = id.unwrap_or_else(aaa_common::util::new_id);
                eprintln!("이미지 작업: {id}"); // 어떤 전송 실패에서도 같은 요청을 찾을 수 있다.
                let initial = view(
                    send(ImageRequest::Begin(ImageBegin {
                        id: id.clone(),
                        prompt,
                        profile,
                        account,
                        transparent,
                        inputs: inputs.iter().map(|b| descriptor(b)).collect(),
                    }))
                    .await?,
                )?;
                if initial.state == ImageState::Draft {
                    for (index, bytes) in inputs.into_iter().enumerate() {
                        for (n, chunk) in bytes.chunks(IMAGE_CHUNK).enumerate() {
                            send(ImageRequest::Input {
                                id: id.clone(),
                                index: index as u32,
                                offset: (n * IMAGE_CHUNK) as u64,
                                data: B64(chunk.to_vec()),
                            })
                            .await?;
                        }
                    }
                }
                send(ImageRequest::Start { id: id.clone() }).await?;
                (wait(&id).await?, Some(output_dir))
            }
        };
        let paths = if let Some(dir) = output { Some(fetch(&job, &dir).await?) } else { None };
        if json {
            println!("{}", serde_json::json!({"job":job,"files":paths}));
        } else {
            println!("{}\t{:?}\t{}\t{}", job.id, job.state, job.account.as_deref().unwrap_or("-"), job.reason);
            if let Some(paths) = paths {
                for path in paths {
                    println!("{}", path.display());
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    })
}
