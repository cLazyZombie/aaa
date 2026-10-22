"""병렬 CI의 로컬 로그를 읽어 단계 이름과 함께 출력한다. 검증 프로세스와 pipe를 공유하지 않는다."""

import codecs
import sys
import time
from contextlib import ExitStack
from pathlib import Path


def follow(logs: Path, done: Path, stages: list[str]) -> None:
    with ExitStack() as stack:
        streams = [(stage, stack.enter_context((logs / f"{stage}.log").open("rb")))
                   for stage in stages]
        decoders = {stage: codecs.getincrementaldecoder("utf-8")(errors="replace")
                    for stage in stages}
        while True:
            # 완료 표지를 먼저 확인한 뒤 마지막 바이트까지 읽는다(EOF 확인과 완료 사이의 쓰기도 보존).
            finished = done.exists()
            had_output = False
            for stage, stream in streams:
                data = stream.read(8192)
                had_output = had_output or bool(data)
                output = decoders[stage].decode(data)
                for line in output.splitlines(keepends=True):
                    sys.stdout.write(f"[{stage}] {line}")
                    if not line.endswith(("\n", "\r")):
                        sys.stdout.write("\n")
                # 개행 없는 진행 출력도 즉시 보인다. 원본 로그는 바이트 그대로 남는다.
                if finished and not data:
                    tail = decoders[stage].decode(b"", final=True)
                    if tail:
                        sys.stdout.write(f"[{stage}] {tail}\n")
                sys.stdout.flush()
            if finished and not had_output:
                return
            if not had_output:
                time.sleep(0.05)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    follow(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:])
