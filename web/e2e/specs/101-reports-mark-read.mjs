// 실제 host DB에 105개를 보내 첫 50개 밖의 안 봄도 한 요청으로 처리한다.
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, step, shot, browser, webkit, pairPage } from "../lib/harness.mjs";

export default async function () {
  await step("보고서 모두 읽음: 확인·취소·오류·탭 범위·배지·Reader·휴대폰", async () => {
    aaa("project", "create", "demo");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    const put = db.prepare("INSERT INTO library_reports(project,name,updated,to_user,title,sent_at) VALUES('demo',?,1,1,?,1)");
    const reset = () => db.exec("UPDATE library_reports SET seen_at=NULL WHERE to_user=1");
    for (let i = 1; i <= 105; i++) {
      const name = `sample-${i}`;
      fs.mkdirSync(`${root}/projects/demo/library/${name}`, { recursive: true });
      fs.writeFileSync(`${root}/projects/demo/threads/${name}.files.json`, "[]");
      put.run(name, `완료 보고 ${i}`);
    }
    // 보관함 한 줄은 내 보고서 일괄 처리의 범위 밖이다.
    fs.mkdirSync(`${root}/projects/demo/library/r-0001`, { recursive: true });
    fs.writeFileSync(`${root}/projects/demo/threads/r-0001.files.json`, "[]");
    put.run("r-0001", "보관 보고");
    db.exec("INSERT INTO report_ids(project,seq,id,source,revision,ready) VALUES('demo',1,'r-0001','archive-source','fixture',1)");
    db.exec("INSERT INTO library_archives(project,name) VALUES('demo','r-0001')");
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
        for (const width of [1280, 390]) {
          reset();
          let requests = 0, fail = false;
          const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
            serviceWorkers: "block",
            setup: (ctx) => ctx.route("**/api/v1/call", async (route) => {
              if (route.request().postDataJSON()?.op !== "library_mark_read") return route.fallback();
              requests++;
              if (fail) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "실험실 읽음 저장 실패" } }) });
              return route.fallback();
            }),
          });
          const page = paired.page;
          try {
            await inList(page, "activity-reports");
            await page.getByTestId("reports-mark-read").waitFor();
            await page.getByTestId("reports-upload").waitFor();
            await domIs(page, "105개 안 봄", () => document.querySelector('[data-testid="reports-unseen"]')?.getAttribute("aria-label") === "안 본 보고서 106");
            await domIs(page, "첫 50개", () => document.querySelectorAll('.report-mine').length === 50);
            await shot(page, `reports-mark-read-${label}-${width}`);
            await page.getByTestId("reports-mark-read").click();
            const dialog = page.getByRole("alertdialog");
            await dialog.getByText("내 보고서의 안 본 보고서 105개를 읽음으로 바꾼다.", { exact: true }).waitFor();
            await shot(page, `reports-mark-read-confirm-${label}-${width}`);
            await dialog.getByRole("button", { name: "취소", exact: true }).click();
            if (requests !== 0) throw new Error("취소가 요청을 보냈다");
            await page.getByTestId("reports-tab-work").click();
            if (await page.getByTestId("reports-mark-read").isEnabled()) throw new Error("빈 작업 탭의 읽음 버튼이 켜졌다");
            await page.getByTestId("reports-tab-mine").click();
            await page.getByTestId("reports-mark-read").click();
            fail = true;
            await dialog.getByRole("button", { name: "모두 읽음", exact: true }).click();
            await dialog.getByRole("alert").filter({ hasText: "실험실 읽음 저장 실패" }).waitFor();
            if (db.prepare("SELECT COUNT(*) AS n FROM library_reports WHERE seen_at IS NOT NULL").get().n !== 0) throw new Error("실패했는데 읽음으로 바뀌었다");
            await dialog.getByRole("button", { name: "취소", exact: true }).click();
            fail = false;
            requests = 0;
            await page.getByTestId("reports-mark-read").click();
            await dialog.getByRole("button", { name: "모두 읽음", exact: true }).click();
            await dialog.waitFor({ state: "hidden" });
            await domIs(page, "보관함 안 봄 1개만 남음", () => document.querySelector('[data-testid="activity-reports"]')?.getAttribute("aria-label") === "보고서, 안 본 보고서 1" && document.querySelector('[data-testid="reports-unseen"]')?.textContent === "1");
            if (requests !== 1) throw new Error(`일괄 처리 요청 ${requests}번`);
            if (await page.getByTestId("reports-mark-read").isEnabled()) throw new Error("처리 뒤 버튼이 켜졌다");
            // 내 보고서 원본 전체만 바뀌며, 별도 보관 판은 아직 안 봄이다.
            if (db.prepare("SELECT COUNT(*) AS n FROM library_reports WHERE name LIKE 'sample-%' AND seen_at IS NOT NULL").get().n !== 105) throw new Error("화면 밖을 놓쳤다");
            await page.getByTestId("reports-tab-archive").click();
            await page.getByTestId("reports-mark-read").click();
            await dialog.getByText("보관함의 안 본 보고서 1개를 읽음으로 바꾼다.", { exact: true }).waitFor();
            await dialog.getByRole("button", { name: "모두 읽음", exact: true }).click();
            await dialog.waitFor({ state: "hidden" });
            await domIs(page, "배지 모두 없어짐", () => document.querySelector('[data-testid="activity-reports"]')?.getAttribute("aria-label") === "보고서" && !document.querySelector('[data-testid="reports-unseen"]'));
            await domIs(page, "가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
            if (paired.errors.length) throw new Error(paired.errors.join("; "));
          } finally { await page.context().close(); }
        }
      }
      const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
      try {
        await inList(reader.page, "activity-reports");
        await domIs(reader.page, "Reader 목록 완료", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
        if (await reader.page.getByTestId("reports-mark-read").count()) throw new Error("Reader에 모두 읽음 버튼");
      } finally { await reader.page.context().close(); }
    } finally { await safari.close(); db.close(); }
  });
}
