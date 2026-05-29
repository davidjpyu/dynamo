<!--
SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# Benchmark Evidence — Nemotron-3-Ultra H200 (vLLM Patch06+humming+tailfix)

Full sweep evidence backing the H200 vLLM recipe.

**Host:** viking-prod-214 (8× H200 SXM 143 GiB HBM3e)
**Image:** `nvcr.io/nvstaging/nim/sungsooh:nemotron-ultra-vllm-reasoning-api-validated-tailfix-20260528T070932Z@sha256:bfa2d02fd0dd1daab3fd41e4f2acfd8b131c44b49f0a4282937b5716e04fc265`
**Checkpoint:** `nvidia/Nemotron-Ultra-V3-rl3-050826-mixed_nvfp4-fp8_amax_1024x65k` (SHA `469ed01`)
**Spec decode:** `--spec-method nemotron_h_mtp --spec-tokens 1`
**Workload:** Mooncake traces from NGC `nvstaging/nim/nim_turbo_traces`, filtered against `max_model_len=65536`

## Methodology

For each (topology × concurrency × server config) combo, ran AIPerf 0.8.0 `profile` with native Mooncake trace loader (`--custom-dataset-type mooncake_trace`, sequential dataset sampling, `--concurrency` / `--num-requests` / `--use-server-token-count`). Per-server warmup pass (c=4 r=10) before any measured point.

**Sustained r values:**
- chat: r=2000 (~25% of 10,237 filtered trace records — enough to amortize JIT warmup and ISL distribution head bias)
- SWE: r=1000 (~10% of 11,201 filtered records — long-ISL requests slow per-request, r=1000 sufficient)

**SLA:** `TPS/user ≥ 50`. Sweet spot = highest c that passes SLA.

## Headline Numbers

| Workload | sweet c | TPS/GPU | TPS/user | Per-node TPS | TTFT avg | ITL avg |
|---|---:|---:|---:|---:|---:|---:|
| Chat (8K/1K @70%) | 12 | 84.0 | 56.0 | 672 | 713 ms | 17.6 ms |
| SWE (64K/400 @90%) | 10 | 63.0 | 50.4 | 504 | 570 ms | 18.2 ms |

vs Sungsoo B200 (NVIDIA-internal reference): chat 42%, SWE 65% of B200 per-node throughput. H200 PRD-compliant but below per-GPU parity due to sm90 lacking native NVFP4 MoE kernels (Triton/JIT path slower than sm100 CUTLASS).

## Topology Sweep — All Topologies × Workloads (r=2000 sustained)

### Chat (8K ISL / 1K OSL @70% cache)

| Topology | GPUs | sweet c | TPS/GPU | TPS/user | Per-node TPS | TTFT | ITL |
|---|---:|---:|---:|---:|---:|---:|---:|
| AGG1 TP4 | 4 | 6 | 78.5 | 52.3 | 314 | 1165 ms | 17.8 ms |
| AGG2 TP4 | 8 | 12 | 78.2 | 52.1 | 626 | 1263 ms | 18.2 ms |
| 1P+1D TP4 | 8 | 12 | 81.7 | 54.5 | 653 | 4683 ms | 12.1 ms |
| **AGG1 TP8** ★ | 8 | 12 | **84.0** | 56.0 | **672** | 713 ms | 17.6 ms |

**Observations:**
- All 4 topologies converge to **78–84 TPS/GPU** at the SLA-gated sweet spot (within ~7%). This is the H200 hardware ceiling for chat at the 50 TPS/user SLA — TPS/user × c / GPU_count ≈ same ratio for all viable configs.
- AGG1 TP8 marginal winner (+3% over 1P+1D, +7% over AGG2 TP4).
- 1P+1D has lowest ITL (12.1 ms) but highest TTFT (4683 ms) — P/D specialization helps decode at cost of prefill queue.

### SWE (64K ISL / 400 OSL @90% cache)

| Topology | GPUs | sweet c | TPS/GPU | TPS/user | Per-node TPS | TTFT | ITL |
|---|---:|---:|---:|---:|---:|---:|---:|
| AGG2 TP4 | 8 | 6 (cap) | 44.6 | 59.5 | 357 | 1131 ms | 13.6 ms |
| 1P+1D TP4 | 8 | 6 | 41.9 | 55.8 | 335 | 2874 ms | 9.7 ms |
| **AGG1 TP8** ★ | 8 | **10** | **63.0** | 50.4 | **504** | 570 ms | 18.2 ms |

