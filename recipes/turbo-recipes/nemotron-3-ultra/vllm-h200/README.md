<!--
SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# Nemotron-3-Ultra vLLM Recipe — H200 Lane

> **NVFP4-on-H200 validation evidence + recipe deliverable**, mirroring Sungsoo's B200 recipe layout. This is the H200 sibling to `recipes/turbo-recipes/nemotron-3-ultra/vllm/` for cross-platform deployment options.

## Image

Same base + patch stack as B200 recipe — vLLM Patch06+humming with Ultra MTP DS-copy + P/D SSM/NIXL tailfix. The Triton/JIT path means humming kernels and MTP work cross-arch (sm90 + sm100).

```text
nvcr.io/nvstaging/nim/sungsooh:nemotron-ultra-vllm-patch06-humming-mtp-ds-copy-ssm-tailfix-20260526T061806Z
@sha256:b4a948fd7560ba072a46762bc026f1fefdac7ab276ed02798ffd1fc958a7cc3a
```

Reused as-is for H200. No H200-specific rebuild required (sm90 falls through Triton/JIT in vLLM's NVFP4 MoE path, which works without sm100 CUTLASS kernels).

## Topology Decision

**H200 winning recipe = AGG1 TP8** for both chat and SWE workloads.

| Workload | Topology | TP | GPUs | Sweet c (r=2000 sustained) | TPS/GPU | TPS/user |
|---|---|---:|---:|---:|---:|---:|
| Chat (8K/1K @70%) | AGG1 | **8** | 8 | 12 | 84.0 | 56.0 |
| SWE (64K/400 @90%) | AGG1 | **8** | 8 | 10 | 63.0 | 50.4 |

**Per-node throughput vs Sungsoo B200:** chat 42% (672/1616), SWE 65% (504/770). PRD-compliant but below B200 per-GPU efficiency.

> See [`H200_LANE_FINAL_REPORT.md`](../docs/H200_LANE_FINAL_REPORT.md) and [`H200_RECIPE_SWEEP.md`](../docs/H200_RECIPE_SWEEP.md) for full evidence and sweep history.

## What's Different from B200 Recipe

| Aspect | Sungsoo B200 | H200 (this lane) | Why |
|---|---|---|---|
| Topology | AGG1 TP4 (chat) / AGG2 TP4 (SWE) | **AGG1 TP8 (both)** | sm90 needs wider TP to aggregate HBM bandwidth (compensates for no native NVFP4 MoE kernels) |
| GPU count chat | 4 | 8 | TP8 spans full node |
| max-num-seqs chat | 72 | **24** | mns ≈ 2× operating c (more honest test than 256) |
| max-num-seqs SWE | 32 | **20** | Same principle |
| max-model-len | 262144 | **65536** | H200 KV budget; labeled `server_cap_65536_trace_slice` |
| block-size | 32 (Sungsoo k8s default) | 64 (currently) | block-size tuning in progress |
| gpu-memory-utilization | 0.9 | 0.85 | H200 tighter HBM budget |
| nodeSelector | NVIDIA-B200 | NVIDIA-H200 | platform gate |
| Sweet c chat | 68 | **12** | H200 saturates earlier — wider TP = lower c at SLA |
| Sweet c SWE | 32 | **10** | Same effect — H200 SWE c ≈ 1/3 of B200 |
| Required env (Mamba P/D) | — | `VLLM_ALLOW_CHUNKED_LOCAL_ATTN_WITH_HYBRID_KV_CACHE=1` | sm90 path needs this for hybrid KV manager |
| Required env (P/D NIXL) | — | `VLLM_SSM_CONV_STATE_LAYOUT=DS` | 3-read Mamba conv transfer asserts |
| docker GPU flag (bare metal) | `--device=nvidia.com/gpu=all` | `--gpus all` | viking-prod-214 has no CDI |
| AIPerf cache_salt | accept | `CACHE_SALT_MODE=omit` | Dynamo frontend rejects (HTTP 400) — works with fresh server |

## Operating Points

### Chat AGG1 TP8 (`agg1tp8/deploy-chat-c12.yaml`)

| Metric | Value |
|---|---:|
| Sweet concurrency | **12** |
| TPS/GPU | 84.0 |
| TPS/user | 56.0 (+12% above 50 SLA) |
| TTFT avg | 713 ms |
| ITL avg | 17.6 ms |
| Per-node total | 672 TPS |
| Burst cap (`mns`) | 24 |

### SWE AGG1 TP8 (`agg1tp8/deploy-swe-c10.yaml`)

| Metric | Value |
|---|---:|
| Sweet concurrency | **10** |
| TPS/GPU | 63.0 |
| TPS/user | 50.4 (just above 50 SLA — tight margin) |
| TTFT avg | 570 ms |
| ITL avg | 18.2 ms |
| Per-node total | 504 TPS |
| Burst cap (`mns`) | 20 |

**SWE has tight SLA margin** — operators should monitor TPS/user and avoid pushing c above 10 in production.

## v1alpha1 Mirror (Testing Convenience)

The primary deploy YAMLs use `apiVersion: nvidia.com/v1beta1`. For clusters whose `dynamo-platform` release only serves `v1alpha1` (e.g. the public `dynamo-platform-1.1.1` on NGC, 2026-05-09), we ship a functionally identical `v1alpha1` mirror alongside each deploy YAML:

| v1beta1 (primary) | v1alpha1 (mirror) |
|---|---|
| `agg1tp8/deploy-chat-c12.yaml` | `agg1tp8/deploy-chat-c12-v1alpha1.yaml` |
| `agg1tp8/deploy-swe-c10.yaml` | `agg1tp8/deploy-swe-c10-v1alpha1.yaml` |

The two versions are kept in lockstep for server params (TP, mns, mbt, block-size, c, image, env). The mirror exists only so QA can validate H200 lane while the test cluster catches up to v1beta1. **Do not diverge server-parameter values between the two.**

Both v1alpha1 mirrors have been schema-validated locally against the v1alpha1 CRD schema from `ai-dynamo/dynamo:main`.

## Pareto Curve (chat AGG1 TP8, r=2000 sustained)

If the customer can relax the TPS/user floor below 50, higher c is available:

| c | TPS/GPU | TPS/user | TTFT | ITL | SLA |
|---:|---:|---:|---:|---:|:---:|
| 8 | 68.9 | 68.9 | 743 ms | 13.6 ms | ✅ +38% margin |
| **12** | 84.0 | 56.0 | 713 ms | 17.6 ms | ✅ +12% margin (recommended) |
| 16 | 93.1 | 46.5 | 741 ms | 21.1 ms | ❌ below 50 SLA |

For latency-priority deployments, choose c=8 (faster ITL, more TPS/user headroom, but only 551 per-node TPS). For throughput-priority, c=12 is the maximum that still passes SLA.

## Server Configuration

Server flags applied to `dynamo.vllm` (in agg1tp8/deploy-chat-c12.yaml):

```bash
python3 -m dynamo.vllm \
  --model "${MODEL_PATH}" \
  --served-model-name "${SERVED_MODEL_NAME}" \
  --tensor-parallel-size 8 \
  --trust-remote-code \
  --max-model-len 65536 \
  --max-num-seqs 24 \
  --max-num-batched-tokens 32768 \
  --gpu-memory-utilization 0.85 \
  --block-size 64 \
  --mamba-cache-mode align \
  --enable-prefix-caching \
  --spec-method nemotron_h_mtp --spec-tokens 1 \
  --dyn-tool-call-parser qwen3_coder \
  --dyn-reasoning-parser nemotron3 \
  --reasoning-parser-plugin "${MODEL_PATH}/ultra_v3_reasoning_parser.py" \
  --reasoning-parser nemotron_v3
```

Required envs (set in deployment spec):

```bash
VLLM_ALLOW_CHUNKED_LOCAL_ATTN_WITH_HYBRID_KV_CACHE=1
# (P/D only) VLLM_SSM_CONV_STATE_LAYOUT=DS
```

## H200 Blockers Documented

TRT-LLM and SGLang are H200-blocked due to NVFP4 sm90 kernel gaps. Only vLLM is viable on Hopper for NVFP4 Nemotron Ultra. See:

- [`TRTLLM_NVFP4_MOE_SM90_BLOCKER.md`](../docs/TRTLLM_NVFP4_MOE_SM90_BLOCKER.md) — TRT-LLM trtllmGen NVFP4 MoE kernel is sm100-only
- [`SGLANG_MODELOPT_MIXED_SM90_BLOCKER.md`](../docs/SGLANG_MODELOPT_MIXED_SM90_BLOCKER.md) — SGLang `ModelOptMixedPrecisionConfig.get_min_capability()=100`

## Mooncake Trace Reference

Same NGC `nim_turbo_traces` model as B200 lane:
- Chat: `nim_turbo_8k_1k_70kv_chat_new_noschedule.jsonl`
- SWE: `nim_turbo_64k_400_90kv_agent_new_noschedule.jsonl`

Filter against `max_model_len=65536` (H200 cap) with `template_margin=512`:
- Chat: 12,031 raw → 10,237 filtered (85% kept)
- SWE: 23,608 raw → 11,201 filtered (47% kept — many over-cap records dropped due to 64K context cap)

Filter output at `/tmp/davidyu/mooncake-aiperf-prep/moon_aiperf_<ts>/filtered_traces/`.

## P/D 1P+1D Alternative (Optional Template)

For deployments that want disagg topology (e.g., for latency separation between prefill and decode), 1P+1D is functional on H200 chat workload (matches AGG1 per-GPU efficiency, scales to 2× total with 2× GPUs).

SWE 1P+1D works but per-GPU is lower than AGG1 TP8 (Sungsoo's "not perf-ready" caveat softens on H200 chat, persists on SWE).

P/D YAML not yet shipped — to add to `disagg/` subfolder if needed.

## Local Direct-Docker Test (bare metal)

Equivalent of K8s YAML for local validation. Used during this lane's R&D on viking-prod-214:

```bash
docker run -d --name h200-agg1tp8 \
  --gpus all --network host --ipc=host --shm-size 64g \
  --ulimit memlock=-1 --ulimit stack=67108864 --user 0:0 \
  -e HF_HOME=/hf-cache -e HF_HUB_CACHE=/hf-cache/hub \
  -e HF_MODULES_CACHE=/tmp/hf_modules \
  -e HF_HUB_OFFLINE=1 -e TRANSFORMERS_OFFLINE=1 \
  -e VLLM_ALLOW_CHUNKED_LOCAL_ATTN_WITH_HYBRID_KV_CACHE=1 \
  -v "$HF_CACHE_ROOT:/hf-cache:ro" -v "$RUN_ROOT:/artifacts" \
  --entrypoint /bin/bash "$IMAGE" -c "sleep infinity"

# Then inside container:
DYN_FILE_KV=/tmp/dynamo_store_kv \
python3 -m dynamo.frontend --discovery-backend file --router-mode round-robin \
  --http-host 0.0.0.0 --http-port 18750 &

CUDA_VISIBLE_DEVICES=0,1,2,3,4,5,6,7 \
python3 -m dynamo.vllm \
  --discovery-backend file \
  --model /hf-cache/patched/nemotron-ultra-ea-trtllm-tokenizer-patch-... \
  --served-model-name nemotron-ultra-ea \
  --tensor-parallel-size 8 --trust-remote-code \
  --max-model-len 65536 --max-num-seqs 24 --max-num-batched-tokens 32768 \
  --gpu-memory-utilization 0.85 --block-size 64 \
  --enable-prefix-caching --mamba-cache-mode align \
  --spec-method nemotron_h_mtp --spec-tokens 1 \
  --dyn-tool-call-parser qwen3_coder --dyn-reasoning-parser nemotron3 \
  --reasoning-parser-plugin /hf-cache/patched/.../ultra_v3_reasoning_parser.py \
  --reasoning-parser nemotron_v3
```

Driver scripts at: `scripts/run_agg1tp8_*.sh` (host-side bare-metal harness used during sweep).

## Status

- ✅ All evidence collected (chat + SWE sustained r=2000/1000 mooncake)
- ✅ K8s YAML drafts for AGG1 TP8 chat c=12 + SWE c=10
- 🔄 mns/mbt/block-size tuning in flight (Round 5)
- ⏳ Cross-validation on K8s STG (not yet attempted — needs Dynamo Operator + H200 cluster)
- ⏳ Optional 1P+1D `disagg/` YAML

YAML files are authored from sweep evidence and follow Sungsoo's recipe convention. Pending K8s STG deployment validation before MR-ing into the `dynamo` repo.
