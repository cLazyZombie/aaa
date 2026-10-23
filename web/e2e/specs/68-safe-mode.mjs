// 안전 모드 W6b(plan §8.370): 정상 시작이 그 설정을 「마지막 정상 설정」으로 보관한다. host.toml을 깨고 host를 다시 띄우면 끝나지 않고
// 같은 주소에 안전 모드 웹만 연다: 조작 기기는 오류 줄 표시와 가린 원문(env 값 없음), 보기 전용 기기는 안내만 본다. API는 503
// unavailable이고, 열려 있던 앱은 그 글을 보인다. 「마지막 정상 설정으로 되돌리기」는 지금 파일을 옆에 남기고 정상 설정으로 바꿔, 같은 프로세스가 정상 host로 이어 간다.
import fs from "node:fs";
import net from "node:net";
import { once } from "node:events";
import { root, aaa, step, start, stop, until, browser, pairPage, shot, base, procs, inTime, domIs } from "../lib/harness.mjs";
import { SafeModeDiagnostics } from "../lib/safe-mode-diagnostics.mjs";

const toml = `${root}/c/host.toml`;
const kept = `${root}/c/last-good/host.toml`;
const secret = "e2e-safe-secret-value";

export default async function ({ page }) {
  const diag = new SafeModeDiagnostics(root, procs);
  diag.watchPage("controller", page);
  let control;
  const held = Promise.withResolvers();
  const gate = net.createServer((socket) => {
    control = socket;
    let text = "";
    socket.on("data", (chunk) => {
      text += chunk;
      if (text.includes("\n")) held.resolve({ waiting: text, control: socket });
    });
    socket.on("error", (error) => diag.record("control-error", { error: String(error) }));
  });
  gate.listen(`${root}/reload.sock`);
  await once(gate, "listening");
  try {
    await safeMode(page, diag, held);
  } finally {
    control?.destroy();
    gate.close();
  }
}

