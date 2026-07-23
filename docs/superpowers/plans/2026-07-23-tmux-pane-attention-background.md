# Tmux Pane Attention Background Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every tmux pane with unseen agent attention the Sidebar completed-card green background, then restore its original pane styles when that attention clears.

**Architecture:** Keep `@pane_attention` as the only attention source of truth. Add tmux option helpers that save and restore explicit pane styles, reconcile highlighted panes from the existing Sidebar refresh snapshot, and use the current compare-and-clear path for immediate focus restoration. Extend the single `list-panes` snapshot with one internal backup marker so a restarted Sidebar can repair a stale style without adding per-pane polling commands.

**Tech Stack:** Rust 2024, tmux 3.4 pane options, Ratatui theme options, existing thread-local tmux test mock, Cargo test/fmt/clippy.

## Global Constraints

- Both `ActionRequired` and `Completed` use the actual-pane background derived from `@sidebar_color_attention_completed_bg`, defaulting to `colour22`.
- Sidebar card backgrounds and icons remain unchanged.
- Preserve and restore explicit `window-style` and `window-active-style` pane options.
- Never alter pane borders or global tmux styles.
- A terminal application may cover tmux's default background with explicitly painted cells; do not rewrite pane output.
- Work in the current checkout, preserve `.idea/`, create local commits only, and do not push.
- Run `cargo fmt` before every commit and run `cargo build --release` after implementation.

---

### Task 1: Pane Style Preservation Helpers

**Files:**
- Modify: `src/tmux/options.rs`
- Modify: `src/tmux.rs`

**Interfaces:**
- Consumes: `get_option`, `get_pane_option_value`, `set_pane_option`, `unset_pane_option`, and `SIDEBAR_COLOR_ATTENTION_COMPLETED_BG`.
- Produces:
  - `PANE_ATTENTION_PREV_WINDOW_STYLE: &str`
  - `PANE_ATTENTION_PREV_WINDOW_ACTIVE_STYLE: &str`
  - `pane_attention_background() -> String`
  - `apply_pane_attention_style(pane: &str, background: &str)`
  - `restore_pane_attention_style(pane: &str)`
  - `clear_pane_option_if_value(...)` restoring styles after a successful attention clear.

- [ ] **Step 1: Add failing color-normalization and style-lifecycle tests**

Add focused tests to `src/tmux/options.rs`:

```rust
#[test]
fn pane_attention_background_normalizes_theme_values() {
    assert_eq!(normalize_pane_attention_color(Some("22")), "colour22");
    assert_eq!(
        normalize_pane_attention_color(Some("#005f00")),
        "#005f00"
    );
    assert_eq!(normalize_pane_attention_color(Some("005F00")), "#005F00");
    assert_eq!(normalize_pane_attention_color(Some("invalid")), "colour22");
    assert_eq!(normalize_pane_attention_color(None), "colour22");
}

#[test]
fn attention_style_restores_inherited_pane_styles() {
    let _guard = test_mock::install();
    test_mock::set_global("window-style", "fg=default,bg=#1f2f38");
    test_mock::set_global("window-active-style", "fg=default,bg=terminal");

    apply_pane_attention_style("%1", "colour22");

    assert_eq!(
        test_mock::get("%1", "window-style").as_deref(),
        Some("fg=default,bg=#1f2f38,bg=colour22")
    );
    assert_eq!(
        test_mock::get("%1", "window-active-style").as_deref(),
        Some("fg=default,bg=terminal,bg=colour22")
    );

    restore_pane_attention_style("%1");

    assert!(!test_mock::contains("%1", "window-style"));
    assert!(!test_mock::contains("%1", "window-active-style"));
}

#[test]
fn attention_style_preserves_explicit_pane_styles_across_reapply() {
    let _guard = test_mock::install();
    test_mock::set("%1", "window-style", "fg=white,bg=blue");
    test_mock::set("%1", "window-active-style", "fg=yellow,bold");

    apply_pane_attention_style("%1", "colour22");
    apply_pane_attention_style("%1", "colour22");
    restore_pane_attention_style("%1");

    assert_eq!(
        test_mock::get("%1", "window-style").as_deref(),
        Some("fg=white,bg=blue")
    );
    assert_eq!(
        test_mock::get("%1", "window-active-style").as_deref(),
        Some("fg=yellow,bold")
    );
}

#[test]
fn clearing_matching_attention_restores_saved_styles() {
    let _guard = test_mock::install();
    test_mock::set("%1", PANE_ATTENTION, "completed:1-1");
    apply_pane_attention_style("%1", "colour22");

    assert!(clear_pane_option_if_value(
        "%1",
        PANE_ATTENTION,
        "completed:1-1"
    ));
    assert!(!test_mock::contains("%1", PANE_ATTENTION));
    assert!(!test_mock::contains(
        "%1",
        PANE_ATTENTION_PREV_WINDOW_STYLE
    ));
    assert!(!test_mock::contains("%1", "window-style"));
}
```

