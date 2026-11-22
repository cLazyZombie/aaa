# macOS Intel 웹 E2E 업로드 실패와 DNS 관측

main `01c5a088`의 macOS Intel 1/3에서 E2E 15개가 통과한 뒤 결과 업로드의 `CreateArtifact`가 `ENOTFOUND`로 실패했다. 앞선 두 job에서도 E2E 15개와 16개가 각각 통과한 뒤 같은 종류의 업로드 오류가 관측됐다. 마지막 job의 원문에는 요청 호스트와 `getaddrinfo` 상세가 없다. 세 사건의 공통 원인은 확인되지 않았다. [첫 job](https://github.com/cLazyZombie/aaa/actions/runs/44240774133/job/134587851354) · [둘째 job](https://github.com/cLazyZombie/aaa/actions/runs/44650035030/job/136002341458) · [마지막 job](https://github.com/cLazyZombie/aaa/actions/runs/44970807181/job/137049946573)

## CI에서 보이는 결과

Checks 목록의 이름은 `web-e2e + artifact`이고, 각 shard는 Actions 요약에 웹 E2E와 결과 업로드의 상태를 따로 적는다. 요약은 단계 로그에도 남는다. 업로드만 실패하면 E2E는 통과, 업로드는 실패이며 job은 실패로 끝난다. E2E가 실패하면 기존 결과 업로드와 E2E 실패 로그를 실행한다. 선행 단계가 실패해 E2E가 시작되지 않으면 요약에 두 단계가 실행되지 않았다고 적는다.

고정된 `actions/upload-artifact` v7이 Twirp 요청 오류를 다시 포장하기 전에 Node HTTP 진단 채널에서 작업명, 공개 대상 호스트, 해석 오류의 공개 호스트, 코드, 시스템 호출, OS 오류 번호를 읽는다. 비공개 호스트는 가린다. 헤더·토큰·본문·오류 메시지·URL 경로와 쿼리는 진단 출력에서 제외한다. 업로드 단계에만 Node preload를 적용한다. [고정 업로드 액션](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/dist/upload/index.js) · [Node HTTP 진단 채널](https://nodejs.org/download/release/v24.22.3/docs/api/diagnostics_channel.html#http)

업로드 직후 같은 러너에서 shard·OS·아키텍처·이미지 버전·프록시 설정 유무와 네트워크 관측을 남긴다. 공개 결과 서비스 호스트와 대조용 `github.com`에 각각 한 번 무인증 HTTPS HEAD를 보내 DNS 주소 계열 또는 오류, TCP·TLS 연결, HTTP 응답 코드를 기록한다. 실제 업로드 오류에는 요청 당시의 값이 남고, HEAD 결과는 후속 직접 연결의 표본이다. 루트의 HTTP 404도 DNS·TCP·TLS를 거쳐 HTTP 응답을 받은 경우다. [GitHub 호스팅 러너의 통신 대상](https://docs.github.com/en/actions/reference/runners/github-hosted-runners#communication-requirements-for-github-hosted-runners)

## 개발 검증

- macOS에서 공식 Node 24.22.3 x64로 `scripts.test_ci_local`을 실행했다. 실제 Node HTTPS 클라이언트의 lookup에 `ENOTFOUND`를 주입해 원래 종료 코드, 진단 행, 비공개 호스트 가림과 토큰·서명 쿼리 비노출을 확인했다.
- CI YAML에서 실제 Bash 요약을 꺼내 E2E 통과·업로드 실패를 포함한 여섯 경우와 단계 실행 조건을 대조했다.
- 같은 Node에서 공개 결과 서비스와 `github.com`의 DNS·TCP·TLS를 관측했다. 두 호스트 모두 IPv4를 반환했고 HTTP 상태는 각각 404와 200이었다.
- actionlint 1.7.12로 작업을 점검했다. Rust 실패 요약의 기존 ShellCheck `SC2013` 한 건은 기준과 변경본 양쪽에서 같았다. 새 경고는 없었다.
- 실제 GitHub Intel 러너에서의 확인은 다음 정규 CI에서 한다. 개발 검증에서는 원격 branch나 재실행을 만들지 않았다.

진단을 위해 업로드 구현을 교체하는 방법, Runner의 단계 환경 전달, Node HTTP 진단 채널을 비교했다. 기존 GitHub 업로드 액션과 Runner 환경 전달을 이용하고 Node에서 원래 오류를 관측한다. 세 구현의 라이선스는 각각 MIT이며 이번 변경에는 외부 소스를 복사하지 않았다. [upload-artifact 라이선스](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/LICENSE) · [Runner 라이선스](https://github.com/actions/runner/blob/main/LICENSE) · [Node 라이선스](https://github.com/nodejs/node/blob/v24.22.3/LICENSE)
