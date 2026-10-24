# 질문 카드에 시안 넣기

thread는 `report.md`의 `## Questions` 안에 질문을 쓴다. coordinator가 직접 물을 때도 post 본문에 같은 문법을 쓴다.
각 질문의 본문에 판단할 그림·영상·표·근거를 함께 둔다. `## Report`에만 자료를 두면 질문 카드에서 바로 비교할 수 없다.

```markdown
## Questions
### 1. 이 시안으로 진행할까?
그림은 정지 화면이며 영상은 같은 장면의 8초 동작이다.

![시안](library/preview/a.png)
![시안 영상](library/preview/a.mp4)

[전·후 비교 보고서](library/preview/index.html#A)

- A 진행한다 (추천) — 영상에서 확인한 동작을 적용한다
- B 고친다 — 고칠 점을 글로 적는다
```

영상은 카드·보고 본문 안 재생 단추를 한 번 누르면 그 자리에서 재생된다. `mp4`·`webm`의 `![]()` 문법을 권장한다.
기존 `[시안 영상](library/preview/a.mp4)` 링크도 본문 재생기로 표시한다. 재생기는 controls·muted·playsinline이며,
멈춤·탐색·음소거 해제를 제공한다. 선택지 아래에 넣은 영상도 재생만으로 답이 선택되지 않는다.
HTML 링크의 `#A` 같은 장면 위치는 카드의 미리보기와 새 탭에서도 보존된다.

Safari 호환성을 위해 H.264 MP4(yuv420p·faststart)를 우선한다. 짧은 시안은 5~10초·10 MiB 이하를 목표로 하고,
긴 자료는 장면별로 나누어 설명·길이·용량을 적는다. 이는 작성 권장 기준이며 실제 회수 상한은 별도다.
기본 자동 재생·반복은 끈다. 소리 없는 반복 시안이 필요하면 [HTML 보고서 지침](library.md)의 정지 단추·동작 줄이기 규칙을 따른다.

thread는 파일을 library 안에 먼저 두고 보고를 마지막에 쓴다. coordinator는 `aaa library publish <폴더> --name preview`로
자료를 올린 뒤 `library/preview/…`를 쓴다. 특정 판을 고르게 할 때는 발행한 r-ID와 `library/r-NNNN/…`를 쓴다.
로컬 절대 경로·앱 API·외부 그림/영상 주소는 카드 자료로 쓰지 않는다. HTML은 origin 없는 sandbox에서 자기 파일만 읽는다.

## coordinator auto mode

coordinator 머리의 ⋯ 메뉴에서 `auto mode 켜기`를 누른다. CLI는 `aaa project auto-mode on|off`이며,
값 없이 부르면 상태를 읽는다. 모드 변경은 사람의 웹 조작 기기·세션 밖 CLI만 가능하다.
프로젝트별 host 상태라 host·coordinator 재시작에도 유지되고 사용자가 끌 때까지 계속된다.
입력창의 AUTO 스위치, 상태줄의 「눌러서 끄기」, 머리 메뉴에서 바로 끈다. 끌 때 이번 실행의 기간과 결정 수를 한 번 표시한다.

해당 프로젝트의 검증된 coordinator는 다음 명령으로 일반 질문에 자동 답을 남긴다.

```sh
aaa question answer q-1000 B --auto --reason-file reason.txt
```

까닭은 비어 있지 않은 평문(16 KiB 이하)이다. host는 추천·원문·까닭·고른 답·시각·`coordinator(auto)`와
전달 상태를 보존한다. 질문 묶음의 일부만 자동 답하면 남은 질문은 계속 카드에 있다.
`pending`은 전달 확인 필요, `failed`는 오류 원문을 확인해야 하는 상태다. 둘 다 자동으로 다시 보내지 않는다.
일반 질문 답은 되돌릴 수 있는지 아직 알 수 없다고 표시한다. host는 후속 실행의 가역성을 추측하지 않는다.

coordinator가 스스로 정한 일은 다음 형식의 JSON 파일을 `--body-file`로 준다.

```json
{"question":"다음 구현 순서","answer":"입력창부터","reason":"사용자 상태를 먼저 표시","action":"구현 순서를 정함","reversible":true}
```

```sh
aaa post user --data --topic auto-decision --title '구현 순서' --body-file decision.json
```

「답할 것」의 「자동으로 정한 것」에서 까닭을 확인하고 「괜찮음」으로 확인 처리하거나 다른 답을 보낸다.
새 답은 coordinator inbox에 전달하며 원래 선택과 실행 결과는 그대로 남는다. 이미 한 일을 host가 되돌리지는 않는다.
목록은 최근 기록부터 읽는다. 「이전 자동 결정 더 보기」로 더 읽으며 오래된 질문 주소도 해당 기록을 바로 연다.
자동 결정 원장과 그때의 질문 자료는 일반 「답한 것」의 14일 정리와 별개로 보존한다.
자동 결정은 외부 푸시를 보내지 않는다. 사용자 전용 예외 질문과 전체 완료 알림은 기존 규칙을 따른다.

사용자가 직접 정할 질문 제목에는 `[사람]`을 붙인다. host는 확인창 및 `[사람]`·`[사용자 전용]`·`[비밀값]`·
`[암호]`·`[확인창]` 표지의 질문에 자동 답을 거부한다. 실제 암호·토큰은 질문이나 파일에 쓰지 않고 전용 가림 입력칸에서 받는다.
로그인·GUI·실기 확인·청취, 되돌릴 수 없는 삭제, force push·기록 수정·검사 우회, safety·profile·권한·시스템 설정·
자동화 승인, 결제·구독, 외부 게시·메일, host 재시작·사용자 서비스 설치도 늘 사용자에게 묻는다.
모드는 harness 권한을 변경하거나 사용자 전용 배포·병합 승인을 대신하지 않는다.
