"""시험 전용 비밀 fixture 규약 v1. 운영 인증에는 쓰지 않는다."""
import hashlib
import re
import secrets


def assert_opaque(value):
    if not isinstance(value, str) or not re.fullmatch(r"(?:[g-v](?:0[0-9]|1[0-5])){32}", value):
        raise ValueError("인증 fixture는 fixture_secret.token/fresh_token으로 만든 opaque 값이어야 한다")
    return value


def token(seed):
    """UTF-8 seed의 SHA-256 각 byte를 g..v 문자·00..15 두 숫자으로 무손실 변환한다."""
    return assert_opaque("".join(chr(103 + b // 16) + f"{b % 16:02d}" for b in hashlib.sha256(seed.encode("utf-8")).digest()))


def fresh_token():
    return token(secrets.token_hex(32))
