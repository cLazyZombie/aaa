// Playwright 1.64.1의 기본 비활성 feature 목록을 보존한다. --disable-features는 누적되지 않는다.
// 원문: microsoft/playwright v1.64.1 packages/playwright-core/src/server/chromium/chromiumSwitches.ts
// E2E01은 실제 기본 실행 인자와 비교하므로 도구 갱신 때 누락되면 실패한다.
const disabledFeatures = [
  "AvoidUnnecessaryBeforeUnloadCheckSync",
  "DestroyProfileOnBrowserClose",
  "DialMediaRouteProvider",
  "GlobalMediaControls",
  "HttpsUpgrades",
  "LensOverlay",
  "MediaRouter",
  "PaintHolding",
  "ThirdPartyStoragePartitioning",
  "BlockOriginHeaderModificationOnRedirect",
  "AvoidCorsURLLoaderRestartOnRedirect",
  "Translate",
  "AutoDeElevate",
  "OptimizationHints",
  "NetworkTimeServiceQuerying",
  "AimEnabled",
  "msForceBrowserSignIn",
  "msEdgeUpdateLaunchServicesPreferredVersion",
];

export function chromiumLaunchOptions() {
  // 별도 GPU 자식이 책임 계보를 끊으면 이 Mac 실행 환경에서 dyld open에 막혀 첫 frame sink가 오지 않는다.
  // GPU를 별도 프로세스로 유지하며 브라우저의 실행 책임만 잇는다. OS 판·화면 전원·첫 포인터에 의존하지 않는다.
  return process.platform === "darwin" ? {
    args: [`--disable-features=${[...disabledFeatures, "MacDisclaimGpuTccResponsibility"].join(",")}`],
  } : {};
}

export function launchChromium(chromium) {
  return chromium.launch(chromiumLaunchOptions());
}
