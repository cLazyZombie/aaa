"""서비스를 실행하지 않고 host 시험 도구의 번호·경로 검사를 확인한다."""
import ast
import re
import unittest
from pathlib import Path


class ShortIdTests(unittest.TestCase):
    def test_rollout_target_keeps_the_full_thread_number(self):
        # 이 host 도구는 pty 등 Unix 전용 모듈도 읽는다. 순수 검증 함수만 실행한다.
        tree = ast.parse(Path('scripts/test_windows_rollout.py').read_text(encoding='utf-8'))
        function = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'validate_test_target')
        namespace = {'re': re}
        exec(compile(ast.Module(body=[function], type_ignores=[]), '<target validator>', 'exec'), namespace)
        validate = namespace['validate_test_target']
        for n in ['0965', '9999', '10000', '100000']:
            validate(f'aaa-test-{n}-a', f'C:\\aaa-test\\t-{n}')
        for tag, root in [('aaa-test-10000-a', 'C:\\aaa-test\\t-0000'),
                          ('aaa-test-10000-a', 'C:\\aaa-test\\t-10000\\child'),
                          ('aaa-test-965-a', 'C:\\aaa-test\\t-965'),
                          ('aaa-test-１００００-a', 'C:\\aaa-test\\t-１００００')]:
            with self.subTest(tag=tag, root=root), self.assertRaises(AssertionError):
                validate(tag, root)

    def test_unix_instance_validator_has_no_four_digit_ceiling(self):
        tree = ast.parse(Path('scripts/setup-unix-e2e.py').read_text(encoding='utf-8'))
        check = next(node for node in ast.walk(tree) if isinstance(node, ast.Assert)
                     and 'args.instance' in ast.unparse(node.test))
        for name in ['aaa-test-9999-a', 'aaa-test-10000-a', 'aaa-test-100000-a']:
            args = type('Args', (), {'instance': name})()
            exec(compile(ast.Module(body=[check], type_ignores=[]), '<instance validator>', 'exec'), {'re': re, 'args': args})
        for name in ['aaa-test-965-a', 'aaa-test-１００００-a', 'other-10000-a', 'aaa-test-10000-a/child']:
            args = type('Args', (), {'instance': name})()
            with self.subTest(name=name), self.assertRaises(AssertionError):
                exec(compile(ast.Module(body=[check], type_ignores=[]), '<instance validator>', 'exec'), {'re': re, 'args': args})
