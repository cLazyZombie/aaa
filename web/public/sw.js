// aaa service worker: 홈 화면 앱(PWA), 셸·파일 캐시, Web Push 알림. 캐시는 herdr-web-ui(aa624cb) public/sw.js 1~190줄을
// 옮겼다(고지는 web/src/herdr/README.md, plan §8.104, 차이 목록 M5).
// - 화면 이동(navigate)은 네트워크 먼저: 연결되면 늘 host가 주는 지금 빌드다. 네트워크가 안 되면 캐시한 셸로 연다
//   (앱이 뜨고 host에 다시 붙는다). 셸을 아직 갖지 못했으면 미리 둔 「연결 없음」 화면(offline.html, aaa 것)이다.
// - 해시 붙은 빌드 파일(/assets/)·아이콘·글꼴(/fonts/)은 캐시 먼저: 이름이 내용이거나 거의 바뀌지 않는다.
//   manifest는 캐시하지 않는다(브라우저가 다시 읽어 설치된 앱을 고친다). API·WebSocket(/api/)·결과물 보기(/v/)는 건드리지 않는다.
// - 빌드마다 worker가 따로다: 빌드가 dist/sw.js에 BUILD·BUILD_FILES를 찍는다(web/scripts/service-worker-build.ts).
//   캐시 이름이 그 빌드라 지난 빌드의 파일은 지난 캐시와 함께 간다. 새 캐시가 셸과 그 셸이 여는 파일을 다 가진 뒤에야
//   옛 캐시를 지운다(오프라인 첫 새로 고침이 셸 없이 끝나지 않게). 글꼴·아이콘은 빌드 것이 아니라 FILES에 남는다.
// 아이콘을 바꾸면(v=2: F3 a 블록 커서) 아이콘 주소의 v와 FILES 이름을 함께 올린다. 그래야 설치된 앱도 새 파일을 받는다.
const CACHE_NAME = "aaa-web-v3";
const BUILD = "";
const BUILD_FILES = null;
const CACHE = BUILD ? `${CACHE_NAME}-${BUILD}` : CACHE_NAME;
const FILES = "aaa-files-v2";
const OFFLINE_URL = "/offline.html";
const buildFiles = BUILD_FILES ? new Set(BUILD_FILES) : null;

/** 이 빌드가 쓰는 파일인지. 빌드가 찍지 않은 worker(개발 서버)는 모든 파일 */
function ofThisBuild(pathname) {
  return !buildFiles || buildFiles.has(pathname);
}

const CACHE_FIRST_PATHS = new Set([
  "/icon.svg",
  "/icon-192.png",
  "/icon-512.png",
  "/icon-maskable-192.png",
  "/icon-maskable-512.png",
  "/apple-touch-icon.png",
  "/favicon-32.png",
  OFFLINE_URL,
]);

function isCacheFirst(pathname) {
  if (pathname.startsWith("/assets/") || pathname.startsWith("/fonts/")) return true;
  return CACHE_FIRST_PATHS.has(pathname);
}

/** 글꼴이나 아이콘: 빌드를 넘어 FILES에 남는다 */
function isLongLived(pathname) {
  if (pathname.startsWith("/fonts/") || CACHE_FIRST_PATHS.has(pathname)) return true;
  return pathname.startsWith("/assets/") && /\.(woff2?|ttf)$/.test(pathname);
}

/** 파일을 물었는데 화면이 왔다: 그 파일이 없는 서버가 셸로 답했다 */
function isPage(response) {
  return (response.headers.get("content-type") || "").startsWith("text/html");
}

