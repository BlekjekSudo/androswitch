# ADR 005 — Real SQLite through an isolated child process

**Status:** accepted (Comprix e2e, round 3)

## Context

Room apps (Comprix, and most modern apps) issue real SQL: JOINs, ORDER BY,
aggregates, `sqlite_master` and pragma queries, triggers, and
`rawQueryWithFactory` with bind args. The M8 in-memory SQL subset (ADR 003)
cannot run them. Comprix died on its first Room query.

The options considered were:
1. extending the hand-written subset;
2. linking `rusqlite` with bundled SQLite (C code, `unsafe` FFI, a C
   toolchain in every build, and MSRV churn);
3. a child process that owns a real SQLite.

## Decision

Option 3. `crates/rd-vm/src/jsqlite.rs` runs `python3 -u` with a small
`sqlite3` server script and speaks JSON lines with it (open, exec, query,
transaction, close). `SQLiteDatabase`, `SQLiteStatement`, `Cursor` and
`SQLiteOpenHelper` are served on top of it.

- Database files live in a per-session temporary directory that mirrors
  `/data/user/0/<pkg>/databases/`.
- The interpreter is configurable with `RD_SQLITE_PYTHON`.
- `RD_SQLITE=subset`, `Engine::use_sqlite_subset()` or a missing Python
  falls back to the M8 subset, which stays deterministic and dependency-free.

## Consequences

- No `unsafe` and no C build in rd-vm; the workspace policy and MSRV 1.76
  hold. A crash in SQLite cannot take the VM down. This mirrors the ADR 004
  isolation choice for native code.
- Each call costs an IPC round trip, which is microseconds and well below
  DEX interpretation cost.
- Python 3 is a runtime dependency only for apps that use SQLite. CI images
  already have it.
- Tests that inspect the subset model (`Engine::m8_database`) must opt into
  the subset with `use_sqlite_subset()`.
