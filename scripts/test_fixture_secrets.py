"""공통 비밀 fixture의 언어 호환과 업무 단어 재유입 방지 계약."""
import json
import unittest
from pathlib import Path

from scripts.fixture_secret import token, fresh_token, assert_opaque
from scripts.check_fixture_secrets import check, violations

ROOT = Path(__file__).resolve().parent.parent


class FixtureSecrets(unittest.TestCase):
    def test_vectors_reject_business_words_and_keep_labels_outside_tokens(self):
        for vector in json.loads((ROOT / 'scripts/fixtures/secret-vectors.json').read_text(encoding="utf-8")):
            value = token(vector['seed'])
            self.assertEqual(value, vector['token'])
            self.assertNotIn(vector['seed'], value)
        self.assertEqual((ROOT / 'scripts/fixtures/secret-canary.txt').read_text(encoding="utf-8"), token('installation'))
        for value in ['automation', 'owner', 'access_token', 'refresh_token', 'fixture-automation-token']:
            with self.assertRaises(ValueError) as caught:
                assert_opaque(value)
            self.assertNotIn(value, str(caught.exception))
        for value in [token("owner") + "\n", token("owner")[1:], "g99" * 32]:
            with self.assertRaises(ValueError):
                assert_opaque(value)
        self.assertNotEqual(fresh_token(), fresh_token())

    def test_literal_authentication_is_rejected_in_all_fixture_languages(self):
        for source in [
            'let secret = "automation";',
            'remember("fixture-automation-token");',
            'h.accounts.import("work", "claude", "automation");',
            'secret: SecretText("automation".into()),',
            'r#"{"tokens":{"access_token":"automation"}}"#',
            'dict(refresh_token="automation")',
            'const token = `fixture-${name}-automation`;',
            "$auth=@{OPENAI_API_KEY='automation'}",
        ]:
            self.assertTrue(violations(source), source)
        self.assertFalse(violations('let secret = aaa_test_support::token("automation");'))
        self.assertFalse(violations('access_token: fixtureToken("automation")'))
        self.assertFalse(violations('dict(access_token=token("automation"))'))
        self.assertEqual(check(), [])
