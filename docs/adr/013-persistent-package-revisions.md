# ADR 013: immutable package revisions and one persistent VFS

Status: implemented for M2; hardware validation pending.

Use `as-platform::fs::FileSystem` as the bounded root-relative platform contract.
`as-package` copies validated APKs into exclusively reserved revision directories.
Publishing a checksummed `commit.json` to a new destination avoids relying on
rename-overwrite behavior on the Horizon SD filesystem. Incomplete revisions
are invisible. Loading checks package, revision and copied APK hash. A stale
launcher request is rejected. Registry corruption is reported, not silently
rolled back. Power-cut resilience is not established by simulated I/O failures.

Private data remains outside installed revisions. Uninstall retains it; clearing
data is separate. One mount resolver serves Java and native file APIs, with
bounded positioned I/O for OBB and assets. API/ABI/split acceptance belongs to
shared package policy, not package-specific profiles. Signatures are currently
inventoried, not verified.

See [M2–M4 tests and limits](../M2_M4_E2E.md).
