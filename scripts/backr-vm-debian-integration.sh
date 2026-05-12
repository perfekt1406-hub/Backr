#!/usr/bin/env bash
#
# Debian cloud guest under KVM with SSH forwarded to 127.0.0.1:2222, then runs the ignored
# `vm_backend_debian_guest` integration test (uses backr_lib over real ssh/rsync).
#
# Depends on: qemu-system-x86_64 KVM curl qemu-utils xorrisofs OR cloud-localds, openssh-client rsync cargo
#
# Usage:
#   ./scripts/backr-vm-debian-integration.sh [-- extra cargo test args]
#
# If host port 2222 is busy (another QEMU/user), override:
#   BACKR_VM_LOCAL_SSH_PORT=2223 ./scripts/backr-vm-debian-integration.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="${BACKR_VM_WORKDIR:-/tmp/backr-debian-vm}"
LOCAL_SSH="${BACKR_VM_LOCAL_SSH_PORT:-2222}"
mkdir -p "$WORKDIR"
cd "$WORKDIR"

IMG_URL="${BACKR_DEBIAN_QCOW_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-generic-amd64.qcow2}"
IMG_NAME="$(basename "${IMG_URL}")"

if [[ ! -f "$IMG_NAME" ]]; then
	echo "Fetching $IMG_URL ..."
	curl -fsSL --retry 4 "$IMG_URL" -o "${IMG_NAME}.partial"
	mv "${IMG_NAME}.partial" "$IMG_NAME"
fi

KEY="$WORKDIR/backr_vm_ed25519"
if [[ ! -f "$KEY" ]]; then
	ssh-keygen -t ed25519 -f "$KEY" -N "" -q
fi
read -r PUB <"${KEY}.pub"

cat >meta-data <<META
instance-id: backr-vm-ssh-001
local-hostname: backr-guest-debian
META

cat >user-data <<YAML
#cloud-config
users:
  - name: debian
    ssh_authorized_keys:
      - ${PUB}
    sudo: ALL=(ALL) NOPASSWD:ALL

packages:
  - rsync

runcmd:
  - [ mkdir, -p, /srv/backups ]
  - [ chown, debian:debian, /srv/backups ]
YAML

seed_cidata() {
	rm -f cidata.iso cidata.img
	if command -v cloud-localds >/dev/null; then
		cloud-localds cidata.iso user-data meta-data
	elif command -v xorrisofs >/dev/null; then
		xorrisofs -output cidata.iso -volid cidata -rock user-data meta-data
	else
		echo "need cloud-localds (cloud-image-utils) or xorrisofs (xorriso)" >&2
		exit 1
	fi
}

seed_cidata

OVERLAY="$WORKDIR/vm-root.qcow2"
if [[ ! -f "$OVERLAY" ]]; then
	qemu-img create -f qcow2 -F qcow2 -b "$WORKDIR/$IMG_NAME" "$OVERLAY"
fi

PIDF="$WORKDIR/qemu.pid"
if [[ -f "$PIDF" ]] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then
	echo "QEMU already running (pid $(cat "$PIDF"))"
else
	qemu-system-x86_64 \
		-machine accel=kvm,type=q35 \
		-cpu host \
		-smp 2 \
		-m 2048 \
		-display none \
		-netdev user,id=n0,hostfwd=tcp:127.0.0.1:${LOCAL_SSH}-:22 \
		-device virtio-net-pci,netdev=n0 \
		-drive "file=${OVERLAY},if=virtio,format=qcow2" \
		-drive "file=${WORKDIR}/cidata.iso,if=virtio,media=cdrom,readonly=on" \
		-daemonize \
		-pidfile "$PIDF"
	echo "started QEMU (pid $(cat "$PIDF"))"
	sleep 12
fi

ssh_ok() {
	ssh \
		-o BatchMode=yes \
		-o ConnectTimeout=10 \
		-o StrictHostKeyChecking=no \
		-o UserKnownHostsFile=/dev/null \
		-p "${LOCAL_SSH}" \
		-i "$KEY" debian@127.0.0.1 "$@"
}

echo "waiting for Debian SSH on ${LOCAL_SSH} ..."
for _ in $(seq 1 90); do
	if ssh_ok echo backr-vm-ready; then
		export BACKR_VM_HOST="${BACKR_VM_HOST:-127.0.0.1}"
		export BACKR_VM_PORT="${BACKR_VM_PORT:-${LOCAL_SSH}}"
		export BACKR_VM_USER="${BACKR_VM_USER:-debian}"
		export BACKR_VM_KEY_PATH="$KEY"
		cd "$ROOT/src-tauri"
		set +e
		cargo test --test vm_backend_debian_guest -- --ignored --nocapture "$@"
		st=$?
		set -e
		exit "$st"
	fi
	sleep 4
done
echo "timed out waiting for Debian guest SSH" >&2
exit 1
