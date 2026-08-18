#!/usr/bin/env bash
set -euo pipefail

# Binary-only update. Databases, settings, and release metadata are preserved.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BINARY_PATH="${VAMORASYS_BINARY:-/usr/local/bin/vamorasys}"

if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
    TARGET_USER="${SUDO_USER}"
    TARGET_HOME="$(getent passwd "${TARGET_USER}" | cut -d: -f6)"
else
    TARGET_USER="${USER:-$(id -un)}"
    TARGET_HOME="${HOME}"
fi

if [[ "${EUID}" -eq 0 && -z "${SUDO_USER:-}" ]]; then
    echo "Run update.sh as your normal user; it uses sudo only to replace the binary." >&2
    exit 1
fi

USE_SUDO=0
if [[ "${BINARY_PATH}" == "/usr/local/bin/vamorasys" ||
    "${VAMORASYS_USE_SUDO:-0}" == "1" ]]; then
    USE_SUDO=1
fi

run_privileged() {
    if [[ "${EUID}" -eq 0 || "${USE_SUDO}" -eq 0 ]]; then
        "$@"
    else
        command -v sudo >/dev/null 2>&1 || {
            echo "sudo is required to replace the system binary; install it or use a custom path" >&2
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

echo "Building the new vamorasys binary..."
run_as_target_user cargo build --manifest-path "${SCRIPT_DIR}/Cargo.toml" --release

if [[ ! -x "${SCRIPT_DIR}/target/release/vamorasys" ]]; then
    echo "release binary was not produced" >&2
    exit 1
fi

run_privileged install -d -m 0755 -- "$(dirname -- "${BINARY_PATH}")"
run_privileged install -m 0755 -- "${SCRIPT_DIR}/target/release/vamorasys" "${BINARY_PATH}"
echo "Updated ${BINARY_PATH}; existing VamoraSys data was preserved."