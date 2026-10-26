import InlineWorker from "./terminalInflate.worker?worker&inline";

// 작은 압축 Worker를 앱과 함께 넣어 Safari의 별도 Worker 자산 로드·접근 제어 실패를 없앤다.
export const createTerminalWorker = (): Worker => new InlineWorker({ name: "aaa-terminal-inflate" });
