# 아이디어 진행 모델 검증(t-1581)

[plan §8.559](plan.md#8559-아이디어-진행-모델-고르기보기t-1581)의 고르기·보기·다음 말에 바꾸기를 확인한 기록이다. 정한 시간을 자지 않고 host 기록·PTY 출력·DOM 표지로 끝낸다.

## 새 시험과 까닭

| 시험 | 꼭 필요한 까닭 |
| --- | --- |
| host `idea_thread::tests::switches_only_when_the_chosen_model_differs` | 고른 모델이 다를 때만 바꾸고, 고르지 않은 아이디어는 프로젝트 기본이 바뀌어도 잇는 경계. 새 thread가 새 대화임을 받는 글 |
| host `idea_thread::tests::only_people_choose_the_idea_model` | 모델 변경 권한 경계(thread 세션·보기 기기 거절)와 고르지 않았을 때 보이는 모델(기본, 열린 thread가 있으면 그 profile) |
| 웹 `test/ideas.test.mjs` 「진행 모델 칩」·「진행 모델 목록」 | 바꾼 뒤 다음 말 전까지 앞 모델 표시, 끝난 thread·옛 host 처리, host 머신 profile만 고르기 |
| 웹 E2E 131 `131-idea-model.mjs` | 실제 host·agent·PTY에서 칩 목록(허용 안 된 profile 제외)·고르기만으로는 thread 유지·다음 말에 앞 thread 끝냄(알림함 없음)과 새 profile thread의 brief·말 전달·거절 |

## E2E에서 찾아 고친 것

- 모델을 고른 바로 뒤의 말이 앞 모델 thread에 갔다. 앞 말(「준비」)을 보내던 배경 배달 일이 아직 돌다가 새로 쌓인 말을 그대로
  앞 thread에 보냈고, 모델을 바꿀 배경 일은 그 뒤에 줄 서 있었다(host 기록: 17바이트 입력이 앞 pane에 간 뒤 「앞 thread를
  끝냈다」). 배달 일이 고른 모델과 thread profile이 다르면 멈추게 했다. 고를 때 아직 보내지 못한 말이 있으면 바로 바꾸고 보낸다.

## 결과(11-07, 로컬 Mac 하나)

- `verify.py develop --package aaa-host --package aaa-proto --spec 120,130,131`: host 864·proto 54 시험, strict clippy, 웹 타입·번들·유닛,
  E2E 120·130·131(131은 5.5초), 공개 검사 통과. 위 수리 전 첫 실행은 131 「다음 말」 단계에서 실패했다.
- `cargo clippy --profile verify -p aaa --all-targets -- -D warnings`·`cargo test --profile verify -p aaa --bin aaa`(103): 통과.
- 고르지 않은 아이디어의 표시 모델 수리 뒤 3회차: host 시험에서 `safe_mode::tests::reload_honors_ephemeral_port_changed_address_and_disabled_web`이
  한 번 실패했다(웹 끔 경우에 닫은 옛 주소로 연결이 됐다: 병렬 시험이 풀린 임시 port를 다시 잡은 것으로 본다). 이 변경은 safe_mode·web을
  고치지 않았고, 단독 실행은 이 branch와 origin/main 모두 통과했다. 같은 실행의 나머지 863개는 통과했고, 건너뛴 clippy와
  웹·E2E 120·130·131·공개 검사는 따로 돌렸다.
