// host 계정 화면(관리 GUI W5a, plan §8.351): 설정 창 「계정」에서 새 로그인·진행·취소, 메일·plan·최초 인증일·최근 인증된 날,
// 기본 지정·해제, 다시 로그인의 「이 메일로 로그인하세요」, 다른 메일이 들어왔을 때 고르기, 곧 만료 배지와 알림함 확인 필요,
// 보기 전용 기기(메일 없음), 휴대폰 390. host는 PATH 앞의 가짜 `claude`·`codex`를 공식 CLI 자리로 띄운다(실제 인증·브라우저 없음).
// 가짜 로그인은 시험이 FIFO에 쓸 때만 끝난다: 시간을 기다리지 않고 화면 표지와 host 기록을 본다.
import fs from "node:fs";
import fsp from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { root, aaa, step, browser, webkit, pairPage, domIs, shot, openSettings, inTime } from "../lib/harness.mjs";

const bin = `${root}/fakebin`;
fs.mkdirSync(bin, { recursive: true });
// 공식 CLI 흉내: `go-<kind>` FIFO에서 한 줄(ok·fail)을 받으면 결과를 남기고 끝난다. 결과 글은 시험이 미리 써 둔다
const fake = `#!/bin/sh
kind=$(basename "$0")
# 상태 표시용 판 조회는 로그인 FIFO를 열지 않는다.
if [ "$1" = --version ]; then printf "%s fixture-version\\n" "$kind"; exit 0; fi
read cmd < "${bin}/go-$kind"
[ "$cmd" = ok ] || exit 1
if [ "$kind" = codex ]; then
  cp "${bin}/codex-auth.json" "$CODEX_HOME/auth.json" && chmod 600 "$CODEX_HOME/auth.json"
else
  cat "${bin}/claude-out.txt"
  [ -f "${bin}/claude.json" ] && cp "${bin}/claude.json" "$CLAUDE_CONFIG_DIR/.claude.json"
fi
`;
for (const kind of ["claude", "codex"]) {
  fs.writeFileSync(`${bin}/${kind}`, fake, { mode: 0o755 });
  execFileSync("mkfifo", [`${bin}/go-${kind}`]);
  // FIFO에 쓰는 쪽이 없어도 --version은 즉시 끝나야 한다(로그인 신호를 소비하지 않는다).
  const version = execFileSync(`${bin}/${kind}`, ["--version"], { timeout: 10_000, encoding: "utf8" }).trim();
  if (version !== `${kind} fixture-version`) throw new Error(`가짜 CLI 판 조회: ${version}`);
}
// 계정 화면의 자동 사용량 수집도 공식 RPC 자리의 가짜 자식만 쓴다. 로그인 FIFO와 경합하지 않는다.
const quotaExe = `${bin}/quota-rpc`;
fs.writeFileSync(quotaExe, `#!${process.execPath}
import readline from 'node:readline';
for await (const line of readline.createInterface({input:process.stdin})) {
 const v=JSON.parse(line);
 if(v.method==='initialize') console.log(JSON.stringify({id:v.id,result:{}}));
 else if(v.method==='account/rateLimits/read') console.log(JSON.stringify({id:v.id,result:{rateLimitsByLimitId:{codex:{primary:{usedPercent:37,windowDurationMins:300,resetsAt:Math.floor(Date.now()/1000)+3600}}}}}));
}
`, { mode: 0o755 });
export const labEnv = { PATH: `${bin}:${process.env.PATH}`, AAA_TEST_QUOTA_CODEX: quotaExe };

