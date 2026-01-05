//! Simple WebSocket tests to verify basic connectivity patterns.
//!
//! These tests validate the foundational WebSocket behavior that the
//! Titan client relies on.

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Test basic WebSocket without any extra headers.
#[tokio::test]
async fn test_simple_websocket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws_stream = tokio_tungstenite::accept_async(stream).await;
        if let Ok(mut ws) = ws_stream {
            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        ws.send(Message::Text(format!("echo: {}", text).into()))
                            .await
                            .ok();
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let url = format!("ws://{}", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("Failed to connect");

    ws.send(Message::Text("hello".into())).await.unwrap();
    if let Some(Ok(msg)) = ws.next().await {
        assert!(matches!(msg, Message::Text(_)));
    }
    ws.close(None).await.ok();

    server_task.abort();
}

/// Test WebSocket with subprotocol handling (using accept_hdr_async).
#[tokio::test]
async fn test_websocket_with_subprotocol() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();

        let callback = |req: &Request,
                        mut response: Response|
         -> Result<
            Response,
            tokio_tungstenite::tungstenite::http::Response<Option<String>>,
        > {
            if let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol") {
                if let Ok(protocols_str) = protocols.to_str() {
                    if let Some(first) = protocols_str.split(',').next() {
                        response
                            .headers_mut()
                            .insert("Sec-WebSocket-Protocol", first.trim().parse().unwrap());
                    }
                }
            }
            Ok(response)
        };

        let ws_stream = tokio_tungstenite::accept_hdr_async(stream, callback).await;
        if let Ok(mut ws) = ws_stream {
            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        ws.send(Message::Text(format!("echo: {}", text).into()))
                            .await
                            .ok();
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let url = format!("ws://{}", addr);
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "v1.api.titan.ag".parse().unwrap());

    let result = tokio_tungstenite::connect_async(request).await;
    let (mut ws, response) = result.expect("Failed to connect");

    // Verify subprotocol was echoed back
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .map(|v| v.to_str().unwrap()),
        Some("v1.api.titan.ag")
    );

    ws.send(Message::Text("hello".into())).await.unwrap();
    if let Some(Ok(msg)) = ws.next().await {
        assert!(matches!(msg, Message::Text(_)));
    }
    ws.close(None).await.ok();

    server_task.abort();
}

/// Test that URL with trailing slash and query param works.
/// This validates the fix for the HTTP format error.
#[tokio::test]
async fn test_url_with_query_param() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws_stream = tokio_tungstenite::accept_async(stream).await;
        if let Ok(mut ws) = ws_stream {
            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        ws.send(Message::Text(format!("echo: {}", text).into()))
                            .await
                            .ok();
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // URL MUST have a path before query param: ws://host:port/?query=value
    // ws://host:port?query=value will fail with HTTP format error
    let url = format!("ws://{}/?auth=test-token", addr);
    let request = url.into_client_request().expect("Failed to build request");

    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Failed to connect");

    ws.send(Message::Text("hello".into())).await.unwrap();
    if let Some(Ok(msg)) = ws.next().await {
        assert!(matches!(msg, Message::Text(_)));
    }
    ws.close(None).await.ok();

    server_task.abort();
}

mod common;

/// Test MockTitanServer with proper subprotocol handling.
#[tokio::test]
async fn test_mock_server() {
    use common::MockTitanServer;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let server = MockTitanServer::start().await;
    let url = server.url();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "v1.api.titan.ag".parse().unwrap());

    let (ws, response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Failed to connect to mock server");

    // Verify subprotocol was negotiated
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .map(|v| v.to_str().unwrap()),
        Some("v1.api.titan.ag")
    );

    drop(ws);
    server.stop().await;
}
