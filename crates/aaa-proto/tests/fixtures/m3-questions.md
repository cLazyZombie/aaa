## Questions
1. **M3 순서**: macOS 감독자(install I3)가 없을 때 M3를 어떻게 할까요?
   - **A(추천)** install I3이 main에 올 때까지 M3 착지를 미룬다. 지금은 binary 전송·검증과 host 쪽 흐름(제안 → 웹 확인 → 서명 → bootstrap → 적용 → 등록)만 만들어 둔다. I3이 오면 `--service launchd`로 끝까지 맞추고 착지한다.
   - B 지금 `--service none`으로 착지한다. 파일과 등록까지 하고, agent 시작은 「필요한 일: install I3 뒤 다시 설치」 카드로 남긴다(반쪽 기능이 main에 들어간다).
   - C install 순서를 바꿔 macOS(I3)를 Windows 최소 bootstrap보다 앞당기도록 t-0485에 요청한다.
2. **M3 끝까지 시험할 곳**: 실제 install SSH 입구는 root sshd가 있어야 열린다. 어디서 끝까지 볼까요?
   - **A(추천)** 이 Mac에서는 host 흐름을 가짜 `ssh` 대본으로 시험한다. 실제 root sshd 경로는 GitHub CI(macos-15·ubuntu-24.04)에서 sudo로 띄운 sshd로 시험한다. 이 Mac의 시스템 설정은 바꾸지 않는다.
   - B 이 Mac의 「원격 로그인」을 사용자가 직접 켜고, loopback 자기 자신으로 끝까지 시험한다.
