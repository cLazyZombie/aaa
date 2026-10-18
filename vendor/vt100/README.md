# vt100 0.16.2(aaa가 글자 폭을 고친 판)

[vt100](https://github.com/doy/vt100-rust) 0.16.2(MIT, `LICENSE` 그대로)를 들여왔다. session host 화면(aaa-vt)이 웹
터미널(xterm 5.5 + `@xterm/addon-unicode-graphemes` 0.4.0 + herdr-web-ui 보정)과 같은 칸에 글자를 놓게 하려는 것이다
(plan §8.150). upstream은 code point마다 `unicode-width`로 폭을 세어, 이모지 묶음(⚠️·ZWJ 가족·국기·피부색)과 한글
자모·태국 모음 같은 묶음에서 다시 붙은 웹(replay)의 칸이 살아 있는 웹과 어긋났다.

workspace의 `[patch.crates-io]`가 이 판을 쓴다. workspace 밖(`exclude`)이라 시험·clippy는 aaa-vt에서 돈다.

## 바꾼 것

- **폭과 묶음(`src/widths.rs`, `src/widths_table.rs`, `Screen::text`).** 글자마다 표(grapheme 부류·폭·묶기 방식)를 보고,
  앞 글자와 묶을지는 addon `shouldJoin`과 같은 규칙으로 정한다. 앞 칸 묶음에 붙는 글자는 그 칸에 더하고, 붙어서 한 칸
  묶음이 두 칸이 되면(⚠ 뒤 VS16) 그 자리에서 넓힌다. 끝 칸이라 자리가 없으면 xterm처럼 이은 줄로 다음 줄 처음에 옮긴다.
  - 표는 `node web/scripts/vt-widths.ts`가 웹 provider에서 만든다(손으로 고치지 않는다). 만들 때 모든 code point × 앞
    상태 스물에서 「표 + 규칙」이 provider와 같은지 대조하고 다르면 멈춘다.
  - `Cell::set`은 넓은지를 받는다(놓는 쪽이 정한다).
- **묶음 끊기(`src/perform.rs`, `Screen::set_size`).** 제어 문자·CSI·ESC·OSC·DCS 끝 뒤에는 앞 글자와 묶지 않는다(xterm
  parser의 `precedingJoinState = 0`). 받은 덩이 경계에서는 끊지 않는다. 크기를 바꿀 때도 끊는다(xterm은 끊지 않는다).
  aaa-vt가 폭을 바꿔 다시 접을 때는 화면을 새 parser에 다시 쓰므로 어차피 이어지지 않는다. 다른 것은 다시 접지 않는
  크기 바뀜이 한 묶음의 글자 사이에 끼는 경우뿐이다.
- **다시 쓸 때 끊기(`Row::write_contents_formatted`, `Grid::write_contents_formatted`).** escape로 따로 놓인 칸을 escape
  없이 이어 쓰면 받는 터미널이 한 묶음으로 합친다. 그런 자리에는 모양(SGR)을 다시 써서 끊는다. 이은 줄을 넘어서도 본다.
- **공개 함수 `joins_after`.** aaa-vt가 제 replay·reflow 쓰기에서 같은 끊기를 하려고 쓴다.
- `rustfmt.toml`: upstream 서식(폭 78). 상위 aaa 설정이 섞이지 않게 둔다.
- `Cargo.toml`: upstream 것에서 작성자 줄(email)을 빼고 `publish = false`로 했다(공개 저장소 규칙).

## 알려진 한계

- 칸 하나의 글은 22바이트까지다(upstream 그대로). 그보다 긴 묶음(피부색 넷인 가족 등)은 뒤 글자가 잘린다. 칸 폭은
  맞고, 잘린 글자는 그 칸에 보이지 않는다.
- 폭 표는 웹의 판(xterm·addon·herdr 보정)을 따라 다시 만들어야 한다. web 유닛 시험이 표가 생성기 출력과 같은지 본다.
