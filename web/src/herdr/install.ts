import { useCallback, useSyncExternalStore } from "react";

interface InstallChoice {
  outcome: "accepted" | "dismissed";
  platform: string;
}

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<InstallChoice>;
}

type Listener = () => void;

let pendingPrompt: BeforeInstallPromptEvent | null = null;
let installedByEvent = false;
let revision = 0;
const listeners = new Set<Listener>();

function emitChange(): void {
  revision += 1;
  for (const listener of listeners) listener();
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function snapshot(): number {
  return revision;
}

function isStandalone(): boolean {
  if (typeof window === "undefined" || typeof navigator === "undefined") return false;
  const iosNavigator = navigator as Navigator & { standalone?: boolean };
  return installedByEvent || iosNavigator.standalone === true || window.matchMedia?.("(display-mode: standalone)").matches === true;
}

if (typeof window !== "undefined") {
  window.addEventListener("beforeinstallprompt", (event) => {
    event.preventDefault();
    pendingPrompt = event as BeforeInstallPromptEvent;
    emitChange();
  });
  window.addEventListener("appinstalled", () => {
    installedByEvent = true;
    pendingPrompt = null;
    emitChange();
  });
  const displayMode = window.matchMedia?.("(display-mode: standalone)");
  displayMode?.addEventListener("change", emitChange);
}

export interface InstallEnvironment {
  userAgent: string;
  /** window.isSecureContext: browsers only offer installation over HTTPS or localhost */
  secure: boolean;
  /** iPadOS reports a Mac user agent; touch points tell the two apart */
  maxTouchPoints: number;
}

const NO_PROMPT = "브라우저가 설치 창을 주지 않았다.";

/** Why there is no install button when the browser offers no prompt of its own (iOS never does). */
export function installHelp(env: InstallEnvironment): string {
  const ua = env.userAgent;
  const ios = /iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && env.maxTouchPoints > 1);
  if (ios) return NO_PROMPT;
  if (!env.secure) return "HTTPS(또는 localhost)가 아니라 설치할 수 없다.";
  if (/Firefox\//.test(ua) && !/Android/.test(ua)) return "데스크톱 Firefox는 웹 앱을 설치할 수 없다.";
  return NO_PROMPT;
}

function currentInstallHelp(): string {
  if (typeof window === "undefined" || typeof navigator === "undefined") return "";
  return installHelp({ userAgent: navigator.userAgent, secure: window.isSecureContext, maxTouchPoints: navigator.maxTouchPoints ?? 0 });
}

export interface InstallPromptState {
  canInstall: boolean;
  installed: boolean;
  install(): Promise<void>;
  /** what to tell the user when there is no prompt to show */
  help: string;
}

export function useInstallPrompt(): InstallPromptState {
  useSyncExternalStore(subscribe, snapshot, snapshot);
  const installed = isStandalone();
  const install = useCallback(async (): Promise<void> => {
    const prompt = pendingPrompt;
    if (prompt === null || isStandalone()) return;
    await prompt.prompt();
    await prompt.userChoice;
    if (pendingPrompt === prompt) pendingPrompt = null;
    emitChange();
  }, []);
  return { canInstall: pendingPrompt !== null && !installed, installed, install, help: currentInstallHelp() };
}
