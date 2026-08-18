#!/usr/bin/env bash
set -euo pipefail

# Full, intentionally destructive VamoraSys installation.
# Existing release metadata is preserved; a bundled file is installed only when
# the target does not already have one.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SYSTEM_ROOT="${VAMORASYS_SYSTEM_ROOT:-../etc/VamoraSys}"
BINARY_PATH="${VAMORASYS_BINARY:-/usr/local/bin/vamorasys}"

if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
    TARGET_USER="${SUDO_USER}"
    TARGET_HOME="$(getent passwd "${TARGET_USER}" | cut -d: -f6)"
else
    TARGET_USER="${USER:-$(id -un)}"
    TARGET_HOME="${HOME}"
fi
USER_ROOT="${VAMORASYS_USER_ROOT:-${TARGET_HOME}/.VamoraSys}"

if [[ "${EUID}" -eq 0 && -z "${SUDO_USER:-}" ]]; then
    echo "Run install.sh as your normal user; it uses sudo only for system files." >&2
    exit 1
fi

USE_SUDO=0
if [[ "${SYSTEM_ROOT}" == "/etc/VamoraSys" ||
    "${BINARY_PATH}" == "/usr/local/bin/vamorasys" ||
    "${VAMORASYS_USE_SUDO:-0}" == "1" ]]; then
    USE_SUDO=1
fi

run_privileged() {
    if [[ "${EUID}" -eq 0 || "${USE_SUDO}" -eq 0 ]]; then
        "$@"
    else
        command -v sudo >/dev/null 2>&1 || {
            echo "sudo is required for system installation; install it or use custom paths" >&2
            exit 1
        }
        sudo "$@"
    fi
}

run_as_target_user() {
    if [[ "${EUID}" -eq 0 && "${TARGET_USER}" != "root" ]]; then
        sudo -u "${TARGET_USER}" -H env HOME="${TARGET_HOME}" "$@"
    else
        "$@"
    fi
}

echo "Building vamorasys..."
run_as_target_user cargo build --manifest-path "${SCRIPT_DIR}/Cargo.toml" --release

echo "Removing the existing VamoraSys binary and system settings..."
run_privileged rm -f -- "${BINARY_PATH}"
run_privileged rm -rf -- "${SYSTEM_ROOT}/settings"
run_privileged install -d -m 0755 -- "${SYSTEM_ROOT}"
run_privileged install -d -m 0755 -- "$(dirname -- "${BINARY_PATH}")"
run_privileged install -m 0755 -- "${SCRIPT_DIR}/target/release/vamorasys" "${BINARY_PATH}"
if [[ ! -e "${SYSTEM_ROOT}/vamora-release.vmf" && -f "${SCRIPT_DIR}/vamora-release.vmf" ]]; then
    run_privileged install -m 0644 -- "${SCRIPT_DIR}/vamora-release.vmf" "${SYSTEM_ROOT}/vamora-release.vmf"
fi

export VAMORASYS_SYSTEM_DIR="${SYSTEM_ROOT}/settings"
export VAMORASYS_USER_DIR="${USER_ROOT}/settings"
export VAMORASYS_RELEASE_DIR="${SYSTEM_ROOT}"

run_privileged env \
    VAMORASYS_SYSTEM_DIR="${VAMORASYS_SYSTEM_DIR}" \
    VAMORASYS_USER_DIR="${VAMORASYS_USER_DIR}" \
    VAMORASYS_RELEASE_DIR="${VAMORASYS_RELEASE_DIR}" \
    "${BINARY_PATH}" initialize --system
run_as_target_user env \
    VAMORASYS_SYSTEM_DIR="${VAMORASYS_SYSTEM_DIR}" \
    VAMORASYS_USER_DIR="${VAMORASYS_USER_DIR}" \
    VAMORASYS_RELEASE_DIR="${VAMORASYS_RELEASE_DIR}" \
    "${BINARY_PATH}" initialize --user
run_privileged chmod 0555 -- "${SYSTEM_ROOT}/settings"
run_privileged chmod 0644 -- "${SYSTEM_ROOT}/settings/"*.db
run_as_target_user chmod 0700 -- "${USER_ROOT}"
run_as_target_user chmod 0700 -- "${USER_ROOT}/settings"

echo "Verifying the fresh installation..."
run_as_target_user env \
    VAMORASYS_SYSTEM_DIR="${VAMORASYS_SYSTEM_DIR}" \
    VAMORASYS_USER_DIR="${VAMORASYS_USER_DIR}" \
    VAMORASYS_RELEASE_DIR="${VAMORASYS_RELEASE_DIR}" \
    "${BINARY_PATH}" status
echo "VamoraSys installation complete."