# lmstudio-firefox-proxy

A lightweight Rust proxy that bridges Firefox's AI sidebar to a local [LM Studio](https://lmstudio.ai/) instance.

Firefox's AI chatbot sidebar sends `GET /?q=<prompt>` requests to the configured provider URL. LM Studio expects OpenAI-compatible `POST /v1/chat/completions` requests. This proxy translates between the two.

## Prerequisites

- **Rust** 1.85+ (`rustc 1.94.1` recommended)
- **LM Studio** running locally with the server enabled (default port 1234)

## Build

```sh
cargo build --release
```

The binary will be at `target/release/lmstudio-firefox-proxy` (or `.exe` on Windows).

## Run

```sh
# Use defaults (listen on 127.0.0.1:8000, LM Studio at localhost:1234)
./target/release/lmstudio-firefox-proxy

# Specify a model explicitly
./target/release/lmstudio-firefox-proxy --model "lmstudio-community/gemma-3-27B-it-qat-GGUF"

# Custom listen address and LM Studio URL
./target/release/lmstudio-firefox-proxy --listen 127.0.0.1:9090 --lmstudio-url http://192.168.1.100:1234
```

All options can also be set via environment variables:

| Flag | Env var | Default |
|---|---|---|
| `--listen` / `-l` | `LISTEN_ADDR` | `127.0.0.1:8000` |
| `--lmstudio-url` | `LMSTUDIO_URL` | `http://localhost:1234` |
| `--model` / `-m` | `MODEL` | *(empty — uses LM Studio's loaded model)* |

## Firefox Configuration

1. Open `about:config` in Firefox
2. Set `browser.ml.chat.enabled` to `true`
3. Set `browser.ml.chat.hideLocalhost` to `false`
4. Set `browser.ml.chat.provider` to `http://127.0.0.1:8000` (or whichever address the proxy listens on)
5. Open the AI chatbot sidebar (Ctrl+Alt+X or via the sidebar menu)

You should see a "Proxy is running" landing page. Select text on any page and use the "Ask AI" context menu, or use the sidebar directly.

## How it works

```
Firefox AI Sidebar                    This Proxy                         LM Studio
      |                                   |                                  |
      |  GET /?q=Explain+this+code        |                                  |
      |---------------------------------->|                                  |
      |                                   |  POST /v1/chat/completions       |
      |                                   |  {"messages":[{"role":"user",    |
      |                                   |    "content":"Explain this..."}]}|
      |                                   |--------------------------------->|
      |                                   |                                  |
      |                                   |  {"choices":[{"message":         |
      |                                   |    {"content":"Here is..."}}]}   |
      |                                   |<---------------------------------|
      |  200 OK (text/markdown)           |                                  |
      |  "Here is the explanation..."     |                                  |
      |<----------------------------------|                                  |
```

## License

MIT
