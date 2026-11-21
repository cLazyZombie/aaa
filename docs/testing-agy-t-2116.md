# Gemini worker와 Antigravity CLI 검증

## 담당 시험

```sh
cargo test --profile verify --locked -p aaa-agent -p aaa-config -p aaa-host -p aaa-proto --lib agy
cargo test --locked -p aaa-proto typescript
cargo test --locked -p aaa --test deps
```

agent 시험은 현재 클릭 가능한 입력창과 확인창, 첫 입력 접수, 루트와 하위 대화, 승인·크레딧 설정,
계정의 모델 목록과 버킷 파서를 본다. host 시험은 같은 구독을 쓰는 세 머신의 예약이 하나인지,
DB를 다시 열거나 무관한 pane이 끝나도 자리가 유지되는지, 구독 표본이 한 줄에 갱신되는지 검사한다.
표본에 리셋 시각이 없는 경우와 현재 모델에 해당하지 않는 버킷도 포함한다.

## macOS 실기

- 공식 agy 1.5.13을 PTY에서 실행했다. Starter Quota의 Gemini 3.8 Flash Low를 사용했다.
  새 대화에서 최초 PreInvocation이 ID가 있는 첫 상태줄보다 먼저 올 수 있었다. aaa가 띄운 worker에서 brief의 자동 제출과
  실제 파일 읽기를 확인했다. 두 대화 사이의 직접 복원과 `aaa thread restart --resume`에서 원래 대화 ID를 확인했다.
- 실제 승인창에서 `aaa report`를 한 번 승인하자 thread는 37%를 기록하고 입력 대기로 돌아왔다. 새 명령을 거절했을 때는
  진행률이 유지됐다. 동일한 작업 폴더의 쓰기 검사는 open부터 OS 오류 1을 반환했고 시스템 임시 폴더의 쓰기는 통과했다.
  별도 권한을 적용하지 않은 macOS sandbox에서 현재 pane의 임시 진행 값을 루트 hook이 회수했다.
- 루트와 읽기 전용 하위 agent가 각각 다른 대화 ID로 hook을 실행했다. 하위 agent의 파일 읽기가
  끝난 뒤에도 aaa가 표시한 대화 ID는 루트 것과 일치했다.
- `/usage`와 상태줄의 Gemini 주간 버킷 수치가 같은 것을 확인했다. 이 계정에서는 5시간 버킷이 관측되지 않았다.

## 세 머신 실기

- 같은 Starter Quota 계정으로 로그인한 세 macOS agent를 격리 host에 붙였다. 동시 시작 세 건 중 하나만 성공했다.
  나머지 두 요청은 구독 자리 대기였으며 추가 pane을 만들지 않았다. 열린 pane의 agent 연결만 끊었을 때도 다른 두 머신은
  계속 같은 자리를 기다렸다. agent가 다시 붙었을 때 원래 pane이 이어졌고, pane 종료를 확인한 뒤 다음 머신이 실행할 수 있었다.
- 세 머신에서 차례로 공식 상태줄을 받았다. Chromium의 실제 사용량 응답과 화면에서 매번 구독 한 행,
  `gemini-weekly` 한 행, `3p-weekly` 한 행을 확인했다. 표본 출처는 마지막에 실행한 머신으로 바뀌었다.
- 원격 머신에서 `aaa thread restart --resume`을 실행했다. 새 pane은 저장된 대화 ID로 돌아왔고 같은 brief를 다시 보내지 않았다.
