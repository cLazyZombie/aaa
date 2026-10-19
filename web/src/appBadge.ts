// host가 센 공통 수. focus만으로 지우지 않는다(faviconBadge는 별도 알림 수).
export async function setAppBadge(count: number): Promise<void> {
  if (!Number.isSafeInteger(count) || count < 0) return;
  try {
    if (count === 0) await navigator.clearAppBadge?.();
    else await navigator.setAppBadge?.(count);
  } catch {
    // 미지원·권한 거부·종료 중에도 앱은 그대로 쓴다.
  }
}
