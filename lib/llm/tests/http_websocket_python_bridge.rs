// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end WebSocket test that exercises the same wire path the Python
//! bidirectional bridge uses: a typed
//! [`PushRouter<RealtimeClientEvent, Annotated<RealtimeServerEvent>>`] on the
//! frontend side serializes each event to JSON, hands it to an
//! [`Ingress<ManyIn<serde_json::Value>, ManyOut<Annotated<serde_json::Value>>>`]
//! worker that depythonization-equivalent reads it as a plain
//! [`serde_json::Value`], emits a spec-shaped server event back as a `Value`,
//! and the frontend reads it back as `Annotated<RealtimeServerEvent>`.
//!
//! The worker engine in this test is a Rust engine that consumes
//! `ManyIn<Value>` and yields `ManyOut<Annotated<Value>>` — identical wire
//! shape to [`dynamo_py3::engine::PythonBidirectionalEngine`]. The Python
//! adapter's only additional responsibility (translating `Value` ↔ Python
//! `dict`) is already covered by the bindings-level smoke tests in
//! `lib/bindings/python/tests/test_bidirectional_endpoint.py`; this test
//! confirms the cross-typed-router wire compatibility claim end to end.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use dynamo_llm::endpoint_type::EndpointType;
use dynamo_llm::http::service::service_v2::HttpService;
use dynamo_protocols::types::realtime::{RealtimeClientEvent, RealtimeServerEvent};
use dynamo_runtime::{
    CancellationToken, DistributedRuntime, Runtime,
    distributed::DistributedConfig,
    engine::{AsyncEngine, AsyncEngineContextProvider, DataStream},
    pipeline::{
        ManyIn, ManyOut, ResponseStream,
        network::{
            Ingress,
            egress::push_router::{PushRouter, RouterMode},
        },
    },
    protocols::annotated::Annotated,
};

#[path = "common/ports.rs"]
mod ports;
use ports::bind_random_port;

const MODEL: &str = "wire-echo";

/// Minimal `RealtimeResponse` payload satisfying the required-field set on
/// the upstream deserializer (`id`, `max_output_tokens`, `object`, `output`,
/// `output_modalities`, `status`). Engines emit this on `response.created`
/// and `response.done` so the typed `PushRouter` reader on the frontend
/// side can decode the envelope cleanly.
fn response_payload(id: &str, status: &str) -> Value {
    json!({
        "id": id,
        "max_output_tokens": "inf",
        "object": "realtime.response",
        "output": [],
        "output_modalities": ["audio"],
        "status": status,
    })
}

async fn wait_for_health(port: u16) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("frontend never became healthy on port {port}");
}

/// Rust-side worker engine that mirrors the wire-level shape of the Python
/// bidirectional adapter: pulls `serde_json::Value` frames off the inbound
/// request stream and yields `Annotated<serde_json::Value>` frames shaped
/// like `RealtimeServerEvent` variants.
///
/// Behaviour:
///   - `session.update` → `session.updated` echoing the supplied session.
///   - `input_audio_buffer.append` → `response.created` →
///     `response.output_audio.delta` → `response.output_audio.done` →
///     `response.done`. Mirrors the Rust [`EchoBidirectionalEngine`] shape
///     so the wire-typed `PushRouter<RealtimeClientEvent, ..>` reader is
///     exercised across multiple `RealtimeServerEvent` variants on one
///     turn.
///   - Other client events → an `error` event with code
///     `unsupported_client_event`, matching the lenient-recovery contract
///     of the realtime frontend.
struct WireEchoEngine;

