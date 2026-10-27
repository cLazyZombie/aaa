# host 보고서 운영

보고서는 host가 `/v/`에서 같은 sandbox·서명 링크 규칙으로 보여 준다. 공유 링크는 기본 30일이며 폐기할 수 있다.
만료·폐기돼도 pairing한 앱에서는 「앱에서 열기」로 본다.

## 질문 카드·보고 본문에 영상 넣기

[질문 작성 문서](questions.md)와 같이 그림과 같은 문법으로 library의 영상을 본문에 넣는다. 질문에서는 반드시 `## Questions` 안에 넣는다.

```markdown
## Questions
### 1. 시안 동작
![시안](library/preview/a.png)
![시안 영상](library/preview/a.mp4)

- A 적용한다 (추천) — 영상의 동작을 적용한다
- B 고친다 — 고칠 점을 글로 적는다
```

카드·보고 본문에서 영상의 재생 단추를 한 번 누르면 새 탭 없이 바로 재생한다. `mp4`·`webm`의
`![]()`와 기존 `[시안 영상](library/preview/a.mp4)` 모두 controls·muted·playsinline 재생기로 표시한다.
멈춤·탐색·음소거 해제는 재생기에서 한다. 기본 자동 재생·반복은 없으며 답을 골라도 영상 재생으로 선택이 바뀌지 않는다.
Markdown 영상은 첫 재생 누름에서만 주소를 붙인다. 누르기 전에는 metadata 요청도 보내지 않는다.
H.264 MP4(yuv420p, faststart)를 권장한다. 시안은 5~10초·10 MiB 이하를 목표로 하고, 긴 자료는 장면별로 나누어
길이·용량·비교할 점을 적는다. WebM은 보조 형식으로 쓸 수 있지만 Safari용 MP4를 우선한다.
이는 작성 권장 기준이며 업로드·회수의 실제 상한은 별도다. 답한 질문 사본의 합계 상한은 64 MiB다.

HTML 보고서는 아래처럼 썸네일·선택 재생을 기본으로 한다. `preload="none"`만 주는 것보다 첫 누름 전 `src` 자체를
붙이지 않는 것이 초기 영상 요청을 확실히 막는다. 썸네일도 library에 두고 상대 주소로 쓴다.

```html
<img src="images/chart.png" alt="비교 그림" width="1280" height="720" loading="lazy" decoding="async" style="max-width:100%;height:auto">
<video id="preview" controls muted playsinline preload="none" poster="preview/a.jpg" style="max-width:100%"></video>
<button id="play-preview" type="button">시안 영상 재생</button>
<p id="preview-state" role="status"></p>
<script>
document.querySelector('#play-preview').onclick = async () => {
  const video = document.querySelector('#preview');
  if (!video.hasAttribute('src')) video.src = 'preview/a.mp4';
  try {
    await video.play(); // 사용자 누름 안에서 시작
    document.querySelector('#preview-state').textContent = '';
  } catch {
    document.querySelector('#preview-state').textContent = '영상을 읽지 못했습니다. 보고서를 다시 열어 주세요.';
  }
};
</script>
```

