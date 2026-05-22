#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""HTTP shim from KVBM hub prefill dispatch to a Dynamo prefill endpoint.

The current hub dispatcher posts tokenized prefill work to `/v1/completions`.
For Experiment E we still want the prefill execution to go through
`python -m dynamo.vllm`, whose prefill worker registers a Dynamo internal
`ModelType.Prefill` endpoint rather than an OpenAI HTTP completions route.
This shim preserves the hub's HTTP contract and forwards accepted requests to
the configured Dynamo prefill endpoint as `PreprocessedRequest` dictionaries.
The counted KVBM+hub experiment path still requires Dynamo vLLM to preserve
caller-supplied `kv_transfer_params` through the prefill worker.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
import traceback
from typing import Any

from dynamo.runtime import DistributedRuntime, dynamo_worker
from dynamo.runtime.logging import configure_dynamo_logging

configure_dynamo_logging()


def _json_response(status: int, payload: dict[str, Any]) -> bytes:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    reason = {
        200: "OK",
        400: "Bad Request",
        404: "Not Found",
        500: "Internal Server Error",
    }.get(status, "OK")
    headers = [
        f"HTTP/1.1 {status} {reason}",
        "content-type: application/json",
        f"content-length: {len(body)}",
        "connection: close",
        "",
        "",
    ]
    return "\r\n".join(headers).encode("ascii") + body


async def _read_http_request(reader: asyncio.StreamReader) -> tuple[str, str, bytes]:
    header_blob = await reader.readuntil(b"\r\n\r\n")
    header_text = header_blob.decode("iso-8859-1")
    lines = header_text.split("\r\n")
    method, path, _version = lines[0].split(" ", 2)
    content_length = 0
    for line in lines[1:]:
        if not line:
            continue
        name, _, value = line.partition(":")
        if name.lower() == "content-length":
            content_length = int(value.strip())
    body = await reader.readexactly(content_length) if content_length else b""
    return method.upper(), path, body


def _payload_to_preprocessed(payload: dict[str, Any], model: str) -> dict[str, Any]:
    prompt = payload.get("prompt")
    if not isinstance(prompt, list) or not all(isinstance(t, int) for t in prompt):
        raise ValueError("prompt must be a list of token ids")

    extra_args = {}
    if "kv_transfer_params" in payload:
        extra_args["kv_transfer_params"] = payload["kv_transfer_params"]

    max_tokens = payload.get("max_tokens", 1)
    if not isinstance(max_tokens, int) or max_tokens < 1:
        max_tokens = 1

    return {
        "model": payload.get("model") or model,
        "token_ids": prompt,
        "stop_conditions": {"max_tokens": max_tokens, "min_tokens": 1},
        "sampling_options": {
            "temperature": payload.get("temperature", 0),
        },
        "output_options": {},
        "eos_token_ids": [],
        "annotations": [],
        "extra_args": extra_args or None,
    }


async def _collect_prefill(client: Any, request: dict[str, Any]) -> list[Any]:
    items: list[Any] = []
    stream = await client.generate(request)
    async for item in stream:
        if hasattr(item, "is_error") and item.is_error():
            comments = item.comments() if hasattr(item, "comments") else []
            raise RuntimeError("; ".join(comments) or "Dynamo prefill returned error")
        data = item.data() if hasattr(item, "data") else item
        if data is not None:
            items.append(data)
    return items


async def _serve(runtime: DistributedRuntime) -> None:
    model = os.environ["MODEL"]
    endpoint_path = os.environ["KVBM_HUB_PREFILL_ENDPOINT"]
    host = os.environ.get("KVBM_HUB_PREFILL_SHIM_HOST", "127.0.0.1")
    port = int(os.environ.get("KVBM_HUB_PREFILL_SHIM_PORT", "8001"))

    endpoint = runtime.endpoint(endpoint_path)
    client = await endpoint.client()
    await client.wait_for_instances()

    async def handle(
        reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        try:
            method, path, body = await _read_http_request(reader)
            if method == "GET" and path == "/v1/models":
                response = _json_response(
                    200,
                    {
                        "object": "list",
                        "data": [
                            {
                                "id": model,
                                "object": "model",
                                "created": int(time.time()),
                                "owned_by": "nvidia",
                            }
                        ],
                    },
                )
            elif method == "POST" and path == "/v1/completions":
                payload = json.loads(body.decode("utf-8"))
                preprocessed = _payload_to_preprocessed(payload, model)
                outputs = await _collect_prefill(client, preprocessed)
                response = _json_response(
                    200,
                    {
                        "id": f"cmpl-prefill-{int(time.time() * 1000)}",
                        "object": "text_completion",
                        "model": model,
                        "choices": [{"index": 0, "text": "", "finish_reason": "stop"}],
                        "dynamo_prefill_output_count": len(outputs),
                    },
                )
            else:
                response = _json_response(404, {"error": "not found"})
        except Exception as exc:  # noqa: BLE001 - return failure to the hub.
            traceback.print_exc()
            response = _json_response(500, {"error": str(exc)})

        writer.write(response)
        await writer.drain()
        writer.close()
        await writer.wait_closed()

    server = await asyncio.start_server(handle, host, port)
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets or [])
    print(
        f"[prefill-shim] serving /v1/completions for {endpoint_path} on {addrs}",
        flush=True,
    )
    async with server:
        await server.serve_forever()


@dynamo_worker()
async def worker(runtime: DistributedRuntime) -> None:
    await _serve(runtime)


if __name__ == "__main__":
    asyncio.run(worker())
