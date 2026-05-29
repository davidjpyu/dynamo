<!--
SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# Prewarm — Populate shared-model-cache PVC

The vLLM-H200 K8s recipe expects the `shared-model-cache` PVC mounted at `/opt/models` to contain both:

1. The HuggingFace snapshot of `nvidia/Nemotron-Ultra-V3-rl3-050826-mixed_nvfp4-fp8_amax_1024x65k` (revision `469ed01...`) under `hub/`
2. A tokenizer-patched view under `patched/nemotron-ultra-ea-trtllm-tokenizer-patch-469ed01.../` with `tokenizer_class: PreTrainedTokenizerFast` (the default `TokenizersBackend` is not picked up by vLLM)

A clean namespace will NOT have this populated. The deploy YAML mounts `/opt/models` read-only with `HF_HUB_OFFLINE=1`, so the worker cannot self-populate.

## When You Need This

Run this Job **before applying the deploy YAMLs** if:
- Your namespace's `shared-model-cache` PVC is empty
- You're deploying into a new cluster / new region
- Worker is failing with `HFValidationError: Repo id must be in the form 'repo_name'...` (means `MODEL_PATH` directory doesn't exist)

## Prerequisites

1. **`shared-model-cache` PVC**. Apply `prewarm/pvc.yaml` to create it (edit `storageClassName` first to match your cluster — `kubectl get storageclass`). The PVC requests 1 TiB with `ReadWriteMany`. The same PVC name is consumed by the deploy YAMLs.
2. **`hf-token-secret`** exists in your namespace and contains `HF_TOKEN` matching an HF account with access to the (currently private) `nvidia/Nemotron-Ultra-V3-...` checkpoint:
   ```bash
   kubectl -n <namespace> create secret generic hf-token-secret \
     --from-literal=HF_TOKEN="hf_..."
   ```
3. **`nvcr-secret`** image pull secret (only needed if your cluster blocks `docker.io/library/python:3.12-slim`; otherwise can be removed from the manifest).

## Run

```bash
# 1. Create PVC (edit storageClassName first)
kubectl -n <namespace> apply -f prewarm/pvc.yaml

# 2. Populate the PVC with the checkpoint + tokenizer-patched view
kubectl -n <namespace> apply -f prewarm/prewarm-model-cache.yaml

# Watch the Job complete (~10-30 min depending on network and PVC throughput)
kubectl -n <namespace> wait --for=condition=complete --timeout=60m \
  job/ultra-h200-prewarm-model-cache

kubectl -n <namespace> logs job/ultra-h200-prewarm-model-cache | tail -20
```

Expected final log line: `PREWARM_PASS ts=...`

## What It Does

The Job mounts `shared-model-cache` read-write at `/opt/models`, then:

1. Verifies `HF_TOKEN` is set
2. Verifies PVC is mounted writable
3. Skips early if `/opt/models/patched/<patch-name>/config.json` already exists (idempotent)
4. `pip install "huggingface_hub[hf_xet]==1.16.4"` (pinned)
5. `hf download <repo> --revision <sha> --cache-dir /opt/models/hub` (~329 GiB, ~6-15 min with Xet high-performance transfer on good networks)
6. Builds the tokenizer-patched view at `/opt/models/patched/<patch-name>/` using relative symlinks back to `../../hub/...` plus a patched `tokenizer_config.json` (changes `tokenizer_class: TokenizersBackend` → `PreTrainedTokenizerFast`, removes `backend` and `is_local` fields)
7. Verifies `config.json`, `ultra_v3_reasoning_parser.py`, `tokenizer_config.json`, `chat_template.jinja` all present in patched view

## Failure Classes

| Class | Cause | Fix |
|---|---|---|
| `missing_hf_token` | `HF_TOKEN` env not populated | Create `hf-token-secret` with `HF_TOKEN=hf_...` |
| `pvc_not_writable` | Can't `mkdir` under `/opt/models` | Check PVC `accessModes` (needs `ReadWriteOnce` or `ReadWriteMany`), check PV bindings |
| `snapshot_not_found` | `hf download` didn't write expected path | Check network access to `huggingface.co`, HF token has read permission |
| `missing_required_file` | After symlinking, expected file not found | Likely partial download; delete `/opt/models/hub/...` and rerun |
| Pod `OOMKilled` (exit 137) | Container memory limit too low for `hf download` + `hf_xet` chunk-graph state | Resources block sized at 64 GiB request / 128 GiB limit with `--max-workers 4`. **Do not lower memory limit without also lowering `--max-workers`** in the args. A previous 16 GiB limit + `--max-workers 8` was reported OOMed by QA on 2026-05-29. |

## After Prewarm: Apply Deploy

```bash
# Chat workload
kubectl -n <namespace> apply -f agg1tp8/deploy-chat-c12.yaml

# OR SWE workload
kubectl -n <namespace> apply -f agg1tp8/deploy-swe-c10.yaml
```

The worker's prestart preflight will now find `${MODEL_PATH}/config.json` and proceed past GPU guard into model load.

## Why Not Build This into the Worker?

The deploy YAML deliberately mounts `/opt/models` **read-only** so the running worker cannot mutate the cache. This avoids issues like:
- Concurrent workers racing to download the same model
- Worker downloads corrupting cache mid-flight
- Worker's offline-mode invariant being broken by an unexpected download attempt

The prewarm Job is the canonical, deterministic, one-shot path to populate the cache.

## Idempotency

Running this Job repeatedly is safe — it detects existing `config.json` + `ultra_v3_reasoning_parser.py` in the patched view and exits early (`PREWARM_SKIP class=already_populated`). Safe to apply across multiple deploy cycles.