self.addEventListener("install", (event) => {
  // aaa: 셸을 갖기 전에 끊긴 처음 실행을 위한 「연결 없음」 화면만 미리 둔다(참고는 아무것도 미리 두지 않는다)
  event.waitUntil(
    caches
      .open(FILES)
      .then((c) => c.add(new Request(OFFLINE_URL, { cache: "reload" })))
      .catch(() => undefined)
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  // 여기서는 캐시를 지우지 않는다: CACHE_NAME·BUILD·keepShell
  event.waitUntil(self.clients.claim());
});

/** 셸이 열 때 꼭 있어야 하는 파일: 입구 script와 stylesheet */
function shellFiles(html) {
  const files = new Set();
  for (const [, path] of html.matchAll(/<(?:script|link)\b[^>]*?\s(?:src|href)="(\/assets\/[^"?#]+\.(?:js|css))"/g)) files.add(path);
  return [...files];
}

/** 다른 이름 캐시의 파일, 아니면 네트워크의 파일. 둘 다 없으면 null */
async function fileFor(path) {
  const older = await caches.match(path);
  if (older && !isPage(older)) return older;
  try {
    const response = await fetch(path);
    return response.ok && response.type === "basic" && !isPage(response) ? response : null;
  } catch (err) {
    return null;
  }
}

/**
 * 방금 받은 셸을 이 캐시의 "/"로 둔다. 그 셸이 여는 파일을 이 캐시가 다 가진 뒤에만: 앞 worker가 답했을 수 있고,
 * 페이지가 다시 묻기 전에 끊기면 셸만 남는다. 그때까지 옛 캐시와 그 셸이 남아 오프라인으로도 온전히 연다.
 * 그 뒤 다른 이름 캐시를 지운다. 글꼴·아이콘은 먼저 FILES로 옮긴다.
 */
async function keepShell(response) {
  const html = await response.clone().text();
  const cache = await caches.open(CACHE);
  for (const path of shellFiles(html)) {
    if (await cache.match(path)) continue;
    const file = await fileFor(path);
    if (!file) return;
    await cache.put(path, file);
  }
  await cache.put("/", response);
  const files = await caches.open(FILES);
  const names = (await caches.keys()).filter((name) => name !== CACHE && name !== FILES);
  for (const name of names) {
    const old = await caches.open(name);
    // 물러나는 캐시의 셸이 여는 파일: 그 셸을 받은 페이지가 아직 묻지 않았을 수 있다
    const oldShell = await old.match("/");
    const starts = new Set(oldShell ? shellFiles(await oldShell.text()) : []);
    for (const request of await old.keys()) {
      const path = new URL(request.url).pathname;
      const into = isLongLived(path) ? files : starts.has(path) || (buildFiles && buildFiles.has(path)) ? cache : null;
      if (!into || (await into.match(request))) continue;
      const file = await old.match(request);
      if (file && !isPage(file)) await into.put(request, file);
    }
  }
  await Promise.all(names.map((name) => caches.delete(name)));
}

/** 오프라인에서 열 셸: 이 캐시의 것, 아니면 아직 지우지 않은 캐시의 것 */
async function cachedShell() {
  const cache = await caches.open(CACHE);
  return (await cache.match("/")) || (await caches.match("/"));
}

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/v/")) return;

  if (request.mode === "navigate") {
    event.respondWith(
      (async () => {
        let response;
        try {
          response = await fetch(request);
        } catch (err) {
          return (await cachedShell()) || (await caches.match(OFFLINE_URL)) || Response.error();
        }
        if (response.ok) {
          // 기다리지 않는다: 페이지는 셸을 오는 대로 받고, worker는 사본을 둘 때까지 산다(put은 본문을 다 읽는다).
          // 그사이 묻는 빌드 파일은 옛 캐시가 답할 수 있다: 해시 파일은 어느 쪽이든 같다. 캐시를 못 써도 페이지는 잃지 않는다
          event.waitUntil(keepShell(response.clone()).catch(() => undefined));
        }
        return response;
      })(),
    );
    return;
  }

  if (isCacheFirst(url.pathname)) {
    event.respondWith(
      (async () => {
        const cache = await caches.open(isLongLived(url.pathname) ? FILES : CACHE);
        const own = await cache.match(request);
        if (own) return own;
        const older = await caches.match(request);
        // 옛 worker가 파일 주소로 둔 셸은 그 파일의 사본이 아니다
        if (older && !isPage(older)) {
          // 앞 이름 캐시만 가진 파일을 새 셸 사본을 두는 동안 물었다: 여기에도 둔다(keepShell이 그 캐시를 지워도 새 셸이 쓰는
          // 파일의 하나뿐인 사본을 잃지 않게). 다른 빌드의 파일은 답만 한다
          if (ofThisBuild(url.pathname)) event.waitUntil(cache.put(request, older.clone()).catch(() => undefined));
          return older;
        }
        const response = await fetch(request);
        // 서버에 더는 없는 파일은 두지 않는다: 업데이트 뒤 옛 페이지가 제 빌드 파일을 물으면 404, 옛 서버면 셸이 온다
        if (response.ok && response.type === "basic" && !isPage(response)) {
          cache.put(request, response.clone());
        }
        return response;
      })(),
    );
  }
});

// Web Push(host push.rs가 보낸다). 받으면 늘 알림을 띄운다(Safari는 조용한 push가 이어지면 구독을 없앤다).
// 지금 보이는 aaa 창이 있으면 그 창이 이미 바뀐 것을 보이므로 소리·진동 없이 띄운다(herdr-web-ui sw.js push).
self.addEventListener("push", (e) => {
  let d = {};
  try {
    d = e.data ? e.data.json() : {};
  } catch {
    d = { body: e.data ? e.data.text() : "" };
  }
  e.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const watching = wins.some((c) => c.visibilityState === "visible");
      await self.registration.showNotification(d.title || "aaa", {
        body: d.body || "",
        tag: d.tag || "aaa",
        renotify: !watching,
        silent: watching,
        icon: "/icon-192.png?v=2",
        badge: "/favicon-32.png?v=2",
        data: { url: d.url || "/" },
      });
    })(),
  );
});

