// 웹 E2E thread와 알림: Web Push·보고서·이 기기 알림·보는 동안·두 기기·끝난 thread·사후 평가자.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 12-threads-push(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import {
  chromium, root, aaa, start, stop, until, domIs, hashIs, screenText, shot, pairPage, pushes, pushPort, pushOrigin,
  b64u, pushDevice, nextPush, pushKey, base, step, browser, setPushKey, openSettings, focusIs, waitOutput,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  const pushDev = pushDevice("push/dev1");
  const goneDev = pushDevice("gone/dev2");
  await step("Web Push: 구독 검사 → 시험 알림이 암호화·서명돼 온다 → service worker가 알림을 띄운다", async () => {
    const api = (path, body) =>
      page.evaluate(
        async ({ path, body }) => {
          const r = await fetch(path, body ? { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) } : {});
          return { status: r.status, json: await r.json() };
        },
        { path, body },
      );
    const info = await api("/api/v1/push");
    if (!info.json.ok?.enabled || info.json.ok.subscribed !== 0) throw new Error(JSON.stringify(info.json));
    setPushKey(info.json.ok.key);
    if (Buffer.from(pushKey, "base64url").length !== 65) throw new Error(`VAPID 공개키 ${pushKey}`);
    // 허용하지 않은 곳·loopback이라도 목록에 없는 이름·틀린 키는 받지 않는다
    for (const bad of [
      { ...pushDev.json, endpoint: "https://evil.example/push" },
      { ...pushDev.json, endpoint: `http://localhost:${pushPort}/x` },
      { ...pushDev.json, keys: { ...pushDev.json.keys, p256dh: b64u(Buffer.alloc(65, 1)).slice(0, 40) } },
    ]) {
      const r = await api("/api/v1/push/subscribe", bad);
      if (r.status !== 400) throw new Error(`거부하지 않았다: ${JSON.stringify(bad)} → ${r.status}`);
    }
    if ((await api("/api/v1/push/subscribe", pushDev.json)).status !== 200) throw new Error("구독 실패");
    const from = pushes.length;
    const t = await api("/api/v1/push/test", {});
    if (t.json.ok?.sent !== 1) throw new Error(JSON.stringify(t.json));
    const { msg, claims } = await nextPush(pushDev, from, "시험 알림");
    if (msg.title !== "aaa" || msg.body !== "알림이 켜졌다") throw new Error(JSON.stringify(msg));
    const now = Date.now() / 1000;
    if (claims.aud !== pushOrigin || claims.exp < now || claims.exp > now + 24 * 3600 || !/^(mailto:|https:)/.test(claims.sub)) {
      throw new Error(`claims ${JSON.stringify(claims)}`);
    }
    // 끄면 지워진다. 보고 알림을 받을 기기(dev1)와 브라우저가 버린 구독(dev2, 410)을 둔다.
    if (!(await api("/api/v1/push/unsubscribe", { endpoint: pushDev.json.endpoint })).json.ok?.removed) throw new Error("구독 해제");
    if ((await api("/api/v1/push")).json.ok.subscribed !== 0) throw new Error("해제 뒤에도 구독이 남았다");
    for (const d of [pushDev, goneDev]) {
      if ((await api("/api/v1/push/subscribe", d.json)).status !== 200) throw new Error("다시 구독 실패");
    }
    // service worker: push를 받으면 알림을 띄운다(push 서비스 없이 CDP로 직접 넣는다).
    // headless shell은 알림을 지원하지 않아 이 부분만 정식 Chromium(새 headless)으로 한다.
    const full = await chromium.launch({ channel: "chromium" });
    try {
      // 이 기기 알림은 꺼 둔다: 권한이 있으면 열 때마다 구독을 다시 등록하는데(참고), 그러면 정식 Chromium이 실제 push 서비스에 닿는다
      const mob = await pairPage(full, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, {
        init: () => {
          try {
            localStorage.setItem("aaa:alerts-on", "0");
          } catch {}
        },
      });
      const ctx = mob.page.context();
      await ctx.grantPermissions(["notifications"], { origin: base.slice(0, -1) });
      await mob.page.reload();
      await mob.page.getByTestId("machine-labA").waitFor();
      // 알림 켜기는 머리 「더 보기」 메뉴의 「알림」이다(참고 App.tsx moreItems). 보기 권한 기기도 알림은 받는다. 꺼져 있으면 메뉴 단추에 점
      await mob.page.getByTestId("more").click();
      await until("알림 항목", async () => (await mob.page.getByTestId("alerts-item").innerText()).includes("이 기기에서 꺼짐"));
      if ((await mob.page.getByTestId("more").getAttribute("aria-label")) !== "더 보기 · 알림 꺼짐") throw new Error("꺼진 알림 표시가 없다");
      await mob.page.keyboard.press("Escape");
      const cdp = await ctx.newCDPSession(mob.page);
      const regId = new Promise((r) =>
        cdp.on("ServiceWorker.workerRegistrationUpdated", (e) => {
          const reg = e.registrations.find((x) => x.scopeURL === base && !x.isDeleted);
          if (reg) r(reg.registrationId);
        }),
      );
      // 실패하면 service worker 판 상태를 남긴다(10-17 22:20 전체 실행에서 한 번 알림이 뜨지 않았고 따로는 재현되지 않았다)
      const versions = [];
      cdp.on("ServiceWorker.workerVersionUpdated", (e) => versions.push(...e.versions.map((v) => `${v.versionId}:${v.status}/${v.runningStatus}`)));
      cdp.on("ServiceWorker.workerErrorReported", (e) => versions.push(`error:${e.errorMessage.errorMessage}`));
      await cdp.send("ServiceWorker.enable");
      const data = JSON.stringify({ title: "aaa · demo", body: "t-0009 question: 확인창", url: "/#thread=demo%2Ft-0009", tag: "aaa-demo" });
      // 활성화된 worker에만 보낸다: 설치·활성화 중에 넣은 push는 사건 없이 버려질 수 있다(10-17 두 번 난 「알림 표시」 실패의 후보)
      await until("service worker 활성", () => mob.page.evaluate(async () => (await navigator.serviceWorker.getRegistration())?.active?.state === "activated"));
      const atDeliver = versions.slice(-4).join(",");
      await cdp.send("ServiceWorker.deliverPushMessage", { origin: base.slice(0, -1), registrationId: await regId, data });
      const shown = await until("알림 표시", async () => {
        const v = await mob.page.evaluate(async () =>
          (await (await navigator.serviceWorker.ready).getNotifications()).map((n) => [n.title, n.body, n.data?.url]),
        );
        return v.length ? v : null;
      }).catch(async (e) => {
        const state = await mob.page.evaluate(async () => {
          const reg = await navigator.serviceWorker.getRegistration();
          return {
            permission: Notification.permission,
            controller: !!navigator.serviceWorker.controller,
            active: reg?.active?.state ?? null,
            waiting: reg?.waiting?.state ?? null,
            installing: reg?.installing?.state ?? null,
            shown: reg ? (await reg.getNotifications()).length : null,
          };
        });
        throw new Error(`${e.message}: ${JSON.stringify(state)} 판 ${versions.slice(-8).join(",")} 보낼 때 판 ${atDeliver} 오류 ${mob.errors.join("; ")}`);
      });
      if (JSON.stringify(shown[0]) !== JSON.stringify(["aaa · demo", "t-0009 question: 확인창", "/#thread=demo%2Ft-0009"])) {
        throw new Error(JSON.stringify(shown));
      }
      if (mob.errors.length) throw new Error(`페이지 오류: ${mob.errors.join("; ")}`);
    } finally {
      await full.close();
    }
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  const pushFrom = pushes.length;
  // 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다(herdr-web-ui presence, plan §8.69). headless 화면은 늘 focus가
  // 있으므로, 보고 알림이 기기에 가는 것을 볼 동안 이 화면은 쓰지 않는 것으로 둔다(focus만 없앤다)
  const setInUse = (p, on) =>
    p.evaluate((on) => {
      if (on) delete document.hasFocus;
      else document.hasFocus = () => false;
      window.dispatchEvent(new Event(on ? "focus" : "blur"));
    }, on);
  const pushInUse = async () => (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use));
  /** 그 화면 기기가 본 쓰는 기기(plan §8.105): `user`(앞 7자)·`you` */
  const pushWho = (p) => p.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok);
  await step("thread 보고서·결과물: Markdown 보고, sandbox 결과물, agent에 남은 큰 파일, ack", async () => {
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "웹 보기");
    const t = aaa("thread", "start", "demo", "--title", "웹 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir",
      "--task-file", `${root}/task.md`);
    await until("보고 회수", () => aaa("thread", "show", "demo", t.id).report_hash);
    const item = page.getByTestId(`thread-demo-${t.id}`);
    await until("thread 목록에 검토 대기", async () => (await item.innerText()).includes("검토 대기"));
    // thread 줄 표시는 그 thread pane의 agent(참고 Sidebar.tsx rowMark): 표시가 없는 profile(fake)은 머리글자 표시
    const tm = await item.locator(".sidebar-mark .agent-mark").evaluate((a) => `${a.className}|${a.textContent}`);
    if (tm !== "agent-mark fake|F") throw new Error(`thread 줄 표시 ${tm}`);
    // thread 줄은 그 agent pane을 열고(차이 목록 L4, herdr-projects: thread는 자기 agent 줄) 보고서는 터미널 머리 「보고」로
    await item.click();
    await page.getByTestId("open-report").click();
    const report = page.getByTestId("report");
    await until("보고 Markdown", async () => (await report.locator("strong").first().innerText()) === "확인");
    // herdr-web-ui Markdown대로 그린다(차이 목록 R1): 표·굵게 안 code·링크·번호 목록 안 목록·code block
    const md = await report.locator(".markdown").evaluate((m) => ({
      firstCode: m.querySelector("li code")?.textContent,
      strongCode: m.querySelector("strong > code")?.textContent,
      link: [m.querySelector("a[href='https://example.com/doc']")?.textContent, m.querySelector("a[href='https://example.com/doc']")?.target],
      head: [...m.querySelectorAll(".markdown-table-wrap table thead th")].map((x) => x.textContent),
      cells: [...m.querySelectorAll(".markdown-table-wrap table tbody tr")].map((r) => [...r.children].map((c) => c.innerHTML.replace(/<\/?span>/g, ""))),
      nested: m.querySelector("ol > li > ul > li code")?.textContent,
      ol: [...m.querySelectorAll(":scope > ol > li")].length,
      fence: [m.querySelector(".markdown-code")?.dataset.language, m.querySelector(".markdown-code pre code")?.textContent],
      raw: /\*\*|\|/.test(m.textContent),
      // code block 안 code는 바탕을 따로 갖지 않는다(참고 `.markdown :not(pre) > code`만 칩)
      fenceBg: getComputedStyle(m.querySelector(".markdown-code pre code")).backgroundColor,
    }));
    const want = {
      firstCode: "index.html",
      strongCode: "a1b2c3d",
      link: ["참고 문서", "_blank"],
      head: ["ID", "판정", "근거"],
      cells: [
        ["<code>T2</code>", "같음", "<code>prefs.rs</code>"],
        ["R1", "<strong>일부</strong>", "표와 <code>code</code>"],
      ],
      nested: "하나",
      ol: 2,
      fence: ["sh", "cargo test -p aaa"],
      raw: false,
      fenceBg: "rgba(0, 0, 0, 0)",
    };
    if (JSON.stringify(md) !== JSON.stringify(want)) throw new Error(`보고 Markdown ${JSON.stringify(md)}`);
    const frameEl = await page.getByTestId("library-frame").elementHandle();
    const frame = await until("결과물 frame", async () => frameEl && (await frameEl.contentFrame()));
    const probe = await until("결과물 HTML 실행", async () => {
      const d = await frame.evaluate(() => ({
        title: document.getElementById("t")?.textContent,
        pic: document.getElementById("pic")?.naturalWidth,
        ...document.body.dataset,
      }));
      return d.api ? d : null;
    });
    if (probe.title !== "결과 보고" || probe.pic !== 1) throw new Error(`결과물 표시: ${JSON.stringify(probe)}`);
    // opaque origin: 부모 문서·cookie·제어 API에 닿지 못한다
    if (probe.parent !== "blocked" || probe.cookie === "visible" || probe.api === "200") {
      throw new Error(`결과물이 격리되지 않았다: ${JSON.stringify(probe)}`);
    }
    const big = page.getByTestId("files").locator("li", { hasText: "big.bin" });
    if (!(await big.innerText()).includes("agent에서 읽음")) throw new Error(await big.innerText());
    const href = await big.locator("a").getAttribute("href");
    const n = await page.evaluate(async (u) => (await (await fetch(u)).arrayBuffer()).byteLength, href);
    if (n !== 2097152) throw new Error(`agent 파일 길이 ${n}`);
    await shot(page, "desktop-thread-library");
    // 휴대폰 폭: 넓은 표는 그 안에서만 옆으로 밀리고 화면은 밀리지 않는다(참고 .markdown-table-wrap)
    await page.setViewportSize({ width: 390, height: 844 });
    await report.locator(".markdown-table-wrap").scrollIntoViewIfNeeded();
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await shot(page, "phone-report-markdown");
    await page.setViewportSize({ width: 1280, height: 800 });
    // 알림을 보내기 전에 확인하면 그 알림은 빠진다(plan §8.69): 다음 단계가 볼 보고 알림이 나간 뒤에 확인한다
    await until("보고 알림 보냄", () => pushes.slice(pushFrom).some((x) => x.path === new URL(pushDev.json.endpoint).pathname));
    await page.getByRole("button", { name: "확인(ack)" }).click();
    await until("ack 뒤 검토 대기 아님", async () => !(await item.innerText()).includes("검토 대기"));
    if (aaa("thread", "show", "demo", t.id).acked_hash !== aaa("thread", "show", "demo", t.id).report_hash) {
      throw new Error("ack가 기록되지 않았다");
    }
    // 다음 단계는 t1 터미널을 본다
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("Web Push: 보고 준비 알림이 기기에 오고(제목·자리는 그 thread) 그 thread로 연다, 버려진 구독(410)은 지운다, 쓰는 화면이 있으면 어느 기기에도 보내지 않는다(차이 목록 N5·N6)", async () => {
    const { msg } = await nextPush(pushDev, pushFrom, "보고 알림");
    // 한 thread 알림: 제목은 「프로젝트 · thread 제목」, 자리(tag)는 그 thread(차이 목록 N6)
    if (msg.title !== "demo · 웹 결과물" || msg.tag !== "aaa-demo/t-0001" || !msg.body.includes("new report") || msg.url !== "/#thread=demo%2Ft-0001") {
      throw new Error(JSON.stringify(msg));
    }
    await until("410 구독 정리", async () =>
      (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed)) === 1,
    );
    const kinds = aaa("events", "demo").events.map((e) => e.kind);
    if (!kinds.includes("push_sent") || !kinds.includes("push_gone")) throw new Error(kinds.join(","));
    // 구독 주소(기기에 보내는 열쇠)는 기록에 남지 않는다
    if (JSON.stringify(aaa("events", "demo")).includes("dev1")) throw new Error("흐름 사건에 endpoint가 남았다");
    // 화면을 다시 쓰면(보이고 focus, 화면을 켰으니 이 기기가 쓰는 기기) 새 보고가 나와도 어느 기기에도 보내지 않는다
    // (herdr-web-ui push.ts inUse, 차이 목록 N5. 쓰는 기기는 plan §8.105)
    await setInUse(page, true);
    await until("쓰는 화면", async () => (await pushInUse()) === 1);
    const from = pushes.length;
    fs.writeFileSync(`${root}/task2.md`, "보는 중 보고");
    const t2 = aaa("thread", "start", "demo", "--title", "보는 중", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task2.md`);
    await until("보는 중 보고 회수", () => aaa("thread", "show", "demo", t2.id).report_hash);
    const skipped = await until("보내지 않음 기록", () =>
      aaa("events", "demo").events.find((e) => e.kind === "push_skipped" && / · 쓰는 화면 1개\(/.test(e.detail)),
    );
    if (pushes.slice(from).some((x) => x.path === new URL(pushDev.json.endpoint).pathname)) throw new Error(`쓰는 화면이 있는데 보냈다 ${skipped.detail}`);
    // 보내지 않은 대신 쓰는 화면에는 앱 안 알림이 내려온다(plan §8.70)
    await until("쓰는 화면의 앱 안 알림", async () => (await page.locator(".droplet-title").allInnerTexts()).includes("보는 중"));
    // 화면을 닫으면(연결이 끊기면) 쓰는 화면이 아니다
    const extra = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await until("둘째 화면도 씀", async () => (await pushInUse()) === 2);
    await extra.context().close();
    await until("닫은 화면은 빠짐", async () => (await pushInUse()) === 1);
    aaa("thread", "ack", "demo", t2.id);
  });

  await step("이 기기 알림(herdr-web-ui Droplet·alertSound·notifications·PushTestControls·더 보기, 차이 목록 N1·N2·N8·N9): 새 보고에 앱 안 알림이 내려오고 누르면 그 thread, 소리는 켜면 미리 듣고 알림마다, 가려지면 탭 알림, 앱 안을 끄면 안 내려옴, 메뉴로 끄면 아무것도, 시험 보내기는 구독이 없다고", async () => {
    // 소리·탭 알림·권한·push 구독은 기록하는 것으로 바꾼다(실제 소리·알림·push 서비스 없이)
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__alertLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        constructor(t) {
          log.push(`new ${t}`);
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async function (t, o) {
        log.push(`tab ${t}|${o.tag}|${o.data?.url}`);
      };
      PushManager.prototype.subscribe = async () => {
        throw new DOMException("시험에서는 push 서비스가 없다", "AbortError");
      };
      PushManager.prototype.getSubscription = async () => null;
      class FakeAudio {
        state = "suspended";
        destination = {};
        // 소리 하나가 울리는 동안은 같은 소리를 겹쳐 울리지 않는다(참고 chime): 시계가 흘러야 한다
        get currentTime() {
          return performance.now() / 1000;
        }
        constructor() {
          window.__evLog?.push("ac new");
        }
        async resume() {
          window.__evLog?.push(`ac resume ${this.state}`);
          this.state = "running";
        }
        createOscillator() {
          const o = {
            type: "",
            frequency: { value: 0 },
            connect: (g) => g,
            start: (at) => {
              log.push(`tone ${o.frequency.value}`);
              // 이 소리가 끝나는 때(참고 NOTE_LENGTH_S 0.3초)
              window.__chimeEnd = Math.max(window.__chimeEnd ?? 0, at + 0.3);
              window.__evLog?.push(`osc ${o.frequency.value} at ${at.toFixed(2)} now ${this.currentTime.toFixed(2)}`);
            },
            stop() {},
          };
          return o;
        }
        createGain() {
          return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: (d) => d };
        }
      }
      Object.defineProperty(window, "AudioContext", { configurable: true, value: FakeAudio });
      // iPhone·iPad Safari의 소리 범주(Audio Session API, plan §8.83): 바꿀 때마다 남긴다
      let sessionType = "auto";
      const session = {
        get type() {
          return sessionType;
        },
        set type(v) {
          sessionType = v;
          log.push(`session ${v}`);
        },
      };
      Object.defineProperty(navigator, "audioSession", { configurable: true, value: session });
      // 진단: 상태 사건 WS의 열림·닫힘과 thread 사건(보고 hash)
      const ev = [];
      Object.defineProperty(window, "__evLog", { value: ev });
      new MutationObserver((ms) => {
        for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("droplet")) ev.push(`drop ${n.dataset.kind}`);
      }).observe(document, { childList: true, subtree: true });
      addEventListener("pagehide", () => ev.push("pagehide"));
      const RealBC = window.BroadcastChannel;
      window.BroadcastChannel = class extends RealBC {
        postMessage(m) {
          ev.push(`bc ${m?.type} @${(performance.now() / 1000).toFixed(2)}`);
          super.postMessage(m);
        }
      };
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          const t0 = performance.now();
          const at = () => Math.round(performance.now() - t0);
          this.addEventListener("open", () => ev.push(`open`));
          this.addEventListener("close", (e) => ev.push(`close ${e.code} ${at()}`));
          this.addEventListener("message", (m) => {
            try {
              const d = JSON.parse(m.data);
              if (d.ev === "thread") ev.push(`thr ${d.id} ${String(d.report_hash).slice(0, 6)} ${String(d.acked_hash).slice(0, 6)} @${(performance.now() / 1000).toFixed(2)}`);
            } catch {}
          });
        }
      };
    };
    const apOpen = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init });
    const ap = apOpen.page;
    const log = () => ap.evaluate(() => window.__alertLog.join(","));
    const clear = () => ap.evaluate(() => void window.__alertLog.splice(0));
    // 기록을 기다린다. 실패하면 그때의 기록·고른 자리·소리 상태를 남긴다
    const waitLog = (what, ok) =>
      until(what, async () => ok(await log())).catch(async (e) => {
        const at = await ap.evaluate(() => ({ hash: location.hash, vis: document.visibilityState, sound: localStorage.getItem("aaa:alert-sound") }));
        const ev = await ap.evaluate(() => window.__evLog.slice(-16).join(","));
        throw new Error(`${e.message}: 기록 [${await log()}] ${JSON.stringify(at)} 앱 안 ${await droplet.count()} 오류 [${apOpen.errors.join("; ")}] 사건 [${ev}] 보고 ${String(aaa("thread", "show", "demo", t.id).report_hash).slice(0, 6)}`);
      });
    const droplet = ap.locator(".droplet");
    // 들어오는 움직임(참고 Droplet.css)이 끝난 모습
    const landed = (d) => d.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
    fs.writeFileSync(`${root}/task3.md`, "알림 시험");
    const t = aaa("thread", "start", "demo", "--title", "알림 시험", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task3.md`);
    const target = `#thread=demo%2F${t.id}`;
    // 첫 보고: 앱 안 알림(작업 완료). 누르면 그 thread로 가고 접힌다
    await until("앱 안 알림", async () => (await droplet.count()) === 1 && (await droplet.locator(".droplet-title").innerText()) === "알림 시험");
    if ((await droplet.getAttribute("data-kind")) !== "done" || (await droplet.locator(".droplet-detail").innerText()) !== "작업 완료") {
      throw new Error(`앱 안 알림 ${await droplet.getAttribute("data-kind")} ${await droplet.locator(".droplet-detail").innerText()}`);
    }
    await landed(droplet);
    await shot(ap, "desktop-droplet");
    await droplet.locator(".droplet-card").click();
    await until("누르면 그 thread", async () => (await ap.evaluate(() => location.hash)) === target);
    await until("접힘", async () => (await droplet.count()) === 0);
    const work = aaa("thread", "show", "demo", t.id).work_dir;
    let n = 0;
    // 새 보고를 낸다. 돌려주는 hash로 이 화면이 그 thread 사건을 받았는지 본다(evLog `thr`)
    const report = async (what, { chime = true } = {}) => {
      // 앞 소리가 울리는 동안 온 「완료」는 그 소리가 이미 알렸다고 보고 겹쳐 울리지 않는다(참고 alertSound.ts chime).
      // 알림마다 소리를 보려면 앞 소리가 끝난 뒤에 다음 보고를 낸다
      if (chime) await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
      const before = aaa("thread", "show", "demo", t.id).report_hash;
      fs.writeFileSync(`${work}/report.md`, `## Report\n- ${what} ${++n}\n`);
      return await until(`${what} 보고`, () => {
        const h = aaa("thread", "show", "demo", t.id).report_hash;
        return h !== before && h;
      });
    };
    // 이 화면이 그 보고 사건을 받아 처리하고(앱 처리기는 진단 기록 뒤에 돈다) 두 번 그린 뒤
    const handled = async (hash) => {
      await until("보고 사건 받음", () => ap.evaluate((h) => window.__evLog.some((e) => e.startsWith("thr ") && e.includes(` ${h.slice(0, 6)} `)), hash));
      await ap.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)))));
    };
    const dropsSince = (i) => ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("drop")).length, i);
    const evMark = () => ap.evaluate(() => window.__evLog.length);
    // 그 thread를 보고 있으면 띄우지 않는다: 다른 곳을 본다
    await ap.getByTestId("pane-t1").click();
    // 소리: 켜는 누름이 미리 듣기(작업 완료 음 880→660), 그 뒤 알림마다
    const dialog = await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "소리" }).click();
    await waitLog("미리 듣기", (l) => l === "tone 880,tone 660");
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-sound"))) !== "1") throw new Error("소리 설정이 저장되지 않았다");
    await clear();
    await report("소리");
    await waitLog("알림 소리", (l) => l === "tone 880,tone 660");
    await until("소리와 함께 앱 안 알림", async () => (await droplet.count()) === 1);
    // 무음 모드에서도(사용자 보고 10-18 iPhone, plan §8.83): 끈 동안은 소리 범주를 건드리지 않는다(위 기록에 session이 없다).
    // 켜는 누름이 미리 듣기이고, 소리를 낼 때만 "playback"(무음 스위치를 무시)으로 두었다가 끝나면 "auto"로 되돌린다
    await openSettings(ap, "알림");
    const silent = dialog.getByRole("switch", { name: "무음 모드에서도" });
    await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
    await clear();
    await silent.click();
    await waitLog("무음에서도 미리 듣기", (l) => l === "session playback,tone 880,tone 660,session auto");
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-silent"))) !== "1") throw new Error("무음 모드 설정이 저장되지 않았다");
    await until("앞 알림 접힘", async () => (await droplet.count()) === 0);
    await clear();
    await report("무음에서도");
    await waitLog("무음에서도 알림 소리", (l) => l === "session playback,tone 880,tone 660,session auto");
    await openSettings(ap, "알림");
    await silent.click();
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-silent"))) !== null) throw new Error("끈 무음 모드 설정이 남았다");
    // 가려지면: 앱 안 알림은 없고, 소리와 탭 알림(이 기기에 push 구독이 없다)
    const hide = (h) =>
      ap.evaluate((h) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (h ? "hidden" : "visible") });
        Object.defineProperty(document, "hidden", { configurable: true, get: () => h });
        document.dispatchEvent(new Event("visibilitychange"));
      }, h);
    await until("앞 알림 접힘", async () => (await droplet.count()) === 0);
    await clear();
    await hide(true);
    await report("가려짐");
    await waitLog("탭 알림", (l) => l.includes(`tab demo · 알림 시험|aaa-demo/${t.id}|/${target}`));
    // 소리는 탭끼리 누가 울릴지 정하는 짧은 틈(참고 alertTurns WINDOW_MS) 뒤에 울린다
    await waitLog("가려진 동안 소리", (l) => l.includes("tone 880,tone 660"));
    if ((await droplet.count()) !== 0) throw new Error(`가려진 동안 앱 안 알림 ${await log()}`);
    await hide(false);
    // 앱 안을 끄면 소리만
    await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await ap.keyboard.press("Escape");
    await clear();
    const offMark = await evMark();
    await handled(await report("앱 안 끔"));
    await waitLog("앱 안 끈 뒤 소리", (l) => l === "tone 880,tone 660");
    if ((await dropsSince(offMark)) !== 0 || (await droplet.count()) !== 0) throw new Error("앱 안을 껐는데 내려왔다");
    // 더 보기 → 알림: 권한이 있고 구독이 없으면 「이 탭에서 켜짐」. 누르면 꺼지고 메뉴 단추에 점
    await ap.getByTestId("more").click();
    const item = ap.getByTestId("alerts-item");
    if (!(await item.innerText()).includes("이 탭에서 켜짐") || (await item.getAttribute("aria-checked")) !== "true") throw new Error(`알림 항목 ${await item.innerText()}`);
    if ((await ap.getByTestId("more").getAttribute("aria-label")) !== "더 보기") throw new Error("켜진 알림인데 점");
    await shot(ap, "desktop-more-menu");
    await item.click();
    await until("메뉴 단추 점", async () => (await ap.getByTestId("more").getAttribute("aria-label")) === "더 보기 · 알림 꺼짐");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alerts-on"))) !== "0") throw new Error("끈 알림이 저장되지 않았다");
    // 끄면 아무것도: 이 화면이 그 사건을 받아 처리한 뒤에도 소리·소리 차례·앱 안 알림·탭 알림이 없다
    await clear();
    const quietMark = await evMark();
    await handled(await report("끔"));
    const turns = await ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("bc ")).length, quietMark);
    if ((await log()) !== "" || (await dropsSince(quietMark)) !== 0 || turns !== 0) throw new Error(`끈 뒤 [${await log()}] 소리 차례 ${turns}`);
    // 시험 보내기: 이 기기에 구독이 없다고 하고 「알림 다시 켜기」를 권한다
    await openSettings(ap, "알림");
    if ((await dialog.getByTestId("push-test-result").innerText()) !== "알림 꺼짐") throw new Error(`시험 줄 ${await dialog.getByTestId("push-test-result").innerText()}`);
    await shot(ap, "desktop-settings-alerts-off");
    await dialog.getByRole("button", { name: "알림 다시 켜기" }).click();
    await until("다시 켠 알림", async () => (await ap.getByTestId("more").getAttribute("aria-label")) === "더 보기");
    await dialog.getByRole("button", { name: "테스트 보내기" }).click();
    await until("구독 없음", async () => (await dialog.getByTestId("push-test-result").innerText()) === "이 기기의 푸시 구독이 없습니다. 알림을 다시 켜세요.");
    await ap.keyboard.press("Escape");
    await ap.context().close();
    // 휴대폰(390×844): 앱 안 알림과 「더 보기」 메뉴가 화면 안, 머리 아래에 있다(겹침·잘림 없음)
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const inside = (r, what) => {
      if (!r || r.left < 0 || r.right > 390 || r.top < 0 || r.bottom > 844) throw new Error(`${what}가 화면 밖 ${JSON.stringify(r)}`);
    };
    const rect = (l) => l.evaluate((e) => e.getBoundingClientRect().toJSON());
    await report("휴대폰", { chime: false });
    const pd = pp.locator(".droplet");
    await until("휴대폰 앱 안 알림", async () => (await pd.count()) === 1);
    await landed(pd);
    const head = await rect(pp.locator(".list-head"));
    const card = await rect(pd.locator(".droplet-card"));
    inside(card, "앱 안 알림");
    if (card.top < head.bottom) throw new Error(`앱 안 알림이 머리를 가린다 ${card.top} < ${head.bottom}`);
    await shot(pp, "phone-droplet");
    await pd.locator(".droplet-close").click();
    await until("휴대폰 알림 닫힘", async () => (await pd.count()) === 0);
    await pp.getByTestId("more").click();
    inside(await rect(pp.locator(".more-menu")), "더 보기 메뉴");
    const it = await rect(pp.getByTestId("alerts-item"));
    const menu = await rect(pp.locator(".more-menu"));
    if (it.bottom > menu.bottom) throw new Error(`알림 항목이 메뉴 밖으로 나온다 ${it.bottom} > ${menu.bottom}`);
    await shot(pp, "phone-more-menu");
    await pp.context().close();
    aaa("thread", "ack", "demo", t.id);
  });

  await step("보는 동안은 push 대신 앱 안 알림(사용자 화면 10-18 00:27 iPhone 홈 화면 앱 배너, herdr-web-ui server/push.ts inUse·sendTest·App.tsx 다시 등록): 구독이 있는 휴대폰을 열고 다시 열어도(다시 등록) push 0건, 보는 동안 새 보고는 push 0건·앱 안 알림 1건, 가려지면 push 1건", async () => {
    // 이 기기(휴대폰)는 알림을 켜 두어 이미 구독이 있다(홈 화면 앱처럼). 브라우저 push 서비스 대신 가짜 push 서비스의 주소를 준다
    const dev = pushDevice("push/dev3");
    const devPath = new URL(dev.json.endpoint).pathname;
    const toDev = (from) => pushes.slice(from).filter((x) => x.path === devPath);
    const subInit = (a) => {
      const log = [];
      Object.defineProperty(window, "__pushLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        constructor(t) {
          log.push(`new ${t}`);
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async function (t) {
        log.push(`tab ${t}`);
      };
      const key = Uint8Array.from(atob(a.key.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)).buffer;
      const sub = {
        endpoint: a.sub.endpoint,
        options: { applicationServerKey: key },
        toJSON: () => a.sub,
        unsubscribe: async () => (log.push("unsubscribe"), true),
      };
      PushManager.prototype.getSubscription = async () => sub;
      PushManager.prototype.subscribe = async () => (log.push("subscribe"), sub);
      try {
        localStorage.setItem("aaa:alerts-on", "1");
      } catch {}
      // 이 화면이 받은 thread 사건(보고 hash)
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          this.addEventListener("message", (m) => {
            try {
              const d = JSON.parse(m.data);
              if (d.ev === "thread") log.push(`thr ${String(d.report_hash).slice(0, 6)}`);
            } catch {}
          });
        }
      };
    };
    const init = `(${subInit})(${JSON.stringify({ key: pushKey, sub: dev.json })})`;
    // 다른 화면은 쓰지 않는 것으로 둔다: 이 휴대폰 하나만 본다
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    const start = pushes.length;
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init });
    const pp = phone.page;
    const subscribed = () => pp.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed);
    const bellPush = async () => {
      await pp.getByTestId("more").click();
      const ok = (await pp.getByTestId("alerts-item").innerText()).includes("켜짐, 이 기기로 푸시");
      await pp.keyboard.press("Escape");
      return ok;
    };
    // (1) 열면 권한·구독이 있는 기기는 조용히 다시 등록한다(참고 App.tsx 「re-registers on every load」). 다시 열어도 같다
    await until("열 때 다시 등록", async () => (await subscribed()) === 1 && (await bellPush()));
    await pp.reload();
    await pp.getByTestId("machine-labA").waitFor();
    await until("다시 열 때 다시 등록", () => bellPush());
    if ((await pp.evaluate(() => window.__pushLog.join(","))) !== "") throw new Error(`다시 등록이 구독을 바꿨다: ${await pp.evaluate(() => window.__pushLog.join(","))}`);
    await until("휴대폰을 봄", async () => (await pushInUse()) === 1);
    const work = (title) => {
      fs.writeFileSync(`${root}/task-${title}.md`, title);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-${title}.md`);
      const hash = until(`${title} 보고`, () => aaa("thread", "show", "demo", t.id).report_hash);
      return { t, hash };
    };
    const seen = (hash) => until("보고 사건 받음", () => pp.evaluate((h) => window.__pushLog.includes(`thr ${h.slice(0, 6)}`), hash));
    // (2) 보는 동안: host는 보내지 않고(push_skipped) 이 화면에 앱 안 알림이 하나 내려온다
    const skippedBefore = aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length;
    const a = work("휴대폰으로 보는 중");
    await seen(await a.hash);
    await until("보내지 않음 기록", () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length > skippedBefore);
    await until("보는 동안 앱 안 알림", async () => (await pp.locator(".droplet-title").allInnerTexts()).includes("휴대폰으로 보는 중"));
    if ((await pp.locator(".droplet").count()) !== 1) throw new Error(`앱 안 알림 ${await pp.locator(".droplet").count()}개`);
    if (toDev(start).length !== 0) throw new Error(`보는 동안·여는 동안 push ${toDev(start).length}건`);
    await pp.locator(".droplet-close").click();
    await until("앱 안 알림 닫힘", async () => (await pp.locator(".droplet").count()) === 0);
    // (3) 가려지면(홈 화면으로 나감): 쓰는 화면이 없어 이 기기에 push가 하나 간다. 앱 안 알림·탭 알림은 없다
    await pp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.hasFocus = () => false;
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("가려진 휴대폰", async () => (await pushInUse()) === 0);
    const b = work("휴대폰을 닫은 동안");
    const { msg } = await nextPush(dev, start, "가려진 동안 push");
    if (msg.title !== "demo · 휴대폰을 닫은 동안" || msg.tag !== `aaa-demo/${b.t.id}`) throw new Error(JSON.stringify(msg));
    await seen(await b.hash);
    await pp.evaluate(() => new Promise((r) => setTimeout(r)));
    if ((await pp.locator(".droplet").count()) !== 0) throw new Error("가려진 동안 앱 안 알림이 내려왔다");
    const tabs = (await pp.evaluate(() => window.__pushLog.filter((e) => !e.startsWith("thr ")).join(",")));
    if (tabs !== "") throw new Error(`push가 있는 기기에 탭 알림: ${tabs}`);
    // 처음부터 끝까지 이 기기에 간 push는 가려진 동안의 하나뿐(확인 push 「알림이 켜졌다」 없음)
    if (toDev(start).length !== 1) throw new Error(`이 기기에 push ${toDev(start).length}건`);
    if (phone.errors.length) throw new Error(`페이지 오류: ${phone.errors.join("; ")}`);
    await pp.context().close();
    // 머신마다 도는 thread 수에 한도가 있다(max_threads_per_machine): 다 본 thread는 정리한다
    for (const x of [a, b]) aaa("thread", "resolve", "demo", x.t.id);
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse()) === 1);
  });

  await step("두 기기·죽은 연결(coordinator 실기 10-18 00:58 iPhone: push는 「쓰는 화면 1개」로 생략, 앱 안 알림도 없음, herdr-web-ui ws.ts Liveness·server/push.ts inUse): A가 쓰는 동안 보이기만 하는 B에 앱 안 알림이 내려오고 push는 없다, 사건이 오지 않는 연결은 웹이 ping으로 알아채 다시 붙고, 답하지 않는 연결은 host가 끊어 쓰는 화면에서 뺀다, ⋯ 메뉴는 열면 첫 항목에 focus·화살표로 옮기고 Esc로 닫힌다(herdr-web-ui RowMenu)", async () => {
    // B(휴대폰): 보이지만 focus는 없다. 상태 사건 소켓을 「얼릴」 수 있게 감싼다: 언 뒤에는 받은 것을 버리고 보낸 것을 삼킨다.
    // 브라우저의 WebSocket ping 응답(pong)은 그대로라 host는 그 연결이 살아 있다고 본다(잠든 휴대폰·바뀐 망의 반쯤 열린 연결)
    const freezeInit = () => {
      document.hasFocus = () => false;
      const log = [];
      Object.defineProperty(window, "__evSock", { value: log });
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          log.push("new");
          const me = this;
          me.frozen = false;
          window.__freezeEvents = () => void (me.frozen = true);
          this.addEventListener("message", (e) => {
            if (!me.frozen) return;
            log.push(`dropped ${String(e.data).slice(0, 24)}`);
            e.stopImmediatePropagation();
          });
        }
        send(d) {
          if (String(d).startsWith('{"t":"ping"')) log.push("ping");
          if (this.frozen) {
            log.push(`swallowed ${String(d).slice(0, 24)}`);
            return;
          }
          super.send(d);
        }
      };
    };
    const B = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init: freezeInit });
    const bp = B.page;
    await until("A만 쓰는 화면", async () => (await pushInUse()) === 1);
    // A가 쓰는 기기다(마지막으로 누른 기기, plan §8.105)
    await page.evaluate(() => window.dispatchEvent(new PointerEvent("pointerdown")));
    await until("A가 쓰는 기기", () => pushWho(page).then((w) => w.you));
    const skips = () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped");
    const devicePushes = (from) => pushes.slice(from).filter((x) => !x.path.startsWith("/gone/"));
    let n = 0;
    const work = (title) => {
      fs.writeFileSync(`${root}/task-dead-${++n}.md`, title);
      return aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-dead-${n}.md`);
    };
    const ts = [];
    // (1) A(본 화면)가 쓰는 동안: 어느 기기에도 push가 없고(참고 inUse는 모든 기기), 보이기만 하는 B에는 앱 안 알림이 내려온다
    let from = pushes.length;
    let k = skips().length;
    ts.push(work("두 기기 A 쓰는 중"));
    const s1 = await until("A 때문에 보내지 않음", () => skips().slice(k)[0]);
    if (!/^쓰는 기기 [0-9A-Za-z]{1,7} · 쓰는 화면 1개\(기기 [0-9A-Za-z]{1,7}\) · 항목 1개$/.test(s1.detail)) throw new Error(`push_skipped ${s1.detail}`);
    await until("보이기만 하는 B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("두 기기 A 쓰는 중"));
    if (devicePushes(from).length !== 0) throw new Error(`A가 쓰는 동안 push ${devicePushes(from).length}건`);
    await bp.locator(".droplet-close").click();
    // 머신마다 도는 thread 수에 한도가 있다(max_threads_per_machine): 다 본 thread는 정리한다
    const done = () => {
      while (ts.length) aaa("thread", "resolve", "demo", ts.shift().id);
    };
    done();
    // (2) 이제 B만 쓰고, B의 연결이 반쯤 열린 채 죽는다: host는 B를 쓰는 화면으로 세어 push를 생략하고 B는 사건을 받지 못한다(실기와 같은 상태)
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    await bp.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    await until("B만 쓰는 화면", async () => (await pushInUse()) === 1 && (await bp.evaluate(() => document.hasFocus())));
    await bp.evaluate(() => window.__freezeEvents());
    k = skips().length;
    ts.push(work("죽은 연결 동안"));
    await until("죽은 B 때문에 보내지 않음", () => skips().slice(k)[0]);
    await until("B가 그 사건을 받지 못함", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith("dropped") && e.includes('"thread"'))));
    // 웹: 망이 바뀌거나(online) 다시 보이면 ping하고, 그 pong이 두 번 오지 않으면 끊고 바로 다시 붙는다
    await bp.evaluate(() => window.dispatchEvent(new Event("online")));
    await until("ping을 보냄", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith('swallowed {"t":"ping"'))));
    await until("다시 붙음", () => bp.evaluate(() => window.__evSock.filter((e) => e === "new").length === 2));
    await until("새 연결이 쓰는 화면", async () => (await pushInUse()) === 1);
    // 다시 붙으면 끊긴 동안 놓친 보고를 알린다(t3code afterSequence처럼 놓친 사건을 잇는다)
    await until("놓친 보고의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("죽은 연결 동안"));
    await bp.locator(".droplet-close").click();
    await until("놓친 보고 알림 닫힘", async () => (await bp.locator(".droplet").count()) === 0);
    done();
    k = skips().length;
    ts.push(work("다시 붙은 뒤"));
    await until("다시 붙은 B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("다시 붙은 뒤"));
    await until("다시 붙은 B 때문에 보내지 않음", () => skips().slice(k)[0]);
    // 10초 넘게 가려졌다가 다시 보이면 묻지 않고 바로 새로 붙는다(t3code application-active-reconnect). 시계만 앞으로 돌린다
    const mark = await bp.evaluate(() => window.__evSock.length);
    await bp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
      const real = Date.now;
      Date.now = () => real() + 11_000;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("오래 가려진 뒤 바로 다시 붙음", () => bp.evaluate(() => window.__evSock.filter((e) => e === "new").length === 3));
    const after = await bp.evaluate((i) => window.__evSock.slice(i), mark);
    if (after.indexOf("ping") !== -1 && after.indexOf("ping") < after.indexOf("new")) throw new Error(`오래 가려진 뒤 묻고 기다렸다: ${after.join(",")}`);
    await until("다시 쓰는 화면", async () => (await pushInUse()) === 1);
    // host: WebSocket ping에도 답하지 않는 연결(읽지 않는 클라이언트)은 끊어 쓰는 화면에서 뺀다
    const cookie = (await bp.context().cookies()).find((c) => c.name === "aaa" || c.name === "__Host-aaa");
    const u = new URL(base);
    const raw = await new Promise((resolve, reject) => {
      const req = http.request({
        host: u.hostname,
        port: u.port,
        path: "/api/v1/events",
        headers: {
          Connection: "Upgrade",
          Upgrade: "websocket",
          "Sec-WebSocket-Key": crypto.randomBytes(16).toString("base64"),
          "Sec-WebSocket-Version": "13",
          Origin: base.slice(0, -1),
          Cookie: `${cookie.name}=${cookie.value}`,
        },
      });
      req.on("upgrade", (_res, socket) => resolve(socket));
      req.on("response", (res) => reject(new Error(`upgrade 거절 ${res.statusCode}`)));
      req.on("error", reject);
      req.end();
    });
    const text = Buffer.from(JSON.stringify({ t: "presence", active: true }));
    const mask = crypto.randomBytes(4);
    raw.write(Buffer.concat([Buffer.from([0x81, 0x80 | text.length]), mask, Buffer.from(text.map((b, i) => b ^ mask[i % 4]))]));
    // 그 뒤로는 아무것도 보내지 않는다(pong도)
    raw.pause();
    await until("읽지 않는 연결도 쓰는 화면", async () => (await pushInUse()) === 2);
    const t0 = Date.now();
    await until("host가 답하지 않는 연결을 끊음", async () => (await pushInUse()) === 1);
    const gone = Date.now() - t0;
    raw.destroy();
    if (gone > 6000) throw new Error(`끊기까지 ${gone}ms(ping 1초·3초 기한)`);
    // ⋯ 메뉴(사용자 화면 10-18: 미러링 키보드 Esc로 닫히지 않음): 키보드 사건이 메뉴로 오도록 첫 항목에 focus
    const focused = () => bp.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.textContent?.trim());
    await bp.getByTestId("more").click();
    await until("열면 첫 항목에 focus", async () => (await focused()) === "링크 복사");
    await bp.keyboard.press("ArrowDown");
    if ((await focused()) !== "alerts-item") throw new Error(`아래 화살표 뒤 focus ${await focused()}`);
    await bp.keyboard.press("Escape");
    await until("Esc로 닫힘", async () => (await bp.locator(".more-menu").count()) === 0);
    if ((await focused()) !== "more") throw new Error(`닫힌 뒤 focus ${await focused()}`);
    if (B.errors.length) throw new Error(`페이지 오류: ${B.errors.join("; ")}`);
    await bp.context().close();
    done();
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse()) === 1);
  });

  await step("끝난 thread 보기(herdr-projects popup.rs Section::Threads, 차이 목록 2장 L3·3장 C1): 끝낸 thread는 목록에서 빠지고 「더 보기 → 프로젝트」에서 묶음마다 보이며 끝남은 맨 아래 흐리게, 거르기, 고르면 보고, 뒤로는 그 프로젝트", async () => {
    const $ = (id) => `[data-testid="${id}"]`;
    if ((await page.locator($("thread-demo-t-0001")).count()) !== 1) throw new Error("t-0001이 목록에 없다");
    aaa("thread", "resolve", "demo", "t-0001");
    await domIs(page, "끝낸 thread가 목록에서 빠짐(sidebar.rs)", (q) => !document.querySelector(q), $("thread-demo-t-0001"));
    // 앞 단계들도 demo thread를 끝낸다: 끝난 수는 host가 말하는 그대로
    const ended = aaa("thread", "list", "demo").threads.filter((t) => t.group === "resolved").length;
    // 프로젝트 고르기(popup 첫 화면): 프로젝트마다 열린 묶음 줄과 끝난 수
    await page.getByTestId("more").click();
    const toProjects = hashIs(page, "#projects");
    await page.getByTestId("projects-item").click();
    await toProjects;
    await domIs(page, "demo 줄의 끝난 수", ([q, n]) => document.querySelector(q)?.textContent.includes(`끝남 ${n}`), [$("project-open-demo"), ended]);
    const toDemo = hashIs(page, "#project=demo");
    await page.getByTestId("project-open-demo").click();
    await toDemo;
    const view = page.getByTestId("project-threads-demo");
    await domIs(page, "끝남 묶음", (q) => document.querySelector(q) !== null, `${$("project-threads-demo")} ${$("project-group-resolved")}`);
    const done = view.getByTestId("project-group-resolved");
    if ((await done.locator("h3").innerText()) !== `끝남 (${ended})`) throw new Error(`끝남 ${ended}: ${await done.innerHTML()}`);
    // 묶음은 popup DISPLAY_ORDER대로, 끝남은 맨 아래
    const order = await view.locator("[data-testid^=project-group-]").evaluateAll((xs) => xs.map((x) => x.dataset.testid.slice("project-group-".length)));
    const rank = ["waiting_on_you", "ready_for_review", "landing", "working", "idle", "resolved"];
    if (order.at(-1) !== "resolved" || order.some((g, i) => i > 0 && rank.indexOf(g) < rank.indexOf(order[i - 1]))) throw new Error(order.join(","));
    // 끝난 줄은 흐리다(popup group_color: Resolved는 DarkGrey)
    const r = view.getByTestId("project-thread-t-0001");
    const dim = await r.locator(".project-threads-name").evaluate((n) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--text-dim)";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      return [getComputedStyle(n).color, want, n.textContent];
    });
    if (dim[0] !== dim[1] || dim[2] !== "t-0001 · 웹 결과물") throw new Error(`끝난 줄 ${JSON.stringify(dim)}`);
    await shot(page, "desktop-project-threads");
    // popup 「/」 거르기처럼 이름(id·제목)에 든 글로 고른다
    const rows = `${$("project-threads-demo")} [data-testid^=project-thread-]`;
    const all = await page.locator(rows).count();
    const filter = view.getByRole("searchbox");
    await filter.fill("웹 결과");
    await domIs(page, "거른 줄 하나", (q) => document.querySelectorAll(q).length === 1, rows);
    await filter.fill("없는 이름");
    await domIs(page, "맞는 것 없음", (q) => document.querySelector(q)?.textContent === "맞는 thread 없음", `${$("project-threads-demo")} [role=status]`);
    await filter.fill("");
    await domIs(page, "거르기 풂", ([q, n]) => document.querySelectorAll(q).length === n, [rows, all]);
    // 고르면 보고(popup detail), 뒤로는 그 프로젝트(popup: detail에서 목록으로)
    const toReport = hashIs(page, "#thread=demo%2Ft-0001");
    await r.click();
    await toReport;
    await domIs(page, "끝난 thread 보고", (q) => document.querySelector(q)?.textContent === "확인", `${$("report")} .markdown strong`);
    const back = hashIs(page, "#project=demo");
    await page.getByTestId("thread-back").click();
    await back;
    // 휴대폰 폭: 목록 대신 이 화면, 옆으로 밀리지 않는다
    await page.setViewportSize({ width: 390, height: 844 });
    await domIs(page, "휴대폰 폭 프로젝트 보기", (q) => document.querySelector(q)?.getBoundingClientRect().width > 0, $("project-threads-demo"));
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await shot(page, "phone-project-threads");
    const toPicker = hashIs(page, "#projects");
    await view.getByRole("button", { name: "프로젝트 목록" }).click();
    await toPicker;
    const closed = hashIs(page, "");
    await page.getByTestId("projects").getByRole("button", { name: "닫기" }).click();
    await closed;
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("줄 메뉴(herdr-web-ui RowMenu·Sidebar menuItems·AgentSidebar, 차이 목록 2장 L9·L10): 데스크톱은 오른쪽 클릭·휴대폰은 ⋯ sheet, 고정은 맨 앞 카드로 가고 다른 기기에도(host 공유 문서), pane 닫기·기록 지우기는 확인 창, 끝난 pane은 다시 열기, 프로젝트 머리에서 프로젝트 thread 보기", async () => {
    const pinWrote = (p) =>
      p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set").then((r) => r.request().postDataJSON());
    const opDone = (p, op) => p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === op);
    const rowOf = (p, name) => p.locator(".machine-list li[data-pin-key]", { has: p.getByTestId(`pane-${name}`) });
    const menuItems = (p) => p.getByRole("menu").locator('[role="menuitem"]').allInnerTexts();
    // 참고대로 host가 고정을 나눈 적이 없으면 고정만 바꿔서는 보내지 않고 첫 설정 변경에 실려 간다(preferencesChange).
    // 이 단계는 다른 기기가 이미 고정을 나눈 host에서 시작한다: 빈 고정을 써 두고 화면을 다시 열어 그 문서를 읽힌다.
    await page.evaluate(() =>
      fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "preferences_set", settings: {}, pins: [] }) }),
    );
    const reread = opDone(page, "preferences");
    await page.reload();
    await reread;
    await page.getByTestId("pane-t1").waitFor();
    // 1. 데스크톱: 오른쪽 클릭 → 첫 항목에 focus, 차례는 참고대로 고정 → 닫기
    const key = await rowOf(page, "t1").getAttribute("data-pin-key");
    await page.getByTestId("pane-t1").click({ button: "right" });
    await focusIs(page, "메뉴 첫 항목", '[role="menu"] [role="menuitem"]');
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["pane 고정", "pane 닫기"])) throw new Error(`메뉴 ${await menuItems(page)}`);
    if ((await page.locator(".machine-list .row-menu-toggle").first().isVisible())) throw new Error("마우스 데스크톱에 ⋯ 단추가 보인다(참고는 오른쪽 클릭)");
    await page.keyboard.press("Escape");
    await domIs(page, "Esc로 메뉴 닫힘", () => !document.querySelector('[role="menu"]'));
    // 고정: 맨 앞 카드에 핀 표시와 함께, host 공유 문서에 그 열쇠
    await page.getByTestId("pane-t1").click({ button: "right" });
    const sent = pinWrote(page);
    await page.getByRole("menuitem", { name: "pane 고정" }).click();
    if (!(await sent).pins?.includes(key)) throw new Error(`고정이 host로 가지 않았다: ${JSON.stringify(await sent)}`);
    await domIs(page, "고정 카드", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"] .sidebar-pin-mark`) !== null, key);
    await shot(page, "desktop-row-pinned");
    // 2. 휴대폰(다른 기기): 고정이 와 있고, ⋯ sheet에서 푼다 → 데스크톱에서도 풀린다
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await domIs(ph.page, "다른 기기의 고정 카드", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"]`) !== null, key);
    await ph.page.locator(`.machine-list .sidebar-pinned [data-pin-key="${key}"] .row-menu-toggle`).click();
    const sheet = ph.page.getByRole("dialog", { name: "t1" });
    await sheet.waitFor();
    // 아래에서 올라오는 움직임이 끝난 뒤 찍는다(움직임의 finished, 시간 기다림 없음)
    await sheet.evaluate((el) => Promise.all(el.closest(".modal-scrim").getAnimations({ subtree: true }).map((a) => a.finished)));
    await shot(ph.page, "phone-row-menu");
    const unsent = pinWrote(ph.page);
    await sheet.getByRole("button", { name: "pane 고정 해제" }).click();
    if ((await unsent).pins?.includes(key)) throw new Error("고정 해제가 host로 가지 않았다");
    await domIs(page, "데스크톱에서도 풀림", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"]`) === null && document.querySelector(`.machine-list [data-pin-key="${k}"]`) !== null, key);
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    // 3. pane 닫기: 확인 창(취소에 focus) → 닫으면 줄이 사라진다
    aaa("tab", "create", "--machine", "labA", "--label", "rowclose", "--", "/bin/sh", "-c", "read x");
    await page.getByTestId("pane-rowclose").waitFor();
    await page.getByTestId("pane-rowclose").click({ button: "right" });
    await page.getByRole("menuitem", { name: "pane 닫기" }).click();
    const ask = page.getByRole("alertdialog").filter({ hasText: "rowclose pane을 닫을까요?" });
    await ask.waitFor();
    await focusIs(page, "확인 창 취소", '[role="alertdialog"] .modal-footer .btn:not(.btn-danger)');
    const closed = opDone(page, "pane_close");
    await ask.getByRole("button", { name: "pane 닫기" }).click();
    await closed;
    await domIs(page, "닫은 pane 줄 사라짐", () => !document.querySelector('[data-testid="pane-rowclose"]'));
    if (aaa("pane", "list").panes.some((x) => x.name === "rowclose")) throw new Error("닫은 pane이 host에 남았다");
    // 4. 끝난 pane: 다시 열기 → 돈다, 다시 끝내고 기록 지우기(확인 창) → 줄이 사라진다
    aaa("tab", "create", "--machine", "labA", "--label", "rowend", "--", "/bin/sh", "-c", "read x");
    await page.getByTestId("pane-rowend").waitFor();
    const endedRow = () => document.querySelector('li.is-ended:has([data-testid="pane-rowend"])') !== null;
    aaa("pane", "kill", "rowend");
    await domIs(page, "끝난 pane", endedRow);
    await page.getByTestId("pane-rowend").click({ button: "right" });
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["pane 고정", "다시 열기", "기록 지우기"])) throw new Error(`끝난 pane 메뉴 ${await menuItems(page)}`);
    const resumed = opDone(page, "pane_resume");
    await page.getByRole("menuitem", { name: "다시 열기" }).click();
    if (!(await (await resumed).json()).ok) throw new Error("다시 열기 실패");
    await domIs(page, "다시 연 pane이 돈다", () => document.querySelector('li:has([data-testid="pane-rowend"])') !== null && document.querySelector('li.is-ended:has([data-testid="pane-rowend"])') === null);
    aaa("pane", "kill", "rowend");
    await domIs(page, "다시 끝난 pane", endedRow);
    await page.getByTestId("pane-rowend").click({ button: "right" });
    await page.getByRole("menuitem", { name: "기록 지우기" }).click();
    const forget = page.getByRole("alertdialog").filter({ hasText: "rowend pane 기록을 지울까요?" });
    await forget.waitFor();
    const forgot = opDone(page, "pane_forget");
    await forget.getByRole("button", { name: "지우기" }).click();
    await forgot;
    await domIs(page, "기록 지운 pane 줄 사라짐", () => !document.querySelector('[data-testid="pane-rowend"]'));
    // 5. 프로젝트 머리: 프로젝트 thread 보기(끝난 것까지, plan §8.89)
    await page.getByTestId("project-labA-demo").locator(".workspace-header").click({ button: "right" });
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["프로젝트 thread 보기", "접기"])) throw new Error(`프로젝트 메뉴 ${await menuItems(page)}`);
    const toDemo = hashIs(page, "#project=demo");
    await page.getByRole("menuitem", { name: "프로젝트 thread 보기" }).click();
    await toDemo;
    // 프로젝트에서 연 보고의 「뒤로」는 그 프로젝트, 다른 길(앱 안 알림·주소)로 간 thread는 목록으로
    const toReport = hashIs(page, "#thread=demo%2Ft-0001");
    await page.getByTestId("project-thread-t-0001").click();
    await toReport;
    if ((await page.getByTestId("thread-back").getAttribute("aria-label")) !== "demo 프로젝트로") throw new Error("프로젝트에서 연 보고의 뒤로");
    const other = aaa("thread", "list", "demo").threads.find((t) => t.id !== "t-0001" && t.role !== "evaluator").id;
    const toOther = hashIs(page, `#thread=demo%2F${other}`);
    await page.evaluate((h) => (location.hash = h), `#thread=demo%2F${other}`);
    await toOther;
    await domIs(page, "다른 길로 간 보고의 뒤로는 목록", () => document.querySelector('[data-testid="thread-back"]')?.getAttribute("aria-label") === "목록으로");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("쓰는 기기(사용자 결정 10-18 「최고 나중에 화면 켠 쪽」, plan §8.105): 마지막으로 치거나 누르거나 화면을 켠 한 기기가 보이고 focus일 때만 push를 건너뛴다. A가 보이고 focus여도 B에서 친 뒤 B가 가려지면 push가 가고, B가 쓰는 동안은 push 없이 B에 앱 안 알림, A에서 다시 치면 A가 쓰는 기기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "who", "--", "/bin/cat");
    const skips = () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped");
    const sents = () => aaa("events", "demo").events.filter((e) => e.kind === "push_sent");
    const devicePushes = (from) => pushes.slice(from).filter((x) => !x.path.startsWith("/gone/"));
    const ts = [];
    let n = 0;
    const work = (title) => {
      fs.writeFileSync(`${root}/task-user-${++n}.md`, title);
      ts.push(aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-user-${n}.md`));
    };
    const done = () => {
      while (ts.length) aaa("thread", "resolve", "demo", ts.shift().id);
    };
    // A(본 화면, 데스크톱)는 처음부터 끝까지 보이고 focus다
    await setInUse(page, true);
    await page.evaluate(() => window.dispatchEvent(new PointerEvent("pointerdown")));
    await until("A가 쓰는 기기", () => pushWho(page).then((w) => w.you));
    // B(휴대폰): 열 때는 focus가 없다(앱을 켠 것이 아니다)
    const B = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init: () => (document.hasFocus = () => false) });
    const bp = B.page;
    await bp.getByTestId("pane-who").click();
    await until("B 터미널", async () => (await bp.locator(".xterm-rows").count()) === 1);
    // 새 연결의 snapshot 전에 친 글은 「붙잡은 입력」이 된다(Term.tsx deliver): 입력할 수 있게 된 뒤 친다
    await until("B 입력 가능", async () => (await bp.getByTestId("readonly-banner").count()) === 0);
    if (!(await pushWho(page)).you) throw new Error("focus 없이 연 B가 쓰는 기기가 됐다");
    // (1) B에서 친다: B가 쓰는 기기(pane 입력, host가 본다). A는 여전히 보이고 focus
    await bp.locator(".xterm-helper-textarea").focus();
    await bp.keyboard.type("b1");
    await waitOutput("who", "b1");
    await until("B가 쓰는 기기", () => pushWho(bp).then((w) => w.you));
    if ((await pushWho(page)).you) throw new Error("B가 친 뒤에도 A가 쓰는 기기");
    // B가 쓰는 화면이 아니다(focus 없음): A가 보이고 focus여도 push가 간다
    let from = pushes.length;
    let k = skips().length;
    let m = sents().length;
    work("쓰는 기기 B가 안 볼 때");
    await until("push 보냄", () => sents().length > m);
    if (skips().length !== k) throw new Error(`B가 안 보는데 건너뜀: ${JSON.stringify(skips().slice(k))}`);
    await until("기기 push", () => devicePushes(from).length >= 1);
    done();
    // (2) B가 보이고 focus(화면을 켰다): push 없이 B에 앱 안 알림
    await bp.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    await until("B 쓰는 화면", async () => (await pushInUse()) === 2 && (await pushWho(bp)).you);
    from = pushes.length;
    k = skips().length;
    work("쓰는 기기 B가 볼 때");
    const s2 = await until("B 때문에 건너뜀", () => skips().slice(k)[0]);
    const bId = (await pushWho(bp)).user;
    if (!s2.detail.startsWith(`쓰는 기기 ${bId} · 쓰는 화면 2개`)) throw new Error(`push_skipped ${s2.detail}(B ${bId})`);
    await until("B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("쓰는 기기 B가 볼 때"));
    if (devicePushes(from).length !== 0) throw new Error(`B가 쓰는 동안 push ${devicePushes(from).length}건`);
    await bp.locator(".droplet-close").click();
    done();
    // B가 가려진다: 쓰는 기기는 B 그대로, 쓰는 화면이 아니므로 push가 간다(A는 보이고 focus)
    await bp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("B 가려짐", async () => (await pushInUse()) === 1);
    from = pushes.length;
    k = skips().length;
    m = sents().length;
    work("쓰는 기기 B가 가려진 뒤");
    await until("가려진 뒤 push 보냄", () => sents().length > m);
    if (skips().length !== k) throw new Error(`B가 가려졌는데 건너뜀: ${JSON.stringify(skips().slice(k))}`);
    await until("가려진 뒤 기기 push", () => devicePushes(from).length >= 1);
    done();
    // (3) A에서 다시 친다: A가 쓰는 기기, push 없음
    await page.getByTestId("pane-who").click();
    // B가 친 화면(snapshot)이 온 뒤에 친다: 그 전에는 읽기 전용이라 글이 붙잡힌다(ci 실패: 「who에 a1이 나오지 않았다」)
    await until("A who 화면", async () => (await screenText(page)).includes("b1"));
    await until("A 입력 가능", async () => (await page.getByTestId("readonly-banner").count()) === 0);
    await page.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("a1");
    await waitOutput("who", "a1");
    await until("A가 다시 쓰는 기기", () => pushWho(page).then((w) => w.you));
    k = skips().length;
    work("쓰는 기기 A");
    const s3 = await until("A 때문에 건너뜀", () => skips().slice(k)[0]);
    const aId = (await pushWho(page)).user;
    if (!s3.detail.startsWith(`쓰는 기기 ${aId} ·`) || aId === bId) throw new Error(`push_skipped ${s3.detail}(A ${aId}, B ${bId})`);
    done();
    if (B.errors.length) throw new Error(`페이지 오류: ${B.errors.join("; ")}`);
    await bp.context().close();
    aaa("pane", "kill", "who");
  });

  await step("사후 평가자: 목록에 섞이지 않고 header badge로 보인다 → 누르면 평가자 화면", async () => {
    const v = aaa("evaluator", "assign", "demo", "--profile", "fake", "--machine", "labA").evaluators[0];
    const id = v.thread.id;
    const badge = page.getByTestId("evaluator-demo");
    await until("평가 badge", async () => (await badge.count()) === 1);
    await until("평가자 brief 전달", () => aaa("thread", "show", "demo", id).brief === "delivered");
    if ((await page.getByTestId(`thread-demo-${id}`).count()) !== 0) throw new Error("평가자가 thread 목록에 보인다");
    if ((await page.getByTestId(`pane-demo/${id}`).count()) !== 0) throw new Error("평가자가 머신 목록에 보인다");
    const text = await badge.innerText();
    if (!/평가\s*●\s*0/.test(text)) throw new Error(`badge: ${text}`);
    await badge.click();
    const pane = aaa("thread", "show", "demo", id).pane;
    await until("평가자 화면 선택", async () => (await page.evaluate(() => location.hash)) === `#pane=${pane}`);
    await until("평가자 터미널", async () => (await screenText(page)).includes("fake>"));
    await shot(page, "desktop-evaluator");
    aaa("evaluator", "unassign", "demo");
    await until("끝낸 평가자 badge 사라짐", async () => (await badge.count()) === 0);
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });
}
