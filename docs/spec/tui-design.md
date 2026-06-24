# TUI Design Guidelines

Specification for the interactive terminal UI (`src/tui.rs`, exposed via the
`tui` subcommand). This document captures the design principles the UI must
follow and serves as the reference for future changes.

## 1. Purpose

The TUI is a **read-oriented browser** for the LocalAI APIs. It complements —
but does not replace — the existing CLI subcommands. The CLI remains the
primary, scriptable interface; the TUI is for interactive exploration and
inspection of live server data.

## 2. Core principles

The four rules below are non-negotiable. Every change to the TUI should be
checked against them.

1. **One hierarchy, shared with the CLI.** The top-level navigation mirrors the
   CLI subcommand structure, not the flat list of API paths. Users who know the
   CLI should find the same groupings in the TUI and vice versa.
2. **Read first.** The TUI primarily surfaces `GET` endpoints. Mutating
   operations (`POST`, `PUT`, `DELETE`, …) are out of scope for the browser
   panels; submit them through the CLI `request` subcommand or dedicated
   subcommands instead.
3. **Never show raw JSON.** Responses are formatted into tables, key/value
   panels, or short human-readable status lines. Errors are plain text, not
   exception dumps.
4. **Prompt only when necessary.** Most `GET` actions run immediately with no
   input. The only time the user is asked for a value is when the request path
   contains a `{template}` parameter that cannot be inferred — and then only for
   the specific missing parameters.

## 3. Layout

The screen is divided vertically:

```
┌─────────────────────────────────────────────┐
│  Tabs                                        │  3 lines
├──────────────────┬──────────────────────────┤
│  List panel      │  Result / detail panel    │  flexible
│                  │                           │
├──────────────────┴──────────────────────────┤
│  Status bar                                  │  1 line
└─────────────────────────────────────────────┘
```

- **Tabs** (top). One row of tab titles corresponding to the CLI command
  groups that retrieve data. The active tab is highlighted (reversed
  background).
- **List panel** (left, ~38 % width). The enumerable entries of the active
  tab — either the available `GET` actions for that group or, for the Tags
  tab, the tags themselves.
- **Result panel** (right, ~62 % width). A formatted view of the currently
  selected action's response, or, for the Tags tab, the list of `GET`
  endpoints belonging to the selected tag.
- **Status bar** (bottom). A single-line summary: contextual keybinding hints
  and the outcome of the last request (`OK GET <path> (N rows)` /
  `FAIL GET <path>`).

## 4. Tabs and the CLI hierarchy

Tabs are the canonical top-level navigation and **must correspond 1:1 to a CLI
command group that retrieves data**. The current set is:

| Tab        | CLI analogue                                  | List content                                    |
|------------|-----------------------------------------------|-------------------------------------------------|
| Models     | `localai_cli models …` (list/available/jobs) | `GET` model-management actions                  |
| Backends   | `localai_cli backends …`                      | `GET` backend-management actions                |
| Endpoints  | `localai_cli endpoints`                      | Every `GET` operation known to `doc.json`       |
| Tags       | `localai_cli tags`                            | Every API tag                                    |

Guidelines for adding or changing tabs:

- A new tab is justified only when a CLI command group gains new read-only
  subcommands and would otherwise be absent.
- Tabs are ordered to match the order in which the CLI subcommands are declared
  in `src/main.rs` (with `Endpoints` and `Tags` kept last as the discovery
  tools they mirror).
- The Endpoints and Tags tabs reuse the structured helpers in
  `src/discover.rs` (`endpoints()`, `tags()`); do not hand-maintain a separate
  list of paths in the TUI.

## 5. Actions

A tab's list is made of **actions**. An action is a fixed, code-defined
description of a `GET` call:

- `name` (human label, shown in the list),
- `method` (always `GET` today),
- `path` (the API path, may contain `{param}` templates),
- `params` (the names of any path-template parameters).

Each CLI-backed tab declares its actions near the top of `src/tui.rs` (see
`model_actions()`, `backend_actions()`). Keep these lists **small, ordered and
curated** — they should read like the help output of the corresponding CLI
command group, not a dump of every endpoint. The Endpoints tab is the only one
that shows the exhaustive list.

Rules:

- Action lists must reflect the CLI subcommand grouping: an action's `name`
  should match the relevant CLI subcommand (`List`, `Available`, `Jobs`,
  `Job by UUID`, …) where one exists.
- An action with no `params` runs immediately on `Enter` and on `r` (refresh).
- An action with `params` opens an input popup (see §7) instead of running.

## 6. Result formatting

The right panel never displays JSON. Use the `ResultView` abstraction
(`kind`, `headers`, `rows`, `text`), backed by `json_to_table`:

