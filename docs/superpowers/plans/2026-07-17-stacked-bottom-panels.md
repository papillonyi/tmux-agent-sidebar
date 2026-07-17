# Stacked Bottom Panels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render Git above Activity in two equally sized cards inside the existing bottom-panel height while retaining an explicit active scroll target.

**Architecture:** `ui::bottom` owns a deterministic vertical split and renders both existing content modules into independently framed rectangles. The current tab selection becomes an active-panel selection used only for scroll routing, title clicks, focus styling, and per-pane preference restoration. Git polling is enabled whenever the configured bottom region is nonzero instead of depending on the active panel.

**Tech Stack:** Rust 2024, Ratatui, Crossterm, inline `insta` snapshots, tmux integration.

## Global Constraints

- Git is the upper card and Activity is the lower card.
- The cards share the existing `bottom_panel_height`; the default `20` gives each card `10` rows including borders.
- For odd heights, Activity receives the extra row.
- `@sidebar_bottom_height=0` hides the complete bottom region and does not start the periodic Git poller.
- `Shift+Tab`, title clicks, keyboard navigation, and mouse-wheel scrolling operate on an active panel without hiding either card.
- Agent panes default to Activity; non-agent panes default to Git; per-pane selection restoration remains intact.
- The Git polling interval remains two seconds and the PR cache policy is unchanged.
- Every rendered-frame assertion uses an inline `insta::assert_snapshot!` snapshot.
- Before every commit, run `cargo fmt` and stage only files belonging to that task.
- Do not stage or modify the existing untracked `.idea/` directory.

---

### Task 1: Render two equally sized bottom cards

**Files:**
- Modify: `src/ui/bottom.rs`
- Test: `src/ui/bottom.rs`

**Interfaces:**
- Produces: `split_bottom_areas(area: Rect) -> (Rect, Rect)`, returning the Git and Activity card rectangles in that order.
- Produces: a private card-frame helper that returns `Option<Rect>` for the content area and skips zero-width or zero-height rectangles.
- Preserves: `activity::draw_activity_content(frame, state, inner)` and `git::draw_git_content(frame, state, inner)`.

- [ ] **Step 1: Add failing layout and rendering tests**

Add unit tests in `src/ui/bottom.rs` for even, odd, and tiny heights:

```rust
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn split_bottom_areas_even_height() {
    let area = Rect::new(3, 5, 28, 20);
    let (git, activity) = split_bottom_areas(area);
    assert_eq!(git, Rect::new(3, 5, 28, 10));
    assert_eq!(activity, Rect::new(3, 15, 28, 10));
}

#[test]
fn split_bottom_areas_odd_height_gives_extra_row_to_activity() {
    let area = Rect::new(3, 5, 28, 9);
    let (git, activity) = split_bottom_areas(area);
    assert_eq!(git, Rect::new(3, 5, 28, 4));
    assert_eq!(activity, Rect::new(3, 9, 28, 5));
}

#[test]
fn split_bottom_areas_one_row_skips_git() {
    let area = Rect::new(0, 0, 28, 1);
    let (git, activity) = split_bottom_areas(area);
    assert_eq!(git.height, 0);
    assert_eq!(activity, Rect::new(0, 0, 28, 1));
}
```

Add a direct renderer that preserves every row, including blank bordered rows:

