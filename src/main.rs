use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
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

// --- Static HTML pages ---

const LANDING_HTML: &str = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>LM Studio Firefox Proxy</title>
<style>
  :root { --bg: #fff; --fg: #1a1a1a; }
  @media (prefers-color-scheme: dark) { :root { --bg: #0d1117; --fg: #e6edf3; } }
  body { font-family: system-ui, sans-serif; max-width: 600px; margin: 60px auto;
         padding: 0 20px; color: var(--fg); background: var(--bg); }
  h1 { font-size: 1.4em; }
</style></head><body>
  <h1>&#x2705; LM Studio Firefox Proxy is running</h1>
  <p>This proxy bridges Firefox's AI sidebar to your local LM Studio instance.</p>
  <p>If you see this page in Firefox's AI sidebar, the connection is working.
     Use the sidebar's chat features to send prompts.</p>
</body></html>"#;

const CHAT_HTML: &str = r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>LM Studio</title>
<link rel="stylesheet"
  href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/github.min.css"
  media="(prefers-color-scheme: light)">
<link rel="stylesheet"
  href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/github-dark.min.css"
  media="(prefers-color-scheme: dark)">
<script src="https://cdnjs.cloudflare.com/ajax/libs/marked/12.0.2/marked.min.js"></script>
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
<style>
:root {
  --bg: #ffffff; --fg: #1a1a1a; --muted: #666; --border: #e5e7eb;
  --code-bg: #f6f8fa; --pre-bg: #f6f8fa; --accent: #2563eb;
  --block-bg: #f9fafb; --err-bg: #fef2f2; --err-fg: #991b1b;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d1117; --fg: #e6edf3; --muted: #8b949e; --border: #30363d;
    --code-bg: #161b22; --pre-bg: #161b22; --accent: #58a6ff;
    --block-bg: #161b22; --err-bg: #3d1f1f; --err-fg: #f8a0a0;
  }
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: var(--bg); color: var(--fg);
  line-height: 1.65; padding: 16px; font-size: 14px;
  overflow-wrap: break-word;
}
#status {
  display: flex; align-items: center; gap: 10px;
  color: var(--muted); padding: 20px 0;
}
.spinner {
  width: 18px; height: 18px;
  border: 2px solid var(--border); border-top-color: var(--accent);
  border-radius: 50%; animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
#error {
  display: none; padding: 12px 16px; border-radius: 8px;
  background: var(--err-bg); color: var(--err-fg);
  font-size: 13px; white-space: pre-wrap;
}

/* ---- Markdown content ---- */
#response > :first-child { margin-top: 0; }
#response h1, #response h2, #response h3,
#response h4, #response h5, #response h6 {
  margin: 1.2em 0 0.5em; line-height: 1.3;
}
#response h1 { font-size: 1.5em; border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }
#response h2 { font-size: 1.3em; border-bottom: 1px solid var(--border); padding-bottom: 0.2em; }
#response h3 { font-size: 1.15em; }
#response p { margin: 0.6em 0; }
#response ul, #response ol { margin: 0.6em 0; padding-left: 1.8em; }
#response li { margin: 0.25em 0; }
#response li > p { margin: 0.3em 0; }
#response blockquote {
  margin: 0.6em 0; padding: 0.4em 1em;
  border-left: 3px solid var(--accent); color: var(--muted);
  background: var(--block-bg); border-radius: 0 6px 6px 0;
}
#response code {
  font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', Consolas, monospace;
  font-size: 0.88em; background: var(--code-bg);
  padding: 0.15em 0.4em; border-radius: 4px;
}
#response pre {
  margin: 0.8em 0; padding: 14px; border-radius: 8px;
  background: var(--pre-bg); overflow-x: auto;
  border: 1px solid var(--border);
}
#response pre code {
  background: none; padding: 0; font-size: 0.85em; line-height: 1.5;
}
#response table {
  margin: 0.8em 0; border-collapse: collapse; width: 100%; font-size: 0.9em;
}
#response th, #response td {
  padding: 8px 12px; border: 1px solid var(--border); text-align: left;
}
#response th { background: var(--block-bg); font-weight: 600; }
#response hr { margin: 1.2em 0; border: none; border-top: 1px solid var(--border); }
#response a { color: var(--accent); text-decoration: none; }
#response a:hover { text-decoration: underline; }
#response img { max-width: 100%; border-radius: 6px; }

/* Blinking cursor while streaming */
#response.streaming > :last-child::after {
  content: '\25CB'; color: var(--accent); animation: blink 1s step-end infinite;
  margin-left: 2px; font-size: 0.8em;
}
@keyframes blink { 50% { opacity: 0; } }
</style></head><body>
<div id="status"><div class="spinner"></div><span>Thinking&#x2026;</span></div>
<div id="response" class="streaming"></div>
<div id="error"></div>
<script>
(function() {
  var statusEl  = document.getElementById('status');
  var responseEl = document.getElementById('response');
  var errorEl   = document.getElementById('error');
  var content = '', isDone = false, renderFrame = null;

  function render() {
    responseEl.innerHTML = marked.parse(content);
    responseEl.querySelectorAll('pre code:not(.hljs)').forEach(function(el) {
      hljs.highlightElement(el);
    });
  }

  function scheduleRender() {
    if (renderFrame) return;
    renderFrame = requestAnimationFrame(function() {
      renderFrame = null;
      render();
      window.scrollTo(0, document.body.scrollHeight);
    });
  }

  function showError(msg) {
    statusEl.style.display = 'none';
    responseEl.classList.remove('streaming');
    errorEl.style.display = 'block';
    errorEl.textContent = msg;
  }

  var es = new EventSource('/api/chat' + window.location.search);

  es.onmessage = function(e) {
    statusEl.style.display = 'none';
    content += e.data;
    scheduleRender();
  };

  es.addEventListener('done', function() {
    isDone = true;
    es.close();
    responseEl.classList.remove('streaming');
    render();
  });

  es.addEventListener('server_error', function(e) {
    isDone = true;
    es.close();
    showError(e.data);
  });

  es.onerror = function() {
    es.close();
    responseEl.classList.remove('streaming');
    if (!isDone && !content) {
      showError('Failed to connect to the proxy server.');
    } else if (content) {
      render();
    }
  };
})();
</script></body></html>"##;

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
