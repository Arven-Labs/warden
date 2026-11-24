//! The rich terminal loop, used only when stdin is a real TTY (see `main.rs`'s `IsTerminal`
//! check) — a bordered input box and a "thinking" status box, both drawn with `ratatui`'s
//! *inline* viewport (not full-screen/alt-screen — the terminal's own scroll history, not a
//! second buffer ratatui owns, is what holds the growing conversation), plus real token-by-token
//! streaming of the assistant's response. `run` clears the screen (and scrollback) before
//! printing anything, so opening Warden reads as starting a clean session rather than a banner
//! stuck below whatever was on screen already (explicit user feedback: the first version of this
//! module didn't do that, and it read as a plain terminal script, not an app). The plain
//! non-interactive loop in `main.rs` is untouched and still handles piped stdin (scripted use,
//! the existing process-level tests).
//!
//! There's no line-editing library here (`rustyline` is gone) — a bordered, redrawn-every-frame
//! input box needs to own the terminal region itself, which a separate line-editing library
//! doesn't compose with. `LineEditor` below is a small hand-rolled one; its cursor/history logic
//! is unit-tested directly (no terminal involved). The line-editing history file this module
//! reads/writes is a plain newline-per-entry text file — a new, simpler format than rustyline's
//! own, since only the on-disk *format* changed, not what it's for (still just remembered input
//! lines, not the conversation itself, which was never persisted here).
//!
//! Key events are read via `crossterm::event::poll`/`read` — bounded, synchronous polls — never
//! `crossterm::event::EventStream`. `EventStream` spawns a background thread that, once polled
//! even once, blocks indefinitely inside crossterm's internal event reader until a byte actually
//! arrives on stdin; that reader is a single process-wide lock shared with
//! `cursor::position()` (what ratatui's inline viewport uses under the hood, on construction and
//! on every `Terminal::clear()`). Since a chat turn spends most of its time with nobody typing,
//! that lock stays held for the whole "thinking" wait, so the `clear()` call that swaps the
//! thinking box for the streamed response can't get the cursor position and fails after
//! crossterm's hardcoded 2s timeout — reproduced directly against a real pty before this comment
//! was written. Bounded polls never hold that lock past their own timeout, so `clear()` always
//! finds it free.
//!
//! Every line that should become permanent conversation history (the user's own message, the
//! assistant's reply, token counts, errors) is committed via `Terminal::insert_before` —
//! `ratatui`'s own API for "print history above a pinned inline box" — never via a bare
//! `print!`/`println!`. An earlier version of this module printed those lines directly to
//! stdout while raw mode was on, bypassing `ratatui`'s bookkeeping of where the inline viewport
//! (the input/status box) actually sits on screen; the box's position is only ever updated by
//! `Terminal::draw`/`clear`/`insert_before`, so a raw print left it stale, and the next
//! `draw()`/`clear()` rendered into the wrong row — the input box would end up staircased away
//! from the conversation instead of pinned below it (confirmed against a real pty, and against
//! the user's own report of a garbled, non-chat-like layout). `insert_before` also sidesteps the
//! `\n`-vs-`\r\n` raw-mode pitfall entirely, since it positions the cursor per cell on the
//! backend rather than relying on the terminal's own newline handling.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use owo_colors::OwoColorize;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::{Terminal, TerminalOptions, Viewport};
use tokio::sync::{mpsc, oneshot};
use unicode_width::UnicodeWidthStr;
use warden_bootstrap::{
    build_model_for, default_limit_configs, default_model_for, load_config_from_path, remove_agent_references, remove_provider_references,
    remove_combo, rename_provider_cascade, resolve_vault_path as bootstrap_resolve_vault_path, save_config, scope_to_agent, AgentConfig, AgentExtras, ComboConfig, FileConfig,
    LimitConfig, LimitScope, Overrides, Provider, ProviderConfig, SshHostConfig,
};
use warden_core::model::{Message, ModelProvider, StreamEvent, Usage};
use warden_core::memory::Vault;
use warden_core::orchestrator::{MessageOutcome, Orchestrator};
use warden_core::skill::{self, Skill, SkillStore};
use warden_core::spend::{now_millis, LimitStatus, Price, SpendGuard, Spent, DEFAULT_EXTEND_STEP, DEFAULT_WARN_AT};
use warden_core::tool::ssh::test_connection;
use warden_core::autonomy::{Grant, GrantBook, Grants, RememberingApprover};
use warden_core::tool::{Answer, ApprovalRequest, Approver};

use warden_bootstrap::arca_hub;
use warden_bootstrap::remote_identities::RemoteIdentity;
use warden_bootstrap::saved_hubs::SavedHub;
use warden_sync::arca_mode::ArcaMode;
use warden_sync::arca_pairing::{HubAccess, PendingOffer};
use warden_sync::arca_sync::{is_auth_rejected, ArcaSyncReport, PendingEpoch};
use warden_sync::arca_key_swap::{run_grant_replacing, run_replace_key, KeySwapError};
use warden_sync::arca_pairing_flow::{run_grant, run_offer, Ask, FlowError, PairingPrompts, Shown, Wait};
use warden_sync::arca_rotation::{PendingRotation, RotationError};

use crate::commands::{self, Command, ParseOutcome};
use crate::{i18n, tr};

/// Height (in terminal rows) of the inline viewport used for both the input box and the
/// "thinking" box — a single line of content plus its top/bottom border.
const VIEWPORT_HEIGHT: u16 = 3;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_HISTORY_ENTRIES: usize = 500;

type CliTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Enables raw mode for the lifetime of this guard and unconditionally restores the terminal on
/// drop — including on an early `?`-return or a panic partway through a turn, so a crash never
/// leaves the user's shell stuck reading raw keystrokes.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

/// A small hand-rolled line editor: a UTF-8-aware cursor over a `Vec<char>`, plus a persisted
/// history list with the usual up/down recall (including restoring an in-progress draft when
/// paging back down past the newest history entry). Every method here is plain, terminal-free
/// logic — see the unit tests below.
struct LineEditor {
    buffer: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: Option<Vec<char>>,
    /// A secret being typed: the box shows a dot per character instead of the text.
    masked: bool,
}

impl LineEditor {
    fn new(history: Vec<String>) -> Self {
        Self { buffer: Vec::new(), cursor: 0, history, history_index: None, draft: None, masked: false }
    }

    fn new_masked() -> Self {
        Self { masked: true, ..Self::new(Vec::new()) }
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Whether the cursor sits at the end of the buffer — the only position `drive_line_editor`
    /// shows/accepts a tab-completion ghost suggestion at, since a suggestion computed from the
    /// whole buffer wouldn't make sense to insert in the middle of already-typed text.
    fn cursor_at_end(&self) -> bool {
        self.cursor == self.buffer.len()
    }

    fn as_str(&self) -> String {
        self.buffer.iter().collect()
    }

    /// The portion of the buffer before the cursor — used to compute the cursor's on-screen
    /// column, since a wide/combining character can take more than one terminal cell.
    fn prefix(&self) -> String {
        self.buffer[..self.cursor].iter().collect()
    }

    fn insert(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    fn clear_line(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    /// Deletes the word immediately before the cursor (Ctrl+W) — skips trailing spaces first,
    /// then deletes back to the next space or the start of the line.
    fn delete_word_backward(&mut self) {
        let mut i = self.cursor;
        while i > 0 && self.buffer[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && self.buffer[i - 1] != ' ' {
            i -= 1;
        }
        self.buffer.drain(i..self.cursor);
        self.cursor = i;
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_index = match self.history_index {
            None => {
                self.draft = Some(self.buffer.clone());
                self.history.len() - 1
            }
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.history_index = Some(next_index);
        self.buffer = self.history[next_index].chars().collect();
        self.cursor = self.buffer.len();
    }

    fn history_down(&mut self) {
        match self.history_index {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.history_index = Some(i + 1);
                self.buffer = self.history[i + 1].chars().collect();
                self.cursor = self.buffer.len();
            }
            Some(_) => {
                self.history_index = None;
                self.buffer = self.draft.take().unwrap_or_default();
                self.cursor = self.buffer.len();
            }
        }
    }

    /// Takes the current buffer as the submitted line, resetting the editor for the next one.
    fn submit(&mut self) -> String {
        let text = self.as_str();
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.draft = None;
        text
    }
}

/// What `load_history` did to the file itself, besides reading it.
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)] // skeleton: the next commit builds `Rewritten` and `Failed` and reads all three
enum HistoryScrub {
    /// Nothing in the file looked like a phrase: it was not rewritten.
    Untouched,
    /// A line that looked like a phrase was erased from the file.
    Rewritten,
    /// A line that looked like a phrase is still in the file: it could not be rewritten.
    Failed,
}

fn load_history(path: &Path) -> (Vec<String>, HistoryScrub) {
    let lines = std::fs::read_to_string(path)
        // A file written by an older session may already hold a phrase: the same rule as for a new line, so it is neither recalled
        // nor written back.
        .map(|contents| contents.lines().filter(|line| keep_in_history(line)).map(str::to_string).collect())
        .unwrap_or_default();
    (lines, HistoryScrub::Untouched)
}

fn save_history(path: &Path, history: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let start = history.len().saturating_sub(MAX_HISTORY_ENTRIES);
    let _ = std::fs::write(path, history[start..].join("\n"));
}

/// `ghost` is the dim, non-editable completion suffix shown right after the typed text (see
/// `drive_line_editor`'s doc comment) — `None` for a wizard field (`read_field`), which never
/// suggests slash-commands since its buffer holds an id/persona/key, not a command.
fn render_input_box(frame: &mut ratatui::Frame, editor: &LineEditor, title: &str, ghost: Option<&str>) {
    let area = frame.area();
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::default().fg(accent_color())).title(title.to_string());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let shown = if editor.masked { "•".repeat(editor.buffer.len()) } else { editor.as_str() };
    let mut spans = vec![Span::raw(shown)];
    if let Some(suffix) = ghost {
        spans.push(Span::styled(suffix.to_string(), Style::default().add_modifier(Modifier::DIM)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);

    let cursor_col = inner.x + if editor.masked { editor.cursor as u16 } else { UnicodeWidthStr::width(editor.prefix().as_str()) as u16 };
    frame.set_cursor_position((cursor_col, inner.y));
}

/// What the live, not-yet-committed assistant preview (see `render_card_preview`) currently
/// shows: a spinner before any content has arrived, or the tail of the response streamed in so
/// far once it has (see `run_turn` and `MAX_PREVIEW_ROWS`).
enum PreviewState<'a> {
    Thinking { elapsed: Duration, spinner_frame: usize },
    AwaitingApproval,
    Streaming { lines: &'a [String] },
}

/// Same rounded border as `render_input_box`, titled like the final committed card
/// (`insert_card`) it turns into once the reply completes — so the transition from "still
/// streaming" to "permanent history" is just the border staying in place while the box grows and
/// then gets committed, not a jump between two unrelated visual styles.
fn render_card_preview(frame: &mut ratatui::Frame, state: PreviewState) {
    let area = frame.area();
    let title = Line::from(vec![Span::styled("● ", Style::default().fg(Color::Rgb(46, 204, 113))), Span::raw("Warden")]);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::default().fg(accent_color())).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match state {
        PreviewState::Thinking { elapsed, spinner_frame } => {
            let glyph = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
            let line = Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(Color::Rgb(241, 196, 15))),
                Span::styled(tr!("preview.thinking", seconds = elapsed.as_secs()), Style::default().add_modifier(Modifier::DIM)),
            ]);
            frame.render_widget(Paragraph::new(line), inner);
        }
        PreviewState::AwaitingApproval => {
            let line = Line::from(vec![
                Span::styled("? ", Style::default().fg(Color::Rgb(241, 196, 15))),
                Span::styled(tr!("preview.awaiting"), Style::default().add_modifier(Modifier::DIM)),
            ]);
            frame.render_widget(Paragraph::new(line), inner);
        }
        PreviewState::Streaming { lines } => {
            let text: Vec<Line> = lines.iter().map(|l| Line::raw(l.clone())).collect();
            frame.render_widget(Paragraph::new(text), inner);
        }
    }
}

fn new_inline_terminal_with_height(height: u16) -> anyhow::Result<CliTerminal> {
    let backend = CrosstermBackend::new(io::stdout());
    Ok(Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Inline(height) })?)
}

fn new_inline_terminal() -> anyhow::Result<CliTerminal> {
    new_inline_terminal_with_height(VIEWPORT_HEIGHT)
}

/// Body rows the live preview shows at once before it stops growing and just scrolls to show the
/// tail (see `ensure_preview_height`) — keeps a very long reply from making the preview take over
/// the whole screen while it's still streaming. The final card (`insert_card`) always gets every
/// row regardless; this only bounds the *live* view.
const MAX_PREVIEW_ROWS: usize = 6;

/// Grows (or shrinks back down) the live card-preview viewport to fit `wanted_rows` of body text
/// plus its border, rebuilding the `Terminal` only when the height actually needs to change.
/// Safe to do repeatedly now that `EventStream` (see the module doc comment) is gone: rebuilding
/// just queries the cursor position once via the backend, and nothing else can hold that query's
/// lock indefinitely anymore, which is what made a fresh `Terminal` per call hang before.
fn ensure_preview_height(terminal: &mut CliTerminal, current_height: &mut u16, wanted_rows: usize) -> anyhow::Result<()> {
    let wanted_height = (wanted_rows.min(MAX_PREVIEW_ROWS) as u16 + 2).max(VIEWPORT_HEIGHT);
    if wanted_height != *current_height {
        terminal.clear()?;
        *terminal = new_inline_terminal_with_height(wanted_height)?;
        *current_height = wanted_height;
    }
    Ok(())
}

fn key_is_ctrl_c(key: &crossterm::event::KeyEvent) -> bool {
    key.kind == KeyEventKind::Press && key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

enum LineOutcome {
    Submitted(String),
    Exit,
}

/// The terminal's current column count — used to wrap history text ourselves before committing
/// it (see `wrap_text`), since `Terminal::insert_before` needs an exact row count up front.
fn terminal_width() -> u16 {
    crossterm::terminal::size().map(|(cols, _)| cols).unwrap_or(80)
}

/// Wraps one line (no embedded newlines) to at most `width` display columns, breaking on spaces
/// where possible and only hard-breaking a single word that alone exceeds `width`. Always returns
/// at least one (possibly empty) row.
fn wrap_segment(segment: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in segment.split_inclusive(' ') {
        let word_width = UnicodeWidthStr::width(word);
        if current_width > 0 && current_width + word_width > width {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width > width {
            for c in word.chars() {
                let c_width = UnicodeWidthStr::width(c.to_string().as_str()).max(1);
                if current_width + c_width > width {
                    rows.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(c);
                current_width += c_width;
            }
            continue;
        }
        current.push_str(word);
        current_width += word_width;
    }
    rows.push(current);
    rows
}

/// Wraps possibly-multi-line `text` (split on real `\n`s first, each segment wrapped on its own)
/// to `width` display columns — Warden's own tiny word-wrapper, used instead of `ratatui`'s
/// (which only reports a wrapped row count through an unstable, render-time API) so a row count
/// is known ahead of the `insert_before` call below.
fn wrap_text(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1) as usize;
    text.split('\n').flat_map(|segment| wrap_segment(segment, width)).collect()
}

/// Permanently commits one block of text (which may be wider than the terminal, in which case it
/// soft-wraps, and may contain embedded newlines) to the scrollback, just above the pinned
/// input/status box, via `insert_before` — see the module doc comment for why this, and never a
/// raw `print!`/`println!`, is the only safe way to grow the conversation history.
fn insert_history_line(terminal: &mut CliTerminal, text: &str, style: Style) -> anyhow::Result<()> {
    let rows = wrap_text(text, terminal_width());
    let height = rows.len() as u16;
    terminal.insert_before(height, move |buf| {
        for (i, row) in rows.iter().enumerate() {
            buf.set_string(buf.area.x, buf.area.y + i as u16, row, style);
        }
    })?;
    Ok(())
}

/// Width (in display columns) available for a message card's content — the full terminal width
/// minus the left `"│ "` and right `" │"` border padding (see `insert_card`).
fn card_content_width() -> usize {
    (terminal_width() as usize).saturating_sub(4).max(1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Header,
    Bullet,
    Code,
    Plain,
}

/// Classifies one already-`\n`-free markdown-ish line, stripping its block syntax so only the
/// content remains. `in_code_block` toggles on a fenced-code delimiter line (`` ``` ``, in which
/// case there's nothing to render — `None`) and must be threaded across an entire turn by the
/// caller, not reset per call.
fn classify_and_strip(line: &str, in_code_block: &mut bool) -> Option<(LineKind, String)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") {
        *in_code_block = !*in_code_block;
        return None;
    }
    if *in_code_block {
        return Some((LineKind::Code, line.to_string()));
    }
    for prefix in ["### ", "## ", "# "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some((LineKind::Header, rest.to_string()));
        }
    }
    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        return Some((LineKind::Bullet, rest.to_string()));
    }
    Some((LineKind::Plain, line.to_string()))
}

/// The index of the first run of `len` consecutive `marker` characters at or after `start`, if
/// any — used by `parse_inline` to find a markdown delimiter's closing counterpart.
fn find_marker(chars: &[char], start: usize, marker: char, len: usize) -> Option<usize> {
    (start..=chars.len().saturating_sub(len)).find(|&i| chars[i..i + len].iter().all(|&c| c == marker))
}

/// Parses `**bold**`, `*italic*`/`_italic_` and `` `inline code` `` out of one line, applying
/// `base` to everything else. Not full CommonMark — just the constructs a chat reply actually
/// uses — and doesn't try to recover from a marker left open at the end of `text`: it's rendered
/// as its own literal characters instead. A marker split across two separately-committed history
/// rows (see `run_turn`) is a rare, self-limited cosmetic edge case for the same reason: once a
/// row is committed via `insert_before` it can't be redrawn once the rest of the markup arrives.
fn parse_inline(text: &str, base: Style) -> Vec<(String, Style)> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !buf.is_empty() {
                spans.push((std::mem::take(&mut buf), base));
            }
        };
    }

    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(end) = find_marker(&chars, i + 1, '`', 1) {
                flush!();
                spans.push((chars[i + 1..end].iter().collect(), base.fg(accent_color())));
                i = end + 1;
                continue;
            }
        }
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find_marker(&chars, i + 2, '*', 2) {
                flush!();
                spans.push((chars[i + 2..end].iter().collect(), base.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
        }
        if chars[i] == '*' || chars[i] == '_' {
            let marker = chars[i];
            if let Some(end) = find_marker(&chars, i + 1, marker, 1) {
                if end > i + 1 {
                    flush!();
                    spans.push((chars[i + 1..end].iter().collect(), base.add_modifier(Modifier::ITALIC)));
                    i = end + 1;
                    continue;
                }
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush!();
    spans
}

/// Same purple as `BRAND` (see its doc comment) — reused here for markdown headers/bullets so
/// they read as part of the same visual identity rather than a random extra color.
fn accent_color() -> Color {
    let (r, g, b) = BRAND;
    Color::Rgb(r, g, b)
}

/// Classifies+strips `line` (a complete, `\n`-terminated logical line — never a still-streaming
/// partial one, see `run_turn`) and turns it into styled spans: a header is bold and accented, a
/// bullet gets a `"• "` marker, a fenced code line is styled as code with no inline parsing
/// (so `**`/`` ` `` inside real code isn't mistaken for markdown), everything else gets inline
/// span parsing. Returns `None` for a fence delimiter line, which renders nothing.
fn markdown_spans_for_line(line: &str, in_code_block: &mut bool) -> Option<Vec<(String, Style)>> {
    let (kind, content) = classify_and_strip(line, in_code_block)?;
    Some(match kind {
        LineKind::Header => parse_inline(&content, Style::default().add_modifier(Modifier::BOLD).fg(accent_color())),
        LineKind::Bullet => {
            let mut spans = vec![("• ".to_string(), Style::default().fg(accent_color()))];
            spans.extend(parse_inline(&content, Style::default()));
            spans
        }
        LineKind::Code => vec![(content, Style::default().fg(accent_color()))],
        LineKind::Plain => parse_inline(&content, Style::default()),
    })
}

/// Word-wraps already-styled spans to `width` display columns — the same greedy algorithm as
/// `wrap_segment`, but keeping each token's style attached, so markdown emphasis survives
/// wrapping instead of collapsing back to plain text.
fn wrap_spans(spans: &[(String, Style)], width: usize) -> Vec<Vec<(String, Style)>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut current: Vec<(String, Style)> = Vec::new();
    let mut current_width = 0usize;

    for (text, style) in spans {
        for word in text.split_inclusive(' ') {
            let word_width = UnicodeWidthStr::width(word);
            if current_width > 0 && current_width + word_width > width {
                rows.push(std::mem::take(&mut current));
                current_width = 0;
            }
            if word_width > width {
                let mut piece = String::new();
                let mut piece_width = 0usize;
                for c in word.chars() {
                    let c_width = UnicodeWidthStr::width(c.to_string().as_str()).max(1);
                    if current_width + piece_width + c_width > width {
                        if !piece.is_empty() {
                            current.push((std::mem::take(&mut piece), *style));
                            piece_width = 0;
                        }
                        rows.push(std::mem::take(&mut current));
                        current_width = 0;
                    }
                    piece.push(c);
                    piece_width += c_width;
                }
                if !piece.is_empty() {
                    current.push((piece, *style));
                    current_width += piece_width;
                }
                continue;
            }
            current.push((word.to_string(), *style));
            current_width += word_width;
        }
    }
    rows.push(current);
    rows
}

/// Classifies+styles a complete assistant reply (all of it, not a still-streaming partial chunk
/// — see `run_turn`) into wrapped, card-ready body rows: every logical (`\n`-separated) line runs
/// through `markdown_spans_for_line` and gets wrapped to `card_content_width()`. Calling this only
/// once the full text is known — instead of line-by-line as it streams, which an earlier version
/// of this module did — means a markdown marker can never end up split across two separately
/// committed rows; see `PENDING.md` P34 for that now-obsolete trade-off. Never empty: an empty
/// reply still gets one empty row, so its card has a body instead of being just two borders.
fn markdown_body_rows(text: &str) -> Vec<Vec<(String, Style)>> {
    let mut in_code_block = false;
    let width = card_content_width();
    let rows: Vec<Vec<(String, Style)>> =
        text.split('\n').flat_map(|line| markdown_spans_for_line(line, &mut in_code_block).map(|spans| wrap_spans(&spans, width)).unwrap_or_default()).collect();
    if rows.is_empty() { vec![Vec::new()] } else { rows }
}

/// Wraps the user's own raw input to `card_content_width()` for their message card — deliberately
/// no markdown parsing: what they typed is literal text, not something to interpret as markup.
fn plain_body_rows(text: &str) -> Vec<Vec<(String, Style)>> {
    wrap_text(text, card_content_width() as u16).into_iter().map(|row| vec![(row, Style::default())]).collect()
}

/// Wraps each already-styled line to `card_content_width()` for a `/models`/`/agents` list card —
/// same idea as `plain_body_rows`, but for a caller that already picked a style per line (e.g. a
/// dimmed marker on the session-active entry) instead of plain text.
fn list_body_rows(lines: Vec<(String, Style)>) -> Vec<Vec<(String, Style)>> {
    let width = card_content_width();
    lines.into_iter().flat_map(|(text, style)| wrap_spans(&[(text, style)], width)).collect()
}

/// One row of a card's bordered content — `"│ "`, the row's styled spans, then a right-aligned
/// `"│"` at the card's own width. A blank `row` (no spans) still draws both border characters, so
/// an empty spacer line inside a card (see `insert_card`'s footer handling) still looks framed.
fn draw_card_row(buf: &mut Buffer, x0: u16, y: u16, width: u16, row: &[(String, Style)], border_style: Style) {
    buf.set_string(x0, y, "│ ", border_style);
    let mut x = x0 + 2;
    for (text, style) in row {
        buf.set_string(x, y, text, *style);
        x += UnicodeWidthStr::width(text.as_str()) as u16;
    }
    buf.set_string(x0 + width - 1, y, "│", border_style);
}

/// The card's own width: sized to fit its widest row (title or content), never wider than
/// `max_width` (the terminal) and never narrower than a sane minimum. Body/footer rows are
/// already wrapped to `card_content_width()` (a `max_width`-based cap, always `<= max_width - 4`),
/// so shrinking to the widest one here can only ever narrow the card, never force a re-wrap — a
/// short reply like "ok" gets a snug card instead of a bar stretched across the whole terminal.
fn card_width(title_width: usize, content_rows: &[Vec<(String, Style)>], max_width: usize) -> u16 {
    let widest_content: usize = content_rows.iter().map(|row| row.iter().map(|(t, _)| UnicodeWidthStr::width(t.as_str())).sum()).max().unwrap_or(0);
    (widest_content + 4).max(title_width + 6).clamp(12, max_width.max(12)) as u16
}

/// Renders one complete message "card" — a bordered box sized to fit its own longest row (never
/// wider than the terminal, never narrower than a sane minimum), with a titled top border,
/// already-wrapped/styled body rows, an optional single footer row (e.g. token count, preceded by
/// a blank spacer), and a bottom border — as one permanent block of history via `insert_before`.
/// A card is only ever committed once its full content is known: `insert_before` commits are
/// permanent, so a border can't be "reopened" to append more body rows into it later — the
/// still-streaming assistant reply is shown live in the pinned preview instead
/// (`render_card_preview`) and only turned into a card once it's complete (see `run_turn`).
fn insert_card(terminal: &mut CliTerminal, title: Vec<(String, Style)>, border_style: Style, body: Vec<Vec<(String, Style)>>, footer: Option<Vec<(String, Style)>>) -> anyhow::Result<()> {
    let title_width: usize = title.iter().map(|(t, _)| UnicodeWidthStr::width(t.as_str())).sum();

    let mut content_rows = body;
    if let Some(footer_row) = footer {
        content_rows.push(Vec::new());
        content_rows.push(footer_row);
    }

    let width = card_width(title_width, &content_rows, terminal_width() as usize);
    let top_dashes = (width as usize).saturating_sub(4 + title_width);
    let height = content_rows.len() as u16 + 2;

    terminal.insert_before(height, move |buf| {
        let x0 = buf.area.x;
        let y0 = buf.area.y;

        buf.set_string(x0, y0, "╭─ ", border_style);
        let mut x = x0 + 3;
        for (text, style) in &title {
            buf.set_string(x, y0, text, *style);
            x += UnicodeWidthStr::width(text.as_str()) as u16;
        }
        buf.set_string(x, y0, format!(" {}╮", "─".repeat(top_dashes)), border_style);

        for (i, row) in content_rows.iter().enumerate() {
            draw_card_row(buf, x0, y0 + 1 + i as u16, width, row, border_style);
        }

        buf.set_string(x0, y0 + height - 1, format!("╰{}╯", "─".repeat((width as usize).saturating_sub(2))), border_style);
    })?;
    Ok(())
}

/// How long each bounded `event::poll` waits before giving the caller a chance to redraw/check
/// other state. Small enough that typing and the spinner both feel instant; see the module-level
/// doc comment for why this must be a bounded poll, never `EventStream`.
const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// Drives one `LineEditor` to completion: redraws the bordered input box (titled `title`) every
/// frame and applies key events to it, exactly like the main chat prompt — shared so a wizard
/// field (`read_field`) gets the same editing keys (arrows, Ctrl+A/E/U/W, history recall if the
/// editor was built with any) without duplicating this loop. By the time this returns, the box
/// has been cleared, so whatever's printed next starts from a clean, normal line of scrollback.
///
/// `suggest_commands` gates the tab-completion ghost suggestion (`commands::ghost_suggestion`) —
/// on for the main chat prompt (`read_line`), off for a wizard field (`read_field`), whose buffer
/// holds an id/persona/key, never a slash-command. `Tab` accepts the currently shown ghost (if
/// any), appending its suffix plus a trailing space — a no-op when there's nothing to accept
/// (ambiguous, already complete, or the cursor isn't at the end of the buffer).
async fn drive_line_editor(terminal: &mut CliTerminal, editor: &mut LineEditor, title: &str, suggest_commands: bool) -> anyhow::Result<LineOutcome> {
    loop {
        let ghost = if suggest_commands && editor.cursor_at_end() { commands::ghost_suggestion(&editor.as_str()) } else { None };
        terminal.draw(|frame| render_input_box(frame, editor, title, ghost.as_deref()))?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match (key.code, key.modifiers) {
                (KeyCode::Enter, _) => {
                    let text = editor.submit();
                    terminal.clear()?;
                    return Ok(LineOutcome::Submitted(text));
                }
                // Matches GNU readline: Ctrl+D only quits on an empty line, so it can't be
                // mistaken for "discard what I just typed" — same reason rustyline's `Eof` never
                // fired on a non-empty buffer before this module dropped it. In a wizard
                // (`read_field`), this same `Exit` outcome means "cancel this field/wizard"
                // rather than "quit the REPL" — the caller decides which.
                (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) && editor.is_empty() => {
                    terminal.clear()?;
                    return Ok(LineOutcome::Exit);
                }
                (KeyCode::Tab, _) => {
                    if let Some(suffix) = &ghost {
                        for c in suffix.chars() {
                            editor.insert(c);
                        }
                        editor.insert(' ');
                    }
                }
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => editor.clear_line(),
                (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => editor.move_home(),
                (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => editor.move_end(),
                (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => editor.clear_line(),
                (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => editor.delete_word_backward(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => editor.insert(c),
                (KeyCode::Backspace, _) => editor.backspace(),
                (KeyCode::Delete, _) => editor.delete_forward(),
                (KeyCode::Left, _) => editor.move_left(),
                (KeyCode::Right, _) => editor.move_right(),
                (KeyCode::Home, _) => editor.move_home(),
                (KeyCode::End, _) => editor.move_end(),
                (KeyCode::Up, _) => editor.history_up(),
                (KeyCode::Down, _) => editor.history_down(),
                _ => {}
            },
            _ => {}
        }
    }
}

async fn read_line(line_history: &mut Vec<String>, terminal: &mut CliTerminal) -> anyhow::Result<LineOutcome> {
    let mut editor = LineEditor::new(line_history.clone());
    let outcome = drive_line_editor(terminal, &mut editor, " Warden ", true).await?;
    if let LineOutcome::Submitted(text) = &outcome {
        remember_line(line_history, text);
    }
    Ok(outcome)
}

/// Whether a submitted line may go into the line history, which is saved in clear text (`save_history`) and recalled with the arrow
/// keys. A blank line does not, and neither does `/arca rotate ...` with anything after it: that is where the 24 words of a
/// `resume` end up when typed on the line (the text of a stop half way tells the person to run `resume`). Nor does a line that is
/// the phrase on its own, typed as chat (`commands::is_bare_phrase`).
fn keep_in_history(line: &str) -> bool {
    !line.trim().is_empty() && !commands::may_hold_a_phrase(line) && !commands::is_bare_phrase(line)
}

fn remember_line(history: &mut Vec<String>, line: &str) {
    if keep_in_history(line) {
        history.push(line.to_string());
    }
}

/// Where a submitted line (already trimmed) goes. The REPL loop dispatches on this and on nothing else, so the only route that reaches
/// the model is `Parsed(ParseOutcome::NotACommand)`, and a recovery phrase typed as chat can never be that one.
enum LineRoute {
    /// Nothing typed: ask again.
    Blank,
    /// `exit` or `quit` without a slash.
    Exit,
    /// The line is the 24-word recovery phrase on its own (`commands::is_bare_phrase`): a fixed card, never the model, never echoed.
    BlockedPhrase,
    /// A command, an unknown command, or chat.
    Parsed(ParseOutcome),
}

fn classify_line(trimmed: &str) -> LineRoute {
    if trimmed.is_empty() {
        LineRoute::Blank
    } else if trimmed == "exit" || trimmed == "quit" {
        LineRoute::Exit
    } else if commands::is_bare_phrase(trimmed) {
        LineRoute::BlockedPhrase
    } else {
        LineRoute::Parsed(commands::parse_command(trimmed))
    }
}

/// What the card says when a rotation is given up (Ctrl+D on the phrase screen or on a question, a "no" to "change it now?"). In
/// `/arca rotate` a phrase was shown and nothing will ever use it, so it says so, like the refusals that wrote nothing; a resume
/// showed no phrase.
fn cancelled_lines(resuming: bool) -> Vec<String> {
    let mut lines = vec![tr!("arca.cancelled")];
    if !resuming {
        lines.push(tr!("arca.rotate_discarded"));
    }
    lines
}

fn rotation_cancelled_card(terminal: &mut CliTerminal, resuming: bool) -> anyhow::Result<()> {
    render_message_card(terminal, "arca", accent_style(), cancelled_lines(resuming).into_iter().map(|line| (line, Style::default())).collect())
}

/// Whether the phrase screen laid out for a terminal of `shown_for` columns x lines may go on, given the size read `now` (`None`
/// when it could not be read). Any change ends it: the rows were worked out once for that size and a smaller terminal cuts them.
fn phrase_screen_still_valid(shown_for: (u16, u16), now: Option<(u16, u16)>) -> bool {
    now == Some(shown_for)
}

/// What the REPL says for a line that is not a command. It names the command by its FIRST word only: the rest of the line may be
/// the 24 words (see `commands::may_hold_a_phrase`), and an unknown command is not worth echoing in full anyway. `/arca rotate ...`
/// with arguments gets a fixed message that says what to do instead.
fn unknown_command_lines(raw: &str) -> String {
    if commands::is_rotate_with_arguments(raw) {
        tr!("arca.rotate_no_arguments")
    } else {
        tr!("command.unknown", command = raw.split_whitespace().next().unwrap_or(""))
    }
}

/// Prompts for one wizard field (`/models add`, `/agents create`, ...): a bordered input box
/// titled `title`, pre-filled with `initial` so pressing Enter with no edits accepts the current/
/// default value verbatim. No history recall (a fresh `LineEditor` with an empty history list) —
/// a one-off field isn't the kind of thing worth recalling later. `Ctrl+D` on an empty field
/// returns `LineOutcome::Exit`, which the caller treats as "cancel this field/wizard", not "quit
/// the REPL" (see `drive_line_editor`'s doc comment).
async fn read_field(terminal: &mut CliTerminal, title: &str, initial: &str) -> anyhow::Result<LineOutcome> {
    let mut editor = LineEditor::new(Vec::new());
    for c in initial.chars() {
        editor.insert(c);
    }
    drive_line_editor(terminal, &mut editor, title, false).await
}

/// Same as `read_field`, for a secret: empty box, a dot per character, nothing kept in any history.
async fn read_secret_field(terminal: &mut CliTerminal, title: &str) -> anyhow::Result<LineOutcome> {
    let mut editor = LineEditor::new_masked();
    drive_line_editor(terminal, &mut editor, title, false).await
}

/// Hands an SSH approval request from the turn's task (where the tool runs) to the loop in
/// `run_turn` (which owns the terminal), and waits for the answer. Anything that goes wrong on the
/// way — the loop is gone, the reply channel is dropped — is a "no".
struct ChannelApprover {
    requests: mpsc::UnboundedSender<AskedApproval>,
}

/// One question for the person: what is asked, what an "always" would cover (`None` when it can't be offered), and where the answer goes.
type AskedApproval = (ApprovalRequest, Option<String>, oneshot::Sender<Answer>);

#[async_trait::async_trait]
impl Approver for ChannelApprover {
    async fn approve(&self, request: ApprovalRequest) -> bool {
        self.ask(request, None).await != Answer::Reject
    }

    async fn ask(&self, request: ApprovalRequest, always: Option<&str>) -> Answer {
        let (reply, answer) = oneshot::channel();
        if self.requests.send((request, always.map(str::to_string), reply)).is_err() {
            return Answer::Reject;
        }
        answer.await.unwrap_or(Answer::Reject)
    }
}

/// What a key means in the approval card: `s`/`y` approves once; `a` approves and remembers, but only when an "always" was offered;
/// `n`, Esc or Enter refuses; any other key is `None` (ignored) so a stray keystroke can't approve anything.
fn approval_key(code: KeyCode, can_always: bool) -> Option<Answer> {
    match code {
        KeyCode::Char('s' | 'S' | 'y' | 'Y') => Some(Answer::Once),
        KeyCode::Char('a' | 'A') if can_always => Some(Answer::Always),
        KeyCode::Char('n' | 'N') | KeyCode::Esc | KeyCode::Enter => Some(Answer::Reject),
        _ => None,
    }
}

/// Shows what the model wants to run and waits for a key (`approval_key`); Ctrl+C refuses too. Gives up
/// (as a "no") if the tool stopped waiting first — its own deadline elapsed and it dropped the reply.
fn ask_approval(terminal: &mut CliTerminal, request: &ApprovalRequest, always: Option<&str>, reply: &oneshot::Sender<Answer>) -> anyhow::Result<Answer> {
    // One card row per line of the detail, so a multi-line command or a whole persona stays readable.
    let mut lines = vec![
        (tr!("approval.target", target = request.target), Style::default()),
        (tr!("approval.action", action = request.action), Style::default()),
    ];
    if let Some(category) = request.category {
        lines.push((tr!("approval.category", category = category.as_str()), Style::default()));
    }
    lines.extend(request.detail.lines().map(|line| (line.to_string(), Style::default())));
    match always {
        Some(covers) => lines.push((tr!("approval.ask_always", covers = covers), dim_style())),
        None => lines.push((tr!("approval.ask"), dim_style())),
    }
    render_message_card(terminal, &tr!("approval.title"), Style::default().fg(Color::Rgb(241, 196, 15)), lines)?;
    loop {
        if reply.is_closed() {
            return Ok(Answer::Reject);
        }
        terminal.draw(|frame| render_card_preview(frame, PreviewState::AwaitingApproval))?;
        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key_is_ctrl_c(&key) {
                    return Ok(Answer::Reject);
                }
                if let Some(answer) = approval_key(key.code, always.is_some()) {
                    return Ok(answer);
                }
            }
        }
    }
}

/// Runs one full turn: spawns the streaming call, grows a live bordered preview (`render_card_
/// preview`) as content arrives, then — once the reply is complete — turns it into a permanent
/// message card (`insert_card`) with full markdown styling and a token-count footer. Returns
/// `Ok(None)` if the user interrupted with Ctrl+C — nothing gets appended to conversation history
/// in that case, matching a turn that never happened. Draws into the caller's long-lived
/// `Terminal` (though it may swap it out for a taller/shorter one via `ensure_preview_height` —
/// see that function's doc comment for why rebuilding mid-turn is safe here).
async fn run_turn(
    orchestrator: &Orchestrator,
    history: &[Message],
    input: &str,
    terminal: &mut CliTerminal,
    model_override: Option<Arc<dyn ModelProvider>>,
    system_prompt: Option<&str>,
    grants: Grants,
) -> anyhow::Result<Option<MessageOutcome>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let orchestrator = match model_override {
        Some(model) => orchestrator.with_model(model),
        None => orchestrator.clone(),
    };
    // Tools that need a human "yes" (`ssh_*` on a host with `require_approval`) ask through here.
    // P122: an "always" to a tool call holds for this agent, so the same kind of action isn't asked again.
    let (approval_tx, mut approval_rx) = mpsc::unbounded_channel::<AskedApproval>();
    let orchestrator = orchestrator.with_approver(Arc::new(RememberingApprover::new(Arc::new(ChannelApprover { requests: approval_tx }), grants)));
    let history_owned = history.to_vec();
    let input_owned = input.to_string();
    let system_prompt_owned = system_prompt.map(str::to_string);
    let handle = tokio::spawn(async move {
        orchestrator
            .handle_turn_streaming(&history_owned, &input_owned, Vec::new(), system_prompt_owned.as_deref(), move |event| {
                let _ = tx.send(event.clone());
            })
            .await
    });

    let start = Instant::now();
    let mut spinner_frame = 0usize;
    // Raw text received so far — re-parsed for markdown as a whole once the reply completes
    // (`markdown_body_rows`), not line-by-line as it streams in. Shown live, plainly wrapped and
    // capped to `MAX_PREVIEW_ROWS`, via `render_card_preview` in the meantime.
    let mut pending = String::new();
    let mut current_height = VIEWPORT_HEIGHT;

    let warden_title = vec![("● ".to_string(), Style::default().fg(Color::Rgb(46, 204, 113))), ("Warden".to_string(), Style::default())];
    let card_border = Style::default().fg(accent_color());
    let dim_style = Style::default().add_modifier(Modifier::DIM);

    // Not a `tokio::select!` over `rx`/a ticker/key events/`handle` — deliberately: this whole
    // turn just needs to notice new state at human-perceptible speed (a spinner frame, a Ctrl+C
    // keypress), and `event::poll`'s bounded wait below already paces the loop at that speed, so
    // it doubles as the tick. See the module doc comment for why key events specifically can't be
    // read via an async `EventStream` here.
    loop {
        while let Ok(event) = rx.try_recv() {
            if let StreamEvent::ContentDelta(delta) = event {
                pending.push_str(&delta);
            }
        }

        if let Ok((request, always, reply)) = approval_rx.try_recv() {
            let answer = ask_approval(terminal, &request, always.as_deref(), &reply)?;
            let _ = reply.send(answer);
            continue;
        }

        if handle.is_finished() {
            let result = handle.await;
            // Reset the preview's height back to the baseline *before* the fallible unwrap below
            // — `?` short-circuits immediately on an error, and the caller (`run`, which prints
            // the error) always expects the terminal to be back at `VIEWPORT_HEIGHT` for the next
            // `read_line`, whether this turn ended in success or failure.
            ensure_preview_height(terminal, &mut current_height, 0)?;
            let outcome = result??;
            // P79 — the turn's provider was down and a reserve answered.
            for switch in &outcome.fallbacks {
                insert_history_line(terminal, &tr!("turn.fallback", to = switch.to, from = switch.from, reason = switch.reason), dim_style)?;
            }
            let footer = outcome.usage.as_ref().map(|usage| {
                vec![(tr!("turn.usage", prompt = usage.prompt_tokens, completion = usage.completion_tokens, total = usage.total_tokens), dim_style)]
            });
            insert_card(terminal, warden_title, card_border, markdown_body_rows(&outcome.content), footer)?;
            terminal.insert_before(1, |_buf| {})?;
            return Ok(Some(outcome));
        }

        let preview_lines = wrap_text(&pending, card_content_width() as u16);
        ensure_preview_height(terminal, &mut current_height, if pending.is_empty() { 0 } else { preview_lines.len() })?;
        if pending.is_empty() {
            spinner_frame = spinner_frame.wrapping_add(1);
            terminal.draw(|frame| render_card_preview(frame, PreviewState::Thinking { elapsed: start.elapsed(), spinner_frame }))?;
        } else {
            let tail_start = preview_lines.len().saturating_sub(MAX_PREVIEW_ROWS);
            terminal.draw(|frame| render_card_preview(frame, PreviewState::Streaming { lines: &preview_lines[tail_start..] }))?;
        }

        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key_is_ctrl_c(&key) {
                    handle.abort();
                    ensure_preview_height(terminal, &mut current_height, 0)?;
                    insert_history_line(terminal, &tr!("turn.interrupted"), dim_style)?;
                    insert_history_line(terminal, "", Style::default())?;
                    return Ok(None);
                }
            }
        }
    }
}

/// Same purple as the desktop app's `--color-accent` in dark mode (`desktop/src/App.css`) — the
/// one brand color this project actually has, reused here instead of the unrelated orange/yellow
/// this module picked ad hoc before. Terminals default to a dark background far more often than
/// light, so the dark-mode shade (lighter, more legible on black) is the one used unconditionally
/// — there's no terminal-side equivalent of the app's light/dark media query to key off of. Kept
/// as plain `(r, g, b)` rather than either color type below, since it's shared between two
/// different color APIs: `owo_colors::OwoColorize::truecolor` for the plain `println!`ed banner,
/// and `ratatui::style::Color::Rgb` for the bordered boxes drawn through `ratatui`.
const BRAND: (u8, u8, u8) = (167, 139, 250);

/// Clears the visible screen *and* the terminal's scrollback (`ESC[3J` — supported by every
/// terminal this project targets: xterm, Alacritty, Kitty, Wezterm, GNOME Terminal, Windows
/// Terminal), then homes the cursor. Called once, before anything else is printed, so opening
/// Warden feels like starting a session — not a wall of old shell history with a banner tacked on
/// below it. Confirmed against a real pty that this doesn't disturb the input box's cursor-query
/// dance from the module doc comment: it runs before raw mode / the `Terminal` even exist.
fn clear_screen() -> io::Result<()> {
    print!("\x1b[2J\x1b[3J\x1b[H");
    io::stdout().flush()
}

fn print_banner() {
    let (r, g, b) = BRAND;
    println!("{}", "Warden".truecolor(r, g, b).bold());
    println!("{}", tr!("banner.tagline").dimmed());
    // A thin rule, sized to the real terminal width (not a hardcoded guess) — separates the
    // one-time header from the conversation that scrolls below it, so the top of the screen reads
    // as a session's title bar rather than two unrelated lines of text.
    let width = crossterm::terminal::size().map(|(cols, _)| cols).unwrap_or(60) as usize;
    println!("{}", "─".repeat(width).truecolor(r, g, b).dimmed());
    println!();
}

/// Per-session state for the slash-commands (`/models`, `/agents`, `/usage`) — lives only for the
/// lifetime of one `run()` call, on top of (not persisted like) `config.toml` itself. `provider_id`/
/// `agent_id` hold only chosen *ids*, never a resolved `Arc<dyn ModelProvider>` or persona string:
/// `resolve_turn_context` re-reads the config fresh from disk before every turn that needs it, the
/// same "never cache a resolved object across calls" pattern the desktop's `send_message` IPC
/// command already uses to avoid staleness after a provider/agent is renamed or removed mid-session.
/// `usage_total`/`turn_count` are the one piece of state here that's genuinely session-local (no
/// config-file equivalent to re-read) — accumulated in `run()`'s loop after every completed turn.
struct CliSession {
    config_path: Option<PathBuf>,
    /// `--vault-path` as passed on the command line, if any (P37's `/sync` commands need to
    /// resolve the same vault the running `Orchestrator` already uses, but `Orchestrator` itself
    /// doesn't expose its `Vault`'s root path back out — this mirrors `bootstrap()`'s own
    /// override-then-config-then-default precedence independently, see `resolve_vault_path`).
    vault_path_override: Option<String>,
    provider_id: Option<String>,
    agent_id: Option<String>,
    usage_total: Usage,
    turn_count: usize,
    /// When this session opened — `/usage` asks the spending ledger for what the `cli` channel
    /// spent from here on, since that is where each call is priced by the model it ran on.
    started_at: u64,
    /// Every tool the running orchestrator has — what an agent's `allowed_tools` is checked against
    /// in the `/agents` wizard, which has no orchestrator of its own.
    tool_names: Vec<String>,
    /// The spending ledger (P4) `/limits` reads and `/extend` grants against — the very one the
    /// orchestrator checks each turn, so what these commands show is what a turn will hit. `None`
    /// when no limit is in force.
    spend_guard: Option<Arc<SpendGuard>>,
    /// The folder of this computer the session works in (P102), picked with `/folder` before the first message: the
    /// file tools and the shell act there, and the shell asks before every command. Never changed after the first turn.
    workdir: Option<String>,
    /// What the person said "always" to in this terminal (P122), per agent: kept in `grants/cli.json` beside the config unless
    /// `persist_grants = false`, and in memory only otherwise.
    grants: GrantBook,
    /// Set by a command that changed what the orchestrator was built from (a pull that brought a new `config.toml` or an
    /// MCP login, P54); the loop rebuilds the orchestrator once the command is done (`reload_if_requested`).
    reload_requested: bool,
}

impl CliSession {
    /// Takes from `orchestrator` what the commands read off it: its tool names and its spending ledger. Called when the
    /// session opens and again whenever the orchestrator is rebuilt.
    fn refresh_from(&mut self, orchestrator: &Orchestrator) {
        self.tool_names = orchestrator.tools().iter().map(|t| t.spec().name).collect();
        self.spend_guard = orchestrator.spend_guard().cloned();
    }
}

/// Rebuilds the orchestrator when a command asked for it (`CliSession::reload_requested`), so a `config.toml` or an MCP
/// login a pull brought is seen at once instead of at the next start. `None` when nothing was asked. If the rebuild fails
/// the orchestrator the session had stays as it was, and the request is cleared so the failure is reported once.
async fn reload_if_requested<F, Fut>(session: &mut CliSession, orchestrator: &mut Orchestrator, rebuild: &F) -> Option<anyhow::Result<()>>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Orchestrator>>,
{
    if !std::mem::take(&mut session.reload_requested) {
        return None;
    }
    Some(rebuild().await.map(|rebuilt| {
        session.refresh_from(&rebuilt);
        *orchestrator = rebuilt;
    }))
}

/// The one conversation the terminal has, as far as the "always" book goes: it keeps no conversations of its own, so an "always"
/// holds for the agent, whatever the session.
const CLI_CONVERSATION: &str = "cli";

/// The terminal's book of "always": in a file beside the config (`persist_grants`, `grant_ttl_days`), or in memory when the config says
/// not to keep them or there is no config path to put a file beside.
fn cli_grant_book(config_path: Option<&Path>) -> GrantBook {
    match config_path.and_then(|path| warden_bootstrap::grant_policy_at(path, false)) {
        Some(policy) => GrantBook::persistent(policy.dir.join("cli.json"), policy.ttl),
        None => GrantBook::default(),
    }
}

/// Uses `warden_bootstrap::resolve_vault_path` (P61) — needed here because `Orchestrator` doesn't
/// expose its `Vault`'s root back out, and `/sync` needs the exact same path the running session
/// already reads/writes. Previously reimplemented the same override-then-config-then-default
/// precedence by hand; now shares the one implementation `bootstrap()` itself uses.
fn resolve_vault_path(config_path: Option<&Path>, vault_path_override: Option<&str>) -> anyhow::Result<PathBuf> {
    let config = load_fresh_config(config_path)?;
    let overrides = Overrides { vault_path: vault_path_override.map(String::from), ..Default::default() };
    Ok(bootstrap_resolve_vault_path(&overrides, &config, PathBuf::from("vault")))
}

/// Builds a `SyncEngine` pointed at the same vault/config this CLI session already uses — called
/// fresh by every `/sync` command (same "never cached" posture as `load_fresh_config`).
fn make_sync_engine(session: &CliSession) -> anyhow::Result<warden_sync::SyncEngine> {
    let vault_path = resolve_vault_path(session.config_path.as_deref(), session.vault_path_override.as_deref())?;
    let config_path = session.config_path.clone().unwrap_or_else(|| PathBuf::from("config.toml"));
    let secrets_path = warden_sync::paths::default_sync_secrets_path().unwrap_or_else(|| PathBuf::from("sync_secrets.json"));
    let manifest_path = warden_sync::paths::default_sync_manifest_path().unwrap_or_else(|| PathBuf::from("sync_manifest.json"));
    Ok(warden_sync::SyncEngine::new(vault_path, config_path, secrets_path, manifest_path).with_oauth_dir(warden_bootstrap::oauth_credentials_dir()))
}

/// Builds a `GitSyncEngine` (P63) — same vault/secrets/manifest paths `make_sync_engine` uses
/// (the two backends share init/pairing/status, only push/pull differ), plus `[git_sync]` read
/// fresh from `config.toml`. Errors clearly if that section isn't configured yet, same "tell the
/// user exactly what to do" posture as `SyncEngine::load_secrets`'s own error.
fn make_git_sync_engine(session: &CliSession) -> anyhow::Result<warden_sync::GitSyncEngine> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    let git_sync = config
        .git_sync
        .ok_or_else(|| anyhow::anyhow!(tr!("sync.git_not_configured")))?;

    let vault_path = resolve_vault_path(session.config_path.as_deref(), session.vault_path_override.as_deref())?;
    let config_path = session.config_path.clone().unwrap_or_else(|| PathBuf::from("config.toml"));
    let secrets_path = warden_sync::paths::default_sync_secrets_path().unwrap_or_else(|| PathBuf::from("sync_secrets.json"));
    let manifest_path = warden_sync::paths::default_sync_manifest_path().unwrap_or_else(|| PathBuf::from("sync_manifest.json"));
    let git_repo_path = warden_sync::paths::default_git_sync_repo_path().unwrap_or_else(|| PathBuf::from("git-sync-repo"));
    Ok(warden_sync::GitSyncEngine::new(vault_path, config_path, secrets_path, manifest_path, git_repo_path, git_sync.remote_url, git_sync.token)
        .with_oauth_dir(warden_bootstrap::oauth_credentials_dir()))
}

