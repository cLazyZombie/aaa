/** Readline unix-word-rubout: 커서 앞 공백과 그 앞의 공백 아닌 덩어리를 지운다. 선택은 선택만 지운다. */
export function wordDeleteRange(value: string, start: number, end: number): [number, number] {
  if (start !== end) return [start, end];
  const before = value.slice(0, start);
  const suffix = /\S+\s*$|\s+$/u.exec(before);
  return [suffix?.index ?? start, end];
}
