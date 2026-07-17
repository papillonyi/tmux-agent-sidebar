# Codex Subagent Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show currently running Codex subagents in the existing sidebar tree by wiring Codex `SubagentStart` and `SubagentStop` hooks into the shared subagent lifecycle.

**Architecture:** Extend `CodexAdapter` with two registrations and two parse arms, then reuse the existing `AgentEvent`, hook handlers, `@pane_subagents` storage, tmux parsing, and UI rendering. Keep completed messages and transcript paths transient and out of pane/UI state.

**Tech Stack:** Rust 2024, serde_json, tmux pane options, Ratatui, Cargo tests.

## Global Constraints

- Work in the current workspace; do not create a worktree.
- Preserve the existing `agent_type:agent_id` tmux storage format and `agent_type #<id-prefix>` UI format.
- Display only currently running subagents; remove each entry on its matching stop event.
- Use `subagent` when `agent_type` is absent but `agent_id` is present.
- Do not add an entry when `agent_id` is absent or empty.
- Do not persist or render `last_assistant_message` or `agent_transcript_path`.
- Do not change Claude Code subagent behavior or the sidebar layout.
- Run `cargo fmt` before every commit and `cargo build --release` after implementation.

## File structure

- `src/adapter/codex.rs`: Codex hook registration, payload normalization, and inline unit tests.
- `src/cli/setup/tests.rs`: generated Codex hook configuration assertions.
- `src/cli/hook/handlers/subagent.rs`: shared lifecycle handler documentation only; runtime behavior remains unchanged.

---

### Task 1: Wire Codex subagent lifecycle into the existing tree

**Files:**
- Modify: `src/adapter/codex.rs:9-369`
- Modify: `src/cli/setup/tests.rs:162-195`
- Modify: `src/cli/hook/handlers/subagent.rs:5-15`
- Test: inline tests in `src/adapter/codex.rs` and `src/cli/setup/tests.rs`

**Interfaces:**
- Consumes: Codex hook JSON fields `agent_type`, `agent_id`, `last_assistant_message`, and `agent_transcript_path`.
- Produces: `AgentEvent::SubagentStart { agent_type: String, agent_id: Option<String> }` and `AgentEvent::SubagentStop { agent_type: String, agent_id: Option<String>, last_message: String, transcript_path: String }`.
- Reuses: `on_subagent_start`, `on_subagent_stop`, `@pane_subagents`, `parse_subagents`, and `subagent_rows` without signature changes.

- [ ] **Step 1: Replace the Codex negative subagent tests with failing lifecycle tests**

In `src/adapter/codex.rs`, replace `subagent_start_not_supported` with:

```rust
#[test]
fn subagent_start_extracts_full_payload() {
    let input = json!({
        "hook_event_name": "SubagentStart",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "agent_id": "agent-a81f1234",
        "agent_type": "reviewer",
        "permission_mode": "default"
    });

    assert_eq!(
        CodexAdapter.parse("subagent-start", &input),
        Some(AgentEvent::SubagentStart {
            agent_type: "reviewer".into(),
            agent_id: Some("agent-a81f1234".into()),
        })
    );
}

#[test]
fn subagent_start_missing_type_uses_generic_label() {
    let input = json!({"agent_id": "agent-a81f1234"});

    assert_eq!(
        CodexAdapter.parse("subagent-start", &input),
        Some(AgentEvent::SubagentStart {
            agent_type: "subagent".into(),
            agent_id: Some("agent-a81f1234".into()),
        })
    );
}

#[test]
fn subagent_start_missing_id_keeps_event_untrackable() {
    let input = json!({"agent_type": "reviewer"});

    assert_eq!(
        CodexAdapter.parse("subagent-start", &input),
        Some(AgentEvent::SubagentStart {
            agent_type: "reviewer".into(),
            agent_id: None,
        })
    );
}
```

Replace `subagent_stop_not_supported` with:

