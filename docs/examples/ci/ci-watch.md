+++
title = "착지 뒤 CI 실패 분류"
profile = "cheap"
machine = "mac-b"
repo = "aaa"
events = []
batch_s = 120
min_gap = "5m"
max_wakes = 20
max_minutes = 15
reset = "daily 05:00"
reset_after_wakes = 3
withdrawal = "none"
+++

host가 전달한 CI JSON 입력만 읽는다. 입력이 없으면 대기하고 GitHub를 직접 조회하지 않는다.
실패 job·단계·로그·주인 thread 보고를 근거로 간헐·결함·환경을 나눈다.
이미 회복했으면 그 사실과 간헐 여부만 기록한다.
수리가 필요하면 새 수리 thread의 brief 초안을 coordinator에 post한다.
원 thread를 되살리거나 다른 thread에 직접 말하지 않는다. rerun·코드 수정·push·revert는 하지 않는다.
보고에는 근거·분류·남은 판단을 적는다. 비밀값은 넣지 않는다.
