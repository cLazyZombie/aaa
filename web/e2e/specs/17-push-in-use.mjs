// 웹 E2E 쓰는 화면·쓰는 기기에 따른 push 판정(plan §8.69·§8.105·§8.119): 보는 동안·두 기기·죽은 연결·쓰는 기기·기기별 알림 종류·시작 질문(§8.328).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 17-push-in-use(새 실험실). 준비는 runSpec과 pushLab(서명 키·구독 하나·demo):
// 단계는 서로 기대지 않아 하나만 골라(--grep) 돌 수 있다
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import {
  root, aaa, until, pairPage, pushes, pushDevice, nextPush, pushKey, base, step, browser, openSettings, waitOutput, setInUse, inList,
  pushInUse, pushWho, pushLab, coordinatorPost,
} from "../lib/harness.mjs";

// 알림 묶음 100ms(실험실 설정 1초, notify.rs debounce): push 판정마다 1초를 기다렸다. 설정 1초 길은 12가 돈다
export const labEnv = { AAA_TEST_NOTIFY_DEBOUNCE_MS: "100" };

// 여기서는 push 판정·앱 안 알림을 검증한 뒤 화면을 정리한다. 닫기 단추의 실제 조작은 12가 소유한다.
// 조회 도중 저절로 접혔으면 이미 정리된 것이다. 존재 확인과 click을 같은 JS 작업에서 해 수명과 경쟁하지 않는다.
async function dismissObservedDroplet(page) {
  const action = await page.evaluate(() => {
    const droplet = document.querySelector(".droplet");
    if (!droplet) return "already-gone";
    if (droplet.dataset.phase === "out") return "already-leaving";
    droplet.querySelector(".droplet-close").click();
    return "closed";
  });
  await page.locator(".droplet").waitFor({ state: "detached" });
  return action;
}

