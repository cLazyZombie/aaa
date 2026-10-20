// 보고서 그림 크게 보기(plan §8.353, 사용자 10-20 「시안·보고서 이미지를 눌러도 안 커진다」). 보고서 index.html은 opaque origin
// sandbox iframe이라 host가 HTML 탐색 응답 끝에 붙인 그림 누름 알림만이 앱에 누른 그림을 알린다. 데스크톱(Chromium)·휴대폰(WebKit
// iPhone)에서 보고서 안 그림을 눌러 공용 크게 보기(Lightbox)로 열고, 넘기고(←→·단추·옆으로 쓸기), 키우고(두 번 누름·두 손가락),
// 닫는다(Esc·바깥 누름·아래로 쓸기·휴대폰 뒤로). 원본 파일·Range·내려받기·fetch는 원본 바이트 그대로이고, 보고서 제 script나 다른
// 창이 흉내 낸 알림, 그 보고서 밖 주소는 열지 않는다. 휴대폰 손짓은 PointerEvent를 무대에 직접 보낸다: 실제 iOS 손가락이 어떤
// pointer 사건으로 오는지는 headless가 보이지 않는다(실기 확인).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 59-report-images(새 실험실).

import fs from "node:fs";
import http from "node:http";
import zlib from "node:zlib";
import { aaa, base, domIs, focusIs, inList, inTime, root, shot, step, until, webkit } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

/** 한 색 PNG(w×h): 그림마다 색이 달라 크게 보기에 어느 것이 떴는지 화면에서도 보인다 */
function png(w, h, [r, g, b]) {
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(zlib.crc32(Buffer.concat([Buffer.from(type), data])));
    return Buffer.concat([len, Buffer.from(type), data, crc]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0);
  head.writeUInt32BE(h, 4);
  head.set([8, 2, 0, 0, 0], 8);
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(w * 3).map((_, i) => [r, g, b][i % 3])]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

// 크게 볼 그림은 셋(a·sub/c·d)이다. 링크 안 그림(b)은 그 링크를 따르고, data: 그림은 보기 경로 밖이라 넘기지 않는다
const INDEX = `<!doctype html><html lang=ko><meta charset=utf-8><title>그림 보고</title>
<style>body{font:16px system-ui;margin:16px} img{display:block;margin:8px 0;max-width:100%}</style>
<h1>그림 보고</h1>
<img src="a.png" alt="첫 그림" width="240">
<a href="b.png"><img src="b.png" alt="링크 그림" width="80"></a>
<img src="sub/c.png" alt="둘째 그림" width="240">
<img src="data:image/png;base64,${png(4, 4, [0, 0, 0]).toString("base64")}" alt="데이터 그림" width="40">
<img src="d.png" alt="셋째 그림" width="240">
<button id="forge" type="button">흉내</button>
<script>
  // 보고서 제 script가 흉내 낸 알림: 보고서 밖 주소(앱 API)·다른 origin의 같은 경로·data: 그림. 앱은 열지 않는다
  document.getElementById("forge").onclick = (e) => {
    e.preventDefault();
    const send = (src, alt) => parent.postMessage({ type: "aaa:report-image", at: 0, images: [{ src, alt }] }, "*");
    send(location.origin + "/api/v1/me", "API");
    send("https://example.ts.net" + location.pathname.replace(/index\.html$/, "a.png"), "다른 origin");
    send(document.images[3].src, "data");
  };
</script>
</html>`;

/** 크게 보기 표지: 보는 자리·그림 수·배율·그림 주소 */
const box = (p) =>
  p.evaluate(() => {
    const l = document.querySelector('[data-testid="lightbox"]');
    const img = l?.querySelector('[data-testid="lightbox-img"]');
    return l ? { at: Number(l.dataset.at), count: Number(l.dataset.count), scale: l.dataset.scale, src: img?.getAttribute("src") ?? "", alt: img?.getAttribute("alt") ?? "", transform: img?.style.transform ?? "" } : null;
  });
const boxIs = (p, what, want) => domIs(p, what, (w) => {
  const l = document.querySelector('[data-testid="lightbox"]');
  if (w === null) return l === null;
  const src = l?.querySelector('[data-testid="lightbox-img"]')?.getAttribute("src") ?? "";
  return l !== null && Number(l.dataset.at) === w.at && Number(l.dataset.count) === w.count && (w.file === undefined || src.endsWith(`/${w.file}`)) && (w.scale === undefined || l.dataset.scale === w.scale);
}, want);

/**
 * 무대에 손가락 사건을 보낸다(touch PointerEvent). steps: [[사건, id, x, y, 대상]] — 대상 "img"면 그림에, 아니면 무대에. 좌표는 무대
 * 왼쪽 위 기준 px
 */
