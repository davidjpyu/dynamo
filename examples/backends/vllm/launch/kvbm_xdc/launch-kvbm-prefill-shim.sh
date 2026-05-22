#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$SCRIPT_DIR/hardware-profiles.sh"
kvbm_xdc_apply_hardware_profile model-only

WORKTREE=${WORKTREE:-/workspace}
VENV=${VENV:-/opt/dynamo/venv}
SOURCE_KVBM_RUNTIME_ENV=${SOURCE_KVBM_RUNTIME_ENV:-1}
KVBM_HUB_PREFILL_ENDPOINT=${KVBM_HUB_PREFILL_ENDPOINT:?KVBM_HUB_PREFILL_ENDPOINT is required}
KVBM_HUB_PREFILL_SHIM_HOST=${KVBM_HUB_PREFILL_SHIM_HOST:-127.0.0.1}
KVBM_HUB_PREFILL_SHIM_PORT=${KVBM_HUB_PREFILL_SHIM_PORT:-8001}

: "${MODEL:?MODEL must be set by KVBM_HARDWARE_PROFILE or env override}"

export PATH="$VENV/bin:/usr/local/cargo/bin:/usr/local/cuda/bin:$PATH"
export PYTHONHASHSEED=${PYTHONHASHSEED:-0}

if [[ "$SOURCE_KVBM_RUNTIME_ENV" == "1" && -f "$WORKTREE/.claude/skills/disagg-bringup/env.sh" ]]; then
  export KVBM_REPO=${KVBM_REPO:-$WORKTREE}
  export KVBM_VENV=${KVBM_VENV:-$VENV}
  set +u
  # shellcheck source=/dev/null
  . "$WORKTREE/.claude/skills/disagg-bringup/env.sh"
  set -u
fi

for lib_dir in "$WORKTREE/.image-target/debug/deps" "$WORKTREE/.image-target-kvbm/debug/deps" /usr/local/lib/python*/site-packages/nixl_cu*.libs; do
  if [[ -d "$lib_dir" ]]; then
    export LD_LIBRARY_PATH="$lib_dir:${LD_LIBRARY_PATH:-}"
  fi
done
if [[ -d "$WORKTREE/lib/bindings/python/src" ]]; then
  export PYTHONPATH="$WORKTREE/lib/bindings/python/src:${PYTHONPATH:-}"
fi
if [[ -d "$WORKTREE/lib/bindings/kvbm/python" ]]; then
  export PYTHONPATH="$WORKTREE/lib/bindings/kvbm/python:${PYTHONPATH:-}"
fi
if [[ -d "$WORKTREE/components/src" ]]; then
  export PYTHONPATH="$WORKTREE/components/src:${PYTHONPATH:-}"
fi

echo "[prefill-shim] model=$MODEL endpoint=$KVBM_HUB_PREFILL_ENDPOINT port=$KVBM_HUB_PREFILL_SHIM_PORT"
exec "$VENV/bin/python" "$SCRIPT_DIR/kvbm-prefill-completions-shim.py"
