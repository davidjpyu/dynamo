// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Velo handlers for the hub-side KV indexer.

use std::sync::Arc;

use velo::Handler;
use velo_ext::InstanceId;

use super::index::PositionalIndex;
use super::protocol::{FindBlocksHit, QUERY_HANDLER, QueryRequest};

/// Build the typed Velo lookup handler installed by [`IndexerManager`].
pub fn create_query_handler(index: Arc<PositionalIndex>) -> Handler {
    Handler::typed_unary_async::<QueryRequest, Option<FindBlocksHit>, _, _>(
        QUERY_HANDLER,
        move |ctx| {
            let index = Arc::clone(&index);
            async move {
                let hit = index
                    .query_holders(&ctx.input.hashes)
                    .map(|(matched, ids)| FindBlocksHit {
                        matched,
                        candidates: ids
                            .into_iter()
                            .map(|id| InstanceId::from(uuid::Uuid::from_u128(id)))
                            .collect(),
                    });
                Ok(hit)
            }
        },
    )
    .build()
}
