# 실행 파일 크기와 장애 진단

단일 실행 파일의 웹 자산과 최적화 설정을 줄인다. 터미널·설치·로그·panic 복구를 위한 unwind는 유지한다.
실행 파일을 만든 뒤 다시 쓰는 후처리는 하지 않으므로 기존 서명·launcher 결속·provenance·SHA 검증이 같은 bytes를 다룬다.

## 기준과 원인

기준은 `24cab7fc`, Rust 1.99.0, macOS aarch64의 release 빌드다. 배포판과 같은 크기인 94,566,984바이트다.
아래 수치는 MB가 아닌 바이트이며, 서로 겹치는 행을 합산하지 않는다.

| 부분 | 바이트 | 의미 |
| --- | ---: | --- |
| 기계어 `__text` | 41,366,524 | 역할 crate와 제네릭·직렬화 코드 |
| 읽기 전용 상수 `__const` | 19,895,392 | 웹 자산 포함 |
| 전체 웹 자산 | 18,160,351 | 위 상수의 대부분 |
| 예외 표 `__gcc_except_tab` | 2,378,944 | panic unwind에 필요 |
| `__unwind_info` + `__eh_frame` | 5,255,524 | 스택 복원 정보 |
| `__LINKEDIT` | 24,281,088 | 심볼·이름·링크 정보·서명 포함 |
| 심볼 항목 | 7,255,632 | `__LINKEDIT`의 일부 |
| 심볼 이름 문자열 | 15,540,904 | `__LINKEDIT`의 일부 |