/// Reads `config.toml` fresh — never cached across turns/commands (see `CliSession`'s doc
/// comment). A missing file (no path configured, or a path that doesn't exist yet — e.g. before
/// the very first `/models add`) is treated as an empty config, not an error; a malformed file
/// still surfaces as one.
fn load_fresh_config(config_path: Option<&Path>) -> anyhow::Result<FileConfig> {
    match config_path {
        Some(path) => load_config_from_path(path, false),
        None => Ok(FileConfig::default()),
    }
}

/// Renders one card as a `/models`/`/agents` command's output (a list, a confirmation, or an
/// error) — same card machinery (`insert_card`) and trailing spacer as the user/assistant/error
/// cards in `run()`'s own loop, wrapped through `list_body_rows` so a long line never overflows
/// the card's border.
/// How long leaving waits for agents still answering a `message_agent` note (P87) — the same
/// wait the tool's own `wait: true` gives.
const EXIT_WAIT_FOR_AGENTS: Duration = Duration::from_secs(180);

/// A note's answer runs in this process: leaving now would kill it and leave the note unanswered in
/// the conversation. Waits for it (Ctrl+C leaves at once).
async fn wait_for_agents_answering(terminal: &mut CliTerminal) -> anyhow::Result<()> {
    let busy = warden_bootstrap::message_agent::answers_in_flight();
    if busy == 0 {
        return Ok(());
    }
    render_message_card(
        terminal,
        &tr!("notes.title"),
        accent_style(),
        vec![(tr!("notes.waiting", count = busy), Style::default())],
    )?;
    let deadline = std::time::Instant::now() + EXIT_WAIT_FOR_AGENTS;
    while warden_bootstrap::message_agent::answers_in_flight() > 0 && std::time::Instant::now() < deadline {
        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}

fn render_message_card(terminal: &mut CliTerminal, title: &str, style: Style, lines: Vec<(String, Style)>) -> anyhow::Result<()> {
    insert_card(terminal, vec![(title.to_string(), style)], style, list_body_rows(lines), None)?;
    terminal.insert_before(1, |_buf| {})?;
    Ok(())
}

fn accent_style() -> Style {
    Style::default().fg(accent_color())
}

fn error_style() -> Style {
    Style::default().fg(Color::Red)
}

fn dim_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Prompts one wizard field via `read_field`, translating `LineOutcome` into `Option<String>` —
/// `None` means the user cancelled (`Ctrl+D` on an empty field), which every wizard step treats as
/// "abandon the whole wizard", not just this field.
async fn prompt_field(terminal: &mut CliTerminal, title: &str, initial: &str) -> anyhow::Result<Option<String>> {
    match read_field(terminal, title, initial).await? {
        LineOutcome::Submitted(text) => Ok(Some(text)),
        LineOutcome::Exit => Ok(None),
    }
}

struct TurnContext {
    model_override: Option<Arc<dyn ModelProvider>>,
    system_prompt: Option<String>,
    /// The session's orchestrator scoped to the active agent (P46/P72 c): its skills, its own tool
    /// list and the opt-in tools it has flags for — see `warden_bootstrap::scope_to_agent`.
    orchestrator: Orchestrator,
}

/// Resolves what this turn should actually use, given the session's `provider_id`/`agent_id`
/// selections — a model override (only when it differs from whatever `run()`'s own `orchestrator`
/// parameter already is), a persona to pass as `system_prompt`, and (P46) the orchestrator scoped to
/// the active agent (`scope_to_agent`: skills, `allowed_tools`, `delegate_to_agent`, `manage_agents`;
/// and `message_agent`, whose notes land in the desktop's conversations directory, P87). Reloads config fresh only when at least one selection
/// is active; when neither is, returns an empty context immediately with no disk access at
/// all, so a session that never touches `/models`/`/agents` behaves exactly as before this
/// feature existed. An explicit `/models use` always wins over an active agent's own
/// `provider_id` (which only pre-fills when nothing more specific was chosen) — same precedence
/// the desktop's chat header uses between its agent and provider selectors.
fn resolve_turn_context(session: &CliSession, orchestrator: &Orchestrator) -> anyhow::Result<TurnContext> {
    if session.provider_id.is_none() && session.agent_id.is_none() {
        return Ok(TurnContext { model_override: None, system_prompt: None, orchestrator: orchestrator.clone() });
    }

    let config = load_fresh_config(session.config_path.as_deref())?;

    let scoped = session
        .agent_id
        .as_deref()
        .and_then(|id| scope_to_agent(orchestrator, &config, session.config_path.as_deref(), id, cli_agent_extras()));
    let agent_provider_id = scoped.as_ref().and_then(|s| s.provider_id.clone());
    let (system_prompt, scoped_orchestrator) = match scoped {
        Some(scoped) => (Some(scoped.persona), scoped.orchestrator),
        // An agent removed from the file since `/agents use` keeps only its skill scope, as before.
        None => (None, orchestrator.with_agent(session.agent_id.clone())),
    };

    let effective_provider_id = session.provider_id.clone().or(agent_provider_id);

    let model_override = match effective_provider_id {
        Some(provider_id) => Some(build_model_for(&config, &provider_id, None)?),
        None => None,
    };

    Ok(TurnContext { model_override, system_prompt, orchestrator: scoped_orchestrator })
}

/// The terminal keeps no conversations of its own, so `message_agent` notes (P87) go to the
/// desktop's directory: the "A → B" conversation shows up there, and `read` works here. Nothing is
/// told when one changes (the desktop sees it on its next list reload).
fn cli_agent_extras() -> AgentExtras {
    AgentExtras { conversations_dir: warden_bootstrap::default_conversations_dir(), ..AgentExtras::default() }
}

async fn cmd_help(terminal: &mut CliTerminal) -> anyhow::Result<()> {
    let style = Style::default();
    // Each line is the command (never translated) followed by its description (the catalog).
    let lines = [
        ("/exit, /quit", "help.exit"),
        ("/help", "help.help"),
        ("/lang [en|pt-BR]", "help.lang"),
        ("/usage", "help.usage"),
        ("/folder [path|off]", "help.folder"),
        ("/limits", "help.limits"),
        ("/limits add", "help.limits_add"),
        ("/limits edit <id>", "help.limits_edit"),
        ("/limits remove <id>", "help.limits_remove"),
        ("/limits off", "help.limits_off"),
        ("/limits reset", "help.limits_reset"),
        ("/extend <id>", "help.extend"),
        ("/grants", "help.grants"),
        ("/grants revoke <what> [agent]", "help.grants_revoke"),
        ("/prices", "help.prices"),
        ("/prices add", "help.prices_add"),
        ("/prices edit <model>", "help.prices_edit"),
        ("/prices remove <model>", "help.prices_remove"),
        ("/models", "help.models"),
        ("/models use <id>", "help.models_use"),
        ("/models reset", "help.models_reset"),
        ("/models add", "help.models_add"),
        ("/models edit <id>", "help.models_edit"),
        ("/models remove <id>", "help.models_remove"),
        ("/combos", "help.combos"),
        ("/combos add <name> <provider> [<provider>...]", "help.combos_add"),
        ("/combos remove <name>", "help.combos_remove"),
        ("/agents", "help.agents"),
        ("/agents use <id> | none", "help.agents_use"),
        ("/agents tree", "help.agents_tree"),
        ("/agents create", "help.agents_create"),
        ("/agents edit <id>", "help.agents_edit"),
        ("/agents remove <id>", "help.agents_remove"),
        ("/skills", "help.skills"),
        ("/skills show <name>", "help.skills_show"),
        ("/skills create", "help.skills_create"),
        ("/skills edit <name>", "help.skills_edit"),
        ("/skills remove <name>", "help.skills_remove"),
        ("/skills path [name]", "help.skills_path"),
        ("/skills file <name> <file>", "help.skills_file"),
        ("/skills attach <name> <file> <path>", "help.skills_attach"),
        ("/skills detach <name> <file>", "help.skills_detach"),
        ("/ssh", "help.ssh"),
        ("/ssh add", "help.ssh_add"),
        ("/ssh edit <id>", "help.ssh_edit"),
        ("/ssh remove <id>", "help.ssh_remove"),
        ("/ssh on <id> | off <id>", "help.ssh_on_off"),
        ("/ssh test <id>", "help.ssh_test"),
        ("/ssh log [n]", "help.ssh_log"),
        ("/sync", "help.sync"),
        ("/sync push", "help.sync_push"),
        ("/sync pull", "help.sync_pull"),
        ("/sync pair", "help.sync_pair"),
        ("/sync pair <code> [ip]", "help.sync_pair_join"),
        ("/sync git push", "help.sync_git_push"),
        ("/sync git pull", "help.sync_git_pull"),
        ("/arca", "help.arca"),
        ("/arca on", "help.arca_on"),
        ("/arca sync", "help.arca_sync"),
        ("/arca ack", "help.arca_ack"),
        ("/arca swap grant", "help.arca_swap_grant"),
        ("/arca swap offer", "help.arca_swap_offer"),
        ("/arca rotate", "help.arca_rotate"),
        ("/arca rotate resume", "help.arca_rotate_resume"),
        ("/arca pair offer", "help.arca_pair_offer"),
        ("/arca pair grant", "help.arca_pair_grant"),
    ]
    .into_iter()
    .map(|(command, description)| (format!("{command} — {}", tr!(description)), style))
    .chain([(tr!("help.syncignore_1"), style), (tr!("help.syncignore_2"), style)])
    .collect();
    render_message_card(terminal, &tr!("help.title"), accent_style(), lines)
}

/// The `$` line of `/usage`. `spent` is `None` when no spending ledger is running (limits are off),
/// which leaves nothing to price the calls with. Prices are only the ones in `[[prices]]`, so a call
/// on a model without one is said out loud instead of silently counted as free.
fn cost_line(spent: Option<&Spent>) -> String {
    let Some(spent) = spent else {
        return tr!("cost.unavailable_no_limits");
    };
    let priced = spent.calls - spent.unpriced_calls;
    match (priced, spent.unpriced_calls) {
        (0, 0) => "$ 0.0000".to_string(),
        (0, unpriced) => tr!("cost.unavailable_unpriced", count = unpriced),
        (_, 0) => tr!("cost.priced", cost = format!("{:.4}", spent.cost_usd)),
        (_, unpriced) => tr!("cost.at_least", cost = format!("{:.4}", spent.cost_usd), count = unpriced),
    }
}

/// `/usage` — what was spent so far in *this* terminal session. Tokens come from
/// `CliSession.usage_total` (reset every process start, never persisted); dollars from the spending
/// ledger, because only there is each call priced by its own model (a sub-agent may run on another).
/// The ledger is per channel, so a second terminal open at the same time counts here too.
async fn cmd_usage(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    if session.turn_count == 0 {
        return render_message_card(terminal, &tr!("usage.title"), accent_style(), vec![(tr!("usage.none_yet"), Style::default())]);
    }
    let usage = &session.usage_total;
    let spent = session.spend_guard.as_ref().map(|guard| guard.spent_since("cli", session.started_at));
    let lines = vec![
        (tr!("usage.messages", count = session.turn_count), Style::default()),
        (tr!("usage.prompt_tokens", count = usage.prompt_tokens), Style::default()),
        (tr!("usage.completion_tokens", count = usage.completion_tokens), Style::default()),
        (tr!("usage.total_tokens", count = usage.total_tokens), Style::default()),
        (cost_line(spent.as_ref()), Style::default()),
    ];
    render_message_card(terminal, &tr!("usage.title"), accent_style(), lines)
}

/// One limit as `/limits` shows it: how much of each ceiling is used, what is left, and when room
/// starts coming back. Reads out the same numbers `LimitStatus::describe` gives the model.
fn limit_line(status: &LimitStatus) -> String {
    let mut parts = Vec::new();
    if let Some(max) = status.max_tokens {
        parts.push(tr!("limit.tokens", used = status.used_tokens, max = max, left = status.remaining_tokens.unwrap_or(0)));
    }
    if let Some(max) = status.max_cost_usd {
        parts.push(tr!(
            "limit.cost",
            used = format!("{:.4}", status.used_cost_usd),
            max = format!("{max:.2}"),
            left = format!("{:.4}", status.remaining_cost_usd.unwrap_or(0.0))
        ));
    }
    let mut line = tr!(
        "limit.line",
        id = status.id,
        scope = status.scope,
        hours = status.window_hours,
        percent = format!("{:.0}", status.fraction * 100.0),
        parts = parts.join("; ")
    );
    if status.unpriced_calls > 0 && status.max_cost_usd.is_some() {
        line.push_str(&tr!("limit.underestimated", count = status.unpriced_calls));
    }
    if let Some(min) = status.frees_up_in_minutes {
        line.push_str(&tr!("limit.frees_up", minutes = min));
    }
    line
}

/// `/limits` — every configured spending limit and where it stands (P4). Shows all of them, not only
/// those that cover this terminal, so a limit on Telegram or an agent is visible from here too.
async fn cmd_limits(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let Some(guard) = &session.spend_guard else {
        return render_message_card(
            terminal,
            &tr!("limits.title"),
            accent_style(),
            vec![(tr!("limits.none_active_detail"), Style::default())],
        );
    };
    let mut lines: Vec<(String, Style)> = guard
        .status(None)
        .iter()
        .map(|s| {
            let style = if s.exceeded {
                error_style()
            } else if s.warn {
                Style::default().fg(Color::Rgb(241, 196, 15))
            } else {
                Style::default()
            };
            (limit_line(s), style)
        })
        .collect();
    if let Some(error) = guard.last_error() {
        lines.push((tr!("limits.write_warning", error = error), error_style()));
    }
    lines.push((tr!("limits.extend_hint"), dim_style()));
    render_message_card(terminal, &tr!("limits.title"), accent_style(), lines)
}

/// `/extend <id>` — allows one more step on a limit, for the rest of its window (P4). The same thing
/// answering "sim" to a paused turn does, for when the limit is hit outside a turn that could ask
/// (a background bot) or a person wants to make room ahead of time.
async fn cmd_extend(terminal: &mut CliTerminal, session: &CliSession, id: String) -> anyhow::Result<()> {
    let Some(guard) = &session.spend_guard else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limits.none_active"), Style::default())]);
    };
    match guard.extend(&id) {
        Ok(grant) => {
            let mut added = Vec::new();
            if grant.tokens > 0 {
                added.push(tr!("extend.tokens", count = grant.tokens));
            }
            if grant.cost_usd > 0.0 {
                added.push(tr!("extend.cost", amount = format!("{:.2}", grant.cost_usd)));
            }
            render_message_card(
                terminal,
                &tr!("limits.title"),
                accent_style(),
                vec![(tr!("extend.done", id = id, added = added.join(&tr!("extend.and"))), Style::default())],
            )
        }
        Err(err) => render_message_card(
            terminal,
            &tr!("common.error"),
            error_style(),
            vec![(tr!("extend.failed", error = format!("{err:#}")), Style::default())],
        ),
    }
}

/// How long ago something happened, in the same words the web screen uses ("now", "N min ago", "N h ago", "N d ago"), in the current language.
fn ago_label(then_ms: u64, now_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(then_ms) / 60_000;
    match minutes {
        0 => tr!("ago.now"),
        1..=59 => tr!("ago.minutes", count = minutes),
        60..=1439 => tr!("ago.hours", count = minutes / 60),
        _ => tr!("ago.days", count = minutes / 1440),
    }
}

/// The lines `/grants` shows for what was answered "always" in the terminal: one per grant, then what the term is. `None` when there is nothing.
fn grant_lines(grants: &[Grant], ttl_days: Option<u32>, now_ms: u64) -> Option<Vec<(String, Style)>> {
    let mine: Vec<&Grant> = grants.iter().filter(|g| g.conversation == CLI_CONVERSATION).collect();
    if mine.is_empty() {
        return None;
    }
    let mut lines: Vec<(String, Style)> = mine
        .iter()
        .map(|g| {
            let agent = g.agent.clone().unwrap_or_else(|| tr!("grants.default_agent"));
            (tr!("grants.entry", covers = g.covers, agent = agent, used = ago_label(g.last_used_ms, now_ms)), Style::default())
        })
        .collect();
    let term = match ttl_days {
        Some(days) => tr!("grants.expire_after", days = days),
        None => tr!("grants.expire_on_close"),
    };
    lines.push((term, dim_style()));
    lines.push((tr!("grants.revoke_hint"), dim_style()));
    Some(lines)
}

/// `/grants` — what was answered "always" in this terminal (P122), per agent.
async fn cmd_grants_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    match grant_lines(&session.grants.list(), session.grants.ttl_days(), now_millis()) {
        Some(lines) => render_message_card(terminal, &tr!("grants.title"), accent_style(), lines),
        None => render_message_card(terminal, &tr!("grants.title"), accent_style(), vec![(tr!("grants.none"), Style::default())]),
    }
}

/// `/grants revoke <covers> [agent]` — takes one "always" back, so the agent asks again. Without `agent` it is the active one.
async fn cmd_grants_revoke(terminal: &mut CliTerminal, session: &CliSession, covers: String, agent: Option<String>) -> anyhow::Result<()> {
    let agent = match agent {
        Some(named) => Some(named).filter(|a| !a.is_empty()),
        None => session.agent_id.clone(),
    };
    let who = agent.clone().unwrap_or_else(|| tr!("grants.default_agent"));
    if session.grants.revoke(CLI_CONVERSATION, agent.as_deref(), &covers) {
        render_message_card(terminal, &tr!("grants.title"), accent_style(), vec![(tr!("grants.revoked", covers = covers, who = who), Style::default())])
    } else {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("grants.revoke_missing", covers = covers, who = who), Style::default())])
    }
}

// --- `[[limits]]`/`[[prices]]` wizard (P4) ------------------------------------------------------
//
// `/limits`/`/extend` (above) only read the state already in force; what follows writes `config.toml`, in the
// same mold as the SSH wizard (`prompt_ssh_host` and friends, further down in this file): one `prompt_*`
// that builds the whole struct field by field (cancellable at any point, `Esc`/`Ctrl+C`), validated
// for real by `LimitConfig::to_limit()` — the same rule the startup and the desktop screen use —
// and one `wizard_*`/`cmd_*` per command that loads, applies and saves.

/// Loops a single wizard field through a pure parser until it's valid, re-prompting with the
/// parser's own error card — the same shape `prompt_provider_kind` uses for its one `match`,
/// generalized so every numeric field below shares one loop.
async fn prompt_parsed<T>(terminal: &mut CliTerminal, title: &str, initial: &str, parse: impl Fn(&str) -> Result<T, String>) -> anyhow::Result<Option<T>> {
    loop {
        let Some(input) = prompt_field(terminal, title, initial).await? else {
            return Ok(None);
        };
        match parse(&input) {
            Ok(value) => return Ok(Some(value)),
            Err(err) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(err, Style::default())])?,
        }
    }
}

/// `/limits add`/`edit`'s "window (hours)" field: an integer of at least 1.
fn parse_limit_window_hours(input: &str) -> Result<u32, String> {
    match input.trim().parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(tr!("parse.window_hours")),
    }
}

/// A blank "max tokens" field is "no cap" (`None`); anything else must be a positive integer.
fn parse_optional_u64(input: &str) -> Result<Option<u64>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<u64>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(tr!("parse.whole_above_zero")),
    }
}

/// Same shape as `parse_optional_u64` but for a dollar figure (`max_cost_usd`), which may have a
/// decimal point.
fn parse_optional_cost(input: &str) -> Result<Option<f64>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<f64>() {
        Ok(n) if n.is_finite() && n > 0.0 => Ok(Some(n)),
        _ => Err(tr!("parse.above_zero")),
    }
}

/// A model price (`$`/1M tokens) is the same shape as a cost, but `0` is valid — a free/local
/// model — and it's never optional (both prices are always asked).
fn parse_price_amount(input: &str) -> Result<f64, String> {
    match input.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && n >= 0.0 => Ok(n),
        _ => Err(tr!("parse.zero_or_more")),
    }
}

/// `warn_at`/`extend_step` as typed in percent (1-100); blank keeps the default. Converts to the
/// 0-1 fraction the config stores — same convention as the desktop's `PercentField`.
fn parse_optional_percent(input: &str) -> Result<Option<f64>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<f64>() {
        Ok(n) if n.is_finite() && n > 0.0 && n <= 100.0 => Ok(Some(n / 100.0)),
        _ => Err(tr!("parse.percent")),
    }
}

fn limit_scope_label(scope: LimitScope) -> &'static str {
    match scope {
        LimitScope::Global => "global",
        LimitScope::Agent => "agent",
        LimitScope::Channel => "channel",
        LimitScope::User => "user",
        LimitScope::Person => "person",
    }
}

fn parse_limit_scope(input: &str) -> Result<LimitScope, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "global" => Ok(LimitScope::Global),
        "agent" => Ok(LimitScope::Agent),
        "channel" => Ok(LimitScope::Channel),
        "user" => Ok(LimitScope::User),
        "person" => Ok(LimitScope::Person),
        _ => Err(tr!("parse.scope")),
    }
}

/// Validates the target text against the scope it goes with (`LimitConfig::target`): `global`
/// takes none, `agent` must name a configured agent, `channel` just can't be blank, `user` needs
/// the `channel:user` shape `to_limit()` itself checks again later. The real cross-field rule
/// stays in `LimitConfig::to_limit()` — this only turns typed text into a typed value with an
/// error in the wizard's voice.
fn validate_limit_target(scope: LimitScope, target: &str, known_agents: &[String]) -> Result<Option<String>, String> {
    let trimmed = target.trim();
    match scope {
        LimitScope::Global => Ok(None),
        LimitScope::Agent if trimmed.is_empty() => Err(tr!("target.agent_id")),
        LimitScope::Agent if !known_agents.iter().any(|a| a == trimmed) => Err(tr!("target.agent_missing", id = trimmed)),
        LimitScope::Agent => Ok(Some(trimmed.to_string())),
        LimitScope::Channel if trimmed.is_empty() => Err(tr!("target.channel")),
        LimitScope::Channel => Ok(Some(trimmed.to_string())),
        LimitScope::User if !trimmed.contains(':') => Err(tr!("target.user")),
        LimitScope::User => Ok(Some(trimmed.to_string())),
        LimitScope::Person if trimmed.is_empty() => Err(tr!("target.person")),
        LimitScope::Person => Ok(Some(trimmed.to_string())),
    }
}

/// The effective `[[limits]]` list a wizard edit should start from: the file's own list when it
/// has one, or the built-in safety net materialized into real entries when it's `None` — so
/// `/limits add`/`edit` on a config that never customized limits never silently drops the default
/// protection (same intent as the desktop's "Customize limits" button, done automatically since
/// there's no single screen here to show both cards for review first).
fn effective_limit_configs(config: &FileConfig) -> Vec<LimitConfig> {
    config.limits.clone().unwrap_or_else(default_limit_configs)
}

