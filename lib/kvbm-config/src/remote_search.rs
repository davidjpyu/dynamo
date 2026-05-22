// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Remote-search configuration for hub-backed KVBM leaders.

use serde::{Deserialize, Serialize};
use validator::Validate;

/// Enables remote KV lookup through the hub-side indexer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate, PartialEq, Eq)]
pub struct RemoteSearch {
    /// Minimum number of remote tokens before attempting a remote search.
    #[serde(default)]
    pub min_remote_tokens: usize,
}
