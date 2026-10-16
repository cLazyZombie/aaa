// aaa service worker(§8.26): 홈 화면 앱(PWA)과 Web Push 알림.
// - 화면·API·자산을 캐시하지 않는다. 업그레이드 뒤에도 늘 host가 주는 지금 화면을 연다.
// - 화면 이동(navigate)만 네트워크가 안 될 때 미리 둔 「연결 없음」 화면을 보인다.
const OFFLINE_CACHE = "aaa-offline-v1";
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

self.addEventListener("push", (e) => {
  let d = {};
  try {
    d = e.data ? e.data.json() : {};
  } catch {
    d = { body: e.data ? e.data.text() : "" };
  }
  e.waitUntil(
    self.registration.showNotification(d.title || "aaa", {
      body: d.body || "",
      tag: d.tag || "aaa",
      renotify: true,
      icon: "/icon-192.png",
      badge: "/favicon-32.png",
      data: { url: d.url || "/" },
    }),
  );
});

// 알림을 누르면 열린 aaa 창으로 가서 그 thread를 보인다. 창이 없으면 새로 연다.
self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const url = new URL((e.notification.data && e.notification.data.url) || "/", self.location.origin);
  e.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const win = wins.find((c) => new URL(c.url).origin === url.origin);
      if (win) {
        await win.focus();
        win.postMessage({ type: "open", hash: url.hash });
        return;
      }
      await self.clients.openWindow(url.href);
    })(),
  );
});
