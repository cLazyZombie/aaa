# 실행 중인 thread 모델 변경 검증

E2E 146은 실험실의 실행 한도를 1로 두고 작업 폴더에서 예약 thread를 띄운다. 실제 worker와 coordinator pane의 CLI를 각각 호출한다.

- worker의 직접 요청, 근거 없는 coordinator 요청, 종료 동의가 빠진 실행 중 변경, 다른 harness의 대화 잇기, 세션 ID를 모르는 상태의 대화 잇기를 거부한다. 원래 pane·thread 상태·할 일 모델을 함께 확인한다.
- 다른 harness는 기존 pane의 종료가 확인된 뒤 새 thread를 띄운다. 이전 작업 파일을 읽을 수 있고 할 일과 실행 중인 예약이 새 thread와 모델을 가리킨다.
- 같은 harness의 모델 변경은 같은 thread·작업 폴더를 쓰고 새 pane에 기존 대화 ID와 새 모델 인자를 넘긴다.
- 두 변경의 전후 모델과 인용한 근거를 할 일 기록에서 확인한다. Chromium과 WebKit에서 이전 thread의 변경 기록을 열고 새 thread 링크를 따라 다음 변경 기록까지 확인한다.

`aaa verify -- nice -n 15 bash scripts/ci-local.sh --only web,e2e,public --spec 146`으로 실행한다. 첫 실험실에서는 작업 폴더 자동 배정이 꺼져 예약이 대기했다. host의 예약 사유를 확인하고 시험 머신의 작업 폴더 자동 배정을 준비 단계에서 허용했다.
