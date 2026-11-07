# 새 대화 기록 생성 전 상태 검증(t-1580)

## 변경 계약

Claude·Codex가 기록 폴더나 JSONL 파일을 아직 만들지 않은 상태는 빈 대화로 응답한다.
빈 응답에는 메시지·앞 쪽 cursor·history가 없으며, 기록 생성 뒤 같은 조회에서 정상 메시지가 보인다.
파일 경로를 검증한 뒤 열기 전에 사라지는 경우도 같은 상태다.

`NotFound`를 문자열로 추측하지 않고 내부 오류 종류로 구분한다. 권한·다른 I/O 오류,
외부 경로·상위 경로·잘못된 확장자·일반 파일이 아닌 대상의 거부는 오류 응답으로 남긴다.
실제로 읽을 파일은 기존처럼 canonicalize 후 기록 root 안의 일반 JSONL인지 검증한다.
Codex 계정 홈의 후보 기록이 읽기·메타데이터 손상으로 실패한 경우도 기존 오류를 유지한다.
이전 실행·다른 작업의 정상 기록만 있으면 새 기록이 아직 없는 상태로 반환한다.

## 회귀 owner와 필요성

- `chat_paths_tests::new_chat_is_empty_until_claude_and_codex_create_the_record`:
  실제 agent 요청으로 두 harness 각각 폴더 없음 → 폴더만 있음 → 첫 기록 표시를 확인한다.
  빈 기록에 history가 발급되지 않는 것과 검증 직후 파일이 사라지는 경합도 확인한다.
  시작 시 오류와 이후 기록을 놓치는 회귀를 함께 막는다.
- `chat_paths_tests::chat_keeps_path_rejections_and_real_io_errors`:
  기존·미생성 외부 파일, 상대·상위 경로, 확장자, 폴더, symlink 탈출과 실제 파일 권한 거부를 확인한다.
  Codex 탐색의 폴더가 파일인 I/O 실패도 오류여야 한다. 없는 기록 처리로 보안 거부·읽기 실패를 가리는 회귀를 막는다.
- `chat_paths_tests::codex_account_chat_keeps_unreadable_or_invalid_record_errors`:
  계정 홈의 ID 보완 탐색에서 이전 기록만 있을 때는 빈 대화이며, 손상된 메타데이터·실제 읽기 권한 거부는 오류인지 확인한다.
- E2E37 `새 기록 생성 전후: Chromium claude`, `새 기록 생성 전후: WebKit codex`:
  실험실 JSONL → agent → host → 웹을 통과한다. 빈 대화 DOM을 기다린 뒤 기록을 만들고 첫 줄 표시를 기다린다.
  그동안 경고 띠가 잠깐이라도 생겼는지 MutationObserver로 관측한다. 응답을 가로채거나 sleep하지 않는다.
  기존 E2E37의 긴 대화·앞 쪽 읽기도 같은 spec에 유지한다.

## 재현과 확인

수정 전 새 세션 owner는 `기록 폴더를 열지 못했다: NotFound (os error 2)` 오류 응답으로 실패했다.
수정 후 agent 전체 207개 시험이 통과했다. 추가한 오류 경계 시험의 최초 실행은
fixture의 Codex 홈 부모 폴더가 없어 실패했으며, fixture에서 부모 폴더를 명시적으로 만든 뒤 통과했다.
Clippy가 지적한 Unix 권한 리터럴도 8진수 표기로 고쳤다.
추가 검토에서 이전 Codex 기록만 있는 계정 홈도 오류로 남는 것을 재현했고, 실제 후보 읽기 실패와
단순히 이번 기록이 없는 경우를 나눠 수리했다.
웹의 작업 분할 owner는 E2E37 단계가 정확히 두 개라고 가정해 새 두 단계 추가 뒤 실패했다.
기존 긴 대화 두 단계와 새 두 단계의 이름을 모두 명시하고, 반복마다 각각 한 번 실행되는지와
네 단계에 대한 기존 시간 가중치 분배를 확인하도록 바꿨다. 제품의 시한·허용 오차·재시도 설정은 바꾸지 않았다.

개발 검증 명령은 다음과 같다. 착지 gate는 승인된 착지 창의 최종 통합 SHA에서 수행한다.

```sh
NODE_PATH="$PWD/target/playwright/node_modules" aaa verify -- nice -n 15 python3 scripts/verify.py develop --base origin/main --package aaa-agent --spec 37
```

최종 개발 확인 결과:

| 범위 | 결과 |
| --- | --- |
| `aaa-agent` 전체 시험 | 207개 통과, doctest 0개 |
| `aaa-agent` 전체 target strict Clippy | 통과 |
| 웹 타입·번들·유닛 | 빌드 성공, 711개 통과 |
| E2E37 | 기존 긴 대화 두 단계와 새 기록 두 단계, 4/4 통과 |
| E2E 목록 수집 | 새 두 단계를 포함한 네 이름 수집 확인 |
| 공개 검사·diff 공백 검사·fmt | 통과 |

새 브라우저 단계는 각각 5초 안에 끝났고 고정 sleep은 추가하지 않았다.
검증 중 미커밋 상태여서 공통 원장 자동 수집은 수행되지 않았으며, 개발 실행 결과는 로컬 로그에 보존했다.
이 결과는 관련 owner의 개발 확인이다. 전체 ci-local·OS cross·Windows production은 착지 후보마다 반복하지 않았다.

## 참고한 공개 구현

- [OpenAI Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs),
  [Apache-2.0](https://github.com/openai/codex/blob/main/LICENSE): JSONL 기록 작성과 읽기 수명을 참고했다.
- [OpenCode storage](https://github.com/sst/opencode/blob/dev/packages/opencode/src/storage/storage.ts),
  [MIT](https://github.com/sst/opencode/blob/dev/LICENSE): 없는 파일을 별도 `NotFoundError`로 구분하는 방식을 참고했다.

외부 코드는 가져오지 않았고 새 의존성도 추가하지 않았다.