Extend the test mock with global values:

```rust
pub fn set_global(key: &str, value: &str) {
    set(GLOBAL_SCOPE, key, value);
}

pub(super) fn intercept_get_global(key: &str) -> Option<String> {
    intercept_get(GLOBAL_SCOPE, key)
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cargo test pane_attention_background_normalizes_theme_values -- --nocapture
cargo test attention_style_ -- --nocapture
cargo test clearing_matching_attention_restores_saved_styles -- --nocapture
```

Expected: compilation fails because the new constants, mock helper, and style functions do not exist.

- [ ] **Step 3: Implement normalization, backup, apply, and restore**

Add internal constants and helpers in `src/tmux/options.rs`:

```rust
pub const PANE_ATTENTION_PREV_WINDOW_STYLE: &str =
    "@pane_attention_prev_window_style";
pub const PANE_ATTENTION_PREV_WINDOW_ACTIVE_STYLE: &str =
    "@pane_attention_prev_window_active_style";

const WINDOW_STYLE: &str = "window-style";
const WINDOW_ACTIVE_STYLE: &str = "window-active-style";
const UNSET_STYLE: &str = "__tmux_agent_sidebar_unset__";

fn normalize_pane_attention_color(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return "colour22".to_string();
    };
    if let Ok(index) = value.parse::<u8>() {
        return format!("colour{index}");
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return format!("#{hex}");
    }
    "colour22".to_string()
}

pub fn pane_attention_background() -> String {
    let configured = get_option(SIDEBAR_COLOR_ATTENTION_COMPLETED_BG);
    normalize_pane_attention_color(configured.as_deref())
}

fn highlighted_style(base: &str, background: &str) -> String {
    if base.is_empty() {
        format!("bg={background}")
    } else {
        format!("{base},bg={background}")
    }
}

fn apply_saved_style(
    pane: &str,
    option: &str,
    backup_option: &str,
    background: &str,
) {
    let backup = get_pane_option_value(pane, backup_option);
    let original = if backup.is_empty() {
        let explicit = get_pane_option_value(pane, option);
        let saved = if explicit.is_empty() {
            UNSET_STYLE
        } else {
            explicit.as_str()
        };
        set_pane_option(pane, backup_option, saved);
        explicit
    } else if backup == UNSET_STYLE {
        String::new()
    } else {
        backup
    };
    let base = if original.is_empty() {
        get_option(option).unwrap_or_default()
    } else {
        original
    };
    set_pane_option(pane, option, &highlighted_style(&base, background));
}

pub fn apply_pane_attention_style(pane: &str, background: &str) {
    apply_saved_style(
        pane,
        WINDOW_STYLE,
        PANE_ATTENTION_PREV_WINDOW_STYLE,
        background,
    );
    apply_saved_style(
        pane,
        WINDOW_ACTIVE_STYLE,
        PANE_ATTENTION_PREV_WINDOW_ACTIVE_STYLE,
        background,
    );
}

fn restore_saved_style(pane: &str, option: &str, backup_option: &str) {
    let backup = get_pane_option_value(pane, backup_option);
    if backup.is_empty() {
        return;
    }
    if backup == UNSET_STYLE {
        unset_pane_option(pane, option);
    } else {
        set_pane_option(pane, option, &backup);
    }
    unset_pane_option(pane, backup_option);
}

pub fn restore_pane_attention_style(pane: &str) {
    restore_saved_style(
        pane,
        WINDOW_STYLE,
        PANE_ATTENTION_PREV_WINDOW_STYLE,
    );
    restore_saved_style(
        pane,
        WINDOW_ACTIVE_STYLE,
        PANE_ATTENTION_PREV_WINDOW_ACTIVE_STYLE,
    );
}
```

