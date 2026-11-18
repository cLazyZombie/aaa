# 완료 체크 뒤 할 일 확인 카드(t-2000)

「답할 것」의 할 일 카드는 활성 할 일 목록의 `attention`에서 나온다. inbox의 `task needs check`는 별도 알림이다.
완료 체크와 ticker의 완료 보관 사이에 목록을 읽으면 완료된 말단 할 일에도 끝난 thread의 보고 표지로 `attention`을 붙였다.
이제 완료 체크한 말단은 보관 전에도 확인 카드가 없다. 같은 tick의 보관 순회 직후에 들어온 완료 체크도 확인 알림 생성을 막는다.
화면을 열거나 그 화면으로 돌아올 때 목록을 다시 읽어 빠진 갱신 사건을 회복한다.

- `aaa-host`의 `task_attention_clears_on_explicit_completion_before_archiving`은 `[결정 필요]` 보고에서 완료 체크를 종료 앞뒤로 각각 수행한다. 완료 응답·갱신 사건·보관 전 목록·답한 기록·실제 보관을 검사한다. 종료가 뒤라면 해당 tick의 보관 순회가 끝난 뒤 완료 체크가 들어온 경우에도 확인 알림을 생성하지 않는지 본다.
- `aaa-host`의 기존 `task_attention_requires_a_fresh_block_and_preserves_children_and_followups`는 하위 할 일과 미처리 후속을 보존하는 경계를 검사한다.
- 웹 E2E 128은 Chromium과 WebKit에서 실제 CLI 완료의 할 일 갱신 사건만 가로채고 기존 카드가 남는 것을 확인한다. 화면에 돌아오면 새로 읽은 목록으로 카드가 닫히고 답한 기록이 보인다. 기존 데스크톱·휴대폰 완료·다시 맡기기·연결만 풀기 검증도 함께 돈다.

```sh
aaa verify -- nice -n 15 python3 scripts/verify.py develop --base <기준-커밋> --package aaa-host
aaa verify -- nice -n 15 bash scripts/ci-local.sh --only web,e2e,public --spec 128
```