#[async_trait]
impl AsyncEngine<ManyIn<Value>, ManyOut<Annotated<Value>>, Error> for WireEchoEngine {
    async fn generate(&self, input: ManyIn<Value>) -> Result<ManyOut<Annotated<Value>>, Error> {
        let ctx = input.context();
        let session_id = ctx.id().to_string();
        let (request_stream, _ctx_unit) = input.into_parts();
        let mut inbound = request_stream
            .take()
            .ok_or_else(|| anyhow::anyhow!("RequestStream::take called twice on WireEchoEngine"))?;

        let session_for_loop = session_id.clone();
        let ctx_for_stream = ctx.clone();
        let stream = async_stream::stream! {
            let mut frame: u64 = 0;
            while let Some(value) = inbound.next().await {
                if ctx_for_stream.is_stopped() {
                    break;
                }
                let event_type = value
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();

                match event_type.as_str() {
                    "session.update" => {
                        frame += 1;
                        let session = value
                            .get("session")
                            .cloned()
                            .unwrap_or(Value::Null);
                        yield Annotated::from_data(json!({
                            "type": "session.updated",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "session": session,
                        }));
                    }
                    "input_audio_buffer.append" => {
                        let audio = value
                            .get("audio")
                            .and_then(|a| a.as_str())
                            .unwrap_or("")
                            .to_string();
                        let response_id = format!("resp_{session_for_loop}_{frame}");
                        let item_id = format!("item_{session_for_loop}_{frame}");

                        frame += 1;
                        yield Annotated::from_data(json!({
                            "type": "response.created",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "response": response_payload(&response_id, "in_progress"),
                        }));

                        // Emit one delta covering the full audio payload —
                        // the test only needs the multi-frame envelope shape,
                        // not the chunk-size behaviour the realtime echo
                        // engine uses for its own coverage.
                        frame += 1;
                        yield Annotated::from_data(json!({
                            "type": "response.output_audio.delta",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "response_id": response_id,
                            "item_id": item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "delta": audio,
                        }));

                        frame += 1;
                        yield Annotated::from_data(json!({
                            "type": "response.output_audio.done",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "response_id": response_id,
                            "item_id": item_id,
                            "output_index": 0,
                            "content_index": 0,
                        }));

                        frame += 1;
                        yield Annotated::from_data(json!({
                            "type": "response.done",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "response": response_payload(&response_id, "completed"),
                        }));
                    }
                    other => {
                        frame += 1;
                        yield Annotated::from_data(json!({
                            "type": "error",
                            "event_id": format!("event_{session_for_loop}_{frame}"),
                            "error": {
                                "type": "invalid_request_error",
                                "code": "unsupported_client_event",
                                "message": format!("wire echo does not support client event {other}"),
                            },
                        }));
                    }
                }
            }
        };

        let stream: DataStream<Annotated<Value>> = Box::pin(stream);
        Ok(ResponseStream::new(stream, ctx))
    }
}

/// Holds everything an end-to-end realtime test needs: a live HTTP service
/// with a typed `PushRouter` installed as the realtime engine, plus the
/// distributed runtime backing the worker side. Drop order matters: cancel
/// the HTTP token, await the join handle, then shut down the runtime.
struct Harness {
    port: u16,
    service_token: CancellationToken,
    service_handle: JoinHandle<Result<()>>,
    runtime: Runtime,
    _drt: DistributedRuntime,
}

impl Harness {
    async fn shutdown(self) {
        self.service_token.cancel();
        let _ = self.service_handle.await;
        self.runtime.shutdown();
    }
}

/// Build the full wire path: spin up a worker endpoint running an
/// `Ingress<ManyIn<Value>, ManyOut<Annotated<Value>>>` over a Rust
/// [`WireEchoEngine`], wait for discovery, build a typed
/// `PushRouter<RealtimeClientEvent, Annotated<RealtimeServerEvent>>`,
/// register it on the HTTP service's model manager, and start the HTTP
/// service on an ephemeral port.
async fn spawn_harness() -> Harness {
    let rt = Runtime::from_current().expect("current runtime");
    let drt = DistributedRuntime::new(rt.clone(), DistributedConfig::process_local())
        .await
        .expect("distributed runtime");

    let ns = drt
        .namespace("test_python_bridge_e2e".to_string())
        .expect("namespace");
    let component = ns
        .component("realtime_component".to_string())
        .expect("component");
    let endpoint = component.endpoint("realtime_endpoint".to_string());

    // Same Ingress shape as `JsonBidirectionalIngress` in the Python
    // bindings — the worker side accepts any valid JSON frame and emits
    // any Annotated<JSON> frame.
    let ingress: Arc<Ingress<ManyIn<Value>, ManyOut<Annotated<Value>>>> =
        Ingress::for_engine(Arc::new(WireEchoEngine)).expect("for_engine");

    let endpoint_for_server = endpoint.clone();
    tokio::spawn(async move {
        let _ = endpoint_for_server
            .endpoint_builder()
            .handler(ingress)
            .start()
            .await;
    });

    let client = endpoint.client().await.expect("client");
    client.wait_for_instances().await.expect("instances");

    // Build the typed PushRouter on the frontend side. This is the same
    // wire-typed router the realtime watcher builds at
    // `lib/llm/src/discovery/watcher.rs` when a worker advertises
    // `ModelType::Realtime + ModelInput::Text`. Serializes `RealtimeClientEvent`
    // to JSON on send, deserializes `Annotated<RealtimeServerEvent>` on
    // receive — proving wire compatibility across the type mismatch.
    let realtime_router =
        PushRouter::<RealtimeClientEvent, Annotated<RealtimeServerEvent>>::from_client(
            client,
            RouterMode::RoundRobin,
        )
        .await
        .expect("realtime push router");

    let (listener, port) = bind_random_port().await;
    let service = HttpService::builder()
        .port(port)
        .build()
        .expect("http build");
    service.enable_model_endpoint(EndpointType::Realtime, true);
    service
        .model_manager()
        .add_realtime_model(MODEL, "0", Arc::new(realtime_router))
        .expect("register realtime router as model engine");

    let token = CancellationToken::new();
    let handle = service.spawn_with_listener(token.clone(), listener).await;
    wait_for_health(port).await;

    Harness {
        port,
        service_token: token,
        service_handle: handle,
        runtime: rt,
        _drt: drt,
    }
}

