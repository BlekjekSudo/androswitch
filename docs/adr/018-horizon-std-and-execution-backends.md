# ADR 018: Horizon std and execution backends

Date: 2026-10-26. Status: accepted by the user.

## Context

The original Switch target did not ship a Rust std component. AndroSwitch used
core/alloc and libnx adapters. In the inherited VM, `std` also selected desktop
state, output and process transports. Turning it on unchanged would disconnect
the runner's JNI, persistent filesystem, execution owners and GC roots.

The user now requests completing std support and migrating the current product
to it, principally to reduce friction when reusing upstream RustyDroid code.

## Decision

Build real std from pinned source with the task-local Horizon/newlib PAL.
Separate Rust library support (`std`) from execution (`desktop-runtime`). The
isolated `as-vm` package shares the original source without exposing the desktop
backend. Preserve the existing embedded state graph and platform contracts.
Default device crates use std; parsers and interfaces can still use core/alloc.

SDK compatibility belongs in a reproducible copied source overlay, libc ABI
adaptation and C PAL. Do not patch installed toolchains or ignore linker errors.
Keep Mesa's compiler-matched Rust runtime separate at its native ABI boundary.

## Consequences

Rust collections, I/O, clocks, synchronization and threads become available to
device source. Upstream algorithms can use std without selecting desktop
processes, global Android state, Python SQL or Unicorn. Unsupported OS contracts
remain errors. Fixed-permission SD file copying is a documented mount policy;
explicit chmod does not pretend to work.

This supersedes the product library restriction in ADRs 010–012. Their single
VM, shared algorithms, platform transport, state ownership and ABI requirements
remain in force. Full upstream API integration is still tracked separately.

Every compiler/SDK update must rerun ABI facts, std lifecycle/thread tests,
inherited VM suites and product/E2E regressions, with hardware evidence recorded
separately. The 32-case probe alone cannot certify all std or Android behavior.
See [migration evidence and reproduction](../HORIZON_STD_MIGRATION.md).
