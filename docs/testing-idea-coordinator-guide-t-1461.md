# 아이디어 요청의 coordinator 안내(t-1461)

## 변경

생성되는 coordinator AGENTS.md의 「메시지마다 세 가지 중 하나」 앞에 아이디어 요청의 예외를 명시했다.
모델 이름을 지정해도 아이디어 탭의 `aaa idea new`·`aaa idea say`와 n-ID 전용 thread를 쓰고,
할 일·일반 thread는 만들지 않는다. 별도 guide 주제 없이 매 턴 읽는 본문에서 바로 안내한다.
기존의 관계없는 일마다 task·thread 하나씩 만드는 문장은 뜻을 유지하며 줄였다.

문안만 바꾸므로 새 시험은 만들지 않고 기존 `aaa-host` 안내 owner로 확인한다.

## 실패 원인과 수리

첫 문안은 생성 안내가 16,540바이트여서 기존 `coordinator_guide_stays_small`의 16 KiB 상한을 넘었다.
새 문안과 같은 절의 중복 표현을 줄였다. 상한·기대값은 바꾸지 않았다.

## 검증

검증 명령은 모두 `aaa verify --`를 통해 실행했다.

- `nice -n 15 cargo test --profile verify --locked -p aaa-host --lib guide::tests::`: 4개 통과.
  안내 크기·주제 목록과 참조·질문 안내의 기존 검사 3개와 같은 필터에 포함된 enroll 안내 검사 1개다.
- `nice -n 15 cargo clippy --profile verify --locked -p aaa-host --lib -- -D warnings`: 통과.
- `nice -n 15 cargo fmt --all -- --check`: 통과.
- `nice -n 15 bash scripts/check-public.sh`: 새 문서를 추적에 넣은 뒤 통과.

미커밋 작업에서 실행했으므로 `aaa verify`의 workspace 원장은 수집되지 않았다.
명령 종료 코드와 실제 시험 출력을 확인한 개발 검사이며, 전체 gate 통과를 뜻하지 않는다.
착지 창에서 수행하는 최종 통합 SHA의 gate는 이 개발 검사에 포함하지 않는다.