async function safeMode(page, diag, held) {
  const diagnose = (e) => diag.save(e);
  let good, reader, phone;
  await step("정상 시작: 고정 port 설정을 마지막 정상 설정으로 보관한다", async () => {
    // 실험실 host는 :0으로 뜬 뒤 port가 적힌다: 한 번 다시 띄워 그 설정을 보관하게 한다
    reader = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 })).page;
    phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    diag.watchPage("reader", reader);
    diag.watchPage("phone", phone);
    await stop("host");
    good = fs.readFileSync(toml, "utf8");
    diag.watchProcess("host-good", start("host-good", "host"));
    await until("마지막 정상 설정 보관", () => fs.existsSync(kept) && fs.readFileSync(kept, "utf8") === good);
    await until("labA 다시 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
  }, { diagnose });

  let bad;
  await step("설정을 깨면 같은 주소에 안전 모드: 오류 줄·가린 원문, API는 503 unavailable", async () => {
    await stop("host-good");
    fs.writeFileSync(toml, `${good}\n[profiles.fake.env]\nE2E_LABEL = "${secret}"\n[broken\n`);
    bad = fs.readFileSync(toml, "utf8");
    const badLine = bad.split("\n").indexOf("[broken") + 1;
    diag.watchProcess("host-safe", start("host-safe", { AAA_TEST_SAFE_RELOAD_HOLD: `${root}/reload.sock` }, "host"));
    const me = await until("안전 모드 웹", async () => {
      const res = await fetch(`${base}api/v1/me`);
      return res.status === 503 && (await res.json());
    });
    if (me.err?.code !== "unavailable" || !me.err.message.includes("안전 모드")) throw new Error(`me: ${JSON.stringify(me)}`);
    // 열려 있던 앱: 다시 붙기 전 기기 확인이 안전 모드 답을 받아 그 글을 보인다(끊김 배너에 머물지 않는다)
    await page.getByTestId("unreachable").getByText("안전 모드").waitFor();
    await shot(page, "68-open-app-desktop");
    await page.goto(base);
    await page.getByRole("heading", { name: "aaa 안전 모드" }).waitFor();
    await page.getByTestId("safe-errors").getByText(`host.toml:${badLine}:`).waitFor();
    const marked = await page.locator(`pre[data-file="host.toml"] span.err`).allTextContents();
    if (!marked.some((t) => t.includes("[broken"))) throw new Error(`오류 줄 표시: ${marked}`);
    await page.locator('pre[data-file="host.toml"]').getByText("E2E_LABEL = «가림»").waitFor();
    if ((await page.content()).includes(secret)) throw new Error("env 값이 화면에 보인다");
    await shot(page, "68-safe-desktop");
  }, { diagnose });

  await step("보기 전용 기기는 안내만, 휴대폰 390은 폭 안에", async () => {
    await reader.goto(base);
    await reader.getByText("보기 전용 기기에는 설정 원문과 되돌리기를 보이지 않습니다").waitFor();
    if (await reader.getByRole("button", { name: "마지막 정상 설정으로 되돌리기" }).count()) throw new Error("보기 전용 기기에 되돌리기가 보인다");
    if (await reader.locator("pre").count()) throw new Error("보기 전용 기기에 원문이 보인다");
    await phone.goto(base);
    await phone.getByRole("button", { name: "마지막 정상 설정으로 되돌리기" }).waitFor();
    const wide = await phone.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (wide) throw new Error("안전 모드 화면이 가로로 넘친다");
    await shot(phone, "68-safe-phone");
  }, { diagnose });

  await step("되돌리기: 지금 파일은 옆에 남기고 정상 설정으로, 같은 프로세스가 정상 host로 이어 간다", async () => {
    await page.getByRole("button", { name: "마지막 정상 설정으로 되돌리기" }).click();
    await page.getByTestId("safe-reloading").waitFor();
    await shot(page, "68-restored-desktop");
    if (fs.readFileSync(toml, "utf8") !== good) throw new Error("host.toml이 정상 설정으로 돌아오지 않았다");
    const backups = fs.readdirSync(`${root}/c`).filter((n) => n.startsWith("host.toml.broken-"));
    if (backups.length !== 1 || fs.readFileSync(`${root}/c/${backups[0]}`, "utf8") !== bad) throw new Error(`남긴 파일: ${backups}`);
    const { waiting, control } = await inTime(held.promise, 20_000, "정상 host 시작 전 제어 표지");
    if (waiting !== "waiting\n") throw new Error(`복원 제어 표지: ${waiting}`);
    diag.record("reload-held");
    // 정상 host가 아직 시작되지 않아도 같은 주소가 TCP 요청을 받아야 한다.
    // 고정 sleep이나 부하 대신 실제 serving 사이의 경계를 잡는다.
    const refreshed = page.waitForEvent("request", {
      predicate: (r) => r.isNavigationRequest() && r.method() === "GET" && r.url() === base,
      timeout: 20_000,
    });
    // 접속 단언이 먼저 실패해도 남은 event waiter가 unhandled rejection을 만들지 않는다.
    refreshed.catch(() => {});
    const probe = net.connect(Number(new URL(base).port), "127.0.0.1");
    try {
      await inTime(once(probe, "connect"), 20_000, "복원 중 웹 접속");
      diag.record("reload-tcp-connected");
      await refreshed;
      diag.record("refresh-requested-while-held");
    } finally {
      probe.destroy();
      control.end("R");
    }
    // 안내 화면이 스스로 다시 연다(Refresh): 정상 host의 앱이 뜬다
    await page.getByTestId("pane-t1").waitFor({ timeout: 20_000 });
    await until("labA 다시 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
    await page.getByTestId("pane-t1").click();
    await domIs(page, "기존 PTY 출력 복원", () => document.querySelector(".xterm-rows")?.textContent.includes("t1-42"));
    if (procs.get("host-safe")?.exitCode !== null) throw new Error("되돌린 뒤 안전 모드를 연 host 프로세스가 끝났다");
  }, { diagnose });
}
