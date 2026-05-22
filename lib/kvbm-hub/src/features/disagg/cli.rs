// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! `kvbmctl` client CLI for the ConditionalDisagg feature.

use anyhow::Result;
use clap::{ArgMatches, Command};
use futures::future::BoxFuture;

use crate::client::HubClient;
use crate::features::cli::FeatureCli;
use crate::features::disagg::protocol::{ConditionalDisaggInstancesResponse, ROUTE_PREFIX};
use crate::protocol::FeatureKey;

/// Zero-state CLI for the ConditionalDisagg feature.
pub struct DisaggCli;

impl FeatureCli for DisaggCli {
    fn key(&self) -> FeatureKey {
        FeatureKey::ConditionalDisagg
    }

    fn command(&self) -> Command {
        Command::new("disagg")
            .about("Query ConditionalDisagg hub state")
            .subcommand_required(true)
            .arg_required_else_help(true)
            .subcommand(Command::new("get-instances").about("List prefill/decode instances"))
    }

    fn run<'a>(
        &'a self,
        hub: &'a HubClient,
        matches: &'a ArgMatches,
    ) -> BoxFuture<'a, Result<serde_json::Value>> {
        Box::pin(async move {
            let base = format!("/v1/features/{ROUTE_PREFIX}");
            match matches.subcommand() {
                Some(("get-instances", _)) => {
                    let r: ConditionalDisaggInstancesResponse =
                        hub.get_json(&format!("{base}/instances")).await?;
                    Ok(serde_json::to_value(r)?)
                }
                _ => unreachable!("subcommand_required(true) guarantees a subcommand"),
            }
        })
    }
}
