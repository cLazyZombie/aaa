# E72 설치 계획 검사의 경계 수리(t-1238)

## 유래와 원인

첫 착지 후보 `0ac1d63c`의 E72는 `machine-setup`의 `data-phase=failed`를 기다리다
기존 30초 상한에서 실패했다. 보존 DB의 계획 접수와 Failed 저장은 각각
2026-10-28 07:22:28.177 UTC와 07:22:58.576 UTC로, 간격은 30.399초였다.
최종 오류는 의도한 「시스템 SSH 서버로 확인하지 못했다」였다. 작업이 계속 Planning에 머물다
정상적인 거부·정리를 끝내는 시간이 화면 대기와 겹쳤다.

기준 main `e321baab`과 자동화 후보 `bb1da9c7`에서 E72, MachineSetupPanel,
machine_setup/install.rs, machine_setup/binary.rs의 blob은 각각 같다.
이 실험실에는 자동화 규칙이 없으며 새 자동화 입력 경로는 이 작업을 실행하지 않는다.
단독 계측에서도 두 판의 계획 단계는 모두 약 2.9초였고 실제 install의 거부 판단은 약 19ms였다.
유래는 main의 기존 시험 결합 구조로 분류한다.

기존 화면 단계에는 큰 host 실행 파일의 SSH 전송·원격 SHA/판 확인·실제 install 실행·정리까지
들어 있었다. 이 원격 흐름은 별도의 전송·계획 상한을 갖지만 화면은 그 전체를 locator의
30초 안에 마치기를 기대했다. 원본 실행에서 전송·첫 실행·정리 중 정확히 어느 하위 단계가
느렸는지는 당시 상세 계측이 없어 확정하지 않는다. 높은 load만으로 특정 하위 원인을 단정하지 않는다.

## 수리 전 재현

실험실 sshd의 ForceCommand에 관측용 실행기를 두고 원래 SSH 명령과 stdin/stdout은 그대로
전달했다. 업로드 명령만 FIFO에서 보류하고, 원래 화면 대기가 실패한 사건에서 FIFO를 풀었다.
고정 sleep이나 추가 CPU 부하는 쓰지 않았다. 브라우저·SSH·실제 install은 모두 실제 코드다.

| 코드 | 전송 보류 중 | 보류를 푼 뒤 |
| --- | --- | --- |
| 수리 전 main e321baab | 기존 locator 30000ms timeout, exit 1 | 실제 install exit 3, 화면 Failed |
| 수리 전 후보 bb1da9c7 | 같은 timeout, exit 1 | 실제 install exit 3, 화면 Failed |
| 수리한 E72 | 통과, 큰 파일 전송을 호출하지 않음 | 실제 SSH 거부 검사는 별도로 exit 3 확인 |

이 주입은 원본 실행에서 실제 FIFO 정체가 있었다는 뜻이 아니다. 진행 중인 전송과
화면 대기가 결합된 같은 실패 경계를 두 판에서 재현하고 수리 뒤 분리됐음을 확인한다.
각 RED의 원문과 SSH 시작/끝·HTTP 응답·DOM phase 시각은 작업 보고의 library에 보존한다.

## 유지한 계약과 위치

| 계약 | 검사 위치 |
| --- | --- |
| 실제 비특권 sshd의 접속·host key 믿음·OS 감지 | E72 기존 앞 단계 |
| 실제 install-probe가 시스템 SSH 출처가 아니면 exit 3, 출력·상태 쓰기 없음 | E72 새 「실제 install 계획: 비특권 sshd를 거부하고 상태를 쓰지 않는다」 |
| 기본 이름·작업 root·잘못된 이름 거부·설정 전송·거부 까닭 표시 | E72 새 「설치 설정 → 계획 보기: 설정 전송과 SSH 거부 까닭 표시」 |
| 실제 SSH로 파일 전체 전송·0700 폴더·SHA/판 대조·불일치 정리 | host `the_binary_reaches_a_private_remote_temp_dir_and_a_mismatch_removes_it` |
| 실제 host 계획 흐름에서 원격 exit 3을 오류로 표시하고 임시 폴더·계획을 남기지 않음 | host `a_remote_install_that_refuses_this_ssh_fails_with_the_reason_and_leaves_nothing` |
| 계획·승인·등록, 보기 기기·다른 hash 거부 | host `a_detected_machine_is_planned_signed_bootstrapped_installed_and_registered` |

이전 E72 「설치 설정 → 계획 보기: 실제 install은 비특권 sshd를 믿지 않는다(까닭, 원격 임시 폴더 없음)」를
위 두 화면/실물 단계로 대체한다. 큰 binary를 매 화면 검사마다 복사하는 부분과 전역 임시 폴더의 크기 검색을
그 단계에서 제거하며, 전송·정리 단언은 위 backend keeper에 남긴다.
실제 SSH 출처 거부는 같은 loopback sshd에서 현재 binary를 직접 실행하여 확인한다.
화면 단계만 실제 준비된 작업의 응답 자료를 바탕으로 계획 응답을 제공한다. 보내는 job·이름·host·작업 root와
한 번만 보내는 것을 검사한다. 제품의 설치·권한 판정 코드는 바꾸지 않는다.

기존 Windows 가림 입력 단계는 Chromium/WebKit × 1280/390의 네 독립 사례를 한 단계에서 실행해
7.445초였다. 같은 취소·일회 전달·거부·페이지 종료·영속 기록 누출 단언을 네 이름으로 나눴다.
각 사례마다 새 가짜 암호를 쓰고 기록을 검사하며, 단언이나 제품 Windows 경로는 바꾸지 않았다.

locator 30초, SSH/설치 상한, 시스템 SSH 신뢰 정책은 바꾸지 않았다. 제품 코드·API·E2E 번호도 바꾸지 않았다.
원격 전송은 host, 실제 신뢰 판정은 install, 화면은 폼과 응답 표시가 각각 소유한다.

## 확인

- 기존 host machine_setup 시험 26건 통과(전체 2.19초). 전송·거부·정리·서명·등록 keeper를 포함한다.
- 웹 유닛 651건 통과.
- 전송 보류 조건의 수리판 E72 통과: 실제 SSH 거부 310ms, 화면 설정/거부 표시 193ms.
  앞의 실제 SSH·보기 기기·휴대폰과 기존 Windows 가림 입력 검사도 유지한다.
- 최종 E72 통과: 실제 SSH 거부 197ms, 화면 설정/거부 표시 149ms.
  Windows 네 사례는 749/2365/1405/2446ms로 각각 5초 아래다. E72 파일 전체는 13.9초다.
- 최종 웹 유닛 651건 통과. 공개 검사 결과는 작업 보고에 보존한다.
- 착지 전체 CI는 새 착지 알림을 받은 뒤 저부하 머신 전용 worktree의 최신 main에서 수행한다.

관측은 [Playwright response 사건](https://playwright.dev/docs/api/class-page#page-event-response),
화면 응답 경계는 [Playwright network routing](https://playwright.dev/docs/network)과 저장소의
machine-password fixture 방식을 참고했다. Playwright는 [Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)이며
외부 코드를 복사하거나 새 의존성을 추가하지 않았다.
