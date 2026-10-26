"""인증 fixture에 평문·업무 단어 토큰을 다시 넣는 것을 막는 정적 검사.

인증 필드, 가림 등록, 금고 등록의 직접 문자열과 secret/token/canary 선언을 본다.
형식 오류/프로토콜 자료를 시험하는 예외는 정확한 경로·값·사유로만 허용한다.
동적 값은 공통 생성기의 assert_opaque와 기존 전달·가림 owner가 검증한다.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Rust raw 문자열의 JSON과 Python/Node 내장 harness도 같은 규칙으로 읽는다.
LITERAL = r'''["'`]([^"'`\n]*)["'`]'''
PATTERNS = [
    re.compile(r'''(?<![\w])(?:["']?(?:token|access_token|refresh_token|accessToken|refreshToken|OPENAI_API_KEY|ANTHROPIC_API_KEY|CLAUDE_CODE_OAUTH_TOKEN)["']?\s*[:=]\s*)''' + LITERAL),
    re.compile(r'''(?:\blet\s+(?:mut\s+)?(?:secret|token|canary)|\bconst\s+(?:secret|token|canary)|\b(?:const\s+)?CANARY\s*:\s*&str)\s*=\s*b?''' + LITERAL),
    re.compile(r'''(?:remember_env|\.env|\.insert)\(\s*[\"'](?:OPENAI_API_KEY|ANTHROPIC_API_KEY|CLAUDE_CODE_OAUTH_TOKEN|AAA_HOOK_TOKEN)[\"']\s*,\s*''' + LITERAL),
    re.compile(r'''\bSecretText\(\s*''' + LITERAL),
    re.compile(r'''\bremember\(\s*&?''' + LITERAL),
    re.compile(r'''\b(?:import|register_account)\(\s*["'][^"']+["']\s*,\s*["'](?:claude|codex)["']\s*,\s*&?''' + LITERAL),
]


def violations(source):
    # Rust의 일반 문자열 안 JSON은 한 겹 escape를 벗겨 필드를 읽는다.
    source = source.replace('\\"', '"')
    found = set()
    for pattern in PATTERNS:
        for match in pattern.finditer(source):
            value = match.group(1)
            if not value or value.startswith(('{', '[')):
                continue
            line = source.count('\n', 0, match.start(1)) + 1
            found.add((line, value))
    return sorted(found)


def check(root=ROOT):
    exceptions = json.loads((root / 'scripts/fixtures/secret-exceptions.json').read_text(encoding="utf-8"))
    if any(set(e) != {'path', 'value', 'reason'} or not e['reason'].strip() for e in exceptions):
        return ['scripts/fixtures/secret-exceptions.json: 정확한 경로·값·사유가 필요하다']
    allowed = {(e['path'], e['value']): e['reason'] for e in exceptions}
    if len(allowed) != len(exceptions):
        return ['scripts/fixtures/secret-exceptions.json: 중복 예외 금지']
    used = set()
    errors = []
    for folder in ['crates', 'scripts', 'web/e2e', 'web/test']:
        for path in (root / folder).rglob('*'):
            if path.suffix not in {'.rs', '.py', '.mjs', '.ps1'} or path.name in {'check_fixture_secrets.py', 'test_fixture_secrets.py'}:
                continue
            relative = path.relative_to(root).as_posix()
            for line, value in violations(path.read_text(encoding="utf-8")):
                key = (relative, value)
                if key in allowed:
                    used.add(key)
                else:
                    # 의도한 fixture조차 원문을 CI 로그에 쓰지 않는다.
                    errors.append(f'{relative}:{line}: 인증 fixture 직접 문자열 금지; 공통 token 생성기를 쓴다')
    for key in allowed.keys() - used:
        errors.append(f'{key[0]}: 사용하지 않는 인증 fixture 예외')
    return errors


if __name__ == '__main__':
    errors = check()
    if errors:
        print('\n'.join(errors))
    raise SystemExit(bool(errors))
