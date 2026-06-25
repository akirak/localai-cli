# LocalAI CLI

This is a trivial CLI/TUI for the API of [LocalAI](https://github.com/mudler/LocalAI).
It's a vibe-coded internal tool that happens to be available in a public repository, so use it at your own risk.

## Installation

### Cargo

Build and run from the repository:

```bash
cargo run -- --help
```

Install the binary locally:

```bash
cargo install --path .
```

That installs the executable as `localai_cli`.

### Nix

Run it directly from the flake:

```bash
nix run github:akirak/localai-cli
```

## Configuration

By default the CLI talks to `http://localhost:8080`.

You can override connection settings either with flags or environment variables:

```bash
export LOCALAI_URL=http://localhost:8080
export LOCALAI_API_KEY=your-token
```

Equivalent flags:

```bash
localai_cli --base-url http://localhost:8080 --api-key your-token models list
```

## Usage

Show top-level help:

```bash
localai_cli --help
```

Available command groups:

- `models`
- `chat`
- `completions`
- `embeddings`
- `images`
- `audio`
- `backends`
- `request`
- `endpoints`
- `tags`
- `tui`

### Common request patterns

Most write operations accept one of these input styles:

- pass a JSON string with `--body`
- pass a file with `--body @request.json`
- pipe JSON on stdin when `--body` is omitted

Several commands also support convenience flags that populate common fields such as `--model`, `--prompt`, `--input`, or repeated `--message`.

### Examples

List installed models:

```bash
localai_cli models list
```

List installable models:

```bash
localai_cli models available
```

Send a chat request with convenience flags:

```bash
localai_cli chat --model llama-3.1 --message "Hello"
```

Send a chat request from a JSON file:

```bash
localai_cli chat --body @chat.json
```

Send a completion request from stdin:

```bash
printf '%s\n' '{"model":"gpt-oss","prompt":"Write a haiku about Rust."}' | localai_cli completions
```

Create embeddings:

```bash
localai_cli embeddings --model gpt-oss --input "example text"
```

Generate an image:

```bash
localai_cli images generate --model flux --prompt "A retro-futurist city at sunset"
```

Transcribe audio:

```bash
localai_cli audio transcribe --file sample.wav --model whisper --language en
```

Make a raw request:

```bash
localai_cli request GET /v1/models
```

Make a raw request with query parameters:

```bash
localai_cli request GET /models/jobs --query status=running
```

List documented endpoints:

```bash
localai_cli endpoints
```

List documented endpoints for a tag:

```bash
localai_cli endpoints --tag models
```

Start the terminal UI:

```bash
localai_cli tui
```

## Notes

- Responses are printed as formatted JSON in the CLI.
- The TUI is intended for browsing read-oriented APIs and presenting results as tables instead of raw JSON.
- Audio subcommands use multipart uploads and require readable local files.
