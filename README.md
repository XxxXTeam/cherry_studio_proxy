## cherry-studio-proxy

> “An OpenAI-compatible proxy that exposes Cherry Studio-backed models through standard /v1/chat/completions endpoints.”

### Features

- `POST /v1/chat/completions`
- `GET /v1/models`
- `GET /health`
- `GET /`
- Optional Bearer auth via `.env`
- Cherry HMAC signature headers
- JSON and SSE streaming support
- Chat payloads and upstream responses are passed through as much as possible

### Quick start

1. Copy `.env.example` to `.env`
2. Edit `.env` with your settings
3. Run `cargo run --release`

### Build

```bash
cargo build --release
```

The binary will be at `target/release/cherry_studio_proxy.exe`.
