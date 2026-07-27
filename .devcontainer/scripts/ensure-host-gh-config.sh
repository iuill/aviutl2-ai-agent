#!/usr/bin/env bash

set -euo pipefail

config_dir="${1:?GitHub CLI config directory path is required}"

umask 077
mkdir -p "${config_dir}"
chmod 700 "${config_dir}"
