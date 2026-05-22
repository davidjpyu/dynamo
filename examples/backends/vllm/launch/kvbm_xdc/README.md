# KVBM Cross-Datacenter Harness

This directory contains topology-oriented run harnesses for KVBM
cross-datacenter experiments. Experiment letters are metadata, not script
names, and hardware assumptions live in profile/env variables rather than in
workflow names.

## Fixed Experiment E Contract

| Field | Value |
|---|---|
| Model | `deepseek-ai/DeepSeek-R1-Distill-Llama-8B` |
| Framework | Dynamo vLLM: `python -m dynamo.vllm` |
| User endpoint | `dynamo.frontend` OpenAI `/v1/chat/completions` |
| Endpoint type | `chat` |
| Streaming | `true` |
| Required E transfer path | KVBM + hub conditional-disagg |
| Recorded GPU class | H100 or A100 only |

Do not use `python -m vllm.entrypoints.openai.api_server` for the primary
Experiment E path. That is a raw vLLM diagnostic or legacy hub-CD smoke, not
the Dynamo vLLM framework under test. Experiment F is intentionally separate:
it validates `kvbm_hub` P2P transfer primitives between two raw vLLM KVBM
instances because the P2P mechanism under test lives below the Dynamo frontend
routing layer.

## Hardware Profiles

Hardware, model, sizing, and placement defaults live in
`hardware-profiles.sh`. Workflow scripts source that file and fail early if the
selected profile or caller-provided env does not define the values they need.

| Profile | Purpose | Model | Placement |
|---|---|---|---|
| `h100-a100` | Recorded Experiment E/F runs | `deepseek-ai/DeepSeek-R1-Distill-Llama-8B` | same-node `all`: decode GPU 0, prefill GPU 1; single-role workers default to local GPU 0 |
| `spark-gb10` | Local smoke/debug only | `Qwen/Qwen3-0.6B` | decode and prefill share GPU 0 |
| `custom` | Any other cluster allocation | caller-set | caller-set |

Use `KVBM_HARDWARE_PROFILE=h100-a100` or a fully specified `custom` profile for
results that feed Experiment E. `spark-gb10` keeps the workflow debuggable on
Spark, but those outputs are not comparable experiment data.

For a non-Spark cluster, keep the workflow script the same and provide the
profile explicitly:

```bash
KVBM_HARDWARE_PROFILE=custom \
MODEL=deepseek-ai/DeepSeek-R1-Distill-Llama-8B \
GPU_CLASS=H100 \
GPU_MEMORY_UTILIZATION=0.70 \
MAX_MODEL_LEN=2048 \
MAX_NUM_SEQS=8 \
CPU_CACHE_GB=16 \
DECODE_CUDA_VISIBLE_DEVICES=0 \
PREFILL_CUDA_VISIBLE_DEVICES=1 \
ARTIFACT_DIR=/tmp/kvbm-xdc/same-node \
bash run-dynamo-kvbm-chat-smoke.sh
```

For cross-node single-role workers, set the role's local device explicitly when
needed; otherwise the worker role defaults to GPU 0 on that allocation.

## Experiment Map

| Experiment | Primary question | Topology | Script |
|---|---|---|---|
| E baseline | What is TTFT on the same-node KVBM + hub path for this model? | `dynamo.frontend` + decode/prefill `dynamo.vllm` workers using KVBM conditional-disagg through `kvbm_hub` | First pass the R1/R2 smoke with `KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=all bash run-dynamo-kvbm-chat-smoke.sh`; the Experiment E data point counts only after the locked AIPerf pass |
| E cross-DC | What changes when the same model spans datacenters? | Same KVBM + hub path as baseline, with shared NATS/etcd and explicit routable side-channel hosts | Same script split by role; run only after the same-node E gate passes |
| Diagnostic | Does the existing Dynamo vLLM KVBM/NIXL composition still transfer and reuse KV? | `dynamo.frontend` + decode `dynamo.vllm` using `NixlConnector` + prefill `dynamo.vllm` using `PdConnector(DynamoConnector,NixlConnector)` | `KVBM_TRANSFER_TOPOLOGY=nixl-pd bash run-dynamo-kvbm-chat-smoke.sh`; useful for comparison, but not sufficient to complete E |
| F | Does the hub-controlled P2P G2 transfer path work and what trace does it produce? | `kvbm_hub` P2P transfer primitives between two KVBM instances | `.claude/skills/p2p-smoke/p2p-smoke.sh` |

Raw hub conditional-disagg scripts remain diagnostics until the same transfer
path is exercised through `dynamo.frontend` and `python -m dynamo.vllm`.

## Integration Dependency

The `kvbm-hub` path assumes lower-layer Dynamo vLLM support for preserving a
hub-provided `kv_transfer_params` payload through the prefill worker request.
If that support is absent, the hub prefill shim is only a topology bridge: the
run may still produce diagnostic logs, but it must not be counted as an
Experiment E smoke or AIPerf baseline/cross-datacenter result.

