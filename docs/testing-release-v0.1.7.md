# v0.1.7 릴리스 준비 검증(t-1324)

## 범위

`Cargo.toml`과 lockfile의 workspace 10개 판을 0.1.6에서 0.1.7로 올렸다.
전후 TOML을 비교해 외부 의존성 및 판 외 필드가 그대로임을 확인했다.
[변경 요약과 배포 파일](releases/v0.1.7.md)은 기존 release workflow의 14개 산출물을 따른다.
제품 코드·Windows 전용 코드·release workflow는 변경하지 않았다.

## 관련 검사

- `python3 scripts/release.py check-tag --tag v0.1.7`: 성공.
- `nice -n 15 python3 -m unittest scripts.test_release`: 기존 계약 6개 성공.
  실제 CLI의 manifest·checksum·기호/launcher 결속·bootstrap 원문·판 불일치 거부를 확인한다.
- `bash scripts/check-public.sh`: 성공.
- 새로운 시험은 추가하지 않았다. 판 번호 변경은 기존 release 계약으로 확인한다.

## 착지 확인

[검증 규칙](verification.md)에 따라 착지 시작 main을 보존하고 최종 통합 SHA에서
전체 ci-local 및 Linux/Windows strict cross를 한 번 실행한다. Windows production은
최종 SHA salt로 한 번 빌드하고 같은 exe/PDB의 기호·SHA·실제 Windows 판/config를 확인한다.
실행 원문과 최종 결과는 작업 보고의 library에 보존한다. 첫 제품 검증 실패 시 재시도하지 않는다.
태그 전송·공개 게시는 main 반영과 대상 CI 확인 뒤 별도 사용자 승인 대상으로 남긴다.
