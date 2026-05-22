// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Velo client for hub-side KV index lookups.

use std::{fmt, sync::Arc};

use anyhow::{Context, Result};
use kvbm_logical::SequenceHash;
use velo::Messenger;
use velo_ext::InstanceId;

use super::protocol::{FindBlocksHit, QUERY_HANDLER, QueryRequest};

/// Typed client for the hub's Velo-plane index lookup handler.
#[derive(Clone)]
pub struct IndexerLookupClient {
    messenger: Arc<Messenger>,
    hub_instance_id: InstanceId,
}

impl fmt::Debug for IndexerLookupClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexerLookupClient")
            .field("hub_instance_id", &self.hub_instance_id)
            .field("messenger", &"<redacted>")
            .finish()
    }
}

impl IndexerLookupClient {
    /// Create a client targeting the hub's Velo instance.
    pub fn new(messenger: Arc<Messenger>, hub_instance_id: InstanceId) -> Arc<Self> {
        Arc::new(Self {
            messenger,
            hub_instance_id,
        })
    }

    /// Find the deepest indexed block among `hashes`.
    pub async fn find_blocks(&self, hashes: Vec<SequenceHash>) -> Result<Option<FindBlocksHit>> {
        let req = QueryRequest { hashes };
        let hit = self
            .messenger
            .typed_unary::<Option<FindBlocksHit>>(QUERY_HANDLER)
            .context("building indexer query RPC")?
            .payload(req)
            .context("encoding indexer query RPC")?
            .send_to(self.hub_instance_id)
            .await
            .context("indexer query RPC")?;
        Ok(hit)
    }
}