/// Asks every field of a spending limit, pre-filled from `current` when editing. `existing` is the
/// full effective list (including `current` itself, when editing) — uniqueness is checked the same
/// way `prompt_ssh_host`/`prompt_provider_id` do it, against everything but the entry being kept.
/// `Ok(None)` means cancelled or a real validation error already rendered as a card. The final say
/// is always `LimitConfig::to_limit()`, run once more on the assembled value.
async fn prompt_limit(terminal: &mut CliTerminal, config: &FileConfig, existing: &[LimitConfig], current: Option<&LimitConfig>) -> anyhow::Result<Option<LimitConfig>> {
    let keep_id = current.map(|l| l.id.as_str());
    let id = loop {
        let Some(input) = prompt_field(terminal, &tr!("limit_prompt.id"), keep_id.unwrap_or("")).await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if candidate.is_empty() {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limit_prompt.id_blank"), Style::default())])?;
        } else if Some(candidate.as_str()) != keep_id && existing.iter().any(|l| l.id == candidate) {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limit_prompt.id_taken", id = candidate), Style::default())])?;
        } else {
            break candidate;
        }
    };

    let known_agents: Vec<String> = config.agents.iter().map(|a| a.id.clone()).collect();
    let (scope, target) = loop {
        let Some(scope_input) = prompt_field(terminal, &tr!("limit_prompt.scope"), current.map_or("global", |l| limit_scope_label(l.scope))).await? else {
            return Ok(None);
        };
        let scope = match parse_limit_scope(&scope_input) {
            Ok(s) => s,
            Err(err) => {
                render_message_card(terminal, &tr!("common.error"), error_style(), vec![(err, Style::default())])?;
                continue;
            }
        };
        if scope == LimitScope::Global {
            break (scope, None);
        }
        let hint = match scope {
            LimitScope::Agent if !known_agents.is_empty() => tr!("limit_prompt.target_agent", agents = known_agents.join(", ")),
            LimitScope::Agent => tr!("limit_prompt.target_no_agents"),
            LimitScope::Channel => tr!("limit_prompt.target_channel"),
            LimitScope::User => tr!("limit_prompt.target_user"),
            LimitScope::Person => tr!("limit_prompt.target_person"),
            LimitScope::Global => unreachable!("handled above"),
        };
        let Some(target_input) = prompt_field(terminal, &hint, current.and_then(|l| l.target.as_deref()).unwrap_or("")).await? else {
            return Ok(None);
        };
        match validate_limit_target(scope, &target_input, &known_agents) {
            Ok(target) => break (scope, target),
            Err(err) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(err, Style::default())])?,
        }
    };

    let Some(window_hours) = prompt_parsed(terminal, &tr!("limit_prompt.window"), &current.map_or(24, |l| l.window_hours).to_string(), parse_limit_window_hours).await? else {
        return Ok(None);
    };
    let Some(max_tokens) = prompt_parsed(
        terminal,
        &tr!("limit_prompt.max_tokens"),
        &current.and_then(|l| l.max_tokens).map(|n| n.to_string()).unwrap_or_default(),
        parse_optional_u64,
    )
    .await?
    else {
        return Ok(None);
    };
    let Some(max_cost_usd) = prompt_parsed(
        terminal,
        &tr!("limit_prompt.max_cost"),
        &current.and_then(|l| l.max_cost_usd).map(|n| n.to_string()).unwrap_or_default(),
        parse_optional_cost,
    )
    .await?
    else {
        return Ok(None);
    };
    if max_tokens.is_none() && max_cost_usd.is_none() {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limit_prompt.need_a_cap"), Style::default())])?;
        return Ok(None);
    }
    let Some(warn_at) = prompt_parsed(
        terminal,
        &tr!("limit_prompt.warn_at", percent = format!("{:.0}", DEFAULT_WARN_AT * 100.0)),
        &current.and_then(|l| l.warn_at).map(|n| format!("{:.0}", n * 100.0)).unwrap_or_default(),
        parse_optional_percent,
    )
    .await?
    else {
        return Ok(None);
    };
    let Some(extend_step) = prompt_parsed(
        terminal,
        &tr!("limit_prompt.extend_step", percent = format!("{:.0}", DEFAULT_EXTEND_STEP * 100.0)),
        &current.and_then(|l| l.extend_step).map(|n| format!("{:.0}", n * 100.0)).unwrap_or_default(),
        parse_optional_percent,
    )
    .await?
    else {
        return Ok(None);
    };

    let entry = LimitConfig { id, scope, target, window_hours, max_tokens, max_cost_usd, warn_at, extend_step };
    if let Err(err) = entry.to_limit() {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(format!("{err:#}"), Style::default())])?;
        return Ok(None);
    }
    Ok(Some(entry))
}

/// `what_key` names the thing that was cancelled (a catalog key: creation, edit, removal...).
fn limits_cancelled(terminal: &mut CliTerminal, what_key: &str) -> anyhow::Result<()> {
    render_message_card(terminal, &tr!("limits.title"), dim_style(), vec![(tr!(what_key), dim_style())])
}

async fn wizard_limits_add(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let materialized = config.limits.is_none();
    let mut limits = effective_limit_configs(&config);

    let Some(limit) = prompt_limit(terminal, &config, &limits, None).await? else {
        return limits_cancelled(terminal, "cancelled.creation");
    };
    let id = limit.id.clone();
    limits.push(limit);
    config.limits = Some(limits);

    save_config_and_reload(session, &config).await?;
    let mut lines = vec![(tr!("limits.created", id = id), Style::default())];
    if materialized {
        lines.push((tr!("limits.kept_safety_net"), dim_style()));
    }
    render_message_card(terminal, &tr!("limits.title"), accent_style(), lines)
}

async fn wizard_limits_edit(terminal: &mut CliTerminal, session: &mut CliSession, target_id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let materialized = config.limits.is_none();
    let mut limits = effective_limit_configs(&config);
    let Some(index) = limits.iter().position(|l| l.id == target_id) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limits.not_found", id = target_id), Style::default())]);
    };
    let current = limits[index].clone();

    let Some(updated) = prompt_limit(terminal, &config, &limits, Some(&current)).await? else {
        return limits_cancelled(terminal, "cancelled.edit");
    };
    let new_id = updated.id.clone();
    limits[index] = updated;
    config.limits = Some(limits);

    save_config_and_reload(session, &config).await?;
    let mut lines = vec![(tr!("limits.updated", id = new_id), Style::default())];
    if materialized {
        lines.push((tr!("limits.kept_defaults"), dim_style()));
    }
    render_message_card(terminal, &tr!("limits.title"), accent_style(), lines)
}

async fn cmd_limits_remove(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let mut limits = effective_limit_configs(&config);
    if !limits.iter().any(|l| l.id == id) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limits.not_found", id = id), Style::default())]);
    }
    let Some(answer) = prompt_field(terminal, &tr!("limits.confirm_remove", id = id), "").await? else {
        return limits_cancelled(terminal, "cancelled.removal");
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return limits_cancelled(terminal, "cancelled.removal");
    }
    limits.retain(|l| l.id != id);
    config.limits = Some(limits);
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("limits.title"), accent_style(), vec![(tr!("limits.removed", id = id), Style::default())])
}

/// `/limits off` — the riskiest of the bunch (drops every ceiling, so nothing stops a runaway
/// agent), hence the confirmation and the explicit warning in the prompt itself, not just the
/// generic "(s/n)" the other confirmations use.
async fn cmd_limits_off(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if config.limits.as_ref().is_some_and(Vec::is_empty) {
        return render_message_card(terminal, &tr!("limits.title"), accent_style(), vec![(tr!("limits.already_off"), Style::default())]);
    }
    let Some(answer) = prompt_field(terminal, &tr!("limits.confirm_off"), "").await? else {
        return limits_cancelled(terminal, "cancelled.disable");
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return limits_cancelled(terminal, "cancelled.disable");
    }
    config.limits = Some(Vec::new());
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("limits.title"), accent_style(), vec![(tr!("limits.all_off"), Style::default())])
}

/// `/limits reset` — back to the built-in safety net (`limits` field removed from the file, not
/// written as `[]`), discarding any customized list.
async fn cmd_limits_reset(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if config.limits.is_none() {
        return render_message_card(terminal, &tr!("limits.title"), accent_style(), vec![(tr!("limits.already_default"), Style::default())]);
    }
    let Some(answer) = prompt_field(terminal, &tr!("limits.confirm_reset"), "").await? else {
        return limits_cancelled(terminal, "cancelled.restore");
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return limits_cancelled(terminal, "cancelled.restore");
    }
    config.limits = None;
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("limits.title"), accent_style(), vec![(tr!("limits.reset_done"), Style::default())])
}

/// Asks the two fields of a `[[prices]]` entry, `model` kept fixed when editing (same
/// keep-if-same-as-current convention `prompt_limit`'s id uses). `0` is a valid price here — a
/// free/local model — unlike a limit's ceilings.
async fn prompt_price(terminal: &mut CliTerminal, existing: &[Price], current: Option<&Price>) -> anyhow::Result<Option<Price>> {
    let keep_model = current.map(|p| p.model.as_str());
    let model = loop {
        let Some(input) = prompt_field(terminal, &tr!("price_prompt.model"), keep_model.unwrap_or("")).await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if candidate.is_empty() {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("price_prompt.model_blank"), Style::default())])?;
        } else if Some(candidate.as_str()) != keep_model && existing.iter().any(|p| p.model == candidate) {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("price_prompt.model_taken", model = candidate), Style::default())])?;
        } else {
            break candidate;
        }
    };
    let Some(input_per_mtok) = prompt_parsed(terminal, &tr!("price_prompt.input"), &current.map_or(String::new(), |p| p.input_per_mtok.to_string()), parse_price_amount).await? else {
        return Ok(None);
    };
    let Some(output_per_mtok) = prompt_parsed(terminal, &tr!("price_prompt.output"), &current.map_or(String::new(), |p| p.output_per_mtok.to_string()), parse_price_amount).await? else {
        return Ok(None);
    };
    Ok(Some(Price { model, input_per_mtok, output_per_mtok }))
}

async fn cmd_prices_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    if config.prices.is_empty() {
        return render_message_card(terminal, &tr!("prices.title"), accent_style(), vec![(tr!("prices.none"), Style::default())]);
    }
    let lines = config
        .prices
        .iter()
        .map(|p| {
            let line = tr!("prices.entry", model = p.model, input = format!("{:.4}", p.input_per_mtok), output = format!("{:.4}", p.output_per_mtok));
            (line, Style::default())
        })
        .collect();
    render_message_card(terminal, &tr!("prices.title"), accent_style(), lines)
}

async fn wizard_prices_add(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(price) = prompt_price(terminal, &config.prices, None).await? else {
        return render_message_card(terminal, &tr!("prices.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
    };
    let model = price.model.clone();
    config.prices.push(price);
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("prices.title"), accent_style(), vec![(tr!("prices.added", model = model), Style::default())])
}

async fn wizard_prices_edit(terminal: &mut CliTerminal, session: &mut CliSession, target_model: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(index) = config.prices.iter().position(|p| p.model == target_model) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("prices.not_found_list", model = target_model), Style::default())]);
    };
    let current = config.prices[index].clone();
    let Some(updated) = prompt_price(terminal, &config.prices, Some(&current)).await? else {
        return render_message_card(terminal, &tr!("prices.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
    };
    let model = updated.model.clone();
    config.prices[index] = updated;
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("prices.title"), accent_style(), vec![(tr!("prices.updated", model = model), Style::default())])
}

async fn cmd_prices_remove(terminal: &mut CliTerminal, session: &mut CliSession, model: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if !config.prices.iter().any(|p| p.model == model) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("prices.not_found", model = model), Style::default())]);
    }
    let Some(answer) = prompt_field(terminal, &tr!("prices.confirm_remove", model = model), "").await? else {
        return render_message_card(terminal, &tr!("prices.title"), dim_style(), vec![(tr!("prices.remove_cancelled"), dim_style())]);
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return render_message_card(terminal, &tr!("prices.title"), dim_style(), vec![(tr!("prices.remove_cancelled"), dim_style())]);
    }
    config.prices.retain(|p| p.model != model);
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, &tr!("prices.title"), accent_style(), vec![(tr!("prices.removed", model = model), Style::default())])
}

fn provider_display_model(provider: &ProviderConfig) -> String {
    provider.model.clone().or_else(|| default_model_for(provider.kind).map(str::to_string)).unwrap_or_else(|| "?".to_string())
}

async fn cmd_models_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    if config.providers.is_empty() {
        return render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.none"), Style::default())]);
    }
    let active = session.provider_id.clone().or_else(|| config.active_provider.clone());
    let active_marker = tr!("marker.active");
    let marker = |id: &str| if active.as_deref() == Some(id) { active_marker.as_str() } else { "" };
    let mut lines: Vec<(String, Style)> = config
        .providers
        .iter()
        .map(|p| (format!("{} ({}) — {}{}", p.id, commands::kind_label(p.kind), provider_display_model(p), marker(&p.id)), Style::default()))
        .collect();
    // Combos (P90) are picked the same way, with /models use.
    lines.extend(config.combos.iter().map(|c| (format!("{} (combo) — {}{}", c.id, c.providers.join(" → "), marker(&c.id)), Style::default())));
    render_message_card(terminal, &tr!("models.title"), accent_style(), lines)
}

async fn cmd_models_use(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    if !config.providers.iter().any(|p| p.id == id) && !config.combos.iter().any(|c| c.id == id) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("models.use_not_found", id = id), Style::default())]);
    }
    session.provider_id = Some(id.clone());
    render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.now_active", id = id), Style::default())])
}

async fn cmd_models_reset(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    session.provider_id = None;
    render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.reset_done"), Style::default())])
}

/// Loops a single wizard field until it parses as a valid `Provider` kind, re-prompting with an
/// error card on an invalid entry. Returns `Ok(None)` if the user cancels.
async fn prompt_provider_kind(terminal: &mut CliTerminal, initial: Provider) -> anyhow::Result<Option<Provider>> {
    loop {
        let Some(input) = prompt_field(terminal, " kind: gemini | openai | anthropic | openai_compatible ", commands::kind_label(initial)).await? else {
            return Ok(None);
        };
        match commands::parse_provider_kind(&input) {
            Some(kind) => return Ok(Some(kind)),
            None => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("models.invalid_kind"), Style::default())])?,
        }
    }
}

/// Loops a single wizard field until it's a non-blank id that doesn't collide with an existing
/// provider (other than `keep_if_same`, so editing a provider without renaming it doesn't trip
/// the uniqueness check against itself). Returns `Ok(None)` if the user cancels.
async fn prompt_provider_id(terminal: &mut CliTerminal, providers: &[ProviderConfig], initial: &str, keep_if_same: Option<&str>) -> anyhow::Result<Option<String>> {
    loop {
        let Some(input) = prompt_field(terminal, &tr!("models.prompt_id"), initial).await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if candidate.is_empty() {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limit_prompt.id_blank"), Style::default())])?;
        } else if Some(candidate.as_str()) != keep_if_same && providers.iter().any(|p| p.id == candidate) {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("models.id_taken", id = candidate), Style::default())])?;
        } else {
            return Ok(Some(candidate));
        }
    }
}

async fn wizard_models_add(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;

    let Some(kind) = prompt_provider_kind(terminal, Provider::Gemini).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
    };
    let Some(id) = prompt_provider_id(terminal, &config.providers, "", None).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
    };
    let Some(api_key) = prompt_field(terminal, &tr!("models.prompt_api_key"), "").await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
    };
    let base_url = if kind == Provider::OpenaiCompatible {
        let Some(input) = prompt_field(terminal, &tr!("models.prompt_base_url_required"), "").await? else {
            return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
        };
        non_empty(input)
    } else {
        None
    };
    let Some(model_input) = prompt_field(terminal, " model ", default_model_for(kind).unwrap_or("")).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.add_cancelled"), dim_style())]);
    };

    let had_active_provider = config.active_provider.is_some();
    config.providers.push(ProviderConfig { id: id.clone(), kind, api_key: non_empty(api_key), base_url, model: non_empty(model_input), node: None });
    if !had_active_provider {
        config.active_provider = Some(id.clone());
    }

    save_config_or_report(session, &config).await?;
    render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.added", id = id), Style::default())])
}

async fn wizard_models_edit(terminal: &mut CliTerminal, session: &mut CliSession, target_id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(index) = config.providers.iter().position(|p| p.id == target_id) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("models.not_found", id = target_id), Style::default())]);
    };
    let current = config.providers[index].clone();

    let Some(kind) = prompt_provider_kind(terminal, current.kind).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
    };
    let Some(new_id) = prompt_provider_id(terminal, &config.providers, &current.id, Some(&current.id)).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
    };
    let Some(api_key) = prompt_field(terminal, &tr!("models.prompt_api_key"), current.api_key.as_deref().unwrap_or("")).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
    };
    let base_url = if kind == Provider::OpenaiCompatible {
        let Some(input) = prompt_field(terminal, " base url ", current.base_url.as_deref().unwrap_or("")).await? else {
            return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
        };
        non_empty(input)
    } else {
        None
    };
    let model_default = current.model.clone().unwrap_or_else(|| default_model_for(kind).unwrap_or("").to_string());
    let Some(model_input) = prompt_field(terminal, " model ", &model_default).await? else {
        return render_message_card(terminal, &tr!("models.title"), dim_style(), vec![(tr!("prices.edit_cancelled"), dim_style())]);
    };

    let old_id = current.id.clone();
    // A node's model (P93) isn't set up from this wizard; editing one keeps its node.
    config.providers[index] = ProviderConfig { id: new_id.clone(), kind, api_key: non_empty(api_key), base_url, model: non_empty(model_input), node: current.node.clone() };
    if new_id != old_id {
        rename_provider_cascade(&mut config, &old_id, &new_id);
        if session.provider_id.as_deref() == Some(old_id.as_str()) {
            session.provider_id = Some(new_id.clone());
        }
    }

    save_config_or_report(session, &config).await?;
    render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.updated", id = new_id), Style::default())])
}

async fn cmd_models_remove(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let before = config.providers.len();
    config.providers.retain(|p| p.id != id);
    if config.providers.len() == before {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("models.not_found", id = id), Style::default())]);
    }
    remove_provider_references(&mut config, &id);
    // Also a combo this removal emptied (and so deleted).
    if session.provider_id.as_deref().is_some_and(|sid| sid == id || (!config.providers.iter().any(|p| p.id == sid) && !config.combos.iter().any(|c| c.id == sid))) {
        session.provider_id = None;
    }

    save_config_or_report(session, &config).await?;
    render_message_card(terminal, &tr!("models.title"), accent_style(), vec![(tr!("models.removed", id = id), Style::default())])
}

async fn cmd_combos_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    if config.combos.is_empty() {
        return render_message_card(terminal, "combos", accent_style(), vec![(tr!("combos.none"), Style::default())]);
    }
    let lines = config.combos.iter().map(|c| (format!("{} — {}", c.id, c.providers.join(" → ")), Style::default())).collect();
    render_message_card(terminal, "combos", accent_style(), lines)
}

/// `/combos add` creates the combo, or replaces the providers of one that already has this name.
async fn cmd_combos_add(terminal: &mut CliTerminal, session: &mut CliSession, id: String, members: Vec<String>) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let mut combos: Vec<ComboConfig> = config.combos.iter().filter(|c| c.id != id).cloned().collect();
    combos.push(ComboConfig { id: id.clone(), providers: members });
    match warden_bootstrap::settings::check_combos(combos, &config.providers) {
        Ok(checked) => config.combos = checked,
        Err(message) => return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())]),
    }
    save_config_or_report(session, &config).await?;
    let combo = config.combos.iter().find(|c| c.id == id).map(|c| c.providers.join(" → ")).unwrap_or_default();
    render_message_card(terminal, "combos", accent_style(), vec![(tr!("combos.saved", id = id, combo = combo), Style::default())])
}

async fn cmd_combos_remove(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if !config.combos.iter().any(|c| c.id == id) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("combos.not_found", id = id), Style::default())]);
    }
    remove_combo(&mut config, &id);
    if session.provider_id.as_deref() == Some(id.as_str()) {
        session.provider_id = None;
    }
    save_config_or_report(session, &config).await?;
    render_message_card(terminal, "combos", accent_style(), vec![(tr!("combos.removed", id = id), Style::default())])
}

async fn cmd_agents_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    // P84: members' own agents are theirs, not listed here.
    if !config.agents.iter().any(|a| a.owner.is_none()) {
        return render_message_card(terminal, &tr!("agents.title"), accent_style(), vec![(tr!("agents.none"), Style::default())]);
    }
    let lines = config
        .agents
        .iter()
        .filter(|a| a.owner.is_none())
        .map(|a| {
            let preview: String = a.persona.chars().take(48).collect();
            let preview = if a.persona.chars().count() > 48 { format!("{preview}…") } else { preview };
            let provider = a.provider_id.clone().unwrap_or_else(|| "-".to_string());
            let marker = if session.agent_id.as_deref() == Some(a.id.as_str()) { tr!("marker.active") } else { String::new() };
            let delegate_marker = if a.can_delegate_to_agents { tr!("marker.delegates") } else { String::new() };
            let manage_marker = if a.can_manage_agents { tr!("marker.creates") } else { String::new() };
            let message_marker = if a.can_message_agents { tr!("marker.notes") } else { String::new() };
            let tasks_marker = if a.can_manage_tasks { tr!("marker.tasks") } else { String::new() };
            let tools_marker = a.allowed_tools.as_ref().map(|t| tr!("marker.tools", count = t.len())).unwrap_or_default();
            let autonomy_marker = if a.autonomy != warden_bootstrap::default_autonomy() { tr!("marker.autonomy", level = a.autonomy) } else { String::new() };
            let approval_marker = if a.approval_required.is_empty() { String::new() } else { tr!("marker.approves", count = a.approval_required.len()) };
            let org_marker = a.reports_to.as_ref().map(|boss| tr!("marker.reports_to", boss = boss)).unwrap_or_default();
            (
                format!("{} ({}) — {}{}{}{}{}{}{}{}{}{}", a.id, provider, preview, marker, delegate_marker, manage_marker, message_marker, tasks_marker, tools_marker, autonomy_marker, approval_marker, org_marker),
                Style::default(),
            )
        })
        .collect();
    render_message_card(terminal, &tr!("agents.title"), accent_style(), lines)
}

/// Who reports to whom (P120), as a text tree; only the owner's agents (a member's are theirs).
async fn cmd_agents_tree(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    let tree = warden_bootstrap::org::build_org(&config.agents);
    if tree.is_empty() {
        return render_message_card(terminal, &tr!("agents.org_title"), accent_style(), vec![(tr!("agents.none"), Style::default())]);
    }
    let mut lines: Vec<(String, Style)> = warden_bootstrap::org::render_org(&tree).into_iter().map(|line| (line, Style::default())).collect();
    if tree.iter().all(|node| node.children.is_empty()) {
        lines.push((tr!("agents.nobody_reports"), dim_style()));
    }
    render_message_card(terminal, &tr!("agents.org_title"), accent_style(), lines)
}

async fn cmd_agents_use(terminal: &mut CliTerminal, session: &mut CliSession, id: Option<String>) -> anyhow::Result<()> {
    let Some(id) = id else {
        session.agent_id = None;
        return render_message_card(terminal, &tr!("agents.title"), accent_style(), vec![(tr!("agents.none_active"), Style::default())]);
    };
    let config = load_fresh_config(session.config_path.as_deref())?;
    if !config.agents.iter().any(|a| a.id == id && a.owner.is_none()) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("agents.use_not_found", id = id), Style::default())]);
    }
    session.agent_id = Some(id.clone());
    render_message_card(terminal, &tr!("agents.title"), accent_style(), vec![(tr!("agents.now_active", id = id), Style::default())])
}

/// Loops a single wizard field until it's a non-blank id that doesn't collide with an existing
/// agent (other than `keep_if_same`). Returns `Ok(None)` if the user cancels.
async fn prompt_agent_id(terminal: &mut CliTerminal, agents: &[AgentConfig], initial: &str, keep_if_same: Option<&str>) -> anyhow::Result<Option<String>> {
    loop {
        let Some(input) = prompt_field(terminal, &tr!("agents.prompt_id"), initial).await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if candidate.is_empty() {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("limit_prompt.id_blank"), Style::default())])?;
        } else if Some(candidate.as_str()) != keep_if_same && agents.iter().any(|a| a.id == candidate) {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("agents.id_taken", id = candidate), Style::default())])?;
        } else {
            return Ok(Some(candidate));
        }
    }
}

/// Loops a single wizard field until it's blank (= no default provider) or a real provider id.
/// Returns `Ok(None)` if the user cancels the wizard (distinct from `Ok(Some(None))`, "no
/// default provider chosen").
async fn prompt_agent_provider_id(terminal: &mut CliTerminal, providers: &[ProviderConfig], initial: &str) -> anyhow::Result<Option<Option<String>>> {
    loop {
        let Some(input) = prompt_field(terminal, &tr!("agents.prompt_provider"), initial).await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if candidate.is_empty() {
            return Ok(Some(None));
        } else if providers.iter().any(|p| p.id == candidate) {
            return Ok(Some(Some(candidate)));
        } else {
            render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("agents.provider_missing", id = candidate), Style::default())])?;
        }
    }
}

/// Loops a single wizard field until it parses as yes/no. Returns `Ok(None)` if the user cancels
/// (distinct from `Ok(Some(false))`, "answered no").
async fn prompt_agent_can_delegate(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_delegate"), initial).await
}

async fn prompt_agent_can_manage(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_manage"), initial).await
}

async fn prompt_agent_can_message(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_message"), initial).await
}

async fn prompt_agent_can_manage_tasks(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_manage_tasks"), initial).await
}

async fn prompt_agent_can_start_tasks(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_start_tasks"), initial).await
}

async fn prompt_agent_can_message_user(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_message_user"), initial).await
}

async fn prompt_agent_can_choose_models(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_choose_models"), initial).await
}

async fn prompt_agent_can_create_workers(terminal: &mut CliTerminal, initial: bool) -> anyhow::Result<Option<bool>> {
    prompt_agent_flag(terminal, &tr!("agents.can_create_workers"), initial).await
}

/// 1 to 5, how much the agent may do without asking (P122).
async fn prompt_agent_autonomy(terminal: &mut CliTerminal, initial: u8) -> anyhow::Result<Option<u8>> {
    loop {
        let Some(input) = prompt_field(terminal, &tr!("agents.prompt_autonomy"), &initial.to_string()).await? else {
            return Ok(None);
        };
        match parse_agent_autonomy(&input) {
            Ok(level) => return Ok(Some(level)),
            Err(message) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())])?,
        }
    }
}

/// The agent's role and who it reports to (P120): both blank for none. The superior's id is checked against the
/// others (and for a circle) when the agent is saved.
async fn prompt_agent_role_and_superior(
    terminal: &mut CliTerminal,
    role: Option<&str>,
    reports_to: Option<&str>,
) -> anyhow::Result<Option<(Option<String>, Option<String>)>> {
    let Some(role) = prompt_field(terminal, &tr!("agents.prompt_role"), role.unwrap_or("")).await? else {
        return Ok(None);
    };
    let Some(superior) = prompt_field(terminal, &tr!("agents.prompt_superior"), reports_to.unwrap_or("")).await? else {
        return Ok(None);
    };
    let blank_is_none = |text: String| Some(text.trim().to_string()).filter(|t| !t.is_empty());
    Ok(Some((blank_is_none(role), blank_is_none(superior))))
}

/// The kinds of action the agent must have approved even when it acts alone (P122), as ids separated by commas.
async fn prompt_agent_approval_categories(terminal: &mut CliTerminal, initial: &[warden_core::autonomy::Category]) -> anyhow::Result<Option<Vec<warden_core::autonomy::Category>>> {
    let all = warden_core::autonomy::Category::ALL.map(|c| c.as_str()).join(", ");
    let title = tr!("agents.prompt_approval", all = all);
    let initial_text = initial.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ");
    loop {
        let Some(input) = prompt_field(terminal, &title, &initial_text).await? else {
            return Ok(None);
        };
        match parse_approval_categories(&input) {
            Ok(categories) => return Ok(Some(categories)),
            Err(message) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())])?,
        }
    }
}

/// Blank is none; otherwise ids from `Category::ALL`, each kept once.
fn parse_approval_categories(input: &str) -> Result<Vec<warden_core::autonomy::Category>, String> {
    let ids: Vec<String> = input.split(',').map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect();
    warden_bootstrap::settings::categories_from_ids(&ids).map_err(|_| {
        let all = warden_core::autonomy::Category::ALL.map(|c| c.as_str()).join(", ");
        tr!("agents.categories_only", all = all)
    })
}

fn parse_agent_autonomy(input: &str) -> Result<u8, String> {
    match input.trim().parse::<u8>() {
        Ok(level) if (1..=5).contains(&level) => Ok(level),
        _ => Err(tr!("agents.autonomy_range")),
    }
}

/// Loops a single wizard field until it's blank (= every tool) or a comma-separated list of tools
/// that exist. Returns `Ok(None)` if the user cancels (distinct from `Ok(Some(None))`, "all tools").
async fn prompt_agent_tools(terminal: &mut CliTerminal, known: &[String], initial: Option<&[String]>) -> anyhow::Result<Option<Option<Vec<String>>>> {
    loop {
        let initial_text = initial.map(|t| t.join(", ")).unwrap_or_default();
        let Some(input) = prompt_field(terminal, &tr!("agents.prompt_tools"), &initial_text).await? else {
            return Ok(None);
        };
        match parse_agent_tools(&input, known) {
            Ok(tools) => return Ok(Some(tools)),
            Err(message) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())])?,
        }
    }
}

/// `None` for blank ("every tool"), else the trimmed, de-duplicated names — all of which must exist.
/// `delegate_to_agent`/`manage_agents` are refused: they follow the two yes/no questions instead.
fn parse_agent_tools(input: &str, known: &[String]) -> Result<Option<Vec<String>>, String> {
    if input.trim().is_empty() {
        return Ok(None);
    }
    let mut tools: Vec<String> = Vec::new();
    for name in input.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if matches!(name, "delegate_to_agent" | "manage_agents" | "message_agent" | "manage_tasks") {
            return Err(tr!("agents.tool_not_listable", name = name));
        }
        if !known.iter().any(|k| k == name) {
            return Err(tr!("agents.tool_missing", name = name, available = known.join(", ")));
        }
        if !tools.iter().any(|t| t == name) {
            tools.push(name.to_string());
        }
    }
    Ok(Some(tools))
}

/// The models the agent may delegate with (P123): ids of providers, combos or policies separated by commas, the first one the default
/// for a delegation that names none; blank = the choice is open.
async fn prompt_agent_delegation_models(terminal: &mut CliTerminal, known: &[String], initial: &[String]) -> anyhow::Result<Option<Vec<String>>> {
    let title = tr!("agents.prompt_delegation_models");
    let initial_text = initial.join(", ");
    loop {
        let Some(input) = prompt_field(terminal, &title, &initial_text).await? else {
            return Ok(None);
        };
        match parse_delegation_models(&input, known) {
            Ok(models) => return Ok(Some(models)),
            Err(message) => render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())])?,
        }
    }
}

/// Blank is an empty list (open); otherwise the trimmed, de-duplicated ids, all of which must be in `known`.
fn parse_delegation_models(input: &str, known: &[String]) -> Result<Vec<String>, String> {
    let mut models: Vec<String> = Vec::new();
    for id in input.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        if !known.iter().any(|k| k == id) {
            let available = if known.is_empty() { tr!("agents.none_registered") } else { known.join(", ") };
            return Err(tr!("agents.model_missing", id = id, available = available));
        }
        if !models.iter().any(|m| m == id) {
            models.push(id.to_string());
        }
    }
    Ok(models)
}

/// Every id an agent's delegation may be limited to: providers, combos and named policies.
fn delegation_model_ids(config: &FileConfig) -> Vec<String> {
    config.providers.iter().map(|p| p.id.clone()).chain(config.combos.iter().map(|c| c.id.clone())).chain(config.model_policies.iter().map(|p| p.id.clone())).collect()
}

async fn prompt_agent_flag(terminal: &mut CliTerminal, title: &str, initial: bool) -> anyhow::Result<Option<bool>> {
    let initial_text = if initial { tr!("common.yes_letter") } else { "n".to_string() };
    loop {
        let Some(input) = prompt_field(terminal, title, &initial_text).await? else {
            return Ok(None);
        };
        match input.trim().to_lowercase().as_str() {
            "s" | "sim" | "y" | "yes" => return Ok(Some(true)),
            "n" | "nao" | "não" | "no" => return Ok(Some(false)),
            _ => {
                render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("common.answer_yes_no"), Style::default())])?;
            }
        }
    }
}

async fn wizard_agents_create(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;

    let Some(id) = prompt_agent_id(terminal, &config.agents, "", None).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(persona) = prompt_field(terminal, &tr!("agents.prompt_persona_new"), "").await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(provider_id) = prompt_agent_provider_id(terminal, &config.providers, "").await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_delegate_to_agents) = prompt_agent_can_delegate(terminal, false).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_manage_agents) = prompt_agent_can_manage(terminal, false).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_message_agents) = prompt_agent_can_message(terminal, false).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_manage_tasks) = prompt_agent_can_manage_tasks(terminal, false).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_start_tasks) = prompt_agent_can_start_tasks(terminal, true).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_create_workers) = prompt_agent_can_create_workers(terminal, true).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_message_user) = prompt_agent_can_message_user(terminal, true).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(can_choose_models) = prompt_agent_can_choose_models(terminal, true).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(autonomy) = prompt_agent_autonomy(terminal, warden_bootstrap::default_autonomy()).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };
    let Some(approval_required) = prompt_agent_approval_categories(terminal, &[]).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };

    let Some(allowed_tools) = prompt_agent_tools(terminal, &session.tool_names, None).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };

    let Some((role, reports_to)) = prompt_agent_role_and_superior(terminal, None, None).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };

    let Some(delegation_models) = prompt_agent_delegation_models(terminal, &delegation_model_ids(&config), &[]).await? else {
        return agents_cancelled(terminal, "cancelled.creation");
    };

    config.agents.push(AgentConfig {
        id: id.clone(),
        persona,
        provider_id,
        can_delegate_to_agents,
        can_manage_agents,
        can_message_agents,
        can_manage_tasks,
        allowed_tools,
        autonomy,
        approval_required,
        role,
        reports_to,
        owner: None,
        shared_with: Vec::new(),
        delegation_models,
        can_start_tasks,
        can_create_workers,
        can_message_user,
        can_choose_models,
    });
    if let Err(message) = warden_bootstrap::org::check_hierarchy(&config.agents) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())]);
    }

    save_config_or_report(session, &config).await?;
    render_message_card(terminal, &tr!("agents.title"), accent_style(), vec![(tr!("agents.created", id = id), Style::default())])
}

/// `what_key` names what was cancelled (a catalog key: creation, edit...).
fn agents_cancelled(terminal: &mut CliTerminal, what_key: &str) -> anyhow::Result<()> {
    render_message_card(terminal, &tr!("agents.title"), dim_style(), vec![(tr!(what_key), dim_style())])
}

async fn wizard_agents_edit(terminal: &mut CliTerminal, session: &mut CliSession, target_id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    // P84: members' own agents are theirs — not the owner's to edit here.
    let Some(index) = config.agents.iter().position(|a| a.id == target_id && a.owner.is_none()) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("agents.not_found", id = target_id), Style::default())]);
    };
    let current = config.agents[index].clone();

    let Some(new_id) = prompt_agent_id(terminal, &config.agents, &current.id, Some(&current.id)).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(persona) = prompt_field(terminal, " persona ", &current.persona).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(provider_id) = prompt_agent_provider_id(terminal, &config.providers, current.provider_id.as_deref().unwrap_or("")).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_delegate_to_agents) = prompt_agent_can_delegate(terminal, current.can_delegate_to_agents).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_manage_agents) = prompt_agent_can_manage(terminal, current.can_manage_agents).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_message_agents) = prompt_agent_can_message(terminal, current.can_message_agents).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_manage_tasks) = prompt_agent_can_manage_tasks(terminal, current.can_manage_tasks).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_start_tasks) = prompt_agent_can_start_tasks(terminal, current.can_start_tasks).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_create_workers) = prompt_agent_can_create_workers(terminal, current.can_create_workers).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_message_user) = prompt_agent_can_message_user(terminal, current.can_message_user).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(can_choose_models) = prompt_agent_can_choose_models(terminal, current.can_choose_models).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(autonomy) = prompt_agent_autonomy(terminal, current.autonomy).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };
    let Some(approval_required) = prompt_agent_approval_categories(terminal, &current.approval_required).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };

    let Some(allowed_tools) = prompt_agent_tools(terminal, &session.tool_names, current.allowed_tools.as_deref()).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };

    let Some((role, reports_to)) = prompt_agent_role_and_superior(terminal, current.role.as_deref(), current.reports_to.as_deref()).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };

    let Some(delegation_models) = prompt_agent_delegation_models(terminal, &delegation_model_ids(&config), &current.delegation_models).await? else {
        return agents_cancelled(terminal, "cancelled.edit");
    };

    let old_id = current.id.clone();
    config.agents[index] = AgentConfig {
        id: new_id.clone(),
        persona,
        provider_id,
        can_delegate_to_agents,
        can_manage_agents,
        can_message_agents,
        can_manage_tasks,
        allowed_tools,
        autonomy,
        approval_required,
        role,
        reports_to,
        owner: None,
        shared_with: current.shared_with.clone(),
        delegation_models,
        can_start_tasks,
        can_create_workers,
        can_message_user,
        can_choose_models,
    };
    // Whoever reported to it under the old name reports to it under the new one.
    warden_bootstrap::org::rename_in_reports(&mut config.agents, &old_id, &new_id);
    if let Err(message) = warden_bootstrap::org::check_hierarchy(&config.agents) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())]);
    }
    if new_id != old_id && session.agent_id.as_deref() == Some(old_id.as_str()) {
        session.agent_id = Some(new_id.clone());
    }

    save_config_or_report(session, &config).await?;
    render_message_card(terminal, &tr!("agents.title"), accent_style(), vec![(tr!("agents.updated", id = new_id), Style::default())])
}

