# aaa 앱 아이콘 출처

모양은 Codex 내장 이미지 생성 시안(F3 「a 블록 커서」)을 참고해 aaa 프로젝트가
SVG로 다시 그렸습니다. 생성 PNG를 제품 자산으로 사용하지 않았습니다.

- 원본: `icon.svg`. 외부 이미지·글꼴 없이 사각형과 채움 경로로 구성했습니다.
- 색: 황금색 `#F5CA66`, 어두운 바탕 `#172735`.
- PNG 여섯 개는 SVG를 로컬에서 렌더했습니다. 일반 아이콘과 favicon은 모서리를
  둥글게 처리하고, maskable·Apple touch 아이콘은 바탕을 끝까지 채웠습니다.
- 생성 시안과 prompt는 검토 자료로만 별도 보존하며 저장소에 포함하지 않습니다.

- Mac launcher 아이콘 `p0/launcher/AppIcon.icns`도 같은 `icon.svg`에서 만들었습니다
  (`p0/launcher/make-icon.mjs`: macOS 앱 아이콘 격자의 둥근 사각형에 크기마다 다시 렌더, `iconutil`로 묶음).
- 웹 화면 머리·연결·빈 화면의 표시도 `icon.svg`를 그대로 씁니다.

SVG·PNG·ICNS에는 저장소의 MIT 라이선스(`LICENSE`)가 적용됩니다.
