# coordinator 대화 첨부 검증 (t-2220)

웹 E2E 144는 별도 상태 디렉터리의 TLS agent를 등록하고 영상이 들어 있는 실제 thread 보고를 수집한다.
그 agent가 끊어진 상태에서 원본 바이트의 HEAD·Range 응답과 coordinator 대화의 영상 재생을 검사한다.
보고 판 외부 파일·지원 외 형식·미인증 기기·교차 출처·opaque origin은 첨부로 열리지 않아야 한다.
host의 `chat_media_reads_only_listed_bounded_report_files`는 목록에 없는 파일, 상대 경로 이탈,
16 MiB를 넘는 이미지와 Unix symlink를 거부하는 경계를 검사한다.

Chromium·WebKit에서 데스크톱 1160px와 휴대폰 390px를 각각 연다. 실제로 올린 사용자 이미지와 coordinator
답에 붙은 이미지·영상의 미리보기 크기를 비교한다. 이미지는 원본 확대와 닫기·휴대폰 뒤로 가기,
영상은 첫 프레임이 들어 있는 미리보기, 큰 재생기의 시작·멈춤·탐색과 닫기를 확인한다.
macOS WebKit의 탐색 예외와 재검증 조건은 [t-2236 기록](testing-webkit-seek-t-2236.md)을 따른다.

## 개발 중에 발견한 문제

- WebKit은 동일 MP4가 미리보기와 확대 재생기에 남아 있을 때 전체 바이트를 받은 뒤에도 일시정지 상태의
  탐색이 `seeking=true`에 머무를 수 있었다. aaa 밖의 기본 HTML 재생기로도 확인했다. 첫 프레임을 작은 정지
  이미지로 남긴 뒤 미리보기 디코더를 해제한다.
- 생성되는 coordinator 안내가 16 KiB를 넘었다. 본문에는 안내를 가리키는 한 줄을 두고 실행 방법을
  `aaa guide reports`로 옮겼다. 기존 안내 크기 검사가 이 경계를 유지한다.
- 개발 중 웹 번들 다시 만들기와 Rust의 내장 자산 대조를 겹쳐 실행했다. 자산 대조가 이전 번들의 파일을
  찾을 수 없었으므로 새 번들을 고정한 뒤 Rust 검사에 사용했다.

## 검증 범위

- 웹 유닛 727개와 웹 E2E 23, 115, 144가 통과했다.
- Apple Silicon에서 `aaa-host` 시험 994개와 실행 파일의 crate 의존 경계 시험 2개가 통과했다.
- Rust 포맷, Apple Silicon native와 Linux·Windows strict Clippy, 공개 저장소 검사를 함께 확인한다.