const fingers = (p, steps) =>
  p.evaluate((steps) => {
    const stage = document.querySelector('[data-testid="lightbox-stage"]');
    const r = stage.getBoundingClientRect();
    for (const [type, id, x, y, on] of steps) {
      const target = on === "img" ? stage.querySelector("img") : stage;
      target.dispatchEvent(new PointerEvent(type, { pointerId: id, pointerType: "touch", isPrimary: id === 1, clientX: r.left + x, clientY: r.top + y, button: 0, buttons: type === "pointerup" ? 0 : 1, bubbles: true, cancelable: true, composed: true }));
    }
  }, steps);

/** 보고서 iframe의 그림이 다 읽혔다(캡처·크게 보기 전) */
const imagesLoaded = (frame) =>
  until("보고서 그림 다 읽힘", () => frame.locator("img").evaluateAll((all) => all.length === 5 && all.every((i) => i.complete && i.naturalWidth > 0)));

/** 열린 크게 보기들의 기록(뜬 때마다 그림 수·주소). 흉내 낸 알림이 잠깐이라도 열었는지 본다 */
const watchOpens = (p) =>
  p.evaluate(() => {
    window.__lightboxOpens = [];
    new MutationObserver((records) => {
      for (const r of records)
        for (const n of r.addedNodes)
          if (n instanceof Element && n.matches('[data-testid="lightbox"]')) window.__lightboxOpens.push({ count: n.dataset.count, src: n.querySelector("img")?.getAttribute("src") });
    }).observe(document.body, { childList: true });
  });

