// 웹 E2E 멈춘 쓰는 화면(plan §8.133): 쓰는 기기의 페이지가 멈추면 lease 뒤 쓰는 화면에서 빠지고 push가 간다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 18-push-stalled(새 실험실). 준비는 runSpec과 pushLab(서명 키·구독 하나·demo).
// 이 파일만 host lease를 줄인다(labEnv): 한 실험실 안의 모든 화면에 듣는다
import fs from "node:fs";
import {
  root, aaa, until, pairPage, pushes, pushDevice, step, browser, setInUse, pushInUse, pushWho, pushLab,
} from "../lib/harness.mjs";

// 「쓰는 화면」 lease 4초(기본 25초, web.rs presence_lease). 쓰는 동안 남아야 하는 화면은 presence를 lease보다 자주 알린다(아래 stallInit)
export const labEnv = { AAA_TEST_PRESENCE_LEASE_MS: "4000" };

export default async function ({ page }) {
  await pushLab(page);

  await step("멈춘 쓰는 화면(coordinator 실기 10-18 07:39 iPhone: 「쓰는 화면」으로 push 생략, 앱 안 알림도 없음, plan §8.133): 쓰는 기기의 페이지가 멈추면(timer·소켓 사건이 서고 보내지 못한다, 브라우저 네트워크 층은 WebSocket pong을 계속 한다) host는 앱 글이 lease(25초) 넘게 없을 때 쓰는 화면에서 빼고 push를 보낸다. 다시 깨면 presence가 다시 세어진다. push 판정 근거와 앱 안 알림 결과가 흐름 사건에 남는다", async () => {
    const events = (kind) => aaa("events", "demo").events.filter((e) => e.kind === kind);
    const ts = [];
    let n = 0;
    const work = (title) => {
      fs.writeFileSync(`${root}/task-frozen-${++n}.md`, title);
      ts.push(aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-frozen-${n}.md`));
    };
    const done = () => {
      while (ts.length) aaa("thread", "resolve", "demo", ts.shift().id);
    };
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    // B(휴대폰): 보이고 focus, 누른 기기라 쓰는 기기다. 소리를 켜 둔다(아래 (3) 늦은 소리).
    // 페이지 멈춤(iOS가 뒤로 간 앱을 멈춘 것): headless Chromium은 CDP Page.setWebLifecycleState frozen으로 보이는 페이지를
    // 멈추지 않는다(timer가 계속 돈다, 최소화해도 가려지지 않는다). 그래서 페이지 안에서 멈춘다: 멈춘 동안 timer 일은 돌지
    // 않고(setTimeout은 깰 때 돈다), 상태 사건 소켓은 보내지 못하며 받은 것은 깰 때 차례대로 온다. pong은 브라우저가 한다
    const stallInit = () => {
      localStorage.setItem("aaa:alert-sound", "1");
      let frozen = false;
      const held = [];
      const realInterval = window.setInterval.bind(window);
      const realTimeout = window.setTimeout.bind(window);
      // 이 파일의 host lease는 4초다(labEnv): 쓰는 동안 presence 다시 알림(웹 PRESENCE_EVERY_MS 10초, 10초 setInterval은 그것 하나)을
      // 이 화면만 1초로 당긴다. 단계가 당긴 수를 본다(window.__beats)
      window.__beats = 0;
      window.setInterval = (fn, ms, ...args) => {
        if (ms === 10_000) window.__beats++;
        return realInterval(() => frozen || fn(...args), ms === 10_000 ? 1_000 : ms);
      };
      window.setTimeout = (fn, ms, ...args) =>
        realTimeout(function run() {
          if (frozen) held.push(run);
          else fn(...args);
        }, ms);
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          this.addEventListener("message", (e) => {
            if (!frozen || e.replayed) return;
            e.stopImmediatePropagation();
            held.push(() => {
              const again = new MessageEvent("message", { data: e.data });
              again.replayed = true;
              this.dispatchEvent(again);
            });
          });
        }
        send(d) {
          if (!frozen) super.send(d);
        }
      };
      window.__stall = () => void (frozen = true);
      window.__wake = () => {
        frozen = false;
        for (const run of held.splice(0)) run();
      };
    };
    const B = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init: stallInit });
    const bp = B.page;
    await bp.evaluate(() => window.dispatchEvent(new PointerEvent("pointerdown")));
    await until("B가 쓰는 기기·쓰는 화면", async () => (await pushInUse(page)) === 1 && (await pushWho(bp)).you);
    const bId = (await pushWho(bp)).user;
    if ((await bp.evaluate(() => window.__beats)) !== 1) throw new Error(`presence 다시 알림(10초 setInterval) ${await bp.evaluate(() => window.__beats)}개: stallInit이 당길 timer를 다시 고른다`);
    // (1) 살아 있는 B: 건너뛰고, 그 근거(쓰는 기기를 알린 길, 연결의 presence 까닭·화면 값, 앱 글)와 B가 띄운 앱 안 알림이 남는다
    let k = events("push_skipped").length;
    let a = events("alert_outcome").length;
    work("멈추기 전");
    const s1 = await until("살아 있는 B 때문에 건너뜀", () => events("push_skipped").slice(k)[0]);
    const basis1 = new RegExp(`근거: 쓰는 기기 ${bId}\\(touch \\d+초 전\\) · 연결#\\d+ 쓰는 중\\(presence \\d+초 전 true (open|focus|heartbeat) 보임·focus, 앱 글 \\d+초 전`);
    if (!basis1.test(s1.detail)) throw new Error(`push_skipped 근거 ${s1.detail}`);
    // 그 연결의 웹 빌드(주 묶음 파일 이름): 배포 뒤 옛 페이지가 도는지 가린다(plan §8.150)
    if (!/, 웹 index-[\w-]+\)/.test(s1.detail)) throw new Error(`push_skipped 근거에 웹 빌드가 없다 ${s1.detail}`);
    // B의 앱 안 알림 결과. 흐름 사건의 기기 이름은 앞 7자(ULID 시각, 약 33초 폭)라 몇 초 안에 짝지은 첫 화면도 같은 이름이다
    // (t-0338 report 「찾은 결함」). 첫 화면은 focus가 없고 멈추지 않아(위 setInUse) 그 결과에는 늘 「focus 없음」이 있다: 그것을 뺀 것이 B의 것이다.
    // 첫 화면의 상태 사건 연결이 이 단계 안에서 다시 붙지 않는다는 전제다(다시 붙으면 놓친 사건을 늦은 알림으로 읽어, 구독 dev1을 가진
    // 그 기기도 「push로 받음 · 안 띄움」을 남길 수 있다)
    const outcomeOfB = (from, at) =>
      until(`B의 앱 안 알림 결과(thread=${at})`, () =>
        events("alert_outcome").slice(from).find((e) => e.detail.startsWith(`기기 ${bId} · done · thread=${at}`) && !e.detail.includes(" · focus 없음 · ")),
      ).catch((e) => {
        throw new Error(`${e.message}: ${events("alert_outcome").slice(from).map((x) => x.detail).join(" | ")}`);
      });
    const o1 = await outcomeOfB(a, "");
    if (!/ · 띄움 · focus · /.test(o1.detail)) throw new Error(`alert_outcome ${o1.detail}`);
    await until("B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("멈추기 전"));
    await bp.locator(".droplet-close").click();
    done();
    // (2) B가 멈춘다: 앱 글(이 화면은 1초마다 presence)이 끊긴다. lease가 끝나면 쓰는 화면이 아니다(연결은 그대로)
    await bp.evaluate(() => window.__stall());
    await until("lease 끝", async () => (await pushInUse(page)) === 0);
    const m = events("push_sent").length;
    k = events("push_skipped").length;
    work("멈춘 동안");
    const sent = await until("멈춘 B 대신 push 보냄", () => events("push_sent").slice(m)[0]);
    if (events("push_skipped").length !== k) throw new Error(`멈춘 B 때문에 건너뜀: ${JSON.stringify(events("push_skipped").slice(k))}`);
    // host가 연결을 끊은 것이 아니라 lease로 뺐다(pong은 왔다)
    const frozenId = ts[0].id;
    if (!new RegExp(`근거: 쓰는 기기 ${bId}\\(touch \\d+초 전\\) · 연결#\\d+ 안 씀\\(presence \\d+초 전 lease 끝\\(앱 글 \\d+초 없음\\), 앱 글 \\d+초 전, pong \\d+초 전`).test(sent.detail))
      throw new Error(`push_sent 근거 ${sent.detail}`);
    done();
    // (3) 다시 깬다: 다음 presence로 다시 쓰는 화면. 멈춘 동안 쌓인 보고는 이 기기에 push가 가지 않았으므로(B는 구독이 없다)
    // 앱 안 알림으로 내려오되 소리는 버린다(늦은 소리, coordinator 실기 10-18 07:45 「알림 순간에 못 내면 버린다」)
    a = events("alert_outcome").length;
    await bp.evaluate(() => window.__wake());
    await until("깨면 다시 쓰는 화면", async () => (await pushInUse(page)) === 1);
    const late = await outcomeOfB(a, `demo%2F${frozenId} · `);
    if (!/ · 띄움 · focus · 늦어 소리 버림 · 웹 index-[\w-]+$/.test(late.detail)) throw new Error(`alert_outcome ${late.detail}`);
    // (4) 이 기기에 이미 push로 간 알림은 깨어나도 앱 안에서 다시 띄우지 않는다(사용자 결정 10-18 「A」, plan §8.141).
    // B가 push 구독을 가진 채 멈춘다: lease 뒤 B에 push가 가고, 깨면 그 보고는 「push로 받음 · 안 띄움」
    const bDev = pushDevice("push/dev-frozen");
    const bPath = new URL(bDev.json.endpoint).pathname;
    const subscribe = (sub) =>
      fetch("/api/v1/push/subscribe", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(sub) }).then((r) => r.ok);
    if (!(await bp.evaluate(subscribe, bDev.json))) throw new Error("B push 구독 실패");
    await bp.evaluate(() => window.__stall());
    await until("다시 lease 끝", async () => (await pushInUse(page)) === 0);
    const fromPush = pushes.length;
    work("멈춘 동안 push로 받음");
    const pushedId = ts[0].id;
    await until("멈춘 B에 push", () => pushes.slice(fromPush).some((x) => x.path === bPath));
    done();
    a = events("alert_outcome").length;
    await bp.evaluate(() => window.__wake());
    const got = await outcomeOfB(a, `demo%2F${pushedId} · `);
    if (!/ · push로 받음 · 안 띄움 · 웹 index-[\w-]+$/.test(got.detail)) throw new Error(`alert_outcome ${got.detail}`);
    // 결과를 알린 뒤에 본다(판단이 끝났다)
    if ((await bp.locator(".droplet-title").allInnerTexts()).includes("멈춘 동안 push로 받음")) throw new Error("push로 받은 알림이 앱 안에 다시 떴다");
    // (5) 실기 10-18 10:22 iPhone(#90 push_sent·#91 「띄움」, plan §8.150): 앱을 내리면 가려지고(presence false) 멈춘다. 10초
    // 넘어 다시 올리면 웹은 묻지 않고 새로 붙는다(쌓인 사건은 버린다) → 놓친 보고를 다시 읽는다. 그 사이 이 기기에 간 push는
    // 가려진 때부터 봐야 한다(끊긴 때 = 다시 올린 때부터 보면 그 push를 놓친다). 다시 올릴 때 visibilitychange가 깨어남보다 먼저다
    await bp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("B 가려짐", async () => (await pushInUse(page)) === 0);
    await bp.evaluate(() => window.__stall());
    const fromPush2 = pushes.length;
    work("가려진 동안 push로 받음");
    const hiddenId = ts[0].id;
    await until("가려진 B에 push", () => pushes.slice(fromPush2).some((x) => x.path === bPath));
    // 열린 채 둔다: 다시 붙어 읽는 목록은 열린 thread만이다(실기에서도 열려 있었다)
    a = events("alert_outcome").length;
    // 다시 올림: 10초 넘게 가려졌다(시계만 앞으로) → 보임 → 깨어남
    await bp.evaluate(() => {
      const real = Date.now;
      Date.now = () => real() + 11_000;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
      document.dispatchEvent(new Event("visibilitychange"));
      window.__wake();
    });
    const back = await outcomeOfB(a, `demo%2F${hiddenId} · `);
    if (!/ · push로 받음 · 안 띄움 · 웹 index-[\w-]+$/.test(back.detail)) throw new Error(`alert_outcome ${back.detail}`);
    if ((await bp.locator(".droplet-title").allInnerTexts()).includes("가려진 동안 push로 받음")) throw new Error("가려진 동안 push로 받은 알림이 앱 안에 다시 떴다");
    done();
    if (B.errors.length) throw new Error(`페이지 오류: ${B.errors.join("; ")}`);
    await bp.context().close();
  });
}
