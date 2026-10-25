import { logIgnoredFailure } from "./errorDiagnostic.ts";

/** 이전 이름 기반 터미널 초안은 고유 pane ID로 찾아 ID 기반 키로 한 번 옮긴다. */
export function migrateMachineDrafts(machineId: string, paneId: string, storage: Storage): void {
  for (const prefix of ["herdr-web-ui:terminal-draft:", "herdr-web-ui:terminal-line:"]) {
    try {
      const target = `${prefix}${machineId}:${paneId}`;
      if (storage.getItem(target) !== null) continue;
      const matches: string[] = [];
      for (let i = 0; i < storage.length; i++) {
        const key = storage.key(i);
        if (key?.startsWith(prefix) && key.endsWith(`:${paneId}`)) matches.push(key);
      }
      // 같은 pane에 서로 다른 옛 키가 있으면 임의로 고르거나 지우지 않는다.
      if (matches.length !== 1) continue;
      const value = storage.getItem(matches[0]!);
      if (value === null) continue;
      storage.setItem(target, value);
      storage.removeItem(matches[0]!);
    } catch (e) { logIgnoredFailure(e, "machineIdentity.ts:초안 신원 이전"); }
  }
}
