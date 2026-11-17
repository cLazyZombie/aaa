#!/usr/bin/env python3
"""GitHub Windows의 확인된 ConPTY 정체만 새 프로세스에서 한 번 재확인한다."""
import argparse
import codecs
import json
import os
from pathlib import Path
import re
import subprocess
import sys

OWNERS = {
    'worker_lifetime::approved_worker_outlives_its_cli_and_records_the_same_id_after_private_display_disappears',
    'wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli',
    'recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied',
}


def stalled_owners(output):
    failed = set(re.findall(r'^test (\S+) \.\.\. FAILED\s*$', output, re.M))
    if not failed or not failed <= OWNERS:
        return []
    # 다른 crate·binary의 실패나 Cargo 자체 오류를 재확인으로 덮지 않는다.
    if re.findall(r'^error: (\d+) targets? failed:', output, re.M) != ['1'] or not output.rstrip().endswith('`-p aaa --test install`'):
        return []
    blocks = {}
    sections = re.split(r'^---- (\S+) (?:stdout|stderr) ----\s*$', output, flags=re.M)
    for name, body in zip(sections[1::2], sections[2::2]):
        blocks[name] = blocks.get(name, '') + body
    for name in failed:
        body = blocks.get(name, '')
        if not all((
            re.search(r'install 승인 PTY\(\d+\) 사건 상한', body),
            'PTY stalled:' in body,
            'ReadConsoleW' in body,
            re.search(r'"reader_io_pending"\s*:\s*\{[^}]*"value"\s*:\s*true', body),
            re.search(r'"prefix_enter_down"\s*:\s*true', body),
        )):
            return []
    return sorted(failed)


def execute(command, cwd, log):
    decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
    with log.open('wb') as file:
        with subprocess.Popen(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              bufsize=0) as child:
            while chunk := child.stdout.read(8192):
                file.write(chunk)
                file.flush()
                sys.stdout.write(decoder.decode(chunk))
                sys.stdout.flush()
            sys.stdout.write(decoder.decode(b'', final=True))
            return child.wait()


def run(cwd, logs, cargo=('cargo',)):
    logs.mkdir(parents=True, exist_ok=True)
    first = logs / 'workspace-first.log'
    code = execute([*cargo, 'test', '--profile', 'verify', '--workspace', '--locked', '--no-fail-fast'], cwd, first)
    summary = {'first_exit': code, 'rechecks': [], 'exit': code}
    if code == 101:
        for index, name in enumerate(stalled_owners(first.read_text(encoding='utf-8', errors='replace')), 1):
            log = logs / f'owner-{index}.log'
            result = execute([*cargo, 'test', '--profile', 'verify', '--locked', '-p', 'aaa', '--test', 'install',
                              name, '--', '--exact'], cwd, log)
            summary['rechecks'].append({'owner': name, 'exit': result, 'log': log.name})
        if summary['rechecks']:
            summary['exit'] = 0 if all(row['exit'] == 0 for row in summary['rechecks']) else 101
            print('::warning title=Windows ConPTY 최초 정체::Enter 도착·ReadConsoleW 대기 서명이 확인된 owner만 새 프로세스에서 한 번 재확인했다. 최초 원문을 보존한다.')
    (logs / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    if summary['rechecks'] and os.environ.get('GITHUB_STEP_SUMMARY'):
        with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a', encoding='utf-8') as file:
            file.write('\n### Windows ConPTY 재확인\n\n최초 workspace 실패를 보존한다.\n\n| owner | 최초 | 새 프로세스 1회 |\n| --- | --- | --- |\n')
            for row in summary['rechecks']:
                file.write(f"| {row['owner']} | 정체 | {'통과' if row['exit'] == 0 else '실패'} |\n")
    return summary['exit']


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace', newline='')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--logs', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('AAA_CI_WINDOWS_INSTALL_RECHECK') != '1':
        parser.error('명시한 GitHub Windows CI 전용이다. 로컬 gate는 최초 실패로 끝난다')
    return run(Path.cwd(), args.logs)


if __name__ == '__main__':
    sys.exit(main())
