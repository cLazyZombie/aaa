// aaa service worker(§8.26): 홈 화면 앱(PWA)과 Web Push 알림.
// - 화면·API·자산을 캐시하지 않는다. 업그레이드 뒤에도 늘 host가 주는 지금 화면을 연다.
// - 화면 이동(navigate)만 네트워크가 안 될 때 미리 둔 「연결 없음」 화면을 보인다.
// 아이콘을 바꾸면(v=2: F3 a 블록 커서) 이름과 아이콘 주소의 v를 함께 올린다. 그래야 설치된 앱도 새 worker·파일을 받는다.
const OFFLINE_CACHE = "aaa-offline-v2";
const OFFLINE_URL = "/offline.html";

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches
      .open(OFFLINE_CACHE)
      .then((c) => c.add(new Request(OFFLINE_URL, { cache: "reload" })))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== OFFLINE_CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (e) => {
  if (e.request.mode !== "navigate") return;
  e.respondWith(fetch(e.request).catch(() => caches.match(OFFLINE_URL)));
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
 * 앱을 띄운 창인지(herdr-web-ui sw.js isAppWindow). host API(`/api/`)가 연 파일·결과물 탭과 함께 보내는 파일
 * (아이콘·manifest·sw.js·offline.html)은 앱 창이 아니다: 마지막 부분에 확장자가 있으면 파일이다(index.html 빼고).
 */
function isAppWindow(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  const path = parsed.pathname;
  if (path.startsWith("/api/")) return false;
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
