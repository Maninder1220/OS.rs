#!/usr/bin/env bash
set -euo pipefail

LOG_FILE="/var/log/local-ssd-setup.log"
MOUNT_POINT="/mnt/local-ssd"
DEVICE="/dev/disk/by-id/google-local-nvme-ssd-0"

exec > >(tee -a "${LOG_FILE}") 2>&1

echo "[INFO] Startup script started at $(date -Is)"

mkdir -p "${MOUNT_POINT}"

if [[ ! -e "${DEVICE}" ]]; then
  echo "[WARN] Local SSD device not found at ${DEVICE}. Skipping mount."
  exit 0
fi

if ! blkid "${DEVICE}" >/dev/null 2>&1; then
  echo "[INFO] Creating XFS filesystem on ${DEVICE}"
  mkfs.xfs -f "${DEVICE}"
else
  echo "[INFO] Existing filesystem found on ${DEVICE}"
fi

UUID="$(blkid -s UUID -o value "${DEVICE}")"

if ! grep -q "${UUID}" /etc/fstab; then
  echo "UUID=${UUID} ${MOUNT_POINT} xfs defaults,nofail,discard 0 2" >> /etc/fstab
fi

mount "${MOUNT_POINT}" || mount -a
chmod 1777 "${MOUNT_POINT}"

echo "[INFO] Local SSD mounted at ${MOUNT_POINT}"

