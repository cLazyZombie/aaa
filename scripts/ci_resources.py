"""Rust와 브라우저가 같은 CPU를 중복 예약하지 않게 기본 실행 예산을 나눈다."""

import os
import sys


def budget(cpus, rust, web):
    cpus = max(1, cpus)
    if rust and web:
        browsers = max(1, min(6, cpus * 2 // 5))
        return max(1, cpus - browsers), browsers
    return (cpus if rust else 0), (max(1, min(6, cpus // 2)) if web else 0)


if __name__ == "__main__":
    cpus = (getattr(os, "process_cpu_count", os.cpu_count)() or 1)
    rust, web = budget(cpus, sys.argv[1] == "1", sys.argv[2] == "1")
    print(f"{cpus}|{rust}|{web}")
