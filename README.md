# lmstudio-firefox-proxy

[![CI](https://github.com/blu3r4y/lmstudio-firefox-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/blu3r4y/lmstudio-firefox-proxy/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE.txt)

A lightweight Rust proxy that bridges **Firefox's AI sidebar** to a local **[LM Studio](https://lmstudio.ai/)** instance.

Firefox's AI chatbot sidebar sends `GET /?q=<prompt>` requests to the configured provider URL. LM Studio expects OpenAI-compatible `POST /v1/chat/completions` requests. This proxy translates between the two — streaming tokens in real time with a polished, rendered UI.

## 🪄 100% Vibe-coded

This project was **100% vibe-coded** — every single line of Rust, HTML, CSS, and JavaScript was written by AI through conversational prompting. No code was written by hand.

**Built with:**

- [GitHub Copilot](https://github.com/features/copilot) (agent mode)
- [Claude Opus 4.6](https://www.anthropic.com/claude) by Anthropic

The human provided the idea, direction, and feedback. The AI wrote the code.

## Features

- **Streaming** — Responses appear token-by-token as the model generates them (SSE)
- **Rendered Markdown** — Code blocks with syntax highlighting, tables, lists, and more
- **Thinking model support** — `<think>` blocks are shown in a collapsible panel during generation, then auto-collapsed once reasoning completes
- **Dark / Light mode** — Follows the system `prefers-color-scheme` automatically
- **Fully offline** — All frontend dependencies (marked.js, highlight.js) are vendored and embedded in the binary
- **Single binary** — No runtime dependencies, no config files, just run it

## Installation

### Pre-built binaries

Download the latest release for your platform from the [Releases](../../releases) page:

| Platform            | File                                                      |
| ------------------- | --------------------------------------------------------- |
| Linux x86_64        | `lmstudio-firefox-proxy-x86_64-unknown-linux-gnu.tar.gz`  |
| Linux aarch64       | `lmstudio-firefox-proxy-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64        | `lmstudio-firefox-proxy-x86_64-apple-darwin.tar.gz`       |
| macOS Apple Silicon | `lmstudio-firefox-proxy-aarch64-apple-darwin.tar.gz`      |
| Windows x86_64      | `lmstudio-firefox-proxy-x86_64-pc-windows-msvc.zip`       |

### Build from source

Requires [Rust](https://rustup.rs/) 1.85+.

```sh
cargo install --git https://github.com/blu3r4y/lmstudio-firefox-proxy
```

Or clone and build:

```sh
git clone https://github.com/blu3r4y/lmstudio-firefox-proxy
cd lmstudio-firefox-proxy
cargo build --release
```

The binary will be at `target/release/lmstudio-firefox-proxy` (`.exe` on Windows).

## Usage

```sh
# Use defaults (listen on 127.0.0.1:8000, LM Studio at localhost:1234)
lmstudio-firefox-proxy

# Specify a model explicitly
lmstudio-firefox-proxy --model "lmstudio-community/gemma-3-27B-it-qat-GGUF"

# Custom listen address and LM Studio URL
lmstudio-firefox-proxy --listen 127.0.0.1:9090 --lmstudio-url http://192.168.1.100:1234
```

All options can also be set via environment variables:

| Flag              | Env var        | Default                                   |
| ----------------- | -------------- | ----------------------------------------- |
| `--listen` / `-l` | `LISTEN_ADDR`  | `127.0.0.1:8000`                          |
| `--lmstudio-url`  | `LMSTUDIO_URL` | `http://localhost:1234`                   |
| `--model` / `-m`  | `MODEL`        | _(empty — uses LM Studio's loaded model)_ |

## Firefox Configuration

1. Open `about:config` in Firefox
2. Set `browser.ml.chat.enabled` to `true`
3. Set `browser.ml.chat.hideLocalhost` to `false`
4. Set `browser.ml.chat.provider` to `http://127.0.0.1:8000` (or whichever address the proxy listens on)
5. Open the AI chatbot sidebar (**Ctrl+Alt+X** or via the sidebar menu)

You should see a "Proxy is running" landing page. Select text on any page and use the "Ask AI" context menu, or use the sidebar directly.

## How it works

```txt
Firefox AI Sidebar                    This Proxy                         LM Studio
      |                                   |                                  |
      |  GET /?q=Explain+this+code        |                                  |
      |---------------------------------->|                                  |
      |  200 OK  (HTML page with JS)      |                                  |
      |<----------------------------------|                                  |
      |                                   |                                  |
      |  JS opens EventSource:            |                                  |
      |  GET /api/chat?q=Explain+...      |                                  |
      |---------------------------------->|                                  |
      |                                   |  POST /v1/chat/completions       |
      |                                   |  {stream: true, messages: [...]} |
      |                                   |--------------------------------->|
      |                                   |                                  |
      |                                   |  data: {"choices":[{"delta":     |
      |                                   |    {"content":"Here"}}]}         |
      |  SSE: data: Here                  |<---------------------------------|
      |<----------------------------------|  ...token by token...            |
      |  (JS renders markdown live)       |                                  |
      |                                   |  data: [DONE]                    |
      |  SSE: event: done                 |<---------------------------------|
      |<----------------------------------|                                  |
```
