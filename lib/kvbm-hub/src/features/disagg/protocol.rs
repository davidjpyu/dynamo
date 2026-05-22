// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! HTTP protocol for the ConditionalDisagg feature namespace.

use serde::{Deserialize, Serialize};
use velo_ext::InstanceId;

/// Feature route prefix under `/v1/features`.
pub const ROUTE_PREFIX: &str = "disagg";

/// Velo queue name used by ConditionalDisagg prefill routing.
pub const CD_PREFILL_QUEUE: &str = crate::protocol::CD_PREFILL_QUEUE;

/// ConditionalDisagg route fragments.
pub mod paths {
    /// List prefill/decode instances.
    pub const INSTANCES: &str = "/instances";
}

/// Response body for `GET /v1/features/disagg/instances`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConditionalDisaggInstancesResponse {
    /// Instances registered as prefill workers.
    pub prefill: Vec<InstanceId>,
    /// Instances registered as decode workers.
    pub decode: Vec<InstanceId>,
}
