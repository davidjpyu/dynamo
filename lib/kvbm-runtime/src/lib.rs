// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Runtime construction helpers for KVBM.
//!
//! This crate holds constructors that require runtime-heavy dependencies such
//! as Velo and NIXL, keeping `kvbm-config` limited to serializable config
//! types.

use std::net::TcpListener;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use dynamo_memory::nixl::NixlBackendConfig;
use kvbm_config::{DiscoveryConfig, MessengerConfig, NixlConfig};
use velo::discovery::PeerDiscovery;
use velo::transports::tcp::TcpTransportBuilder;
use velo::transports::uds::UdsTransportBuilder;
use velo::{Messenger, Velo};

/// Build a [`Velo`] instance from messenger configuration.
pub async fn build_velo(config: &MessengerConfig) -> Result<Arc<Velo>> {
    build_velo_with_discovery(config, None).await
}

/// Build a [`Velo`] instance, optionally overriding peer discovery.
///
/// When `discovery_override` is `Some`, it takes precedence over the discovery
/// field in `config`. This allows callers such as the hub-backed connector path
/// to inject dynamic discovery without making `kvbm-config` depend on Velo.
pub async fn build_velo_with_discovery(
    config: &MessengerConfig,
    discovery_override: Option<Arc<dyn PeerDiscovery>>,
) -> Result<Arc<Velo>> {
    let bind_addr = config.backend.resolve_bind_addr()?;
    let listener = TcpListener::bind(bind_addr)
        .with_context(|| format!("Failed to bind TCP listener to {}", bind_addr))?;
    let actual_addr = listener
        .local_addr()
        .context("Failed to get local address from listener")?;
    tracing::info!("Built TCP transport bound to {}", actual_addr);

    let tcp_transport = TcpTransportBuilder::new()
        .from_listener(listener)?
        .build()
        .context("Failed to build TCP transport")?;
    let tcp_transport = Arc::new(tcp_transport);

    let mut builder = Velo::builder();

    if config.backend.uds_enabled {
        let dir = config
            .backend
            .uds_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        let socket_path = dir.join(format!("velo-kvbm-{}.sock", uuid::Uuid::new_v4()));
        let uds_transport = UdsTransportBuilder::new()
            .socket_path(&socket_path)
            .build()
            .context("Failed to build UDS transport")?;
        tracing::info!("Built UDS transport bound to {}", socket_path.display());
        builder = builder.add_transport(Arc::new(uds_transport));
    }

    builder = builder.add_transport(tcp_transport);

    if let Some(discovery) = discovery_override {
        builder = builder.discovery(discovery);
        tracing::info!("Using injected discovery backend (override)");
    } else if let Some(discovery_config) = &config.discovery {
        match discovery_config {
            DiscoveryConfig::Etcd(_cfg) => {
                bail!("Etcd discovery not yet supported in velo");
            }
            DiscoveryConfig::P2p(_cfg) => {
                bail!("P2P discovery not yet supported in velo");
            }
            DiscoveryConfig::Filesystem(cfg) => {
                use velo::discovery::FilesystemPeerDiscovery;

                let peer_discovery = FilesystemPeerDiscovery::new(&cfg.path)
                    .context("Failed to build filesystem discovery")?;

                builder = builder.discovery(Arc::new(peer_discovery));
                tracing::info!("Built filesystem discovery from: {:?}", cfg.path);
            }
        }
    }

    builder.build().await.context("Failed to build Velo")
}

/// Build a [`Messenger`] instance from messenger configuration.
pub async fn build_messenger(config: &MessengerConfig) -> Result<Arc<Messenger>> {
    let velo = build_velo(config).await?;
    Ok(velo.messenger().clone())
}

/// Convert serializable KVBM NIXL config into the NIXL backend config.
pub fn nixl_backend_config(config: &NixlConfig) -> NixlBackendConfig {
    NixlBackendConfig::new(config.backends.clone())
}