export default async function ({ page }) {
  const dir = `${root}/pics`;

  await step("보고서 그림 누름 알림: host는 HTML 문서 탐색 응답에만 붙이고 원본 파일·Range·내려받기·fetch는 원본 바이트 그대로다", async () => {
    aaa("project", "create", "demo");
    fs.mkdirSync(`${dir}/sub`, { recursive: true });
    fs.writeFileSync(`${dir}/index.html`, INDEX);
    fs.writeFileSync(`${dir}/a.png`, png(320, 200, [220, 60, 60]));
    fs.writeFileSync(`${dir}/b.png`, png(80, 80, [60, 60, 220]));
    fs.writeFileSync(`${dir}/sub/c.png`, png(200, 320, [60, 180, 80]));
    fs.writeFileSync(`${dir}/d.png`, png(640, 400, [230, 170, 40]));
    const view = new URL(aaa("library", "publish", dir, "--project", "demo", "--name", "pics", "--to-user", "--title", "그림 보고").base, base).href;
    const original = fs.readFileSync(`${dir}/index.html`);
    // node:http: fetch는 sec-fetch-mode를 cors로 덮어 브라우저 탐색 머리를 흉내 내지 못한다
    const get = (path, headers = {}) =>
      new Promise((done, fail) => {
        http.get(new URL(path, view), { headers }, (r) => {
          const parts = [];
          r.on("data", (c) => parts.push(c));
          r.on("end", () => done({ status: r.statusCode, body: Buffer.concat(parts), length: r.headers["content-length"], csp: r.headers["content-security-policy"] ?? "" }));
          r.on("error", fail);
        }).on("error", fail);
      });
    const nav = { "sec-fetch-mode": "navigate", "sec-fetch-dest": "iframe" };
    // fetch(파일 보기의 소스 읽기)·Range·내려받기·그림은 원본
    for (const [what, r, want] of [
      ["fetch", await get("index.html"), original],
      ["Range", await get("index.html", { ...nav, range: "bytes=0-99" }), original.subarray(0, 100)],
      ["내려받기", await get("index.html?download=1", nav), original],
      ["그림", await get("a.png", nav), fs.readFileSync(`${dir}/a.png`)],
    ])
      if (!r.body.equals(want)) throw new Error(`${what}: 원본과 다르다(${r.status}, ${r.body.length}B)`);
    // iframe 탐색: 원본 뒤에 알림 script 하나, 길이도 맞고 sandbox CSP는 그대로다
    const framed = await get("index.html", nav);
    if (!framed.body.subarray(0, original.length).equals(original) || !framed.body.subarray(original.length).toString().startsWith("<script>"))
      throw new Error("iframe 탐색 응답이 원본 + 알림이 아니다");
    if (Number(framed.length) !== framed.body.length) throw new Error(`content-length ${framed.length} ≠ ${framed.body.length}`);
    if (!framed.csp.startsWith("sandbox allow-scripts") || framed.csp.includes("allow-same-origin")) throw new Error(`sandbox가 바뀌었다: ${framed.csp}`);
    const root = await get("", { ...nav, "sec-fetch-dest": "document" });
    if (!root.body.toString().includes('"aaa:report-image"')) throw new Error("문서 탐색(공유 링크)에 알림이 없다");
  });

  await step("데스크톱 보고서 그림: 눌러 크게 보고 ←→·단추로 넘기며 두 번 눌러 키우고, Esc·바깥 누름으로 닫는다. 흉내 낸 알림은 열지 않는다", async () => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await inList(page, "activity-reports");
    await page.getByTestId("reports-tab-mine").click();
    await page.getByTestId("report-demo-pics").getByRole("button", { name: "열기", exact: true }).click();
    const frame = page.frameLocator('[data-testid="report-frame"]');
    await frame.getByRole("img", { name: "셋째 그림" }).waitFor();
    await imagesLoaded(frame);
    await watchOpens(page);
    await shot(page, "59-report-desktop");
    // 흉내: 보고서 제 script(보고서 밖 주소·다른 origin·data:)와 앱 창 자신(그 보고서 그림 주소)이 보낸 알림. 앱이 넷을 다 본 표지 뒤에
    // 연 것이 없다
    const marks = () => page.getByTestId("report-frame").evaluate((f) => ({ seen: Number(f.dataset.messages ?? 0), opened: Number(f.dataset.opened ?? 0) }));
    const was = await marks();
    await frame.getByRole("button", { name: "흉내" }).click();
    await page.evaluate(() => {
      const src = new URL("a.png", document.querySelector('[data-testid="report-frame"]').src).href;
      window.postMessage({ type: "aaa:report-image", at: 0, images: [{ src, alt: "앱 창" }] }, "*");
    });
    await domIs(page, "흉내 낸 알림 넷을 봄", (n) => Number(document.querySelector('[data-testid="report-frame"]')?.dataset.messages ?? 0) >= n, was.seen + 4);
    const forged = await marks();
    if (forged.opened !== was.opened || (await box(page)) !== null) throw new Error(`흉내 낸 알림이 크게 보기를 열었다: ${JSON.stringify(forged)}`);
    await frame.getByRole("img", { name: "둘째 그림" }).click();
    await boxIs(page, "둘째 그림 크게", { at: 1, count: 3, file: "sub/c.png", scale: "1.00" });
    const opens = await page.evaluate(() => window.__lightboxOpens);
    if (opens.length !== 1 || opens[0].count !== "3" || (await marks()).opened !== was.opened + 1) throw new Error(`크게 보기 기록: ${JSON.stringify(opens)}`);
    await page.getByTestId("lightbox-img").evaluate((i) => i.decode());
    await shot(page, "59-lightbox-desktop");
    // 누른 곳은 iframe 안이다: 크게 보기가 focus를 가져온 뒤의 키는 앱이 받는다
    await focusIs(page, "크게 보기 닫기", '[data-testid="lightbox-close"]');
    await page.keyboard.press("ArrowRight");
    await boxIs(page, "→ 셋째 그림", { at: 2, count: 3, file: "d.png" });
    if ((await page.getByTestId("lightbox-next").count()) !== 0) throw new Error("마지막 그림에 다음 단추");
    await page.getByTestId("lightbox-prev").click();
    await page.getByTestId("lightbox-prev").click();
    await boxIs(page, "앞 단추 둘 → 첫 그림", { at: 0, count: 3, file: "a.png" });
    // 두 번 누름: 그 자리를 키우고 다시 두 번 누르면 맞춘 크기
    await page.getByTestId("lightbox-img").dblclick();
    await boxIs(page, "두 번 눌러 키움", { at: 0, count: 3, scale: "2.50" });
    await shot(page, "59-lightbox-desktop-zoom");
    await page.getByTestId("lightbox-img").dblclick();
    await boxIs(page, "두 번 눌러 맞춤", { at: 0, count: 3, scale: "1.00" });
    // 그림 한 번 누름은 닫지 않는다. Esc가 닫고 보고서는 그대로
    await page.getByTestId("lightbox-img").click();
    if ((await box(page)) === null) throw new Error("그림을 누르니 닫혔다");
    await page.keyboard.press("Escape");
    await boxIs(page, "Esc 닫음", null);
    if ((await page.getByTestId("report-detail").count()) !== 1) throw new Error("Esc가 보고서까지 닫았다");
    // 바깥 누름
    await frame.getByRole("img", { name: "셋째 그림" }).click();
    await boxIs(page, "셋째 그림 크게", { at: 2, count: 3, file: "d.png" });
    await page.mouse.click(30, 780);
    await boxIs(page, "바깥 눌러 닫음", null);
  });

  await step("휴대폰(WebKit iPhone) 보고서 그림: 눌러 열고 쓸어 넘기며 두 손가락으로 키워 끌고, 뒤로·아래로 쓸기가 크게 보기만 닫는다", async () => {
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: "#report=demo%2Fpics" });
      const frame = p.frameLocator('[data-testid="report-frame"]');
      await frame.getByRole("img", { name: "첫 그림" }).waitFor();
      // headless WebKit은 sandbox 속성과 CSP sandbox 머리가 겹친 iframe 문서의 `img-src 'self'` 그림을 그리지 않는다(main도 같다,
      // plan §8.353 「남은 것」). 그래서 iframe 안 그림이 읽히기를 기다리지 않는다: 크게 보기는 앱 문서에서 그 주소를 읽는다
      await frame.getByRole("img", { name: "셋째 그림" }).waitFor();
      await shot(p, "59-report-phone");
      const hash = await p.evaluate(() => location.hash);
      await frame.getByRole("img", { name: "첫 그림" }).tap();
      await boxIs(p, "첫 그림 크게", { at: 0, count: 3, file: "a.png", scale: "1.00" });
      await p.getByTestId("lightbox-img").evaluate((i) => i.decode());
      await shot(p, "59-lightbox-phone");
      // 왼쪽으로 쓸면 다음, 오른쪽이면 앞. 짧게 끈 것은 그대로
      await fingers(p, [["pointerdown", 1, 300, 400], ["pointermove", 1, 200, 405], ["pointermove", 1, 120, 410], ["pointerup", 1, 120, 410]]);
      await boxIs(p, "왼쪽 쓸기 → 둘째", { at: 1, count: 3, file: "sub/c.png" });
      await fingers(p, [["pointerdown", 1, 200, 400], ["pointermove", 1, 225, 400], ["pointerup", 1, 225, 400]]);
      await fingers(p, [["pointerdown", 1, 80, 400], ["pointermove", 1, 200, 400], ["pointermove", 1, 300, 400], ["pointerup", 1, 300, 400]]);
      await boxIs(p, "오른쪽 쓸기 → 첫째", { at: 0, count: 3, file: "a.png" });
      // 두 손가락 벌리기: 90px → 190px(약 2.11배). 끈 뒤 한 손가락으로 옮긴다
      await fingers(p, [["pointerdown", 1, 150, 400, "img"], ["pointerdown", 2, 240, 400, "img"], ["pointermove", 1, 120, 400], ["pointermove", 2, 270, 400], ["pointermove", 1, 100, 400], ["pointermove", 2, 290, 400]]);
      await boxIs(p, "두 손가락 확대", { at: 0, count: 3, scale: "2.11" });
      await fingers(p, [["pointerup", 2, 290, 400]]);
      const before = (await box(p)).transform;
      await fingers(p, [["pointermove", 1, 160, 400], ["pointerup", 1, 160, 400]]);
      const after = await box(p);
      if (after.scale !== "2.11" || after.transform === before) throw new Error(`한 손가락 끌기: ${before} → ${after.transform}`);
      await shot(p, "59-lightbox-phone-zoom");
      // 키운 동안 옆으로 끈 것은 옮기기다(넘기지 않는다)
      if (after.at !== 0) throw new Error("키운 채 끌기가 넘겼다");
      // 두 번 누름은 맞춘 크기로
      await fingers(p, [["pointerdown", 1, 195, 400, "img"], ["pointerup", 1, 195, 400, "img"], ["pointerdown", 1, 195, 400, "img"], ["pointerup", 1, 195, 400, "img"]]);
      await boxIs(p, "두 번 눌러 맞춤", { at: 0, count: 3, scale: "1.00" });
      // 휴대폰 뒤로는 크게 보기만 닫는다(보고서·주소 그대로)
      await inTime(p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); })), 20_000, "뒤로의 popstate");
      await boxIs(p, "뒤로 닫음", null);
      if ((await p.evaluate(() => location.hash)) !== hash || (await p.getByTestId("report-detail").count()) !== 1) throw new Error("뒤로가 보고서를 떠났다");
      // 아래로 쓸면 닫고, 그 기록 칸도 거둔다(다음 뒤로가 빈 칸에 걸리지 않는다)
      await frame.getByRole("img", { name: "셋째 그림" }).tap();
      await boxIs(p, "셋째 그림 크게", { at: 2, count: 3, file: "d.png" });
      await fingers(p, [["pointerdown", 1, 195, 300], ["pointermove", 1, 198, 380], ["pointermove", 1, 200, 460], ["pointerup", 1, 200, 460]]);
      await boxIs(p, "아래로 쓸어 닫음", null);
      await until("크게 보기 기록 칸 거둠", () => p.evaluate(() => !(history.state && typeof history.state === "object" && "aaa:sheet" in history.state)));
      if ((await p.evaluate(() => location.hash)) !== hash) throw new Error("쓸어 닫기가 주소를 바꿨다");
      // 닫기 단추
      await frame.getByRole("img", { name: "둘째 그림" }).tap();
      await boxIs(p, "둘째 그림 크게", { at: 1, count: 3, file: "sub/c.png" });
      await p.getByTestId("lightbox-close").tap();
      await boxIs(p, "닫기 단추", null);
    } finally {
      await wk.close();
    }
  });
}
