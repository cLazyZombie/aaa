// 등록 해제가 보류된 요청을 자동 continue하기 전에 각 handler가 자기 요청을 마친다.
export class ThreadRoutes {
  pending = new Set();
  closing = false;

  async run(route, handle) {
    let done;
    const completion = new Promise(resolve => { done = resolve; });
    this.pending.add(completion);
    try {
      if (this.closing) await route.continue();
      else await handle();
    } finally {
      this.pending.delete(completion);
      done();
    }
  }

  async close(context) {
    this.closing = true;
    while (this.pending.size) await Promise.all([...this.pending]);
    await context.unrouteAll({ behavior: "wait" });
  }
}
