// 참고 herdr-web-ui ChatTimeline·t3code MessagesTimeline: 보던 row ID를 유지하며 화면 근처 본문만 mount한다.
// scroll container와 row shell은 유지한다. 높이를 잰 placeholder가 같은 자리를 차지해 DOM/Markdown 비용을 제한한다.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
export function ChatRow({ active = true, id, role, virtual, children, onResize }: {
  active?: boolean; id: string; role: string; virtual: boolean; children: ReactNode; onResize: () => void;
}) {
  const node = useRef<HTMLDivElement>(null);
  const height = useRef(100);
  const [visible, setVisible] = useState(!virtual);
  useLayoutEffect(() => {
    if (!active) return;
    const el = node.current;
    if (!el || !virtual) { setVisible(true); return; }
    const root = el.closest(".chat");
    const box = root?.getBoundingClientRect();
    const row = el.getBoundingClientRect();
    setVisible(!box || row.bottom >= box.top - 1000 && row.top <= box.bottom + 1000);
    // 한 줄도 통지가 늦으면 화면 밖→안 관측이 함께 온다. 마지막 관측을 써야 본문을 빈 채로 두지 않는다.
    const observer = new IntersectionObserver((entries) => setVisible(entries.at(-1)?.isIntersecting ?? false), { root, rootMargin: "1000px 0px" });
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, virtual]);
  useLayoutEffect(() => {
    if (!active) return;
    const el = node.current;
    if (!el || !visible) return;
    const measure = () => {
      height.current = el.getBoundingClientRect().height;
      onResize();
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, visible, onResize]);
  return <div ref={node} className={`chat-row chat-row-${role}`} data-chat-id={id}
    style={visible ? undefined : { height: height.current, flexShrink: 0 }}>
    {visible ? children : null}
  </div>;
}
