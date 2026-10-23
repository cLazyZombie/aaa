"""cross 진입점의 기존 컴파일러 보존과 Zig 인자 전달을 확인한다."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent


class LinuxCross(unittest.TestCase):
    def test_zig_override_preserves_each_argument_and_exit_code(self):
        with tempfile.TemporaryDirectory(prefix="aaa-zig ") as directory:
            zig = Path(directory) / "zig fixture"
            zig.write_text('#!/bin/bash\nprintf "%s\\n" "$@"\nexit 23\n')
            zig.chmod(0o755)
            result = subprocess.run(
                [str(ROOT / "scripts/cross/x86_64-linux-gnu-gcc"),
                 "--target=x86_64-unknown-linux-gnu", "-c",
                 "source with spaces.c", "-DNAME=$literal"],
                env=dict(os.environ, AAA_ZIG=str(zig)), capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 23)
            self.assertEqual(result.stdout.splitlines(),
                             ["cc", "-target", "x86_64-linux-gnu",
                              "--target=x86_64-linux-gnu", "-c",
                              "source with spaces.c", "-DNAME=$literal"])

    def test_existing_compiler_and_linker_are_preserved(self):
        with tempfile.TemporaryDirectory(prefix="aaa-cross-") as directory:
            cargo = Path(directory) / "cargo"
            cargo.write_text(
                '#!/bin/bash\ncommand -v x86_64-linux-gnu-gcc\n'
                'printf "%s\\n" "$CC_x86_64_unknown_linux_gnu" '
                '"$CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER" "$@"\n'
            )
            cargo.chmod(0o755)
            compiler = Path(directory) / "x86_64-linux-gnu-gcc"
            compiler.write_text('#!/bin/bash\nexit 0\n')
            compiler.chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ["PATH"],
                       CC_x86_64_unknown_linux_gnu="/custom/old-zig-cc",
                       CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="/custom/linker")
            result = subprocess.run(["bash", "scripts/ci-local.sh", "--only", "linux-clippy",
                                     "--logs", str(Path(directory) / "logs"), "--tee"],
                                    cwd=ROOT, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            output = (Path(directory) / "logs/linux-clippy.log").read_text().splitlines()
            self.assertEqual(output[:3], [str(compiler), "/custom/old-zig-cc", "/custom/linker"])
            self.assertEqual(output[3:], ["clippy", "--workspace", "--all-targets", "--locked",
                                         "--keep-going", "--target", "x86_64-unknown-linux-gnu",
                                         "--", "-D", "warnings"])
