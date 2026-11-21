# macOS Intel 설치 PTY 상한 진단

[e17a0bd4의 Intel CI](https://github.com/cLazyZombie/aaa/actions/runs/44832774284/job/136621593363)에서 설치 PTY 세 건이 60초 상한에 닿았다. 등록 창 닫힘 시험은 worker의 Stage Running까지, worker 수명·pin 충돌 시험은 최종 승인 입력의 에코까지 기록됐다. 당시 다음 단계와 실패 순간의 프로세스 상태를 가를 자료가 부족했다.

정규 `macos-15-intel` Rust test 단계가 `AAA_CI_MACOS_INSTALL_PTY_DIAGNOSTICS=1`을 준다. PTY의 60초 상한에서 실패를 판정하고 소유한 자식을 정리하기 전에 다음 내용을 기존 분할 로그에 남긴다.

- 평문 입력 API 반환과 승인 통과를 별도 표지로 남긴다. reader 미완료·EOF·읽기 오류와 마지막 수집 시각을 구분한다.
- 직접 띄운 CLI와 그 직계 자식의 부모 PID·UID를 확인하고 BSD 상태, 스레드 수, CPU 시간의 Mach tick과 환산 비율, 상주 메모리, 페이지인, 디스크 I/O 누적 바이트를 기록한다.
- 실행 기록에는 worker PID·내구 단계·종료 코드, 공개 사건 파일에는 순서·단계·상태·결과 코드만 남긴다. 로그 항목은 고정된 필드로 이루어져 승인 입력과 경로·인자·환경·등록 코드를 수집 대상에서 제외한다.
- 실패 시점의 러너 부하·메모리 페이지·시험 볼륨 여유와 PTY 수명 동안의 러너 CPU tick 차이를 기록한다. 시험 단계·분할·상한 및 실패 판정은 기존 계약을 따른다.

Rust 시험 artifact 업로드는 기존 `test.log`, `rust/`를 그대로 사용한다. 앱의 마지막 사건과 OS 프로세스 상태를 함께 비교할 수 있다. 호출 스택과 디스크 요청별 대기 시간은 기록 범위 밖이며, 이 변경에서 원래 Intel 정체의 원인과 회복 여부는 미확정이다.

## 개발 검증

macOS arm64에서 verify profile의 설치 분할 전수를 진단 옵션과 함께 실행했다. worker 4·libtest thread 1·Tokio worker thread 2에서 37개가 통과했고, 실제 LaunchAgent 등록 1개는 기존 CI 전용 ignored였다. worker 수명 시험은 켜진 진단에서 실제 읽기 반환·승인 통과 표지를 확인한다.

pin 충돌 시험의 로컬 임시 변형으로 실제 PTY와 기존 상한을 사용했다. 두 변형은 각 60초에 exit 101로 끝났다.

| 주입 | 로그 대조 |
| --- | --- |
| 마지막 승인 글자 뒤 줄 끝 보류 | 드라이버 2/2 쓰기, 마지막 승인 읽기 반환 미관측, 직계 자식 0, 실행·공개 사건 파일 없음 |
| SAS 표시 뒤 시험의 host 승인 보류 | 실제 승인 읽기 반환과 승인 통과, CLI 직계 자식 PID와 실행 record의 worker PID 일치, FilesApplied·Enroll Running, SAS 값 가림 |

두 변형 모두 reader 대기와 러너 부하·CPU·메모리·디스크 기록을 확인했다. 임시 변형은 원복했다.

## 비교한 방법

Apple libproc의 부모·작업 상태와 Mach 통계를 사용했다. psutil의 자식 CPU tick 해석, py-spy의 자식 계보 확인과 프로세스별 표본 경계를 대조했다. 외부 코드는 들여오지 않았다. [Apple libproc와 APSL](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c) · [psutil macOS 구현](https://github.com/giampaolo/psutil/blob/master/psutil/arch/osx/proc.c) · [psutil BSD 라이선스](https://github.com/giampaolo/psutil/blob/master/LICENSE) · [py-spy dump](https://github.com/benfred/py-spy/blob/master/src/dump.rs) · [py-spy MIT 라이선스](https://github.com/benfred/py-spy/blob/master/LICENSE)