async fn expect_text_event(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_type: &str,
) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .expect("frame within 3s")
        .expect("stream not closed")
        .expect("no transport error");
    let Message::Text(text) = frame else {
        panic!("expected Text frame, got {frame:?}");
    };
    let v: Value = serde_json::from_str(&text).expect("response is valid JSON");
    assert_eq!(
        v.get("type").and_then(|t| t.as_str()),
        Some(expected_type),
        "unexpected event type in {v}"
    );
    v
}

/// Drive a single `session.update` from a WebSocket client all the way to
/// the JSON-typed worker engine and back, asserting the round-tripped
/// `session.updated` decodes through the typed `PushRouter` reader.
#[tokio::test]
async fn realtime_websocket_python_bridge_session_update_round_trip() {
    let harness = spawn_harness().await;

    let url = format!("ws://127.0.0.1:{}/v1/realtime", harness.port);
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");

    expect_text_event(&mut ws, "session.created").await;

    let body = json!({
        "type": "session.update",
        "session": { "type": "realtime", "model": MODEL }
    });
    ws.send(Message::Text(body.to_string().into()))
        .await
        .expect("send session.update");

    let event = expect_text_event(&mut ws, "session.updated").await;
    assert_eq!(
        event.pointer("/session/type").and_then(|s| s.as_str()),
        Some("realtime"),
        "session.type should round-trip through wire-typed router: {event}"
    );
    assert_eq!(
        event.pointer("/session/model").and_then(|s| s.as_str()),
        Some(MODEL),
        "session.model should round-trip: {event}"
    );

    let _ = ws.close(None).await;
    harness.shutdown().await;
}

/// Drive `input_audio_buffer.append` and assert the spec-shaped multi-frame
/// response envelope (`response.created` → `response.output_audio.delta` →
/// `response.output_audio.done` → `response.done`) reaches the WebSocket.
/// Exercises every `RealtimeServerEvent` variant the worker emits — each
/// has to deserialize cleanly through the typed `PushRouter` reader to
/// reach the WebSocket.
#[tokio::test]
async fn realtime_websocket_python_bridge_audio_envelope_round_trip() {
    let harness = spawn_harness().await;

    let url = format!("ws://127.0.0.1:{}/v1/realtime", harness.port);
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");

    expect_text_event(&mut ws, "session.created").await;

    let session_update = json!({
        "type": "session.update",
        "session": { "type": "realtime", "model": MODEL }
    });
    ws.send(Message::Text(session_update.to_string().into()))
        .await
        .expect("send session.update");
    expect_text_event(&mut ws, "session.updated").await;

    let audio = "QUJDREVGRw==".to_string();
    let append = json!({
        "type": "input_audio_buffer.append",
        "audio": audio,
    });
    ws.send(Message::Text(append.to_string().into()))
        .await
        .expect("send append");

    let mut response_id: Option<String> = None;
    let mut deltas = String::new();
    let mut saw_audio_done = false;
    let mut response_done_status: Option<String> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while response_done_status.is_none() && tokio::time::Instant::now() < deadline {
        let frame = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("frame within 2s")
            .expect("stream not closed")
            .expect("no transport error");
        let Message::Text(text) = frame else {
            panic!("expected Text frame, got {frame:?}");
        };
        let event: Value = serde_json::from_str(&text).expect("response is valid JSON");
        let event_type = event
            .get("type")
            .and_then(|t| t.as_str())
            .expect("event has type")
            .to_string();
        match event_type.as_str() {
            "response.created" => {
                response_id = event
                    .pointer("/response/id")
                    .and_then(|s| s.as_str())
                    .map(String::from);
            }
            "response.output_audio.delta" => {
                let delta = event
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .expect("delta is a string");
                deltas.push_str(delta);
                assert_eq!(
                    event.pointer("/response_id").and_then(|s| s.as_str()),
                    response_id.as_deref(),
                    "delta response_id should match response.created"
                );
            }
            "response.output_audio.done" => {
                saw_audio_done = true;
            }
            "response.done" => {
                response_done_status = event
                    .pointer("/response/status")
                    .and_then(|s| s.as_str())
                    .map(String::from);
            }
            other => panic!("unexpected event type {other:?} in {event}"),
        }
    }

    let _ = ws.close(None).await;
    harness.shutdown().await;

    assert!(response_id.is_some(), "should see response.created");
    assert!(saw_audio_done, "should see response.output_audio.done");
    assert_eq!(response_done_status.as_deref(), Some("completed"));
    assert_eq!(
        deltas, audio,
        "concatenated audio deltas should reproduce input"
    );
}