- **Arrays** (including those wrapped under common keys like `data`,
  `models`, `backends`, `jobs`, …): rendered as a columned table. Columns are
  the ordered union of object keys across all elements, capped (today at 6) to
  keep the panel readable; cell text is truncated with a `…` marker.
- **Array of scalars**: rendered as `# | value`.
- **Single object**: rendered as `key | value`.
- **Empty result**: a centred "No data." placeholder.
- **Error**: rendered as a red, wrapped, scrollable plain-text message.

Formatting rules:

- Reuse `extract_array` for wrapper-unwrapping so adding a new wrapper key is a
  one-line change.
- Column widths are computed from the actual content and clamped to a sane min
  and max so a single wide cell never blows out the panel.
- A footer shows the row count and, when the table exceeds the visible height,
  the visible range (`a–b of N rows`).
- Do not pretty-print or syntax-highlight JSON in the UI. If a future need
  arises to expose the raw payload, do it via a separate, opt-in mode that is
  off by default.

## 7. Parameter prompting

The TUI must avoid modals wherever possible. The single permitted popup is an
**input popup** for path-template parameters:

- It appears only when the selected action's `path` contains `{param}`
  placeholders.
- Only the missing template parameters are requested; query/body parameters
  are never prompted for (the TUI targets `GET` reads that need at most a path
  segment).
- It is a small, centered, single-line input with a clear title
  (`Parameter: <name>`), the editable value, and a one-line help footer
  (`Enter: confirm  Esc: cancel`).
- On confirm, the value is substituted into the path and the request fires.
- On cancel (`Esc`), no request is made and the user returns to the list.

No other dialogs (confirm boxes, multi-step wizards, raw-editor forms) should be
added without revising this spec.

## 8. Interaction model

Keys must be discoverable from the status bar and follow these conventions:

| Key                | Action                                         |
|--------------------|-------------------------------------------------|
| `Tab` / `BackTab`  | Cycle tabs                                       |
| `1`–`4`            | Jump to a tab directly                          |
| `j` / `k`, `↓`/`↑` | Move the list selection                          |
| `PageDown` / `PageUp` | Move the list selection by 10               |
| `Enter`            | Run the selected action (or open the param popup)|
| `r`                | Re-run the last runnable action (refresh)       |
| `J` / `K`          | Scroll the result panel                          |
| `q` / `Esc`        | Quit (param popup: cancel instead)              |
| `Ctrl+C`           | Force quit                                       |

Conventions:

- Modal `q`/`Esc` cancels an open popup before quitting the app — never quit
  while a popup is open.
- Inputs accept paste (`Event::Paste`) in addition to typed characters.
- Scrolling is bounded and never over- or under-shoots the content.
- The app restores the terminal on exit regardless of outcome (raw mode off,
  leave alternate screen, show cursor). Failure to tear down must not lose the
  user's terminal.

## 9. Use of dependencies

- **ratatui** is the only widget framework. Prefer its built-in widgets
  (`Tabs`, `List`, `Table`, `Paragraph`, `Block`) over custom drawing.
- **crossterm** provides the terminal backend and event input. Treat only
  `KeyEventKind::Press` as a key activation so terminal paste/-release quirks
  don't double-fire actions.
- Keep all network I/O on the existing `LocalAIClient` (`src/client.rs`).
  The TUI must not introduce a second HTTP client or duplicate request logic;
  it calls `request_json` exactly as the CLI subcommands do.

## 10. Terminal lifecycle

`run()` owns the enable/teardown sequence:

1. Enable raw mode, enter the alternate screen, enable mouse capture and
   bracketed paste.
2. Drive the render/event loop until the user quits.
3. **Always** disable raw mode and leave the alternate screen — even on error
   or panic propagation — so the host terminal is left usable.

No code path should exit the program from inside the event loop without first
restoring the terminal.

## 11. Relationship to the existing CLI

- The CLI subcommands in `src/main.rs` are the source of truth for both the
  hierarchy and the request paths. When adding a CLI read subcommand, add the
  matching TUI action in the same change.
- The TUI must not remove, shadow, or change the behaviour of CLI subcommands.
  It is additive only.
- The TUI reuses `src/discover.rs` for anything derived from `doc.json`
  (endpoint listing, tag listing). Do not re-parse `doc.json` in the TUI.

## 12. Scope guardrails

Things the TUI deliberately does **not** do, to stay maintainable:

- No streaming chat/completions UI — that belongs to a dedicated `chat` view.
- No file upload / `multipart` flows in the TUI — use the CLI `audio` and
  `images` subcommands.
- No editing of request bodies — the browser is read-only; bodies are not
  constructed by the TUI.
- No persistence of state across runs — each invocation starts fresh.
- No configuration beyond the existing `--base-url` / `LOCALAI_URL` and
  `--api-key` / `LOCALAI_API_KEY` already used by the CLI.

Any feature falling outside these guardrails should be revisited in this
spec before being implemented.