"""소형·대형 머신에서 공유 CPU 예산의 누락과 중복 예약을 막는다."""

import os
import pathlib
import signal
import subprocess
import tempfile
import unittest

from scripts.ci_resources import budget


class Resources(unittest.TestCase):
    def test_combined_stages_share_available_cpus(self):
        for cpus in (2, 3, 4, 8, 10, 16, 32):
            rust, web = budget(cpus, True, True)
            self.assertGreaterEqual(rust, 1)
            self.assertGreaterEqual(web, 1)
            self.assertEqual(rust + web, cpus)
        self.assertEqual(budget(1, True, True), (1, 1))  # 셸이 직렬 실행한다.

    def test_a_single_stage_does_not_reserve_an_absent_stage(self):
        self.assertEqual(budget(10, True, False), (10, 0))
        self.assertEqual(budget(10, False, True), (0, 5))
        self.assertEqual(budget(1, False, True), (0, 1))
        self.assertEqual(budget(10, False, False), (0, 0))



class MachineLock(unittest.TestCase):
    """같은 머신의 두 검증은 실행 단계를 차례로 쓴다. 잠금은 셸이 끝나면 풀리고, fd를 닫고 띄운 자식은 쥐지 않는다."""

    def shell(self, lock, logs):
        # ci-local.sh와 같은 꼴: 잠금을 얻으면 got, fd 9를 닫은 자식을 남기고 표준 입력이 닫힐 때까지 쥔다
        script = (f'exec 9<>"$1"; python3 scripts/ci_machine_lock.py 9 {logs} || exit 2; echo got; '
                  'sleep 600 9>&- >/dev/null 2>&1 & echo "child $!"; read -r _ || :')
        shell = subprocess.Popen(["bash", "-c", script, "lock", str(lock)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, text=True)
        self.addCleanup(shell.stdout.close)
        return shell

    def held(self, shell):
        """got까지 읽은 줄과 남긴 자식 PID"""
        lines = []
        while not lines or lines[-1] != "got":
            lines.append(shell.stdout.readline().strip())
            self.assertTrue(lines[-1], lines)
        child = int(shell.stdout.readline().split()[1])
        self.children.append(child)
        return lines, child

    def setUp(self):
        self.children = []

    def tearDown(self):
        for child in self.children:
            os.kill(child, signal.SIGTERM)

    def test_second_run_waits_until_the_first_shell_ends_even_if_its_child_lives(self):
        with tempfile.TemporaryDirectory() as d:
            lock = pathlib.Path(d) / "aaa-ci.lock"
            first = self.shell(lock, "first-logs")
            _, child = self.held(first)
            second = self.shell(lock, "second-logs")
            waiting = second.stdout.readline()
            self.assertIn("끝나기를 기다린다", waiting)
            self.assertIn("first-logs", waiting)
            first.stdin.close()
            self.assertEqual(first.wait(timeout=30), 0)
            os.kill(child, 0)  # 첫 셸의 자식은 살아 있지만 잠금을 쥐지 않는다
            self.held(second)
            self.assertIn("second-logs", lock.read_text())
            second.stdin.close()
            self.assertEqual(second.wait(timeout=30), 0)