**Observations:**
- TP8 advantage **much larger on SWE** (~50% better than TP4 topologies) — wider TP helps prefill-heavy 64K workload more than chat.
- 1P+1D ITL is lowest again (9.7 ms decode) but TPS/GPU lowest (41.9) — P/D's prefill bottleneck dominates SWE workload.
- AGG2 TP4 sweep cap was c=6 (sweep param), c=8 might still pass SLA but TP8 wins regardless.

## Chat AGG1 TP8 — Concurrency Curve

Full sustained r=2000 chat measurements at the winning topology:

| c | TPS/GPU | TPS/user | TTFT avg | ITL avg | SLA |
|---:|---:|---:|---:|---:|:---:|
| 8 | 68.9 | 68.9 | 743 ms | 13.6 ms | ✅ +38% margin |
| **12** | **84.0** | **56.0** | **713 ms** | **17.6 ms** | ✅ **sweet (+12%)** |
| 16 | 93.1 | 46.5 | 741 ms | 21.1 ms | ❌ below SLA |

If TPS/user floor is relaxed below 50, c can push higher (c=16 gives 93.1 TPS/GPU but 46.5 TPS/user). For strict 50 SLA, c=12 is the maximum.

For latency-priority deployments, choose c=8 (faster ITL, more TPS/user headroom, 551 per-node TPS). For throughput-priority c=12.

## SWE AGG1 TP8 — Concurrency Curve

| c | TPS/GPU | TPS/user | TTFT avg | ITL avg | SLA |
|---:|---:|---:|---:|---:|:---:|
| 4 | 43.7 | 87.5 | 529 ms | 9.9 ms | ✅ +75% margin |
| 6 | 51.4 | 68.5 | 523 ms | 13.1 ms | ✅ +37% margin |
| 8 | 59.5 | 59.5 | 559 ms | 15.3 ms | ✅ +19% margin |
| **10** | **63.0** | **50.4** | **570 ms** | **18.2 ms** | ✅ **sweet (+0.8% — tight)** |
| 12 | 67.9 | 45.3 | 608 ms | 20.3 ms | ❌ below SLA |

**SWE c=10 is at SLA edge.** For deployments where latency variability matters, c=8 (59.5 TPS/user, +19% margin) gives substantially more safety headroom at the cost of 5% TPS/GPU.

## Server-Param Sensitivity (mns / mbt / block-size)

Focused tuning sweep on AGG1 TP8 chat + SWE workloads at r=1000 sustained, with JIT warmup per server config.

### Chat (AGG1 TP8 r=1000)

| Config | mns | mbt | block | c | TPS/GPU | TPS/user | SLA |
|---|---:|---:|---:|---:|---:|---:|:---:|
| baseline (r=2000) | 256 | 32768 | 64 | 12 | 84.0 | 56.0 | ✅ |
| C1 | 24 | 32768 | 64 | 12 | 79.6 | 53.1 | ✅ |
| C1 | 24 | 32768 | 64 | 14 | 89.3 | 51.0 | ✅ just |
| C2 | 24 | 65536 | 64 | 12 | 78.5 | 52.3 | ✅ |
| C2 | 24 | 65536 | 64 | 14 | 86.9 | 49.7 | ❌ |
| C3 | 28 | 65536 | 32 | 14 | 81.1 | 46.4 | ❌ |
| C3 | 28 | 65536 | 32 | 16 | 92.3 | 46.2 | ❌ |
| sanity | 72 | 32768 | 64 | 12 | 79.7 | 53.1 | ✅ |

**Chat findings:**
- `block-size=32` never helps (C3 6% worse than C2).
- `max-batched-tokens=65536` shows no benefit over 32768.
- `max-num-seqs` between 24 and 72 produces identical chat results (79.6 vs 79.7 at c=12 r=1000).
- `mns=256` gives ~5% higher TPS/GPU than mns=72/24 (84.0 vs 79.7). Part is r=2000 vs r=1000 sampling, part is real mns effect.

### SWE (AGG1 TP8 r=1000)

