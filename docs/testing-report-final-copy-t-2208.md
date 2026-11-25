# 종료 전 마지막 보고 사본과 작업 폴더 정리 검증(t-2208)

## 회수 계약

| 이전 수리 | 대조한 계약 |
| --- | --- |
| t-0304 | 완료 보고 본문이 같아도 마지막 회수는 library를 다시 읽는다. 부분 사본은 worktree를 보존하고 명시적인 복사 생략·폐기 선택을 따른다. |
| t-0430 | 열린 작업의 사본 오류는 같은 hash로 재수집하며 알림을 한 번 발행한다. 용량 상한으로 생긴 부분 사본과 오류를 구별한다. |
| t-0576 | r-ID는 해당 판의 검증된 첨부를 읽는다. 최신 t-ID는 원본이 남아 있을 때 agent에서 부분 사본의 첨부를 읽을 수 있다. |
| t-0638·t-0654 | 세션이 파일을 갱신하는 동안 전송한 바이트를 검증한다. 첨부 또는 불변 판 저장 실패가 생겨도 새 본문과 정상 첨부를 보존한다. |
| t-0689·t-1851 | 보류는 작업 폴더와 이어 할 자리를 보존한다. CLI 종료는 마지막 사본에서 발견한 후속도 정리 전에 검사한다. |
| t-1132·t-1499·t-1559 | 링크된 작은 근거의 우선 전송과 상한 표시, 자동화 기억의 hash 검증, 빈 본문의 공개 보류를 유지한다. |
| t-1180·t-2021 | 바이너리는 검증한 바이트를 보존하고 의심 조각을 별도로 알린다. 종료 보고의 사본은 일반 보존 정책을 따른다. |

배경 회수는 잠금을 잡고 열린 상태와 현재 머신·작업 폴더를 확인한다. 종료·보류는 같은 잠금 안에서
마지막 사본을 받고 상태를 저장한다. 이어 하기는 작업 자리를 바꾸기 전 종료·보류와 같은 작업 잠금을 잡는다.

마지막 회수는 본문 확인과 전송에 필요한 목록을 한 요청에서 받는다. 현재 agent는 보고와 첨부가 모두
없음을 확인한 경우와 보고 없이 첨부가 남은 경우를 구별한다. 쓰는 중인 빈 본문이면 기존 판을 보존하고
종료를 보류한다. 전송·저장 오류가 있으면 원본을 남기고 원인을 표시한다. 직접 먼저 지운 worktree는
기존처럼 남은 Git 기록을 정리한다. 이전 agent가 본문 부재만 알려 준 worktree는 종료에 성공했다고 처리할
근거가 부족하다. 보고를 작성하거나 `--skip-copy`로 원본을 보존하면서 끝낼 수 있다.

## 회귀 시험

- 마지막 사본 요청을 가짜 agent가 받은 사건에서 멈추고, 배경 회수가 잠금을 기다리는 순서를 제어한다.
  종료·보류 뒤 사본 완료 상태, 판 목록, 최신 t-ID와 전후 r-ID의 첨부 바이트를 확인한다. 늦은 요청의 전송·경고·실패 사건도 확인한다.
- 이 시험에서 상태 확인을 잠금 밖으로 되돌리면 종료 후 사본 상태가 `complete`에서 `failed`로 바뀌어 실패한다.
- 실제 host·agent·Git worktree에서 첫 보고 전에 첨부만 만든다. 종료를 요청하면 파일을 보존하고 오류를
  반환한다. 본문을 쓴 뒤 종료하면 작업 폴더를 정리하고 저장한 첨부를 CLI로 읽을 수 있다.
- 빈 보고 중 도착한 마지막 회수, 첨부만 남은 상태, 파일 상태 조회 오류, 연결 오류에서 작업과 앞 판을 보존한다.
  기존 회귀는 부분 사본, 같은 hash 재수집, 원격 전송 끊김, 불변 판 저장 실패, 자동화 기억과 후속 확인을 검증한다.

## 개발 확인

모든 검증 명령은 `aaa verify -- <명령>`으로 실행했다.

| 검사 | 결과 |
| --- | --- |
| `cargo test --profile verify --locked -p aaa-host -p aaa-agent --lib` | host 987개(보고 회수 36개 포함), agent 226개 통과. 기존 host 2개 제외 |
| `cargo test --profile verify --locked -p aaa --test report_copy --test m13 --test deps` | report_copy 4개, m13 101개(보류 3개 포함), deps 2개 통과. 기존 CI 전용 m13 1개 제외 |
| `cargo fmt --all -- --check`와 `scripts/check-public.sh` | 통과 |
| macOS 관련 crate strict Clippy, Linux·Windows 전체 strict 교차 Clippy | 통과 |

macOS에서 Rust 시험을 실행했다. Linux와 Windows 검사는 교차 Clippy다.
착지에서는 최종 통합 SHA에 대해 저장소 전체 gate를 실행한다.

## 화면 감지 줄 경계

첫 착지의 Windows 실기 시험은 CRLF로 체크아웃된 Claude 화면을 읽다가 멈췄다.
화면 영역의 바이트 위치를 `lines()`가 반환한 줄 길이와 1바이트 줄 끝으로 계산해, 가로줄의 UTF-8 문자 중간을 잘랐다.
같은 계산을 하던 Codex 현재 프롬프트 앞 영역과 Claude 마지막 가로줄 뒤 영역까지 실제 구분자를 포함한 원문 길이로 바꿨다.
호출자는 줄의 내용으로 영역을 고르고 원문에서 위치를 센다. 마지막 줄에 개행이 없어도 원문의 끝에서 멈춘다.

새 영역 시험은 한글·이모지·가로줄과 LF·CRLF·빈 줄을 한 화면에 넣어 추출한 원문을 대조한다.
수리 전에는 macOS에서도 Codex 프롬프트 앞의 한글 문자 중간을 자르며 실패했다.
주변 코드를 함께 검사했다. brief 머리는 이미 개행을 포함한 길이를 세고, 대화 JSONL과 질문 안내는
`\n`으로만 나누므로 각 조각의 길이에 `\r`이 포함된다.

## 참고

Tokio의 FIFO 비동기 잠금과 Git의 worktree 정리 조건을 대조했다. 기존 Tokio 잠금과 Git 정리 경로를 사용한다.
불변 보고 판은 GitHub Actions의 아티팩트처럼 수정본에 새 식별자를 부여한다.
[Tokio Mutex](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html) · [MIT](https://github.com/tokio-rs/tokio/blob/master/tokio/LICENSE)
· [Git worktree](https://git-scm.com/docs/git-worktree) · [GPL-2.0](https://github.com/git/git/blob/master/COPYING)
· [Actions 아티팩트](https://github.com/actions/upload-artifact/blob/main/docs/MIGRATION.md#overwriting-an-artifact)
· [MIT](https://github.com/actions/upload-artifact/blob/main/LICENSE)
