# Meslotendard 0.3.1 (웹 터미널 글꼴)

aaa 웹 터미널의 첫 글꼴이다(plan §8.31). 영문·아이콘은 MesloLGM Nerd Font Mono, 한글은 Pretendard이고, 한글 한 글자가 영문 정확히 두 칸이다.

- 출처: https://github.com/cLazyZombie/Meslotendard release `v0.3.1`의 `Meslotendard.zip`
- 라이선스: SIL Open Font License 1.1(`LICENSE`). 구성 원본의 고지와 라이선스는 `licenses/`(Meslo: Apache 2.0, Nerd Fonts, Pretendard: OFL 1.1·Reserved Font Name "Pretendard")에 있다. `FONTLOG.md`는 원본의 변경 기록이다.
- 받은 TTF의 sha256(release `SHA256SUMS.txt`와 같다)
  - `Meslotendard-Regular.ttf` `47e36779bbfc81f4aafd9a14c1184a784cbce7f58b80696a8cce89c0fc71b1bc`
  - `Meslotendard-Bold.ttf` `28626c8d17b2d973dfeacdb5690fc935cfc4862115efb7b3dbdf5c09d06171b6`
- 넣은 것: Regular·Bold를 WOFF2로만 바꿨다(fontTools 4.55.3, glyf 변환 없이 표를 brotli로 묶음). 글자 모양·메트릭·이름 표는 원본과 bytes가 같다. 다른 점은 WOFF2 규격이 요구하는 `head`의 「손실 없이 변환함」 표시(flags bit 11)와 그에 따른 checksum뿐이다. subset·이름 변경은 하지 않았다(Pretendard는 Reserved Font Name이라 변경판을 만들지 않는다).
  - `Meslotendard-Regular.woff2` `424c09291f3a046e7905fd0bc8c1303fa44d220af49cbfa88134e5ef41003a58`
  - `Meslotendard-Bold.woff2` `1662385a4e220b2bf6992a57d565be3ec36ec09eecc5142d74c9e2d75cc9d82b`
- 넣지 않은 것: Italic·BoldItalic. Claude Code 화면에 기울임 글자가 없었다. 기기에 Meslotendard가 깔려 있으면 그 기울임꼴을 쓰고, 없으면 브라우저가 Regular를 기울여 그린다.
- 버전을 바꾸면 폴더 이름(`meslotendard-<버전>`)도 바꾼다. host는 `fonts/` 아래를 1년 동안 바뀌지 않는 파일로 보낸다.
