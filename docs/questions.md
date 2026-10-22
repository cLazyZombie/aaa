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
