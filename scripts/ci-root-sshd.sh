#!/usr/bin/env bash
# CI(macos-15 일회성 runner) 전용: root가 띄운 시스템 /usr/sbin/sshd로 「SSH로 추가」 설치·등록을 끝까지 본다(plan §8.420).
# install의 검증 SSH 입구는 root 소유 /usr/sbin/sshd 계보만 믿으므로 비특권 실험실 sshd로는 볼 수 없다.
#
# - sshd는 sudo로 loopback 빈 포트에만 띄우고 실험실 키만 받는다. 끝나면 이 스크립트가 띄운 PID만 끝낸다.
# - 원격 HOME은 `SetEnv HOME=<실험실>/home`이다: 설치 자리·LaunchAgent plist·launcher가 그 아래에 놓인다. 시험이 runner 실제 HOME이
#   그대로인지 본다. gui domain 등록(dev.aaa.agent)은 계정 전역이라 시험이 없을 때만 등록하고 끝에 bootout한다.
# - 운영 Mac에서는 돌리지 않는다(GITHUB_ACTIONS가 아니면 멈춘다).
#
# 사용: bash scripts/ci-root-sshd.sh
set -euo pipefail

[ "${GITHUB_ACTIONS:-}" = true ] || { echo "CI(GitHub Actions) 전용이다" >&2; exit 64; }
[ "$(uname)" = Darwin ] || { echo "macOS 전용이다" >&2; exit 64; }

LAB="${RUNNER_TEMP:?}/aaa-root-sshd"
rm -rf "${LAB:?}"
mkdir -p "$LAB/home"
ssh-keygen -q -t ed25519 -N '' -f "$LAB/hostkey"
ssh-keygen -q -t ed25519 -N '' -f "$LAB/userkey"
cp "$LAB/userkey.pub" "$LAB/authorized_keys"
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
ME=$(id -un)

cat > "$LAB/sshd_config" <<EOF
ListenAddress 127.0.0.1:$PORT
HostKey $LAB/hostkey
PidFile $LAB/sshd.pid
AuthorizedKeysFile $LAB/authorized_keys
AllowUsers $ME
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
SetEnv HOME=$LAB/home
EOF
cat > "$LAB/ssh_config" <<EOF
Host ci
  HostName 127.0.0.1
  Port $PORT
  User $ME
  IdentityFile $LAB/userkey
  IdentitiesOnly yes
EOF

sudo -n /usr/sbin/sshd -t -f "$LAB/sshd_config"
sudo -n /usr/sbin/sshd -f "$LAB/sshd_config" -E "$LAB/sshd.log"
stop() {
  if [ -s "$LAB/sshd.pid" ]; then sudo -n kill "$(cat "$LAB/sshd.pid")" || true; fi
}
trap stop EXIT
for _ in $(seq 1 100); do
  if [ -s "$LAB/sshd.pid" ] && nc -z 127.0.0.1 "$PORT" 2>/dev/null; then break; fi
  sleep 0.1
done
nc -z 127.0.0.1 "$PORT" || { echo "root sshd가 뜨지 않았다" >&2; cat "$LAB/sshd.log" >&2; exit 1; }
echo "root sshd pid $(cat "$LAB/sshd.pid") port $PORT"

# 진단: 이 sshd로 연 원격 명령의 조상(사용자·실행 image). install 검증 SSH 입구는 이 사슬에서 root 소유 /usr/sbin/sshd를 찾는다
ssh -F "$LAB/ssh_config" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR ci \
  'p=$$; while [ "${p:-0}" -gt 0 ]; do ps -o pid=,ppid=,user=,comm= -p "$p"; [ "$p" -eq 1 ] && break; p=$(ps -o ppid= -p "$p" | tr -d " "); done' || true

if ! AAA_ROOT_SSHD_LAB="$LAB" cargo test -p aaa --test m13 --locked \
  ssh_install::ssh_install_through_the_system_sshd_starts_a_launch_agent_that_connects_and_checks \
  -- --ignored --exact --nocapture; then
  echo "── sshd.log ──" >&2
  tail -n 60 "$LAB/sshd.log" >&2 || true
  exit 1
fi