```rust
fn render_bottom(state: &mut AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_bottom(frame, state, Rect::new(0, 0, width, height)))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

Use it for an inline snapshot proving the two empty cards occupy ten rows each:

```rust
#[test]
fn renders_git_above_activity_in_equal_cards() {
    let mut state = AppState::new("%99".into());
    insta::assert_snapshot!(render_bottom(&mut state, 28, 20), @r"
    ╭ Git ─────────────────────╮
    │                          │
    │                          │
    │                          │
    │    Working tree clean    │
    │                          │
    │                          │
    │                          │
    │                          │
    ╰──────────────────────────╯
    ╭ Activity ────────────────╮
    │                          │
    │                          │
    │                          │
    │     No activity yet      │
    │                          │
    │                          │
    │                          │
    │                          │
    ╰──────────────────────────╯
    ");
}
```

Add a second direct snapshot with populated Git and Activity state:

```rust
#[test]
fn renders_populated_git_and_activity_together() {
    let mut state = AppState::new("%99".into());
    state.git.branch = "main".into();
    state.git.diff_stat = Some((3, 1));
    state.git.unstaged_files = vec![crate::git::GitFileEntry {
        status: 'M',
        name: "src/lib.rs".into(),
        additions: 3,
        deletions: 1,
        path: String::new(),
    }];
    state.activity.entries = vec![crate::activity::ActivityEntry {
        timestamp: "10:00".into(),
        tool: "Read".into(),
        label: "src/lib.rs".into(),
    }];

    insta::assert_snapshot!(render_bottom(&mut state, 28, 20), @r"
    ╭ Git ─────────────────────╮
    │                          │
    │main                      │
    │+3/-1              1 files│
    │──────────────────────────│
    │Unstaged (1)              │
    │M src/lib.rs         +3/-1│
    │                          │
    │                          │
    ╰──────────────────────────╯
    ╭ Activity ────────────────╮
    │                          │
    │10:00                 Read│
    │  src/lib.rs              │
    │                          │
    │                          │
    │                          │
    │                          │
    │                          │
    ╰──────────────────────────╯
    ");
}
```

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```bash
cargo test ui::bottom::tests::split_bottom_areas_even_height
cargo test ui::bottom::tests::renders_git_above_activity_in_equal_cards
cargo test ui::bottom::tests::renders_populated_git_and_activity_together
```

Expected: the split test fails to compile because `split_bottom_areas` does not exist, and both snapshots fail because the current renderer emits one `Activity │ Git` tab card.

- [ ] **Step 3: Implement deterministic splitting and two card frames**

In `src/ui/bottom.rs`, add the split and generalized frame helpers:

```rust
fn split_bottom_areas(area: Rect) -> (Rect, Rect) {
    let git_height = area.height / 2;
    let activity_height = area.height - git_height;
    (
        Rect::new(area.x, area.y, area.width, git_height),
        Rect::new(
            area.x,
            area.y.saturating_add(git_height),
            area.width,
            activity_height,
        ),
    )
}

fn draw_card_frame(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    title: &str,
    selected: bool,
) -> Option<Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let border_color = if selected && state.focus_state.focus == Focus::ActivityLog {
        state.theme.accent
    } else {
        state.theme.border_inactive
    };
    let title_color = if selected {
        state.theme.accent
    } else {
        state.theme.text_muted
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title_width = display_width(title);
    let fill_width = (area.width as usize).saturating_sub(title_width + 4);
    let top_line = Line::from(vec![
        Span::styled("╭ ", Style::default().fg(border_color)),
        Span::styled(title.to_string(), Style::default().fg(title_color)),
        Span::styled(
            format!(" {}╮", "─".repeat(fill_width)),
            Style::default().fg(border_color),
        ),
    ]);
    frame.render_widget(Paragraph::new(top_line), Rect::new(area.x, area.y, area.width, 1));

    let bottom_line = Line::from(Span::styled(
        format!("╰{}╯", "─".repeat((area.width as usize).saturating_sub(2))),
        Style::default().fg(border_color),
    ));
    frame.render_widget(
        Paragraph::new(bottom_line),
        Rect::new(
            area.x,
            area.y.saturating_add(area.height.saturating_sub(1)),
            area.width,
            1,
        ),
    );

    (inner.width > 0 && inner.height > 0).then_some(inner)
}
```

Replace the `match state.bottom_tab` render branch with unconditional rendering:

```rust
let (git_area, activity_area) = split_bottom_areas(area);
if let Some(inner) = draw_card_frame(
    frame,
    state,
    git_area,
    "Git",
    state.bottom_tab == BottomTab::GitStatus,
) {
    git::draw_git_content(frame, state, inner);
}
if let Some(inner) = draw_card_frame(
    frame,
    state,
    activity_area,
    "Activity",
    state.bottom_tab == BottomTab::Activity,
) {
    activity::draw_activity_content(frame, state, inner);
}
```

Delete `build_tab_title`. Keep the current rounded-border characters and title spacing so the two title rows are exactly `╭ Git ... ╮` and `╭ Activity ... ╮`.

- [ ] **Step 4: Run focused tests and make them GREEN**

Run:

```bash
cargo test ui::bottom::tests
```

Expected: PASS. If an expected snapshot differs only because Ratatui center alignment assigns the odd horizontal padding differently, inspect the full buffer and update the inline snapshot to the actual centered result.

- [ ] **Step 5: Reconcile existing full-UI snapshots and run the full suite**

Run:

```bash
cargo insta test --accept --test bottom_tests
cargo insta test --accept --test styled_tests
cargo insta test --accept --test color_tests
cargo insta test --accept --test ui_snapshot
git diff -- tests
cargo test
```

Expected: all visual changes show two stacked cards, every accepted assertion
remains an inline snapshot, and the complete test suite passes. Remove tests
whose only distinction was hiding one of the two tabs, but retain selected-Git
and selected-Activity style coverage.

- [ ] **Step 6: Commit the stacked renderer and reconciled snapshots**

```bash
cargo fmt
git add src/ui/bottom.rs tests/bottom_tests.rs tests/styled_tests.rs tests/color_tests.rs tests/ui_snapshot.rs
git commit -m "feat: stack git and activity panels"
```

---

### Task 2: Convert tab state into active-panel state and route input

**Files:**
- Modify: `src/state.rs`
- Modify: `src/state/tab.rs`
- Modify: `src/state/focus.rs`
- Modify: `src/state/pane_runtime.rs`
- Modify: `src/state/layout.rs`
- Modify: `src/state/refresh.rs`
- Modify: `src/app.rs`
- Modify: `src/app/input.rs`
- Modify: `src/app/workers.rs`
- Modify: `src/ui/bottom.rs`
- Modify: `tests/bottom_tests.rs`
- Modify: `tests/color_tests.rs`
- Modify: `tests/state_tests.rs`
- Modify: `tests/styled_tests.rs`
- Modify: `tests/ui_snapshot.rs`

**Interfaces:**
- Produces: `BottomPanel::{Activity, Git}`.
- Produces: `AppState::active_bottom_panel: BottomPanel`.
- Produces: `AppState::next_bottom_panel(&mut self)`.
- Produces: `AppState::handle_bottom_panel_title_click(&mut self, row: u16, col: u16, term_height: u16, bottom_panel_height: u16)`.
- Produces: `Focus::BottomPanel` and `PaneRuntimeState::bottom_panel_pref`.
- Preserves: `AppState::scroll_bottom(delta)` as the single scroll-dispatch entry point.

- [ ] **Step 1: Add failing active-panel click and navigation tests**

Add state tests using a 50-row terminal and 20-row bottom region:

```rust
#[test]
fn bottom_panel_title_click_selects_each_panel() {
    let mut state = AppState::new("%99".into());
    state.handle_bottom_panel_title_click(30, 2, 50, 20);
    assert_eq!(state.active_bottom_panel, BottomPanel::Git);

    state.handle_bottom_panel_title_click(40, 2, 50, 20);
    assert_eq!(state.active_bottom_panel, BottomPanel::Activity);
}

#[test]
fn bottom_panel_title_click_ignores_content_rows_and_text_outside_title() {
    let mut state = AppState::new("%99".into());
    state.active_bottom_panel = BottomPanel::Activity;
    state.handle_bottom_panel_title_click(31, 2, 50, 20);
    state.handle_bottom_panel_title_click(30, 20, 50, 20);
    state.handle_bottom_panel_title_click(30, 0, 50, 20);
    assert_eq!(state.active_bottom_panel, BottomPanel::Activity);
}
```

Add an `src/app/input.rs` test proving `Shift+Tab` switches the active target but leaves both sections renderable, and retain scroll-dispatch tests for both panel variants.

- [ ] **Step 2: Run the active-panel tests and verify RED**

Run:

```bash
cargo test bottom_panel_title_click_selects_each_panel
cargo test shift_tab_switches_active_bottom_panel
```

Expected: compilation fails because the panel-oriented type, field, and click API do not exist yet.

- [ ] **Step 3: Perform the scoped semantic rename**

Apply these exact renames across production and test Rust files:

```text
BottomTab                    -> BottomPanel
BottomTab::GitStatus         -> BottomPanel::Git
bottom_tab                   -> active_bottom_panel
Focus::ActivityLog           -> Focus::BottomPanel
tab_pref                     -> bottom_panel_pref
auto_switch_tab              -> auto_select_bottom_panel
next_bottom_tab              -> next_bottom_panel
handle_bottom_tab_click      -> handle_bottom_panel_title_click
save_current_tab             -> save_current_bottom_panel
resolve_bottom_tab           -> resolve_bottom_panel
resolve_tab_for_focused_pane -> resolve_panel_for_focused_pane
TabDecision                  -> PanelDecision
```

Keep the existing default-selection and per-pane restoration branches intact; only their terminology changes.

- [ ] **Step 4: Implement title-row hit testing and update input routing**

Use the same deterministic split as the renderer. The bottom region begins at
`term_height.saturating_sub(bottom_panel_height)`, the Git title is its first
row when the Git height is nonzero, and the Activity title is the first row of
the lower rectangle. Only the visible title text is clickable:

```rust
pub fn handle_bottom_panel_title_click(
    &mut self,
    row: u16,
    col: u16,
    term_height: u16,
    bottom_panel_height: u16,
) {
    let bottom_start = term_height.saturating_sub(bottom_panel_height);
    let git_height = bottom_panel_height / 2;
    let Some(title_col) = col.checked_sub(2).map(usize::from) else {
        return;
    };

    if git_height > 0 && row == bottom_start && title_col < "Git".len() {
        self.active_bottom_panel = BottomPanel::Git;
    } else if row == bottom_start.saturating_add(git_height)
        && title_col < "Activity".len()
    {
        self.active_bottom_panel = BottomPanel::Activity;
    }
}
```

Update the mouse-down path in `src/app/input.rs` to pass row, column, terminal
height, and configured bottom height for any row in the bottom region. Keep
mouse scrolling routed through `handle_mouse_scroll`, which in turn calls
`scroll_bottom`. Change the `BackTab` key path to call `next_bottom_panel()`.

- [ ] **Step 5: Run focused state and input tests and make them GREEN**

Run:

```bash
cargo test state::tab::tests
cargo test app::input::tests
cargo test --test state_tests
cargo test --test bottom_tests test_scroll_bottom_dispatches
```

Expected: PASS with active-panel defaults, restoration, keyboard selection,
title clicks, and scroll dispatch unchanged except for the new names.

- [ ] **Step 6: Commit panel semantics and input**

```bash
cargo fmt
git add src/state.rs src/state/tab.rs src/state/focus.rs src/state/pane_runtime.rs src/state/layout.rs src/state/refresh.rs src/app.rs src/app/input.rs src/app/workers.rs src/ui/bottom.rs tests/bottom_tests.rs tests/color_tests.rs tests/state_tests.rs tests/styled_tests.rs tests/ui_snapshot.rs
git commit -m "refactor: model the active bottom panel"
```

---

### Task 3: Poll visible Git state independently of panel selection

**Files:**
- Modify: `src/app.rs`
- Modify: `src/app/input.rs`
- Modify: `src/app/workers.rs`
- Test: `src/app/workers.rs`

**Interfaces:**
- Produces: `git_polling_enabled(bottom_panel_height: u16) -> bool`.
- Changes: `git_poll_loop(tmux_pane: &str, git_tx: &mpsc::Sender<GitData>)` no longer accepts an `AtomicBool`.
- Changes: `input::handle_event` and `input::handle_key_event` no longer accept or mutate a Git-tab activity flag.

- [ ] **Step 1: Replace flag-oriented worker tests with failing height tests**

Replace the simulated active/inactive flag tests in `src/app/workers.rs` with:

```rust
#[test]
fn git_polling_is_enabled_for_visible_bottom_region() {
    assert!(git_polling_enabled(20));
    assert!(git_polling_enabled(1));
}

#[test]
fn git_polling_is_disabled_when_bottom_region_is_hidden() {
    assert!(!git_polling_enabled(0));
}
```

- [ ] **Step 2: Run the worker tests and verify RED**

Run:

```bash
cargo test app::workers::tests::git_polling_is_enabled_for_visible_bottom_region
cargo test app::workers::tests::git_polling_is_disabled_when_bottom_region_is_hidden
```

Expected: compilation fails because `git_polling_enabled` does not exist.

- [ ] **Step 3: Remove the selection gate and conditionally spawn the worker**

Implement:

```rust
fn git_polling_enabled(bottom_panel_height: u16) -> bool {
    bottom_panel_height > 0
}
```

In `workers::spawn`, start the Git thread only when this function returns true.
Remove `git_tab_active` from `Workers`, remove the shared `AtomicBool`, and
delete the `active.load(...)` skip in `git_poll_loop`. Retain the two-second
sleep, `last_path`, `PrCache`, and channel-close exit behavior unchanged.

Remove the flag argument and stores from `app::run`, `input::handle_event`,
`input::handle_key_event`, and their tests. Do not change the process-wide
SIGUSR1 `AtomicBool` used by `app::run`.

- [ ] **Step 4: Run focused worker and input tests and make them GREEN**

Run:

```bash
cargo test app::workers::tests
cargo test app::input::tests
cargo test app::render::tests
```

Expected: PASS. No test or production identifier named `git_tab_active` remains.

- [ ] **Step 5: Commit the polling change**

```bash
cargo fmt
git add src/app.rs src/app/input.rs src/app/workers.rs
git commit -m "refactor: poll git for the visible panel"
```

---

### Task 4: Update state documentation and remove stale tab terminology

**Files:**
- Modify: `docs/state-management.md`
- Modify: Rust tests that still describe tabs in names or comments.

**Interfaces:**
- Documents: `BottomPanel`, `active_bottom_panel`, `Focus::BottomPanel`, per-pane `bottom_panel_pref`, two independent visible viewports, and height-gated continuous Git polling.
- Preserves: the public tmux option `@sidebar_bottom_height` and its `0` behavior.

- [ ] **Step 1: Run the full test suite and terminology search**

Run:

```bash
cargo test
```

Expected: PASS because Task 1 already reconciled the layout snapshots and
Tasks 2-3 preserve the rendered output.

```bash
rg -n "Activity │ Git|BottomTab|bottom_tab|Focus::ActivityLog|GitStatus|git_tab_active" src tests docs/state-management.md
```

Expected: the test suite passes. The search identifies only stale test names,
comments, and state-management prose; production identifiers were already
renamed in Task 2.

- [ ] **Step 2: Update `docs/state-management.md` and test descriptions**

Replace tab-oriented rows and prose with the new state model:

```text
Focus::BottomPanel             keyboard focus is in the stacked bottom region
active_bottom_panel            selected Git or Activity scroll target
bottom_panel_pref              per-pane remembered active target
scrolls.git                    Git file-list offset and viewport
activity.scroll                Activity offset and viewport
Git polling                    every two seconds when bottom_panel_height > 0
```

Update the data-flow diagram and invariants so they state that both renderers
run into equal card rectangles and Git is not gated by the active selection.
Rename test functions and comments that still say `tab` when they now describe
active-panel selection. Do not change event-layer `ActivityLog` names; those
refer to agent hook events rather than bottom-region focus.

- [ ] **Step 3: Run focused regressions**

Run:

```bash
cargo test --test bottom_tests
cargo test --test styled_tests
cargo test --test color_tests
cargo test --test ui_snapshot
cargo test --test state_tests
```

Expected: PASS with no `.snap.new` files and no old tab-header text.

- [ ] **Step 4: Commit documentation and terminology cleanup**

```bash
cargo fmt
git add docs/state-management.md tests/bottom_tests.rs tests/state_tests.rs tests/styled_tests.rs
git commit -m "docs: describe stacked bottom panels"
```

---

### Task 5: Final verification and release build

**Files:**
- Verify only; modify production or test files only if a new failing test reproduces a discovered defect.

**Interfaces:**
- Verifies all success criteria from `docs/superpowers/specs/2026-07-17-stacked-bottom-panels-design.md`.

- [ ] **Step 1: Check formatting and stale terminology**

Run:

```bash
cargo fmt --check
rg -n "Activity │ Git|BottomTab|bottom_tab|Focus::ActivityLog|GitStatus|git_tab_active" src tests docs/state-management.md
git diff --check
```

Expected: formatting and diff checks pass; the terminology search returns no stale identifiers or old combined tab titles.

- [ ] **Step 2: Run CI-equivalent validation**

Run:

```bash
cargo test
cargo clippy -- -D warnings
```

Expected: all tests pass and Clippy reports no warnings.

- [ ] **Step 3: Build the release binary**

Run:

```bash
cargo build --release
```

Expected: successful optimized build at `target/release/tmux-agent-sidebar`.

- [ ] **Step 4: Review task scope and repository state**

Run:

```bash
git status --short
git log --oneline -6
git diff HEAD~4..HEAD --stat
```

Expected: `.idea/` remains untracked and unchanged; only the design, plan,
stacked-panel implementation, state documentation, and related tests appear in
the task commits. Do not push.