| Config | mns | mbt | block | c | TPS/GPU | TPS/user | SLA |
|---|---:|---:|---:|---:|---:|---:|:---:|
| baseline | 256 | 32768 | 64 | 10 | 63.0 | 50.4 | ✅ |
| S1 | 20 | 32768 | 64 | 10 | 61.8 | 49.4 | ❌ |
| S1 | 20 | 32768 | 64 | 12 | 67.3 | 44.9 | ❌ |
| S2 | 20 | 65536 | 64 | 10 | 60.6 | 48.5 | ❌ |
| S2 | 20 | 65536 | 64 | 12 | 65.9 | 43.9 | ❌ |
| S3 | 20 | 65536 | 32 | 10 | 60.2 | 48.2 | ❌ |
| S3 | 20 | 65536 | 32 | 12 | 65.3 | 43.5 | ❌ |
| sanity | 64 | 32768 | 64 | 10 | 62.2 | 49.7 | ❌ |

**SWE findings:**
- **`max-num-seqs=20` always fails SLA** for c=10 (49.4 TPS/user vs 50 floor) — 5 different (mns=20, mbt, block) combinations all fail.
- Even `mns=64` fails by 0.3 TPS/user at c=10 (49.7).
- Only `mns=256` passes SLA at c=10 (50.4 — margin 0.4).
- Trend is monotonic: higher mns → higher TPS/user → can pass SLA at SWE c=10.
- For production deployment, **mns=256 is required** at SWE c=10 to pass SLA. Alternatively drop to c=8 with any reasonable mns for safety margin (+19%).

## Cache / Prefix Reuse Evidence

vLLM prefix caching enabled (`--enable-prefix-caching`) on all configurations. Per-request cached token statistics from `dynamo_frontend_cached_tokens` metric:

| Workload (sustained run) | Cached tokens avg/req | ISL avg | Implied hit rate | PRD trace design target |
|---|---:|---:|---:|---:|
| Chat c=12 r=2000 | 8,658 | ~12.9K | **~67%** | 70% |
| SWE c=10 r=1000 | 13,084 | ~14.1K | **~93%** | 90% |

**Both workloads achieve the trace's design cache hit rate**, validating that prefix caching is working and KV reuse is happening at the expected rate.

Note: `dynamo_component_router_kv_hit_rate` is 0 because all AGG runs used `--router-mode round-robin` (KV-aware routing only fires in `--router-mode kv` with multiple workers).

## Why r=200 (Initial Sweeps) Were Inflated

Initial concurrency sweep used `r=200` (first 200 trace records). This produced numbers later shown to be **biased high by 1.5–1.9×** at high c. Bias sources:

1. **Trace head sampling**: First 200 records have shorter ISL than full distribution. At high c, queue depth amplifies per-request latency for long-ISL records.
2. **Hot Triton JIT cache**: Original sweep ran c=8 → c=16 → c=32 in sequence on same server. By c=32, JIT cache was warm. Fresh-server cold-start at c=32 paid JIT overhead.

| | r=200 (initial) | r=2000 (sustained) | Bias factor |
|---|---:|---:|---:|
| Chat c=8 | 68.2 TPS/GPU | 68.9 TPS/GPU | **None at low c** |
| Chat c=16 | 139.6 | 93.1 | 1.5× inflated |
| Chat c=32 | 200.9 | 105.5 | **1.9× inflated** |
| SWE c=4 | 44.7 | 43.7 | None |
| SWE c=8 | 80.8 | 59.5 | 1.36× inflated |
| SWE c=16 | 108.8 | 72.8 | 1.5× inflated |

Sustained r=2000 (chat) / r=1000 (SWE) is the proper PRD evidence regime. Numbers in this document are sustained values.

## Bare-Metal Direct-Docker Reproducibility

For local validation without K8s, use direct `docker run` with these flags. Equivalent to the YAML deployment config in `agg1tp8/deploy-chat-c12.yaml`:

```bash
docker run -d --name nemotron-ultra-h200-chat \
  --gpus all --network host --ipc=host --shm-size 64g \
  --ulimit memlock=-1 --ulimit stack=67108864 --user 0:0 \
  -e HF_HOME=/hf-cache -e HF_HUB_CACHE=/hf-cache/hub \
  -e HF_HUB_OFFLINE=1 -e TRANSFORMERS_OFFLINE=1 \
  -e VLLM_ALLOW_CHUNKED_LOCAL_ATTN_WITH_HYBRID_KV_CACHE=1 \
  -v "$HF_CACHE_ROOT:/hf-cache:ro" \
  --entrypoint /bin/bash \
  nvcr.io/nvstaging/nim/sungsooh:nemotron-ultra-vllm-reasoning-api-validated-tailfix-20260528T070932Z \
  -c "sleep infinity"

# Inside container:
docker exec -d <name> bash -c "
python3 -m dynamo.frontend --discovery-backend file --router-mode round-robin \
  --http-host 0.0.0.0 --http-port 18750 > /tmp/frontend.log 2>&1
"

docker exec -d <name> bash -c "
DYN_DISCOVERY_BACKEND=file DYN_FILE_KV=/tmp/dyn_kv \
CUDA_VISIBLE_DEVICES=0,1,2,3,4,5,6,7 \
python3 -m dynamo.vllm --discovery-backend file \
  --model /hf-cache/patched/nemotron-ultra-ea-trtllm-tokenizer-patch-... \
  --served-model-name nemotron-ultra-ea \
  --tensor-parallel-size 8 --trust-remote-code \
  --max-model-len 65536 --max-num-seqs 256 --max-num-batched-tokens 32768 \
  --gpu-memory-utilization 0.85 --block-size 64 \
  --enable-prefix-caching --mamba-cache-mode align \
  --spec-method nemotron_h_mtp --spec-tokens 1 \
  --dyn-tool-call-parser qwen3_coder --dyn-reasoning-parser nemotron3 \
  --reasoning-parser-plugin /hf-cache/patched/.../ultra_v3_reasoning_parser.py \
  --reasoning-parser nemotron_v3 > /tmp/worker.log 2>&1
"
```

## AIPerf Command (for reproducing benchmark)

Direct AIPerf invocation against the running server (chat c=12 sweet, r=1000):

```bash
docker run --rm --network host --user $(id -u):$(id -g) \
  -v "$HF_CACHE_ROOT:/hf-cache:ro" \
  -v "$PREP_ROOT:/prep:ro" \
  -v "$RUN_ROOT:/artifacts" \
  -e HOME=/tmp -e HF_HOME=/hf-cache \
  -e HF_HUB_OFFLINE=1 -e TRANSFORMERS_OFFLINE=1 -e HF_DATASETS_OFFLINE=1 \
  --entrypoint aiperf \
  nvcr.io/nvidia/ai-dynamo/aiperf:0.8.0 \
  profile -m nemotron-ultra-ea -u http://127.0.0.1:18750 \
    --endpoint v1/chat/completions --endpoint-type chat --streaming \
    --input-file /prep/filtered_traces/chat_maxlen65536_oslcap1024_margin512.jsonl \
    --custom-dataset-type mooncake_trace --dataset-sampling-strategy sequential \
    --concurrency 12 --workers-max 12 --num-requests 1000 \
    --warmup-request-count 4 --warmup-concurrency 4 \
    --prompt-input-tokens-block-size 512 \
    --synthesis-max-isl 64000 --synthesis-max-osl 1024 \
    --tokenizer /hf-cache/patched/nemotron-ultra-ea-trtllm-tokenizer-patch-... \
    --tokenizer-trust-remote-code \
    --extra-inputs ignore_eos:true \
    --use-server-token-count --random-seed 42 \
    --export-level records --ui-type none \
    --artifact-dir /artifacts/aiperf/chat_c12_r1000
```

For SWE, change `--input-file` to `swe_maxlen65536_oslcap400_margin512.jsonl`, `--synthesis-max-osl 400`, `--concurrency 10 --workers-max 10`, `--num-requests 1000`.

**Important AIPerf flag**: `CACHE_SALT_MODE=omit` is required because Dynamo frontend rejects the `cache_salt` extra-input (HTTP 400). Omit means rely on fresh server / cache reset between runs instead of cache salt namespacing.

## Reference

- B200 / Sungsoo's recipe: https://github.com/sungsooha/dynamo/tree/nemotron-ultra-recipe-draft/recipes/turbo-recipes/nemotron-3-ultra/vllm
- Mooncake traces: NGC `nvstaging/nim/nim_turbo_traces` (`nim_turbo_8k_1k_70kv_chat_new_noschedule.jsonl`, `nim_turbo_64k_400_90kv_agent_new_noschedule.jsonl`)
- AIPerf: `nvcr.io/nvidia/ai-dynamo/aiperf:0.8.0`