const b64u = (s) => Buffer.from(s).toString("base64url");
/** 공식 Codex 로그인이 남기는 auth.json 꼴(id_token 본문에 메일·plan, 서명은 가짜) */
function codexResult(email, plan, n) {
  const claims = { email, "https://api.openai.com/auth": { chatgpt_plan_type: plan } };
  fs.writeFileSync(
    `${bin}/codex-auth.json`,
    JSON.stringify({ tokens: { access_token: `e2e-access-${n}-0123456789`, refresh_token: `e2e-refresh-${n}-0123456789`, id_token: `e30.${b64u(JSON.stringify(claims))}.c2ln` } }),
  );
}
/** 가짜 로그인을 놓아준다. 쓰기는 읽는 쪽이 FIFO를 열 때까지 막히므로 한도를 둔다 */
const release = (kind, word = "ok") => inTime(fsp.writeFile(`${bin}/go-${kind}`, `${word}\n`), 10_000, `가짜 ${kind} 로그인 놓기`);
const account = (name) => aaa("account", "list").accounts.find((a) => a.name === name);
const loginState = (pane, state) => domIs(pane.page(), `로그인 ${state}`, (s) => document.querySelector('[data-testid="account-login"]')?.dataset.state === s, state);
const row = (pane, name) => pane.locator(`[data-testid="account-row"][data-account="${name}"]`);

