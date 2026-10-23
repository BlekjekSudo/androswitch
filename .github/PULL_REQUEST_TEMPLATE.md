## Problem and resulting behavior

<!-- Explain the trigger, previous behavior and result for a reviewer new to the change. -->

## Scope and provenance

<!-- Identify the AndroSwitch milestone and shared Android API/ABI contract.
     For imported code/fixtures, include source, revision, license and attribution.
     Preserve RustyDroid credit and notices; do not add package-specific runtime hooks. -->

## Validation

<!-- List commands, fixture hashes and actual outcomes. Mark unrun checks explicitly. -->

| Backend | Result / evidence |
| --- | --- |
| Host | |
| Horizon emulator | |
| Physical Switch | |

## Review notes

- [ ] Missing APIs/symbols return structured errors rather than fabricated success
- [ ] Device changes preserve the `no_std + alloc` platform boundary
- [ ] Relevant tests, format, clippy and documentation checks pass
- [ ] Compatibility inventory/roadmap updated if behavior changed
- [ ] RustyDroid and other applicable source credits/licenses retained
- [ ] AI assistance, if used, described along with how the result was verified

<!-- Add an ADR for significant platform/VM/JNI/storage/lifecycle decisions. -->