Mach-O 실행 파일 자체에는 DWARF 섹션이 없다. `debug="line-tables-only"`를 쓴다고 90MB 대부분이
소스 줄 정보인 것은 아니다. macOS의 기본 분리 방식은 `unpacked`이며, 줄 정보를 위한 빌드 산출물을
배포 폴더에 함께 보관하지 않으면 실행 파일만으로 모든 줄을 찾을 수 없다.
[Cargo profile 문서](https://doc.rust-lang.org/cargo/reference/profiles.html)를 기준으로 플랫폼 차이를 구분한다.

`llvm-nm --demangle`의 함수 주소 간격으로 추정한 코드 기여는 core 8.80MB, serde 7.18MB,
aaa-host 6.16MB, aaa-proto 4.03MB, serde_json 3.84MB 순이다. Mach-O 심볼에는 크기가 없어
다음 주소까지를 계산했으며, 제네릭 소유 crate와 inlining 때문에 정확한 crate별 청구서가 아니다.
가장 큰 단일 함수 중 하나는 CLI 하위 명령 구성 코드다(약 159KB).

JavaScript는 13,400,774바이트이며 gzip 대조에서 2,839,907바이트였다. 구문 강조 언어와 worker도 포함한다.
글꼴은 4,301,136바이트로 이미 WOFF2이며 추가 압축 효과가 작다. 언어·글꼴·화면 기능을 삭제하지 않는다.
Rust·SQLite·ring 등은 정적으로 포함하지만 macOS 시스템 framework는 동적 링크다. SQLite 함수의 주소 간격 합은
약 0.64MB여서 이 의존성을 빼는 것이 주된 해법은 아니다. Windows의 정적 CRT와 Linux release의 musl도 유지한다.
base64·getrandom·hashbrown 등 서로 다른 의존 버전은 있으나 proc-macro와 target 전용 의존성까지
실행 파일 중복으로 계산하지 않는다. TLS는 이미 ring을 명시하고 불필요한 기본 backend를 끈 상태다.

## 최적화 후보와 성능

| 후보(자산 압축·콜스택 포함) | 실행 파일 바이트 | 빌드 관측 초 |
| --- | ---: | ---: |
| 기준(변경 전) | 94,566,984 | 227.83 |
| profile 변경 없음 | 84,009,144 | 187.93 |
| Thin LTO만 | 84,473,560 | 203.68 |
| opt-level s만 | 78,970,440 | 113.11 |
| s + Thin LTO | 74,152,168 | 118.94 |
| s + Thin LTO + codegen unit 1 | 62,285,016 | 253.44 |
| 제외: packed dSYM(kache에서 줄 정보 누락) | 62,402,520 | 248 |

선택한 실행 파일은 34.1% 작다. 같은 worktree에서 각 profile의 첫 빌드를 관측했고 kache를 유지했다.
의존성·profile별 캐시 조건이 다르므로 표의 시간을 순수한 최적화 비용으로 단정하지 않는다.
일반 release에서도 `strip="none"`, `panic="unwind"`, `debug="line-tables-only"`를 명시한다.
macOS producer의 최종 확인 빌드는 236초였다. unpacked를 유지하고 별도 dSYM을 보관한다. packed 실험은 kache 1.2.2에서
object 부재 경고와 함께 UUID가 일치하지만 Rust 줄 정보가 없는 dSYM을 만들었다. 이 후보는 제외했다.
심볼 파일 존재·UUID 일치만으로 파일·줄 해석을 검증하지 않는다.

빌드 종료 후 기준/최종 사본을 같은 이름 `aaa`로 번갈아 31회 실행했다. 첫 실행을 뺀 30회 중앙값은
version 7.41→6.91ms, help 7.62→7.13ms, zsh 완성 14.43→14.39ms다. stdout 원문도 같았다.
전체 서비스 처리량·장기 메모리 사용의 보증은 아니다.

격리한 실제 host에서 주 JS gzip 요청은 1,547,936→464,919바이트, 중앙값 0.47→0.52ms였다.
identity 요청은 원문 복원 때문에 0.84→6.58ms로 느려졌다. 브라우저는 gzip을 협상하며, 압축을
받지 않는 클라이언트의 이 비용은 남는다. 양쪽 응답을 풀어 원본과 바이트 대조했다. 새 DB/상태까지
만드는 최초 host 준비의 단일 관측은 일반적인 시작 속도 결론에 쓰지 않는다.

## 플랫폼별 산출물

| 플랫폼 | 실행 파일 바이트 | 별도 진단 파일 |
| --- | ---: | --- |
| macOS aarch64 | 62,285,016 | dSYM 134,210,326, gzip tar 26,380,342바이트 |
| Windows x86_64 MSVC | 44,400,128 | PDB 107,446,272, gzip 24,878,532바이트 |
| Linux x86_64 musl | 81,699,016 | 줄 정보를 zlib DWARF로 실행 파일 안에 보관 |

Windows는 교차 release 링크와 실행 파일/PDB 식별자 일치, 정적 CRT의 추가 DLL 의존 없음까지 확인했다.
Linux는 static PIE이며 동적 라이브러리 NEEDED가 없다. 압축 전 최적화 후보는 146,102,808바이트였다.
objcopy 실험 사본의 78,616,816바이트와 정식 링크의 81,699,016바이트는 다른 압축 생산 경로이므로
같은 최종 산출물로 쓰지 않는다. 정식 파일에서 `aaa_common::panic_log::latest`의 상대 소스 파일·줄 해석을 확인했다.
두 교차 빌드는 약 242초·247초였고 GNU/Linux·Windows workspace/all-targets strict clippy도 통과했다.
macOS와 다른 CPU·링커·시스템 라이브러리 구성이므로 플랫폼 사이 수치 차이를 같은 기능의 회귀로 판단하지 않는다.
Windows/Linux native 실행과 나머지 두 CPU target은 release CI에서 확인한다.

## 자산 보관과 전송

host build script가 gzip의 시각·파일 이름을 비워 결정적인 bytes를 만든다. gzip이 32바이트 넘게 작을 때만
그 표현을 실행 파일에 넣고, 그렇지 않으면 원문을 넣는다. 압축본과 원문을 동시에 내장하지 않는다.
HTTP가 gzip을 받으면 그대로 보내고 `Content-Encoding: gzip`, `Vary: Accept-Encoding`을 붙인다.
헤더가 없거나 명시적으로 gzip을 거부하면 원래 bytes로 복원한다. MIME·cache·404·SPA fallback은 유지한다.
`index.html`에서 읽는 웹 빌드 이름도 복원한 원문에서 구한다. 사용자 보고서·동영상의 Range/sandbox 경로는 바꾸지 않는다.

## panic 진단과 심볼

이전 hook은 `RUST_BACKTRACE=full`이어도 콜스택을 전혀 출력하지 않았다. 이제 같은 안전한 JSON 기록에
최대 64개 프레임을 넣는다. 함수 이름은 실행 파일 심볼에서 얻고 512글자로 제한한다. 경로·제어문자가 섞인
이름은 숨긴다. 소스 경로는 Git이 추적하는 저장소 상대 경로만 허용한다. payload·환경·임의 thread 이름은
읽거나 포맷하지 않는다. 심볼 해석이 실패한 프레임도 `unknown`으로 남기며, 지원되는 플랫폼에서는 module 상대 주소를 기록한다.
위치·역할·thread/task 숫자 ID·빌드 SHA·반복 횟수와 `latest()`는 유지한다.

macOS/Linux는 실행 파일의 함수 심볼을 보존한다. macOS 배포·release는 `split-debuginfo=unpacked`를 유지한다. kache가 dSYM을 만들지 않은 환경은 남은 object로 `dsymutil`을 실행한다. 그 dSYM은 deploy-build 출력 폴더의 `aaa.dSYM`으로 복사해 임시 worktree 정리 뒤에도 남긴다. release는 OS/CPU별 `aaa-macos-*.dSYM.tar.gz`를 같은 실행 파일 SHA에 묶어 게시한다. 압축을 풀어 `aaa.dSYM`을 실행 파일 옆에 두면 줄 정보를 더 읽을 수 있다. 소스 줄은 해당 플랫폼의 디버그 정보가 있을 때 표시한다.
Linux musl release는 DWARF를 실행 파일 안에 둔다. `.cargo/config.toml`의 target별
`--compress-debug-sections=zlib`가 링크 단계에서 줄 정보를 압축하고 backtrace의 gimli/miniz reader가 읽는다.
함수 심볼·실행 코드·unwind를 지우거나 완성된 파일을 다시 쓰지 않는다.
Windows MSVC의 함수·줄 정보는 [별도 PDB](https://learn.microsoft.com/en-us/windows/win32/debug/symbol-files)에 있다.
release는 같은 빌드에서 나온 `aaa-windows-x86_64.pdb.gz`를 게시하고, `manifest.json`의 `symbols`에 PDB SHA와
대응 실행 파일 SHA를 함께 기록한다. `SHA256SUMS`에도 포함한다. 설치·업데이트가 받는 실행 파일 계약은 그대로다.
Windows에서 이름까지 즉시 해석하려면 해당 압축을 풀어 PDB를 `aaa.pdb`라는 이름으로 `aaa.exe` 옆에 둔다.
PDB가 없는 설치에서는 이름이 `unknown`일 수 있으므로 같은 판 PDB로 상대 주소를 해석한다. 서로 다른 빌드의 PDB를 섞지 않는다.

새 hook은 Rust panic 진단이다. 메모리 손상·SIGSEGV·강제 종료를 Rust panic으로 바꾸거나 임의 signal handler를
추가하지 않는다. OS crash dump 분석에 필요한 심볼과 unwind 정보를 제거하지 않는다.

## 선택에서 제외한 변경

- `panic="abort"`: host·설치 감독자의 기존 panic 격리/복구와 `catch_unwind`를 바꾸므로 제외한다.
- 심볼 전체 제거: 함수 이름과 사후 crash 분석이 손상되므로 제외한다.
- 전역 debug 정보 제거: OS별 파일·줄 정보와 Windows PDB 생성에 미치는 효과가 달라 적용하지 않는다.
- `opt-level="z"`: 벡터화를 끄는 변화까지 포함하므로 이번에는 `s`와 링크 최적화의 실측을 우선한다.
- 의존성·구문 강조 언어·글꼴 삭제: 필요한 기능을 줄이는 방법으로 크기를 맞추지 않는다.

## 검증 범위

자산 시험은 생성된 모든 파일을 원본 bytes와 대조하고 실제 handler의 gzip/identity·명시적 거부·MIME·cache·404를 확인한다.
panic 시험은 네 역할의 실제 자식 panic, 비밀 비노출, 함수 이름, 파일 허용 목록, unwind 후 생존과 `latest()`를 확인한다.
release workflow도 각 native target에서 이 release panic 시험을 실행한다. Windows 교차 컴파일은 native 실행 성공과 구분한다.
전체 ci-local은 실제 브라우저의 압축 자산 로딩과 기존 설치·업데이트·배포 회귀를 포함한다.

이번 전체 실행은 web·fmt·build·clippy·웹 E2E90·공개 검사가 통과했고 Rust restart 시험 하나가 실패했다.
Python probe가 `Killed: 9`/exit 137로 끝났으며 같은 시각 macOS syspolicyd가 파일 열기 한도 초과,
YARA 검사 실패, Malware rejection을 기록했다. 재실행으로 통과 처리하거나 시스템 설정·시험 시한·기대값을
바꾸지 않았다. 새 자산·panic 시험 및 설치 owner는 통과했지만 전체 gate 통과로 보고하지 않는다.

Linux DWARF 압축 대조에서 모든 `.debug_*`·실행·unwind 섹션의 복원 bytes와 심볼 이름·주소·크기가 일치했다.
`llvm-dwarfdump --verify`는 압축 전후 모두 같은 최적화 DIE 범위 오류로 실패했다(부모 범위 3,361개,
겹침 4,091개). 압축 자체의 정보 손실 증거는 없지만 이 verifier가 통과했다고 쓰지 않는다.
macOS에서 PE/ELF를 교차 빌드할 때 kache의 dSYM 처리기가 외부 형식을 거절하는 진단도 따로 남겼다.
Cargo 링크 성공, 산출물 형식·심볼 확인, native 실행 판정은 서로 구분한다.
LLVM 22 symbolizer의 Mach-O dSYM hint 조회는 도구 자체가 crash했다. macOS 줄 해석은 Apple dwarfdump와
실제 backtrace reader가 실행된 panic 출력으로 확인했다. 최종 단독 실행 사본은 함수 이름을, dSYM 동반 사본은
저장소 상대 파일·줄 19곳을 출력했다.

## 2026-10-25 native 장애 진단 재확인

기준 `04dd0417`, Rust 1.99.0의 임시 panic 계측 사본으로 기존 release 설정을 재확인했다.
웹 산출물 370개를 고정해 macOS aarch64, Windows x86_64 MSVC와 Linux aarch64 musl에서 실제 실행했다.
이 절의 크기는 앞 절과 소스·CPU·도구가 달라 앞 절의 회귀 비교로 쓰지 않는다. 임시 panic 코드는 제품에 넣지 않는다.
Windows만 설치된 Rust 1.99.1을 사용했다.

| native 대상 | 실행 파일 바이트 | 기호 보관 | 실행 검증 |
| --- | ---: | --- | --- |
| macOS aarch64 | 73,939,224 | 같은 UUID의 독립 dSYM | JSON hook·기본 backtrace의 내부 47행/호출자 41행 |
| Windows x86_64 MSVC | 52,358,656 | PDB 127,488,000바이트 | 같은 두 스택 줄, PE/PDB GUID·age 일치 |
| Linux aarch64 musl | 88,092,136 | 실행 파일 안의 zlib DWARF | 같은 두 스택 줄, 정적 링크·ELF interpreter 없음 |

원래 Cargo target을 숨기고 실행 파일과 macOS dSYM/Windows PDB 사본으로 실행했다.
Windows PDB를 치우면 스택의 파일·줄이 사라지고 module 상대 주소는 남았다. 같은 판 PDB를 복원한 뒤
LLVM symbolizer 19.1.5가 실제 상대 주소 `0x198461`/`0x198470`을 각각 `crates/aaa/src/main.rs:47`/`:41`로 해석했다.
panic 머리의 파일 위치만을 스택 줄 정보로 세지 않았다. Linux는 격리된 native VM에서 실제 musl 바이너리를 실행했다.
이 결과는 Rust panic과 사후 심볼 조회 증거이며 모든 OS signal/crash dump 수집 경로를 실행했다는 뜻은 아니다.

macOS 동일 설정 기준 반복은 nice 15·Cargo 기본 병렬도, 시작 load1<5·외부 빌드 없음,
에이전트 갱신 관측 전후 185초 제외, 조건별 3회 중앙값이다. shared kache·OS page cache는 유지했다.
clean은 해당 작업의 target만 비우고, 수정 뒤 빌드는 고유한 함수 본문 상수를 바꿨다.

| 동일한 release 설정의 두 반복 묶음 | clean·kache 끔 | clean·kache 켬 | 본문 수정 뒤·kache 켬 |
| --- | ---: | ---: | ---: |
| 기준1 | 273.206초 | 146.461초 | 141.339초 |
| 기준2 | 274.068초 | 146.309초 | 141.970초 |

이는 설정 개선 비교가 아니다. 차이는 모두 0.5% 이내였다. 기존 release의 `debug="line-tables-only"`,
`strip="none"`, `panic="unwind"`와 dev를 유지한다. 별도 Cargo profile인 `deploy-build`는 없고
`scripts/deploy-build.sh`와 bench는 release를 쓴다. 현재 test/verify/e2e의 상속·줄 정보는 아래 profile 역할 표를 따른다.
release workflow와 deploy-build의 기존 dSYM/PDB 보관, Linux의 압축 DWARF를 유지한다.

## 배포와 내부 빌드

[현재 profile 역할·경로](build-profiles.md)를 따른다. 실제 배포 release의 opt-level=s·thin LTO·CGU1·line tables는 유지한다. ci-local의 E2E 실행 파일과 P0 모의는 debug=0의 e2e를, unit·실패 재현은 verify/test를 쓴다. 앞의 native release 기호 확인은 배포 설정의 근거이며 내부 e2e의 스택 파일·줄 보장을 뜻하지 않는다.
