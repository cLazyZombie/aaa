// herdr-web-ui(aa624cb) src/lib/phone.ts에서 세 함수만 그대로 옮겼다: isLoopbackHost·deviceLabel·takePairCode. 휴대폰 주소 판단(phonePlan)은
// 참고의 RemoteAccess 대신 aaa PublishState를 읽어 web/src/PhonePanel.tsx에 있다. 고지는 README.md
export function isLoopbackHost(hostname: string): boolean {
  return hostname === "localhost" || hostname.endsWith(".localhost") || hostname.startsWith("127.") || hostname === "[::1]" || hostname === "::1";
}

/** A name for this device to pair under, from its user agent: "iPhone · Safari", "Windows · Chrome". */
export function deviceLabel(userAgent: string, maxTouchPoints = 0): string {
  const ua = userAgent;
  const device = /iPhone/.test(ua) ? "iPhone" : /iPad/.test(ua) || (/Macintosh/.test(ua) && maxTouchPoints > 1) ? "iPad" : /Android/.test(ua) ? "Android" : /Macintosh/.test(ua) ? "Mac" : /Windows/.test(ua) ? "Windows" : /CrOS/.test(ua) ? "Chromebook" : /Linux/.test(ua) ? "Linux" : "Device";
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\/|Chromium\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : null;
  return browser ? `${device} · ${browser}` : device;
}

/** The `?pair=CODE` a QR code carries, taken off the address at once so it stays out of history and referrers. */
export function takePairCode(): string {
  if (typeof window === "undefined") return "";
  const url = new URL(window.location.href);
  const code = url.searchParams.get("pair") ?? "";
  if (code !== "") {
    url.searchParams.delete("pair");
    window.history.replaceState(window.history.state, "", url.pathname + url.search + url.hash);
  }
  return code.replace(/\D/g, "").slice(0, 6);
}
