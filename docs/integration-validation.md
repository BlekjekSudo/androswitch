# Runtime integration validation — 2026-10-20

Branch: `feat/comprix-e2e` · target: `main` · PR #160.
AI-assisted implementation; human review required before merge.
Checkpoint: 2026-10-20T05:54:08.894388+00:00.

## Local gates

- `cargo test --workspace --all-features`: **510 passed, zero failed**.
  Native prerequisites: three cross-compiled fixtures and Unicorn 2.1.4.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo +1.76.0 check --workspace --all-features`: passed.
- `cargo check -p rd-vm --no-default-features`: passed.
- `cargo fmt --all -- --check`, internal Markdown links, generated compatibility
  matrix comparison and `git diff --check`: passed.
- Real JVM golden comparison: **150/150 passed**; gate self-test also rejects
  empty/149-vector false greens.
- MCP contract-no-app suite: **8/8 scenarios passed**.
- JIT differential tests cover integer edge/random values, literals/narrowing,
  division fallback and fuel. Backend native ABI/arity checks pass.

## Real Comprix checkpoint (endurance still running)

Release binary; Comprix 1.6.0 x86_64 release APK, SHA-256:
`39a4930c89f1a3670eca03cc6eb37114a0467c52687bf028dd633a6b60c6d6f4`.
Desktop Linux sandbox with a 2 GiB memory limit, not physical Moto E5 hardware.

- Actual typed entry `2 kg de arroz` → **Arroz / 2 x kg / 1 item**.
- Named icon targets include Buscar listas, Minhas lojas, Voltar, Marcar como
  favorita and the comparison toolbar's copy/export/store controls.
- GC off: **2 complete tab cycles / 17 MCP steps**, zero errors; an additional
  portable-harness GC-off smoke passed 1 cycle / 12 steps.
- GC on + JIT on, populated-list soak checkpoint: **101 cycles /
  522 MCP steps**, no fatal error recorded. Duration so far:
  1363.79 seconds. Final result remains pending.
- Latest tracked app heap: 48,046,429 bytes; peak:
  111,996,429 bytes against the 268,435,456-byte budget.
  This accounting is not total process RSS.
- Lifetime fuel: 1,827,165,937; healthy sessions can exceed one operation's
  200-million instruction budget because each MCP tool receives a fresh bounded
  fuel window.
- JIT: 2 native leaves, 380,155 native calls,
  0 compile errors. Unsupported methods stay interpreted.
- GC: 80 collections
  (47 minor / 33 major),
  2,267,713,732 cumulative tracked bytes reclaimed.

Repeated frames are independently audited against unique **screen titles**,
not bottom-bar labels or the observer's previous-frame diff. The portable
harness uses these stronger assertions itself.

## Reproduce

```sh
cargo build --release -p rd-cli --features jit
python3 scripts/comprix-regression.py --apk /path/to/comprix.apk --typed --gc on --jit on --cycles 100
python3 scripts/comprix-regression.py --apk /path/to/comprix.apk --gc off --jit off --cycles 2
```

The harness does not download APKs. It writes `summary.json`, full JSON-RPC
transcripts, stderr, and a typed-item screenshot under
`target/comprix-campaigns/<name>/`. Each operation has a 120-second watchdog;
tool errors, provider/frame warnings and wrong screen titles fail the run.

## Issues found and corrected during strict real-APK validation

1. Missing Region/AccessibilityAction APIs prevented the standard Compose
   provider from initializing. Implemented bounded platform models and tests.
2. Dialog accessibility queries inherited Context.getPackageName through
   ContextThemeWrapper. Delegation now honors real base DEX overrides and
   inherited Activity/Application host methods; cyclic wrapper chains fail.
3. A high pinned live object count caused a collection at every instruction
   above the fixed 75% slot watermark. Adaptive headroom/backoff and a dedicated
   >786k-object regression prevent this collection storm.
4. A negative integer immediate needed zero-extended i32 bits in Cranelift IR.
   Differential literal/narrowing tests now pass.
5. Host temporaries returned from a guest callback need protection before a
   second callback can collect. The return-root regression now passes.

## Explicit limits / merge gates

- The collector is generational and stop-the-world, not incremental/concurrent.
  Some measured desktop major pauses exceed 100 ms, especially under concurrent
  compilation. No Moto E5 latency or memory budget certification is claimed.
- JIT is off by default and restricted to integer leaves. ARMv7 is not a native
  JIT target; ARM64 execution still needs device validation.
- Provider traversal, Java regex, Region and collection views remain bounded
  subsets; see ADRs 006/007 and Comprix notes. Camera/Scanner is not covered.
- GitHub CI for the final pushed commit, completed endurance result and human
  review must be checked separately. A repository-protection API request was
  denied by token permissions; protection requirements are **unknown**, not
  assumed absent. Do not bypass them or fabricate review approval.
