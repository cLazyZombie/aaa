// PTY의 read 조각과 SIZE 알림 경계는 입력의 경계가 아니다. 받은 순서로 bytes를 잇는다.
export function receivedInput(events) {
  return events.filter(([, kind]) => kind === "GOT").map(([, , value]) => value).join("");
}
