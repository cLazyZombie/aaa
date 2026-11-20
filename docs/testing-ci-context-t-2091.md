# CI 실패 문맥 전달 검증

t-1983에서 확인한 웹 E2E 실패는 로그 꼬리 32KiB 바깥에 Playwright 오류가 있어 분류 입력에 작업 수 요약만 남았다. 실패 로그를 읽는 동안 오류 표지와 이어지는 줄을 보존해, GitHub job의 단계명과 함께 분류 입력에 넣는다. 오류 표지는 웹 FAIL과 JS 예외, Rust panic과 컴파일 오류, Python traceback, GitHub 오류 annotation을 다룬다.

job마다 앞에서 찾은 문맥을 최대 네 구간, 구간당 12줄과 2KiB까지 보존한다. 기존 job당 32KiB와 사건당 128KiB에 문맥을 포함한다. 합계가 차면 문맥이 로그 꼬리에 앞서며, 들어갈 자리가 없는 구간은 생략하고 잘림을 표시한다. GitHub runner가 가린 로그에 aaa에 등록된 인증값 가림을 적용한다.

- `aaa-agent`의 `early_error_and_stack_survive_the_job_tail_cut_with_secrets_masked`는 작업 수 요약만 남는 32KiB 꼬리를 재현한다. 오류와 스택·단계명, 가림, 기존 실패 목록과 job 상한을 확인한다.
- `aaa-common`의 `failure_context_bounds_multibyte_lines_and_keeps_following_frames`는 한글 긴 줄을 자른 뒤에도 스택이 남고 초과 구간이 표시되는지 확인한다. `playwright_api_error_without_an_error_type_keeps_its_stack`는 API 메서드명으로 시작하는 오류를 확인한다. `failed_python_suite_keeps_the_wrapped_traceback`는 실패한 Python suite의 종료 코드 접두사를 벗기고 스택을 보존하는지 확인한다. 성공한 Python fixture의 실패 인용은 기존 표지와 새 문맥 양쪽에서 제외하는 단언을 함께 둔다.
- `aaa-host`의 `failed_ci_delivers_each_jobs_context_before_full_legacy_tails`는 네 옛 agent 형식의 로그가 128KiB를 먼저 채운 뒤 두 job의 오류 문맥을 함께 받는다. 실제 CI 판정과 자동화 저장 입력에서 두 문맥, 단계명, 실패 목록, 상한·잘림 표지를 확인한다.

GitHub CLI의 단계별 로그 줄 출력, Playwright의 GitHub 오류 보고, Actions runner의 출력 가림을 대조했다. 기존 CLI와 aaa 가림을 재사용했다. [GitHub CLI 소스](https://github.com/cli/cli/blob/trunk/pkg/cmd/run/view/view.go) · [GitHub CLI MIT](https://github.com/cli/cli/blob/trunk/LICENSE) · [Playwright GitHub reporter](https://playwright.dev/docs/test-reporters#github-reporter) · [Playwright API 오류 스택](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/client/channelOwner.ts) · [Playwright Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE) · [Actions runner 출력 가림](https://github.com/actions/runner/blob/main/docs/design/auth.md) · [Actions runner MIT](https://github.com/actions/runner/blob/main/LICENSE)