export default async function ({ page }) {
  await pushLab(page);

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
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    const start = pushes.length;
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init });
    const pp = phone.page;
    await pp.clock.install();
    const subscribed = () => pp.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed);
    const bellPush = async () => {
      await inList(pp, "more");
      const ok = (await pp.getByTestId("alerts-item").innerText()).includes("켜짐, 이 기기로 푸시");
      await pp.keyboard.press("Escape");
      return ok;
    };
    // (1) 열면 권한·구독이 있는 기기는 조용히 다시 등록한다(참고 App.tsx 「re-registers on every load」). 다시 열어도 같다
    await until("열 때 다시 등록", async () => (await subscribed()) === 1 && (await bellPush()));
    await pp.reload();
    await pp.getByTestId("machine-labA").waitFor({ state: "attached" });
    await until("다시 열 때 다시 등록", () => bellPush());
    if ((await pp.evaluate(() => window.__pushLog.join(","))) !== "") throw new Error(`다시 등록이 구독을 바꿨다: ${await pp.evaluate(() => window.__pushLog.join(","))}`);
    await until("휴대폰을 봄", async () => (await pushInUse(page)) === 1);
    const work = async (title) => {
      fs.writeFileSync(`${root}/task-${title}.md`, title);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-${title}.md`);
      const hash = until(`${title} 보고`, () => aaa("thread", "show", "demo", t.id).report_hash);
      await hash;
      await coordinatorPost();
      return { t, hash };
    };
    const seen = (hash) => until("보고 사건 받음", () => pp.evaluate((h) => window.__pushLog.includes(`thr ${h.slice(0, 6)}`), hash));
    // (2) 보는 동안: host는 보내지 않고(push_skipped) 이 화면에 앱 안 알림이 하나 내려온다
    const skippedBefore = aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length;
    const a = await work("휴대폰으로 보는 중");
    await seen(await a.hash);
    await until("보내지 않음 기록", () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length > skippedBefore);
    await until("보는 동안 앱 안 알림", async () => (await pp.locator(".droplet-title").allInnerTexts()).includes("휴대폰으로 보는 중"));
    if ((await pp.locator(".droplet").count()) !== 1) throw new Error(`앱 안 알림 ${await pp.locator(".droplet").count()}개`);
    if (toDev(start).length !== 0) throw new Error(`보는 동안·여는 동안 push ${toDev(start).length}건`);
    // CLI 조회 중 hold가 끝난 순서를 가상 시계로 고정한다. 닫힘 사건 뒤의 정리는 없어질 단추를 기다리면 안 된다.
    await pp.clock.runFor(6000); // reveal + 3.6초 hold + fold를 넘긴다(실제 sleep 없음)
    await pp.locator(".droplet").waitFor({ state: "detached" });
    console.log('droplet-close-race {"observed":true,"expiredBeforeCleanup":true}');
    const cleanup = await dismissObservedDroplet(pp);
    if (cleanup !== "already-gone") throw new Error(`자동 닫힘 뒤 정리: ${cleanup}`);
    await until("앱 안 알림 닫힘", async () => (await pp.locator(".droplet").count()) === 0);
    // (3) 가려지면(홈 화면으로 나감): 쓰는 화면이 없어 이 기기에 push가 하나 간다. 앱 안 알림·탭 알림은 없다
    await pp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.hasFocus = () => false;
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("가려진 휴대폰", async () => (await pushInUse(page)) === 0);
    const b = await work("휴대폰을 닫은 동안");
    const { msg } = await nextPush(dev, start, "가려진 동안 push");
    if (msg.title !== "demo" || msg.tag !== "aaa-demo" || msg.body !== "coordinator 맡긴 일 완료") throw new Error(JSON.stringify(msg));
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
    await until("본 화면 다시 씀", async () => (await pushInUse(page)) === 1);
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
    await until("A만 쓰는 화면", async () => (await pushInUse(page)) === 1);
    // A가 쓰는 기기다(마지막으로 누른 기기, plan §8.105)
    await page.evaluate(() => window.dispatchEvent(new PointerEvent("pointerdown")));
    await until("A가 쓰는 기기", () => pushWho(page).then((w) => w.you));
    const skips = () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped");
    const devicePushes = (from) => pushes.slice(from).filter((x) => !x.path.startsWith("/gone/"));
    let n = 0;
    const work = async (title) => {
      fs.writeFileSync(`${root}/task-dead-${++n}.md`, title);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-dead-${n}.md`);
      await until(`${title} 보고`, () => aaa("thread", "show", "demo", t.id).report_hash);
      await coordinatorPost();
      return t;
    };
    const ts = [];
    // (1) A(본 화면)가 쓰는 동안: 어느 기기에도 push가 없고(참고 inUse는 모든 기기), 보이기만 하는 B에는 앱 안 알림이 내려온다
    let from = pushes.length;
    let k = skips().length;
    ts.push(await work("두 기기 A 쓰는 중"));
    const s1 = await until("A 때문에 보내지 않음", () => skips().slice(k)[0]);
    if (!/^쓰는 기기 [0-9A-Za-z]{1,7} · 쓰는 화면 1개\(기기 [0-9A-Za-z]{1,7}\) · 항목 1개 · 근거: 쓰는 기기 /.test(s1.detail)) throw new Error(`push_skipped ${s1.detail}`);
    await until("보이기만 하는 B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("두 기기 A 쓰는 중"));
    if (devicePushes(from).length !== 0) throw new Error(`A가 쓰는 동안 push ${devicePushes(from).length}건`);
    await dismissObservedDroplet(bp);
    // 머신마다 도는 thread 수에 한도가 있다(max_threads_per_machine): 다 본 thread는 정리한다
    const done = () => {
      while (ts.length) aaa("thread", "resolve", "demo", ts.shift().id);
    };
    done();
    // (2) 이제 B만 쓰고, B의 연결이 반쯤 열린 채 죽는다: host는 B를 쓰는 화면으로 세어 push를 생략하고 B는 사건을 받지 못한다(실기와 같은 상태)
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    await bp.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    await until("B만 쓰는 화면", async () => (await pushInUse(page)) === 1 && (await bp.evaluate(() => document.hasFocus())));
    await bp.evaluate(() => window.__freezeEvents());
    k = skips().length;
    ts.push(await work("죽은 연결 동안"));
    await until("죽은 B 때문에 보내지 않음", () => skips().slice(k)[0]);
    await until("B가 그 사건을 받지 못함", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith("dropped") && e.includes('"thread"'))));
    // 웹: 망이 바뀌거나(online) 다시 보이면 ping하고, 그 pong이 두 번 오지 않으면 끊고 바로 다시 붙는다
    await bp.evaluate(() => window.dispatchEvent(new Event("online")));
    await until("ping을 보냄", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith('swallowed {"t":"ping"'))));
    await until("다시 붙음", () => bp.evaluate(() => window.__evSock.filter((e) => e === "new").length === 2));
    await until("새 연결이 쓰는 화면", async () => (await pushInUse(page)) === 1);
    // 다시 붙으면 끊긴 동안 놓친 보고를 알린다(t3code afterSequence처럼 놓친 사건을 잇는다)
    await until("놓친 보고의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("죽은 연결 동안"));
    await dismissObservedDroplet(bp);
    await until("놓친 보고 알림 닫힘", async () => (await bp.locator(".droplet").count()) === 0);
    done();
    k = skips().length;
    ts.push(await work("다시 붙은 뒤"));
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
    await until("다시 쓰는 화면", async () => (await pushInUse(page)) === 1);
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
    await until("읽지 않는 연결도 쓰는 화면", async () => (await pushInUse(page)) === 2);
    const t0 = Date.now();
    await until("host가 답하지 않는 연결을 끊음", async () => (await pushInUse(page)) === 1);
    const gone = Date.now() - t0;
    raw.destroy();
    if (gone > 6000) throw new Error(`끊기까지 ${gone}ms(ping 1초·3초 기한)`);
    // ⋯ 메뉴(사용자 화면 10-18: 미러링 키보드 Esc로 닫히지 않음): 키보드 사건이 메뉴로 오도록 첫 항목에 focus
    const focused = () => bp.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.textContent?.trim());
    await inList(bp, "more");
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
    await until("본 화면 다시 씀", async () => (await pushInUse(page)) === 1);
  });

  await step("쓰는 기기(사용자 결정 10-18 「최고 나중에 화면 켠 쪽」, plan §8.105): 마지막으로 치거나 누르거나 화면을 켠 한 기기가 보이고 focus일 때만 push를 건너뛴다. A가 보이고 focus여도 B에서 친 뒤 B가 가려지면 push가 가고, B가 쓰는 동안은 push 없이 B에 앱 안 알림, A에서 다시 치면 A가 쓰는 기기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "who", "--", "/bin/cat");
    const skips = () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped");
    const sents = () => aaa("events", "demo").events.filter((e) => e.kind === "push_sent");
    const devicePushes = (from) => pushes.slice(from).filter((x) => !x.path.startsWith("/gone/"));
    const ts = [];
    let n = 0;
    const work = async (title) => {
      fs.writeFileSync(`${root}/task-user-${++n}.md`, title);
      ts.push(aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-user-${n}.md`));
      await until(`${title} 보고`, () => aaa("thread", "show", "demo", ts.at(-1).id).report_hash);
      await coordinatorPost();
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
    await inList(bp, "pane-who");
    // 터미널이 입력을 받기 전에 친 글은 붙잡힌다(herdr-web-ui draft): who pane이 붙어 입력을 받을 때까지
    const writable = (p) => until("who 터미널 입력 받음", async () => (await p.locator('[data-testid="terminal"][data-pane-name="who"][data-readonly="false"]').count()) === 1);
    await writable(bp);
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
    await work("쓰는 기기 B가 안 볼 때");
    await until("push 보냄", () => sents().length > m);
    if (skips().length !== k) throw new Error(`B가 안 보는데 건너뜀: ${JSON.stringify(skips().slice(k))}`);
    await until("기기 push", () => devicePushes(from).length >= 1);
    done();
    // (2) B가 보이고 focus(화면을 켰다): push 없이 B에 앱 안 알림
    await bp.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    await until("B 쓰는 화면", async () => (await pushInUse(page)) === 2 && (await pushWho(bp)).you);
    from = pushes.length;
    k = skips().length;
    await work("쓰는 기기 B가 볼 때");
    const s2 = await until("B 때문에 건너뜀", () => skips().slice(k)[0]);
    const bId = (await pushWho(bp)).user;
    if (!s2.detail.startsWith(`쓰는 기기 ${bId} · 쓰는 화면 2개`)) throw new Error(`push_skipped ${s2.detail}(B ${bId})`);
    await until("B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("쓰는 기기 B가 볼 때"));
    if (devicePushes(from).length !== 0) throw new Error(`B가 쓰는 동안 push ${devicePushes(from).length}건`);
    await dismissObservedDroplet(bp);
    done();
    // B가 가려진다: 쓰는 기기는 B 그대로, 쓰는 화면이 아니므로 push가 간다(A는 보이고 focus)
    await bp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("B 가려짐", async () => (await pushInUse(page)) === 1);
    from = pushes.length;
    k = skips().length;
    m = sents().length;
    await work("쓰는 기기 B가 가려진 뒤");
    await until("가려진 뒤 push 보냄", () => sents().length > m);
    if (skips().length !== k) throw new Error(`B가 가려졌는데 건너뜀: ${JSON.stringify(skips().slice(k))}`);
    await until("가려진 뒤 기기 push", () => devicePushes(from).length >= 1);
    done();
    // (3) A에서 다시 친다: A가 쓰는 기기, push 없음
    await page.getByTestId("pane-who").click();
    await writable(page);
    await page.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("a1");
    await waitOutput("who", "a1");
    await until("A가 다시 쓰는 기기", () => pushWho(page).then((w) => w.you));
    k = skips().length;
    await work("쓰는 기기 A");
    const s3 = await until("A 때문에 건너뜀", () => skips().slice(k)[0]);
    const aId = (await pushWho(page)).user;
    // 기기 이름은 기기 ID(ULID)의 뒤 무작위 7자다: 앞 7자(시각)는 약 33초 안에 짝지은 두 기기가 같았다(t-0338 report
    // 「찾은 결함」, 10-18 실기, plan §8.166). 쓰는 기기가 B에서 A로 바뀐 것은 근거에 적힌 그 기기의 연결 번호로도 본다
    const devOf = async (p) => (await p.evaluate(() => fetch("/api/v1/me").then((r) => r.json()))).ok.device;
    const [aDev, bDev] = [await devOf(page), await devOf(bp)];
    if (aId !== aDev.slice(19) || bId !== bDev.slice(19) || aId === bId)
      throw new Error(`기기 이름 A ${aId}(ID ${aDev.length}자 …${aDev.slice(19)}) · B ${bId}(…${bDev.slice(19)})`);
    const conns = (s) => [...s.detail.matchAll(/연결#(\d+)/g)].map((x) => x[1]);
    if (!s3.detail.startsWith(`쓰는 기기 ${aId} ·`) || !conns(s3).length || conns(s3).some((c) => conns(s2).includes(c))) {
      throw new Error(`push_skipped ${s3.detail}(A ${aId}, B ${bId}; B가 쓸 때 ${s2.detail})`);
    }
    done();
    if (B.errors.length) throw new Error(`페이지 오류: ${B.errors.join("; ")}`);
    await bp.context().close();
    aaa("pane", "kill", "who");
  });

  await step("기기별 알림 종류(herdr-web-ui AlertsPage 「입력 필요」·「작업 완료」, notify-policy alertsAllow, 차이 목록 N3, plan §8.119): 설정에서 「작업 완료」를 끄면 다시 등록해 host가 이 기기로 새 보고 push를 보내지 않고 앱 안 알림도 없으며, 다시 켜면 보낸다", async () => {
    const dev = pushDevice("push/dev-kinds");
    const devPath = new URL(dev.json.endpoint).pathname;
    const toDev = (from) => pushes.slice(from).filter((x) => x.path === devPath);
    // 알림을 켜 둔 기기(권한·구독 있음). 브라우저 push 서비스 대신 가짜 push 서비스 주소를 준다(위 「보는 동안」과 같다)
    const init = `(${(a) => {
      const log = [];
      Object.defineProperty(window, "__pushLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async () => {};
      const key = Uint8Array.from(atob(a.key.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)).buffer;
      const sub = { endpoint: a.sub.endpoint, options: { applicationServerKey: key }, toJSON: () => a.sub, unsubscribe: async () => true };
      PushManager.prototype.getSubscription = async () => sub;
      PushManager.prototype.subscribe = async () => sub;
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
    }})(${JSON.stringify({ key: pushKey, sub: dev.json })})`;
    // 본 화면은 쓰지 않는 것으로 둔다: 이 휴대폰만 본다
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    const start = pushes.length;
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init });
    const pp = phone.page;
    const mine = () => pp.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok);
    await until("열 때 다시 등록(기본 둘 다 켬)", async () => {
      const i = await mine();
      return i.subscribed === 1 && i.alerts?.input === true && i.alerts?.done === true;
    });
    const doneSwitch = async (on) => {
      const dialog = await openSettings(pp, "알림");
      const sw = dialog.getByRole("switch", { name: "작업 완료" });
      if ((await sw.getAttribute("aria-checked")) !== String(!on)) throw new Error(`작업 완료 스위치 ${await sw.getAttribute("aria-checked")}`);
      await sw.click();
      await pp.keyboard.press("Escape");
      // 바꾸면 다시 등록해 host가 이 기기의 고른 것을 안다
      await until(`host가 작업 완료 ${on}을 앎`, async () => (await mine()).alerts?.done === on);
    };
    const hide = (hidden) =>
      pp.evaluate((hidden) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (hidden ? "hidden" : "visible") });
        Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
        if (hidden) document.hasFocus = () => false;
        else delete document.hasFocus;
        document.dispatchEvent(new Event("visibilitychange"));
        window.dispatchEvent(new Event(hidden ? "blur" : "focus"));
      }, hidden);
    const ts = [];
    const work = async (title) => {
      fs.writeFileSync(`${root}/task-kinds-${ts.length}.md`, title);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-kinds-${ts.length}.md`);
      ts.push(t);
      const hash = await until(`${title} 보고`, () => aaa("thread", "show", "demo", t.id).report_hash);
      await until(`${title} 보고 사건 받음`, () => pp.evaluate((h) => window.__pushLog.includes(`thr ${h.slice(0, 6)}`), hash));
      await coordinatorPost();
      return t;
    };
    await doneSwitch(false);
    // (1) 보는 동안: 이 기기가 고르지 않은 종류라 앱 안 알림이 없다(이 화면이 그 사건을 받아 처리한 뒤)
    const kinds = (kind) => aaa("events", "demo").events.filter((e) => e.kind === kind);
    const skipped = kinds("push_skipped").length;
    await work("완료 알림 끈 동안 봄");
    await pp.evaluate(() => new Promise((r) => setTimeout(r)));
    if ((await pp.locator(".droplet").count()) !== 0) throw new Error("작업 완료를 껐는데 앱 안 알림이 내려왔다");
    // host가 이 보고를 정할 때까지(모으는 기다림 뒤 쓰는 화면이라 건너뜀). 기다리지 않고 가리면 이 보고가 (2)의 결정이 되고
    // (2)의 보고가 (3)으로 밀린다(ci-local 07:2x에서 (3)의 push가 (2)의 thread였다)
    await until("보는 동안 보고는 건너뜀", () => kinds("push_skipped").length > skipped);
    // (2) 가려진 동안: host는 이 기기에 보내지 않고 그 까닭을 남긴다
    await hide(true);
    await until("가려진 휴대폰", async () => (await pushInUse(page)) === 0);
    const k = kinds("push_unwanted").length;
    await work("완료 알림 끈 동안 닫음");
    const unwanted = await until("고르지 않은 종류 기록", () => kinds("push_unwanted")[k]);
    if (!/^기기 1개가 고른 알림 종류가 아니다 · 항목 1개$/.test(unwanted.detail)) throw new Error(`push_unwanted ${unwanted.detail}`);
    if (toDev(start).length !== 0) throw new Error(`작업 완료를 껐는데 push ${toDev(start).length}건`);
    // (3) 다시 켜면 보낸다
    await hide(false);
    await doneSwitch(true);
    await hide(true);
    await until("다시 가려진 휴대폰", async () => (await pushInUse(page)) === 0);
    await work("완료 알림 다시 켬");
    const { msg } = await nextPush(dev, start, "다시 켠 뒤 push");
    if (msg.tag !== "aaa-demo") throw new Error(JSON.stringify(msg));
    if (toDev(start).length !== 1) throw new Error(`이 기기에 push ${toDev(start).length}건`);
    if (phone.errors.length) throw new Error(`페이지 오류: ${phone.errors.join("; ")}`);
    await pp.context().close();
    for (const x of ts) aaa("thread", "resolve", "demo", x.id);
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse(page)) === 1);
  });

  await step("시작 질문(plan §8.328): thread가 구현 전에 [결정 필요]로 모아 묻고, coordinator가 확인 필요 post(제목은 질문 요약·추천)로 사용자에게 넘긴다. 앱을 보는 동안은 thread 표지의 앱 안 알림 하나뿐이고 post push는 건너뛴다. 앱을 닫은 동안은 앱 안 알림 없이 post push 하나가 가린 질문 요약·추천을 싣는다", async () => {
    const dev = pushDevice("push/dev-ask");
    const devPath = new URL(dev.json.endpoint).pathname;
    const toDev = (from) => pushes.slice(from).filter((x) => x.path === devPath);
    // 알림을 켜 둔 기기(위 「기기별 알림 종류」와 같다)
    const init = `(${(a) => {
      const log = [];
      Object.defineProperty(window, "__pushLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async () => {};
      const key = Uint8Array.from(atob(a.key.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)).buffer;
      const sub = { endpoint: a.sub.endpoint, options: { applicationServerKey: key }, toJSON: () => a.sub, unsubscribe: async () => true };
      PushManager.prototype.getSubscription = async () => sub;
      PushManager.prototype.subscribe = async () => sub;
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
    }})(${JSON.stringify({ key: pushKey, sub: dev.json })})`;
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    const start = pushes.length;
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init });
    const pp = phone.page;
    await until("열 때 다시 등록", async () => (await pp.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok)).subscribed === 1);
    await until("휴대폰을 봄", async () => (await pushInUse(page)) === 1);
    const hide = (hidden) =>
      pp.evaluate((hidden) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (hidden ? "hidden" : "visible") });
        Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
        if (hidden) document.hasFocus = () => false;
        else delete document.hasFocus;
        document.dispatchEvent(new Event("visibilitychange"));
        window.dispatchEvent(new Event(hidden ? "blur" : "focus"));
      }, hidden);
    const kinds = (kind) => aaa("events", "demo").events.filter((e) => e.kind === kind);
    const ts = [];
    // fake agent는 할 일에 「모아 묻기 시험」이 있으면 [결정 필요] 질문 보고만 쓴다. 그 보고를 받은 coordinator가 post를 보낸다
    const ask = async (title, question) => {
      fs.writeFileSync(`${root}/task-ask-${ts.length}.md`, `${title}\n모아 묻기 시험`);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-ask-${ts.length}.md`);
      ts.push(t);
      const hash = await until(`${title} 질문 보고`, () => {
        const v = aaa("thread", "show", "demo", t.id);
        return v.report_attention === "needs_user" && v.report_hash;
      });
      await until(`${title} 보고 사건 받음`, () => pp.evaluate((h) => window.__pushLog.includes(`thr ${h.slice(0, 6)}`), hash));
      await coordinatorPost({ request: true, title: question });
    };
    // (1) 보는 동안: host는 post push를 건너뛰고, 이 화면에는 thread 표지의 앱 안 알림(입력 필요) 하나만 내려온다
    const skipped = kinds("push_skipped").length;
    await ask("배치 질문 보는 중", "질문 1개 · 추천: A안");
    await until("보는 동안 post push 건너뜀", () => kinds("push_skipped").length > skipped);
    await until("thread 표지의 앱 안 알림", async () => (await pp.locator(".droplet-title").allInnerTexts()).includes("배치 질문 보는 중"));
    if ((await pp.locator(".droplet").count()) !== 1) throw new Error(`앱 안 알림 ${await pp.locator(".droplet").count()}개`);
    if ((await pp.locator(".droplet").getAttribute("data-kind")) !== "blocked") throw new Error(`앱 안 알림 종류 ${await pp.locator(".droplet").getAttribute("data-kind")}`);
    if (toDev(start).length !== 0) throw new Error(`보는 동안 push ${toDev(start).length}건`);
    await dismissObservedDroplet(pp);
    // (2) 닫은 동안: 앱 안 알림은 없고, post push 하나가 질문 요약·추천을 싣는다(경로는 가린다)
    await hide(true);
    await until("가려진 휴대폰", async () => (await pushInUse(page)) === 0);
    await ask("배치 질문 닫은 동안", "질문 1개 · 추천: A안(기존과 같음) ~/work/plan.md");
    const { msg } = await nextPush(dev, start, "닫은 동안 질문 push");
    const want = { title: "demo", body: "coordinator 사용자 결정 요청 · 질문 1개 · 추천: A안(기존과 같음) [숨김]", url: "/#threads", tag: "aaa-demo" };
    for (const [k, v] of Object.entries(want)) if (msg[k] !== v) throw new Error(`질문 push ${k}: ${JSON.stringify(msg)}`);
    await pp.evaluate(() => new Promise((r) => setTimeout(r)));
    if ((await pp.locator(".droplet").count()) !== 0) throw new Error("닫은 동안 앱 안 알림이 내려왔다");
    if (toDev(start).length !== 1) throw new Error(`이 기기에 push ${toDev(start).length}건`);
    if (phone.errors.length) throw new Error(`페이지 오류: ${phone.errors.join("; ")}`);
    await pp.context().close();
    for (const x of ts) aaa("thread", "resolve", "demo", x.id);
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse(page)) === 1);
  });
}
