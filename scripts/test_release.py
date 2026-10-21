"""실제 release 도구 CLI가 만든 파일·거부 결과를 본다. 네트워크·Git 쓰기·신호는 없다."""

import hashlib
import json
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

from scripts.release import ASSETS, LAUNCHERS, ROOT


class ReleaseContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="aaa-release-contract-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        self.version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.payloads = {}
        for platform, name in ASSETS.items():
            body = f"독립된 빌드 표본: {platform}\n".encode()
            self.payloads[platform] = body
            (self.artifacts / name).write_bytes(body)

        for platform, name in LAUNCHERS.items():
            package = dict(schema=1, version=self.version, platform=platform,
                           binary_sha256=hashlib.sha256(self.payloads[platform]).hexdigest(),
                           files={name: "dGVzdA==" for name in ("Contents/MacOS/aaa-launcher", "Contents/Info.plist", "Contents/Resources/AppIcon.icns", "Contents/_CodeSignature/CodeResources")})
            (self.artifacts / name).write_text(json.dumps(package))

    def run_tool(self, tag, output):
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/release.py"), "manifest", "--tag", tag,
             "--artifacts", str(self.artifacts), "--output", str(output)],
            capture_output=True, text=True, check=False,
        )

    def test_manifest_and_checksums_describe_actual_artifacts(self):
        output = self.root / "output"
        result = self.run_tool(f"v{self.version}", output)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text())
        self.assertEqual(manifest["version"], self.version)
        self.assertIn("macos-aarch64", manifest["assets"])
        self.assertIn("windows-x86_64", manifest["assets"])
        sums = dict(line.split("  ")[::-1] for line in (output / "SHA256SUMS").read_text().splitlines())
        for platform, asset in manifest["assets"].items():
            name = asset["url"].rsplit("/", 1)[1]
            self.assertEqual(asset["url"], f"https://github.com/cLazyZombie/aaa/releases/download/v{self.version}/{name}")
            self.assertEqual((self.artifacts / name).read_bytes(), self.payloads[platform])
            expected = hashlib.sha256(self.payloads[platform]).hexdigest()
            self.assertEqual(asset["sha256"], expected)
            self.assertEqual(sums[name], expected)

    def test_launcher_is_bound_to_the_same_binary_and_is_in_checksum_list(self):
        output = self.root / "output"
        result = self.run_tool(f"v{self.version}", output)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((output / "manifest.json").read_text())
        for platform, asset in manifest["launchers"].items():
            path = self.artifacts / asset["url"].rsplit("/", 1)[1]
            package = json.loads(path.read_text())
            self.assertEqual(package["binary_sha256"], manifest["assets"][platform]["sha256"])
            self.assertEqual(asset["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertIn(f'{asset["sha256"]}  {path.name}', (output / "SHA256SUMS").read_text())
        output = self.root / "rejected-launcher"
        package_path = self.artifacts / LAUNCHERS["macos-aarch64"]
        package = json.loads(package_path.read_text())
        package["binary_sha256"] = "0" * 64
        package_path.write_text(json.dumps(package))
        result = self.run_tool(f"v{self.version}", output)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(output.exists())

    def test_wrong_tag_and_missing_binary_do_not_emit_manifest(self):
        output = self.root / "rejected"
        major, minor, patch = map(int, self.version.split("."))
        result = self.run_tool(f"v{major + 1}.{minor}.{patch}", output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Cargo 버전", result.stderr)
        self.assertFalse(output.exists())
        (self.artifacts / "aaa-windows-x86_64.exe").unlink()
        result = self.run_tool(f"v{self.version}", output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("aaa-windows-x86_64.exe", result.stderr)
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