Make `get_option` use `test_mock::intercept_get_global` when a test mock is
installed. Update `unset_pane_option` so clearing `PANE_ATTENTION` first calls
`restore_pane_attention_style`. Update both the mock and production branches
of `clear_pane_option_if_value` so successful matching clears also restore.

Re-export the public constants and functions from `src/tmux.rs`.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run:

```bash
cargo test pane_attention_background_normalizes_theme_values -- --nocapture
cargo test attention_style_ -- --nocapture
cargo test clear_pane_option_if_value -- --nocapture
```

Expected: all selected tests pass.

- [ ] **Step 5: Format, inspect, and commit Task 1**

Run:

```bash
cargo fmt
cargo fmt --check
git diff --check
git diff -- src/tmux/options.rs src/tmux.rs
git add src/tmux/options.rs src/tmux.rs
git commit -m "feat: preserve tmux pane attention styles"
```

Expected: format and diff checks pass; the commit contains only the two tmux
files.

---

### Task 2: Refresh Reconciliation and Stale-Style Repair

**Files:**
- Modify: `src/state/refresh.rs`
- Modify: `src/tmux/query.rs`
- Modify: `docs/state-management.md`

**Interfaces:**
- Consumes:
  - `tmux::pane_attention_background() -> String`
  - `tmux::apply_pane_attention_style(pane: &str, background: &str)`
  - `tmux::restore_pane_attention_style(pane: &str)`
  - `tmux::PANE_ATTENTION_PREV_WINDOW_STYLE`
- Produces:
  - `AppState::reconcile_pane_attention_styles(&self)`
  - one additional trailing `list-panes` field used only to detect stale style backups.

- [ ] **Step 1: Add failing refresh and stale-repair tests**

Add a refresh-module test:

```rust
#[test]
fn reconcile_pane_attention_styles_applies_completed_green() {
    let _guard = tmux::test_mock::install();
    let mut pane = test_pane("%ATTENTION");
    pane.attention = tmux::PaneAttention::parse("action_required:1-1");
    let state = state_with_panes(vec![pane]);

    state.reconcile_pane_attention_styles();

    assert_eq!(
        tmux::test_mock::get("%ATTENTION", "window-style").as_deref(),
        Some("bg=colour22")
    );
    assert_eq!(
        tmux::test_mock::get("%ATTENTION", "window-active-style").as_deref(),
        Some("bg=colour22")
    );
}
```

