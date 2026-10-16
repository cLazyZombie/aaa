// 작은 선 아이콘(직접 그렸다). 색은 currentColor를 따른다.
import type { ReactNode } from "react";

const paths: Record<string, ReactNode> = {
  // 셸 프롬프트
  shell: (
    <>
      <path d="M3.5 5.5 7 9l-3.5 3.5" />
      <path d="M9 13h6" />
    </>
  ),
  // Claude: 여덟 갈래 빛
  claude: (
    <>
      <path d="M9 2.5v13M2.5 9h13M4.4 4.4l9.2 9.2M13.6 4.4l-9.2 9.2" />
    </>
  ),
  // Codex: 육각형
  codex: <path d="M9 2.2 15 5.6v6.8L9 15.8 3 12.4V5.6z" />,
  // 일반 agent
  agent: (
    <>
      <rect x="3" y="5" width="12" height="9" rx="3" />
      <path d="M9 2.5V5M6.5 9.5h.01M11.5 9.5h.01" />
    </>
  ),
  // thread: 문서
  thread: (
    <>
      <path d="M4.5 2.5h6l3 3v10h-9z" />
      <path d="M7 8.5h4M7 11.5h4" />
    </>
  ),
  machine: (
    <>
      <rect x="2.5" y="3" width="13" height="9" rx="1.5" />
      <path d="M6.5 15h5M9 12v3" />
    </>
  ),
  plus: <path d="M9 3.5v11M3.5 9h11" />,
  send: <path d="M9 14.5V3.5M4.5 8 9 3.5 13.5 8" />,
  image: (
    <>
      <rect x="2.5" y="3.5" width="13" height="11" rx="2" />
      <circle cx="6.5" cy="7.5" r="1.2" />
      <path d="m15.5 11.5-3.5-3.5-6.5 6.5" />
    </>
  ),
  back: <path d="M11 3.5 5.5 9l5.5 5.5" />,
  fit: (
    <>
      <path d="M3 6.5V3h3.5M11.5 3H15v3.5M15 11.5V15h-3.5M6.5 15H3v-3.5" />
    </>
  ),
  sun: (
    <>
      <circle cx="9" cy="9" r="3" />
      <path d="M9 1.8v1.6M9 14.6v1.6M1.8 9h1.6M14.6 9h1.6M3.9 3.9 5 5M13 13l1.1 1.1M3.9 14.1 5 13M13 5l1.1-1.1" />
    </>
  ),
  moon: <path d="M14.8 11.2A6.3 6.3 0 0 1 6.8 3.2a6.3 6.3 0 1 0 8 8z" />,
  auto: (
    <>
      <circle cx="9" cy="9" r="6.3" />
      <path d="M9 2.7v12.6A6.3 6.3 0 0 0 9 2.7z" fill="currentColor" />
    </>
  ),
  file: (
    <>
      <path d="M5 2.5h5.5l3 3v10H5z" />
      <path d="M10.5 2.5v3h3" />
    </>
  ),
  check: <path d="m4 9.5 3.2 3L14 5.5" />,
  download: (
    <>
      <path d="M9 3v8.5M5.5 8 9 11.5 12.5 8" />
      <path d="M3.5 13.5v1.5h11v-1.5" />
    </>
  ),
  close: <path d="M4.5 4.5l9 9M13.5 4.5l-9 9" />,
};

export function Icon({ name, title }: { name: keyof typeof paths | string; title?: string }) {
  return (
    <svg className="i" viewBox="0 0 18 18" aria-hidden={title ? undefined : true} role={title ? "img" : undefined}>
      {title && <title>{title}</title>}
      {paths[name] ?? paths.agent}
    </svg>
  );
}
