/** 구독 사용량 판의 요금제 칸은 좁아 월 금액(「$200/월」)을 빼고 이름·상태만 보인다. 설정 선택 목록은 금액으로 Pro 단계를 가르므로 그대로 둔다. */
export function usagePlanName(plan: string): string {
  return plan.split(" · ").filter(part => !/^\$[\d.,]+\/월$/.test(part)).join(" · ");
}