Add a query-module test that appends a non-empty
`PANE_ATTENTION_PREV_WINDOW_STYLE` field while leaving `PANE_ATTENTION` empty,
preloads both backup options and temporary green styles in `test_mock`, parses
the line, and asserts that both styles and both backups are restored/removed.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test reconcile_pane_attention_styles_applies_completed_green -- --nocapture
cargo test parse_pane_line_restores_stale_attention_style -- --nocapture
```

Expected: compilation or assertion failure because refresh reconciliation and
the trailing style marker do not exist.

- [ ] **Step 3: Add refresh reconciliation**

In `src/state/refresh.rs`, add:

```rust
fn reconcile_pane_attention_styles(&self) {
    let background = tmux::pane_attention_background();
    for pane in self
        .repo_groups
        .iter()
        .flat_map(|group| group.panes.iter().map(|(pane, _)| pane))
        .filter(|pane| pane.attention.is_some())
    {
        tmux::apply_pane_attention_style(&pane.pane_id, &background);
    }
}
```

Call it in `refresh()` immediately after `apply_session_snapshot(...)`. Focus
acknowledgement has already updated the in-memory pane, so the focused pane is
not re-highlighted.

- [ ] **Step 4: Add stale-style detection to the existing pane snapshot**

Append `q(PANE_ATTENTION_PREV_WINDOW_STYLE)` to `pane_format()` and define its
trailing pane-line index. In `parse_pane_line`, parse attention once and:

```rust
let attention = PaneAttention::parse(&parts[pane_line_field::PANE_ATTENTION]);
let has_saved_attention_style = parts
    .get(pane_line_field::ATTENTION_PREV_WINDOW_STYLE)
    .is_some_and(|value| !value.is_empty());
if attention.is_none() && has_saved_attention_style {
    super::options::restore_pane_attention_style(
        &parts[pane_line_field::PANE_ID],
    );
}
```

Use `attention` when constructing `PaneInfo`. The field is optional for
backward-compatible parser tests; do not raise `MIN_FIELDS`.

Because every teardown path already clears `PANE_ATTENTION` through
`unset_pane_option`, the Task 1 restoration hook covers SessionEnd, dead-agent
cleanup, and shell fallback without duplicating style keys in teardown arrays.

- [ ] **Step 5: Run focused and module tests**

Run:

```bash
cargo test reconcile_pane_attention_styles -- --nocapture
cargo test parse_pane_line_restores_stale_attention_style -- --nocapture
cargo test pane_attention -- --nocapture
cargo test state::refresh -- --nocapture
```

Expected: all selected tests pass.

- [ ] **Step 6: Document the pane-style lifecycle**

Update `docs/state-management.md`:

- describe the actual-pane `colour22` highlight next to the two Sidebar card
  theme values;
- add the two internal backup options to the per-pane option table;
- explain refresh reconciliation, focus restoration, teardown restoration, and
  the explicit-background terminal limitation.

- [ ] **Step 7: Run full verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo test
cargo clippy
cargo build --release
git diff --check
```

Expected:

- formatting passes;
- every Rust test passes;
- clippy introduces no new warnings;
- the release binary builds successfully; and
- the diff has no whitespace errors.

- [ ] **Step 8: Inspect and commit Task 2**

Run:

```bash
git diff -- src/state/refresh.rs src/tmux/query.rs docs/state-management.md
git status --short
git add src/state/refresh.rs src/tmux/query.rs docs/state-management.md
git commit -m "feat: highlight tmux panes needing attention"
```

Expected: only Task 2 files are staged; `.idea/` remains untracked; the branch
is not pushed.

---

### Task 3: Final Regression Audit

**Files:**
- Verify only; modify a Task 1 or Task 2 file only if a regression is found.

**Interfaces:**
- Consumes: the completed pane-style helper and refresh reconciliation.
- Produces: verified local commits and a release binary in `target/release/`.

- [ ] **Step 1: Audit the accepted behavior against the design**

Confirm from tests and code that:

```text
ActionRequired -> actual pane colour22
Completed      -> actual pane colour22
focus clear    -> prior pane styles restored
activity clear -> prior pane styles restored
teardown       -> prior pane styles restored
Sidebar card   -> existing semantic colors unchanged
pane borders   -> unchanged
global styles  -> unchanged
```

- [ ] **Step 2: Verify the final history and worktree**

Run:

```bash
git log -4 --oneline
git status --short --branch
git diff HEAD --check
```

Expected: the design, plan, helper, and integration commits are present;
`.idea/` is the only unrelated untracked path; there are no unstaged task
changes and no push was performed.