export default async function ({ page }) {
  let dialog;
  let pane;
  await step("새 계정 로그인: host Mac 브라우저 안내와 진행 → 끝나면 메일·plan·날짜·정상 배지", async () => {
    aaa("project", "create", "demo");
    await page.setViewportSize({ width: 1280, height: 800 });
    dialog = await openSettings(page, "계정");
    pane = dialog.getByTestId("settings-accounts");
    await pane.getByTestId("accounts-codex").or(pane.getByText("이 harness의 host 계정이 없다").first()).first().waitFor();
    codexResult("work@example.com", "plus", 1);
    await pane.getByTestId("account-alias").fill("work");
    await pane.getByTestId("account-harness").selectOption("codex");
    await pane.getByRole("button", { name: "로그인", exact: true }).click();
    await loginState(pane, "waiting");
    const card = await pane.getByTestId("account-login").innerText();
    if (!card.includes("host Mac 브라우저에서 인증을 마치세요")) throw new Error(`안내: ${card}`);
    await release("codex");
    await loginState(pane, "succeeded");
    await row(pane, "work").getByTestId("account-email").filter({ hasText: "work@example.com" }).waitFor();
    const a = account("work");
    if (a.plan !== "plus" || a.health !== "ok" || !a.first_auth_ms || a.first_auth_ms !== a.last_auth_ms) throw new Error(JSON.stringify(a));
    const text = await row(pane, "work").innerText();
    if (!text.includes("plus") || !text.includes("정상") || (text.match(/\d{4}-\d{2}-\d{2}/g) ?? []).length < 2) throw new Error(`줄: ${text}`);
    await shot(page, "58-accounts-desktop");
  });

  await step("기본으로 → 기본 배지와 host 기본 계정, 기본 해제", async () => {
    await pane.getByRole("button", { name: "work 기본으로" }).click();
    await row(pane, "work").locator(".acct-badge.is-default").waitFor();
    if (aaa("account", "default", "codex").defaults.find((d) => d.harness === "codex")?.account !== "work") throw new Error("host 기본");
    await pane.getByRole("button", { name: "work 기본 해제" }).click();
    await domIs(page, "기본 해제", () => !document.querySelector('[data-account="work"] .acct-badge.is-default'));
  });

  await step("로그인 취소: 진행 중에 취소하면 계정이 생기지 않는다", async () => {
    await pane.getByTestId("account-alias").fill("spare");
    await pane.getByTestId("account-harness").selectOption("claude");
    await pane.getByRole("button", { name: "로그인", exact: true }).click();
    await loginState(pane, "waiting");
    await pane.getByTestId("account-login").getByRole("button", { name: "취소" }).click();
    await loginState(pane, "cancelled");
    if (account("spare")) throw new Error("취소한 로그인이 계정을 만들었다");
    await pane.getByTestId("account-login").getByRole("button", { name: "닫기" }).click();
  });

  await step("Claude 로그인: 유효 기간 3일 → 곧 만료 배지, 알림함에 확인 필요 한 번(메일 없음)", async () => {
    // 공개 저장소 검사가 token 꼴 글을 막는다: 가짜 값도 실행 중에 만든다
    fs.writeFileSync(`${bin}/claude-out.txt`, `Your OAuth token (valid for 3 days):\nsk-ant-${"oat01"}-e2e-only-0123456789abcdef\n`);
    fs.writeFileSync(`${bin}/claude.json`, JSON.stringify({ oauthAccount: { emailAddress: "ci@example.com" } }));
    await pane.getByTestId("account-alias").fill("ci");
    await pane.getByTestId("account-harness").selectOption("claude");
    await pane.getByRole("button", { name: "로그인", exact: true }).click();
    await loginState(pane, "waiting");
    await release("claude");
    await loginState(pane, "succeeded");
    await row(pane, "ci").getByTestId("account-health").filter({ hasText: "곧 만료(3일 남음)" }).waitFor();
    await row(pane, "ci").getByTestId("account-email").filter({ hasText: "ci@example.com" }).waitFor();
    const alerts = aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.item_kind === "account" && !i.done);
    if (alerts.length !== 1 || alerts[0].subject !== "ci" || alerts[0].event !== "곧 만료" || alerts[0].summary.includes("@")) throw new Error(JSON.stringify(alerts));
    await pane.getByTestId("account-login").getByRole("button", { name: "닫기" }).click();
  });

  await step("다시 로그인: 「이 메일로 로그인하세요」, 다른 메일이면 옛 인증을 둔 채 고른다 → 새 메일로 바꾸기", async () => {
    codexResult("other@example.com", "pro", 2);
    await pane.getByRole("button", { name: "work 다시 로그인" }).click();
    await loginState(pane, "waiting");
    await pane.getByTestId("account-login").getByText("이 메일로 로그인하세요: work@example.com").waitFor();
    await release("codex");
    await loginState(pane, "mismatch");
    const card = await pane.getByTestId("account-login").innerText();
    if (!card.includes("로그인한 메일: other@example.com") || account("work").version !== 1) throw new Error(`고르기 전: ${card} ${JSON.stringify(account("work"))}`);
    await shot(page, "58-accounts-mismatch-desktop");
    await pane.getByRole("button", { name: "새 메일로 바꾸기" }).click();
    await loginState(pane, "succeeded");
    await row(pane, "work").getByTestId("account-email").filter({ hasText: "other@example.com" }).waitFor();
    const a = account("work");
    if (a.version !== 2 || a.plan !== "pro") throw new Error(JSON.stringify(a));
    await pane.getByTestId("account-login").getByRole("button", { name: "닫기" }).click();
  });

  await step("이름 바꾸기: 두 엔진 × 1280/390에서 입력 검증·충돌·성공·기본과 인증 기록 유지", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      let index = 0;
      for (const [engine, label] of [[browser, "chromium"], [wk, "webkit"]]) {
        for (const width of [1280, 390]) {
          const { page: view } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
          try {
            const settings = await openSettings(view, "계정");
            const p = settings.getByTestId("settings-accounts");
            await row(p, "work").getByTestId("account-email").waitFor();
            aaa("account", "default", "codex", "work");
            const before = account("work");
            await row(p, "work").getByRole("button", { name: "work 이름 바꾸기", exact: true }).click();
            const input = row(p, "work").getByRole("textbox", { name: "work 새 이름", exact: true });
            const save = row(p, "work").getByRole("button", { name: "이름 저장", exact: true });
            await input.fill("bad/name");
            if (!(await save.isDisabled())) throw new Error("잘못된 이름 저장 가능");
            await row(p, "work").getByText("영문·숫자·-·_ 1~64자를 입력하세요.", { exact: true }).waitFor();
            await input.fill("ci");
            if (!(await save.isDisabled())) throw new Error("중복 이름 저장 가능");
            await row(p, "work").getByText("이미 쓰는 계정 이름입니다.", { exact: true }).waitFor();
            // 화면을 연 뒤 다른 기기에서 생긴 중복은 host가 거부하고 폼·인증은 그대로 남는다.
            const next = `renamed-${++index}`;
            await input.fill(next);
            const formFits = await p.getByTestId("account-rename").evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.getBoundingClientRect().right <= innerWidth + 1);
            if (!formFits) throw new Error(`${label} ${width} 이름 입력 가로 넘침`);
            await shot(view, `58-account-rename-form-${label}-${width}`);
            const collision = view.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "account_rename");
            // WebKit의 Service Worker가 있는 요청도 같은 경계를 거치게 원래 fetch 직전만 붙잡는다.
            // 실제 URL·본문·헤더·응답은 원래 fetch에 그대로 넘긴다.
            await view.exposeFunction("beforeAccountRename", (body) => {
              if (body.to !== next) throw new Error(`저장할 별칭: ${body.to}`);
              aaa("account", "import", next, "--harness", "codex", "--file", `${bin}/codex-auth.json`);
            });
            await view.evaluate(() => {
              const fetch = window.fetch.bind(window);
              let armed = true;
              window.fetch = async (input, init) => {
                const body = typeof init?.body === "string" ? JSON.parse(init.body) : null;
                if (armed && body?.op === "account_rename") {
                  armed = false;
                  await window.beforeAccountRename(body);
                }
                return fetch(input, init);
              };
            });
            await save.click();
            const result = await (await collision).json();
            if (result.err?.code !== "name_taken") throw new Error(`이름 충돌 응답: ${JSON.stringify(result)}`);
            await p.getByRole("alert").filter({ hasText: "이미 쓰는 계정 이름" }).waitFor();
            if (!account("work") || account("work").version !== before.version) throw new Error("실패 뒤 원본 변경");
            aaa("account", "remove", next);
            await input.fill(`${next}-ok`);
            await save.click();
            await row(p, `${next}-ok`).getByTestId("account-email").waitFor();
            await p.getByTestId("account-renamed").filter({ hasText: `work → ${next}-ok` }).waitFor();
            const after = account(`${next}-ok`);
            for (const key of ["harness", "version", "email", "plan", "first_auth_ms", "last_auth_ms"]) {
              if (after[key] !== before[key]) throw new Error(`이름 변경 뒤 ${key} 변경`);
            }
            if (account("work") || aaa("account", "default", "codex").defaults.find((d) => d.harness === "codex")?.account !== `${next}-ok`) throw new Error("옛 별칭·기본 참조");
            const fits = await p.evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.getBoundingClientRect().right <= innerWidth + 1);
            if (!fits) throw new Error(`${label} ${width} 이름 변경 가로 넘침`);
            await shot(view, `58-account-rename-${label}-${width}`);
            // 실제 CLI 경로로 되돌려 다음 화면도 같은 fixture로 검사한다.
            aaa("account", "rename", `${next}-ok`, "work");
          } finally { await view.context().close(); }
        }
      }
    } finally { await wk.close(); }
  });

  await step("보기 전용 기기: 별칭·상태만, 메일·로그인 단추 없음", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    const d = await openSettings(rd, "계정");
    const p = d.getByTestId("settings-accounts");
    await row(p, "ci").getByTestId("account-health").waitFor();
    const text = await p.innerText();
    if (text.includes("@example.com") || (await p.getByTestId("account-email").count()) !== 0) throw new Error("보기 기기에 메일");
    if ((await p.getByRole("button", { name: /이름 바꾸기|다시 로그인|기본으로|^로그인$/ }).count()) !== 0) throw new Error("보기 기기에 로그인 단추");
    const denied = await rd.evaluate(async () => (await fetch('/api/v1/call', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({op:'account_rename',name:'work',to:'denied'}) })).json());
    if (denied.err?.code !== "forbidden") throw new Error(`보기 기기 이름 변경: ${JSON.stringify(denied)}`);
    await rd.context().close();
  });

  await step("휴대폰 390: 계정 페이지가 폭 안에 들어온다", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const d = await openSettings(ph, "계정");
    const p = d.getByTestId("settings-accounts");
    await row(p, "work").getByTestId("account-email").waitFor();
    const fits = await ph.evaluate(() => {
      const el = document.querySelector('[data-testid="settings-accounts"]');
      const box = el.getBoundingClientRect();
      return box.left >= 0 && box.right <= innerWidth + 0.5 && el.scrollWidth <= el.clientWidth + 1 ? true : JSON.stringify({ l: box.left, r: box.right, sw: el.scrollWidth, cw: el.clientWidth });
    });
    if (fits !== true) throw new Error(`휴대폰 폭: ${fits}`);
    await shot(ph, "58-accounts-phone");
    await ph.context().close();
  });
}
