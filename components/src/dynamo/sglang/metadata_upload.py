# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass, field
from typing import Any


def _upload_url_from_request(request: dict[str, Any]) -> str | None:
    scopes = [request.get("nvext")]
    extra_args = request.get("extra_args")
    if isinstance(extra_args, dict):
        scopes.append(extra_args.get("nvext"))

    for scope in scopes:
        if not isinstance(scope, dict):
            continue
        candidate = scope.get("metadata_upload")
        if isinstance(candidate, dict):
            url = candidate.get("url")
            if isinstance(url, str) and url.strip():
                return url.strip()
    return None


async def _upload_bytes(url: str, storage_path: str, data: bytes) -> str:
    try:
        from dynamo.common.storage import get_fs, upload_to_fs
    except ImportError as exc:
        raise RuntimeError(
            "SGLang metadata upload requires fsspec support. "
            "Install fsspec and the backend extra, for example `fsspec[s3]` for S3."
        ) from exc

    return await upload_to_fs(get_fs(url), storage_path, data)


def _serialize_zstd_json(payload: dict[str, Any]) -> bytes:
    try:
        import zstandard as zstd
    except ImportError as exc:
        raise RuntimeError(
            "SGLang metadata upload requires zstandard. "
            "Install ai-dynamo[sglang] or add the zstandard package."
        ) from exc

    raw = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode(
        "utf-8"
    )
    try:
        return zstd.ZstdCompressor().compress(raw)
    finally:
        del raw


@dataclass
class ChoiceMetadata:
    choice_index: int
    log_probs: list[float] = field(default_factory=list)
    top_logprobs: list[list[dict[str, Any]]] = field(default_factory=list)
    routed_experts: Any = None

    def add_logprobs(
        self,
        log_probs: list[float] | None,
        top_logprobs: list[list[dict[str, Any]]] | None,
    ) -> None:
        if log_probs:
            self.log_probs.extend(log_probs)
        if top_logprobs:
            self.top_logprobs.extend(top_logprobs)

    def has_payload(self) -> bool:
        return bool(
            self.log_probs or self.top_logprobs or self.routed_experts is not None
        )

    def release_payload(self) -> None:
        self.log_probs.clear()
        self.top_logprobs.clear()
        self.routed_experts = None

    def to_payload(self) -> dict[str, Any]:
        metadata: dict[str, Any] = {}
        if self.log_probs:
            metadata["log_probs"] = self.log_probs
        if self.top_logprobs:
            metadata["top_logprobs"] = self.top_logprobs
        if self.routed_experts is not None:
            metadata["routed_experts"] = self.routed_experts

        return {
            "schema_version": 1,
            "metadata": metadata,
        }


@dataclass(frozen=True)
class MetadataUploader:
    url: str

    @classmethod
    def from_request(cls, request: dict[str, Any]) -> MetadataUploader | None:
        url = _upload_url_from_request(request)
        return cls(url=url) if url is not None else None

    async def upload_choice(self, choice: ChoiceMetadata) -> dict[str, Any] | None:
        if not choice.has_payload():
            return None

        storage_path = f"choice_{choice.choice_index}.json.zst"
        payload = choice.to_payload()
        data = await asyncio.to_thread(_serialize_zstd_json, payload)
        try:
            url = await _upload_bytes(self.url, storage_path, data)
        finally:
            del data
            del payload
        return {
            "url": url,
        }


def metadata_upload_requested(request: dict[str, Any]) -> bool:
    return _upload_url_from_request(request) is not None