```rust
#[test]
fn subagent_stop_extracts_full_payload() {
    let input = json!({
        "hook_event_name": "SubagentStop",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "agent_id": "agent-a81f1234",
        "agent_type": "reviewer",
        "agent_transcript_path": "/tmp/codex-subagent.jsonl",
        "last_assistant_message": "Review complete",
        "stop_hook_active": false,
        "permission_mode": "default"
    });

    assert_eq!(
        CodexAdapter.parse("subagent-stop", &input),
        Some(AgentEvent::SubagentStop {
            agent_type: "reviewer".into(),
            agent_id: Some("agent-a81f1234".into()),
            last_message: "Review complete".into(),
            transcript_path: "/tmp/codex-subagent.jsonl".into(),
        })
    );
}

#[test]
fn subagent_stop_missing_type_uses_generic_label() {
    let input = json!({"agent_id": "agent-a81f1234"});

    assert_eq!(
        CodexAdapter.parse("subagent-stop", &input),
        Some(AgentEvent::SubagentStop {
            agent_type: "subagent".into(),
            agent_id: Some("agent-a81f1234".into()),
            last_message: String::new(),
            transcript_path: String::new(),
        })
    );
}
```

In `src/cli/setup/tests.rs`, add:

```rust
#[test]
fn snippet_codex_subagent_hooks_map_to_lifecycle_commands() {
    let v = build_agent_snippet("codex", FAKE_HOOK).unwrap();
    for (trigger, event) in [
        ("SubagentStart", "subagent-start"),
        ("SubagentStop", "subagent-stop"),
    ] {
        let command = v
            .pointer(&format!("/hooks/{trigger}/0/hooks/0/command"))
            .and_then(Value::as_str);
        let expected = format!("bash /fake/hook.sh codex {event}");
        assert_eq!(command, Some(expected.as_str()));
    }
}
```

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```bash
cargo test adapter::codex::tests::subagent --lib
cargo test cli::setup::tests::snippet_codex_subagent_hooks_map_to_lifecycle_commands --lib
```

Expected: the adapter tests fail because `parse()` returns `None`, and the setup test fails because the generated hook paths are absent.

- [ ] **Step 3: Add the two Codex hook registrations**

In `CodexAdapter::HOOK_REGISTRATIONS`, insert before `PostToolUse`:

```rust
HookRegistration {
    trigger: "SubagentStart",
    matcher: None,
    kind: AgentEventKind::SubagentStart,
},
HookRegistration {
    trigger: "SubagentStop",
    matcher: None,
    kind: AgentEventKind::SubagentStop,
},
```

Update the adapter documentation to say that it wires six registrations and
that only `PreToolUse`, `PermissionRequest`, and compaction hooks remain
unwired.

- [ ] **Step 4: Add the minimal Codex parse arms**

In `CodexAdapter::parse`, insert before `activity-log`:

```rust
"subagent-start" => {
    let agent_type = json_str(input, "agent_type");
    Some(AgentEvent::SubagentStart {
        agent_type: if agent_type.is_empty() {
            "subagent".into()
        } else {
            agent_type.into()
        },
        agent_id: optional_str(input, "agent_id"),
    })
}
"subagent-stop" => {
    let agent_type = json_str(input, "agent_type");
    Some(AgentEvent::SubagentStop {
        agent_type: if agent_type.is_empty() {
            "subagent".into()
        } else {
            agent_type.into()
        },
        agent_id: optional_str(input, "agent_id"),
        last_message: json_str(input, "last_assistant_message").into(),
        transcript_path: json_str(input, "agent_transcript_path").into(),
    })
}
```

In `src/cli/hook/handlers/subagent.rs`, replace the Claude-specific comment
above the missing-id guard with:

```rust
// Supported subagent hook schemas provide agent_id. Drop malformed events
// without it so the tree never gains an entry that SubagentStop cannot remove.
```

Do not change the guard or any other shared handler behavior.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
cargo test adapter::codex --lib
cargo test cli::setup --lib
```

Expected: all Codex adapter and setup tests pass, including the registration drift test and generated command assertions.

- [ ] **Step 6: Format and run complete verification**

Run:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
cargo fmt --check
cargo build --release
```

Expected: every command exits successfully with no test failures or Clippy errors.

Run the generated setup command:

```bash
target/release/tmux-agent-sidebar setup codex
```

Inspect the JSON and confirm it includes `SubagentStart` mapped to
`subagent-start` and `SubagentStop` mapped to `subagent-stop`.

- [ ] **Step 7: Review and commit only implementation files**

Run:

```bash
git diff --check
git diff -- src/adapter/codex.rs src/cli/setup/tests.rs src/cli/hook/handlers/subagent.rs
git status --short
```

Confirm there are no conflict markers and no unrelated paths. Then commit:

```bash
git add src/adapter/codex.rs src/cli/setup/tests.rs src/cli/hook/handlers/subagent.rs
git commit -m "feat: show Codex subagents"
```

Do not push.