async fn cmd_agents_remove(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if !config.agents.iter().any(|a| a.id == id && a.owner.is_none()) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("agents.not_found", id = id), Style::default())]);
    }
    // Also takes the agent out of the SSH servers that named it: a server left with no agent is
    // switched off rather than opened to every agent, and no dangling name is left behind.
    let effects = remove_agent_references(&mut config, &id);
    if session.agent_id.as_deref() == Some(id.as_str()) {
        session.agent_id = None;
    }

    // The agent itself is re-read from disk each turn; only a change to an SSH server (baked into the orchestrator by
    // `bootstrap()`) needs a rebuild.
    if effects.is_empty() {
        save_config_or_report(session, &config).await?;
    } else {
        save_config_and_reload(session, &config).await?;
    }
    let mut lines = vec![(tr!("agents.removed", id = id), Style::default())];
    for effect in effects {
        lines.push((
            if effect.switched_off {
                tr!("agents.ssh_blocked", host = effect.host_id)
            } else {
                tr!("agents.ssh_unlisted", host = effect.host_id)
            },
            Style::default(),
        ));
    }
    render_message_card(terminal, &tr!("agents.title"), accent_style(), lines)
}

async fn cmd_ssh_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    if config.ssh_hosts.is_empty() {
        return render_message_card(terminal, "ssh", accent_style(), vec![(tr!("ssh.none"), Style::default())]);
    }
    let lines = config
        .ssh_hosts
        .iter()
        .map(|h| {
            let state = if h.enabled { tr!("ssh.state_on") } else { tr!("ssh.state_off") };
            let who = if h.agents.is_empty() { tr!("ssh.who_all") } else { tr!("ssh.who_only", agents = h.agents.join(", ")) };
            let ask = if h.require_approval { tr!("ssh.asks_each_time") } else { String::new() };
            (format!("{} — {}@{}:{} [{}] ({}{})", h.id, h.user, h.host, h.port, state, who, ask), Style::default())
        })
        .collect();
    render_message_card(terminal, "ssh", accent_style(), lines)
}

/// `/ssh log [n]` (P47): the last `count` calls of the SSH tools, newest first, one line each. The log never holds the output of a command.
fn cmd_ssh_log(terminal: &mut CliTerminal, count: usize) -> anyhow::Result<()> {
    let Some(path) = warden_bootstrap::default_ssh_audit_log_path() else {
        return render_message_card(terminal, "ssh", dim_style(), vec![(tr!("ssh.no_config_dir"), dim_style())]);
    };
    let entries = warden_bootstrap::ssh_audit::read_ssh_audit(&path, count);
    if entries.is_empty() {
        return render_message_card(terminal, "ssh", dim_style(), vec![(tr!("ssh.no_calls"), dim_style())]);
    }
    let lines = entries.iter().map(|entry| (entry.summary_line(), if entry.approval == "denied" || entry.approval == "unavailable" { error_style() } else { Style::default() })).collect();
    render_message_card(terminal, "ssh log", accent_style(), lines)
}

/// `what_key` names what was cancelled (a catalog key: creation, edit, removal).
fn ssh_cancelled(terminal: &mut CliTerminal, what_key: &str) -> anyhow::Result<()> {
    render_message_card(terminal, "ssh", dim_style(), vec![(tr!(what_key), dim_style())])
}

/// Asks every field of an SSH host, pre-filled from `current` when editing. `Ok(None)` means the
/// user cancelled (Esc/Ctrl+C); a rejected value renders its error and also yields `None`, so
/// nothing half-valid is ever saved — the final `validate()` is the same one `ssh_exec` runs.
async fn prompt_ssh_host(terminal: &mut CliTerminal, config: &FileConfig, current: Option<&SshHostConfig>) -> anyhow::Result<Option<SshHostConfig>> {
    let keep_id = current.map(|h| h.id.as_str());
    let yes = tr!("common.yes_letter");
    let Some(id) = prompt_field(terminal, &tr!("ssh_prompt.id"), keep_id.unwrap_or("")).await? else {
        return Ok(None);
    };
    let id = id.trim().to_string();
    if id.is_empty() || (Some(id.as_str()) != keep_id && config.ssh_hosts.iter().any(|h| h.id == id)) {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh_prompt.id_invalid"), Style::default())])?;
        return Ok(None);
    }
    let Some(host) = prompt_field(terminal, &tr!("ssh_prompt.host"), current.map_or("", |h| h.host.as_str())).await? else {
        return Ok(None);
    };
    let Some(user) = prompt_field(terminal, &tr!("ssh_prompt.user"), current.map_or("", |h| h.user.as_str())).await? else {
        return Ok(None);
    };
    let Some(port) = prompt_field(terminal, &tr!("ssh_prompt.port"), &current.map_or(22, |h| h.port).to_string()).await? else {
        return Ok(None);
    };
    let Ok(port) = port.trim().parse::<u16>() else {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh_prompt.port_invalid"), Style::default())])?;
        return Ok(None);
    };
    let Some(identity) = prompt_field(terminal, &tr!("ssh_prompt.identity"), current.and_then(|h| h.identity_file.as_deref()).unwrap_or("")).await? else {
        return Ok(None);
    };
    let Some(agents) = prompt_field(terminal, &tr!("ssh_prompt.agents"), &current.map_or(String::new(), |h| h.agents.join(", "))).await? else {
        return Ok(None);
    };
    let agents: Vec<String> = agents.split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_string).collect();
    if let Some(unknown) = agents.iter().find(|a| !config.agents.iter().any(|known| &known.id == *a)) {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("target.agent_missing", id = unknown), Style::default())])?;
        return Ok(None);
    }
    let Some(enabled) = prompt_field(terminal, &tr!("ssh_prompt.enable"), if current.is_some_and(|h| h.enabled) { yes.as_str() } else { "n" }).await? else {
        return Ok(None);
    };
    let enabled = matches!(enabled.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes");
    let Some(approval) = prompt_field(
        terminal,
        &tr!("ssh_prompt.require_approval"),
        if current.is_some_and(|h| h.require_approval) { yes.as_str() } else { "n" },
    )
    .await?
    else {
        return Ok(None);
    };
    let require_approval = matches!(approval.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes");

    let host = SshHostConfig {
        id,
        host: host.trim().to_string(),
        user: user.trim().to_string(),
        port,
        identity_file: non_empty(identity),
        enabled,
        agents,
        require_approval,
    };
    if let Err(err) = host.to_host().validate() {
        render_message_card(terminal, &tr!("common.error"), error_style(), vec![(format!("{err:#}"), Style::default())])?;
        return Ok(None);
    }
    Ok(Some(host))
}

async fn wizard_ssh_add(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(host) = prompt_ssh_host(terminal, &config, None).await? else {
        return ssh_cancelled(terminal, "cancelled.creation");
    };
    let id = host.id.clone();
    config.ssh_hosts.push(host);
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, "ssh", accent_style(), vec![(tr!("ssh.added", id = id), Style::default())])
}

async fn wizard_ssh_edit(terminal: &mut CliTerminal, session: &mut CliSession, target_id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(index) = config.ssh_hosts.iter().position(|h| h.id == target_id) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh.not_found", id = target_id), Style::default())]);
    };
    let current = config.ssh_hosts[index].clone();
    let Some(host) = prompt_ssh_host(terminal, &config, Some(&current)).await? else {
        return ssh_cancelled(terminal, "cancelled.edit");
    };
    let id = host.id.clone();
    config.ssh_hosts[index] = host;
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, "ssh", accent_style(), vec![(tr!("ssh.updated", id = id), Style::default())])
}

async fn cmd_ssh_remove(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    if !config.ssh_hosts.iter().any(|h| h.id == id) {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh.not_found", id = id), Style::default())]);
    }
    let Some(answer) = prompt_field(terminal, &tr!("ssh.confirm_remove", id = id), "").await? else {
        return ssh_cancelled(terminal, "cancelled.removal");
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return ssh_cancelled(terminal, "cancelled.removal");
    }
    config.ssh_hosts.retain(|h| h.id != id);
    save_config_and_reload(session, &config).await?;
    render_message_card(terminal, "ssh", accent_style(), vec![(tr!("ssh.removed", id = id), Style::default())])
}

async fn cmd_ssh_enable(terminal: &mut CliTerminal, session: &mut CliSession, id: String, enabled: bool) -> anyhow::Result<()> {
    let mut config = load_fresh_config(session.config_path.as_deref())?;
    let Some(host) = config.ssh_hosts.iter_mut().find(|h| h.id == id) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh.not_found", id = id), Style::default())]);
    };
    host.enabled = enabled;
    save_config_and_reload(session, &config).await?;
    let state = if enabled { tr!("ssh.enabled_for_ai") } else { tr!("ssh.blocked_for_ai") };
    render_message_card(terminal, "ssh", accent_style(), vec![(tr!("ssh.state_changed", id = id, state = state), Style::default())])
}

async fn cmd_ssh_test(terminal: &mut CliTerminal, session: &mut CliSession, id: String) -> anyhow::Result<()> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    let Some(entry) = config.ssh_hosts.iter().find(|h| h.id == id) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("ssh.not_found", id = id), Style::default())]);
    };
    let outcome = test_connection(&entry.to_host()).await?;
    let (title, style) = if outcome.ok { ("ssh".to_string(), accent_style()) } else { (tr!("common.error"), error_style()) };
    render_message_card(terminal, &title, style, vec![(outcome.message, Style::default())])
}

/// Builds a `SkillStore` over the same vault this CLI session already uses — fresh per command,
/// same "never cached" posture as `load_fresh_config`. Skills live in the vault (not in
/// `config.toml`), so there's no config to write back: every command here acts on the files.
fn make_skill_store(session: &CliSession) -> anyhow::Result<SkillStore> {
    let vault_path = resolve_vault_path(session.config_path.as_deref(), session.vault_path_override.as_deref())?;
    Ok(SkillStore::new(Arc::new(Vault::new(vault_path))))
}

fn skill_error_card(terminal: &mut CliTerminal, message: String) -> anyhow::Result<()> {
    render_message_card(terminal, &tr!("common.error"), error_style(), vec![(message, Style::default())])
}

async fn cmd_skills_list(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let skills = make_skill_store(session)?.list();
    if skills.is_empty() {
        return render_message_card(terminal, "skills", accent_style(), vec![(tr!("skills.none"), Style::default())]);
    }
    let no_description = tr!("skills.no_description");
    let lines = skills
        .iter()
        .map(|s| {
            let description = if s.description.is_empty() { no_description.as_str() } else { s.description.as_str() };
            let scope = if s.agents.is_empty() { String::new() } else { format!(" [{}]", s.agents.join(", ")) };
            (format!("{}{} — {}", s.name, scope, description), Style::default())
        })
        .collect();
    render_message_card(terminal, "skills", accent_style(), lines)
}

async fn cmd_skills_show(terminal: &mut CliTerminal, session: &CliSession, name: String) -> anyhow::Result<()> {
    let skill = match make_skill_store(session)?.get(&name) {
        Ok(skill) => skill,
        Err(e) => return skill_error_card(terminal, e.to_string()),
    };
    let description = if skill.description.is_empty() { tr!("skills.no_description") } else { skill.description.clone() };
    let mut lines = vec![(tr!("skills.description", text = description), dim_style())];
    let agents = if skill.agents.is_empty() { tr!("skills.all_agents") } else { skill.agents.join(", ") };
    lines.push((tr!("skills.agents", agents = agents), dim_style()));
    let files = match make_skill_store(session)?.list_files(&skill.name) {
        Ok(files) => files,
        Err(e) => return skill_error_card(terminal, e.to_string()),
    };
    if !files.is_empty() {
        lines.push((tr!("skills.attachments", files = files.join(", ")), dim_style()));
    }
    lines.extend(skill.body.lines().map(|line| (line.to_string(), Style::default())));
    render_message_card(terminal, &format!("skill {}", skill.name), accent_style(), lines)
}

async fn cmd_skills_file(terminal: &mut CliTerminal, session: &CliSession, name: String, file: String) -> anyhow::Result<()> {
    match make_skill_store(session)?.read_file(&name, &file) {
        Ok(content) => {
            let lines = content.lines().map(|line| (line.to_string(), Style::default())).collect();
            render_message_card(terminal, &format!("skill {name} — {file}"), accent_style(), lines)
        }
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

/// Copies a local text file into the skill's attachments (replacing one of the same name). The
/// content is a copy: editing the original afterwards doesn't change the skill.
async fn cmd_skills_attach(
    terminal: &mut CliTerminal,
    session: &CliSession,
    name: String,
    file: String,
    source: String,
) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    let content = match std::fs::read_to_string(&source) {
        Ok(content) => content,
        Err(e) => return skill_error_card(terminal, tr!("skills.read_failed", source = source, error = e)),
    };
    match store.save_file(&name, &file, &content) {
        Ok(()) => render_message_card(
            terminal,
            "skills",
            accent_style(),
            vec![(tr!("skills.attached", file = file, name = name, bytes = content.len()), Style::default())],
        ),
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

async fn cmd_skills_detach(terminal: &mut CliTerminal, session: &CliSession, name: String, file: String) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    if let Err(e) = store.read_file(&name, &file) {
        return skill_error_card(terminal, e.to_string());
    }
    let Some(answer) = prompt_field(terminal, &tr!("skills.confirm_detach", file = file, name = name), "").await? else {
        return render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.removal"), dim_style())]);
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.removal"), dim_style())]);
    }
    match store.delete_file(&name, &file) {
        Ok(()) => render_message_card(terminal, "skills", accent_style(), vec![(tr!("skills.detached", file = file, name = name), Style::default())]),
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

async fn cmd_skills_path(terminal: &mut CliTerminal, session: &CliSession, name: Option<String>) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    let path = match name {
        Some(name) => match store.path_of(&name) {
            Ok(path) => path,
            Err(e) => return skill_error_card(terminal, e.to_string()),
        },
        None => resolve_vault_path(session.config_path.as_deref(), session.vault_path_override.as_deref())?.join(warden_core::memory::SKILLS_DIR),
    };
    render_message_card(terminal, "skills", accent_style(), vec![(path.display().to_string(), Style::default())])
}

/// Loops one wizard field until it's a valid skill name that doesn't collide with an existing one.
/// Returns `Ok(None)` if the user cancels.
async fn prompt_skill_name(terminal: &mut CliTerminal, store: &SkillStore) -> anyhow::Result<Option<String>> {
    loop {
        let Some(input) = prompt_field(terminal, &tr!("skills.prompt_name"), "").await? else {
            return Ok(None);
        };
        let candidate = input.trim().to_string();
        if let Err(e) = skill::validate_name(&candidate) {
            skill_error_card(terminal, e.to_string())?;
        } else if store.exists(&candidate) {
            skill_error_card(terminal, tr!("skills.exists", name = candidate))?;
        } else {
            return Ok(Some(candidate));
        }
    }
}

/// The ids in `ids` that no agent in `known` has, in the order typed and without repeats. A skill
/// bound to an id nobody has is visible to no agent, which `Skill::validate` can't tell (the core
/// doesn't know the registry), so the wizard checks it (P72 c).
fn unknown_agent_ids(ids: &[String], known: &[String]) -> Vec<String> {
    let mut unknown: Vec<String> = Vec::new();
    for id in ids {
        if !known.contains(id) && !unknown.contains(id) {
            unknown.push(id.clone());
        }
    }
    unknown
}

/// Prompts `description` then, only if `edit_body`, `body`, then the agent restriction (P72 c),
/// re-asking until the whole skill passes `Skill::validate` — the same rules the model's
/// `manage_skill` and the desktop form enforce — and every agent id is one in `known_agents`.
/// The body is one line here (the field editor is single-line); a multi-line body is edited in the
/// file itself, see `/skills path`. Returns `Ok(None)` if the user cancels.
async fn prompt_skill_text(
    terminal: &mut CliTerminal,
    name: &str,
    description: &str,
    body: &str,
    agents: &[String],
    known_agents: &[String],
    edit_body: bool,
) -> anyhow::Result<Option<Skill>> {
    let mut description = description.to_string();
    let mut body = body.to_string();
    let mut agents = agents.join(", ");
    loop {
        let Some(new_description) = prompt_field(terminal, &tr!("skills.prompt_description"), &description).await? else {
            return Ok(None);
        };
        description = new_description.trim().to_string();
        if edit_body {
            let Some(new_body) = prompt_field(terminal, &tr!("skills.prompt_body"), &body).await? else {
                return Ok(None);
            };
            body = new_body.trim().to_string();
        }
        let Some(new_agents) = prompt_field(terminal, &tr!("skills.prompt_agents"), &agents).await? else {
            return Ok(None);
        };
        agents = new_agents.trim().to_string();
        let agent_ids: Vec<String> = agents.split(',').map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect();
        let unknown = unknown_agent_ids(&agent_ids, known_agents);
        if !unknown.is_empty() {
            skill_error_card(terminal, tr!("skills.unknown_agents", ids = unknown.join(", ")))?;
            continue;
        }
        let skill = Skill { name: name.to_string(), description: description.clone(), body: body.clone(), agents: agent_ids, proposed: false, source: None, proposed_at: None, revises: None };
        match skill.validate() {
            Ok(()) => return Ok(Some(skill)),
            Err(e) => skill_error_card(terminal, e.to_string())?,
        }
    }
}

/// The ids of the owner's agents — a member's own agents (P84) are theirs, not the owner's to bind a skill to.
fn owner_agent_ids(session: &CliSession) -> anyhow::Result<Vec<String>> {
    let config = load_fresh_config(session.config_path.as_deref())?;
    Ok(config.agents.iter().filter(|a| a.owner.is_none()).map(|a| a.id.clone()).collect())
}

async fn wizard_skills_create(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    let cancelled = |terminal: &mut CliTerminal| render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.creation"), dim_style())]);

    let Some(name) = prompt_skill_name(terminal, &store).await? else {
        return cancelled(terminal);
    };
    let known_agents = owner_agent_ids(session)?;
    let Some(skill) = prompt_skill_text(terminal, &name, "", "", &[], &known_agents, true).await? else {
        return cancelled(terminal);
    };
    match store.save(&skill) {
        Ok(()) => render_message_card(terminal, "skills", accent_style(), vec![(tr!("skills.created", name = name), Style::default())]),
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

async fn wizard_skills_edit(terminal: &mut CliTerminal, session: &CliSession, name: String) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    let current = match store.get(&name) {
        Ok(skill) => skill,
        Err(e) => return skill_error_card(terminal, e.to_string()),
    };

    // A multi-line body can't be shown in (or survive a round trip through) the single-line field
    // editor, so it's left untouched here — only the description is editable in that case.
    let body_editable = !current.body.contains('\n');
    if !body_editable {
        render_message_card(
            terminal,
            "skills",
            dim_style(),
            vec![(tr!("skills.multiline_body", name = name), dim_style())],
        )?;
    }
    let known_agents = owner_agent_ids(session)?;
    let Some(skill) = prompt_skill_text(terminal, &name, &current.description, &current.body, &current.agents, &known_agents, body_editable).await? else {
        return render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.edit"), dim_style())]);
    };
    match store.save(&skill) {
        Ok(()) => render_message_card(terminal, "skills", accent_style(), vec![(tr!("skills.updated", name = name), Style::default())]),
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

/// Unlike `/agents remove`, asks for confirmation first: a skill is user-written prose that lives
/// only in its file (not a config row that's quick to re-enter), and the model itself has no
/// delete tool exactly so that nothing removes one by accident (see ARCHITECTURE.md, P16).
async fn cmd_skills_remove(terminal: &mut CliTerminal, session: &CliSession, name: String) -> anyhow::Result<()> {
    let store = make_skill_store(session)?;
    if let Err(e) = skill::validate_name(&name).and_then(|()| store.get(&name).map(|_| ())) {
        return skill_error_card(terminal, e.to_string());
    }
    let Some(answer) = prompt_field(terminal, &tr!("skills.confirm_delete", name = name), "").await? else {
        return render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.removal"), dim_style())]);
    };
    if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
        return render_message_card(terminal, "skills", dim_style(), vec![(tr!("cancelled.removal"), dim_style())]);
    }
    match store.delete(&name) {
        Ok(()) => render_message_card(terminal, "skills", accent_style(), vec![(tr!("skills.deleted", name = name), Style::default())]),
        Err(e) => skill_error_card(terminal, e.to_string()),
    }
}

fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Saves `config` to `session.config_path`, surfacing "no config path available" (only possible
/// if `dirs::config_dir()` itself returned `None` — no `$HOME`/`$XDG_CONFIG_HOME`) as the same
/// kind of error card every other failure in a wizard gets, via `?` bubbling out of the caller.
async fn save_config_or_report(session: &CliSession, config: &FileConfig) -> anyhow::Result<()> {
    let path = session
        .config_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!(tr!("config.no_path")))?;
    save_config(path, config)?;
    Ok(())
}

/// Saves like `save_config_or_report` and, once the file is written, asks the loop to rebuild the orchestrator: for a change
/// to what `bootstrap()` builds once (spending limits and prices, SSH servers), which a running session would otherwise only
/// see at the next start. A save that fails asks for nothing.
async fn save_config_and_reload(session: &mut CliSession, config: &FileConfig) -> anyhow::Result<()> {
    save_config_or_report(session, config).await?;
    session.reload_requested = true;
    Ok(())
}

/// `/sync` — status only, no network call beyond what `SyncEngine::status` itself does (a local
/// diff against the manifest, no Arweave/TruthID touch).
async fn cmd_sync_status(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let status = engine.status()?;
    let lines = vec![
        (tr!("sync.paired", answer = if status.paired { tr!("common.yes") } else { tr!("common.no") }), Style::default()),
        (tr!("sync.owner_address", address = status.owner_address.as_deref().unwrap_or("—")), Style::default()),
        (
            tr!("sync.last_synced", when = status.last_synced_at_ms.map(|ms| ms.to_string()).unwrap_or_else(|| tr!("sync.never"))),
            Style::default(),
        ),
        (
            tr!(
                "sync.pending",
                files = status.pending_vault_changes,
                config = if status.pending_config_changed { " + config.toml" } else { "" },
                logins = if status.pending_oauth_changes > 0 { tr!("sync.pending_logins", count = status.pending_oauth_changes) } else { String::new() }
            ),
            Style::default(),
        ),
        (tr!("sync.syncignore_rules", count = status.syncignore_pattern_count), Style::default()),
    ];
    render_message_card(terminal, "sync", accent_style(), lines)
}

/// `/sync push` — the QR is rendered as Unicode block art right in the terminal (not a PNG file),
/// which is exactly what makes this usable from a headless box reached over SSH: whoever is at
/// the keyboard sees the QR in their own terminal and scans it with their phone.
async fn cmd_sync_push(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let Some(begin) = engine.begin_push()? else {
        return render_message_card(
            terminal,
            "sync",
            accent_style(),
            vec![(tr!("sync.nothing_to_send"), Style::default())],
        );
    };
    // P175(e): a device that is behind is told before the QR goes up (`finish_push` checks again, under the lock).
    engine.check_remote_not_newer().await?;

    let qr_json = begin.pending.qr_payload_json()?;
    let qr_code = qrcode::QrCode::new(qr_json.as_bytes()).map_err(|e| anyhow::anyhow!(tr!("sync.qr_failed", error = e)))?;
    let qr_ascii = qr_code.render::<qrcode::render::unicode::Dense1x2>().build();
    let qr_rows: Vec<Vec<(String, Style)>> = qr_ascii.split('\n').map(|line| vec![(line.to_string(), Style::default())]).collect();
    insert_card(terminal, vec![("sync".to_string(), accent_style())], accent_style(), qr_rows, None)?;
    terminal.insert_before(1, |_buf| {})?;

    let outcome = engine.finish_push(begin).await?;
    render_message_card(
        terminal,
        "sync",
        accent_style(),
        vec![(tr!("sync.sent_tx", tx = outcome.tx_id, files = outcome.files_changed), Style::default())],
    )
}

async fn cmd_sync_pull(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let outcome = engine.pull().await?;
    session.reload_requested |= outcome.needs_reload();
    let mut lines = vec![(
        tr!(
            "sync.pull_done",
            written = outcome.files_written,
            deleted = outcome.files_deleted,
            ignored = if outcome.files_ignored > 0 { tr!("sync.ignored", count = outcome.files_ignored) } else { String::new() },
            config = if outcome.config_updated { tr!("sync.config_updated") } else { String::new() }
        ),
        Style::default(),
    )];
    lines.extend(pull_reload_note(outcome.needs_reload()).map(|note| (note, Style::default())));
    for warning in &outcome.warnings {
        lines.push((warning.clone(), Style::default()));
    }
    render_message_card(terminal, "sync", accent_style(), lines)
}

/// What a manual pull tells the person when it brought a new `config.toml` or an MCP login (P54): the loop rebuilds the
/// orchestrator right after the command (`reload_if_requested`) and says how that went.
fn pull_reload_note(needs_reload: bool) -> Option<String> {
    needs_reload.then(|| tr!("sync.reloading"))
}

/// `/sync pair` (no args) — shows a code and blocks until another device joins with it, or the
/// pairing times out. Blocking here (unlike the desktop's background+event-emit approach) is fine
/// on the CLI: there's nothing else useful to do at this prompt meanwhile, and `Ctrl+C`-style
/// cancellation isn't wired into any other long-running command either.
async fn cmd_sync_pair_show(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let host = engine.pairing_host().await?;
    render_message_card(
        terminal,
        "sync",
        accent_style(),
        vec![
            (tr!("sync.pairing_code", code = host.code()), Style::default()),
            (tr!("sync.pairing_instructions"), Style::default()),
        ],
    )?;
    host.wait_for_join().await?;
    render_message_card(terminal, "sync", accent_style(), vec![(tr!("sync.paired_ok"), Style::default())])
}

async fn cmd_sync_pair_join(terminal: &mut CliTerminal, session: &CliSession, code: String, host: Option<String>) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    match host {
        Some(host) => {
            let ip = host.parse::<std::net::Ipv4Addr>().map_err(|_| anyhow::anyhow!(tr!("sync.not_ipv4", host = host)))?;
            engine.pairing_join_with_hosts(&code, vec![ip]).await?;
        }
        None => engine.pairing_join(&code).await?,
    }
    render_message_card(terminal, "sync", accent_style(), vec![(tr!("sync.paired_ok"), Style::default())])
}

/// The person at this terminal, for `warden_sync::arca_pairing_flow`: what this device computed goes on the screen (`show`),
/// what the person types comes back from a field (`ask`). The flow never gives `ask` anything to fill in.
struct TerminalPrompts<'a> {
    terminal: &'a mut CliTerminal,
    /// The "swap the key" path (`/arca swap ...`): the same screens, with the words of that path.
    swap: bool,
}

/// What `show` puts on the screen, one line each. In the swap path the key line carries the fingerprint AND the head code, and the
/// instructions say to type both.
fn shown_lines(shown: Shown, swap: bool) -> Vec<String> {
    match (shown, swap) {
        (Shown::OfferCode(code), false) => vec![tr!("arca.offer_code", code = code), tr!("arca.offer_instructions")],
        (Shown::OfferCode(code), true) => vec![tr!("arca.offer_code", code = code), tr!("arca.swap_offer_instructions")],
        (Shown::KeyFingerprint(fingerprint), false) => vec![tr!("arca.key_fingerprint", fingerprint = fingerprint), tr!("arca.fingerprint_instructions")],
        (Shown::KeyFingerprint(fingerprint), true) => vec![tr!("arca.swap_key_fingerprint", fingerprint = fingerprint), tr!("arca.swap_fingerprint_instructions")],
    }
}

/// The title of the field `ask` opens. Only the fingerprint field changes in the swap path: it takes the head code after it.
fn ask_title(ask: Ask, swap: bool) -> String {
    match (ask, swap) {
        (Ask::CodeShownByTheNewDevice, _) => tr!("arca.ask_code"),
        (Ask::FingerprintShownByTheOldDevice, false) => tr!("arca.ask_fingerprint"),
        (Ask::FingerprintShownByTheOldDevice, true) => tr!("arca.ask_fingerprint_head"),
    }
}

/// Throws away the keys waiting in the queue: whatever was typed while the program was not reading must not land in the next field.
fn discard_pending_keys() {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        if event::read().is_err() {
            break;
        }
    }
}

impl PairingPrompts for TerminalPrompts<'_> {
    /// Waits `pause` reading the keyboard: Ctrl+D or Ctrl+C gives up, every other key is thrown away. A terminal that fails to
    /// read counts as giving up.
    async fn wait(&mut self, pause: Duration) -> Wait {
        let deadline = Instant::now() + pause;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Wait::Continue;
            }
            match event::poll(left.min(POLL_INTERVAL)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press && key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'd')) => {
                        return Wait::Cancelled;
                    }
                    Ok(_) => {}
                    Err(_) => return Wait::Cancelled,
                },
                Ok(false) => tokio::task::yield_now().await,
                Err(_) => return Wait::Cancelled,
            }
        }
    }

    fn show(&mut self, shown: Shown) {
        let lines = shown_lines(shown, self.swap).into_iter().map(|line| (line, Style::default())).collect();
        // If the card cannot be drawn the person simply has nothing to type on the other device, and the wait runs out.
        let _ = render_message_card(self.terminal, "arca", accent_style(), lines);
    }

    async fn choose_offer(&mut self, offers: &[PendingOffer]) -> Option<usize> {
        if offers.len() == 1 {
            return Some(0);
        }
        let mut lines = vec![(tr!("arca.offers_waiting"), Style::default())];
        lines.extend(offers.iter().enumerate().map(|(i, offer)| (format!("  {}. {}", i + 1, offer.device_id), Style::default())));
        render_message_card(self.terminal, "arca", accent_style(), lines).ok()?;
        let typed = prompt_field(self.terminal, &tr!("arca.pick_offer"), "").await.ok().flatten()?;
        parse_pick(&typed, offers.len())
    }

    async fn ask(&mut self, ask: Ask) -> Option<String> {
        let title = ask_title(ask, self.swap);
        discard_pending_keys();
        prompt_field(self.terminal, &title, "").await.ok().flatten().filter(|typed| !typed.trim().is_empty())
    }
}

/// The offer a person chose by number (1 to `count`), as an index; `None` for anything else.
fn parse_pick(typed: &str, count: usize) -> Option<usize> {
    let typed = typed.trim();
    if typed.is_empty() || !typed.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    typed.parse::<usize>().ok()?.checked_sub(1).filter(|index| *index < count)
}

/// Asks for the hub address and this device's token (not echoed, not stored anywhere). `None` if the person gave up or left a
/// field empty.
async fn ask_hub_access(terminal: &mut CliTerminal) -> anyhow::Result<Option<HubAccess>> {
    let Some((url, token)) = ask_hub_credentials(terminal).await? else { return Ok(None) };
    Ok(Some(HubAccess::new(&url, &token)?))
}

/// The hub address and the token this device was issued for it, trimmed (`None` as in `ask_hub_access`). The token is read without
/// echo and goes nowhere but the connection: not to a file, not to the line history.
async fn ask_hub_credentials(terminal: &mut CliTerminal) -> anyhow::Result<Option<(String, String)>> {
    let Some(url) = prompt_field(terminal, &tr!("arca.ask_url"), "").await? else { return Ok(None) };
    let LineOutcome::Submitted(token) = read_secret_field(terminal, &tr!("arca.ask_token")).await? else { return Ok(None) };
    if url.trim().is_empty() || token.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some((url.trim().to_string(), token.trim().to_string())))
}

/// `/arca` — whether the vault is in arca mode, and the newer key epochs that stop it, if any. No network. Listing the epochs takes
/// the sync lock (it can wait for a cycle that is running), so a failure there does not hide the status: it is said on its own line.
async fn cmd_arca_status(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let mut lines = arca_status_lines(engine.is_initialized(), engine.arca_mode()?.as_ref());
    lines.extend(arca_epoch_check_lines(engine.pending().await));
    render_message_card(terminal, "arca", accent_style(), lines.into_iter().map(|line| (line, Style::default())).collect())
}

fn arca_status_lines(has_key: bool, mode: Option<&ArcaMode>) -> Vec<String> {
    match mode {
        Some(mode) => vec![tr!("arca.status_on"), if mode.secret_accepted { tr!("arca.status_logins_on") } else { tr!("arca.status_logins_off") }],
        None if has_key => vec![tr!("arca.status_off")],
        None => vec![tr!("arca.status_no_key")],
    }
}

/// The notice `/arca` adds when newer key epochs are pending. Empty when there are none.
fn arca_epoch_lines(pending: &[PendingEpoch]) -> Vec<String> {
    if pending.is_empty() {
        return Vec::new();
    }
    let items = pending.iter().map(|found| {
        let id = found.id.to_string();
        let short = id.rsplit('/').next().unwrap_or(&id).chars().take(16).collect::<String>();
        tr!("arca.epochs_item", epoch = found.epoch, id = short)
    });
    std::iter::once(tr!("arca.epochs_notice")).chain(items).chain(std::iter::once(tr!("arca.epochs_hint"))).collect()
}

/// The epoch notice, or, when the list could not be read, a line saying so: an error shown as "nothing pending" would hide a rotation.
fn arca_epoch_check_lines(listed: Result<Vec<PendingEpoch>, String>) -> Vec<String> {
    match listed {
        Ok(pending) => arca_epoch_lines(&pending),
        Err(message) => vec![tr!("arca.epochs_unreadable", error = message)],
    }
}

/// The pairs of `seen` that are still in `fresh`, in the order of `fresh`, each once. The whole pair is compared (epoch and id):
/// the next epoch is predictable, so the epoch alone would let a forged object hide the real rotation.
fn epochs_seen_still_pending(fresh: &[PendingEpoch], seen: &[PendingEpoch]) -> Vec<PendingEpoch> {
    fresh.iter().filter(|pending| seen.contains(pending)).copied().collect()
}

/// What `arca_ack_run` needs from the engine: the fresh list and the acknowledgement. A seam so the choice of WHICH pairs reach the
/// engine is tested with real `PendingEpoch`s (only a hub can make the engine have one pending).
trait EpochSource {
    async fn pending(&self) -> Result<Vec<PendingEpoch>, String>;
    async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String>;
}

impl EpochSource for warden_sync::SyncEngine {
    async fn pending(&self) -> Result<Vec<PendingEpoch>, String> {
        self.arca_pending_epochs().await.map_err(|e| format!("{e:#}"))
    }

    async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String> {
        self.arca_acknowledge_epochs(seen).await.map_err(|e| format!("{e:#}"))
    }
}

/// What `/arca ack` came to.
#[derive(Debug, PartialEq, Eq)]
enum ArcaAck {
    NothingPending,
    Cancelled,
    Done { count: usize },
    Failed(String),
}

/// The part of `/arca ack` that cannot be undone in practice: only after a typed yes, and only the pairs the person saw. `seen` is
/// looked up in a fresh list, so no id from the screen reaches the engine and whatever showed up after the person looked stays pending.
async fn arca_ack_run(source: &impl EpochSource, seen: &[PendingEpoch], answer: Option<&str>) -> ArcaAck {
    if !answer.is_some_and(is_yes) {
        return ArcaAck::Cancelled;
    }
    let fresh = match source.pending().await {
        Ok(fresh) => fresh,
        Err(message) => return ArcaAck::Failed(message),
    };
    let to_acknowledge = epochs_seen_still_pending(&fresh, seen);
    if to_acknowledge.is_empty() {
        return ArcaAck::NothingPending;
    }
    match source.acknowledge(&to_acknowledge).await {
        Ok(()) => ArcaAck::Done { count: to_acknowledge.len() },
        Err(message) => ArcaAck::Failed(message),
    }
}

fn arca_ack_lines(outcome: &ArcaAck) -> Vec<String> {
    match outcome {
        ArcaAck::NothingPending => vec![tr!("arca.ack_nothing")],
        ArcaAck::Cancelled => vec![tr!("arca.cancelled")],
        ArcaAck::Done { count } => vec![tr!("arca.ack_done", count = count)],
        ArcaAck::Failed(message) => vec![message.clone()],
    }
}

/// `/arca ack` — lists the newer key epochs and, after a typed yes, acknowledges the ones just listed. The keys typed before the
/// question are thrown away so a stray Enter cannot answer it.
async fn cmd_arca_ack(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if engine.arca_mode()?.is_none() {
        return arca_error_card(terminal, tr!("arca.ack_needs_mode"));
    }
    let seen = match engine.pending().await {
        Ok(seen) => seen,
        Err(message) => return arca_error_card(terminal, tr!("arca.epochs_unreadable", error = message)),
    };
    if seen.is_empty() {
        return render_message_card(terminal, "arca", accent_style(), vec![(tr!("arca.ack_none_pending"), Style::default())]);
    }
    render_message_card(terminal, "arca", accent_style(), arca_epoch_lines(&seen).into_iter().map(|line| (line, Style::default())).collect())?;
    discard_pending_keys();
    let answer = prompt_field(terminal, &tr!("arca.ack_confirm"), "").await?;
    let outcome = arca_ack_run(&engine, &seen, answer.as_deref()).await;
    let style = if matches!(outcome, ArcaAck::Failed(_)) { error_style() } else { accent_style() };
    render_message_card(terminal, "arca", style, arca_ack_lines(&outcome).into_iter().map(|line| (line, Style::default())).collect())
}

/// What `/arca on` came to.
#[derive(Debug, PartialEq, Eq)]
enum ArcaOn {
    AlreadyOn,
    NoKey,
    Cancelled,
    Done { needs_reload: bool },
    Failed(String),
}

/// Only an explicit yes confirms (the same words as every other confirmation here); an empty line, `n` or anything else does not.
fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

/// Why `/arca on` has nothing to ask: the mode is already on, or this computer has no vault key to build the first manifest from.
fn arca_on_blocker(has_key: bool, mode_on: bool) -> Option<ArcaOn> {
    if mode_on {
        Some(ArcaOn::AlreadyOn)
    } else if !has_key {
        Some(ArcaOn::NoKey)
    } else {
        None
    }
}

/// The part of `/arca on` that cannot be undone: it turns the mode on only after a typed yes, and through the same sequence as the
/// desktop (the Pull first, nothing turned on if it fails). No Send can be waiting here (the CLI has no such state), and the MCP
/// logins stay out: the person accepts them later.
async fn arca_on_run(engine: &warden_sync::SyncEngine, answer: Option<&str>, now_ms: u64) -> ArcaOn {
    if !answer.is_some_and(is_yes) {
        return ArcaOn::Cancelled;
    }
    match engine.arca_enable_after_pull(false, false, now_ms).await {
        Ok(needs_reload) => ArcaOn::Done { needs_reload },
        Err(err) => ArcaOn::Failed(format!("{err:#}")),
    }
}

