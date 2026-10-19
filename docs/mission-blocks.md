# 임무의 막힘 이유와 복귀

웹 「특수」와 `aaa mission show <프로젝트> <이름>`의 blocked_reason이 막힌 이유를 구분한다. 단순한 화면 확인창과 입력을 전달했는지 모르는 상황은 복귀 방법이 다르다.

| 이유 | 화면 말 | 복귀 |
|---|---|---|
| question | 막힘 · 확인창 | 사람이 답한 뒤 Working/Idle이 관찰되면 자동으로 일함/대기 |
| message_unknown | 막힘 · 입력 전달 모름 | 입력 파일·화면을 확인하고 명시 restart |
| wake_unknown | 막힘 · 재시작 중 깨움 모름 | 이전 host의 깨움 전달을 확정할 수 없어 restart 대기 |
| brief_unknown | 막힘 · brief 전달 모름 | brief 전달을 확정할 수 없어 restart 대기 |
| restart_failed | 막힘 · 다시 시작 실패 | 오류를 확인하고 명시 restart |
| legacy_unknown 또는 이유 없는 옛 막힘 | 막힘 · 이유 모름 또는 막힘 | 원인을 추정하지 않고 restart 대기 |

확인창이 사라진 것만으로 새 입력을 만들어 깨우지는 않는다. 쌓인 입력이 있고 Idle이면 기존 batch·min_gap·깨움/시간 예산·사용량 문턱을 거쳐 한 번 깨운다. Working이면 지금 작업을 계속한다. disabled·pause 상태도 존중한다. 자동화 기록에는 「막힘 · 확인창」과 「확인창 해제」를 남긴다.

전달 모름은 뒤에 확인창이 뜨거나 Working/Idle로 바뀌어도 보존한다. pause/resume로 해제되지 않는다. await 중 늦게 온 전달 모름도 이전 사본이 덮지 않으며 깨움 내구 표지 저장 전에 다시 확인한다. 이미 전달했을 수 있는 글을 저절로 재전송하지 않는다. 명시한 `aaa mission restart`와 새 대화 확정만 이 원인을 지운다. restart는 실제 작업/화면을 보고 결정한다.

이유 없는 옛 DB 기록을 확인창으로 간주해 자동 복귀시키지 않는다. 운영 임무의 restart·승인·설정 변경은 coordinator가 맡는다.
