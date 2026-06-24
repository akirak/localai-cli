//! Interactive terminal UI for browsing LocalAI's read (GET) APIs.
//!
//! The layout mirrors the CLI subcommand hierarchy: top-level tabs correspond
//! to the CLI command groups that retrieve data (Models, Backends, Endpoints,
//! Tags). Each tab shows a list of GET operations on the left and a formatted
//! table of the result on the right — raw JSON is never shown. Path-template
//! parameters (e.g. `{uuid}`) are the only values the user is ever prompted
//! for.

use std::collections::HashMap;
use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap,
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
use reqwest::Method;
use serde_json::Value;

use crate::client::LocalAIClient;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Take over the terminal and run the UI.
pub async fn run(client: LocalAIClient) -> Result<()> {
    enable_raw_mode().context("enabling raw mode")?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
    .context("entering alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("creating terminal")?;

    let result = run_app(&mut terminal, client).await;

    // Restore the terminal regardless of outcome.
    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();

    result
}

async fn run_app<B>(terminal: &mut Terminal<B>, client: LocalAIClient) -> Result<()>
where
    B: ratatui::backend::Backend,
    <B as ratatui::backend::Backend>::Error: std::fmt::Display + Send + Sync + 'static,
{
    let mut app = App::new(client)?;
    loop {
        terminal
            .draw(|f| draw(f, &mut app))
            .map_err(|e| anyhow::anyhow!("terminal draw failed: {e}"))?;

        if !crossterm::event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let event = crossterm::event::read()?;
        if !handle_event(&mut app, event).await {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Static description of the CLI hierarchy
// ---------------------------------------------------------------------------

const TABS: [&str; 4] = ["Models", "Backends", "Endpoints", "Tags"];

#[derive(Clone, Debug)]
struct Action {
    name: String,
    method: String,
    path: String,
    /// Path-template parameter names (e.g. `["uuid"]` for `/models/jobs/{uuid}`).
    params: Vec<String>,
}

impl Action {
    fn get(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            method: "GET".to_string(),
            path: path.to_string(),
            params: path_param_names(path),
        }
    }
}

fn model_actions() -> Vec<Action> {
    vec![
        Action::get("Models", "/v1/models"),
        Action::get("Available", "/models/available"),
        Action::get("Galleries", "/models/galleries"),
        Action::get("Jobs", "/models/jobs"),
        Action::get("Job by UUID", "/models/jobs/{uuid}"),
    ]
}

fn backend_actions() -> Vec<Action> {
    vec![
        Action::get("Installed", "/backends"),
        Action::get("Available", "/backends/available"),
        Action::get("Known", "/backends/known"),
        Action::get("Galleries", "/backends/galleries"),
        Action::get("Jobs", "/backends/jobs"),
        Action::get("Job by UUID", "/backends/jobs/{uuid}"),
    ]
}

// ---------------------------------------------------------------------------
// Result formatting (JSON → table)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum ResultKind {
    Table,
    Error,
    Empty,
}

struct ResultView {
    kind: ResultKind,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    text: String,
    path: String,
}

impl Default for ResultView {
    fn default() -> Self {
        Self {
            kind: ResultKind::Empty,
            headers: Vec::new(),
            rows: Vec::new(),
            text: String::new(),
            path: String::new(),
        }
    }
}

impl ResultView {
    fn set_table(&mut self, value: &Value, path: &str) {
        self.path = path.to_string();
        self.text.clear();
        let (headers, rows) = json_to_table(value);
        if rows.is_empty() && headers.is_empty() {
            self.kind = ResultKind::Empty;
            self.headers.clear();
            self.rows.clear();
            self.text = "No data.".to_string();
        } else {
            self.kind = ResultKind::Table;
            self.headers = headers;
            self.rows = rows;
        }
    }

    fn set_error(&mut self, msg: String, path: &str) {
        self.path = path.to_string();
        self.kind = ResultKind::Error;
        self.text = msg;
        self.headers.clear();
        self.rows.clear();
    }
}

/// First array found in `value`, unwrapping common wrapper objects (`data`, …).
fn extract_array(v: &Value) -> Option<&Vec<Value>> {
    match v {
        Value::Array(a) => Some(a),
        Value::Object(o) => {
            for k in [
                "data",
                "models",
                "backends",
                "jobs",
                "galleries",
                "tags",
                "results",
                "items",
                "endpoints",
                "available",
            ] {
                if let Some(Value::Array(a)) = o.get(k) {
                    return Some(a);
                }
            }
            for val in o.values() {
                if let Value::Array(a) = val {
                    return Some(a);
                }
            }
            None
        }
        _ => None,
    }
}

fn cell_string(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(o) => format!("{{{} fields}}", o.len()),
    };
    truncate(&s, 40)
}

fn truncate(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

/// Convert a JSON value into (headers, rows) for a `Table`.
fn json_to_table(v: &Value) -> (Vec<String>, Vec<Vec<String>>) {
    if let Some(arr) = extract_array(v) {
        if arr.is_empty() {
            return (Vec::new(), Vec::new());
        }
        // Ordered union of object keys.
        let mut keys: Vec<String> = Vec::new();
        for el in arr {
            if let Some(o) = el.as_object() {
                for k in o.keys() {
                    if !keys.contains(k) {
                        keys.push(k.clone());
                    }
                }
            }
        }
        if keys.is_empty() {
            // Array of scalars.
            let rows = arr
                .iter()
                .enumerate()
                .map(|(i, e)| vec![i.to_string(), cell_string(e)])
                .collect();
            return (vec!["#".to_string(), "value".to_string()], rows);
        }
        keys.truncate(6);
        let rows = arr
            .iter()
            .map(|el| {
                let o = el.as_object();
                keys.iter()
                    .map(|k| {
                        o.and_then(|m| m.get(k))
                            .map(cell_string)
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        return (keys, rows);
    }
    if let Some(o) = v.as_object() {
        if o.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let rows = o
            .iter()
            .map(|(k, val)| vec![k.clone(), cell_string(val)])
            .collect();
        return (vec!["key".to_string(), "value".to_string()], rows);
    }
    (vec!["value".to_string()], vec![vec![cell_string(v)]])
}

fn compute_widths(headers: &[String], rows: &[Vec<String>], _max_width: u16) -> Vec<Constraint> {
    let n = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in rows.iter().take(80) {
        for (i, cell) in r.iter().enumerate().take(n) {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    for w in &mut widths {
        *w = (*w).clamp(6, 28);
    }
    widths
        .into_iter()
        .map(|w| Constraint::Length(w as u16))
        .collect()
}

// ---------------------------------------------------------------------------
// Input popup state (for path-template parameters)
// ---------------------------------------------------------------------------

struct InputState {
    label: String,
    value: String,
    cursor: usize,
}

impl InputState {
    fn new(label: String) -> Self {
        Self {
            label,
            value: String::new(),
            cursor: 0,
        }
    }
    fn insert(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += 1;
    }
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let c = self.cursor - 1;
        self.value.remove(c);
        self.cursor -= 1;
    }
    fn move_cursor(&mut self, delta: i16) {
        let mut c = self.cursor as i16 + delta;
        if c < 0 {
            c = 0;
        }
        if c > self.value.len() as i16 {
            c = self.value.len() as i16;
        }
        self.cursor = c as usize;
    }
}

enum Popup {
    None,
    Input,
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct App {
    client: LocalAIClient,
    tab: usize,
    model_actions: Vec<Action>,
    backend_actions: Vec<Action>,
    endpoint_actions: Vec<Action>,
    tags: Vec<String>,
    tag_endpoints: HashMap<String, Vec<(String, String)>>,
    list_states: Vec<ListState>,
    result: ResultView,
    result_scroll: usize,
    status: String,
    popup: Popup,
    input: InputState,
    /// Action awaiting a parameter value before being sent.
    pending: Option<Action>,
}

impl App {
    fn new(client: LocalAIClient) -> Result<Self> {
        let model_actions = model_actions();
        let backend_actions = backend_actions();

        // Endpoints tab: every GET operation known to the doc.
        let endpoint_actions: Vec<Action> = crate::discover::endpoints()?
            .into_iter()
            .filter(|e| e.method == "GET")
            .map(|e| {
                let name = if e.summary.is_empty() {
                    e.path.clone()
                } else {
                    e.summary.clone()
                };
                Action::get(&name, &e.path)
            })
            .collect();

        // Tags tab + tag → endpoints map.
        let tags = crate::discover::tags()?;
        let mut tag_endpoints: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for ep in crate::discover::endpoints()? {
            if ep.method != "GET" {
                continue;
            }
            let summary = if ep.summary.is_empty() {
                String::new()
            } else {
                ep.summary.clone()
            };
            for t in &ep.tags {
                tag_endpoints
                    .entry(t.clone())
                    .or_default()
                    .push((ep.path.clone(), summary.clone()));
            }
        }
        for v in tag_endpoints.values_mut() {
            v.sort_by(|a, b| a.0.cmp(&b.0));
        }

        let lens = [
            model_actions.len(),
            backend_actions.len(),
            endpoint_actions.len(),
            tags.len(),
        ];
        let mut list_states: Vec<ListState> =
            (0..TABS.len()).map(|_| ListState::default()).collect();
        for (i, st) in list_states.iter_mut().enumerate() {
            if lens[i] > 0 {
                st.select(Some(0));
            }
        }

        Ok(Self {
            client,
            tab: 0,
            model_actions,
            backend_actions,
            endpoint_actions,
            tags,
            tag_endpoints,
            list_states,
            result: ResultView::default(),
            result_scroll: 0,
            status:
                "Tab: switch panel  j/k: move  Enter: run  r: refresh  J/K: scroll result  q: quit"
                    .into(),
            popup: Popup::None,
            input: InputState::new(String::new()),
            pending: None,
        })
    }

    fn list_len(&self) -> usize {
        match self.tab {
            0 => self.model_actions.len(),
            1 => self.backend_actions.len(),
            2 => self.endpoint_actions.len(),
            3 => self.tags.len(),
            _ => 0,
        }
    }

    fn list_state(&self) -> &ListState {
        &self.list_states[self.tab]
    }

    fn list_state_mut(&mut self) -> &mut ListState {
        &mut self.list_states[self.tab]
    }

    fn selected(&self) -> Option<usize> {
        self.list_state().selected()
    }

    fn move_list(&mut self, delta: i16) {
        let n = self.list_len() as i16;
        if n == 0 {
            return;
        }
        let mut idx = self.list_state().selected().unwrap_or(0) as i16 + delta;
        if idx < 0 {
            idx = 0;
        } else if idx >= n {
            idx = n - 1;
        }
        self.list_state_mut().select(Some(idx as usize));
        // For the Tags tab the right panel follows the selection.
        if self.tab == 3 {
            self.result_scroll = 0;
        }
    }

    fn switch_tab(&mut self, delta: i16) {
        let n = TABS.len() as i16;
        let mut t = self.tab as i16 + delta;
        if t < 0 {
            t = n - 1;
        } else if t >= n {
            t = 0;
        }
        self.tab = t as usize;
        self.result_scroll = 0;
    }

    /// The action selected on the current tab, if any.
    fn current_action(&self) -> Option<Action> {
        let idx = self.selected()?;
        match self.tab {
            0 => self.model_actions.get(idx).cloned(),
            1 => self.backend_actions.get(idx).cloned(),
            2 => self.endpoint_actions.get(idx).cloned(),
            _ => None,
        }
    }

    fn run_selected(&mut self) {
        let Some(action) = self.current_action() else {
            return;
        };
        if action.params.is_empty() {
            // No parameters needed: fire immediately.
            let path = action.path.clone();
            self.spawn_fetch(path);
        } else {
            // Prompt for the (single) path parameter.
            self.pending = Some(action);
            self.input = InputState::new(self.pending.as_ref().unwrap().params[0].clone());
            self.popup = Popup::Input;
            self.status = format!("Enter value for parameter: {}", self.input.label);
        }
    }

    /// Spawn an async fetch. We can't hold a borrow across await easily here,
    /// so callers use `spawn_fetch` which records the path and the actual
    /// request is performed in the event loop via `perform_fetch`.
    fn spawn_fetch(&mut self, path: String) {
        self.status = format!("GET {path} …");
        // Mark a pending fetch by stashing an empty action with the path.
        self.pending = Some(Action {
            name: String::new(),
            method: "GET".into(),
            path,
            params: Vec::new(),
        });
        self.result_scroll = 0;
    }

    async fn perform_fetch(&mut self) {
        let path = match &self.pending {
            Some(a) => a.path.clone(),
            None => return,
        };
        let res = self
            .client
            .request_json(Method::GET, &path, None, None)
            .await;
        match res {
            Ok(v) => {
                self.result.set_table(&v, &path);
                self.status = format!("OK   GET {path}  ({} rows)", self.result.rows.len());
                self.pending = None;
            }
            Err(e) => {
                self.result.set_error(format!("{e:#}"), &path);
                self.status = format!("FAIL GET {path}");
                self.pending = None;
            }
        }
    }

    fn scroll_result(&mut self, delta: i32, height: usize) {
        let total = match self.result.kind {
            ResultKind::Table => self.result.rows.len(),
            _ => self.result.text.lines().count(),
        };
        let max = total.saturating_sub(height);
        let mut next = self.result_scroll as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next > max as i32 {
            next = max as i32;
        }
        self.result_scroll = next as usize;
    }
}

/// Return a reference to the conceptual "list" of a tab (for length only).
fn path_param_names(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                name.push(c);
            }
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

async fn handle_event(app: &mut App, event: Event) -> bool {
    match event {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return true;
            }
            handle_key(app, key).await
        }
        Event::Paste(text) => {
            match &app.popup {
                Popup::Input => {
                    app.input.value.push_str(&text);
                    app.input.cursor = app.input.value.len();
                }
                Popup::None => {}
            }
            true
        }
        _ => true,
    }
}

async fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return false;
    }

    // Input popup takes all keys.
    if matches!(app.popup, Popup::Input) {
        match key.code {
            KeyCode::Esc => {
                app.popup = Popup::None;
                app.pending = None;
                app.status = "Cancelled.".into();
            }
            KeyCode::Enter => {
                let value = app.input.value.clone();
                if let Some(action) = app.pending.take()
                    && let Some(param) = action.params.first()
                {
                    let path = action.path.replacen(&format!("{{{param}}}"), &value, 1);
                    app.popup = Popup::None;
                    app.spawn_fetch(path);
                    app.perform_fetch().await;
                }
            }
            KeyCode::Backspace => app.input.backspace(),
            KeyCode::Left => app.input.move_cursor(-1),
            KeyCode::Right => app.input.move_cursor(1),
            KeyCode::Char(ch) => app.input.insert(ch),
            _ => {}
        }
        return true;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Tab => app.switch_tab(1),
        KeyCode::BackTab => app.switch_tab(-1),
        KeyCode::Char('1') => app.tab = 0,
        KeyCode::Char('2') => app.tab = 1,
        KeyCode::Char('3') => app.tab = 2,
        KeyCode::Char('4') => app.tab = 3,
        KeyCode::Down | KeyCode::Char('j') => app.move_list(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_list(-1),
        KeyCode::PageDown => app.move_list(10),
        KeyCode::PageUp => app.move_list(-10),
        KeyCode::Char('J') => app.scroll_result(3, 20),
        KeyCode::Char('K') => app.scroll_result(-3, 20),
        KeyCode::Enter => {
            if app.tab != 3 {
                app.run_selected();
                if matches!(app.popup, Popup::None) {
                    app.perform_fetch().await;
                }
            }
        }
        KeyCode::Char('r') => {
            if app.tab != 3
                && let Some(a) = app.current_action()
                && a.params.is_empty()
            {
                app.spawn_fetch(a.path);
                app.perform_fetch().await;
            }
        }
        _ => {}
    }
    true
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_tabs(f, app, chunks[0]);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(chunks[1]);

    draw_list(f, app, main[0]);

    if app.tab == 3 {
        draw_tag_endpoints(f, app, main[1]);
    } else {
        draw_result(f, app, main[1]);
    }

    draw_status(f, app, chunks[2]);

    if matches!(app.popup, Popup::Input) {
        draw_input_popup(f, app, area);
    }
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let titles: Vec<Line> = TABS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == app.tab {
                Line::from(Span::styled(
                    format!(" {t} "),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(format!(" {t} "))
            }
        })
        .collect();
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" LocalAI Browser "),
        )
        .divider(Span::raw("│"));
    f.render_widget(tabs, area);
}

fn list_items(app: &App) -> Vec<ListItem<'static>> {
    match app.tab {
        0 => app.model_actions.iter().map(action_item).collect(),
        1 => app.backend_actions.iter().map(action_item).collect(),
        2 => app
            .endpoint_actions
            .iter()
            .map(|a| {
                let path = Span::styled(a.path.clone(), Style::default().fg(Color::Green));
                let sep = Span::raw("  ");
                let name = Span::styled(
                    truncate(&a.name, 26),
                    Style::default().add_modifier(Modifier::BOLD),
                );
                ListItem::new(Line::from(vec![name, sep, path]))
            })
            .collect(),
        3 => app
            .tags
            .iter()
            .map(|t| ListItem::new(Line::from(Span::raw(t.clone()))))
            .collect(),
        _ => Vec::new(),
    }
}

fn action_item(a: &Action) -> ListItem<'static> {
    let mut spans = vec![Span::styled(
        format!("{:<6}", a.method),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        truncate(&a.name, 22),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        a.path.clone(),
        Style::default().fg(Color::DarkGray),
    ));
    if !a.params.is_empty() {
        spans.push(Span::styled(
            format!("  ✎{}", a.params.join(",")),
            Style::default().fg(Color::Yellow),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.tab {
        0 => " Models ",
        1 => " Backends ",
        2 => " Endpoints (GET) ",
        3 => " Tags ",
        _ => " ",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let list = List::new(list_items(app))
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, app.list_state_mut());
}

fn draw_result(f: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.result.path.is_empty() {
        " Result ".to_string()
    } else {
        format!(" GET {} ", app.result.path)
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    match app.result.kind {
        ResultKind::Empty => {
            f.render_widget(
                Paragraph::new(if app.result.text.is_empty() {
                    "Run an action with Enter.".to_string()
                } else {
                    app.result.text.clone()
                })
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
                inner,
            );
        }
        ResultKind::Error => {
            let p = Paragraph::new(app.result.text.clone())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false })
                .scroll((app.result_scroll as u16, 0));
            f.render_widget(p, inner);
        }
        ResultKind::Table => {
            draw_table(f, app, inner);
        }
    }
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let headers = &app.result.headers;
    let rows = &app.result.rows;
    if headers.is_empty() {
        f.render_widget(
            Paragraph::new("No data.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let widths = compute_widths(headers, rows, area.width);
    let header_row = Row::new(headers.clone()).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let height = area.height as usize;
    let max_body_rows = height.saturating_sub(2); // header + footer
    let total = rows.len();
    let start = app
        .result_scroll
        .min(total.saturating_sub(max_body_rows.max(1)));
    let end = (start + max_body_rows).min(total);

    let body_rows: Vec<Row> = rows[start..end]
        .iter()
        .map(|r| Row::new(r.clone()))
        .collect();

    let table = Table::new(body_rows, widths).header(header_row);
    f.render_widget(table, area);

    // Footer: row count + scroll position.
    let footer = if total > max_body_rows {
        format!(" {}–{} of {} rows ", start + 1, end, total)
    } else {
        format!(" {} rows ", total)
    };
    let footer_area = Rect {
        x: area.x,
        y: area.bottom().saturating_sub(1),
        width: area.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(Color::Black).bg(Color::DarkGray))
            .alignment(Alignment::Right),
        footer_area,
    );
}

fn draw_tag_endpoints(f: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.selected() {
        Some(i) if i < app.tags.len() => format!(" Endpoints tagged “{}” ", app.tags[i]),
        _ => " Endpoints ".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(sel) = app.selected() else {
        f.render_widget(
            Paragraph::new("Select a tag.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };
    let tag = &app.tags[sel];
    let entries = app.tag_endpoints.get(tag).cloned().unwrap_or_default();

    if entries.is_empty() {
        f.render_widget(
            Paragraph::new("No GET endpoints for this tag.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let widths = [Constraint::Percentage(45), Constraint::Percentage(55)];

    let height = inner.height as usize;
    let max_body_rows = height.saturating_sub(2).max(1);
    let total = entries.len();
    let start = app.result_scroll.min(total.saturating_sub(max_body_rows));
    let end = (start + max_body_rows).min(total);

    let body_rows: Vec<Row> = entries[start..end]
        .iter()
        .map(|(p, s)| Row::new([p.clone(), s.clone()]))
        .collect();
    let table = Table::new(body_rows, widths).header(
        Row::new(["path", "summary"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    );
    f.render_widget(table, inner);

    let footer = if total > max_body_rows {
        format!(" {}–{} of {} ", start + 1, end, total)
    } else {
        format!(" {} endpoints ", total)
    };
    let footer_area = Rect {
        x: inner.x,
        y: inner.bottom().saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(Color::Black).bg(Color::DarkGray))
            .alignment(Alignment::Right),
        footer_area,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let p = Paragraph::new(format!(" {} ", app.status))
        .style(Style::default().fg(Color::Black).bg(Color::DarkGray));
    f.render_widget(p, area);
}

fn draw_input_popup(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = centered_rect(50, 20, area);
    f.render_widget(Clear, rect);

    let prompt = format!(" Parameter: {} ", app.input.label);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(prompt)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let line = Line::from(vec![
        Span::raw(app.input.value.clone()),
        Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
    ]);
    let p = Paragraph::new(line);
    f.render_widget(p, inner);

    let help = "Enter: confirm  Esc: cancel";
    let help_area = Rect {
        x: inner.x,
        y: inner.bottom().saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(help)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        help_area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let pop = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(100 - percent_y),
            Constraint::Percentage(percent_y),
        ])
        .split(area)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(pop)[1]
}