fn arca_on_lines(outcome: &ArcaOn) -> Vec<String> {
    match outcome {
        ArcaOn::AlreadyOn => vec![tr!("arca.already_on")],
        ArcaOn::NoKey => vec![tr!("arca.status_no_key")],
        ArcaOn::Cancelled => vec![tr!("arca.cancelled")],
        ArcaOn::Done { needs_reload } => std::iter::once(tr!("arca.on_done")).chain(pull_reload_note(*needs_reload)).collect(),
        ArcaOn::Failed(message) => vec![message.clone()],
    }
}

/// `/arca on` — turns the arca mode on for this vault. One way only (arca ADR 0012), so it explains that and asks to confirm; the
/// keys typed before the question are thrown away so a stray Enter cannot answer it.
async fn cmd_arca_on(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if let Some(blocked) = arca_on_blocker(engine.is_initialized(), engine.arca_mode()?.is_some()) {
        return render_arca_on(terminal, &blocked);
    }
    discard_pending_keys();
    let answer = prompt_field(terminal, &tr!("arca.on_confirm"), "").await?;
    let outcome = arca_on_run(&engine, answer.as_deref(), now_millis()).await;
    if let ArcaOn::Done { needs_reload } = &outcome {
        session.reload_requested |= needs_reload;
    }
    render_arca_on(terminal, &outcome)
}

fn render_arca_on(terminal: &mut CliTerminal, outcome: &ArcaOn) -> anyhow::Result<()> {
    let style = if matches!(outcome, ArcaOn::Failed(_)) { error_style() } else { accent_style() };
    render_message_card(terminal, "arca", style, arca_on_lines(outcome).into_iter().map(|line| (line, Style::default())).collect())
}

/// `/arca sync` — one cycle against a hub (arca ADR 0012). Asks for the address and the token like the pairing does, so nothing
/// secret is read from or written to a file here. Turning the mode on is `/arca on`.
async fn cmd_arca_sync(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if engine.arca_mode()?.is_none() {
        return arca_error_card(terminal, tr!("arca.sync_needs_mode"));
    }
    let Some((url, token)) = ask_hub_credentials(terminal).await? else { return cancelled_card(terminal) };
    let hub = match hub_store_for(&url, &token) {
        Ok(hub) => hub,
        Err(message) => return arca_error_card(terminal, message),
    };
    match engine.arca_sync(&hub, now_millis()).await {
        Ok(report) => render_message_card(terminal, "arca", accent_style(), arca_report_lines(&report).into_iter().map(|line| (line, Style::default())).collect()),
        Err(err) if is_auth_rejected(&err) => arca_error_card(terminal, tr!("arca.hub_refused")),
        Err(err) => arca_error_card(terminal, format!("{err:#}")),
    }
}

fn arca_error_card(terminal: &mut CliTerminal, message: String) -> anyhow::Result<()> {
    render_message_card(terminal, "arca", error_style(), vec![(message, error_style())])
}

/// The hub as the arca's client wants it, from what was typed. The refusals (plain `http` to another machine, an address with a
/// path, an unusable token) are the desktop's: the same function.
fn hub_store_for(url: &str, token: &str) -> Result<warden_sync::arca_sync::HubStore, String> {
    let hub = SavedHub { id: String::new(), name: url.to_string(), url: url.to_string() };
    let identity = RemoteIdentity { device_id: String::new(), device_token: Some(token.to_string()) };
    arca_hub::hub_store(&hub, &identity)
}

/// What one cycle did, one line each. What it left out or set aside is said on purpose: "done" alone would hide that a class was
/// not sent (the arca refuses to publish an emptied class) or that files are waiting for the person on the desktop.
fn arca_report_lines(report: &ArcaSyncReport) -> Vec<String> {
    let mut lines = vec![tr!("arca.sync_done", pulled = report.pulled, pushed = report.pushed, files = report.files_changed)];
    if report.conflicts > 0 {
        lines.push(tr!("arca.sync_conflicts", conflicts = report.conflicts, merges = report.merges));
    }
    if report.secret_losers_saved > 0 {
        lines.push(tr!("arca.sync_losers", count = report.secret_losers_saved));
    }
    if report.secret_ignored > 0 {
        lines.push(tr!("arca.sync_ignored", count = report.secret_ignored));
    }
    if !report.secret_skipped.is_empty() {
        let files = report.secret_skipped.iter().map(|skipped| format!("{} ({})", skipped.path, skipped.reason)).collect::<Vec<_>>().join(", ");
        lines.push(tr!("arca.sync_skipped", files = files));
    }
    if report.recovered_kept > 0 {
        lines.push(tr!("arca.sync_kept", count = report.recovered_kept));
    }
    if report.vault_emptied {
        lines.push(tr!("arca.sync_vault_emptied"));
    }
    if report.secret_emptied {
        lines.push(tr!("arca.sync_secret_emptied"));
    }
    if report.secret_folder_missing {
        lines.push(tr!("arca.sync_folder_missing"));
    }
    lines
}

fn cancelled_card(terminal: &mut CliTerminal) -> anyhow::Result<()> {
    render_message_card(terminal, "arca", accent_style(), vec![(tr!("arca.cancelled"), Style::default())])
}

/// `/arca pair offer` — on the new device: publishes an offer, shows its code and waits for the key (arca ADR 0016).
async fn cmd_arca_pair_offer(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let Some(access) = ask_hub_access(terminal).await? else { return cancelled_card(terminal) };
    let Some(device_id) = prompt_field(terminal, &tr!("arca.ask_device"), "").await?.map(|id| id.trim().to_string()).filter(|id| !id.is_empty()) else {
        return cancelled_card(terminal);
    };
    let result = run_offer(&access, &device_id, &engine, &mut TerminalPrompts { terminal, swap: false }, ARCA_OFFER_POLL, ARCA_OFFER_TIMEOUT).await;
    finish_arca_pairing(terminal, result, "arca.stored_ok")
}

/// `/arca pair grant` — on the device that has the key: lists the waiting offers and hands the key to the chosen one.
async fn cmd_arca_pair_grant(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let Some(access) = ask_hub_access(terminal).await? else { return cancelled_card(terminal) };
    let result = run_grant(&access, &engine, &mut TerminalPrompts { terminal, swap: false }).await;
    finish_arca_pairing(terminal, result, "arca.granted_ok")
}

fn finish_arca_pairing(terminal: &mut CliTerminal, result: Result<(), FlowError>, ok_key: &str) -> anyhow::Result<()> {
    match result {
        Ok(()) => render_message_card(terminal, "arca", accent_style(), vec![(tr!(ok_key), Style::default())]),
        Err(FlowError::Cancelled) => cancelled_card(terminal),
        Err(e) => render_message_card(terminal, "arca", error_style(), vec![(e.to_string(), error_style())]),
    }
}

/// What a key swap that did not happen says, one line. `KeySwapError` only has `Debug`, so every variant gets its own words here.
fn key_swap_lines(error: &KeySwapError) -> Vec<String> {
    let line = match error {
        KeySwapError::NotInArcaMode => tr!("arca.swap_needs_mode"),
        KeySwapError::NothingNewer => tr!("arca.swap_nothing_newer"),
        KeySwapError::NotNewer => tr!("arca.swap_not_newer"),
        KeySwapError::NoKey(message) => tr!("arca.swap_no_key", message = message),
        KeySwapError::NoOffers => tr!("arca.swap_no_offers"),
        KeySwapError::Cancelled => tr!("arca.cancelled"),
        KeySwapError::TimedOut => tr!("arca.swap_timed_out"),
        KeySwapError::HashMismatch => tr!("arca.swap_hash_mismatch"),
        KeySwapError::Pairing(error) => error.to_string(),
        KeySwapError::Failed(message) => tr!("arca.swap_failed", message = message),
    };
    vec![line]
}

fn finish_arca_swap(terminal: &mut CliTerminal, result: Result<(), KeySwapError>, ok_key: &str) -> anyhow::Result<()> {
    match result {
        Ok(()) => render_message_card(terminal, "arca", accent_style(), vec![(tr!(ok_key), Style::default())]),
        Err(KeySwapError::Cancelled) => cancelled_card(terminal),
        Err(error) => render_message_card(terminal, "arca", error_style(), key_swap_lines(&error).into_iter().map(|line| (line, error_style())).collect()),
    }
}

/// `/arca swap grant` — on the device that rotated the key: hands the new key (and shows the head code) to a device that is still on
/// the old one.
async fn cmd_arca_swap_grant(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    let Some(access) = ask_hub_access(terminal).await? else { return cancelled_card(terminal) };
    let result = run_grant_replacing(&access, &engine, &mut TerminalPrompts { terminal, swap: true }).await;
    finish_arca_swap(terminal, result, "arca.swap_granted_ok")
}

/// `/arca swap offer` — on the device that stayed on the old key: asks to confirm, publishes an offer and replaces its key with the
/// new one (arca ADR 0018, Warden ADR 0002). `run_replace_key` checks the preconditions again; they are looked at here first only so
/// the person is not asked to confirm something that cannot happen.
async fn cmd_arca_swap_offer(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if engine.arca_mode()?.is_none() {
        return finish_arca_swap(terminal, Err(KeySwapError::NotInArcaMode), "arca.swap_stored_ok");
    }
    let pending = match engine.pending().await {
        Ok(pending) => pending,
        // Nothing started yet, so this is not "the swap did not finish".
        Err(message) => return arca_error_card(terminal, tr!("arca.epochs_unreadable", error = message)),
    };
    if pending.is_empty() {
        return finish_arca_swap(terminal, Err(KeySwapError::NothingNewer), "arca.swap_stored_ok");
    }
    // One question for both the pairing and the hub's reader: they are the same hub and the same token.
    let Some((url, token)) = ask_hub_credentials(terminal).await? else { return cancelled_card(terminal) };
    let access = match HubAccess::new(&url, &token) {
        Ok(access) => access,
        Err(error) => return arca_error_card(terminal, error.to_string()),
    };
    let hub = match hub_store_for(&url, &token) {
        Ok(hub) => hub,
        Err(message) => return arca_error_card(terminal, message),
    };
    let Some(device_id) = prompt_field(terminal, &tr!("arca.ask_device"), "").await?.map(|id| id.trim().to_string()).filter(|id| !id.is_empty()) else {
        return cancelled_card(terminal);
    };
    // What the confirmation is about: the person sees the newer epochs before being asked whether to follow one.
    render_message_card(terminal, "arca", accent_style(), arca_epoch_lines(&pending).into_iter().map(|line| (line, Style::default())).collect())?;
    discard_pending_keys();
    let answer = prompt_field(terminal, &tr!("arca.swap_confirm"), "").await?;
    if !answer.as_deref().is_some_and(is_yes) {
        return cancelled_card(terminal);
    }
    let result = run_replace_key(&access, &hub, &device_id, &engine, &mut TerminalPrompts { terminal, swap: true }, ARCA_OFFER_POLL, ARCA_OFFER_TIMEOUT, now_millis()).await;
    finish_arca_swap(terminal, result, "arca.swap_stored_ok")
}

/// What `/arca rotate` and `/arca rotate resume` came to once the person answered everything. It never holds the phrase or a word
/// the person typed: only what the core said.
#[derive(Debug, PartialEq, Eq)]
enum ArcaRotate {
    Cancelled,
    Done,
    Refused(RotationError),
}

/// The part of a rotation that writes: it goes through `PendingRotation::confirm`, which checks the declaration and the words
/// before the first write and takes `self`, so a key is tried once. `declaration` is the answer to "did you revoke the lost
/// device's token on the hub?" and `confirmation` the answer to "change the key now?"; only a typed yes counts for either (the
/// same words as every other confirmation here). Without the declaration the core refuses and says why, so the confirmation is
/// only decisive once the declaration was given. `typed_words` is empty when the 24 words were typed (`/arca rotate resume`).
async fn arca_rotate_run(engine: &warden_sync::SyncEngine, pending: PendingRotation, typed_words: &[String], declaration: Option<&str>, confirmation: Option<&str>, now_ms: u64) -> ArcaRotate {
    let declared = declaration.is_some_and(is_yes);
    if declared && !confirmation.is_some_and(is_yes) {
        return ArcaRotate::Cancelled;
    }
    let typed: Vec<&str> = typed_words.iter().map(String::as_str).collect();
    match pending.confirm(&typed, declared, engine, now_ms).await {
        Ok(()) => ArcaRotate::Done,
        Err(error) => ArcaRotate::Refused(error),
    }
}

/// What a rotation that did not happen says, in the CLI's own words in both languages (the core's `Display` is English only and says
/// things that are wrong here: "nothing was changed" is false after a stop half way). The match is exhaustive on purpose: a new
/// `RotationError` must stop the build here until someone decides what the terminal says for it.
///
/// `resuming` is `/arca rotate resume`: no phrase was shown, so nothing says it is void, the next step is to resume again, and an
/// earlier try may have written, so nothing says "the key was not changed". In `/arca rotate`, every refusal that wrote nothing
/// ends by saying the phrase that was shown is discarded (the person may throw the paper away). `Failed` is the one that may have
/// written something: it sends the person to resume with the same 24 words and never to a new key, and never calls the phrase void.
fn rotation_error_lines(error: &RotationError, resuming: bool) -> Vec<String> {
    let command = if resuming { "/arca rotate resume" } else { "/arca rotate" };
    let (line, wrote_nothing) = match error {
        RotationError::NotDeclared => (tr!("arca.rotate_not_declared", command = command), true),
        RotationError::WordMismatch => (tr!("arca.rotate_word_mismatch"), true),
        RotationError::BadPhrase => (tr!("arca.rotate_bad_phrase"), false),
        RotationError::NotInArcaMode => (tr!("arca.rotate_needs_mode"), true),
        RotationError::Refused(why) => (tr!("arca.rotate_refused", why = why, command = command), true),
        RotationError::NewerEpoch => (tr!("arca.rotate_newer_epoch"), true),
        RotationError::Failed(why) => (tr!("arca.rotate_partial", why = why), false),
    };
    let mut lines = vec![line];
    if wrote_nothing && !resuming {
        lines.push(tr!("arca.rotate_discarded"));
    }
    lines
}

/// The card of a rotation that worked: whether the new epoch reached the hub. A rotation writes only on this computer; the normal
/// cycle is what publishes it, so `sent` is `None` when no hub was given and the error when that cycle failed.
fn arca_rotated_lines(sent: Option<&Result<ArcaSyncReport, String>>) -> Vec<String> {
    match sent {
        None => vec![tr!("arca.rotate_done_no_hub")],
        Some(Ok(report)) => std::iter::once(tr!("arca.rotate_done_sent")).chain(arca_report_lines(report)).collect(),
        Some(Err(message)) => vec![tr!("arca.rotate_done_not_sent", message = message)],
    }
}

/// The 24 words as rows of `per_row` numbered words, so the person can write them down in order. The only place the phrase becomes
/// text; the rows live as long as the screen that shows them (`show_phrase_once`) and are dropped right after.
fn phrase_rows<'a>(words: impl Iterator<Item = &'a str>, per_row: usize) -> Vec<String> {
    let numbered: Vec<String> = words.enumerate().map(|(index, word)| format!("{:>2}. {:<8}", index + 1, word)).collect();
    numbered.chunks(per_row.max(1)).map(|row| row.join("  ").trim_end().to_string()).collect()
}

/// How many words fit on one row of a terminal `width` columns wide, from 1 to 4. An item is 12 columns ("24. " and 8 for the
/// longest word of the list) and the gap between two is 2, so `n` items need `14 * n - 2`; the box border takes 2 columns and 2 more
/// are left as slack, which is the `+ 2` in `14 * n + 2` that this works out.
fn phrase_per_row(width: usize) -> usize {
    (width.saturating_sub(2) / 14).clamp(1, 4)
}

/// Words in the phrase of the vault key (arca-core `PHRASE_WORDS`).
const PHRASE_WORDS: usize = 24;
/// The narrowest terminal the box is drawn in: one word per row, and the keys row ("Enter: done  Ctrl+D: cancel", at most 27
/// characters) fits inside the 2 border columns.
const PHRASE_MIN_WIDTH: usize = 29;
/// The box is taller than its rows of words by two border lines and the keys row.
const PHRASE_BOX_EXTRA_LINES: usize = 3;

/// Whether a terminal `width` x `height` can show every word of the phrase at once, and with how many words per row if it can. A
/// viewport taller than the terminal is cut, and the person would confirm having seen all 24 words when some were off the screen,
/// so this is asked BEFORE a key is made or anything is shown.
fn phrase_layout(width: usize, height: usize) -> Option<usize> {
    if width < PHRASE_MIN_WIDTH {
        return None;
    }
    let per_row = phrase_per_row(width);
    (PHRASE_WORDS.div_ceil(per_row) + PHRASE_BOX_EXTRA_LINES <= height).then_some(per_row)
}

/// Paints the phrase box into `buf`: the rows of words, then a dim row with the keys (the title can be cut in a narrow terminal, the
/// keys row is short enough not to be).
fn draw_phrase_box(buf: &mut Buffer, area: Rect, rows: &[String], title: &str, keys: &str) {
    use ratatui::widgets::Widget;
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::default().fg(accent_color())).title(title.to_string());
    let inner = block.inner(area);
    block.render(area, buf);
    let mut lines: Vec<Line> = rows.iter().map(|row| Line::raw(row.clone())).collect();
    lines.push(Line::styled(keys.to_string(), dim_style()));
    Paragraph::new(lines).render(inner, buf);
}

fn render_phrase_box(frame: &mut ratatui::Frame, rows: &[String], title: &str, keys: &str) {
    let area = frame.area();
    draw_phrase_box(frame.buffer_mut(), area, rows, title, keys);
}

/// How the screen with the phrase ended.
#[derive(Debug, PartialEq, Eq)]
enum PhraseScreen {
    /// Enter: the person wrote the words down.
    WrittenDown,
    /// Ctrl+C or Ctrl+D.
    Cancelled,
    /// The terminal was resized while the words were shown: the rows were worked out for the old size and may be cut now, so
    /// the person must not go on as if they had seen all 24 words.
    Resized,
}

/// Draws the phrase box until the person presses Enter, gives up with Ctrl+C / Ctrl+D, or the terminal is resized.
fn wait_for_the_phrase_to_be_read(terminal: &mut CliTerminal, rows: &[String], shown_for: (u16, u16)) -> anyhow::Result<PhraseScreen> {
    let (title, keys) = (tr!("arca.rotate_phrase_title"), tr!("arca.rotate_phrase_keys"));
    loop {
        // Looked at before EVERY frame, not only when a resize event arrives: `discard_pending_keys` reads (and so loses) resize
        // events too, and an event can come in between two polls.
        if !phrase_screen_still_valid(shown_for, crossterm::terminal::size().ok()) {
            return Ok(PhraseScreen::Resized);
        }
        terminal.draw(|frame| render_phrase_box(frame, rows, &title, &keys))?;
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => return Ok(PhraseScreen::WrittenDown),
                KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(PhraseScreen::Cancelled),
                _ => {}
            },
            Event::Resize(..) => return Ok(PhraseScreen::Resized),
            _ => {}
        }
    }
}

/// Shows the phrase ONCE, in the live box (the viewport), and clears it when the person is done.
///
/// What this guarantees: no card and no `insert_before` is used, so Warden itself never writes the words into the terminal's scroll
/// history, and `terminal.clear()` erases the box, whatever way the screen ended. What it does NOT guarantee, and the intro card
/// says so: a terminal that reflows on a resize, tmux/screen copy mode, a screen recorder or `script`, and the emulators that keep the
/// cells in their scrollback after a full clear can still hold a copy of what was drawn. The terminal goes back to its usual height
/// before the result is looked at.
///
/// `shown_for` is the terminal size (columns, lines) the rows were laid out for. After a resize `terminal.clear()` is NOT enough:
/// it erases from the viewport origin the terminal object last knew, which only `draw` recomputes, so the words could stay on
/// the screen. On that path the whole screen and its scrollback are wiped (`clear_screen`, as when the program starts), and the
/// card that follows says the words may still be visible, not that they were erased.
fn show_phrase_once(terminal: &mut CliTerminal, rows: &[String], shown_for: (u16, u16)) -> anyhow::Result<PhraseScreen> {
    terminal.clear()?;
    *terminal = new_inline_terminal_with_height((rows.len() + PHRASE_BOX_EXTRA_LINES) as u16)?;
    // This reads (and loses) a resize event that came in meanwhile, so the size is looked at again right after.
    discard_pending_keys();
    let screen = if phrase_screen_still_valid(shown_for, crossterm::terminal::size().ok()) {
        wait_for_the_phrase_to_be_read(terminal, rows, shown_for)
    } else {
        Ok(PhraseScreen::Resized)
    };
    if matches!(screen, Ok(PhraseScreen::Resized)) {
        let _ = terminal.autoresize();
        let _ = terminal.clear();
        clear_screen()?;
    } else {
        terminal.clear()?;
    }
    *terminal = new_inline_terminal()?;
    screen
}

/// The person at the terminal, for the questions of a rotation: one field per question, `None` when they gave up (Ctrl+D). A seam,
/// so the ORDER of the questions and what is done with the answers are tested without a terminal.
trait FieldAsker {
    async fn ask(&mut self, title: &str) -> anyhow::Result<Option<String>>;
}

/// The real one: a bordered field, with the keys typed before the question thrown away so a stray Enter cannot answer it.
struct TerminalAsker<'a> {
    terminal: &'a mut CliTerminal,
}

impl FieldAsker for TerminalAsker<'_> {
    async fn ask(&mut self, title: &str) -> anyhow::Result<Option<String>> {
        discard_pending_keys();
        prompt_field(self.terminal, title, "").await
    }
}

/// The words of the new phrase the person types back, at `positions` (counting from 1). A blank answer asks the SAME word again: an
/// accidental Enter must not throw away the phrase they just wrote down. `None` when they gave up.
async fn typed_words_from(asker: &mut impl FieldAsker, positions: &[usize]) -> anyhow::Result<Option<Vec<String>>> {
    let mut typed = Vec::with_capacity(positions.len());
    for position in positions {
        loop {
            let Some(word) = asker.ask(&tr!("arca.rotate_ask_word", position = position)).await? else { return Ok(None) };
            if !word.trim().is_empty() {
                typed.push(word);
                break;
            }
        }
    }
    Ok(Some(typed))
}

/// The two answers that decide a rotation, in this order: whether the lost device's token was revoked on the hub and, only when that
/// was a yes, whether to change the key now. `None` when the person gave up at the first question; a Ctrl+D at the second comes
/// back as no confirmation (which `arca_rotate_run` reads as cancelled).
async fn rotation_answers_from(asker: &mut impl FieldAsker) -> anyhow::Result<Option<(String, Option<String>)>> {
    let Some(declaration) = asker.ask(&tr!("arca.rotate_ask_revoked")).await? else { return Ok(None) };
    if !is_yes(&declaration) {
        return Ok(Some((declaration, None)));
    }
    let confirmation = asker.ask(&tr!("arca.rotate_confirm")).await?;
    Ok(Some((declaration, confirmation)))
}

/// Everything after the phrase is in the person's hands (or typed again, for a resume): the words asked back (none for a resume),
/// the declaration, the confirmation, and the rotation with exactly what they answered. Both commands go through here, so what the
/// person answers is what decides it.
async fn ask_and_rotate(engine: &warden_sync::SyncEngine, asker: &mut impl FieldAsker, pending: PendingRotation, now_ms: u64) -> anyhow::Result<ArcaRotate> {
    let Some(typed) = typed_words_from(asker, &pending.positions()).await? else { return Ok(ArcaRotate::Cancelled) };
    let Some((declaration, confirmation)) = rotation_answers_from(asker).await? else { return Ok(ArcaRotate::Cancelled) };
    Ok(arca_rotate_run(engine, pending, &typed, Some(declaration.as_str()), confirmation.as_deref(), now_ms).await)
}

/// The cycle that sends the new epoch: a rotation writes only here, and the normal cycle publishes it (the desktop runs one right
/// after too). Asks for the hub like `/arca sync` does; `None` when no hub was given.
async fn send_the_new_epoch(terminal: &mut CliTerminal, engine: &warden_sync::SyncEngine) -> anyhow::Result<Option<Result<ArcaSyncReport, String>>> {
    let Some((url, token)) = ask_hub_credentials(terminal).await? else { return Ok(None) };
    let hub = match hub_store_for(&url, &token) {
        Ok(hub) => hub,
        Err(message) => return Ok(Some(Err(message))),
    };
    Ok(Some(match engine.arca_sync(&hub, now_millis()).await {
        Ok(report) => Ok(report),
        Err(err) if is_auth_rejected(&err) => Err(tr!("arca.hub_refused")),
        Err(err) => Err(format!("{err:#}")),
    }))
}

/// Says how the rotation ended. After one that worked, it says so before asking for the hub (the key has already changed on this
/// computer whatever the hub answers), then runs the cycle that sends it. From that point on nothing may hide that the key changed:
/// a terminal that fails while asking for the hub or drawing the notice becomes "changed here, not sent" (`rotate_done_not_sent`),
/// not an error that says nothing about the rotation. `resuming` is `/arca rotate resume` (see `rotation_error_lines`).
async fn finish_arca_rotate(terminal: &mut CliTerminal, engine: &warden_sync::SyncEngine, outcome: ArcaRotate, resuming: bool) -> anyhow::Result<()> {
    match outcome {
        ArcaRotate::Cancelled => rotation_cancelled_card(terminal, resuming),
        ArcaRotate::Refused(error) => render_message_card(terminal, "arca", error_style(), rotation_error_lines(&error, resuming).into_iter().map(|line| (line, error_style())).collect()),
        ArcaRotate::Done => {
            let _ = render_message_card(terminal, "arca", accent_style(), vec![(tr!("arca.rotate_sending"), Style::default())]);
            let sent = match send_the_new_epoch(terminal, engine).await {
                Ok(sent) => sent,
                Err(error) => Some(Err(format!("{error:#}"))),
            };
            let style = if matches!(sent, Some(Err(_))) { error_style() } else { accent_style() };
            render_message_card(terminal, "arca", style, arca_rotated_lines(sent.as_ref()).into_iter().map(|line| (line, Style::default())).collect())
        }
    }
}

/// `/arca rotate` — after losing a device (arca ADR 0018, Warden ADR 0002, P-49): makes a new key, shows its 24 words once, asks for
/// three of them back, asks whether the lost device's token was revoked on the hub and then to confirm, and rotates. The phrase
/// exists only in memory and on the screen while it is shown: it is not in a card, not in the line history and not in any `Debug`.
/// Nothing is shown for a vault that is not in arca mode, and the key is dropped on every way out before the confirmation.
async fn cmd_arca_rotate(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if engine.arca_mode()?.is_none() {
        return arca_error_card(terminal, tr!("arca.rotate_needs_mode"));
    }
    // Before a key is made or anything is shown: a terminal that cannot show all 24 words would let the person confirm a view that
    // was cut. The size is looked at again while the words are on the screen (`PhraseScreen::Resized`).
    // A size that cannot be read is a refusal too: guessing one (80x24) could show a box that does not fit.
    let Ok((columns, lines)) = crossterm::terminal::size() else { return arca_error_card(terminal, tr!("arca.rotate_size_unknown")) };
    let Some(per_row) = phrase_layout(columns as usize, lines as usize) else { return arca_error_card(terminal, tr!("arca.rotate_terminal_small")) };
    let pending = PendingRotation::generate()?;
    render_message_card(terminal, "arca", accent_style(), vec![(tr!("arca.rotate_intro"), Style::default())])?;
    let rows = phrase_rows(pending.phrase()?.words(), per_row);
    let screen = show_phrase_once(terminal, &rows, (columns, lines));
    drop(rows);
    match screen? {
        PhraseScreen::WrittenDown => {}
        PhraseScreen::Cancelled => return rotation_cancelled_card(terminal, false),
        PhraseScreen::Resized => return arca_error_card(terminal, tr!("arca.rotate_resized")),
    }
    let outcome = ask_and_rotate(&engine, &mut TerminalAsker { terminal: &mut *terminal }, pending, now_millis()).await?;
    finish_arca_rotate(terminal, &engine, outcome, false).await
}

/// `/arca rotate resume` — finishes a rotation that stopped half way (the core says so with `RotationError::Failed`): the same 24
/// words are typed again, in a field that is not echoed and not kept, and they are the confirmation, so no word is asked back. A
/// phrase that is not valid is said at once, before anything else is asked.
async fn cmd_arca_rotate_resume(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_sync_engine(session)?;
    if engine.arca_mode()?.is_none() {
        return arca_error_card(terminal, tr!("arca.rotate_needs_mode"));
    }
    render_message_card(terminal, "arca", accent_style(), vec![(tr!("arca.rotate_resume_intro"), Style::default())])?;
    discard_pending_keys();
    let LineOutcome::Submitted(typed_phrase) = read_secret_field(terminal, &tr!("arca.rotate_ask_phrase")).await? else { return cancelled_card(terminal) };
    if typed_phrase.trim().is_empty() {
        return cancelled_card(terminal);
    }
    let pending = match PendingRotation::from_phrase(&typed_phrase) {
        Ok(pending) => pending,
        Err(error) => return finish_arca_rotate(terminal, &engine, ArcaRotate::Refused(error), true).await,
    };
    drop(typed_phrase);
    let outcome = ask_and_rotate(&engine, &mut TerminalAsker { terminal: &mut *terminal }, pending, now_millis()).await?;
    finish_arca_rotate(terminal, &engine, outcome, true).await
}

/// How long the new device waits for a grant: the same 10 minutes an offer lives on the hub.
const ARCA_OFFER_TIMEOUT: Duration = Duration::from_secs(600);
const ARCA_OFFER_POLL: Duration = Duration::from_secs(2);

/// `/sync git push` (P63) — no QR/phone step like `/sync push` has: a git remote needs no
/// per-push approval, so this is a single call straight through.
async fn cmd_sync_git_push(terminal: &mut CliTerminal, session: &CliSession) -> anyhow::Result<()> {
    let engine = make_git_sync_engine(session)?;
    let Some(outcome) = engine.push().await? else {
        return render_message_card(
            terminal,
            "sync",
            accent_style(),
            vec![(tr!("sync.nothing_to_send"), Style::default())],
        );
    };
    render_message_card(
        terminal,
        "sync",
        accent_style(),
        vec![(tr!("sync.sent_commit", commit = &outcome.commit_sha[..12.min(outcome.commit_sha.len())], files = outcome.files_changed), Style::default())],
    )
}

async fn cmd_sync_git_pull(terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    let engine = make_git_sync_engine(session)?;
    let outcome = engine.pull().await?;
    session.reload_requested |= outcome.needs_reload();
    let mut lines = vec![(
        tr!(
            "sync.git_pull_done",
            commits = outcome.commits_applied,
            written = outcome.files_written,
            deleted = outcome.files_deleted,
            ignored = if outcome.files_ignored > 0 { tr!("sync.ignored", count = outcome.files_ignored) } else { String::new() },
            config = if outcome.config_updated { tr!("sync.config_updated") } else { String::new() }
        ),
        Style::default(),
    )];
    lines.extend(pull_reload_note(outcome.needs_reload()).map(|note| (note, Style::default())));
    for warning in &outcome.warnings {
        lines.push((warning.clone(), Style::default()));
    }
    render_message_card(terminal, "sync", accent_style(), lines)
}

/// `/folder` (P102): shows the session's working folder (`None`), picks one (`Some(path)`) or clears it (`Some("")`).
/// A relative path is read from where the terminal was opened. It has to be a folder that exists, and it can only be
/// chosen or changed before the first message — the same rule as the desktop and the web, so a conversation never moves
/// to another folder halfway.
async fn cmd_folder(terminal: &mut CliTerminal, session: &mut CliSession, change: Option<String>) -> anyhow::Result<()> {
    let say = |terminal: &mut CliTerminal, style: Style, text: String| render_message_card(terminal, &tr!("folder.title"), style, vec![(text, Style::default())]);
    let Some(path) = change else {
        let text = match &session.workdir {
            Some(folder) => tr!("folder.working_in", folder = folder),
            None => tr!("folder.none"),
        };
        return say(terminal, accent_style(), text);
    };
    if session.turn_count > 0 {
        return say(terminal, error_style(), tr!("folder.locked"));
    }
    if path.is_empty() {
        session.workdir = None;
        return say(terminal, accent_style(), tr!("folder.cleared"));
    }
    let resolved = match std::fs::canonicalize(&path) {
        Ok(resolved) if resolved.is_dir() => resolved,
        Ok(_) => return say(terminal, error_style(), tr!("folder.not_a_folder", path = path)),
        Err(_) => return say(terminal, error_style(), tr!("folder.missing", path = path)),
    };
    let folder = resolved.to_string_lossy().into_owned();
    if let Err(err) = warden_core::project::validate_workdir(&folder) {
        return say(terminal, error_style(), format!("{err:#}"));
    }
    session.workdir = Some(folder.clone());
    say(terminal, accent_style(), tr!("folder.set", folder = folder))
}

/// `/lang` (P136): shows the language, or switches it for the whole session and keeps the choice for the next one.
async fn cmd_lang(terminal: &mut CliTerminal, value: Option<String>) -> anyhow::Result<()> {
    let name = |locale: i18n::Locale| match locale {
        i18n::Locale::En => "English",
        i18n::Locale::PtBr => "Português (Brasil)",
    };
    let Some(value) = value else {
        return render_message_card(terminal, &tr!("lang.title"), accent_style(), vec![(tr!("lang.current", language = name(i18n::current())), Style::default())]);
    };
    let Some(locale) = i18n::Locale::parse(&value) else {
        return render_message_card(terminal, &tr!("common.error"), error_style(), vec![(tr!("lang.unknown", value = value), Style::default())]);
    };
    i18n::init(locale);
    let mut lines = vec![(tr!("lang.set", language = name(locale)), Style::default())];
    if let Some(path) = i18n::saved_path() {
        if let Err(err) = i18n::save(&path, locale) {
            lines.push((tr!("lang.not_saved", error = err), dim_style()));
        }
    }
    render_message_card(terminal, &tr!("lang.title"), accent_style(), lines)
}

async fn handle_command(command: Command, terminal: &mut CliTerminal, session: &mut CliSession) -> anyhow::Result<()> {
    match command {
        Command::Exit => unreachable!("Command::Exit is handled by the caller before dispatch"),
        Command::Help => cmd_help(terminal).await,
        Command::Lang(value) => cmd_lang(terminal, value).await,
        Command::Usage => cmd_usage(terminal, session).await,
        Command::FolderShow => cmd_folder(terminal, session, None).await,
        Command::FolderSet(path) => cmd_folder(terminal, session, Some(path)).await,
        Command::FolderOff => cmd_folder(terminal, session, Some(String::new())).await,
        Command::Limits => cmd_limits(terminal, session).await,
        Command::LimitsAdd => wizard_limits_add(terminal, session).await,
        Command::LimitsEdit(id) => wizard_limits_edit(terminal, session, id).await,
        Command::LimitsRemove(id) => cmd_limits_remove(terminal, session, id).await,
        Command::LimitsOff => cmd_limits_off(terminal, session).await,
        Command::LimitsReset => cmd_limits_reset(terminal, session).await,
        Command::Extend(id) => cmd_extend(terminal, session, id).await,
        Command::GrantsList => cmd_grants_list(terminal, session).await,
        Command::GrantsRevoke { covers, agent } => cmd_grants_revoke(terminal, session, covers, agent).await,
        Command::PricesList => cmd_prices_list(terminal, session).await,
        Command::PricesAdd => wizard_prices_add(terminal, session).await,
        Command::PricesEdit(model) => wizard_prices_edit(terminal, session, model).await,
        Command::PricesRemove(model) => cmd_prices_remove(terminal, session, model).await,
        Command::ModelsList => cmd_models_list(terminal, session).await,
        Command::ModelsUse(id) => cmd_models_use(terminal, session, id).await,
        Command::CombosList => cmd_combos_list(terminal, session).await,
        Command::CombosAdd(id, members) => cmd_combos_add(terminal, session, id, members).await,
        Command::CombosRemove(id) => cmd_combos_remove(terminal, session, id).await,
        Command::ModelsReset => cmd_models_reset(terminal, session).await,
        Command::ModelsAdd => wizard_models_add(terminal, session).await,
        Command::ModelsEdit(id) => wizard_models_edit(terminal, session, id).await,
        Command::ModelsRemove(id) => cmd_models_remove(terminal, session, id).await,
        Command::AgentsList => cmd_agents_list(terminal, session).await,
        Command::AgentsUse(id) => cmd_agents_use(terminal, session, id).await,
        Command::AgentsTree => cmd_agents_tree(terminal, session).await,
        Command::AgentsCreate => wizard_agents_create(terminal, session).await,
        Command::AgentsEdit(id) => wizard_agents_edit(terminal, session, id).await,
        Command::AgentsRemove(id) => cmd_agents_remove(terminal, session, id).await,
        Command::SkillsList => cmd_skills_list(terminal, session).await,
        Command::SkillsShow(name) => cmd_skills_show(terminal, session, name).await,
        Command::SkillsCreate => wizard_skills_create(terminal, session).await,
        Command::SkillsEdit(name) => wizard_skills_edit(terminal, session, name).await,
        Command::SkillsRemove(name) => cmd_skills_remove(terminal, session, name).await,
        Command::SkillsPath(name) => cmd_skills_path(terminal, session, name).await,
        Command::SkillsFile(name, file) => cmd_skills_file(terminal, session, name, file).await,
        Command::SkillsAttach { skill, file, source } => cmd_skills_attach(terminal, session, skill, file, source).await,
        Command::SkillsDetach(name, file) => cmd_skills_detach(terminal, session, name, file).await,
        Command::SshList => cmd_ssh_list(terminal, session).await,
        Command::SshAdd => wizard_ssh_add(terminal, session).await,
        Command::SshEdit(id) => wizard_ssh_edit(terminal, session, id).await,
        Command::SshRemove(id) => cmd_ssh_remove(terminal, session, id).await,
        Command::SshEnable(id, enabled) => cmd_ssh_enable(terminal, session, id, enabled).await,
        Command::SshTest(id) => cmd_ssh_test(terminal, session, id).await,
        Command::SshLog(count) => cmd_ssh_log(terminal, count),
        Command::SyncStatus => cmd_sync_status(terminal, session).await,
        Command::SyncPush => cmd_sync_push(terminal, session).await,
        Command::SyncPull => cmd_sync_pull(terminal, session).await,
        Command::SyncPairShow => cmd_sync_pair_show(terminal, session).await,
        Command::SyncPairJoin(code, host) => cmd_sync_pair_join(terminal, session, code, host).await,
        Command::ArcaStatus => cmd_arca_status(terminal, session).await,
        Command::ArcaOn => cmd_arca_on(terminal, session).await,
        Command::ArcaSync => cmd_arca_sync(terminal, session).await,
        Command::ArcaAck => cmd_arca_ack(terminal, session).await,
        Command::ArcaSwapGrant => cmd_arca_swap_grant(terminal, session).await,
        Command::ArcaSwapOffer => cmd_arca_swap_offer(terminal, session).await,
        Command::ArcaRotate => cmd_arca_rotate(terminal, session).await,
        Command::ArcaRotateResume => cmd_arca_rotate_resume(terminal, session).await,
        Command::ArcaPairOffer => cmd_arca_pair_offer(terminal, session).await,
        Command::ArcaPairGrant => cmd_arca_pair_grant(terminal, session).await,
        Command::SyncGitPush => cmd_sync_git_push(terminal, session).await,
        Command::SyncGitPull => cmd_sync_git_pull(terminal, session).await,
    }
}

