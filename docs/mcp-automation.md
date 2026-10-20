# Reliable MCP automation and diagnostics

The generated [tool catalog](mcp-tools.md) remains the source of truth for all
47 registered tools. The original 41 tool definitions are preserved; the six
additions below complement, rather than replace, the existing tools.

## Structured host-view automation

- **`find_views`** returns leaf metadata for AND-combined `resource_id`, `text`
  and/or `class` selectors. At least one nonblank selector is required. Text is
  exact by default; `contains: true` explicitly opts into substring matching.
  Class names match the full name or exact simple name, not a suffix or subclass.
- Optional `enabled`, `clickable` and `visible` filters are booleans. Visibility
  inherits through parents within each window, but not from activity content
  into a separate dialog window. Enabled state follows actual per-view host
  hit testing, not an invented parent-enabled rule.
- Default discovery is visible views, with 50 returned matches; `limit` is
  1–200. Snapshot construction and traversal are capped at 10,000 nodes. A
  result-count limit is **not** tree truncation: `total_is_exact` and
  `truncated` distinguish these cases, while `has_more` is conservative.
- Returned `text` is a UTF-8-safe prefix of at most 4,096 bytes.
  `text_bytes` and `text_truncated` disclose this explicitly. Matching and tap
  identity checks still use the full original text; output clipping never
  changes which view matches.
- **`tap_view`** requires exactly one matching, visible, enabled target in a
  complete snapshot. It clips the target center to the 720×1440 viewport and
  verifies the actual foreground-window hit before injecting input. This
  includes clickable/visibility/enabled identity, not merely equal labels and
  bounds. An open dialog cannot accidentally be dismissed by targeting a view
  behind it. `listener_acionado: false` honestly means no listener fired; it is
  not promoted to application-level success.
- `find_views.tree_token` can be supplied as `tap_view.expected_token`.
  Tokens bind session, package, revision and observed host state. They are
  optimistic concurrency guards, **not authentication tokens**. Potentially
  mutating calls, including waits and failed action attempts, can invalidate
  them; re-discover immediately before a guarded tap. Read-only diagnostics do
  not invalidate them. A stale token produces `STALE_VIEW` before input.
- **`wait_for_view`** polls for `state: "present"` or `"absent"` using virtual
  time, with an immediate initial check. Defaults are 5,000 ms timeout and
  100 ms steps. `timeout_ms: 0` checks immediately, even under a zero-wait
  policy. The last step never overshoots the timeout. Existing policy/fuel
  limits apply, plus at most 1,000 virtual-time steps. `polls` counts checks,
  including the initial check. `condition_met` is the condition result;
  `present` separately reports whether a matching view exists. A timeout is
  **not** a successful assertion. Incomplete trees cannot prove absence or
  authorize a unique tap.

Queries target the **running engine**, which can differ from the active installed
package; `session_info` reports both. Dialog-only activities are supported even
without activity content.

These tools inspect the host view hierarchy, not labels painted by Canvas or
Compose. Continue to use `observe` / `tap_text` for display-list labels and
`screenshot` for visual inspection. Existing assertion tools are unchanged.

Example using the genuine C workbench fixture (the helper `call` sends a normal
MCP `tools/call` and decodes its text JSON):

```python
selector = {"resource_id": "org.rustydroid.demo.c:id/compute", "clickable": True}
found = call("find_views", selector)
assert found["total"] == 1 and found["total_is_exact"]
result = call("tap_view", {**selector, "expected_token": found["tree_token"]})
assert result["listener_acionado"]  # Application-specific expectation.
ready = call("wait_for_view", {
    "resource_id": "org.rustydroid.demo.c:id/result", "timeout_ms": 0,
})
assert ready["condition_met"]
```

## Runtime diagnostics

- **`session_info`** works before install/launch and after an isolated runtime
  failure. It distinguishes active from running package, reports compiled
  capability flags and policy limits, and reports sandbox roots only as a
  count. Compiled native/JIT support does not promise that a runtime dependency
  is installed or an application is compatible. Use `runtime_health` and
  `native_state` for detailed runtime accounting. It does not execute guest
  code, reset its fuel window, force or trigger GC, or mutate the UI revision.
- **`logs_tail`** reads at most `max_bytes` raw bytes (default 4,096; maximum
  65,536). Without a cursor it returns the newest bytes. Save `next_cursor` and
  pass it back for subsequent incremental reads; `has_more` indicates backlog.
  Cursors bind session identity, clear generation and byte offset. A different
  session or `logs(clear=true)` makes an old cursor stale, even if the buffer
  subsequently grows past the old offset. A future offset is invalid rather
  than silently clamped. `data_b64` is authoritative: decode and concatenate
  bytes before decoding UTF-8. `text` is only a convenience view, with
  `text_lossy` disclosing invalid/split UTF-8. Reads never clear the buffer or
  deadlock on a multibyte character split. The legacy log buffer remains
  session-wide and unbounded; this tool bounds reads, not retention.
- **`last_error`** retains the latest typed tool failure, including early
  validation returns, failed batch children, and isolated runtime panics.
  Successful calls do not erase it. `clear: true` returns then clears the
  snapshot without clearing logs or resetting the cumulative error-event
  count. Each retained message field is byte-bounded and UTF-8-safe;
  `truncated` and `cause_bytes` disclose clipping. Original tool results are
  unchanged. Input arguments are not retained, but the original error message
  can itself contain app-provided text or paths; this is not secret redaction.
  Protocol parse/method errors are outside this tool-failure snapshot.

All three diagnostic additions are read-only with respect to the guest,
clock, fuel window, GC and UI revision. Clearing an error snapshot only changes
its diagnostic bookkeeping.

## Validation and reproducible acceptance

Malformed optional values no longer silently become different actions:
`tap_text` (`exact`, `nth`), `type_text` (`replace`), `observe` (`diff`, `boxes`,
`format`), `logs` (`clear`), and `runtime_health` (`gc`) reject types outside their
existing schemas before app code runs. Valid legacy requests retain their
behavior and schemas. New tools reject unknown fields and null/wrong-type
options; limits cannot be bypassed by negative, fractional or huge integers.

The genuine native MCP gate requires positive coverage of **every registered
tool** for C, C++ and Rust, on x86_64, arm64-v8a and armeabi-v7a, with JIT off/on:

```sh
bash golden/native-languages/gate.sh
```

For longer functional testing, after building the genuine fixtures and a
native/JIT-enabled CLI:

```sh
python scripts/mcp-session-soak.py --rd target/debug/rd --apks target/native-languages/apks --seconds 600 --out target/mcp-soak
```

The soak pins its executable, exercises 18 persistent language/ABI/JIT sessions,
checks independently computed JNI results, stale-token rejection, presence and
absence semantics, repeated PNG equality, warm restarts and forced GC. When
available, it also checks diagnostic error context and incremental logs. It
stores bounded summaries and response-stream hashes instead of unbounded image
transcripts. It is functional endurance evidence, **not a performance benchmark**.

### Compatibility note: numeric visibility

The host now accepts Android setVisibility values 4 (INVISIBLE) and 8 (GONE), alongside the pre-existing 0/1/2 ordinal inputs. The legacy numeric getVisibility convention (0/1/2) is deliberately unchanged in this MCP-focused change; fully aligning that numeric getter with Android needs a separate compatibility decision. Structured selectors use the actual visibility state, not that numeric getter.
