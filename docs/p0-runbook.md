# P0 절차서: 서명 identity와 개발자 도구 gate

기획서 [§10.6](plan.md#106-p0-실험)의 P0 실험을 사람이 실제로 돌리는 순서다. 결정 9(자체 서명 1순위, Apple Development 2순위)와 결정 10(GUI 작업은 mac-a, 명령만이면 mac-c ssh)을 따른다.

문서의 머신 이름은 일반 이름이다. mac-a는 화면 접속이 되는 Mac, mac-b는 coordinator Mac, mac-c는 구현 Mac이다.

- 도구: `p0/`(빌드·서명·설치·측정·업그레이드 모의), `aa doctor probe`, `aa service`
- 결과 위치: 실행한 Mac의 `~/aa-p0/results/`
- 판정은 도구가 한다. 첫 실행 추가 지연 median ≤10ms, p95 ≤20ms, 100ms 초과 0, 키체인 실제 항목 읽기 `ok`, 전체 디스크 접근 `ok`, 외장 볼륨 읽기·쓰기 성공.

## 측정 도구가 믿을 만한지(이미 확인, mac-c)

| 조건 | 첫 실행 추가 지연 median | 책임 프로세스 | 전체 디스크 접근 |
|---|---:|---|---|
| 지금 pane(WezTerm 계보, 기준선) | 0.868ms | WezTerm | ok |
| 음성 대조 `--disclaim`(미등록 aa가 자기 책임자) | 223.406ms | aa | denied |
| 미등록 adhoc 앱을 LaunchServices로 실행 | 222.686ms(warm 5.695ms, 약 3배) | aa-adhoc | denied |

느린 계보와 FDA 없음을 각각 잡아낸다. 이전 실험(+221~236ms, GUI 앱 warm 약 3배)도 재현했다. LaunchServices 앱의 QoS는 user_interactive, nice 0이라 warm 저하는 QoS·nice로 설명되지 않는다(E2 조사 항목).

## 어느 Mac에서 무엇을 하나

| 작업 | Mac | 화면 필요 | 이유 |
|---|---|---|---|
| A. 자체 서명 인증서 만들기 | mac-a 터미널(ssh도 가능) | 아니요 | 파일만 만든다(키체인 변경 없음) |
| B. mac-a 키체인에 넣기 | mac-a **GUI 터미널** | 신뢰 설정이 필요할 때만 | E1을 mac-a에서 한다 |
| C. E1·E2 진행(`p0/e1.sh`) | mac-a **GUI 터미널** | 예(시스템 설정 등록) | 개발자 도구·전체 디스크 접근 등록은 시스템 설정 화면에서만 된다 |
| D. 다른 Mac에 인증서 넣기 | mac-c·mac-b, ssh | 아니요(예상) | 나중에 운영 설치 때. P0에는 필요 없다 |
| E5. 재부팅 | 재부팅해도 되는 별도 Mac | 예 | 지금 쓰는 세 Mac은 재부팅하지 않는다 |

mac-c은 지금 화면에 원격 접속할 수 없으므로 시스템 설정 등록이 필요한 E1은 mac-c에서 할 수 없다.

## A. 자체 서명 인증서 만들기 (mac-a, 한 번)

```bash
# mac-a 터미널. 저장소는 thread가 ~/aa-dev/aa에 준비한다.
bash ~/aa-dev/aa/p0/cert/make-selfsigned.sh          # ~/aa-codesign/ 에 만든다
```

- p12 암호를 두 번 묻는다. 이 암호는 사용자가 보관한다(스크립트·보고서에 남기지 않는다).
- 결과: `~/aa-codesign/aa-codesign.p12`(개인 키), `cert.pem`, `fingerprint.txt`(10년 유효).
- GUI 창: 없다. 키체인을 바꾸지 않는다.

## B. mac-a 키체인에 넣기

```bash
# mac-a GUI 터미널(키체인이 이미 풀려 있다)
bash ~/aa-dev/aa/p0/cert/import-identity.sh ~/aa-codesign/aa-codesign.p12
```

스크립트가 하는 일과 확인 창:

1. `security import … -T /usr/bin/codesign -T /usr/bin/security`: p12 암호를 터미널에서 묻는다. GUI 창은 없다.
2. `security set-key-partition-list -S apple-tool:,apple:,codesign: …`: login 키체인 비밀번호를 터미널에서 묻는다. 이 단계가 없으면 codesign을 처음 쓸 때 「codesign이 키에 접근하려고 함」 GUI 창이 뜬다.
3. 임시 파일에 `codesign --sign "aa Code Signing"`을 시험하고, `security find-identity -p codesigning`을 보인다.

**신뢰 설정(시험이 실패할 때만).** 자체 서명 인증서는 신뢰 설정 없이도 서명되는지 아직 확인하지 못했다. 서명 시험이 실패하면 mac-a 화면에서 아래처럼 한다.

- 키체인 접근 앱 → 로그인 → 인증서 → 「aa Code Signing」 더블클릭 → 신뢰 → 「코드 서명」을 「항상 신뢰」로 바꾼다. 닫을 때 비밀번호(또는 Touch ID) 창이 뜬다.
- 명령으로 하면 `security add-trusted-cert -r trustRoot -p codeSign -k ~/Library/Keychains/login.keychain-db ~/aa-codesign/cert.pem`이다. 이 명령도 **GUI 인증 창**을 띄운다. 그래서 화면이 없는 ssh에서는 실패한다.

## C. E1·E2 진행 (mac-a GUI 터미널)

```bash
bash ~/aa-dev/aa/p0/e1.sh
```

세 앱을 나란히 시험한다. 번들 id가 달라 등록이 서로 섞이지 않는다.

| 앱 | 서명 | 역할 |
|---|---|---|
| `/Applications/aa.app` | 자체 서명 「aa Code Signing」 | 1순위. SMAppService LaunchAgent plist 포함(E2) |
| `/Applications/aa-appledev.app` | 이 Mac의 유효한 Apple Development | 2순위 |
| `/Applications/aa-adhoc.app` | adhoc | 음성 대조. 업그레이드 뒤 등록이 풀려야 정상 |

진행 순서. 사용자가 할 일은 굵게 표시했다.

1. 사전 확인: 키체인 풀림, identity, cargo.
2. 세 앱을 지금 소스로 빌드·서명하고 `ditto --noqtn`으로 `/Applications`에 rename 설치한다(quarantine 없음 확인).
3. 키체인 실제 읽기용 시험 항목 `aa-p0-probe`를 만들지 묻는다(y). 값은 난수이고, 세 앱 실행 파일만 확인 창 없이 읽게 허용한다. **GUI 창 없음.**
4. **시스템 설정 등록(사용자, 화면에서).** 스크립트가 멈추고 기다린다.
   - 개인정보 보호 및 보안 > **개발자 도구**: 「+」로 세 앱을 넣고 켠다. 변경할 때 비밀번호(또는 Touch ID) 창이 뜬다.
   - 개인정보 보호 및 보안 > **전체 디스크 접근 권한**: 같은 세 앱을 넣고 켠다.
5. Enter를 누르면 `aa service register`로 LaunchAgent 두 개(`dev.aa.p0probe.standard`·`.interactive`)를 등록한다. 「백그라운드 항목이 추가됨」 알림이 뜬다. 상태가 `requires_approval`이면 **일반 > 로그인 항목 및 확장 프로그램 > 백그라운드에서 허용**에서 aa를 켠다.
6. **before 측정**(앱마다 30표본)
   - aa: LaunchServices 실행, LaunchAgent 두 종류, 이 터미널
   - aa-appledev·aa-adhoc: LaunchServices 실행
7. **업그레이드 모의**: 같은 identity로 다시 빌드(`AA_BUILD_SALT`)하고 다시 서명한 뒤 rename으로 교체한다. designated requirement는 같아야 하고 cdhash는 달라야 한다. **재등록하지 않는다.**
8. **after 측정** 뒤 `~/aa-p0/results/E1-summary.md`를 만든다.

중간에 끊기면 `bash ~/aa-dev/aa/p0/e1.sh <phase>`로 그 단계부터 다시 한다(build·register·before·upgrade·after·summary). 끝나면 thread에 알린다. thread가 ssh로 결과를 가져가 판정한다.

**기록할 것.** 아래 창이 떴으면 무슨 창이었는지 알려 준다.
- 앱 실행 때 「확인되지 않은 개발자」(Gatekeeper)
- 측정 중 「aa가 키체인의 기밀 정보를 사용하려고 함」(이 창은 「거부」를 누른다. 무인 실행이 멈추는 원인이라 실패로 기록한다)
- 업그레이드 뒤 다시 묻는 권한 창

예상 소요는 빌드 3회×2와 측정 약 10분이다.

## D. 다른 Mac에 인증서 넣기 (나중, ssh 명령만)

P0에는 필요 없다. 운영 설치 때 mac-c·mac-b에서 같은 인증서로 서명하려면 아래처럼 한다. 서명된 `aa.app`을 복사해 쓰기만 하면 그 Mac에 인증서가 없어도 된다(서명은 번들에 들어 있다).

```bash
# 1) mac-a → 대상 Mac으로 p12 복사(사용자 터미널에서)
scp ~/aa-codesign/aa-codesign.p12 mac-c:~/aa-codesign.p12
# 2) 대상 Mac에 ssh로 들어가 사용자가 직접 잠금 해제(비밀번호 직접 입력)
ssh mac-c
security unlock-keychain ~/Library/Keychains/login.keychain-db
# 3) 반입·서명 시험(B와 같은 스크립트, GUI 창 없음 예상)
bash ~/work/aa/p0/cert/import-identity.sh ~/aa-codesign.p12
rm ~/aa-codesign.p12
```

- 신뢰 설정이 필요하다고 B에서 확인되면, 화면이 없는 Mac에서는 `add-trusted-cert`의 GUI 인증 창 때문에 막힌다. 그 경우 그 Mac에서는 서명하지 않고, mac-a에서 서명한 앱을 복사한다.
- 개발자 도구·전체 디스크 접근 등록은 Mac마다 시스템 설정 화면에서 한 번 해야 한다. mac-c은 화면 접속이 돌아온 뒤 한다.

## 다음 실험(M1-1 기능이 들어간 뒤)

| 실험 | 필요 조건 | 상태 |
|---|---|---|
| E3 책임 프로세스 수명 | `aa agent`가 LaunchAgent로 돌고 session host가 있어야 한다 | M1-1 뒤 |
| E4 기동 경합 | host·agent·웹 | M1-1 뒤 |
| E5 재부팅 | 재부팅해도 되는 별도 Mac | **사용자 결정 필요** |
| E6 하위 daemon | `aa doctor lineage`(이미 있음)로 codex app-server·kache 계보를 본다 | thread가 진행 가능 |
| E7 업그레이드 부수 효과 | E1 업그레이드 모의에서 quarantine·Gatekeeper·재질문을 본다. hook 신뢰는 M1-3 | E1과 함께 |
