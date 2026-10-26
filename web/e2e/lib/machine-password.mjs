import { freshToken } from "../../../scripts/fixture-secret.mjs";
// Windows 화면 자료만 fixture다. 입력·취소·요청·권한 거부는 실제 웹과 host를 쓴다. 가짜 암호는 실행 중에만 만든다.
import fs from "node:fs";
import { root, aaa, pairPage, browser, webkit, openSettings } from "./harness.mjs";

export async function machinePassword() {
  const password = freshToken() + "한글 '\\";
  const job = {
    job: "01ARZ3NDEKTSV4RRFFQ69G5FAV", target: "pc-b", phase: "review", ssh: null, host_keys: [],
    host_key_state: "known", detected: { os: "windows", arch: "x86_64", raw: "Windows AMD64", chosen: false, home: "C:/Users/me/", shell: null, aaa: null },
    error: null, created_ms: 1, updated_ms: 1,
    settings: { name: "pc-b", host: "mac-a.example.ts.net:7430", work_root: "C:/aaa-work", work_mount: null, harness: [], login: "defer" },
    review: { plan_sha256: "a".repeat(64), binary_sha256: "b".repeat(64), binary_version: "aaa 0.1.5", changes: [], checks: [], needs_user: [], operations: ["register_service"], service_password: true },
  };
  const wk = await webkit.launch({ headless: true });
  try {
  for (const engine of [browser, wk]) for (const width of [1280, 390]) {
    let applied = 0;
    let listed = 0;
    let stage = "화면 열기";
    let leaked = false;
    const controller = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
      serviceWorkers: "block",
      setup: async (context) => {
        await context.route("**/api/v1/call", async (route) => {
          const req = route.request().postDataJSON();
          if (req.op === "machine_setup_list") {
            listed++;
            await route.fulfill({ json: { ok: { kind: "machine_setups", setups: [job] } } });
          } else {
            if (JSON.stringify(req).includes(password)) {
              if (req.op !== "machine_setup_apply" || req.service_password !== password) leaked = true;
              applied++;
            }
            // 가짜 job의 apply는 실제 host가 NotFound로 거부한다. 값은 응답에 없어야 한다.
            const response = await route.fetch();
            if ((await response.text()).includes(password)) leaked = true;
            await route.fulfill({ response });
          }
        });
      },
    });
    const p = controller.page;
    p.on("console", (message) => { if (message.text().includes(password)) leaked = true; });
    p.on("request", (request) => {
      if (request.url().includes(password) || (request.url().includes("/api/") && !request.url().endsWith("/api/v1/call") && (request.postData() ?? "").includes(password))) leaked = true;
    });
    try {
      await openSettings(p, "머신");
      const open = async () => {
        stage = "설치 확인 단추";
        await p.getByTestId("machine-setup-apply").click();
        const input = p.getByTestId("machine-setup-password");
        await input.waitFor();
        if (await input.inputValue()) throw new Error("암호 입력이 재사용됐다");
        if (await input.getAttribute("type") !== "password" || await input.getAttribute("autocomplete") !== "off") throw new Error("전용 가림/저장 금지 입력이 아니다");
        await p.getByRole("alertdialog").getByText(/coordinator 대화창·질문 답·할 일 메모/).waitFor();
        // locator.fill의 실패 진단에도 원문이 들어가지 않게 DOM 입력에만 직접 넣는다.
        await input.evaluate((el, value) => { el.value = value; }, password);
      };
      await open();
      await p.getByRole("alertdialog").getByRole("button", { name: "취소", exact: true }).click();
      if (applied) throw new Error("취소한 암호를 보냈다");
      await open();
      await p.getByRole("alertdialog").getByRole("button", { name: "설치하고 등록", exact: true }).click();
      await p.getByRole("alertdialog").getByRole("alert").waitFor();
      if (applied !== 1 || await p.getByTestId("machine-setup-password").inputValue()) throw new Error("일회 전달/실패 뒤 지우기 실패");
      const privateFree = await p.evaluate((needle) => {
        const visible = [document.body.innerText, location.href, JSON.stringify(history.state), JSON.stringify(localStorage), JSON.stringify(sessionStorage)];
        return visible.every((text) => !text.includes(needle)) && !Array.from(document.querySelectorAll("input,textarea")).some((el) => el.value.includes(needle));
      }, password);
      if (!privateFree || leaked) throw new Error("암호가 대화/응답/로그/브라우저 상태로 샜다");
      await p.getByRole("alertdialog").getByRole("button", { name: "취소", exact: true }).click();
      await open();
      await p.evaluate(() => window.dispatchEvent(new Event("pagehide")));
      if (await p.getByTestId("machine-setup-password").inputValue()) throw new Error("페이지 종료에 암호를 비우지 않았다");
      await p.evaluate(() => window.dispatchEvent(new Event("pageshow")));
    } catch (error) {
      const state = await p.evaluate(() => ({ setups: document.querySelectorAll('[data-testid="machine-setup"]').length,
        phases: Array.from(document.querySelectorAll('[data-job]')).map((e) => e.getAttribute('data-phase')) }));
      throw new Error(`${engine.browserType().name()} ${width}px ${stage}, list=${listed}, DOM=${JSON.stringify(state)}, errors=${controller.errors.length}: ${error.message}`);
    } finally { await p.context().unrouteAll({ behavior: "wait" }); await p.context().close(); }
  }
  } finally { await wk.close(); }
  // 실제 host의 DB/WAL·inbox·질문·작업·보고·library·실험실 harness 기록 전체. 운영 ~/.claude·~/.codex에는 접근하지 않는다.
  const needle = Buffer.from(password);
  const scan = (dir) => {
    for (const file of fs.readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${file.name}`;
      if (file.isDirectory()) scan(path);
      else if (file.isFile() && fs.readFileSync(path).includes(needle)) throw new Error("실험실 영속 기록에 암호가 있다");
    }
  };
  scan(root);
}
