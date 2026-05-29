# Patches Reference (H200 Lane)

The H200 lane uses the **same image** as Sungsoo's B200 work — `Patch06+humming+ds_copy+ssm_nixl_tailfix`. All patches from that image apply unchanged on H200 via Triton/JIT.

Image:
```
nvcr.io/nvstaging/nim/sungsooh:nemotron-ultra-vllm-reasoning-api-validated-tailfix-20260528T070932Z
@sha256:bfa2d02fd0dd1daab3fd41e4f2acfd8b131c44b49f0a4282937b5716e04fc265
```

Patches included by the image:
- `06_vllm_patch02_hash_block_event_port_after_pr42547.patch` — Patch06 hash-block KV event port (for PR #42547)
- `ds_copy_diag_installed_vllm.patch` — Ultra MTP DS-layout conv-tail copy
- `ssm_nixl_tailfix_installed_vllm.patch` — P/D SSM/NIXL tailfix

H200 lane requires **no additional patches** — humming-kernels[cu13]==0.1.0 uses Triton runtime JIT so it's cross-arch (sm90 + sm100). MTP eagle kernels (`eagle_prepare_*`, `rejection_greedy_sample_*`) also Triton-based and JIT-compile on sm90.

Reference: see Sungsoo's full patch documentation in B200 lane (separate branch).