## Result Manifests

Do not hard-code run results into script names or public docs. Each smoke run
writes its fixed contract to `metadata.env` / `contract.env`, request timing to
`chat-ttft.json` or `r*-ttft.json`, and the useful-trace verdict to
`trace-gate.env`. The locked AIPerf pass writes `aiperf-contract.env` and
`aiperf-result.env`; when `BASELINE_AIPERF_JSON` or `BASELINE_METRICS_ENV` is
provided, it also writes `experiment-comparison.env`. Prefer
`BASELINE_AIPERF_JSON` when the original exported AIPerf profile is available.
Use `BASELINE_METRICS_ENV` only for explicitly provenance-labeled baseline
evidence, such as a key/value summary derived from a compute-session log. When
`POSTCHECK_LOG_DIR` or `ROLE_LOG_DIR` points at collected role logs, it writes
`aiperf-postcheck.env` with transfer and failure counters. If that directory
also contains `trace-gate.env`, the postcheck carries the trace verdict and
cache/failure counters forward into the AIPerf artifact. Compare same-node and
cross-datacenter runs only when both artifacts have `trace_useful=true`,
successful transfer evidence, positive external cache reuse, and zero KV load
failures.

## Usage

Candidate Experiment E same-node KVBM + hub path, H100/A100:

```bash
ARTIFACT_DIR=/tmp/kvbm-xdc/e-hub-same-node \
KVBM_EXPERIMENT_ID=E \
KVBM_TRANSFER_TOPOLOGY=kvbm-hub \
KVBM_HARDWARE_PROFILE=h100-a100 \
NODE_ROLE=all \
bash run-dynamo-kvbm-chat-smoke.sh
```

This exercises the API compatibility bridge where `kvbm_hub` dispatches a
vLLM-compatible prefill completion request with `kv_transfer_params` through an
internal Dynamo frontend. The user request still goes to the main
`dynamo.frontend` chat endpoint; the hub's internal prefill POST goes to
`KVBM_HUB_PREFILL_URL`, backed by a `dynamo.vllm` prefill worker registered as
an internal completions backend. Treat it as a candidate until
`trace-gate.env` contains both `trace_useful=true` and
`experiment_e_smoke_valid=true`; otherwise the run is diagnostic evidence for
the next integration fix. A useful smoke trace is not Experiment E completion:
the same-node baseline still requires the locked AIPerf pass against the
`dynamo.frontend` chat endpoint.

Current Dynamo KVBM/NIXL diagnostic, same-node H100/A100:

```bash
ARTIFACT_DIR=/tmp/kvbm-xdc/same-node \
KVBM_HARDWARE_PROFILE=h100-a100 \
NODE_ROLE=all \
bash run-dynamo-kvbm-chat-smoke.sh
```

The hub candidate or NIXL diagnostic can also be split across roles. The
example below uses the hub candidate; set `ETCD_ENDPOINTS`, `NATS_SERVER`, and
each worker's routable `VLLM_NIXL_SIDE_CHANNEL_HOST` through
`DECODE_SIDE_CHANNEL_HOST` or `PREFILL_SIDE_CHANNEL_HOST`.

```bash
# Frontend / runtime-infra node
ARTIFACT_DIR=/tmp/kvbm-xdc/frontend KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=frontend bash run-dynamo-kvbm-chat-smoke.sh

# Internal prefill frontend node
ARTIFACT_DIR=/tmp/kvbm-xdc/hub-prefill-frontend KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=hub-prefill-frontend FRONTEND_HOST=192.0.2.20 bash run-dynamo-kvbm-chat-smoke.sh

# Hub dispatcher node
ARTIFACT_DIR=/tmp/kvbm-xdc/hub KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=hub KVBM_HUB_PREFILL_URL=http://192.0.2.20:8001 bash run-dynamo-kvbm-chat-smoke.sh

# Decode node
ARTIFACT_DIR=/tmp/kvbm-xdc/decode KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=decode DECODE_SIDE_CHANNEL_HOST=192.0.2.21 bash run-dynamo-kvbm-chat-smoke.sh

# Prefill node
ARTIFACT_DIR=/tmp/kvbm-xdc/prefill KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=prefill KVBM_HUB_PREFILL_NAMESPACE=dynamo_hub_prefill PREFILL_SIDE_CHANNEL_HOST=192.0.2.31 bash run-dynamo-kvbm-chat-smoke.sh

# Client / measurement node
ARTIFACT_DIR=/tmp/kvbm-xdc/client KVBM_TRANSFER_TOPOLOGY=kvbm-hub NODE_ROLE=client FRONTEND_HOST=192.0.2.10 bash run-dynamo-kvbm-chat-smoke.sh
```

Locked AIPerf pass against the chat endpoint after a useful diagnostic run:

