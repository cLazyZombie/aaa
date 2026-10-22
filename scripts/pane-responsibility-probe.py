#!/usr/bin/env python3
"""격리된 새 C·Rust·cargo/build.rs/run의 첫 실행·재실행 시간과 실제 macOS 책임자를 기록한다.

pane 안에서 실행한다. --output-dir은 보존할 새 폴더이며 기존 파일을 덮지 않는다.
권한·TCC·launchd·launcher는 바꾸지 않는다. 시각만으로 개발자 도구 승인을 판정하지 않는다.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import uuid


def responsibility():
    system = ctypes.CDLL(None)
    lookup = system.responsibility_get_pid_responsible_for_pid
    lookup.argtypes = [ctypes.c_int]
    lookup.restype = ctypes.c_int
    pid = lookup(os.getpid())
    image = ctypes.create_string_buffer(4096)
    count = system.proc_pidpath(pid, image, len(image)) if pid > 0 else 0
    if count <= 0:
        raise RuntimeError("실제 책임 프로세스 조회 실패")
    return {"pid": os.getpid(), "responsible": pid, "path": os.fsdecode(image.value)}


C = r'''
#include <dlfcn.h>
#include <stdio.h>
#include <unistd.h>
int main(void) {
    int (*lookup)(int) = dlsym(RTLD_DEFAULT, "responsibility_get_pid_responsible_for_pid");
    int (*image)(int, void *, unsigned int) = dlsym(RTLD_DEFAULT, "proc_pidpath");
    int pid = lookup ? lookup(getpid()) : -1;
    char path[4096] = {0};
    if (!image || pid <= 0 || image(pid, path, sizeof(path)) <= 0) return 2;
    printf("{\"nonce\":\"NONCE\",\"pid\":%d,\"responsible\":%d,\"path\":\"%s\"}\n", getpid(), pid, path);
    return 0;
}
'''
RUST = r'''
use std::ffi::{CStr, c_void};
unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
    fn proc_pidpath(pid: i32, path: *mut c_void, size: u32) -> i32;
}
fn main() {
    let pid = std::process::id();
    let responsible = unsafe { responsibility_get_pid_responsible_for_pid(pid as i32) };
    let mut path = [0u8; 4096];
    assert!(responsible > 0 && unsafe { proc_pidpath(responsible, path.as_mut_ptr().cast(), path.len() as u32) } > 0);
    let path = unsafe { CStr::from_ptr(path.as_ptr().cast()) }.to_str().unwrap();
    println!("PREFIX{{\"nonce\":\"NONCE\",\"pid\":{pid},\"responsible\":{responsible},\"path\":\"{path}\"}}");
}
'''


def run(args, **kwargs):
    return subprocess.run([os.fspath(a) for a in args], check=True, capture_output=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--expected-responsible", type=int, required=True)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("macOS 전용 진단이다")
    root = args.output_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    records = []
    own = responsibility()
    if own["responsible"] != args.expected_responsible:
        raise RuntimeError(f"진단 입구의 책임자가 다르다: {own}")
    nonce = uuid.uuid4().hex
    c = root / "fresh.c"
    c.write_text(C.replace("NONCE", nonce))
    rust = root / "fresh.rs"
    rust.write_text(RUST.replace("NONCE", nonce).replace("PREFIX", ""))
    cbin, rbin = root / "fresh-c", root / "fresh-rust"
    run(["cc", "-O2", "-o", cbin, c])
    run(["rustc", "--edition=2024", "-o", rbin, rust])

    def verify(item):
        if item["nonce"] != nonce or item["responsible"] != args.expected_responsible:
            raise RuntimeError(f"실행 내용·책임 프로세스 불일치: {item}")

    for binary in [cbin, rbin]:
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        for invocation in ["first", "warm"]:
            start = time.monotonic_ns()
            output = run([binary])
            elapsed = (time.monotonic_ns() - start) / 1_000_000
            item = json.loads(output.stdout)
            verify(item)
            records.append({"kind": binary.name, "invocation": invocation, "elapsed_ms": elapsed, "sha256": digest, **item})
    crate = root / "cargo-probe"
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text('[package]\nname = "aaa-responsibility-probe"\nversion = "0.0.0"\nedition = "2024"\n[workspace]\n')
    (crate / "src/main.rs").write_text(rust.read_text())
    (crate / "build.rs").write_text(RUST.replace("NONCE", nonce).replace("PREFIX", "cargo:warning=aaa-probe="))
    # 임시 OS 폴더의 cargo target을 만들지 않는다. caller가 보존하는 출력 폴더 안에 둔다.
    env = {**os.environ, "CARGO_TARGET_DIR": str(root / "cargo-target")}
    cargo = run(["cargo", "run", "--offline", "--manifest-path", crate / "Cargo.toml", "-vv"], env=env)
    (root / "cargo.log").write_text(cargo.stdout + cargo.stderr)
    main_record = json.loads(cargo.stdout.strip().splitlines()[-1])
    verify(main_record)
    records.append({"kind": "cargo-run", **main_record})
    build_records = []
    for line in (cargo.stdout + cargo.stderr).splitlines():
        marker = "aaa-probe="
        if marker in line:
            item = json.loads(line.split(marker, 1)[1])
            verify(item)
            if item not in build_records:
                build_records.append(item)
    if len(build_records) != 1:
        raise RuntimeError(f"build.rs 관측 수 오류: {build_records}")
    records.append({"kind": "cargo-build-script", **build_records[0]})
    result = {"entry": own, "nonce": nonce, "records": records}
    (root / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
