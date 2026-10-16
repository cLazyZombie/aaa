// 밝음·어두움 테마. 기본은 시스템 설정을 따르고, 손으로 고르면 이 기기(localStorage)에만 남긴다.
// 실제 색은 style.css의 token이고, 여기서는 <html data-theme>만 정한다.
import { useEffect, useState } from "react";

export type ThemePref = "system" | "light" | "dark";
export type Scheme = "light" | "dark";

const KEY = "aa:theme";
const media = () => window.matchMedia("(prefers-color-scheme: light)");

function readPref(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    // 저장소를 못 쓰는 창(사생활 보호 등)은 시스템을 따른다
  }
  return "system";
}

const resolve = (p: ThemePref): Scheme => (p === "system" ? (media().matches ? "light" : "dark") : p);

const subs = new Set<() => void>();
let pref: ThemePref = readPref();

function apply() {
  const root = document.documentElement;
  if (pref === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", pref);
  const meta = document.querySelector('meta[name="theme-color"]');
  // 휴대폰 상태 표시줄 색을 바탕과 맞춘다(token 값을 그대로 읽는다)
  if (meta) meta.setAttribute("content", getComputedStyle(root).getPropertyValue("--bg").trim() || "#0f1115");
  subs.forEach((f) => f());
}

export function initTheme() {
  apply();
  media().addEventListener("change", () => pref === "system" && apply());
}

export function setThemePref(p: ThemePref) {
  pref = p;
  try {
    if (p === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, p);
  } catch {
    // 남기지 못해도 이번 창에서는 바뀐다
  }
  apply();
}

/** 지금 보이는 색 구성. 테마가 바뀌면 다시 그린다. */
export function useScheme(): [Scheme, ThemePref] {
  const [, tick] = useState(0);
  useEffect(() => {
    const f = () => tick((n) => n + 1);
    subs.add(f);
    return () => {
      subs.delete(f);
    };
  }, []);
  return [resolve(pref), pref];
}

/** 테마가 바뀔 때 부른다(React 밖: xterm 색 갱신) */
export function onThemeChange(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/** CSS token 값 */
export const token = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
