# Claude 계정 thread의 advisorModel 전달(t-1251)

## 변경과 참고

계정 격리 전 실행 환경의 Claude 사용자 설정에서 문자열 `advisorModel` 하나만 읽고
기존 `--settings`의 `quota-settings.json`에 추가한다. 사용자 설정 전체를 복사하지 않는다.
원본은 읽기만 하며 aaa hook·사용량 상태줄·격리 홈·권한 정책은 기존대로 유지한다.
읽기 실패·잘못된 JSON·잘못된 자료형은 원인과 함께 경고하고 실행을 막지 않는다.
키가 없거나 삭제되면 다음 실행의 설정에 넣지 않는다. 모델 별칭은 aaa가 해석하지 않는다.

참고 후보 두 개를 비교했다.

- [Claude Agent SDK Python 설정](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/types.py)은
  파일 설정의 출처와 실행별 `settings`를 구분한다.
  [MIT](https://github.com/anthropics/claude-agent-sdk-python/blob/main/LICENSE).
- [Claude Code Action 설정](https://github.com/anthropics/claude-code-action/blob/main/docs/configuration.md)은
  실행별 JSON·설정 파일로 필요한 설정을 전달한다.
  [MIT](https://github.com/anthropics/claude-code-action/blob/main/LICENSE).

두 후보의 실행별 설정 전달 방식을 참고하고 aaa의 기존 설정 생성 위치를 확장했다.
외부 코드·의존성은 가져오지 않았다. `advisorModel`의 의미는
[공식 advisor 문서](https://code.claude.com/docs/en/advisor), 설정 홈·우선순위는
[공식 설정 문서](https://code.claude.com/docs/en/settings)를 근거로 했다.

## 자동 검증

- `cargo test --profile e2e -p aaa-agent --lib`: 186개 통과, 실행 2.94초.
- `cargo test --profile e2e -p aaa --test m13 accounts::`: 36개 통과, 실행 15.91초.
- `cargo test --profile e2e -p aaa --test deps`: 2개 통과, 실행 0.01초.
- `cargo clippy --profile e2e --workspace --all-targets --locked --keep-going -- -D warnings`: 통과.
- `cargo fmt --all --check`·`git diff --check`·`scripts/check-public.sh`: 통과.
- `node --test web/test/fixed-sleeps.test.mjs`: 8개 통과.

새 유닛 `advisor_reads_only_a_string_and_observes_changes_and_removal`은 설정 없음·문자열·
변경·삭제와 JSON/자료형/읽기 오류를 확인한다. 필요성: 깨진 사용자 설정 때문에 계정 실행을
막거나 오래된 advisor 값을 재사용하지 않아야 한다. 실행은 0.01초 미만이다.

새 통합 `account_advisor_local`·`account_advisor_remote`는 실제 CLI→host→로컬/원격 TLS
agent→session→PTY 경로의 가짜 Claude가 최종 `--settings` 파일을 읽는다.
필요성: profile 환경의 사용자 홈을 계정 격리보다 먼저 읽고 `fable`만 전달해야 한다.
원본의 권한·hooks·env·상태줄을 fixture에 함께 넣고 aaa가 만든 hook·상태줄과 advisor만
최종 파일에 있는지, 원본 사용자 파일이 그대로인지 확인한다. 최초 두 시험 합계 2.44초.
기존 첫 실행 시험도 advisor가 없는 경우의 최종 설정을 확인하도록 확장했다.
계정 시험의 원본 홈은 모두 실험실 안의 가짜 홈이다.

`advisorModel`을 최종 설정에 넣지 않는 결함을 임시로 심으면
`account_advisor_local`이 advisor 전용 단언에서 실패했다(1.04초). 원본 코드는 즉시 복원했고 최종 로컬·원격 두 시험은 1.45초에 통과했다.
고정 sleep·재시도·시한 연장·기대값 완화·시험 삭제는 없다.

## 실제 CLI와 남은 확인

설치된 실제 Claude Code 2.1.312에서 모델 `sonnet`과 로컬 조회 명령 `/advisor`를 사용했다.
인증 파일·키체인·토큰을 직접 열거나 복사하지 않았다. MCP는 strict 설정으로 제외하고
표준 입력을 닫았다. 정상 사용자 로그인 환경에서는 `Advisor: Fable 5.1`을 표시했다.
결과의 `duration_api_ms=0`, `total_cost_usd=0`으로 모델 호출이 없음을 확인했다.

최소 격리 홈과 `advisorModel=fable`만 주고 가짜 인증 또는 인증 없이 같은 명령을 조회하면
`/advisor isn't available in this environment.`를 반환했다. 이를 실제 계정 thread에서의
도구 노출 성공으로 세지 않는다. 서버 도구는 init의 일반 tools 목록만으로 확인하지 않는다.

이 세션의 운영 CLI는 읽기만 가능하고 착지 창도 받지 않았다. 실제 인증 계정으로 후보를
실행하는 운영 thread와 배포를 만들지 않았다. 착지 창에서 후보를 적용한 뒤 새 계정 Claude
thread의 최종 `quota-settings.json`, `/advisor` 표시와 advisor 도구 활성 여부를 확인해야 한다.
설정 전달 회귀는 확인했으며 이 실제 계정 확인은 아직 미완료다.