그림은 실제 width/height로 자리를 예약하고 화면 근처에서 읽는다. 한꺼번에 비교할 필요가 없는 매체를 처음부터 모두 받지 않는다.
기존 r 판의 원본은 그대로 보존하며 새 보고서 작성부터 적용한다.
소리 없는 짧은 시안의 자동 반복은 움직임 비교용 HTML에서만 명시적으로 선택하는 것을 권한다. 이때
`autoplay loop muted playsinline`과 멈춤 controls를 주고 `prefers-reduced-motion`이면 자동 재생하지 않는다.
일반 질문 카드는 정지 화면 비교·여러 영상의 다운로드·접근성을 위해 누르면 재생을 기본으로 한다.
[WebKit 재생 정책](https://webkit.org/blog/6784/new-video-policies-for-ios/)에 따라 muted와 playsinline을 함께 쓴다.

질문 첨부도 인증한 웹 기기에 발급한 2시간 `/v/<권한>/` 경로로 읽는다. 새 탭·sandbox 하위 미디어·Range는
기기 cookie 없이 그 질문/hash 또는 답한 사본만 읽으며, 기기 폐기·권한 만료·질문 교체 후에는 열리지 않는다.
앱 API 주소를 영상 주소로 직접 쓰거나 sandbox에 `allow-same-origin`을 더하지 않는다.

## 로딩 상태와 다시 열기

보고 글의 이미지는 주소 확인·읽기 중·실패를 표시한다. 이미지 로딩/실패 또는 영상 실패의 「상태 확인」은
사용자가 누를 때 같은 host의 `/v/` 파일에 쿠키 없는 HEAD를 한 번 보낸다. HTTP 상태·크기·상대 파일 경로와
이미지의 완료/디코드 상태만 보이고, cap·query·origin·오류 원문은 진단에 넣지 않는다. 파일 자체를 재다운로드하거나 자동 재시도하지 않는다.

앱 안 보기의 2시간이 지나면 만료 안내를 보인다. host 재시작 뒤에도 옛 보기 주소는 끝날 수 있다.
보고서 머리의 「다시 열기」는 현재 선택한 r 판에 새 보기 권한을 받아 이미지·영상·HTML을 다시 연다.
갱신 실패를 성공으로 표시하거나 최신 t 판으로 바꾸지 않는다. 질문 첨부는 그 질문을 다시 열어 권한을 받는다.

보고서의 1KiB 이상 텍스트는 Accept-Encoding이 허용하면 gzip stream으로 보낸다. 이미지·영상·PDF, Range·내려받기와
오류 응답은 원본 bytes를 유지한다. 압축 응답에는 Content-Encoding/Vary를 주고 원본 Content-Length를 남기지 않는다.
HEAD는 같은 압축 협상 헤더와 빈 본문이다. `private, no-store`와 권한·sandbox 경계는 유지한다.

## 웹 보고서 작성과 sandbox

정책과 착지 기록은 [plan §8.381](plan.md#8381-보고서-자기-폴더-fetch음원-혼합10-21-t-0589)에 둔다.

보고서 HTML은 **origin 없는 sandbox**에서 script를 실행한다. 앱과 같은 주소라도 cookie·localStorage·sessionStorage·
부모 화면·앱 API에 접근할 수 없다. `allow-same-origin`을 주지 않는다. 외부 CDN·외부 fetch·WebSocket·EventSource·
service worker·폼 전송도 사용할 수 없다. script·CSS·글꼴·그림·영상·음원·JSON·CSV는 보고서 폴더에 함께 두고 상대 URL로 쓴다.
thread는 library 파일을 먼저 쓰고 report.md를 마지막에 쓴다. symlink·숨은 파일·폴더 밖 파일은 가져오지 않는다.

`<img src="images/chart.png">`, `<audio controls src="audio/tone.wav">`, `<script src="play.js">`와
`fetch('./data.json')`를 쓸 수 있다. fetch/XHR은 **현재 `/v/<권한>/` 아래 보고서 폴더 전체**의 GET/HEAD만 된다.
하위 HTML에서도 `../data.json`처럼 같은 보고서 안으로 올라갈 수 있다. 다른 보고서·앱 API·외부 주소는 CSP가 막는다.
원하는 파일이 없거나 권한이 만료되면 HTTP 실패를 사용자에게 보여 준다. URL에 있는 권한 문자열을 별도로 저장하지 않는다.

fetch 기본 credentials(`same-origin`)는 opaque origin에서 쿠키를 보내지 않는다. 명시하려면 `credentials: 'omit'`을 쓴다.
`credentials: 'include'`·XHR `withCredentials`는 쓰지 않는다. 서버는 `Origin: null`의 유효한 파일 읽기에만 CORS를 주고
credential 허용 헤더·preflight·redirect를 주지 않는다. 쿠키를 동반한 opaque 요청과 opaque 앱 API 요청은 서버에서도 거부한다.
기기 쿠키의 SameSite=Strict·HttpOnly(HTTPS는 Secure)도 유지한다. `/f/` 페이지 보기는 별도 정책으로 fetch가 막혀 있다. 질문 첨부 HTML은 그 질문의 `/v/<권한>/` 폴더 안에서만 읽는다.
`Origin: null` 자체를 신뢰하는 것은 아니다. 파일을 읽을 권한은 매번 검증하는 `/v/<권한>/` 링크에 있고, 그 링크를 아는
다른 opaque 문서도 같은 파일을 읽을 수 있다. 링크는 원래부터 쿠키 없이 공유할 수 있는 읽기 권한이므로 비밀로 다룬다.

음원을 섞거나 음량을 따로 조절하려면 다음처럼 Web Audio를 쓴다. iOS의 `audio.volume`과 opaque origin에서 무음이 될 수 있는
`createMediaElementSource` 대신 bytes를 읽어 decode한다. AudioContext는 사용자 누름에서 열어야 한다.

```html
<button id="play">재생</button><p id="state"></p>
<script>
document.querySelector('#play').onclick = async () => {
  const state = document.querySelector('#state');
  const ctx = new AudioContext(); // 사용자 누름 안에서 만든다
  try {
    await ctx.resume();
    const response = await fetch('audio/tone.wav', { credentials: 'omit' });
    if (!response.ok) throw new Error(`음원 읽기 실패: ${response.status}`);
    const buffer = await ctx.decodeAudioData(await response.arrayBuffer());
    const source = ctx.createBufferSource();
    const gain = ctx.createGain();
    source.buffer = buffer;
    gain.gain.value = 0.5;
    source.connect(gain).connect(ctx.destination);
    source.onended = () => { state.textContent = '재생 끝'; void ctx.close(); };
    source.start();
    state.textContent = '재생 중';
  } catch (error) {
    state.textContent = String(error);
    await ctx.close();
  }
};
</script>
```

회수·publish(웹 업로드·실시간 등록·import 포함)한 HTML/JS/SVG에서 제한 API나 정적으로 확인하지 못한 fetch 경로·옵션이 보이면 coordinator inbox에
`보고서 sandbox 확인` 경고를 남긴다. 복사는 성공으로 유지한다. 문자열 휴리스틱이라 주석의 오탐·별칭/난독화의 누락이 가능하며
보안 경계나 재생 성공 판정은 아니다. Chromium·WebKit에서 실제 버튼을 눌러 읽기·decode·재생 끝과 오류 표시를 확인한다.
WebKit의 휴대폰 크기 시험은 실제 iPhone의 출력 장치·소리 청취까지 보증하지 않는다.

정책 근거: [CSP의 경로 제한과 redirect 규칙](https://www.w3.org/TR/CSP/),
[Fetch의 opaque origin·CORS·credentials 규칙](https://fetch.spec.whatwg.org/).

## 보관함과 완료 보고

모든 프로젝트에서 `[완료]` 보고를 회수하거나 thread를 resolve하면 마지막 보고가 같은 사본으로 「내 보고서」에 안 봄으로 올라온다.
HTML을 새로 만들거나 `--to-user`로 재발행할 필요가 없다. 새 완료 본문만 다시 안 봄이며 재수집·resolve·재시작은 본 열람을 유지한다.
기존 host 사본도 시작할 때 완료 표지·resolve 기록을 확인해 올린다. 사용자에게 외부 푸시를 추가하지 않는다.

「내 보고서」·「작업 보고」에서 「보관함에 넣기」를 누르면 현재 r 판이 보관된다. 여러 판이 있으면 번호를 골라 과거 판을 보관할 수 있다.
작업 보고는 골라서 한꺼번에 넣을 수도 있다. 「보관함」에서 한 번 눌러 빼며, Reader는 읽기만 한다.
이후 새 판을 올려도 선택한 판의 본문·파일이 바뀌지 않는다. 사본은 새로 복제하지 않는다.

보관함 용량은 따로 표시하고 30일 기한·20 GiB 일반 사본 상한과 `aaa library prune`에서 제외한다.
보관함만으로 20 GiB를 넘어도 새 일반 보고서를 밀어내지 않는다. 빼면 원래 발행 시각으로 정리 대상이 된다.
「지우기」와 `aaa library delete`는 보관 여부와 관계없이 기존의 보고서 전체 삭제를 수행한다(같은 이름의 모든 판 포함).
진행 중 thread·완료 할 일이 참조하는 보고서의 기존 삭제 보호는 유지한다.

```sh
aaa library archive r-0001
aaa library archive r-0001 --remove
aaa library prune --dry-run
```

용량·정리 후보는 실제 host library 폴더의 파일과 보존 판 본문을 센다. manifest에서 빠진 잔여 파일·종료 thread의 잔여 폴더도 포함한다.
실시간 등록의 외부 원본·symlink 대상은 세지 않는다. 수집 중 숨은 임시 폴더는 용량에 포함하되 자동 삭제하지 않는다.
자동 정리는 과거 판별로 하며 최신 이름과 그 최신 r 판은 함께 정리한다. 최신 이름을 지운 뒤에도 보관한 r 판은 열린다. 명시 삭제는 보고서 전체 단위를 유지한다.

## 올리기와 실시간 폴더

host 머신의 사람·coordinator CLI에서 실행한다. 원격 머신 publish와 웹에서 host 경로를 등록하는 것은 지원하지 않는다.
웹 조작 기기는 브라우저의 파일을 올려 등록한다(아래 「웹에서 올리기」).

```sh
aaa library publish "<보고서 폴더>" --project P --name summary
aaa library publish "<그림 파일>" --project P --name screenshot
aaa library publish "<저장소의 실제 폴더>" --project P --name design --live
aaa library list --project P
aaa doctor
```

같은 이름으로 publish하면 최신 파일로 바뀌고 없어진 파일도 사본에서 지운다. 유효한 링크는 계속 최신을 가리킨다.
`--live`는 복사하지 않는다. 등록 폴더의 수정·추가·삭제를 즉시 반영하며, 저장소 착지 때 다시 publish하는 routine은 필요 없다.
등록 원본을 옮겼으면 새 실제 폴더로 다시 등록한다. 원본 경로는 웹 목록에 노출하지 않는다.

이름은 소문자·숫자·`-`·`_` 1~100자다. `t-` 접두사는 새 thread의 이름 공간이라 받지 않고 Windows 예약 이름도 받지 않는다.
숨은 이름·symlink·일반 파일 아닌 것은 가져오지 않는다. 등록 폴더 밖 경로, `..`, 역슬래시, 드라이브/ADS 경로도 열지 않는다.
symlink를 원본으로 넘기지 말고 그것이 가리키는 실제 폴더를 명시한다.

### 웹에서 올리기

보고서 화면 「보고서 올리기」(웹 조작 기기, FRONT_API 1.80)는 파일 여러 개나 폴더 하나를 고른다. 폴더를 고르면 그 폴더 안이
보고서가 된다(맨 위 폴더 이름은 빠진다). 숨은 이름(`.DS_Store`·`.git/…`)은 빼고 올린다. 파일당 16MB, 합계 64MB, 1000개까지다.
「사용자 보고서로 보낸다」를 켜면 `--to-user`와 같고 맨 위 `index.html`이 있어야 한다. 이름 규칙·같은 이름 바꾸기·library 상한·
사용자 알림은 CLI publish와 같다(사본만, `--live`는 없다). 브라우저는 파일을 하나씩 host 상태 폴더 `library-uploads/<올리기>/`에
올리고, 등록에 성공하면 그 폴더를 지운다. 등록하지 않고 떠난 올리기는 하루 뒤 다음 올리기 때 지운다. 그래서 올린 수가 맞지 않으면
등록하지 않고 처음부터 다시 올리게 한다. 대소문자나 글자 조합만 다른 이름(`README.md`·`readme.md`)은 함께 올리지 못한다. 옛 0.2 폴더 가져오기
(`library import`)는 host 경로를 다루는 이전 도구라 웹에 두지 않는다.

## 사용자에게 보내기

```sh
aaa library publish "<웹 보고서 폴더>" --project P --name weekly --to-user --title '주간 보고'
```

coordinator가 사용자에게 보여 줄 웹 보고서를 보낼 때 쓴다. 맨 위에 `index.html`이 없으면 거부한다. 제목(`--title`)은 줄바꿈 없는 200자 이내이며 없으면 `P/이름`으로 보인다.
웹 보고서 화면의 「내 보고서」 탭에 카드로 들어가고, 열어 보기 전까지 「안 봄」으로 세어 활동 막대 「보고서」 단추와 탭에 수를 단다.
제목·공유 링크·열기·새 창 어느 길로 열어도 봄이 된다. 채팅·알림의 공유 주소(`/v/<token>/`)를 pairing하지 않은 브라우저에서
바로 열어도 첫 문서 GET이 성공할 때 host가 기록한다. 안 봄 해제와 `library list`의 마지막 열람을 함께 기록하며 연결된 앱의 배지도 바뀐다.
문서 탐색(`Sec-Fetch-Mode: navigate`, 목적지 `document`·`iframe`·`frame`)만 센다. HEAD·fetch·그림 요청·메타데이터 없는
링크 미리보기·`Purpose`/`Sec-Purpose`/`X-Purpose`의 preview·prefetch·prerender·다운로드·실패 응답은 세지 않는다.
같은 이름으로 다시 보내면 다시 안 봄이 된다. `--to-user` 없이 다시 올리면 「작업 보고」로 돌아간다.
보낼 때 사용자 알림 post(`topic report`, 앱 안 알림만, 외부 푸시 없음)를 함께 남긴다. 같은 글은 24시간 안에 다시 남기지 않는다.
`--to-user`가 없는 publish·import와 진행 중 thread 사본(`t-NNNN`)은 「작업 보고」 탭이다. 완료 보고·resolve 뒤 마지막 보고는 자동으로 「내 보고서」에 뜬다.
채팅에서 이 보고서를 가리킬 때는 `aaa:report/<이름>`(thread 사본은 thread 번호)을 쓴다. 웹 대화는 이것을 제목 칩으로 그리고
누르면 앱 안 보고서 화면에서 연다. 터미널에서는 글 그대로 읽힌다. 공유 주소는 사용자가 밖으로 보내 달라고 할 때만 쓴다(plan §8.329).

## 보고서를 낸 thread

보고서 화면(「내 보고서」 카드·「작업 보고」 줄·열람 머리)은 이름 왼쪽에 그 보고서를 낸 thread 번호를 보인다.
publish 때 `--thread t-0441`로 직접 줄 수 있고, 그 프로젝트에 없는 thread면 거부한다. 주지 않으면 host가 이름에 든 번호
(`t-0441`·`t0441`, 앞뒤가 영숫자가 아닐 때)와 원본 경로의 뒤 성분부터(`…/library/t-0441`이 thread 폴더 `…-t-0441/…`보다 먼저)
찾은 번호 중 **그 프로젝트에 실제로 있는 첫 thread**를 적는다. 원본 경로 자체는 적지 않는다. 같은 이름으로 다시 올리면 다시 정한다.
thread 사본(`t-NNNN`)·import(`archive-t-0001`)와 이 기록 전에 올린 보고서는 이름에서 찾는다. 근원을 모르면 번호 없이 보인다. 「작업 보고」의 thread 사본 줄은 이름이 곧 번호라 표지를 따로 달지 않는다.

```sh
aaa library publish "<thread library 폴더>" --name weekly --to-user --title '주간 보고' --thread t-0441
```

## 여러 보고서 가져오기

```sh
aaa library import "<이전 보고서 루트>" --project P --prefix archive-
```

하위 폴더 각각을 보고서로 올린다. 예를 들어 `t-0001/`은 `archive-t-0001`이 된다. 기존 대상 이름과 충돌하면 덮지 않고 건너뛴다.
결과에는 가져온 목록과 **원본 항목별 건너뛴 이유**가 있다. 접두사 없이도 쓸 수 있지만 `t-` 이름들은 거부된다.
숨은 이름·symlink·낱 파일도 건너뛴다. 필요한 낱 파일은 `publish`하고 symlink 항목은 실제 폴더로 `--live` 등록한다.

복사와 실시간 등록은 모두 host.toml `[projects] library_copy_mb` 상한(기본 **200MiB**)을 따른다.
상한을 넘는 보고서는 일부 파일만 성공한 것처럼 올리지 않고 전체를 거부한다. 1~4GB 캡처 묶음은 건너뜀 목록에 남는다.
필요한 HTML·선택한 그림·로그를 상한 안의 별도 폴더로 준비해 publish한다. 큰 원본은 보관하고 상한을 임의로 늘리지 않는다.
숨은 파일이나 symlink에 기대던 HTML의 상대 자원은 그 항목을 빼면 표시되지 않으므로 이전 뒤 확인한다.

## 찾기·정리·키 교체

웹 왼쪽 활동 막대의 「보고서」가 본문을 보고서 화면으로 바꾼다. 「내 보고서」(완료 보고·사용자에게 보낸 것), 「작업 보고」(그 밖), 「보관함」(고정한 판) 세 탭이고, 프로젝트·이름·크기·갱신·만료와 링크 상태를 보며 「열기」는 본문에서, 「새 창」은 새 창에서 연다. 복사·QR·폐기·재발급은 thread 화면과 같다(작업 보고는 「공유 링크」를 눌러 편다).
Reader는 공유 링크를 받지 않고 앱 안 보기만 하며 지우기도 없다.

지우기는 확인 창 한 번 뒤 그 보고서를 **모든 곳에서 영구히** 지운다: host 사본·공유 링크(401)·목록, thread 사본이면 회수한 보고 글(`threads/t-NNNN.md`)까지. 되돌리기는 없다.
「내 보고서」는 카드마다, 「작업 보고」는 골라서(「끝난 것 모두 고르기」) 한꺼번에 지운다. 끝나지 않은(resolve 전) thread의 사본은 고를 수 없고 host도 거부한다.
CLI는 `aaa library delete --project P <이름>…`이다(같은 규칙, 하나라도 진행 중이면 모두 거부). `aaa doctor`의 사본 바이트 합계에는 실시간 원본을 넣지 않는다.

```sh
aaa library prune --dry-run
aaa library prune --dry-run --project P
aaa library prune --older-than 90 --project P
aaa library rotate
```

prune은 보관함 밖에서 최근 게시/회수가 90일보다 오래된 사본을 삭제한다. 진행 중 thread·실시간 원본·최신 thread report.md·thread 기록은 남긴다. 지운 r 판의 본문은 그 사본과 함께 정리한다.
삭제된 보고서의 공유 링크(401)와 앱 안 보기 경로는 끝난다.

host ticker는 시작 뒤 첫 tick에서, 그 뒤 **6시간마다** 사본을 자동 정리한다. host.toml `[projects]` 설정은
`library_max_days = 30`, `library_max_mb = 20480`(20GiB)이며 생략하면 이 기본값을 쓴다.
각각 `0` 또는 `""`이면 해당 제한을 끈다. 둘 다 끄면 자동 정리를 하지 않는다.
기한을 넘은 사본을 먼저 지우고, 남은 합계가 상한을 넘으면 최근 게시·회수가 가장 오래된 순으로 지운다.
진행 중 thread와 완료 보관의 결과·메모가 참조하는 보고서 사본은 항상 보호한다. 보관을 복원해도 완료 당시 참조는 남는다. 실시간 원본은 합계에도 세지 않으며 만료 정리도 하지 않는다.
보호 사본만으로 상한을 넘으면 그 회차에는 삭제하지 않고 doctor와 관련 프로젝트 inbox에 알린다.
같은 초과 상태에서는 재시작·inbox 완료 처리 뒤에도 다시 알리지 않는다. 정상으로 돌아온 뒤 새 초과가 생기면 다시 알린다.

`--dry-run`은 이 자동 규칙의 전체 후보·바이트·기한/용량 사유를 쓰기 없이 보여 준다.
`--project`는 후보만 제한하며 합계·상한은 host 전체다. `--older-than N`을 함께 주면 미리 볼 기한만 N일로 바꾼다.
실제 수동 prune은 기존대로 필수 `--older-than N`의 기한만 적용한다.
doctor와 웹 보고서 목록의 합계·상한·기한 역시 host 전체 값이다.
배포 전 새 binary로 dry-run을 확인한다. 이 요청은 FRONT_API 1.40부터이며 옛 host는 삭제하지 않고 거부한다.
rotate는 보고서 서명 키를 교체해 **기존 공유 링크를 모두 끝낸다**. pairing·TLS 키·보고서는 남으며 앱 안 보기도 된다.
필요한 공유 링크는 `aaa library link P NAME` 또는 웹 「새 링크 만들기」로 다시 발급한다.
웹은 설정 「이 aaa」 → 「공유 링크」 → 「키 교체…」(조작 기기)이며, 확인 창에 지금 살아 있는 공유 링크 수와 「모두 끊긴다」를 보인다.
