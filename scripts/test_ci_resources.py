"""소형·대형 머신에서 공유 CPU 예산의 누락과 중복 예약을 막는다."""

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
