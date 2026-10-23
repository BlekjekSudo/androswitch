# Security policy — AndroSwitch

This policy covers the AndroSwitch fork. The original RustyDroid policy is
preserved in [SECURITY.rustydroid.md](SECURITY.rustydroid.md).

## Project status

AndroSwitch is an experimental runtime. The M1 runner is a development
method probe; it does not implement Android's process/permission isolation.
Parser bounds and guest instruction/heap budgets are tested, but they are
not evidence of a complete Android security boundary.

Security fixes target the current development code in this repository.
There is no supported stable release or published response-time guarantee yet.

## Reporting

Send security-sensitive reports privately to the repository maintainers.
If GitHub private reporting is enabled, use **Security → Report a vulnerability**
on [BlekjekSudo/androswitch](https://github.com/BlekjekSudo/androswitch/security).
If that option is unavailable, establish a private channel with the owner
before sending the payload. Do not include credentials or exploit files in
a public issue.

Include the affected commit/component, host or device backend, input type,
reproduction commands and observed impact. For a crafted APK/DEX/ZIP/ELF,
provide the smallest reproducer and its hash through the private channel.

Retained RustyDroid files still point to upstream's reporting process. Reports
about this port should first reach this repository's maintainers; shared
upstream issues can then be coordinated with RustyDroid.
