use axum::{
    Router,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{
        Html, IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::get,
};
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

/// A proxy that bridges Firefox's AI sidebar to a local LM Studio instance.
///
/// Firefox sends GET requests with `?q=<prompt>` to the configured chat provider URL.
/// This proxy translates those into OpenAI-compatible POST requests to LM Studio's
/// `/v1/chat/completions` endpoint, streaming the response back as rendered HTML.
#[derive(Parser, Clone)]
#[command(version, about)]
struct Args {
    /// Address to listen on
    #[arg(short, long, default_value = "127.0.0.1:8000", env = "LISTEN_ADDR")]
    listen: String,

    /// LM Studio base URL
    #[arg(long, default_value = "http://localhost:1234", env = "LMSTUDIO_URL")]
    lmstudio_url: String,

    /// Model identifier (empty = use whatever model LM Studio has loaded)
    #[arg(short, long, default_value = "", env = "MODEL")]
    model: String,
}

struct AppState {
    client: Client,
    args: Args,
}

// --- OpenAI-compatible request/response types ---

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}

// --- Query parameter from Firefox ---

#[derive(Deserialize)]
struct FirefoxQuery {
    q: Option<String>,
}

// --- Handlers ---

/// GET / — serves either the landing page or the streaming chat UI
async fn handle_page(Query(query): Query<FirefoxQuery>) -> Response {
    if query.q.as_ref().is_some_and(|q| !q.is_empty()) {
        Html(CHAT_HTML).into_response()
    } else {
        Html(LANDING_HTML).into_response()
    }
}

/// GET /api/chat?q=... — SSE streaming endpoint called by the chat UI's JavaScript
async fn handle_stream(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FirefoxQuery>,
) -> Response {
    let Some(prompt) = query.q.filter(|q| !q.is_empty()) else {
        return (StatusCode::BAD_REQUEST, "Missing 'q' parameter").into_response();
    };

    tracing::info!(prompt_len = prompt.len(), "Streaming request");

    let chat_req = ChatRequest {
        model: state.args.model.clone(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }],
        stream: true,
    };

    let url = format!("{}/v1/chat/completions", state.args.lmstudio_url);
    let result = state.client.post(&url).json(&chat_req).send().await;

    let resp = match result {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            tracing::error!(%status, %body, "LM Studio error");
            return sse_error(&format!(
                "LM Studio returned HTTP {}: {}",
                status, body
            ))
            .await;
        }
        Err(e) => {
            tracing::error!(%e, "Connection failed");
            return sse_error(&format!(
                "Could not connect to LM Studio at {}.\n\nIs LM Studio running with the server enabled?\n\nError: {}",
                state.args.lmstudio_url, e
            ))
            .await;
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(%e, "Stream read error");
                    let _ = tx
                        .send(Ok(Event::default()
                            .event("server_error")
                            .data(e.to_string())))
                        .await;
                    return;
                }
            };

            // Normalize \r\n to \n for SSE parsing
            let text = String::from_utf8_lossy(&chunk).replace('\r', "");
            buffer.push_str(&text);

            // Process complete SSE messages (delimited by \n\n)
            while let Some(pos) = buffer.find("\n\n") {
                let message = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                for line in message.lines() {
                    let line = line.trim();
                    let Some(data) = line.strip_prefix("data:") else {
                        continue;
                    };
                    let data = data.trim();

                    if data == "[DONE]" {
                        let _ =
                            tx.send(Ok(Event::default().event("done").data("")))
                                .await;
                        return;
                    }

                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data)
                        && let Some(content) = chunk
                            .choices
                            .first()
                            .and_then(|c| c.delta.content.as_deref())
                        && tx
                            .send(Ok(Event::default().data(content)))
                            .await
                            .is_err()
                    {
                        return; // client disconnected
                    }
                }
            }
        }

        // Stream ended without [DONE] — still signal completion
        let _ = tx
            .send(Ok(Event::default().event("done").data("")))
            .await;
    });

    Sse::new(ReceiverStream::new(rx)).into_response()
}

/// Send a single SSE error event and close the stream.
async fn sse_error(msg: &str) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(1);
    let _ = tx
        .send(Ok(Event::default().event("server_error").data(msg)))
        .await;
    drop(tx);
    Sse::new(ReceiverStream::new(rx)).into_response()
}

// --- Static assets (embedded at compile time) ---

const LANDING_HTML: &str = include_str!("../static/landing.html");
const CHAT_HTML: &str = include_str!("../static/chat.html");
const CHAT_CSS: &str = include_str!("../static/chat.css");
const CHAT_JS: &str = include_str!("../static/chat.js");

async fn serve_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CHAT_CSS)
}

async fn serve_js() -> impl IntoResponse {
    ([
        (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
    ], CHAT_JS)
}

// --- Main ---

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let state = Arc::new(AppState {
        client: Client::new(),
        args: args.clone(),
    });

    let app = Router::new()
        .route("/", get(handle_page))
        .route("/api/chat", get(handle_stream))
        .route("/static/chat.css", get(serve_css))
        .route("/static/chat.js", get(serve_js))
        .with_state(state);

    let listener = TcpListener::bind(&args.listen)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to {}: {}", args.listen, e);
            std::process::exit(1);
        });

    tracing::info!(
        listen = %args.listen,
        lmstudio_url = %args.lmstudio_url,
        model = %if args.model.is_empty() { "(auto)" } else { &args.model },
        "Proxy started"
    );
    eprintln!(
        "LM Studio Firefox Proxy listening on http://{}",
        args.listen
    );
    eprintln!("  LM Studio URL: {}", args.lmstudio_url);
    eprintln!(
        "  Model: {}",
        if args.model.is_empty() {
            "(using LM Studio's loaded model)"
        } else {
            &args.model
        }
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        });
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    eprintln!("\nShutting down...");
}
