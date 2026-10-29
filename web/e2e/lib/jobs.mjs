// 이 spec의 단계는 앞 단계의 상태를 쓰지 않는다. 단계마다 새 실험실을 주고 같은 검사를 모두 실행한다.
const independent = new Set(["37-chat-pages", "93-shell-chat", "113-bulk-actions", "114-bulk-maintenance"]);
const exact = (name) => `^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`;

export function planJobs(specs, { repeat, workers, durations, only }) {
  const jobs = [];
  for (let r = 1; r <= repeat; r++) {
    for (const s of specs) {
      const steps = workers > 1 && independent.has(s.name)
        ? s.steps.filter((name) => !only || only.test(name)) : [null];
      for (const [index, name] of steps.entries()) jobs.push({
        s, r, filter: name === null ? null : exact(name),
        part: name === null ? null : `${index + 1}/${steps.length}`,
        weight: (durations[s.name] ?? Infinity) / steps.length,
      });
    }
  }
  if (workers > 1) jobs.sort((a, b) => b.weight - a.weight || a.r - b.r);
  return jobs;
}

// 분할한 작업의 슬롯 점유 시간을 합친다. wall time은 입구의 전체 실행 시간이 별도로 기록한다.
export function specDurations(runs, name, repeat) {
  return Array.from({ length: repeat }, (_, index) => runs
    .filter((run) => run.name === name && run.r === index + 1)
    .reduce((sum, run) => sum + run.ms, 0)).sort((a, b) => a - b);
}