/**
 * 앱을 띄운 창인지(herdr-web-ui sw.js isAppWindow). host API(`/api/`)가 연 파일, 결과물 보기(`/v/`) 탭과 함께 보내는
 * 파일(아이콘·manifest·sw.js·offline.html)은 앱 창이 아니다: 마지막 부분에 확장자가 있으면 파일이다(index.html 빼고).
 */
function isAppWindow(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  const path = parsed.pathname;
  if (path.startsWith("/api/") || path.startsWith("/v/")) return false;
  const last = path.slice(path.lastIndexOf("/") + 1);
  return !(last.includes(".") && last !== "index.html");
}

// 알림을 누르면 앱을 그 자리(주소 hash, 예 `#thread=`)로 연다(herdr-web-ui sw.js notificationclick).
// 열린 앱 창에는 focus 전에 먼저 알린다: iOS가 focus를 거절하거나 늦게 끝내도 고른 자리를 잃지 않는다.
// focus가 안 되면 새 창으로 연다. 연달아 누르면 마지막 누름만 남는다.
let latestNotificationClick = 0;
let latestNotificationSelection = null;
self.addEventListener("notificationclick", (e) => {
  const click = ++latestNotificationClick;
  e.notification.close();
  const url = new URL((e.notification.data && e.notification.data.url) || "/", self.location.origin);
  const selection = url.hash ? { type: "open", hash: url.hash } : null;
  // 창을 찾기 전에 기억한다: 앞 누름의 focus가 이 누름이 창을 찾는 동안 끝날 수 있다
  latestNotificationSelection = selection;
  e.waitUntil(
    (async () => {
      // 앱 창만 고른 자리를 받는다: 파일 탭에 focus하면 누름을 삼킨다
      const wins = (await self.clients.matchAll({ type: "window", includeUncontrolled: true })).filter((c) => isAppWindow(c.url));
      if (click !== latestNotificationClick) return;
      const selectLatest = (c) => {
        if (c && latestNotificationSelection) c.postMessage(latestNotificationSelection);
      };
      // 새 창도 새 누름 뒤에 열릴 수 있다: 돌아온 창에 마지막 자리를 알린다(주소는 처음 누른 자리로 연다)
      const open = async () => selectLatest(await self.clients.openWindow(url.pathname + url.search + url.hash));
      const target = wins.find((c) => c.focused) || wins[0];
      if (target) {
        if (selection) target.postMessage(selection);
        try {
          await target.focus();
        } catch {
          if (click === latestNotificationClick) await open();
          return;
        }
        // 창마다 focus가 따로 끝난다. 늦게 끝난 앞 누름의 창이 앞으로 와도 마지막 자리를 보인다
        selectLatest(target);
        return;
      }
      await open();
    })(),
  );
});
