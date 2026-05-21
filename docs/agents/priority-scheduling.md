---
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
title: Priority Scheduling
subtitle: Request priority across the Dynamo router and backend engines
---

Priority scheduling lets a client mark one request as more important than
another. In Dynamo, the user-facing request field is
`nvext.agent_hints.priority`.

Higher values mean higher priority at the Dynamo API layer. Clients should send
the intended Dynamo value directly and should not invert the value for a
specific backend. Dynamo normalizes backend-specific priority conventions before
forwarding the request to the engine.

```json
{
    "model": "my-model",
    "messages": [
        { "role": "user", "content": "Summarize this incident." }
    ],
    "nvext": {
        "agent_hints": {
            "priority": 10
        }
    }
}
```

## Priority Layers

Priority can affect three different layers. They are configured separately.

| Layer | What It Controls | Required Configuration |
|-------|------------------|------------------------|
| Router queue | Which waiting request is dispatched first when the router queue is non-empty. | KV routing plus `--router-queue-threshold` set to a value that actually causes queueing. |
| Backend engine | Which admitted request the engine schedules first. | Backend-specific priority scheduling flag, such as vLLM `--scheduling-policy priority` or SGLang `--enable-priority-scheduling`. |
| KV cache policy | Which cached blocks are retained or evicted first under memory pressure. | Backend-specific cache priority configuration, such as SGLang `--radix-eviction-policy priority`. |

These layers are additive. For example, a request can jump ahead in the router
queue but still use default engine scheduling if the backend priority flag is
not enabled.

## Router Queue Priority

The router queue only matters when requests are held before dispatch. If a
request can be routed immediately, there is no pending queue to reorder and the
priority hint will not change TTFT at the router layer.

`--router-queue-threshold` controls when the router starts holding requests. A
request waits in the router queue while every eligible worker is above the
configured threshold. The queue drains when capacity is available, and
higher-priority requests are selected before lower-priority requests according
to the configured `--router-queue-policy`.

The default policy is `fcfs`, which uses the priority value as a positive
arrival-time bump. Higher values move the request earlier in the queue. Negative
priority values are clamped to zero for router queueing, so a request cannot be
pushed behind normal first-come, first-served ordering by sending a negative
priority.

## Backend Engine Priority

The backend receives the same Dynamo semantic priority, but each engine has its
own native scheduling convention. Dynamo handles that conversion internally.

| Backend | Engine Scheduling Requirement | Dynamo Behavior |
|---------|-------------------------------|-----------------|
| vLLM | Start vLLM with `--scheduling-policy priority`. | Dynamo forwards the user priority with the polarity vLLM expects. |
| SGLang | Start SGLang with `--enable-priority-scheduling`. | Dynamo forwards higher Dynamo values as higher SGLang scheduling priority and rejects the inverted SGLang flag. |
| TensorRT-LLM | Per-request engine scheduling priority is not currently exposed through Dynamo. | Priority can still affect router queueing before dispatch. |

Do not negate `nvext.agent_hints.priority` in client code for vLLM. If a test
shows lower user values receiving better TTFT, first check whether the benchmark
harness or endpoint path inverted the value before it reached Dynamo.

## What Priority Does Not Do

Priority is not Kubernetes `PriorityClass`, GPU preemption, or a hard admission
control policy. It does not reserve capacity for high-priority requests.

Priority also does not show an effect unless there is contention at a layer that
uses it:

- Router priority needs a non-empty router queue.
- Engine priority needs backend priority scheduling enabled and engine-side
  queueing or preemption opportunities.
- Cache priority needs memory pressure and a priority-aware eviction policy.

## Related Docs

- [Agent Hints](agent-hints.md)
- [NVIDIA Request Extensions](../components/frontend/nvext.md#agent-hints)
- [Router Configuration and Tuning](../components/router/router-configuration.md)
- [SGLang for Agentic Workloads](../backends/sglang/agents.md)
