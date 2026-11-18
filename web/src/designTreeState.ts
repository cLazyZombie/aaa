import { useLayoutEffect, useRef, useState } from "react";
import { ancestors, designTarget, splitTarget } from "./designModel";

const EXPANSION_KEY = "herdr-web-ui:design-tree-expansion";
type Surface = "sidebar" | "document";
const scrollKey = (surface: Surface) => `herdr-web-ui:design-tree-scroll:${surface}`;

function read<T>(key: string, parse: (value: string) => T, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : parse(raw);
  } catch { return fallback; }
}

function write(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* 이번 화면에서만 기억 */ }
}

type Expansion = { openDirs: Set<string>; closedRepos: Set<string>; selected: string | null };
const emptyExpansion = (): Expansion => ({ openDirs: new Set(), closedRepos: new Set(), selected: null });

function parseExpansion(raw: string): Expansion {
  const value = JSON.parse(raw) as Partial<Record<keyof Expansion, unknown>> | null;
  const paths = (list: unknown) => new Set(Array.isArray(list) ? list.filter((item): item is string => typeof item === "string") : []);
  return {
    openDirs: paths(value?.openDirs),
    closedRepos: paths(value?.closedRepos),
    selected: typeof value?.selected === "string" ? value.selected : null,
  };
}

/** 펼침은 두 트리가 공유한다. 각 칸의 스크롤은 화면 높이가 달라 따로 기억한다. */
export function useDesignTreeState(target: string | null) {
  const [expansion, setExpansion] = useState(() => read(EXPANSION_KEY, parseExpansion, emptyExpansion()));
  const savedExpansion = useRef(expansion);
  const positions = useRef<Record<Surface, number | null>>(null);
  if (positions.current === null) {
    const parse = (raw: string) => {
      const value: unknown = JSON.parse(raw);
      return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
    };
    positions.current = {
      sidebar: read(scrollKey("sidebar"), parse, null),
      document: read(scrollKey("document"), parse, null),
    };
  }
  useLayoutEffect(() => {
    if (!target || target === expansion.selected) return;
    const at = splitTarget(target);
    if (!at) return;
    const closedRepos = new Set(expansion.closedRepos);
    closedRepos.delete(at.repo);
    const openDirs = new Set([...expansion.openDirs, ...ancestors(at.path).map((path) => designTarget(at.repo, path))]);
    setExpansion({ openDirs, closedRepos, selected: target });
  }, [target, expansion]);
  useLayoutEffect(() => {
    if (savedExpansion.current === expansion) return;
    savedExpansion.current = expansion;
    write(EXPANSION_KEY, { ...expansion, openDirs: [...expansion.openDirs], closedRepos: [...expansion.closedRepos] });
  }, [expansion]);
  const toggle = (kind: "openDirs" | "closedRepos", key: string) => {
    setExpansion((old) => {
      const next = new Set(old[kind]);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { ...old, [kind]: next };
    });
  };
  return { expansion, toggle, positions };
}

export type DesignTreeState = ReturnType<typeof useDesignTreeState>;

export function useDesignTreeScroll(
  { positions, expansion }: DesignTreeState,
  surface: Surface,
  visible: boolean,
  selected: string | null,
  loading: boolean,
) {
  const panel = useRef<HTMLElement>(null);
  const previous = useRef(selected);
  const restored = useRef(false);
  const reveal = useRef<string | null>(null);
  // 매 commit에서 확인한다. 저장소 응답, 펼침, 필터가 트리 높이를 바꿀 수 있다.
  useLayoutEffect(() => {
    const changed = selected !== null && selected !== previous.current;
    if (selected) previous.current = selected;
    if (!visible) {
      restored.current = false;
      reveal.current = null;
      return;
    }
    if (changed) reveal.current = selected;
    if (restored.current && reveal.current === null) return;
    const node = panel.current;
    if (!node?.clientHeight) return;
    if (!restored.current && reveal.current === null) {
      const saved = positions.current?.[surface];
      if (saved != null) {
        // 저장소들이 따로 도착한다. 아직 짧은 트리가 기억한 위치를 덮지 않게 한다.
        const max = node.scrollHeight - node.clientHeight;
        if (loading && saved > max) return;
        node.scrollTop = Math.min(saved, max);
      } else {
        reveal.current = selected;
      }
      restored.current = true;
    }
    if (reveal.current === null) return;
    if (reveal.current !== selected) {
      reveal.current = null;
      return;
    }
    if (selected !== expansion.selected) return;
    const row = node.querySelector('.design-row.is-doc[aria-current="true"]');
    if (!row && loading) return;
    reveal.current = null;
    restored.current = true;
    if (row) {
      const view = node.getBoundingClientRect();
      const item = row.getBoundingClientRect();
      node.scrollTop += item.top < view.top ? item.top - view.top : Math.max(0, item.bottom - view.bottom);
    }
  });
  const onScroll = (event: React.UIEvent<HTMLElement>) => {
    const node = event.currentTarget;
    if (!visible || !node.clientHeight) return;
    restored.current = true;
    reveal.current = null;
    // Safari의 탄성 스크롤은 양 끝을 잠시 벗어날 수 있다.
    const top = Math.max(0, Math.min(node.scrollTop, node.scrollHeight - node.clientHeight));
    if (!positions.current || positions.current[surface] === top) return;
    positions.current[surface] = top;
    write(scrollKey(surface), top);
  };
  return { ref: panel, onScroll };
}
