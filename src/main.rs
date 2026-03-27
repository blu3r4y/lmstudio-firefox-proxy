use axum::{
    Router,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;

/// A proxy that bridges Firefox's AI sidebar to a local LM Studio instance.
///
/// Firefox sends GET requests with `?q=<prompt>` to the configured chat provider URL.
/// This proxy translates those into OpenAI-compatible POST requests to LM Studio's
/// `/v1/chat/completions` endpoint and returns the response as renderable content.
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
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

// --- Query parameter from Firefox ---

#[derive(Deserialize)]
struct FirefoxQuery {
    q: Option<String>,
}

// --- Handlers ---

async fn handle_request(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FirefoxQuery>,
) -> Response {
    let Some(prompt) = query.q.filter(|q| !q.is_empty()) else {
        return landing_page().into_response();
    };

    tracing::info!(prompt_len = prompt.len(), "Received prompt from Firefox");

    let chat_req = ChatRequest {
        model: state.args.model.clone(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }],
        stream: false,
    };

    let url = format!("{}/v1/chat/completions", state.args.lmstudio_url);

    let result = state
        .client
        .post(&url)
        .json(&chat_req)
        .send()
        .await;

    let resp = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(%e, "Failed to connect to LM Studio");
            return error_page(&format!(
                "Could not connect to LM Studio at {}.\n\nIs LM Studio running with the server enabled?\n\nError: {}",
                state.args.lmstudio_url, e
            ))
            .into_response();
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::error!(%status, %body, "LM Studio returned an error");
        return error_page(&format!(
            "LM Studio returned HTTP {}.\n\n{}",
            status, body
        ))
        .into_response();
    }

    let chat_resp: ChatResponse = match resp.json().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(%e, "Failed to parse LM Studio response");
            return error_page(&format!("Failed to parse LM Studio response: {}", e))
                .into_response();
        }
    };

    let content = chat_resp
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_else(|| "(No response from model)".into());

    tracing::info!(response_len = content.len(), "Returning response");

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        content,
    )
        .into_response()
}

fn landing_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>LM Studio Firefox Proxy</title>
<style>
  body { font-family: system-ui, sans-serif; max-width: 600px; margin: 60px auto; padding: 0 20px; color: #333; }
  h1 { font-size: 1.4em; }
  code { background: #f0f0f0; padding: 2px 6px; border-radius: 3px; }
</style></head>
<body>
  <h1>&#x2705; LM Studio Firefox Proxy is running</h1>
  <p>This proxy bridges Firefox's AI sidebar to your local LM Studio instance.</p>
  <p>If you see this page in Firefox's AI sidebar, the connection is working.
     Use the sidebar's chat features to send prompts.</p>
</body></html>"#,
    )
}

fn error_page(message: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Proxy Error</title>
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 600px; margin: 60px auto; padding: 0 20px; color: #333; }}
  pre {{ background: #fff0f0; padding: 12px; border-radius: 6px; white-space: pre-wrap; word-break: break-word; }}
</style></head>
<body>
  <h1>&#x26A0;&#xFE0F; Proxy Error</h1>
  <pre>{}</pre>
</body></html>"#,
        html_escape(message)
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
        .route("/", get(handle_request))
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