```bash
URL=http://192.0.2.10:8000 \
ARTIFACT_DIR=/tmp/kvbm-xdc/aiperf \
KVBM_HARDWARE_PROFILE=h100-a100 \
bash run-aiperf-locked.sh
```

To produce a comparison manifest in the same artifact directory, pass the
baseline AIPerf export explicitly:

```bash
URL=http://192.0.2.10:8000 \
ARTIFACT_DIR=/tmp/kvbm-xdc/aiperf-cross-dc \
BASELINE_AIPERF_JSON=/tmp/kvbm-xdc/aiperf-same-node/profile_export_aiperf.json \
POSTCHECK_LOG_DIR=/tmp/kvbm-xdc/collected-role-logs \
KVBM_HARDWARE_PROFILE=h100-a100 \
bash run-aiperf-locked.sh
```

If the original baseline export is not available, pass a provenance-labeled
metrics env file instead:

```bash
URL=http://192.0.2.10:8000 \
ARTIFACT_DIR=/tmp/kvbm-xdc/aiperf-cross-dc \
BASELINE_METRICS_ENV=/tmp/kvbm-xdc/baseline.metrics.env \
POSTCHECK_LOG_DIR=/tmp/kvbm-xdc/collected-role-logs \
KVBM_HARDWARE_PROFILE=h100-a100 \
bash run-aiperf-locked.sh
```

## Runtime Invariants

- Cross-node runs need shared runtime infra reachable from all nodes:
  `ETCD_ENDPOINTS=192.0.2.10:2379`, `NATS_SERVER=nats://192.0.2.10:4222`,
  `DYN_DISCOVERY_BACKEND=etcd`, `DYN_REQUEST_PLANE=tcp`, and
  `DYN_EVENT_PLANE=nats`.
- Set `VLLM_NIXL_SIDE_CHANNEL_HOST` explicitly on each worker to that node's
  routable address. Do not rely on autodetection for cross-datacenter runs.
- The current diagnostic harness uses `PdConnector` wrapping KVBM and NIXL.
  Worker startup validates the canonical KVBM module exports and the NIXL
  registry entry before launching `python -m dynamo.vllm`.
- Use `kvbm.v2.vllm.connector` for the diagnostic `PdConnector` module path.
  The legacy `kvbm.vllm_integration.connector` path remains a compatibility
  shim for existing examples and older configs.
- The diagnostic harness defaults metadata to `experiment=diagnostic`. Do not
  set `KVBM_EXPERIMENT_ID=E` until the run uses the KVBM + hub
  conditional-disagg transfer path and passes the useful-trace gate.
- Capture `chat-ttft.json`, logs, metadata, `trace.html`, and
  `trace-gate.env` for every run.
- Do not use `nvidia-smi` as a pass/fail signal. It is useful only for
  cleanup/visibility. The harness records `runtime-probe.json`, which imports
  Dynamo/KVBM/vLLM, checks `torch.cuda`, and runs a small CUDA operation on the
  requested decode/prefill devices before launch. A counted run still requires
  the streamed chat request and `trace-gate.env` to pass.
- For H100/A100 `kvbm-hub` host-cache runs, the harness also performs a
  transfer-substrate preflight. With `KVBM_EXPERIMENT_ID=E`, missing
  `nvidia_peermem` exits before launch with `trace_useful=false` and a
  `verdict.md`; use `KVBM_ALLOW_SUBSTRATE_FAILURE_ARTIFACT=1` only when
  intentionally collecting a non-benchmark failure artifact. Diagnostic runs
  warn and continue so the old raw KVBM path can still be reproduced while the
  substrate is being characterized.
- Treat `trace.html` as complete only when the harness prints `TRACE_USEFUL`.
  The gate accepts either a `kvbm_audit` transfer chain or Dynamo vLLM evidence
  that the routed chat request selected prefill and decode workers and emitted a
  successful `KV Transfer metrics` line. It also requires positive external
  cache reuse and zero KV load failures. A rendered timeout log, backend
  failure, or fallback-only timeline without successful transfer and cache
  evidence is not an Experiment E/F trace result. The persisted
  `trace-gate.env` must say `trace_useful=true` before the trace is counted.
  For Experiment E smoke, it must also say `experiment_e_smoke_valid=true`;
  `experiment_e_complete` remains `false` until the AIPerf comparison has been
  produced.
  Split-role cross-datacenter runs must collect frontend, decode, and prefill
  logs into the validation artifact before the trace gate can pass.
- Keep machine-specific placement in `KVBM_HARDWARE_PROFILE` or explicit env
  overrides, not in workflow script names.
- If startup takes the compile-heavy vLLM path, use:
  `VLLM_RUNNER=generate`, `VLLM_USE_AOT_COMPILE=0`,
  `VLLM_USE_STANDALONE_COMPILE=0`, and
  `VLLM_ENABLE_V1_MULTIPROCESSING=0`.
