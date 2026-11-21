# Gemini 터미널 설정 검증

```sh
cargo test --profile verify --locked -p aaa --test smoke agy_config_and_profiles
cargo test --profile verify --locked -p aaa-config --lib
cargo test --profile verify --locked -p aaa-agent -p aaa-host --lib agy
cargo test --profile verify --locked -p aaa --test deps
```

실제 CLI를 독립 PTY와 임시 설정 폴더에서 실행한다. 가짜 `agy models`가 모델 ID와 표시 이름을 탭으로 나눈 목록을 돌려준다.
머신 등록은 평문과 해시가 터미널 출력에 찍히지 않고 파일에는 정규화한 주소의 SHA-256만 들어가는지 확인한다.
기존 설정을 보여 준 뒤 거절하면 파일이 바이트 단위로 같고, 승인하면 구독 별칭이 바뀐다.
비대화형 머신 등록과 aaa 세션 안의 프로필 편집은 실패한다.
프로필 추가와 편집을 각각 실행하고 잘못된 모델·effort 조합에서 host.toml이 그대로인지 확인한다.
