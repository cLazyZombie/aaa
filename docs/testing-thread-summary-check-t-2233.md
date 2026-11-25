# 작업 요약 파일의 로컬 검사

`aaa thread summary --check-file <파일>`은 시작·메시지의 `--summary-file`과 같은 형식 검사기를 쓴다.
정상 파일은 종료 코드 0을 돌려주고, 형식이 틀리면 실패한 줄과 고칠 이유를 출력하고 종료 코드 1을 돌려준다.

`cargo test --profile verify --locked -p aaa --test thread_summary`는 설정·상태 폴더가 없는 곳에서 실제 CLI를 실행한다.
정상 파일, 한 절의 넷째 항목, CRLF 빈 줄 뒤의 `## 목표`, `--file`·thread ID와의 충돌을 확인한다.
검사 뒤에도 입력 파일의 바이트가 같고 설정·상태 폴더가 생기지 않는지 확인한다.

`cargo test --profile verify --locked -p aaa-host --lib summary::tests::`로 공통 검사기와 기존 요약 처리의 회귀를 확인한다.
