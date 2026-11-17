// 이 spec의 단계는 앞 단계의 상태를 쓰지 않는다. 단계마다 새 실험실을 주고 같은 검사를 모두 실행한다.
const independent = new Set(["37-chat-pages", "93-shell-chat", "113-bulk-actions", "114-bulk-maintenance"]);
const exact = (name) => `^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`;

// spec 안 단계는 상태를 이어 쓰므로 같은 러너에 둔다. 모르는 시간은 가장 긴
// 기존 spec만큼 예약하고, 모든 shard가 같은 목록·원장으로 같은 배치를 만든다.
export function shardSpecs(specs, shard, durations) {
  if (!shard) return specs;
  const match = /^(\d+)\/(\d+)$/.exec(shard);
  const [index, total] = match ? match.slice(1).map(Number) : [];
  if (!Number.isSafeInteger(index) || !Number.isSafeInteger(total) || index < 1 || index > total || total > specs.length)
    throw new Error("--shard(AAA_E2E_SHARD)는 1/N부터 N/N까지이며 N은 spec 수 이하다");
  const fallback = Math.max(1, ...Object.values(durations));
  const weight = s => durations[s.name] ?? fallback;
  const bins = Array.from({ length: total }, () => ({ weight: 0, names: new Set() }));
  for (const spec of [...specs].sort((a, b) => weight(b) - weight(a) || a.name.localeCompare(b.name))) {
    const bin = bins.reduce((best, next) => next.weight < best.weight ? next : best);
    bin.names.add(spec.name);
    bin.weight += weight(spec);
  }
  return specs.filter(s => bins[index - 1].names.has(s.name));
}

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