/// `rebuild` builds the orchestrator again from the same command-line arguments the session started with (P54): a pull that
/// brings a new `config.toml` or an MCP login asks for it, and the next turn already uses the new one.
pub async fn run<F, Fut>(
    mut orchestrator: Orchestrator,
    history_path: Option<&Path>,
    config_path: Option<PathBuf>,
    vault_path_override: Option<String>,
    rebuild: F,
) -> anyhow::Result<()>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Orchestrator>>,
{
    let (mut line_history, _scrub) = match history_path {
        Some(path) => load_history(path),
        None => (Vec::new(), HistoryScrub::Untouched),
    };
    let mut history: Vec<Message> = Vec::new();

    clear_screen()?;
    print_banner();

    let _raw_mode = RawModeGuard::enable()?;
    // One `Terminal`, built once here and reused for both the input box and the "thinking" box
    // for the whole session, instead of rebuilt per turn (see the module doc comment for why a
    // fresh one per call used to hang/error: its construction queries the terminal's cursor
    // position, which contends with reading key events — reused here means that query only ever
    // happens once, before the loop below starts reading any key at all).
    let mut terminal = new_inline_terminal()?;

    let config_path_for_grants = config_path.clone();
    let mut session = CliSession {
        config_path,
        vault_path_override,
        provider_id: None,
        agent_id: None,
        usage_total: Usage::default(),
        turn_count: 0,
        started_at: now_millis(),
        tool_names: Vec::new(),
        spend_guard: None,
        workdir: None,
        grants: cli_grant_book(config_path_for_grants.as_deref()),
        reload_requested: false,
    };
    session.refresh_from(&orchestrator);

    loop {
        let input = match read_line(&mut line_history, &mut terminal).await? {
            LineOutcome::Submitted(text) => text,
            LineOutcome::Exit => break,
        };

        let trimmed = input.trim();
        let outcome = match classify_line(trimmed) {
            LineRoute::Blank => continue,
            LineRoute::Exit => break,
            // The recovery phrase typed as chat would go to the AI provider as the text of a turn. It is neither sent, nor echoed in
            // the "you" card, nor kept (`read_line` already left it out of the line history); the card is a fixed text.
            LineRoute::BlockedPhrase => {
                render_message_card(&mut terminal, &tr!("common.error"), error_style(), vec![(tr!("arca.phrase_in_chat"), Style::default())])?;
                continue;
            }
            LineRoute::Parsed(outcome) => outcome,
        };

        match outcome {
            ParseOutcome::NotACommand => {}
            ParseOutcome::Recognized(Command::Exit) => break,
            ParseOutcome::Recognized(command) => {
                // A wizard/list command's own validation failures (unknown id, blank field, ...)
                // already return `Ok(())` after rendering their own error card — this only
                // catches a lower-level failure (config file unreadable, no config path at all)
                // that bubbled up via `?`, rendering it the same way `run_turn`'s own `Err` arm
                // does below, instead of ending the whole session over a recoverable mistake.
                if let Err(err) = handle_command(command, &mut terminal, &mut session).await {
                    render_message_card(&mut terminal, &tr!("common.error"), error_style(), vec![(format!("{err:#}"), Style::default())])?;
                }
                match reload_if_requested(&mut session, &mut orchestrator, &rebuild).await {
                    None => {}
                    Some(Ok(())) => render_message_card(&mut terminal, "sync", accent_style(), vec![(tr!("reload.done"), Style::default())])?,
                    Some(Err(err)) => render_message_card(
                        &mut terminal,
                        &tr!("common.error"),
                        error_style(),
                        vec![(tr!("reload.failed", error = format!("{err:#}")), Style::default())],
                    )?,
                }
                continue;
            }
            ParseOutcome::Unrecognized(raw) => {
                render_message_card(&mut terminal, &tr!("common.error"), error_style(), vec![(unknown_command_lines(&raw), Style::default())])?;
                continue;
            }
        }

        // Same card structure as the assistant's own reply (`insert_card`) — muted border/title
        // rather than a bright color, matching how understated an echoed prompt reads in a normal
        // chat UI (the color budget is reserved for the assistant's own markdown, not for
        // restating what the user just typed).
        let dim_style = Style::default().add_modifier(Modifier::DIM);
        insert_card(&mut terminal, vec![(tr!("card.you"), dim_style)], dim_style, plain_body_rows(trimmed), None)?;
        terminal.insert_before(1, |_buf| {})?;

        let TurnContext { model_override, system_prompt, orchestrator: scoped } = match resolve_turn_context(&session, &orchestrator) {
            Ok(resolved) => resolved,
            Err(err) => {
                render_message_card(&mut terminal, &tr!("common.error"), error_style(), vec![(format!("{err:#}"), Style::default())])?;
                continue;
            }
        };

        // P102: a session in a folder runs every turn there; a folder that is gone stops the turn instead of quietly
        // falling back to the vault.
        let scoped = match &session.workdir {
            Some(folder) => match warden_bootstrap::scope_to_workdir(&scoped, folder) {
                Ok(in_folder) => in_folder,
                Err(err) => {
                    render_message_card(&mut terminal, &tr!("common.error"), error_style(), vec![(format!("{err:#}"), Style::default())])?;
                    continue;
                }
            },
            None => scoped,
        };

        let grants = session.grants.of(CLI_CONVERSATION, session.agent_id.as_deref());
        match run_turn(&scoped, &history, trimmed, &mut terminal, model_override, system_prompt.as_deref(), grants).await {
            Ok(Some(outcome)) => {
                session.turn_count += 1;
                if let Some(usage) = &outcome.usage {
                    session.usage_total += usage;
                }
                history.push(Message::user(trimmed));
                history.push(Message::assistant(outcome.content));
            }
            Ok(None) => {}
            Err(err) => {
                let error_border = Style::default().fg(Color::Red);
                insert_card(&mut terminal, vec![(tr!("common.error"), error_border)], error_border, plain_body_rows(&format!("{err:#}")), None)?;
                terminal.insert_before(1, |_buf| {})?;
            }
        }
    }

    wait_for_agents_answering(&mut terminal).await?;
    drop(_raw_mode);

    if let Some(path) = history_path {
        save_history(path, &line_history);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a test that asserts on the words pins first: the language is per thread here, never the process's.
    fn speak(locale: i18n::Locale) {
        i18n::set_locale(locale);
    }

    /// A new, empty directory of its own for a history file (never `temp_dir()` itself: the history code sets the permissions of
    /// the directory that holds the file, and that must not be a shared one). The caller removes it.
    fn history_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("warden-cli-history-{name}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// What is in `dir`, by name (sorted): a temporary file left behind shows up here.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// The phrases the history tests plant in an old file: the official 0xff vector (`ZOO_PHRASE`, from arca-core `recovery.rs`, the
    /// same one the tests above use), the all-"abandon" one and the "legal winner" one (the other two official vectors, as in
    /// `a_phrase_typed_as_chat_is_neither_kept_in_the_history_nor_saved_nor_reloaded`).
    fn planted_phrases() -> [String; 3] {
        [
            ZOO_PHRASE.to_string(),
            format!("{} art", ["abandon"; 23].join(" ")),
            "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title".to_string(),
        ]
    }

    #[test]
    fn a_pull_that_brought_a_config_or_a_login_says_the_warden_is_reloading() {
        assert_eq!(pull_reload_note(false), None);
        speak(i18n::Locale::PtBr);
        let note = pull_reload_note(true).expect("a note when something arrived");
        assert!(note.contains("recarregando"), "{note}");
        speak(i18n::Locale::En);
        assert!(pull_reload_note(true).expect("a note when something arrived").contains("reloading"));
    }

    /// An orchestrator whose only tool is called `name`, so a test can tell which one the session holds.
    fn orchestrator_with_tool(name: &'static str) -> Orchestrator {
        struct Named(&'static str);

        #[async_trait::async_trait]
        impl warden_core::tool::Tool for Named {
            fn spec(&self) -> warden_core::tool::ToolSpec {
                warden_core::tool::ToolSpec { name: self.0.to_string(), description: String::new(), parameters: serde_json::json!({}) }
            }
            async fn call(&self, _args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
                Ok(serde_json::Value::Null)
            }
        }

        let dir = std::env::temp_dir().join(format!("warden-cli-reload-{name}-{}", now_millis()));
        Orchestrator::new(Arc::new(NoModel), Arc::new(warden_core::memory::Vault::new(dir.join("vault")))).with_tool(Arc::new(Named(name)))
    }

    fn idle_session(orchestrator: &Orchestrator) -> CliSession {
        let mut session = CliSession {
            config_path: None,
            vault_path_override: None,
            provider_id: None,
            agent_id: None,
            usage_total: Usage::default(),
            turn_count: 0,
            started_at: 0,
            tool_names: Vec::new(),
            spend_guard: None,
            workdir: None,
            grants: GrantBook::default(),
            reload_requested: false,
        };
        session.refresh_from(orchestrator);
        session
    }

    #[tokio::test]
    async fn nothing_is_rebuilt_unless_a_command_asked_for_it() {
        let mut orchestrator = orchestrator_with_tool("old_tool");
        let mut session = idle_session(&orchestrator);
        let rebuilds = std::cell::Cell::new(0);
        let rebuild = || {
            rebuilds.set(rebuilds.get() + 1);
            async { Ok(orchestrator_with_tool("new_tool")) }
        };

        assert!(reload_if_requested(&mut session, &mut orchestrator, &rebuild).await.is_none());
        assert_eq!(rebuilds.get(), 0);
        assert!(session.tool_names.contains(&"old_tool".to_string()));
    }

    #[tokio::test]
    async fn a_requested_reload_swaps_the_orchestrator_and_what_the_session_read_from_it() {
        let mut orchestrator = orchestrator_with_tool("old_tool");
        let mut session = idle_session(&orchestrator);
        session.reload_requested = true;
        let rebuilds = std::cell::Cell::new(0);
        let rebuild = || {
            rebuilds.set(rebuilds.get() + 1);
            async { Ok(orchestrator_with_tool("new_tool")) }
        };

        assert!(reload_if_requested(&mut session, &mut orchestrator, &rebuild).await.expect("asked for").is_ok());
        assert_eq!(rebuilds.get(), 1);
        assert!(!session.reload_requested, "one request, one rebuild");
        assert!(session.tool_names.contains(&"new_tool".to_string()) && !session.tool_names.contains(&"old_tool".to_string()), "{:?}", session.tool_names);
        assert!(orchestrator.tools().iter().any(|t| t.spec().name == "new_tool"));
        assert!(reload_if_requested(&mut session, &mut orchestrator, &rebuild).await.is_none());
        assert_eq!(rebuilds.get(), 1, "asking is not sticky");
    }

    #[tokio::test]
    async fn saving_a_config_change_asks_for_a_reload() {
        let dir = std::env::temp_dir().join(format!("warden-cli-save-reload-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let orchestrator = orchestrator_with_tool("old_tool");
        let mut session = idle_session(&orchestrator);
        session.config_path = Some(dir.join("config.toml"));

        save_config_and_reload(&mut session, &FileConfig::default()).await.expect("the config is written");

        assert!(session.reload_requested);
        assert!(dir.join("config.toml").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_save_that_fails_asks_for_no_reload() {
        let orchestrator = orchestrator_with_tool("old_tool");
        let mut session = idle_session(&orchestrator);

        assert!(save_config_and_reload(&mut session, &FileConfig::default()).await.is_err(), "no config path to write to");
        assert!(!session.reload_requested);
    }

    #[test]
    fn an_offer_is_picked_by_its_number_and_nothing_else_is_a_pick() {
        for (typed, expected) in [("1", Some(0)), (" 2 ", Some(1)), ("3", Some(2)), ("0", None), ("4", None), ("-1", None), ("abc", None), ("", None), ("+1", None), ("1 2", None)] {
            assert_eq!(parse_pick(typed, 3), expected, "{typed:?}");
        }
    }

    #[tokio::test]
    async fn a_failed_reload_keeps_the_orchestrator_the_session_had() {
        let mut orchestrator = orchestrator_with_tool("old_tool");
        let mut session = idle_session(&orchestrator);
        session.reload_requested = true;
        let rebuild = || async { Err::<Orchestrator, _>(anyhow::anyhow!("config.toml is broken")) };

        let err = reload_if_requested(&mut session, &mut orchestrator, &rebuild).await.expect("asked for").unwrap_err();
        assert!(err.to_string().contains("config.toml is broken"), "{err}");
        assert!(!session.reload_requested, "the failure is reported once, not on every command");
        assert!(session.tool_names.contains(&"old_tool".to_string()));
        assert!(orchestrator.tools().iter().any(|t| t.spec().name == "old_tool"));
    }

    #[test]
    fn the_approval_categories_answer_is_a_list_of_known_ids() {
        use warden_core::autonomy::Category;
        assert_eq!(parse_approval_categories(""), Ok(vec![]));
        assert_eq!(parse_approval_categories(" critical_infra , delete_data,critical_infra "), Ok(vec![Category::CriticalInfra, Category::DeleteData]));
        let refused = parse_approval_categories("critical_infra, tudo").unwrap_err();
        assert!(refused.contains("elevated_agent"), "{refused}");
    }

    #[test]
    fn a_key_in_the_approval_card_says_once_always_or_no_and_a_stray_one_says_nothing() {
        for yes in ['s', 'S', 'y', 'Y'] {
            assert_eq!(approval_key(KeyCode::Char(yes), true), Some(Answer::Once));
        }
        for no in [KeyCode::Char('n'), KeyCode::Char('N'), KeyCode::Esc, KeyCode::Enter] {
            assert_eq!(approval_key(no, true), Some(Answer::Reject));
        }
        assert_eq!(approval_key(KeyCode::Char('a'), true), Some(Answer::Always));
        assert_eq!(approval_key(KeyCode::Char('A'), true), Some(Answer::Always));
        assert_eq!(approval_key(KeyCode::Char('a'), false), None, "no \"always\" was offered, so the key does nothing");
        assert_eq!(approval_key(KeyCode::Char('x'), true), None);
        assert_eq!(approval_key(KeyCode::Tab, true), None);
    }

    #[tokio::test]
    async fn the_channel_approver_tells_the_person_what_an_always_would_cover_and_hands_back_the_answer() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AskedApproval>();
        let approver = ChannelApprover { requests: tx };
        let asking = tokio::spawn(async move { approver.ask(ApprovalRequest::new("shell", "tool_call", "ls"), Some("shell")).await });
        let (request, always, reply) = rx.recv().await.unwrap();
        assert_eq!((request.target.as_str(), always.as_deref()), ("shell", Some("shell")));
        reply.send(Answer::Always).unwrap();
        assert_eq!(asking.await.unwrap(), Answer::Always);

        let (tx, rx) = mpsc::unbounded_channel::<AskedApproval>();
        drop(rx);
        let gone = ChannelApprover { requests: tx };
        assert_eq!(gone.ask(ApprovalRequest::new("shell", "tool_call", ""), None).await, Answer::Reject, "nobody to ask is a no");
        assert!(!gone.approve(ApprovalRequest::new("shell", "tool_call", "")).await);
    }

    #[tokio::test]
    async fn an_always_said_in_the_terminal_is_kept_in_a_file_beside_the_config_and_asked_no_more() {
        let dir = std::env::temp_dir().join(format!("warden-cli-grants-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        std::fs::write(&config, "").unwrap();

        let ask = |book: GrantBook, answer: Answer| async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<AskedApproval>();
            let approver = RememberingApprover::new(Arc::new(ChannelApprover { requests: tx }), book.of(CLI_CONVERSATION, Some("ana")));
            let asking = tokio::spawn(async move { approver.ask(ApprovalRequest::new("shell", "tool_call", "ls"), Some("shell")).await });
            // Answers when asked; when the grant already answers, nothing arrives and `ask` finishes first.
            let asked = tokio::select! {
                got = rx.recv() => got.map(|(_, _, reply)| reply.send(answer).unwrap()).is_some(),
                _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => false,
            };
            (asking.await.unwrap(), asked)
        };

        assert_eq!(ask(cli_grant_book(Some(&config)), Answer::Always).await, (Answer::Always, true));
        assert!(dir.join("grants").join("cli.json").exists());
        // A new session (a new book from the same file) is not asked again, and another agent still is.
        assert_eq!(ask(cli_grant_book(Some(&config)), Answer::Reject).await, (Answer::Always, false));
        let other = cli_grant_book(Some(&config)).of(CLI_CONVERSATION, Some("bia"));
        assert!(!other.allows("shell"));

        // `persist_grants = false` keeps it in memory: nothing is written and the next session asks again.
        let off = dir.join("off.toml");
        std::fs::write(&off, "persist_grants = false\n").unwrap();
        assert_eq!(ask(cli_grant_book(Some(&off)), Answer::Always).await, (Answer::Always, true));
        assert!(!dir.join("grants").join("off.json").exists());
        assert_eq!(ask(cli_grant_book(Some(&off)), Answer::Reject).await, (Answer::Reject, true));
        assert_eq!(cli_grant_book(None).list().len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn how_long_ago_is_told_in_minutes_hours_and_days() {
        speak(i18n::Locale::PtBr);
        const MIN: u64 = 60_000;
        let now = 100 * 24 * 60 * MIN;
        assert_eq!(ago_label(now, now), "agora");
        assert_eq!(ago_label(now - 59_000, now), "agora");
        assert_eq!(ago_label(now - MIN, now), "há 1 min");
        assert_eq!(ago_label(now - 59 * MIN, now), "há 59 min");
        assert_eq!(ago_label(now - 60 * MIN, now), "há 1 h");
        assert_eq!(ago_label(now - 23 * 60 * MIN, now), "há 23 h");
        assert_eq!(ago_label(now - 24 * 60 * MIN, now), "há 1 d");
        assert_eq!(ago_label(now - 3 * 24 * 60 * MIN, now), "há 3 d");
        assert_eq!(ago_label(now + MIN, now), "agora", "a clock that went back never gives a negative");
        speak(i18n::Locale::En);
        assert_eq!(ago_label(now, now), "now");
        assert_eq!(ago_label(now - 3 * 24 * 60 * MIN, now), "3 d ago");
    }

    fn grant(agent: Option<&str>, covers: &str, last_used_ms: u64) -> Grant {
        Grant { conversation: CLI_CONVERSATION.to_string(), agent: agent.map(str::to_string), covers: covers.to_string(), last_used_ms }
    }

    #[test]
    fn the_grants_card_lists_each_always_and_says_how_long_it_lasts() {
        speak(i18n::Locale::PtBr);
        let text = |lines: Vec<(String, Style)>| lines.into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert!(grant_lines(&[], Some(30), 0).is_none(), "nothing given is an empty card");
        let elsewhere = Grant { conversation: "c1".to_string(), ..grant(None, "shell", 0) };
        assert!(grant_lines(&[elsewhere], Some(30), 0).is_none(), "another conversation's grant is not the terminal's");

        let now = 5 * 24 * 60 * 60_000;
        let lines = text(grant_lines(&[grant(None, "shell", now - 3 * 24 * 60 * 60_000), grant(Some("ana"), "delete_data", now)], Some(30), now).unwrap());
        assert_eq!(lines[0], "shell · agente padrão · usado há 3 d");
        assert_eq!(lines[1], "delete_data · ana · usado agora");
        assert_eq!(lines[2], "vencem depois de 30 dias sem uso (cada uso renova)");
        assert!(lines[3].starts_with("/grants revoke"));

        let in_memory = text(grant_lines(&[grant(None, "shell", now)], None, now).unwrap());
        assert_eq!(in_memory[1], "acabam quando o terminal fecha");
    }

    #[tokio::test]
    async fn revoking_a_grant_takes_it_from_that_agent_only_and_the_file_follows() {
        let dir = std::env::temp_dir().join(format!("warden-cli-revoke-{}", now_millis()));
        let file = dir.join("cli.json");
        let book = GrantBook::persistent(file.clone(), warden_core::grants::DEFAULT_TTL);
        for agent in [None, Some("ana"), Some("bia")] {
            let (tx, mut rx) = mpsc::unbounded_channel::<AskedApproval>();
            let approver = RememberingApprover::new(Arc::new(ChannelApprover { requests: tx }), book.of(CLI_CONVERSATION, agent));
            let asking = tokio::spawn(async move { approver.ask(ApprovalRequest::new("shell", "tool_call", ""), Some("shell")).await });
            let (_, _, reply) = rx.recv().await.unwrap();
            reply.send(Answer::Always).unwrap();
            assert_eq!(asking.await.unwrap(), Answer::Always);
        }
        assert_eq!(book.list().len(), 3);

        assert!(book.revoke(CLI_CONVERSATION, Some("ana"), "shell"));
        assert!(!book.revoke(CLI_CONVERSATION, Some("ana"), "shell"), "a second time there is nothing to take");
        let left: Vec<Option<String>> = book.list().into_iter().map(|g| g.agent).collect();
        assert_eq!(left, vec![None, Some("bia".to_string())]);
        assert!(std::fs::read_to_string(&file).unwrap().contains("bia") && !std::fs::read_to_string(&file).unwrap().contains("ana"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_autonomy_answer_is_a_level_from_one_to_five() {
        assert_eq!(parse_agent_autonomy(" 3 "), Ok(3));
        assert_eq!(parse_agent_autonomy("5"), Ok(5));
        for bad in ["", "0", "6", "-1", "três", "2.5"] {
            assert!(parse_agent_autonomy(bad).is_err(), "{bad:?}");
        }
    }

    /// These tests never run a model turn.
    struct NoModel;

    #[async_trait::async_trait]
    impl warden_core::model::ModelProvider for NoModel {
        async fn chat_stream(&self, _messages: Vec<Message>, _tools: Vec<warden_core::tool::ToolSpec>) -> anyhow::Result<warden_core::model::ChatStream> {
            anyhow::bail!("no model in these tests")
        }
    }

    #[test]
    fn an_agent_allowed_to_leave_notes_gets_message_agent_in_the_terminal() {
        let dir = std::env::temp_dir().join(format!("warden-cli-notes-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, "[[agents]]\nid = \"ana\"\npersona = \"p\"\ncan_message_agents = true\n\n[[agents]]\nid = \"bia\"\npersona = \"p\"\n").unwrap();
        let orchestrator = Orchestrator::new(std::sync::Arc::new(NoModel), std::sync::Arc::new(warden_core::memory::Vault::new(dir.join("vault"))));
        let session = |agent: &str| CliSession {
            config_path: Some(config_path.clone()),
            vault_path_override: None,
            provider_id: None,
            agent_id: Some(agent.to_string()),
            usage_total: Usage::default(),
            turn_count: 0,
            started_at: 0,
            tool_names: Vec::new(),
            spend_guard: None,
            workdir: None,
            grants: GrantBook::default(),
            reload_requested: false,
        };
        let tools =|agent: &str| -> Vec<String> {
            resolve_turn_context(&session(agent), &orchestrator).unwrap().orchestrator.tools().iter().map(|t| t.spec().name).collect()
        };
        assert!(tools("ana").contains(&"message_agent".to_string()));
        assert!(!tools("bia").contains(&"message_agent".to_string()), "only with the flag");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_cost_line_never_passes_an_unpriced_call_off_as_free() {
        speak(i18n::Locale::En);
        let in_english = cost_line(Some(&Spent { calls: 3, unpriced_calls: 3, cost_usd: 0.0 }));
        assert!(in_english.contains("unavailable") && in_english.contains("3 model calls"), "{in_english}");
        speak(i18n::Locale::PtBr);
        let spent = |calls, unpriced_calls, cost_usd| Spent { calls, unpriced_calls, cost_usd };
        assert!(cost_line(None).contains("indisponível") && cost_line(None).contains("limites"));
        assert_eq!(cost_line(Some(&spent(0, 0, 0.0))), "$ 0.0000");
        assert_eq!(cost_line(Some(&spent(4, 0, 0.12345))), "$0.1235 (pelos preços do config.toml)");
        let none_priced = cost_line(Some(&spent(3, 3, 0.0)));
        assert!(none_priced.contains("indisponível") && none_priced.contains("3 chamadas") && none_priced.contains("[[prices]]"));
        let some_priced = cost_line(Some(&spent(5, 2, 1.5)));
        assert!(some_priced.starts_with("$1.5000 ou mais") && some_priced.contains("2 chamadas"), "{some_priced}");
    }

    #[test]
    fn limit_window_hours_needs_a_positive_integer() {
        assert_eq!(parse_limit_window_hours("24"), Ok(24));
        assert_eq!(parse_limit_window_hours(" 1 "), Ok(1));
        assert!(parse_limit_window_hours("0").is_err());
        assert!(parse_limit_window_hours("-1").is_err());
        assert!(parse_limit_window_hours("").is_err());
        assert!(parse_limit_window_hours("1.5").is_err());
    }

    #[test]
    fn optional_u64_is_none_when_blank_and_rejects_zero_or_negative() {
        assert_eq!(parse_optional_u64(""), Ok(None));
        assert_eq!(parse_optional_u64("   "), Ok(None));
        assert_eq!(parse_optional_u64("500000"), Ok(Some(500_000)));
        assert!(parse_optional_u64("0").is_err());
        assert!(parse_optional_u64("-5").is_err());
        assert!(parse_optional_u64("abc").is_err());
    }

    #[test]
    fn optional_cost_is_none_when_blank_and_rejects_zero_or_negative() {
        assert_eq!(parse_optional_cost(""), Ok(None));
        assert_eq!(parse_optional_cost("5.5"), Ok(Some(5.5)));
        assert!(parse_optional_cost("0").is_err());
        assert!(parse_optional_cost("-1").is_err());
        assert!(parse_optional_cost("nan").is_err());
    }

    #[test]
    fn price_amount_allows_zero_but_not_negative() {
        assert_eq!(parse_price_amount("0"), Ok(0.0));
        assert_eq!(parse_price_amount("0.15"), Ok(0.15));
        assert!(parse_price_amount("-0.01").is_err());
        assert!(parse_price_amount("nan").is_err());
    }

    #[test]
    fn optional_percent_is_none_when_blank_and_converts_1_to_100_into_a_fraction() {
        assert_eq!(parse_optional_percent(""), Ok(None));
        assert_eq!(parse_optional_percent("80"), Ok(Some(0.8)));
        assert_eq!(parse_optional_percent("100"), Ok(Some(1.0)));
        assert!(parse_optional_percent("0").is_err());
        assert!(parse_optional_percent("101").is_err());
    }

    #[test]
    fn limit_scope_parses_the_four_fixed_words_case_insensitively() {
        assert_eq!(parse_limit_scope("Global"), Ok(LimitScope::Global));
        assert_eq!(parse_limit_scope("agent"), Ok(LimitScope::Agent));
        assert_eq!(parse_limit_scope("CHANNEL"), Ok(LimitScope::Channel));
        assert_eq!(parse_limit_scope("user"), Ok(LimitScope::User));
        assert!(parse_limit_scope("bogus").is_err());
    }

    #[test]
    fn limit_target_validation_matches_each_scope_shape() {
        speak(i18n::Locale::PtBr);
        let agents = vec!["pirata".to_string()];
        assert_eq!(validate_limit_target(LimitScope::Global, "anything", &agents), Ok(None));
        assert_eq!(validate_limit_target(LimitScope::Agent, "pirata", &agents), Ok(Some("pirata".to_string())));
        assert!(validate_limit_target(LimitScope::Agent, "unknown", &agents).unwrap_err().contains("não existe"));
        assert!(validate_limit_target(LimitScope::Agent, "", &agents).is_err());
        assert_eq!(validate_limit_target(LimitScope::Channel, "telegram", &agents), Ok(Some("telegram".to_string())));
        assert!(validate_limit_target(LimitScope::Channel, "  ", &agents).is_err());
        assert_eq!(validate_limit_target(LimitScope::User, "telegram:12345", &agents), Ok(Some("telegram:12345".to_string())));
        assert!(validate_limit_target(LimitScope::User, "12345", &agents).is_err());
    }

    #[test]
    fn agent_tools_input_is_blank_for_all_or_a_checked_deduplicated_list() {
        speak(i18n::Locale::PtBr);
        let known: Vec<String> = ["read_file", "shell"].iter().map(|s| s.to_string()).collect();
        assert_eq!(parse_agent_tools("   ", &known), Ok(None));
        assert_eq!(parse_agent_tools(" shell , read_file,shell,", &known), Ok(Some(vec!["shell".to_string(), "read_file".to_string()])));
        assert!(parse_agent_tools("read_file, teleport", &known).unwrap_err().contains("'teleport' não existe"));
        assert!(parse_agent_tools("manage_agents", &known).unwrap_err().contains("não entra na lista"));
        assert!(parse_agent_tools("delegate_to_agent", &known).is_err());
    }

    #[test]
    fn delegation_models_input_is_blank_for_open_or_a_checked_deduplicated_list() {
        speak(i18n::Locale::PtBr);
        let known: Vec<String> = ["fast", "smart", "cheap"].iter().map(|s| s.to_string()).collect();
        assert_eq!(parse_delegation_models("  ", &known), Ok(Vec::new()));
        assert_eq!(parse_delegation_models(" smart , fast,smart,", &known), Ok(vec!["smart".to_string(), "fast".to_string()]));
        assert!(parse_delegation_models("fast, nope", &known).unwrap_err().contains("'nope' não existe"));
        assert!(parse_delegation_models("fast", &[]).unwrap_err().contains("nenhum cadastrado"));
    }

    #[test]
    fn insert_and_backspace_move_the_cursor_correctly() {
        let mut editor = LineEditor::new(Vec::new());
        editor.insert('o');
        editor.insert('i');
        assert_eq!(editor.as_str(), "oi");
        assert_eq!(editor.cursor, 2);

        editor.backspace();
        assert_eq!(editor.as_str(), "o");
        assert_eq!(editor.cursor, 1);
    }

    #[test]
    fn cursor_movement_is_utf8_char_aware_not_byte_aware() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "héllo".chars() {
            editor.insert(c);
        }
        assert_eq!(editor.cursor, 5);

        editor.move_left();
        editor.move_left();
        editor.move_left();
        assert_eq!(editor.cursor, 2);
        editor.insert('X');
        assert_eq!(editor.as_str(), "héXllo");
    }

    #[test]
    fn insert_in_the_middle_respects_the_cursor_position() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "Wrden".chars() {
            editor.insert(c);
        }
        editor.move_home();
        editor.move_right();
        editor.insert('a');
        assert_eq!(editor.as_str(), "Warden");
    }

    #[test]
    fn delete_forward_removes_the_character_after_the_cursor() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "Wardxen".chars() {
            editor.insert(c);
        }
        editor.move_left();
        editor.move_left();
        editor.move_left();
        editor.delete_forward();
        assert_eq!(editor.as_str(), "Warden");
    }

    #[test]
    fn home_and_end_move_to_the_boundaries() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "hi".chars() {
            editor.insert(c);
        }
        editor.move_home();
        assert_eq!(editor.cursor, 0);
        editor.move_end();
        assert_eq!(editor.cursor, 2);
    }

    #[test]
    fn delete_word_backward_removes_the_last_word_and_trailing_space() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "hello there  ".chars() {
            editor.insert(c);
        }
        editor.delete_word_backward();
        assert_eq!(editor.as_str(), "hello ");
    }

    #[test]
    fn clear_line_empties_the_buffer_and_resets_the_cursor() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "oops".chars() {
            editor.insert(c);
        }
        editor.clear_line();
        assert!(editor.is_empty());
        assert_eq!(editor.cursor, 0);
    }

    #[test]
    fn history_up_recalls_the_most_recent_entry_first() {
        let mut editor = LineEditor::new(vec!["first".to_string(), "second".to_string()]);
        editor.history_up();
        assert_eq!(editor.as_str(), "second");
        editor.history_up();
        assert_eq!(editor.as_str(), "first");
        // Already at the oldest entry — another Up does nothing.
        editor.history_up();
        assert_eq!(editor.as_str(), "first");
    }

    #[test]
    fn history_down_past_the_newest_entry_restores_the_in_progress_draft() {
        let mut editor = LineEditor::new(vec!["first".to_string(), "second".to_string()]);
        for c in "unsent draft".chars() {
            editor.insert(c);
        }

        editor.history_up();
        assert_eq!(editor.as_str(), "second");
        editor.history_down();
        assert_eq!(editor.as_str(), "unsent draft");
    }

    #[test]
    fn submit_returns_the_text_and_resets_the_editor() {
        let mut editor = LineEditor::new(Vec::new());
        for c in "hi".chars() {
            editor.insert(c);
        }
        let text = editor.submit();
        assert_eq!(text, "hi");
        assert!(editor.is_empty());
        assert_eq!(editor.cursor, 0);
    }

    fn plain_text(spans: &[(String, Style)]) -> String {
        spans.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn parse_inline_extracts_bold_italic_and_code_spans() {
        let spans = parse_inline("normal **bold** and *italic* and `code` done", Style::default());
        assert_eq!(plain_text(&spans), "normal bold and italic and code done");
        assert!(spans.iter().any(|(t, s)| t == "bold" && s.add_modifier == Modifier::BOLD));
        assert!(spans.iter().any(|(t, s)| t == "italic" && s.add_modifier == Modifier::ITALIC));
        assert!(spans.iter().any(|(t, _)| t == "code"));
    }

    #[test]
    fn parse_inline_leaves_an_unclosed_marker_as_literal_text() {
        let spans = parse_inline("this **never closes", Style::default());
        assert_eq!(plain_text(&spans), "this **never closes");
    }

    #[test]
    fn classify_and_strip_recognizes_headers_and_bullets() {
        let mut in_code = false;
        assert!(matches!(classify_and_strip("# Title", &mut in_code), Some((LineKind::Header, ref rest)) if rest == "Title"));
        assert!(matches!(classify_and_strip("- item", &mut in_code), Some((LineKind::Bullet, ref rest)) if rest == "item"));
        assert!(matches!(classify_and_strip("plain text", &mut in_code), Some((LineKind::Plain, ref rest)) if rest == "plain text"));
    }

    #[test]
    fn classify_and_strip_toggles_code_block_state_across_calls() {
        let mut in_code = false;
        assert!(classify_and_strip("```rust", &mut in_code).is_none());
        assert!(in_code);
        assert!(matches!(classify_and_strip("fn main() {}", &mut in_code), Some((LineKind::Code, _))));
        assert!(classify_and_strip("```", &mut in_code).is_none());
        assert!(!in_code);
    }

    #[test]
    fn wrap_spans_breaks_on_word_boundaries_and_preserves_style() {
        let spans = vec![("hello world foo".to_string(), Style::default())];
        let rows = wrap_spans(&spans, 8);
        let row_texts: Vec<String> = rows.iter().map(|row| row.iter().map(|(t, _)| t.as_str()).collect()).collect();
        assert_eq!(row_texts, vec!["hello ", "world ", "foo"]);
    }

    #[test]
    fn wrap_spans_hard_breaks_a_single_word_longer_than_the_width() {
        let spans = vec![("supercalifragilistic".to_string(), Style::default())];
        let rows = wrap_spans(&spans, 5);
        assert!(rows.iter().all(|row| row.iter().map(|(t, _)| UnicodeWidthStr::width(t.as_str())).sum::<usize>() <= 5));
        let rejoined: String = rows.iter().flatten().map(|(t, _)| t.as_str()).collect();
        assert_eq!(rejoined, "supercalifragilistic");
    }

    #[test]
    fn card_width_shrinks_to_fit_a_short_message_instead_of_the_full_terminal() {
        let body = vec![vec![("ok".to_string(), Style::default())]];
        assert_eq!(card_width(6, &body, 80), 12);
    }

    #[test]
    fn card_width_grows_up_to_its_widest_row_but_never_past_max_width() {
        let short_body = vec![vec![("hello there".to_string(), Style::default())]];
        assert_eq!(card_width(6, &short_body, 80), 15);

        let long_row = "x".repeat(200);
        let long_body = vec![vec![(long_row, Style::default())]];
        assert_eq!(card_width(6, &long_body, 80), 80);
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn a_skill_bound_to_known_agents_has_no_unknown_ids() {
        assert!(unknown_agent_ids(&ids(&["research", "writer"]), &ids(&["research", "writer", "ops"])).is_empty());
    }

    #[test]
    fn a_skill_with_no_agents_is_visible_to_all_so_nothing_is_unknown() {
        assert!(unknown_agent_ids(&[], &ids(&["research"])).is_empty());
        assert!(unknown_agent_ids(&[], &[]).is_empty());
    }

    #[test]
    fn a_mistyped_agent_id_is_reported_and_the_right_one_is_not() {
        assert_eq!(unknown_agent_ids(&ids(&["reserch", "writer"]), &ids(&["research", "writer"])), ids(&["reserch"]));
    }

    #[test]
    fn every_unknown_id_is_reported_in_the_order_typed() {
        assert_eq!(unknown_agent_ids(&ids(&["b", "research", "a"]), &ids(&["research"])), ids(&["b", "a"]));
    }

    #[test]
    fn an_unknown_id_repeated_is_reported_once() {
        assert_eq!(unknown_agent_ids(&ids(&["ghost", "ghost"]), &ids(&["research"])), ids(&["ghost"]));
    }

    #[test]
    fn with_no_agents_registered_any_id_is_unknown() {
        assert_eq!(unknown_agent_ids(&ids(&["research"]), &[]), ids(&["research"]));
    }

    #[test]
    fn agent_ids_match_exactly_without_case_folding() {
        assert_eq!(unknown_agent_ids(&ids(&["Research"]), &ids(&["research"])), ids(&["Research"]));
    }

    // ---- `/arca` (status) and `/arca sync` ----

    use warden_sync::arca_sync::SkippedLogin;

    fn mode(secret_accepted: bool) -> ArcaMode {
        ArcaMode { version: 1, enabled_at_ms: 1, hub_id: None, secret_accepted }
    }

    #[test]
    fn the_status_of_a_vault_in_arca_mode_says_it_is_on_and_whether_the_logins_travel() {
        speak(i18n::Locale::En);
        let accepted = arca_status_lines(true, Some(&mode(true))).join("\n");
        assert!(accepted.contains("arca mode: on"), "{accepted}");
        assert!(accepted.contains("/arca sync"), "{accepted}");
        assert!(accepted.contains("MCP logins: synced"), "{accepted}");
        let not_accepted = arca_status_lines(true, Some(&mode(false))).join("\n");
        assert!(not_accepted.contains("MCP logins: not synced"), "{not_accepted}");
    }

    #[test]
    fn the_status_of_a_vault_that_is_not_in_arca_mode_says_how_to_get_there() {
        speak(i18n::Locale::En);
        let with_key = arca_status_lines(true, None).join("\n");
        assert!(with_key.contains("arca mode: off"), "{with_key}");
        assert!(with_key.contains("/arca on"), "{with_key}");
        let without_key = arca_status_lines(false, None).join("\n");
        assert!(without_key.contains("no vault key"), "{without_key}");
        assert!(without_key.contains("/arca pair offer"), "{without_key}");
        assert!(without_key.contains("/arca on"), "{without_key}");
    }

    // ---------- the newer key epochs that stop the vault (`/arca` and `/arca ack`) ----------

    /// A pending epoch whose id is the hex of `tag` (up to 32 bytes) padded with zeros: `manifest/<64 hex>`, the way the hub names
    /// its objects. Different tags in their first 6 bytes give different short ids.
    fn epoch(number: u32, tag: &str) -> PendingEpoch {
        let hex: String = tag.bytes().map(|byte| format!("{byte:02x}")).collect();
        PendingEpoch { epoch: number, id: format!("manifest/{hex:0<64}").parse().unwrap() }
    }

    /// An engine with a fixed list of pending epochs that remembers what it was told to acknowledge (`None` until it was asked).
    struct FakeEngine {
        fresh: Result<Vec<PendingEpoch>, String>,
        acknowledged: std::sync::Mutex<Option<Vec<PendingEpoch>>>,
    }

    impl FakeEngine {
        fn with(fresh: Vec<PendingEpoch>) -> Self {
            Self { fresh: Ok(fresh), acknowledged: std::sync::Mutex::new(None) }
        }
        fn told(&self) -> Option<Vec<PendingEpoch>> {
            self.acknowledged.lock().unwrap().clone()
        }
    }

    impl EpochSource for FakeEngine {
        async fn pending(&self) -> Result<Vec<PendingEpoch>, String> {
            self.fresh.clone()
        }
        async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String> {
            *self.acknowledged.lock().unwrap() = Some(seen.to_vec());
            Ok(())
        }
    }

    #[test]
    fn with_no_newer_epoch_the_status_adds_nothing() {
        assert!(arca_epoch_lines(&[]).is_empty());
    }

    #[test]
    fn a_pending_epoch_is_listed_with_its_number_and_short_id_and_the_way_out() {
        speak(i18n::Locale::En);
        let lines = arca_epoch_lines(&[epoch(2, "abc123"), epoch(3, "def456")]).join("\n");
        assert!(lines.contains("newer key"), "{lines}");
        assert!(lines.contains("epoch 2") && lines.contains("616263313233"), "{lines}");
        assert!(lines.contains("epoch 3"), "{lines}");
        // `/arca pair offer` refuses a device that already has a key, so the way out the notice points to is the swap.
        assert!(lines.contains("/arca swap offer") && lines.contains("/arca ack"), "{lines}");
        assert!(!lines.contains("/arca pair offer"), "{lines}");
        // The epoch in an object's header is not authenticated: the notice must not read as a fact.
        assert!(lines.contains("forge"), "{lines}");
        speak(i18n::Locale::PtBr);
        let pt = arca_epoch_lines(&[epoch(2, "abc123")]).join("\n");
        assert!(pt.contains("chave mais nova") && pt.contains("época 2") && pt.contains("/arca ack"), "{pt}");
    }

    #[test]
    fn a_list_that_could_not_be_read_is_said_and_never_shown_as_nothing_pending() {
        speak(i18n::Locale::En);
        let en = arca_epoch_check_lines(Err("lock busy".into())).join("\n");
        assert!(en.contains("lock busy") && en.contains("NOT mean there are none"), "{en}");
        speak(i18n::Locale::PtBr);
        let pt = arca_epoch_check_lines(Err("lock busy".into())).join("\n");
        assert!(pt.contains("lock busy") && pt.contains("NÃO quer dizer"), "{pt}");
        assert!(arca_epoch_check_lines(Ok(vec![])).is_empty());
        assert!(!arca_epoch_check_lines(Ok(vec![epoch(2, "abc123")])).is_empty());
    }

    #[test]
    fn only_the_pairs_seen_and_still_pending_are_kept_in_the_order_of_the_fresh_list() {
        let (a, b, c) = (epoch(1, "a"), epoch(1, "b"), epoch(2, "c"));
        assert_eq!(epochs_seen_still_pending(&[a, b, c], &[b]), vec![b], "the one that was shown");
        assert_eq!(epochs_seen_still_pending(&[a, b, c], &[c, a]), vec![a, c], "in the order of the fresh list");
        assert_eq!(epochs_seen_still_pending(&[a, b, c], &[a, a]), vec![a], "each once");
        assert_eq!(epochs_seen_still_pending(&[a, b, c], &[]), vec![], "nothing seen, nothing acknowledged");
        assert_eq!(epochs_seen_still_pending(&[a], &[b]), vec![], "gone since the person looked");
        assert_eq!(epochs_seen_still_pending(&[], &[a]), vec![]);
    }

    #[test]
    fn the_epoch_alone_is_not_enough_a_forged_object_of_the_same_epoch_is_another_pair() {
        let (real, forged) = (epoch(1, "the real rotation"), epoch(1, "forged with the old key"));
        assert_eq!(epochs_seen_still_pending(&[real, forged], &[real]), vec![real]);
        assert_eq!(epochs_seen_still_pending(&[forged], &[real]), vec![]);
    }

    #[tokio::test]
    async fn without_a_typed_yes_the_engine_is_not_asked_to_acknowledge_anything() {
        for answer in [None, Some(""), Some("n"), Some("nao"), Some("talvez")] {
            let engine = FakeEngine::with(vec![epoch(1, "a")]);
            assert_eq!(arca_ack_run(&engine, &[epoch(1, "a")], answer).await, ArcaAck::Cancelled, "{answer:?}");
            assert_eq!(engine.told(), None, "{answer:?}");
        }
    }

    #[tokio::test]
    async fn only_the_pair_the_person_saw_reaches_the_engine_not_the_forged_one_of_the_same_epoch() {
        let (real, forged, later) = (epoch(1, "the real rotation"), epoch(1, "forged with the old key"), epoch(2, "a later one"));
        let engine = FakeEngine::with(vec![real, forged, later]);
        assert_eq!(arca_ack_run(&engine, &[real], Some("y")).await, ArcaAck::Done { count: 1 });
        assert_eq!(engine.told(), Some(vec![real]), "the forged object and the later epoch stay pending");
    }

    #[tokio::test]
    async fn what_appeared_after_the_person_looked_is_not_acknowledged() {
        let (shown, appeared_later) = (epoch(1, "what the screen listed"), epoch(1, "showed up after"));
        let engine = FakeEngine::with(vec![shown, appeared_later]);
        assert_eq!(arca_ack_run(&engine, &[shown], Some("sim")).await, ArcaAck::Done { count: 1 });
        assert_eq!(engine.told(), Some(vec![shown]));
    }

    #[tokio::test]
    async fn when_everything_seen_is_gone_nothing_is_acknowledged_and_the_person_is_told() {
        let engine = FakeEngine::with(vec![epoch(2, "new")]);
        assert_eq!(arca_ack_run(&engine, &[epoch(1, "old")], Some("y")).await, ArcaAck::NothingPending);
        assert_eq!(engine.told(), None);
    }

    #[tokio::test]
    async fn a_failure_to_read_the_list_is_reported_and_acknowledges_nothing() {
        let engine = FakeEngine { fresh: Err("disk gone".into()), acknowledged: std::sync::Mutex::new(None) };
        assert_eq!(arca_ack_run(&engine, &[epoch(1, "a")], Some("y")).await, ArcaAck::Failed("disk gone".into()));
        assert_eq!(engine.told(), None);
    }

    #[test]
    fn the_outcome_of_the_acknowledgement_is_said_in_words() {
        speak(i18n::Locale::En);
        assert!(arca_ack_lines(&ArcaAck::Done { count: 2 }).join("\n").contains("acknowledged"));
        assert!(arca_ack_lines(&ArcaAck::NothingPending).join("\n").contains("nothing"));
        assert!(arca_ack_lines(&ArcaAck::Cancelled).join("\n").contains("cancelled"));
        assert_eq!(arca_ack_lines(&ArcaAck::Failed("boom".into())), vec!["boom".to_string()]);
    }

    #[test]
    fn the_acknowledgement_says_the_device_keeps_the_old_key_and_nothing_is_deleted() {
        speak(i18n::Locale::En);
        let en = tr!("arca.ack_confirm");
        assert!(en.contains("keeps its current key") && en.contains("Nothing is deleted") && en.contains("(y/n)"), "{en}");
        speak(i18n::Locale::PtBr);
        let pt = tr!("arca.ack_confirm");
        assert!(pt.contains("chave atual") && pt.contains("Nada é apagado") && pt.contains("(s/n)"), "{pt}");
    }

    // ---------- `/arca swap grant` and `/arca swap offer` ----------

    use warden_sync::arca_key_swap::KeySwapError;
    use warden_sync::arca_pairing::PairingError;

    fn every_swap_error() -> Vec<KeySwapError> {
        vec![
            KeySwapError::NotInArcaMode,
            KeySwapError::NothingNewer,
            KeySwapError::NotNewer,
            KeySwapError::NoKey("run a cycle first".into()),
            KeySwapError::NoOffers,
            KeySwapError::Cancelled,
            KeySwapError::TimedOut,
            KeySwapError::HashMismatch,
            KeySwapError::Failed("disk gone".into()),
        ]
    }

    #[test]
    fn each_way_a_swap_can_not_happen_has_its_own_words_in_both_languages() {
        let mut seen_en = Vec::new();
        for error in every_swap_error() {
            speak(i18n::Locale::En);
            let en = key_swap_lines(&error).join("\n");
            speak(i18n::Locale::PtBr);
            let pt = key_swap_lines(&error).join("\n");
            assert!(!en.trim().is_empty() && !pt.trim().is_empty(), "{error:?}");
            assert_ne!(en, pt, "{error:?}: the pt-BR text is not translated");
            assert!(!seen_en.contains(&en), "{error:?} says the same as another: {en}");
            seen_en.push(en);
        }
    }

    #[test]
    fn the_swap_errors_say_what_to_do_next_and_what_was_left_untouched() {
        speak(i18n::Locale::En);
        let mismatch = key_swap_lines(&KeySwapError::HashMismatch).join("\n");
        assert!(mismatch.contains("Nothing was changed") && mismatch.contains("do not trust this hub"), "{mismatch}");
        // The hub only holds the other device's first manifest after it ran a cycle: a right code can fail until then.
        assert!(mismatch.contains("/arca sync"), "{mismatch}");
        // A device in arca mode already has a key, so `/arca pair offer` would refuse it: the texts must not send it there.
        let nothing_newer = key_swap_lines(&KeySwapError::NothingNewer).join("\n");
        assert!(nothing_newer.contains("acknowledged") && !nothing_newer.contains("/arca pair offer"), "{nothing_newer}");
        // `Failed` can come from either device, so the text names where to run what.
        let failed = key_swap_lines(&KeySwapError::Failed("disk gone".into())).join("\n");
        assert!(failed.contains("disk gone") && failed.contains("/arca sync") && failed.contains("/arca swap offer") && failed.contains("replacing its key"), "{failed}");
        assert!(!tr!("arca.ack_confirm").contains("pair it again"), "the acknowledgement must not send the person to a pairing that fails");
        let no_mode = key_swap_lines(&KeySwapError::NotInArcaMode).join("\n");
        assert!(no_mode.contains("/arca on"), "{no_mode}");
    }

    #[test]
    fn a_pairing_error_inside_the_swap_is_said_as_the_pairing_says_it() {
        let error = KeySwapError::Pairing(PairingError::CodeMismatch);
        assert_eq!(key_swap_lines(&error), vec![PairingError::CodeMismatch.to_string()]);
    }

    #[test]
    fn the_swap_screens_send_the_person_to_the_swap_commands_and_say_to_type_the_head_code() {
        speak(i18n::Locale::En);
        let offer = shown_lines(Shown::OfferCode("1234".into()), true).join("\n");
        assert!(offer.contains("1234") && offer.contains("/arca swap grant") && !offer.contains("/arca pair grant"), "{offer}");
        let key = shown_lines(Shown::KeyFingerprint("0123456789abcdef 0000-1111".into()), true).join("\n");
        assert!(key.contains("0123456789abcdef 0000-1111") && key.contains("head code"), "{key}");
        let asked = ask_title(Ask::FingerprintShownByTheOldDevice, true);
        assert!(asked.contains("fingerprint") && asked.contains("head code"), "{asked}");
        speak(i18n::Locale::PtBr);
        let asked_pt = ask_title(Ask::FingerprintShownByTheOldDevice, true);
        assert!(asked_pt.contains("impressão") && asked_pt.contains("código do head"), "{asked_pt}");
    }

    #[test]
    fn the_plain_pairing_screens_do_not_change() {
        speak(i18n::Locale::En);
        let offer = shown_lines(Shown::OfferCode("1234".into()), false).join("\n");
        assert!(offer.contains("/arca pair grant") && !offer.contains("/arca swap"), "{offer}");
        assert_eq!(ask_title(Ask::FingerprintShownByTheOldDevice, false), tr!("arca.ask_fingerprint"));
        assert!(!ask_title(Ask::FingerprintShownByTheOldDevice, false).contains("head code"));
    }

    #[test]
    fn the_swap_confirmation_says_the_local_copy_is_erased_and_rebuilt_from_the_hub() {
        speak(i18n::Locale::En);
        let en = tr!("arca.swap_confirm");
        assert!(en.contains("erase") && en.contains("hub") && en.contains("(y/n)"), "{en}");
        speak(i18n::Locale::PtBr);
        let pt = tr!("arca.swap_confirm");
        assert!(pt.contains("apaga") && pt.contains("hub") && pt.contains("(s/n)"), "{pt}");
    }

    // ---------- `/arca rotate` and `/arca rotate resume` ----------

    /// The official BIP-39 vector for 32 bytes of 0xff (arca-core `recovery.rs`, `VECTORS`, at the revision `warden-sync` pins): the
    /// phrase of a known key, so what a rotation saves can be compared with bytes that were not made by the code under test.
    const ZOO_PHRASE: &str = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote";

    /// A device with a key of its own (random, so never the one a test rotates to) and the arca mode on.
    async fn device_in_arca_mode(name: &str) -> Device {
        let device = arca_device(name, true, false);
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        assert_eq!(arca_on_run(&device.engine, Some("s"), 1).await, ArcaOn::Done { needs_reload: false });
        device
    }

    fn secrets_of(device: &Device) -> warden_sync::manifest::SyncSecrets {
        warden_sync::manifest::load_secrets(&device.base.join("sync_secrets.json")).unwrap().expect("the device has secrets")
    }

    /// What the screen asks back for `pending`: the words of its phrase at the positions it drew.
    fn words_asked(pending: &PendingRotation) -> Vec<String> {
        let phrase = pending.phrase().unwrap();
        let words: Vec<&str> = phrase.words().collect();
        pending.positions().iter().map(|position| words[position - 1].to_string()).collect()
    }

    fn assert_key_untouched(device: &Device, before: &warden_sync::manifest::SyncSecrets, why: &str) {
        let after = secrets_of(device);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch), "{why}: the key and its epoch must stay as they were");
    }

    #[tokio::test]
    async fn a_confirmed_rotation_moves_the_vault_to_the_next_epoch_with_a_new_key() {
        let device = device_in_arca_mode("rotate-ok").await;
        let before = secrets_of(&device);
        assert_eq!(before.key_epoch, 0);
        let pending = PendingRotation::generate().unwrap();
        let typed = words_asked(&pending);
        assert_eq!(typed.len(), 3, "three words are asked back");
        assert_eq!(arca_rotate_run(&device.engine, pending, &typed, Some("s"), Some("sim"), 8).await, ArcaRotate::Done);
        let after = secrets_of(&device);
        assert_eq!(after.key_epoch, 1);
        assert_ne!(after.vault_key, before.vault_key, "the key is the new one, not the old one again");
    }

    #[tokio::test]
    async fn the_words_typed_back_may_have_capitals_and_spaces_around_them() {
        let device = device_in_arca_mode("rotate-capitals").await;
        let pending = PendingRotation::generate().unwrap();
        let typed: Vec<String> = words_asked(&pending).iter().map(|word| format!("  {}  ", word.to_uppercase())).collect();
        assert_eq!(arca_rotate_run(&device.engine, pending, &typed, Some("y"), Some("y"), 8).await, ArcaRotate::Done);
        assert_eq!(secrets_of(&device).key_epoch, 1);
    }

    #[tokio::test]
    async fn without_the_declaration_about_the_lost_token_nothing_is_rotated_even_with_the_right_words() {
        let device = device_in_arca_mode("rotate-undeclared").await;
        let before = secrets_of(&device);
        // The confirmation says yes every time: what is missing is the declaration, and only that.
        for declaration in [None, Some(""), Some("   "), Some("n"), Some("nao"), Some("talvez")] {
            let pending = PendingRotation::generate().unwrap();
            let typed = words_asked(&pending);
            let outcome = arca_rotate_run(&device.engine, pending, &typed, declaration, Some("s"), 8).await;
            assert_eq!(outcome, ArcaRotate::Refused(RotationError::NotDeclared), "{declaration:?}");
            assert_key_untouched(&device, &before, &format!("{declaration:?}"));
        }
    }

    #[tokio::test]
    async fn a_declared_rotation_that_is_not_confirmed_writes_nothing() {
        let device = device_in_arca_mode("rotate-not-confirmed").await;
        let before = secrets_of(&device);
        for confirmation in [None, Some(""), Some("n"), Some("nao"), Some("talvez")] {
            let pending = PendingRotation::generate().unwrap();
            let typed = words_asked(&pending);
            assert_eq!(arca_rotate_run(&device.engine, pending, &typed, Some("s"), confirmation, 8).await, ArcaRotate::Cancelled, "{confirmation:?}");
            assert_key_untouched(&device, &before, &format!("{confirmation:?}"));
        }
    }

    #[tokio::test]
    async fn words_that_are_not_the_ones_asked_do_not_rotate() {
        let device = device_in_arca_mode("rotate-wrong-words").await;
        let before = secrets_of(&device);
        // Each way of getting it wrong, built from what was really asked so none of them is right by luck.
        let ways = ["one letter too many", "a word that exists nowhere", "one word short", "one word too many", "nothing"];
        for (way, what) in ways.iter().enumerate() {
            let pending = PendingRotation::generate().unwrap();
            let mut typed = words_asked(&pending);
            match way {
                0 => typed[0].push('x'),
                1 => {
                    let last = typed.len() - 1;
                    typed[last] = "zzzz-not-a-word".to_string();
                }
                2 => {
                    typed.pop();
                }
                3 => typed.push("abandon".to_string()),
                _ => typed.clear(),
            }
            // Declared and confirmed: the words are the only thing wrong.
            let outcome = arca_rotate_run(&device.engine, pending, &typed, Some("s"), Some("s"), 8).await;
            assert_eq!(outcome, ArcaRotate::Refused(RotationError::WordMismatch), "{what}");
            assert_key_untouched(&device, &before, what);
        }
    }

    #[tokio::test]
    async fn resuming_with_the_24_words_asks_for_no_words_and_saves_exactly_that_key() {
        let device = device_in_arca_mode("rotate-resume").await;
        let before = secrets_of(&device);
        assert_ne!(before.vault_key, [0xff; 32], "the device starts with another key");
        // Typed the way a person does: capitals, wide gaps and a line break around it.
        let typed_phrase = format!("  {}\n", ZOO_PHRASE.to_uppercase().replace(' ', "   "));
        let pending = PendingRotation::from_phrase(&typed_phrase).expect("a valid phrase");
        assert!(pending.positions().is_empty(), "the 24 words are the confirmation: none is asked back");
        assert_eq!(arca_rotate_run(&device.engine, pending, &[], Some("y"), Some("y"), 8).await, ArcaRotate::Done);
        let after = secrets_of(&device);
        assert_eq!(after.vault_key, [0xff; 32], "the key saved is the one the 24 words write");
        assert_eq!(after.key_epoch, 1);
    }

    #[tokio::test]
    async fn resuming_needs_the_declaration_and_the_confirmation_too() {
        let device = device_in_arca_mode("rotate-resume-answers").await;
        let before = secrets_of(&device);
        let resume = || PendingRotation::from_phrase(ZOO_PHRASE).unwrap();
        assert_eq!(arca_rotate_run(&device.engine, resume(), &[], Some("n"), Some("s"), 8).await, ArcaRotate::Refused(RotationError::NotDeclared));
        assert_key_untouched(&device, &before, "no declaration");
        assert_eq!(arca_rotate_run(&device.engine, resume(), &[], Some("s"), Some("n"), 8).await, ArcaRotate::Cancelled);
        assert_key_untouched(&device, &before, "no confirmation");
    }

    #[tokio::test]
    async fn rotating_a_vault_that_is_not_in_arca_mode_is_refused_and_writes_nothing() {
        let device = arca_device("rotate-mode-off", true, false);
        let before = secrets_of(&device);
        let pending = PendingRotation::generate().unwrap();
        let typed = words_asked(&pending);
        assert_eq!(arca_rotate_run(&device.engine, pending, &typed, Some("s"), Some("s"), 8).await, ArcaRotate::Refused(RotationError::NotInArcaMode));
        assert_key_untouched(&device, &before, "mode off");
    }

    #[test]
    fn the_phrase_is_shown_in_numbered_rows_with_every_word_in_order() {
        let pending = PendingRotation::generate().unwrap();
        let phrase = pending.phrase().unwrap();
        let words: Vec<&str> = phrase.words().collect();
        assert_eq!(words.len(), 24);
        for (per_row, rows) in [(4, 6), (3, 8), (2, 12), (1, 24), (5, 5), (24, 1)] {
            let shown = phrase_rows(words.iter().copied(), per_row);
            assert_eq!(shown.len(), rows, "{per_row} per row");
            let tokens: Vec<&str> = shown.iter().flat_map(|row| row.split_whitespace()).collect();
            let numbers: Vec<&str> = tokens.iter().copied().step_by(2).collect();
            let shown_words: Vec<&str> = tokens.iter().copied().skip(1).step_by(2).collect();
            let expected_numbers: Vec<String> = (1..=24).map(|n| format!("{n}.")).collect();
            assert_eq!(numbers, expected_numbers.iter().map(String::as_str).collect::<Vec<_>>(), "{per_row} per row");
            assert_eq!(shown_words, words, "{per_row} per row");
        }
        // A row width of zero must not panic; it is one word per row.
        assert_eq!(phrase_rows(words.iter().copied(), 0).len(), 24);
    }

    #[test]
    fn the_rows_are_as_wide_as_the_terminal_allows() {
        // One item is 12 columns ("24. " and 8 for the longest word), the gap between items 2, the box border 2 and 2 columns of
        // slack: n items need 14 * n + 2. Those numbers are this function's own design, not a fact from outside.
        for (width, per_row) in [(80, 4), (58, 4), (57, 3), (44, 3), (43, 2), (30, 2), (29, 1), (0, 1)] {
            assert_eq!(phrase_per_row(width), per_row, "width {width}");
        }
    }

    /// Rows of text of what `draw_phrase_box` painted in a `width` x `height` area, the border characters turned into spaces.
    fn painted_rows(rows: &[String], title: &str, keys: &str, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        draw_phrase_box(&mut buf, area, rows, title, keys);
        (0..height).map(|y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().replace(['│', '╭', '╮', '╰', '╯', '─'], " ")).collect()
    }

    #[test]
    fn every_word_of_the_phrase_and_the_keys_are_on_the_screen_at_each_width_the_layout_accepts() {
        let pending = PendingRotation::generate().unwrap();
        let phrase = pending.phrase().unwrap();
        let words: Vec<&str> = phrase.words().collect();
        for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
            speak(locale);
            let (title, keys) = (tr!("arca.rotate_phrase_title"), tr!("arca.rotate_phrase_keys"));
            assert!(keys.chars().count() <= 27, "the keys row must fit the narrowest box (29 columns, 2 of them border): {keys}");
            for width in [80u16, 58, 44, 30, 29] {
                let per_row = phrase_layout(width as usize, 100).unwrap_or_else(|| panic!("width {width} is accepted"));
                let rows = phrase_rows(words.iter().copied(), per_row);
                // The box is the rows, the keys row and two borders high.
                let painted = painted_rows(&rows, &title, &keys, width, rows.len() as u16 + 3);
                let inner = &painted[1..=rows.len()];
                let tokens: Vec<&str> = inner.iter().flat_map(|row| row.split_whitespace()).collect();
                let numbers: Vec<&str> = tokens.iter().copied().step_by(2).collect();
                let shown: Vec<&str> = tokens.iter().copied().skip(1).step_by(2).collect();
                let expected_numbers: Vec<String> = (1..=24).map(|n| format!("{n}.")).collect();
                assert_eq!(numbers, expected_numbers.iter().map(String::as_str).collect::<Vec<_>>(), "width {width}: every number, none cut");
                assert_eq!(shown, words, "width {width}: every word whole and in order");
                let last = &painted[rows.len() + 1];
                assert!(last.contains("Enter") && last.contains("Ctrl+D"), "width {width}: the keys are on the screen: {last:?}");
            }
        }
    }

    #[test]
    fn a_terminal_that_cannot_show_every_word_is_refused_before_anything_is_made() {
        // (width, height, words per row if accepted). A row is 24 / per_row lines, plus 2 border lines and the keys row.
        for (width, height, expected) in [
            (80, 24, Some(4)),
            (80, 9, Some(4)),
            (80, 8, None),
            (44, 11, Some(3)),
            (44, 10, None),
            (30, 15, Some(2)),
            (30, 14, None),
            (29, 27, Some(1)),
            (29, 26, None),
            (28, 100, None),
            (0, 0, None),
            (80, 0, None),
        ] {
            assert_eq!(phrase_layout(width, height), expected, "{width}x{height}");
        }
    }

    /// WEAK ON PURPOSE, and said so: it asserts an absence, so any implementation that never prints the phrase passes it, and it
    /// passes the skeleton too. What proves the phrase IS on the screen that shows it is
    /// `every_word_of_the_phrase_and_the_keys_are_on_the_screen_at_each_width_the_layout_accepts` above; this one only guards that
    /// no other text (the questions, the cards, the `Debug` of what the command holds) ever starts to carry it.
    #[test]
    fn the_phrase_is_written_by_no_text_but_the_screen_that_shows_it() {
        let pending = PendingRotation::generate().unwrap();
        let phrase = pending.phrase().unwrap();
        let words: Vec<&str> = phrase.words().collect();
        // The whole phrase and each half of it: a single word could be in an ordinary sentence by chance ("about", "all").
        let secret = [phrase.expose().to_string(), words[..12].join(" "), words[12..].join(" ")];

        let mut elsewhere = vec![format!("{pending:?}"), format!("{:?}", ArcaRotate::Done), format!("{:?}", ArcaRotate::Refused(RotationError::WordMismatch))];
        assert_eq!(elsewhere[0], "PendingRotation(<redacted>)");
        let errors = [
            RotationError::NotDeclared,
            RotationError::WordMismatch,
            RotationError::BadPhrase,
            RotationError::NotInArcaMode,
            RotationError::Refused("same key".into()),
            RotationError::NewerEpoch,
            RotationError::Failed("disk gone".into()),
        ];
        for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
            speak(locale);
            for position in 1..=24 {
                elsewhere.push(tr!("arca.rotate_ask_word", position = position));
            }
            for key in ["arca.rotate_intro", "arca.rotate_resume_intro", "arca.rotate_phrase_title", "arca.rotate_phrase_keys", "arca.rotate_ask_phrase", "arca.rotate_ask_revoked", "arca.rotate_confirm", "arca.rotate_sending", "arca.rotate_terminal_small", "arca.rotate_resized", "arca.rotate_no_arguments"] {
                elsewhere.push(tr!(key));
            }
            for error in &errors {
                elsewhere.extend(rotation_error_lines(error, false));
                elsewhere.extend(rotation_error_lines(error, true));
            }
            elsewhere.extend(arca_rotated_lines(None));
            elsewhere.extend(arca_rotated_lines(Some(&Ok(ArcaSyncReport::default()))));
            elsewhere.extend(arca_rotated_lines(Some(&Err("hub down".to_string()))));
        }
        assert!(elsewhere.len() > 60, "the texts were really collected: {}", elsewhere.len());
        for text in &elsewhere {
            for part in &secret {
                assert!(!text.contains(part.as_str()), "the phrase leaked into: {text}");
            }
        }
    }

    #[test]
    fn each_way_a_rotation_can_stop_has_its_own_words_in_both_languages() {
        let errors = [
            RotationError::NotDeclared,
            RotationError::WordMismatch,
            RotationError::BadPhrase,
            RotationError::NotInArcaMode,
            RotationError::Refused("same key".into()),
            RotationError::NewerEpoch,
            RotationError::Failed("disk gone".into()),
        ];
        let mut seen_en = Vec::new();
        for error in errors {
            for resuming in [false, true] {
                speak(i18n::Locale::En);
                let en = rotation_error_lines(&error, resuming).join("\n");
                speak(i18n::Locale::PtBr);
                let pt = rotation_error_lines(&error, resuming).join("\n");
                assert!(!en.trim().is_empty() && !pt.trim().is_empty(), "{error:?}");
                assert_ne!(en, pt, "{error:?}: the pt-BR text is not translated");
                // The same text may serve both ways of running it (a stop half way, a bad phrase); two different errors never share one.
                if !seen_en.contains(&en) {
                    seen_en.push(en);
                }
            }
        }
        assert!(seen_en.len() >= 7, "each of the 7 errors says something of its own: {}", seen_en.len());
        // Every text and every key this fix added exists in both catalogs (a missing key shows itself).
        for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
            speak(locale);
            for key in ["arca.rotate_phrase_keys", "arca.rotate_terminal_small", "arca.rotate_resized", "arca.rotate_no_arguments", "arca.rotate_discarded", "arca.rotate_refused", "arca.rotate_newer_epoch"] {
                assert_ne!(tr!(key), key, "{key} is missing from the catalog of {locale:?}");
            }
        }
    }

    #[test]
    fn a_refusal_before_the_first_write_never_sends_the_person_to_resume_and_a_stop_half_way_always_does() {
        speak(i18n::Locale::En);
        let lines = |error: &RotationError, resuming: bool| rotation_error_lines(error, resuming).join("\n");
        // `/arca rotate` (a phrase was shown): nothing was written, the phrase is void, the next step is a new rotation.
        for error in [RotationError::NotDeclared, RotationError::WordMismatch, RotationError::NotInArcaMode, RotationError::Refused("same key".into()), RotationError::NewerEpoch] {
            let text = lines(&error, false);
            assert!(text.contains("discarded"), "{error:?}: the person may throw the paper away: {text}");
            if !matches!(error, RotationError::NewerEpoch) {
                assert!(!text.contains("resume"), "{error:?}: nothing was written, so there is nothing to resume: {text}");
            }
        }
        // `/arca rotate resume` (no phrase was shown, an earlier try may have written): never "discarded", never "the key was not changed".
        for error in [RotationError::NotDeclared, RotationError::NotInArcaMode, RotationError::Refused("same key".into()), RotationError::NewerEpoch] {
            let text = lines(&error, true);
            assert!(!text.contains("discarded") && !text.contains("not changed"), "{error:?}: an earlier try may have written: {text}");
        }
        let undeclared = lines(&RotationError::NotDeclared, false);
        assert!(undeclared.contains("revoked") && undeclared.contains("wrote nothing") && undeclared.contains("/arca rotate again"), "{undeclared}");
        let undeclared_resume = lines(&RotationError::NotDeclared, true);
        assert!(undeclared_resume.contains("/arca rotate resume again"), "{undeclared_resume}");
        let mismatch = lines(&RotationError::WordMismatch, false);
        assert!(mismatch.contains("wrote nothing") && mismatch.contains("/arca rotate again"), "{mismatch}");
        // The reason the core gave is said once, in the CLI's own sentence: the core's own "nothing was changed:" is not repeated.
        let refused = lines(&RotationError::Refused("same key".into()), false);
        assert_eq!(refused.matches("same key").count(), 1, "{refused}");
        assert!(!refused.contains("nothing was changed") && refused.contains("/arca rotate again"), "{refused}");
        assert!(lines(&RotationError::Refused("same key".into()), true).contains("/arca rotate resume again"));
        let no_mode = lines(&RotationError::NotInArcaMode, false);
        assert!(no_mode.contains("/arca on"), "{no_mode}");
        // A newer epoch: a new key hits the same wall, so the text says where each cause is handled, and does NOT offer `/arca ack`
        // as a shortcut (the epoch of an object is not authenticated: whoever has the old key can forge one).
        for resuming in [false, true] {
            let newer = lines(&RotationError::NewerEpoch, resuming);
            assert!(newer.contains("/arca swap offer") && newer.contains("/arca rotate resume") && newer.contains("ANOTHER") && newer.contains("not authenticated"), "{newer}");
            assert!(!newer.contains("/arca ack"), "{newer}");
        }
        // A stop half way (it may have written): the same 24 words, never a new key, and nothing says the phrase is void.
        for resuming in [false, true] {
            let half_way = lines(&RotationError::Failed("disk gone".into()), resuming);
            assert!(half_way.contains("(disk gone)") && half_way.contains("/arca rotate resume") && half_way.contains("Do not make a new key") && half_way.contains("same 24 words") && half_way.contains("hidden field"), "{half_way}");
            assert!(!half_way.contains("discarded") && !half_way.contains("the key was not rotated"), "the core's English sentence is not pasted in: {half_way}");
        }
        speak(i18n::Locale::PtBr);
        let half_way_pt = lines(&RotationError::Failed("disk gone".into()), false);
        assert!(half_way_pt.contains("(disk gone)") && half_way_pt.contains("/arca rotate resume") && half_way_pt.contains("Não gere uma chave nova") && half_way_pt.contains("mesmas 24 palavras") && half_way_pt.contains("campo escondido"), "{half_way_pt}");
        assert!(!half_way_pt.contains("the key was not rotated"), "an English sentence of the core inside the pt-BR text contradicts it: {half_way_pt}");
        let newer_pt = lines(&RotationError::NewerEpoch, false);
        assert!(newer_pt.contains("/arca swap offer") && newer_pt.contains("OUTRA") && !newer_pt.contains("/arca ack"), "{newer_pt}");
        assert!(lines(&RotationError::NotDeclared, false).contains("descartada"), "pt-BR says the phrase is void too");
    }

    #[test]
    fn after_a_rotation_the_card_says_whether_the_new_epoch_reached_the_hub() {
        speak(i18n::Locale::En);
        let report = ArcaSyncReport { pulled: 2, pushed: 1, files_changed: 3, ..Default::default() };
        let sent = arca_rotated_lines(Some(&Ok(report))).join("\n");
        assert!(sent.contains("sent to the hub") && sent.contains("/arca swap grant") && sent.contains("/arca swap offer"), "{sent}");
        assert!(sent.contains("cycle done: 2 received, 1 sent, 3 file(s) changed here"), "what the cycle did is said too: {sent}");
        let not_sent = arca_rotated_lines(Some(&Err("hub down".to_string()))).join("\n");
        assert!(not_sent.contains("hub down") && not_sent.contains("/arca sync") && not_sent.contains("changed on this computer"), "{not_sent}");
        assert!(!not_sent.contains("changed and sent"), "a failed send must not read as a sent key: {not_sent}");
        let no_hub = arca_rotated_lines(None).join("\n");
        assert!(no_hub.contains("changed on this computer") && no_hub.contains("/arca sync"), "{no_hub}");
        assert!(!no_hub.contains("changed and sent"), "{no_hub}");
        speak(i18n::Locale::PtBr);
        let sent_pt = arca_rotated_lines(Some(&Ok(ArcaSyncReport::default()))).join("\n");
        assert!(sent_pt.contains("enviada ao hub") && sent_pt.contains("/arca swap grant"), "{sent_pt}");
        let not_sent_pt = arca_rotated_lines(Some(&Err("hub down".to_string()))).join("\n");
        assert!(not_sent_pt.contains("hub down") && not_sent_pt.contains("/arca sync") && not_sent_pt.contains("não foi enviada"), "{not_sent_pt}");
    }

    #[test]
    fn the_questions_say_what_is_at_stake_and_which_word_is_asked() {
        speak(i18n::Locale::En);
        let revoked = tr!("arca.rotate_ask_revoked");
        assert!(revoked.contains("revoke") && revoked.contains("hub") && revoked.contains("(y/n)"), "{revoked}");
        let confirm = tr!("arca.rotate_confirm");
        assert!(confirm.contains("cannot be undone") && confirm.contains("/arca swap offer") && confirm.contains("(y/n)"), "{confirm}");
        assert!(tr!("arca.rotate_ask_word", position = 17).contains("17"));
        let intro = tr!("arca.rotate_intro");
        assert!(intro.contains("24 words") && intro.contains("ONCE") && intro.contains("revoke"), "{intro}");
        // What the clearing does NOT guarantee is said where the person reads it: a resize, tmux/screen and `script` can keep a copy.
        assert!(intro.contains("tmux") && intro.contains("script") && intro.contains("resiz"), "{intro}");
        // The key has already changed when this is shown (it is the `Done` arm), so it must not speak as if it were going to.
        let sending = tr!("arca.rotate_sending");
        assert!(sending.contains("was changed on this computer") && !sending.contains("is being"), "{sending}");
        speak(i18n::Locale::PtBr);
        let intro_pt = tr!("arca.rotate_intro");
        assert!(intro_pt.contains("tmux") && intro_pt.contains("script") && intro_pt.contains("redimension"), "{intro_pt}");
        let sending_pt = tr!("arca.rotate_sending");
        assert!(sending_pt.contains("foi trocada neste computador") && !sending_pt.contains("está sendo"), "{sending_pt}");
        let revoked_pt = tr!("arca.rotate_ask_revoked");
        assert!(revoked_pt.contains("revogou") && revoked_pt.contains("(s/n)"), "{revoked_pt}");
        let confirm_pt = tr!("arca.rotate_confirm");
        assert!(confirm_pt.contains("desfazer") && confirm_pt.contains("(s/n)"), "{confirm_pt}");
        assert!(tr!("arca.rotate_ask_word", position = 17).contains("17"));
    }

    #[test]
    fn a_line_with_words_after_arca_rotate_is_neither_kept_in_the_history_nor_echoed_nor_saved() {
        // Two real phrases: the official 0xff vector and one just made. (The messages below never print the line: a failing run
        // would otherwise put a phrase in the log.)
        let made = PendingRotation::generate().unwrap();
        let made_phrase = made.phrase().unwrap();
        for phrase in [ZOO_PHRASE.to_string(), made_phrase.expose().to_string()] {
            let words: Vec<&str> = phrase.split(' ').collect();
            let secret = [phrase.clone(), words[..12].join(" "), words[12..].join(" ")];
            for line in [format!("/arca rotate resume {phrase}"), format!("/arca rotate {phrase}"), format!("  /ARCA   rotate   resume   {phrase}  ")] {
                // The line history as a session builds it, saved the way the session saves it.
                let mut history = Vec::new();
                for typed in ["/arca on", line.as_str(), "/arca rotate resume", "/arca rotate", "hello", "   "] {
                    remember_line(&mut history, typed);
                }
                assert_eq!(history, vec!["/arca on", "/arca rotate resume", "/arca rotate", "hello"], "only the line with words stays out; blank lines never go in");
                let path = history_dir("rotate-line").join("cli_history.txt");
                save_history(&path, &history);
                let saved = std::fs::read_to_string(&path).unwrap();
                let _ = std::fs::remove_dir_all(path.parent().unwrap());
                for part in &secret {
                    assert!(!saved.contains(part.as_str()), "the phrase reached the history file");
                }
                for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
                    speak(locale);
                    let said = unknown_command_lines(&line);
                    for part in &secret {
                        assert!(!said.contains(part.as_str()), "the phrase was echoed in the unknown-command card");
                    }
                    assert!(said.contains("/arca rotate resume"), "the card says what to do instead: {said}");
                }
            }
        }
        // A short unknown command is still recalled from the history; the card names the command (its first word), not the rest.
        speak(i18n::Locale::En);
        // ("now" would be wrong here: it is inside the word "unknown" of the message itself.)
        let said_short = unknown_command_lines("/nonsense zebra");
        assert!(said_short.contains("/nonsense") && !said_short.contains("zebra"), "{said_short}");
        let mut history = Vec::new();
        remember_line(&mut history, "/nonsense now");
        assert_eq!(history, vec!["/nonsense now"]);
        let said = unknown_command_lines("/arca rotate resume abandon");
        assert!(said.contains("hidden") && !said.contains("abandon"), "{said}");
        speak(i18n::Locale::PtBr);
        assert!(unknown_command_lines("/arca rotate resume abandon").contains("escondido"));
    }

    #[test]
    fn a_phrase_on_any_unknown_slash_line_is_neither_kept_nor_echoed_nor_saved_nor_reloaded() {
        let made = PendingRotation::generate().unwrap();
        let made_phrase = made.phrase().unwrap();
        for phrase in [ZOO_PHRASE.to_string(), made_phrase.expose().to_string()] {
            let words: Vec<&str> = phrase.split(' ').collect();
            let secret = [phrase.clone(), words[..12].join(" "), words[12..].join(" ")];
            let zero_width_joined = phrase.replace(' ', "\u{200B}");
            let comma_joined = phrase.replace(' ', ",");
            let lines = [
                format!("/arca rotat resume {phrase}"),
                format!("/arca rotatex {phrase}"),
                format!("/arca resume {phrase}"),
                format!("/arca rotate-resume {phrase}"),
                format!("/rotate resume {phrase}"),
                format!("/arca\u{200B}rotate resume {phrase}"),
                format!("/arca rotate\u{200B}resume {phrase}"),
                format!("/arca rotat resume {zero_width_joined}"),
                format!("/arca rotat resume {comma_joined}"),
            ];
            for line in &lines {
                let mut history = Vec::new();
                for typed in ["/arca on", line.as_str(), "hello"] {
                    remember_line(&mut history, typed);
                }
                assert_eq!(history, vec!["/arca on", "hello"], "the line with the words stays out of the history");
                // What an OLDER session already wrote is not reloaded (and so not written back either).
                let path = history_dir("unknown-slash").join("cli_history.txt");
                std::fs::write(&path, format!("/arca on\n{line}\n\nhello\n/nonsense now\n")).unwrap();
                let (loaded, _) = load_history(&path);
                let _ = std::fs::remove_dir_all(path.parent().unwrap());
                assert_eq!(loaded, vec!["/arca on", "hello", "/nonsense now"], "the old line with the words is dropped on load; the rest is kept");
                for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
                    speak(locale);
                    let said = unknown_command_lines(line);
                    for part in &secret {
                        assert!(!said.contains(part.as_str()), "the phrase was echoed in the unknown-command card");
                    }
                    assert!(!said.contains("resume") || said.contains("/arca rotate resume"), "only the first word is echoed, never the rest of the line: {said}");
                }
            }
        }
    }

    #[test]
    fn a_phrase_typed_as_chat_is_neither_kept_in_the_history_nor_saved_nor_reloaded() {
        // Same three real phrases as the `commands.rs` tests (official BIP-39 vectors of arca-core `recovery.rs`; `ZOO_PHRASE` is the
        // first), plus one just made: not only the fixture of this module.
        let made = PendingRotation::generate().unwrap();
        let made_phrase = made.phrase().unwrap();
        let abandon = format!("{} art", ["abandon"; 23].join(" "));
        let legal = "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title";
        // 24 words of the list that are NOT a phrase (wrong checksum word), 23 words and a sentence: plain chat, kept as before.
        let wrong_checksum = ["zoo"; 24].join(" ");
        let twenty_three_words = ["zoo"; 23].join(" ");
        let sentence = "what is a zoo".to_string();
        for (index, phrase) in [ZOO_PHRASE.to_string(), abandon, legal.to_string(), made_phrase.expose().to_string()].into_iter().enumerate() {
            for line in [phrase.clone(), format!("  {}  ", phrase.to_uppercase()), phrase.replace(' ', "   ")] {
                let mut history = Vec::new();
                for typed in ["/arca on", line.as_str(), wrong_checksum.as_str(), twenty_three_words.as_str(), sentence.as_str(), "   "] {
                    remember_line(&mut history, typed);
                }
                // (`assert!` and not `assert_eq!`: a failing run must not print the history, which may hold a phrase, in the log.)
                assert!(history == ["/arca on", wrong_checksum.as_str(), twenty_three_words.as_str(), sentence.as_str()], "phrase #{index}: only the phrase stays out ({} lines kept)", history.len());
                let path = history_dir("bare-phrase").join("cli_history.txt");
                save_history(&path, &history);
                let saved = std::fs::read_to_string(&path).unwrap();
                assert!(saved == history.join("\n"), "phrase #{index}: the file holds exactly the lines kept");
                // What an OLDER session already wrote is not reloaded (and so not written back either).
                std::fs::write(&path, format!("/arca on\n{line}\n{sentence}\n")).unwrap();
                let (loaded, _) = load_history(&path);
                let _ = std::fs::remove_dir_all(path.parent().unwrap());
                assert!(loaded == ["/arca on", sentence.as_str()], "phrase #{index}: the old line with the phrase is dropped on load ({} lines loaded)", loaded.len());
            }
        }
    }

    // (The messages of the history-file tests name the case by its index and never print a line or the file: a failing run must not
    // put a phrase in the log.)

    #[cfg(unix)]
    #[test]
    fn a_history_file_is_created_private_in_a_private_directory() {
        let base = history_dir("new-private");
        // The directory of the file does not exist yet: the save creates it (with the parents), as the first session does.
        let path = base.join("warden").join("cli_history.txt");
        save_history(&path, &["/arca on".to_string(), "hello".to_string()]);
        assert!(std::fs::read_to_string(&path).unwrap() == "/arca on\nhello", "the file holds the lines saved");
        let file_mode = mode_of(&path);
        let dir_mode = mode_of(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(&base);
        assert!(file_mode == 0o600, "the new history file must be readable by its owner only (mode {file_mode:o})");
        assert!(dir_mode == 0o700, "the new history directory must be open to its owner only (mode {dir_mode:o})");
    }

    #[cfg(unix)]
    #[test]
    fn a_history_file_and_directory_that_were_open_are_closed_on_load_and_on_save() {
        let dir = history_dir("open-before");
        let path = dir.join("cli_history.txt");
        std::fs::write(&path, "/arca on\nhello").unwrap();
        set_mode(&path, 0o644);
        set_mode(&dir, 0o755);
        // Loading closes them even though nothing is rewritten (no phrase in the file).
        let (loaded, scrub) = load_history(&path);
        assert!(loaded == ["/arca on", "hello"] && scrub == HistoryScrub::Untouched, "a file without a phrase is read as it is");
        let (file_after_load, dir_after_load) = (mode_of(&path), mode_of(&dir));
        // Saving closes them too, if something opened them again between the two.
        set_mode(&path, 0o644);
        set_mode(&dir, 0o755);
        save_history(&path, &loaded);
        let (file_after_save, dir_after_save) = (mode_of(&path), mode_of(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(file_after_load == 0o600 && dir_after_load == 0o700, "after the load: file {file_after_load:o}, directory {dir_after_load:o}");
        assert!(file_after_save == 0o600 && dir_after_save == 0o700, "after the save: file {file_after_save:o}, directory {dir_after_save:o}");
    }

    #[test]
    fn an_old_phrase_is_erased_from_the_history_file_at_load_not_at_the_end_of_the_session() {
        // A phrase typed before the filter existed, between lines that are not phrases: one that is 24 words of the list but not a
        // phrase (wrong checksum), a sentence and an unknown slash command are chat and stay.
        let wrong_checksum = ["zoo"; 24].join(" ");
        for (index, phrase) in planted_phrases().into_iter().enumerate() {
            let dir = history_dir("scrub");
            let path = dir.join("cli_history.txt");
            let old = format!("list my files\n/models\n{phrase}\n{wrong_checksum}\nwhat is a zoo\n  {}  \n/nonsense now", phrase.to_uppercase().replace(' ', "   "));
            std::fs::write(&path, old).unwrap();
            let (loaded, scrub) = load_history(&path);
            // No save in between: what a crash or a kill of the session would leave.
            let on_disk = std::fs::read_to_string(&path).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
            let kept = ["list my files", "/models", wrong_checksum.as_str(), "what is a zoo", "/nonsense now"];
            assert!(scrub == HistoryScrub::Rewritten, "phrase #{index}: the load must say that it erased a line");
            assert!(loaded == kept, "phrase #{index}: the lines that are not phrases stay, in the same order ({} loaded)", loaded.len());
            assert!(!on_disk.contains(&phrase) && !on_disk.contains(&phrase.to_uppercase().replace(' ', "   ")), "phrase #{index}: the phrase is still in the file after the load");
            assert!(on_disk == kept.join("\n"), "phrase #{index}: the file holds exactly the lines that stay, in order");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_rewritten_history_file_is_private_even_if_the_old_one_was_open() {
        let dir = history_dir("scrub-mode");
        let path = dir.join("cli_history.txt");
        std::fs::write(&path, format!("hello\n{ZOO_PHRASE}\n")).unwrap();
        set_mode(&path, 0o644);
        let (_, scrub) = load_history(&path);
        let mode = mode_of(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(scrub == HistoryScrub::Rewritten, "the load must have rewritten the file");
        assert!(mode == 0o600, "the rewritten file must be private (mode {mode:o})");
    }

    #[test]
    fn a_history_file_without_a_phrase_is_not_rewritten_and_nothing_is_said() {
        // Blank lines and a final newline are what a rewrite would take out: they staying is the proof that the file was not touched.
        let old = "/arca on\n\nlist my files\n\n/nonsense now\n";
        let dir = history_dir("no-scrub");
        let path = dir.join("cli_history.txt");
        std::fs::write(&path, old).unwrap();
        let (loaded, scrub) = load_history(&path);
        let on_disk = std::fs::read_to_string(&path).unwrap();
        let names = names_in(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(loaded == ["/arca on", "list my files", "/nonsense now"], "the lines are read as before ({} loaded)", loaded.len());
        assert!(scrub == HistoryScrub::Untouched, "no phrase: no warning");
        assert!(on_disk == old, "the file must stay byte for byte as it was");
        assert!(names == ["cli_history.txt"], "nothing else is created: {names:?}");
        // No file at all (the first session) is not a phrase either.
        let (none, scrub_none) = load_history(&std::env::temp_dir().join("warden-cli-history-that-does-not-exist").join("cli_history.txt"));
        assert!(none.is_empty() && scrub_none == HistoryScrub::Untouched);
    }

    #[test]
    fn rewriting_the_history_file_leaves_no_temporary_file_behind() {
        let dir = history_dir("atomic");
        let path = dir.join("cli_history.txt");
        std::fs::write(&path, format!("hello\n{ZOO_PHRASE}\nworld")).unwrap();
        let (loaded, scrub) = load_history(&path);
        assert!(scrub == HistoryScrub::Rewritten, "the load must have rewritten the file");
        let after_load = names_in(&dir);
        // The save at the end of the session goes the same way.
        save_history(&path, &loaded);
        let after_save = names_in(&dir);
        let on_disk = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(after_load == ["cli_history.txt"], "after the load: {after_load:?}");
        assert!(after_save == ["cli_history.txt"], "after the save: {after_save:?}");
        assert!(on_disk == "hello\nworld", "the file holds the lines that stay");
    }

    #[test]
    fn the_card_for_an_old_phrase_says_it_is_exposed_and_to_rotate_and_repeats_no_word_of_it() {
        // The words of the phrases planted above: the card must not carry one of them (a card that echoed the line would defeat the
        // erasing).
        let phrase_words: std::collections::BTreeSet<String> = planted_phrases().iter().flat_map(|phrase| phrase.split(' ').map(str::to_string)).collect();
        for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
            speak(locale);
            for text in [tr!("history.phrase_scrubbed"), tr!("history.phrase_not_scrubbed", path = "/home/someone/cli_history.txt")] {
                assert!(!text.starts_with("history."), "{locale:?}: the key is missing from the catalog");
                assert!(text.contains("/arca rotate"), "{locale:?}: {text}");
                let repeated: Vec<&str> = text.split(|c: char| !c.is_alphabetic()).filter(|word| phrase_words.contains(&word.to_lowercase())).collect();
                assert!(repeated.is_empty(), "{locale:?}: the card repeats words of a phrase: {repeated:?}");
            }
        }
        speak(i18n::Locale::En);
        let en = tr!("history.phrase_scrubbed");
        assert!(en.contains("EXPOSED") && en.contains("AI provider") && en.contains("erased"), "{en}");
        let not_en = tr!("history.phrase_not_scrubbed", path = "/home/someone/cli_history.txt");
        assert!(not_en.contains("EXPOSED") && not_en.contains("/home/someone/cli_history.txt") && not_en.contains("could not"), "{not_en}");
        speak(i18n::Locale::PtBr);
        let pt = tr!("history.phrase_scrubbed");
        assert!(pt.contains("EXPOSTA") && pt.contains("provedor de IA") && pt.contains("apagada"), "{pt}");
        let not_pt = tr!("history.phrase_not_scrubbed", path = "/home/someone/cli_history.txt");
        assert!(not_pt.contains("EXPOSTA") && not_pt.contains("/home/someone/cli_history.txt") && not_pt.contains("não foi possível"), "{not_pt}");
        assert_ne!(en, pt, "the pt-BR text is not translated");
    }

    #[test]
    fn the_card_for_a_phrase_typed_as_chat_says_it_was_not_sent_nor_kept_and_where_the_rotation_is() {
        for locale in [i18n::Locale::En, i18n::Locale::PtBr] {
            speak(locale);
            let text = tr!("arca.phrase_in_chat");
            assert_ne!(text, "arca.phrase_in_chat", "the key is missing from the catalog of {locale:?}");
            assert!(text.contains("/arca rotate resume") && text.contains("/arca rotate"), "{locale:?}: {text}");
        }
        speak(i18n::Locale::En);
        let en = tr!("arca.phrase_in_chat");
        assert!(en.contains("not sent") && en.contains("not kept"), "{en}");
        speak(i18n::Locale::PtBr);
        let pt = tr!("arca.phrase_in_chat");
        assert!(pt.contains("não foi enviada") && pt.contains("não foi guardada"), "{pt}");
        assert_ne!(en, pt, "the pt-BR text is not translated");
    }

    #[test]
    fn the_loop_routes_a_phrase_typed_as_chat_to_the_blocked_card_and_everything_else_as_before() {
        // The loop dispatches on this route and on nothing else, so a phrase can only reach the model (`Parsed(NotACommand)` is the
        // one route that does) if this function says so. Phrases: the three official vectors and one just made, in the shapes a
        // person types them. (The messages name the case by its index: a failing run must not put a phrase in the log.)
        let made = PendingRotation::generate().unwrap();
        let made_phrase = made.phrase().unwrap();
        let abandon = format!("{} art", ["abandon"; 23].join(" "));
        let legal = "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title";
        for (index, phrase) in [ZOO_PHRASE.to_string(), abandon, legal.to_string(), made_phrase.expose().to_string()].into_iter().enumerate() {
            for (variant, line) in [phrase.clone(), phrase.to_uppercase(), phrase.replace(' ', "   ")].iter().enumerate() {
                assert!(matches!(classify_line(line.trim()), LineRoute::BlockedPhrase), "phrase #{index}, variant #{variant} was not blocked");
            }
            // Behind a slash it is the parser's business and stays so: refused, never blocked and never chat.
            assert!(matches!(classify_line(&format!("/arca rotate resume {phrase}")), LineRoute::Parsed(ParseOutcome::Unrecognized(_))), "phrase #{index} after /arca rotate resume");
        }
        // What was not a phrase goes where it always went.
        let wrong_checksum = ["zoo"; 24].join(" ");
        let twenty_three_words = ["zoo"; 23].join(" ");
        for (line, what) in [("hello", "chat"), ("what is a zoo", "chat"), (wrong_checksum.as_str(), "24 words, wrong checksum"), (twenty_three_words.as_str(), "23 words")] {
            assert!(matches!(classify_line(line), LineRoute::Parsed(ParseOutcome::NotACommand)), "{what} is still chat");
        }
        assert!(matches!(classify_line(""), LineRoute::Blank));
        for line in ["exit", "quit"] {
            assert!(matches!(classify_line(line), LineRoute::Exit), "{line}");
        }
        assert!(matches!(classify_line("/exit"), LineRoute::Parsed(ParseOutcome::Recognized(Command::Exit))));
        assert!(matches!(classify_line("/arca rotate resume"), LineRoute::Parsed(ParseOutcome::Recognized(Command::ArcaRotateResume))));
        assert!(matches!(classify_line("/nonsense"), LineRoute::Parsed(ParseOutcome::Unrecognized(_))));
    }

    #[test]
    fn an_unknown_command_is_echoed_by_its_first_word_only() {
        speak(i18n::Locale::En);
        let said = unknown_command_lines("/models bogus --token hunter2");
        assert!(said.contains("/models") && !said.contains("bogus") && !said.contains("hunter2"), "{said}");
        speak(i18n::Locale::PtBr);
        let said_pt = unknown_command_lines("/models bogus --token hunter2");
        assert!(said_pt.contains("/models") && !said_pt.contains("hunter2"), "{said_pt}");
    }

    #[test]
    fn a_cancelled_rotation_says_the_phrase_that_was_shown_is_void_but_a_resume_has_none_to_void() {
        speak(i18n::Locale::En);
        let rotate = cancelled_lines(false);
        assert_eq!(rotate.len(), 2);
        assert_eq!(rotate[0], tr!("arca.cancelled"));
        assert!(rotate[1].contains("discarded"), "{rotate:?}");
        assert_eq!(cancelled_lines(true), vec![tr!("arca.cancelled")], "a resume showed no phrase");
        speak(i18n::Locale::PtBr);
        assert!(cancelled_lines(false)[1].contains("descartada"));
    }

    #[test]
    fn the_phrase_screen_goes_on_only_while_the_terminal_is_the_size_it_was_laid_out_for() {
        assert!(phrase_screen_still_valid((80, 24), Some((80, 24))));
        for now in [Some((79, 24)), Some((80, 23)), Some((120, 40)), Some((0, 0)), None] {
            assert!(!phrase_screen_still_valid((80, 24), now), "{now:?}: a size that changed or cannot be read ends the screen");
        }
    }

    #[test]
    fn what_the_person_reads_after_a_resize_does_not_say_the_words_were_erased_and_says_to_clear_the_terminal() {
        speak(i18n::Locale::En);
        let resized = tr!("arca.rotate_resized");
        assert!(resized.contains("may still be visible") && resized.contains("clear the terminal") && resized.contains("Nothing was written"), "{resized}");
        assert!(!resized.contains("were cleared"), "{resized}");
        speak(i18n::Locale::PtBr);
        let resized_pt = tr!("arca.rotate_resized");
        assert!(resized_pt.contains("ainda estar visíveis") && resized_pt.contains("limpe o terminal"), "{resized_pt}");
        assert!(!resized_pt.contains("foram apagadas"), "{resized_pt}");
        // When the size cannot be read nothing is made, and the text says that, not "too small".
        speak(i18n::Locale::En);
        let unknown = tr!("arca.rotate_size_unknown");
        assert!(unknown.contains("size") && unknown.contains("no key was made") && !unknown.contains("too small"), "{unknown}");
        speak(i18n::Locale::PtBr);
        assert!(tr!("arca.rotate_size_unknown").contains("tamanho"));
    }

    /// The person, from a script: what each field answers, in order; `None` is Ctrl+D. It remembers the titles it was asked.
    struct ScriptedAsker {
        answers: std::collections::VecDeque<Option<String>>,
        asked: Vec<String>,
    }

    impl ScriptedAsker {
        fn new(answers: Vec<Option<&str>>) -> Self {
            Self { answers: answers.into_iter().map(|answer| answer.map(str::to_string)).collect(), asked: Vec::new() }
        }
        /// The words of `pending` that are asked back, then `then`.
        fn with_words(pending: &PendingRotation, then: Vec<Option<&str>>) -> Self {
            let mut asker = Self::new(then);
            for word in words_asked(pending).into_iter().rev() {
                asker.answers.push_front(Some(word));
            }
            asker
        }
    }

    impl FieldAsker for ScriptedAsker {
        async fn ask(&mut self, title: &str) -> anyhow::Result<Option<String>> {
            self.asked.push(title.to_string());
            Ok(self.answers.pop_front().expect("the command asked more than the script has"))
        }
    }

    #[tokio::test]
    async fn the_declaration_is_asked_first_and_the_confirmation_only_after_a_yes() {
        speak(i18n::Locale::En);
        let mut asker = ScriptedAsker::new(vec![Some("n")]);
        assert_eq!(rotation_answers_from(&mut asker).await.unwrap(), Some(("n".to_string(), None)));
        assert_eq!(asker.asked.len(), 1, "no \"change it now?\" for someone who did not revoke the token");
        assert!(asker.asked[0].contains("revoke"), "{:?}", asker.asked);

        let mut asker = ScriptedAsker::new(vec![Some("s"), Some("sim")]);
        assert_eq!(rotation_answers_from(&mut asker).await.unwrap(), Some(("s".to_string(), Some("sim".to_string()))));
        assert_eq!(asker.asked.len(), 2);
        assert!(asker.asked[0].contains("revoke") && asker.asked[1].contains("change this vault's key"), "the declaration comes first: {:?}", asker.asked);

        // Ctrl+D on the first question gives up; on the second it is a confirmation that never came.
        let mut asker = ScriptedAsker::new(vec![None]);
        assert_eq!(rotation_answers_from(&mut asker).await.unwrap(), None);
        assert_eq!(asker.asked.len(), 1);
        let mut asker = ScriptedAsker::new(vec![Some("s"), None]);
        assert_eq!(rotation_answers_from(&mut asker).await.unwrap(), Some(("s".to_string(), None)));
        // An empty answer is not a yes.
        let mut asker = ScriptedAsker::new(vec![Some("")]);
        assert_eq!(rotation_answers_from(&mut asker).await.unwrap(), Some((String::new(), None)));
        assert_eq!(asker.asked.len(), 1);
    }

    #[tokio::test]
    async fn a_blank_word_is_asked_again_and_only_giving_up_stops_the_questions() {
        speak(i18n::Locale::En);
        // A stray Enter must not throw away the phrase the person just wrote down.
        let mut asker = ScriptedAsker::new(vec![Some(""), Some("   "), Some("abandon"), Some("ability"), Some("able")]);
        let typed = typed_words_from(&mut asker, &[3, 11, 20]).await.unwrap();
        assert_eq!(typed, Some(vec!["abandon".to_string(), "ability".to_string(), "able".to_string()]));
        assert_eq!(asker.asked.len(), 5);
        assert!(asker.asked[..3].iter().all(|title| title.contains("word 3 ")), "the same word is asked again: {:?}", asker.asked);
        assert!(asker.asked[3].contains("word 11 ") && asker.asked[4].contains("word 20 "), "{:?}", asker.asked);
        // Ctrl+D gives up at once.
        let mut asker = ScriptedAsker::new(vec![Some("abandon"), None]);
        assert_eq!(typed_words_from(&mut asker, &[3, 11, 20]).await.unwrap(), None);
        assert_eq!(asker.asked.len(), 2, "nothing is asked after giving up");
        // A resume asks for no word.
        let mut asker = ScriptedAsker::new(Vec::new());
        assert_eq!(typed_words_from(&mut asker, &[]).await.unwrap(), Some(Vec::new()));
        assert!(asker.asked.is_empty());
    }

    #[tokio::test]
    async fn what_the_person_answers_decides_the_rotation_and_nothing_is_decided_for_them() {
        let device = device_in_arca_mode("rotate-questions").await;
        let before = secrets_of(&device);

        // No declaration: the core refuses, the confirmation is never asked, nothing is written.
        let pending = PendingRotation::generate().unwrap();
        let mut asker = ScriptedAsker::with_words(&pending, vec![Some("n")]);
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Refused(RotationError::NotDeclared));
        assert_eq!(asker.asked.len(), 4, "three words and the declaration");
        assert_key_untouched(&device, &before, "no declaration");

        // Declared but not confirmed, and Ctrl+D on the confirmation.
        for confirmation in [Some("n"), None] {
            let pending = PendingRotation::generate().unwrap();
            let mut asker = ScriptedAsker::with_words(&pending, vec![Some("s"), confirmation]);
            assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Cancelled, "{confirmation:?}");
            assert_key_untouched(&device, &before, "not confirmed");
        }

        // Giving up on the second word stops everything: the declaration is never asked.
        let pending = PendingRotation::generate().unwrap();
        let first = words_asked(&pending).remove(0);
        let mut asker = ScriptedAsker::new(vec![Some(first.as_str()), None]);
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Cancelled);
        assert_eq!(asker.asked.len(), 2);
        assert_key_untouched(&device, &before, "gave up on a word");

        // A wrong word, even with both yes.
        let pending = PendingRotation::generate().unwrap();
        let mut asker = ScriptedAsker::with_words(&pending, vec![Some("s"), Some("s")]);
        asker.answers[0].as_mut().unwrap().push('x');
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Refused(RotationError::WordMismatch));
        assert_key_untouched(&device, &before, "wrong word");

        // Everything right: the key changes, once.
        let pending = PendingRotation::generate().unwrap();
        let mut asker = ScriptedAsker::with_words(&pending, vec![Some("s"), Some("sim")]);
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Done);
        assert_eq!(secrets_of(&device).key_epoch, 1);
    }

    #[tokio::test]
    async fn a_resume_asks_the_two_questions_and_no_word_and_saves_the_key_of_the_24_words() {
        let device = device_in_arca_mode("rotate-resume-questions").await;
        let before = secrets_of(&device);
        let mut asker = ScriptedAsker::new(vec![Some("n")]);
        let pending = PendingRotation::from_phrase(ZOO_PHRASE).unwrap();
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 8).await.unwrap(), ArcaRotate::Refused(RotationError::NotDeclared));
        assert_eq!(asker.asked.len(), 1, "the declaration is the first thing a resume asks");
        assert_key_untouched(&device, &before, "resume without a declaration");
        let mut asker = ScriptedAsker::new(vec![Some("s"), Some("s")]);
        let pending = PendingRotation::from_phrase(ZOO_PHRASE).unwrap();
        assert_eq!(ask_and_rotate(&device.engine, &mut asker, pending, 9).await.unwrap(), ArcaRotate::Done);
        assert_eq!(asker.asked.len(), 2);
        assert_eq!(secrets_of(&device).vault_key, [0xff; 32]);
    }

    // ---------- `/arca on` ----------

    use warden_sync::manifest::{save_manifest, SyncManifest};
    use warden_sync::{ArweaveClient, SyncEngine};

    struct Device {
        base: PathBuf,
        engine: SyncEngine,
    }

    fn arca_device(name: &str, with_key: bool, paired_on_arweave: bool) -> Device {
        let base = std::env::temp_dir().join(format!(
            "warden-cli-arca-on-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(base.join("vault")).unwrap();
        // A gateway nobody listens on: a Pull that reaches for it fails at once instead of going to the real network.
        let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
        let engine =
            SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json")).with_arweave_client(dead);
        if with_key {
            engine.init_fresh().unwrap();
        }
        if paired_on_arweave {
            let manifest = SyncManifest { version: 1, owner_address: Some("some-owner".into()), ..Default::default() };
            save_manifest(&base.join("sync_manifest.json"), &manifest).unwrap();
        }
        Device { base, engine }
    }

    fn arca_is_on(device: &Device) -> bool {
        device.engine.arca_mode().unwrap().is_some()
    }

    #[test]
    fn only_an_explicit_yes_confirms() {
        for yes in ["s", "sim", "y", "yes", "SIM", "Yes", "  s  "] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", " ", "n", "nao", "não", "no", "ok", "sims", "1", "y e s"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }

    #[test]
    fn a_vault_already_in_arca_mode_or_without_a_key_has_nothing_to_ask() {
        assert_eq!(arca_on_blocker(true, true), Some(ArcaOn::AlreadyOn));
        assert_eq!(arca_on_blocker(false, true), Some(ArcaOn::AlreadyOn));
        assert_eq!(arca_on_blocker(false, false), Some(ArcaOn::NoKey));
        assert_eq!(arca_on_blocker(true, false), None);
    }

    #[tokio::test]
    async fn without_a_typed_yes_nothing_is_turned_on() {
        for answer in [None, Some(""), Some("n"), Some("nao"), Some("talvez")] {
            let device = arca_device("no-yes", true, false);
            assert_eq!(arca_on_run(&device.engine, answer, 1).await, ArcaOn::Cancelled, "{answer:?}");
            assert!(!arca_is_on(&device), "{answer:?}: the switch must not be written");
        }
    }

    #[tokio::test]
    async fn a_typed_yes_turns_the_mode_on_for_a_vault_that_never_synced_through_arweave() {
        let device = arca_device("yes-fresh", true, false);
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        assert_eq!(arca_on_run(&device.engine, Some("sim"), 7).await, ArcaOn::Done { needs_reload: false });
        let mode = device.engine.arca_mode().unwrap().expect("the switch is on");
        assert_eq!((mode.enabled_at_ms, mode.secret_accepted), (7, false));
    }

    #[tokio::test]
    async fn a_typed_yes_does_not_turn_it_on_when_the_pull_before_it_fails() {
        let device = arca_device("yes-pull-fails", true, true);
        match arca_on_run(&device.engine, Some("s"), 1).await {
            ArcaOn::Failed(message) => assert!(message.contains("Pull") && message.contains("not turned on"), "{message}"),
            other => panic!("expected the Pull failure, got {other:?}"),
        }
        assert!(!arca_is_on(&device), "the first arca manifest must not be made from a vault that may be behind");
    }

    #[test]
    fn the_confirmation_says_there_is_no_way_back_and_what_stops_working() {
        speak(i18n::Locale::En);
        let en = tr!("arca.on_confirm");
        assert!(en.contains("no way back") && en.contains("/sync push") && en.contains("Pull") && en.contains("(y/n)"), "{en}");
        speak(i18n::Locale::PtBr);
        let pt = tr!("arca.on_confirm");
        assert!(pt.contains("sem volta") && pt.contains("/sync push") && pt.contains("Pull") && pt.contains("(s/n)"), "{pt}");
    }

    #[test]
    fn the_cards_say_what_happened_in_english_and_portuguese() {
        speak(i18n::Locale::En);
        let done = arca_on_lines(&ArcaOn::Done { needs_reload: false }).join("\n");
        assert!(done.contains("arca mode: on") && done.contains("/arca sync"), "{done}");
        assert!(!done.contains("reload"), "{done}");
        let reloaded = arca_on_lines(&ArcaOn::Done { needs_reload: true }).join("\n");
        assert!(reloaded.contains("reload"), "{reloaded}");
        assert_eq!(arca_on_lines(&ArcaOn::Cancelled), ["cancelled"]);
        assert!(arca_on_lines(&ArcaOn::AlreadyOn).join(" ").contains("already on"));
        assert!(arca_on_lines(&ArcaOn::NoKey).join(" ").contains("no vault key"));
        assert_eq!(arca_on_lines(&ArcaOn::Failed("boom".into())), ["boom"]);
        speak(i18n::Locale::PtBr);
        assert!(arca_on_lines(&ArcaOn::Done { needs_reload: false }).join("\n").contains("modo arca: ligado"));
        assert!(arca_on_lines(&ArcaOn::AlreadyOn).join(" ").contains("já está ligado"));
        assert_eq!(arca_on_lines(&ArcaOn::Cancelled), ["cancelado"]);
    }

    #[test]
    fn the_sync_that_needs_the_mode_points_to_arca_on() {
        speak(i18n::Locale::En);
        assert!(tr!("arca.sync_needs_mode").contains("/arca on"));
        speak(i18n::Locale::PtBr);
        assert!(tr!("arca.sync_needs_mode").contains("/arca on"));
    }

    #[test]
    fn a_cycle_that_changed_nothing_else_is_one_plain_line_with_the_counts() {
        speak(i18n::Locale::En);
        let report = ArcaSyncReport { pulled: 2, pushed: 1, files_changed: 3, ..Default::default() };
        assert_eq!(arca_report_lines(&report), ["cycle done: 2 received, 1 sent, 3 file(s) changed here"]);
        speak(i18n::Locale::PtBr);
        assert_eq!(arca_report_lines(&report), ["ciclo concluído: 2 recebido(s), 1 enviado(s), 3 arquivo(s) alterado(s) aqui"]);
    }

    /// What the cycle left out or set aside on purpose must reach the person: a quiet "done" would hide that nothing was sent.
    #[test]
    fn everything_the_cycle_left_out_or_set_aside_is_said() {
        speak(i18n::Locale::En);
        let report = ArcaSyncReport {
            merges: 1,
            conflicts: 2,
            secret_losers_saved: 3,
            secret_ignored: 4,
            secret_skipped: vec![SkippedLogin { path: "github.bak".into(), reason: "not accepted by this device" }],
            recovered_kept: 5,
            secret_folder_missing: true,
            vault_emptied: true,
            secret_emptied: true,
            ..Default::default()
        };
        let text = arca_report_lines(&report).join("\n");
        for needle in ["2 conflict(s)", "3 login conflict loser(s)", "4 login(s) from other devices", "github.bak", "5 file(s) left aside", "every vault file is gone", "no login left to send", "logins folder is gone"] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }
}
