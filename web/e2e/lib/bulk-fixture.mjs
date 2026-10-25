import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";

// 외부 fixture writer와 실행 중인 host가 같은 DB에 쓰지 않는다.
// lab 인자는 실제 SQLite 잠금과 비동기 종료를 쓰는 단위 시험에서도 같은 수명을 검사한다.
export async function seedHost(write, lab) {
  const { root, stop, start, inTime } = lab ?? await import("./harness.mjs");
  await stop("host");
  const file = `${root}/s/host/host.db`;
  const db = new DatabaseSync(file);
  try {
    db.exec("BEGIN IMMEDIATE");
    write(db);
    db.exec("COMMIT");
  } catch (error) {
    throw new Error(`fixture DB 준비 실패(${file}): ${error.message}`, { cause: error });
  } finally { db.close(); }
  const host = start("host", "host");
  let watch, exited;
  try {
    await inTime(new Promise((resolve, reject) => {
      const check = () => {
        const log = fs.readFileSync(`${root}/host.log`, "utf8");
        if (log.includes("웹 listen") && log.includes("agent 연결")) resolve();
      };
      watch = fs.watch(`${root}/host.log`, check);
      exited = (code, signal) => reject(new Error(`fixture host 종료: ${code}/${signal}`));
      host.once("exit", exited);
      check();
    }), 20_000, "fixture 뒤 host·agent 재연결");
  } finally {
    watch?.close();
    if (exited) host.off("exit", exited);
  }
}
