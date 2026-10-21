## Questions
### 1. 단추 아이콘
기존 막대 아이콘: 작업(폴더 칸반)·머신(모니터)·자동화(흐름)·답할 것(물음표 말풍선)·보고서·할 일. thread 화면 머리의 「coordinator」 단추는 지금 A(Bot)를 쓴다.

![여섯 안 한눈에](library/icons-all.png)

| 안 | 아이콘 | 뜻 | 걸리는 점 |
| --- | --- | --- | --- |
| A | Bot | coordinator agent | 없음, 머리 단추와 같은 그림 |
| B | BotMessageSquare | agent와 나누는 대화 | 「답할 것」 말풍선과 비슷 |
| C | MessagesSquare | 대화창 | 「답할 것」 말풍선과 헷갈림 |
| D | Network | 하나가 여럿을 맡김 | 「자동화」 흐름 그림과 비슷 |
| E | TowerControl | 관제탑 | 작게 보면 무엇인지 덜 읽힘 |
| F | Headset | 지휘·상담 | 고객 지원으로 읽힐 수 있음 |

- A Bot (추천) — thread 머리 「coordinator」 단추와 같은 그림이라 뜻이 바로 이어진다
  ![A안](library/icon-a.png)
- B BotMessageSquare — agent와의 대화라는 뜻이 가장 직접적이다
  ![B안](library/icon-b.png)
- C MessagesSquare — 대화창 그 자체지만 「답할 것」과 헷갈린다
  ![C안](library/icon-c.png)
- D Network — 하나가 여럿에게 일을 나누는 그림
  ![D안](library/icon-d.png)

E·F안 그림: `library/icon-e.png`(TowerControl), `library/icon-f.png`(Headset). 고르려면 「다른 답」에 E나 F를 쓴다.

![E안](library/icon-e.png)
![F안](library/icon-f.png)

### 2. 안 읽은 coordinator 답·질문 배지
coordinator는 pane 하나라 수 대신 점을 단다. 다른 단추 배지와 같은 자리(아이콘 오른쪽 아래)다. 그림은 A안 아이콘으로 찍었다.

| 상태 | 언제 | 점 |
| --- | --- | --- |
| 새 답 | coordinator가 턴을 끝냈고 아직 그 pane을 안 봤다 | 주황(강조색) |
| 질문 | coordinator가 확인창·질문에서 기다린다 | 빨강(「답할 것」 배지 색) |
| 없음 | 일하는 중·이미 봄 | 없음 |

![새 답](library/badge-reply.png)
![질문](library/badge-ask.png)

- A 점 배지 두 가지 (추천) — 막대만 봐도 coordinator가 나를 기다리는지 안다
  ![점 배지](library/badge-ask.png)
- B 질문일 때만 빨강 점 — 새 답 점은 자주 떠서 시끄러울 수 있다
  ![질문만](library/badge-ask.png)
- C 배지 없음 — 단추는 길 찾기만 한다
  ![배지 없음](library/badge-none.png)
