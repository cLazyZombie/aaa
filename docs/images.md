# host 계정으로 이미지 생성·편집

어느 harness의 worker thread나 프로젝트 coordinator에서도 `aaa image`를 쓴다.
현재 머신의 로그인 대신 host가 등록한 Codex 구독 계정으로 생성하고, 결과 파일만 현재 머신에 받는다.
로컬·원격 agent와 host 모두 `imagegen.v1`이 필요하다. 일반 thread의 Front 조작 권한은 그대로다.

## 준비와 실행

사람이 기존 [계정 관리](accounts.md)로 host의 Codex 기본 계정을 지정하거나 프로젝트의 공통 풀 참여를 켠다.
이 명령은 로그인·기본 계정·풀·profile·시스템 설정을 바꾸지 않는다.
host에 설치된 Codex는 정확히 0.163.0이어야 한다. 인증 전달 전에 그 판을 검사한다.

`--profile`에는 host에서 허용한, 모델을 명시한 Codex profile 이름을 준다.
예시의 `codex-image`는 그런 profile의 자리표시다. helper는 그 profile의 모델·effort만 사용하며,
추가 argv·환경·custom command·hook을 실행하지 않는다. profile 변경 뒤 초안 시작은 거부한다.

thread 안에서 설명을 파일에 쓰고 실행한다:

```sh
aaa image generate --profile codex-image --prompt-file prompt.txt \
  --output-dir .aaa/demo-t-0001/library

# 참고 이미지가 있으면 생성/편집 의도와 각 참고의 역할을 prompt.txt에 쓴다.
aaa image generate --profile codex-image --prompt-file edit.txt \
  --image reference.png --image style.png --transparent \
  --account work-codex --output-dir .aaa/demo-t-0001/library
```

설명은 32KB, 참고는 PNG·JPEG·WebP 5개/합계 50MiB까지다. 한 요청은 이미지 하나를 만들도록 지시한다.
투명 배경은 내장 도구에 alpha를 요청하는 지시이며 결과의 시각적 품질·정확한 크기까지 보장하지 않는다.
결과 파일 이름은 `<작업 ID>-<번호>.<형식>`이다. 길이·SHA-256을 확인한 뒤 저장한다.
같은 파일이면 재사용하고 다른 기존 파일·symlink를 덮어쓰지 않는다. 설명·인증은 명령 인자에 넣지 않는다.

## 계정과 사용량

- 풀 참여 프로젝트는 현재 허용한 Codex 계정·profile 모델·인증 세대·공유 그룹과 최신 창을 검사한다.
  `--account`는 그 풀의 특정 계정으로 후보를 좁힌다. 공유 별칭의 관측은 계속 제약에 포함한다.
- 풀 미참여 프로젝트는 host의 Codex 기본 계정만 사용한다. `--account`도 그 별칭이어야 한다.
  이 경로는 일반 기본 계정 thread와 같이 자동 잔량 선택을 하지 않으며 실제 제공자 한도가 적용된다.
  기본 미지정·인증 오류에 머신 로컬 로그인이나 다른 계정으로 넘어가지 않는다.
- 자동 풀에서는 이미지 한 건에 75%p를 예약한다. 일반 medium 예상량 15%p의 다섯 배다.
  [공식 이미지 안내](https://learn.chatgpt.com/docs/image-generation)의 평균 3~5배 소비 설명을 바탕으로
  보수적으로 잡은 예상량이며 실제 소비 상한·과금액이 아니다. 기존 thread와 같은 SQLite transaction에서
  공유 한도 예약을 합쳐 계산하고 안전 여유 10%p를 유지한다.
- 새 관측이 없는 계정의 첫 실행 규칙도 기존 풀과 공유한다. 이미지와 일반 thread가 각각 첫 실행을 만들지 않는다.
  실행 종료 뒤의 신선한 사용량 관측이 있어야 예약을 걷는다. 결과 불명은 예약을 보존한다.
  생성 전의 확정 실패(지원하지 않는 CLI 등)는 예약을 해제한다.
- 내장 이미지 사용량은 기존 계정별 구독 사용량 조회에 포함된다. 작업 상태의 `total_tokens`는 Codex가
  알린 합계일 때만 보이며, 이미지 토큰 비용·남은 %·요금으로 환산하지 않는다.
- host에서 동시에 4건, thread/coordinator 소유자마다 미완료 4건, 파일을 보관하는 작업 256건까지다.
  입력·결과 파일을 받았으면 `forget`으로 host 사본을 정리할 수 있다.

API key·종량 과금으로 자동 전환하지 않는다. [공식 인증 안내](https://learn.chatgpt.com/docs/auth)의
구독 로그인과 API key 로그인을 구분하고, helper에는 구독 인증만 전달한다.

## 응답 유실·대기·정리

생성 전에 작업 ID를 stderr에 출력한다. `--id <ID>`로 같은 설명·profile·참고 SHA를 다시 보내도
이미 보낸 생성은 반복하지 않는다. 다른 내용을 같은 ID에 보내면 충돌이다.
host 재시작과 같은 thread의 새 pane에서도 원장과 소유권을 유지한다. worker는 다른 thread의 작업을 읽지 못한다.
프로젝트 coordinator는 소속 프로젝트의 목록·상태·결과를 읽고 끝난 파일을 정리할 수 있다.
다른 worker의 입력 전송·생성 시작 권한은 주지 않는다. 원래 thread가 끝난 뒤의 host 파일 정리도 이 경로를 쓴다.

```sh
aaa image list
aaa image status <ID>
aaa image fetch <ID> --output-dir .aaa/demo-t-0001/library
# 사용량·동시 실행 대기만 다시 판정한다. 실패/불명/완료 작업을 재생성하지 않는다.
aaa image start <ID> --output-dir .aaa/demo-t-0001/library
# 초안·대기·완료·확정 실패의 host 입력/출력 파일을 정리한다.
aaa image forget <ID>
```

`waiting`은 계정 배정 대기이며 자동 재시작하지 않는다. `queued`·`running`은 상태 사건을 기다린다.
CLI가 끊겨도 host 작업은 이어진다. `failed`는 확정 실패, `unknown`은 송신 뒤 응답 유실·host 중단 등으로
결과를 모르는 경우다. 둘 다 같은 ID로 재생성하지 않는다. 새 생성은 새 ID를 명시적으로 요청한다.
생성 상한은 10분이며 시한 초과도 불명으로 보존한다. `unknown` 작업은 파일 정리·예약 해제를 자동으로 하지 않는다.

`forget` 뒤에도 중복 방지 영수증과 필요한 사용량 예약은 보존한다. 현재 머신에 이미 받은 파일은 남는다.
helper가 끝나면 자식 종료를 기다린 뒤 인증·임시 홈을 지운다. host 강제 종료 뒤에는 재시작 때
해당 원장 소유의 잔여 인증 파일만 지운다. 남은 결과 파일과 private 임시 홈 전체를 무조건 지우거나
이전 자식을 추측해 종료하지 않는다.

이 경로는 `aaa image` 요청을 중계한다. harness가 따로 실행한 로컬 Codex나 기존 내장 도구 호출을 가로채지는 않는다.
