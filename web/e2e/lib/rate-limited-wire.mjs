import { performance } from "node:perf_hooks";

const realClock = { now: () => performance.now(), setTimeout, clearTimeout };

// 회선의 도착 예정 시각은 바이트 수로 정한다. 방향마다 타이머 하나로 TCP/WS 순서를 보존한다.
export class RateLimitedWire {
  constructor(send, bps, rtt, clock = realClock) {
    this.send = send;
    this.bps = bps;
    this.rtt = rtt;
    this.clock = clock;
    this.end = clock.now();
    this.queue = [];
    this.timer = null;
    this.closed = false;
  }

  push(data, bytes) {
    if (this.closed) return;
    this.end = Math.max(this.end, this.clock.now() + this.rtt / 2) + bytes * 8000 / this.bps;
    this.queue.push({ data, at: this.end });
    if (this.timer === null) this.arm();
  }

  arm() {
    this.timer = this.clock.setTimeout(() => {
      this.timer = null;
      // 소수 지연 반올림이나 부하로 타이머가 일찍/늦게 불려도 예정 시각·FIFO를 지킨다.
      while (this.queue.length && this.queue[0].at <= this.clock.now()) {
        this.send(this.queue.shift().data);
      }
      if (this.queue.length && this.timer === null) this.arm();
    }, Math.max(1, Math.ceil(this.queue[0].at - this.clock.now())));
  }

  close() {
    this.closed = true;
    if (this.timer !== null) this.clock.clearTimeout(this.timer);
    this.timer = null;
    this.queue = [];
  }
}
