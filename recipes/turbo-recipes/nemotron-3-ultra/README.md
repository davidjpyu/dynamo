<!--
SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# Nemotron-3-Ultra Turbo Recipe — H200 Lane

This directory contains the **H200 lane** of the Nemotron-3-Ultra Turbo NIM recipe. It is the H200-specific sibling to Sungsoo Ha's B200 work and ships **only the vLLM backend** because TRT-LLM and SGLang are H200-blocked (see below).

## Backends Available

| Backend | H200 | Path | Notes |
|---|:---:|---|---|
| **vLLM** | ✅ | [`vllm-h200/`](vllm-h200/) | Patch06+humming via Triton/JIT, works on sm90 |
| TRT-LLM | ❌ blocked | — | `trtllmGen` NVFP4 MoE GEMM kernel is sm100-only |
| SGLang | ❌ blocked | — | `ModelOptMixedPrecisionConfig.get_min_capability() = 100` hard-coded sm100 |

## Why No TRT-LLM Recipe for H200

TRT-LLM (1.3.0rc15 via PR #14060 + stock 1.1.1, both tested) **cannot load Nemotron Ultra on H200**. Model load fails at engine init with:

```
[TensorRT-LLM][ERROR] Assertion failed: No kernel found for the given options:
  mDtypeA: E2m1, mDtypeB: E2m1, mDtypeC: E2m1,
  mUseDeepSeekFp8: 0,
  mRouteAct: 1, mFusedAct: 0,
  mIsStaticBatch: 0, mTileSize: 8
```

This is the routed NVFP4 (`E2m1`) MoE GEMM kernel from `FP4BlockScaleMoeRunner`. The entire `trtllmGen` NVFP4 MoE kernel family is registered **only for sm100 (Blackwell)** in current TRT-LLM source. Rebuilding with `CUDA_ARCHS=90-real` produces empty no-op binaries for these kernels because the C++/CUDA implementations don't have sm90 codepaths.

**Verification:** the same assertion fires with both:
- Derived image with PR #14060 (`9c9cde29...` SHA)
- Stock `nvcr.io/nvidia/ai-dynamo/tensorrtllm-runtime:1.1.1`

Confirms the gap is in TRT-LLM itself, not in PR #14060.

**Unblock requires** upstream TRT-LLM to add sm90 kernels for the routed NVFP4 MoE family (or accept a different MoE backend on sm90).

## Why No SGLang Recipe for H200

SGLang (commit `dbac4647` and `acb8310`) **cannot load Nemotron Ultra on H200** because of a hard-coded capability gate:

```python
# sglang/srt/layers/quantization/modelopt_quant.py
class ModelOptMixedPrecisionConfig(ModelOptQuantConfig):
    def get_min_capability(cls) -> int:
        return ModelOptFp4Config.get_min_capability()

class ModelOptFp4Config(ModelOptQuantConfig):
    def get_min_capability(cls) -> int:
        return 100  # hard-coded sm100
```

The model checkpoint is `modelopt_mixed` (NVFP4 + FP8). SGLang's quantization config rejects any GPU with `compute_capability < 100`. H200 is sm90 (capability `(9, 0)`), so SGLang refuses to start.

```
ValueError: ... Minimum capability: 100. Current capability: 90.
```

**Unblock requires** SGLang to either lower the capability minimum or add a sm90 NVFP4 path (currently only Blackwell is supported by their NVFP4 MoE code).

## Why vLLM Works on H200

vLLM (Patch06+humming via Triton/JIT) **does work** on H200. The reasons:

1. **vLLM's NVFP4 MoE path is built on Triton**, which JIT-compiles kernels at runtime for the target compute capability. Triton can emit sm90 PTX from the same kernel source it uses for sm100. No hard sm100 dependency.
2. **`humming-kernels[cu13]==0.1.0`** (the additional kernel package included in Patch06+humming) also uses Triton internally, so it works cross-arch.
3. **Speculative decoding via Nemotron-H MTP head** (`--spec-method nemotron_h_mtp --spec-tokens 1`) — the eagle/MTP kernels (`eagle_prepare_next_token_padded_kernel`, `rejection_greedy_sample_kernel`, etc.) all JIT-compile on sm90 successfully.

**Tradeoff:** Triton/JIT path is slower per-kernel than B200's native sm100 CUTLASS NVFP4 MoE kernels. H200 compensates by using wider TP (TP=8 instead of B200's TP=4) to aggregate more HBM bandwidth and KV capacity.

## Topology Decision

For both PRD workloads, **AGG1 TP=8 single worker** is the winning H200 topology:

| Workload | Topology | TP | GPUs | Sweet c | TPS/GPU | TPS/user |
|---|---|---:|---:|---:|---:|---:|
| Chat (8K/1K @70%) | AGG1 | 8 | 8 | 12 | 84.0 | 56.0 |
| SWE (64K/400 @90%) | AGG1 | 8 | 8 | 10 | 63.0 | 50.4 |

This differs from Sungsoo's B200 recipe (chat AGG1 TP4 c=68 / SWE AGG2 TP4 c=32) because:
- sm90 needs wider TP to aggregate HBM bandwidth and compensate for slower per-kernel performance
- H200 has tighter HBM (143 GB vs B200 192 GB), so TP8 (41 GB weights/GPU) leaves enough KV headroom

## Per-Node Throughput vs B200

| Workload | H200 (this lane) | B200 (Sungsoo) | H200 % of B200 |
|---|---:|---:|---:|
| Chat | 672 TPS | ~1616 TPS | **42%** |
| SWE | 504 TPS | 770 TPS | **65%** |

H200 is PRD-compliant on both workloads (≥50 TPS/user) but per-GPU efficiency is below B200. H200 is a viable production lane, **not at parity with B200**.

## Layout

```
nemotron-3-ultra/
├── README.md                  # This file — H200 lane overview + blockers
└── vllm-h200/
    ├── README.md              # vLLM-specific recipe details
    ├── agg1tp8/
    │   ├── deploy-chat-c12.yaml   # K8s DGD for chat sweet spot
    │   └── deploy-swe-c10.yaml    # K8s DGD for SWE sweet spot
    ├── aiperf/                # AIPerf launchers (TBD)
    ├── patches/               # Patch references (uses Sungsoo's image)
    └── scripts/               # Bare-metal launch harness (TBD)
```

## Evidence

Full sweep evidence is at:
- [H200_LANE_FINAL_REPORT.md](../../../../work-tracker/nim/nim-turbo-dynamo/nemotron-ultra/docs/H200_LANE_FINAL_REPORT.md) (internal)
- [H200_RECIPE_SWEEP.md](../../../../work-tracker/nim/nim-turbo-dynamo/nemotron-ultra/docs/H200_RECIPE_SWEEP.md) (internal)
- TRT-LLM blocker: [TRTLLM_NVFP4_MOE_SM90_BLOCKER.md](../../../../work-tracker/nim/nim-turbo-dynamo/nemotron-ultra/docs/TRTLLM_NVFP4_MOE_SM90_BLOCKER.md) (internal)
- SGLang blocker: [SGLANG_MODELOPT_MIXED_SM90_BLOCKER.md](../../../../work-tracker/nim/nim-turbo-dynamo/nemotron-ultra/docs/SGLANG_MODELOPT_MIXED_SM90_BLOCKER.md) (internal)

## See Also

- B200 / cross-platform recipe (separate branch): https://github.com/sungsooha/dynamo/tree/nemotron-ultra-recipe-draft/recipes/turbo-recipes/nemotron-3-ultra
