import fs from "node:fs";

// 폴더 사건은 마지막 unlink보다 먼저 오거나 합쳐질 수 있다. Reaper가 실제
// remove_dir_all 뒤 쓰는 host 로그도 깨움으로 쓰고, 성공은 항상 디스크로 확인한다.
export async function pendingDeletionCleared(pending, completionLog, inTime, watch = fs.watch) {
  const watchers = [];
  try {
    await inTime(new Promise((resolve, reject) => {
      const check = () => {
        try { if (fs.readdirSync(pending).length === 0) resolve(); }
        catch (error) { reject(error); }
      };
      for (const file of [pending, completionLog]) {
        const watcher = watch(file, check);
        watchers.push(watcher);
        watcher.on("error", reject);
      }
      check();
    }), 20_000, "삭제 대기 사본의 실제 제거");
  } finally { for (const watcher of watchers) watcher.close(); }
}
