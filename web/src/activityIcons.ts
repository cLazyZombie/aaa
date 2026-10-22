// 직접 그린 lucide 꼴 아이콘(24 격자, 둥근 끝·모서리). lucide 컴포넌트와 같은 자리에 쓴다(사용자 결정 10-22 t-0727: 시안 4개씩 중 고름)
import { createLucideIcon } from "lucide-react";

/** 자동화: 시계 방향으로 도는 화살표 안 시계 바늘(정한 때·사건마다 다시 돈다). 지금까지는 Workflow */
export const AutomationIcon = createLucideIcon("aaa-automation", [
  ["path", { d: "M21 12a9 9 0 1 1-2.64-6.36", key: "arc" }],
  ["path", { d: "M21 3v5h-5", key: "head" }],
  ["path", { d: "M12 7.5V12l3 2", key: "hands" }],
]);

/** 할 일: 체크가 밖으로 나가는 큰 상자(줄 목록 꼴인 「모든 thread」 ListChecks와 헷갈리지 않게). 지금까지는 ListTodo */
export const TasksIcon = createLucideIcon("aaa-tasks", [
  ["path", { d: "M21 11v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11", key: "box" }],
  ["path", { d: "m8 11 4 4 9-9.5", key: "check" }],
]);
