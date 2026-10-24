# Native host test backends

The ABI and time scripts accept an explicit host backend. `Wsl` remains the
default. Selecting `Msys` runs the same C sources and assertions with the
installed POSIX compiler at `C:/devkitPro/msys2/usr/bin/gcc.exe`:

```powershell
./scripts/test-native-abi.ps1 -HostBackend Msys
./scripts/test-native-time.ps1 -HostBackend Msys
```

The available compiler reports GCC 15.2.0, target `x86_64-pc-cygwin`, and defines
both `__CYGWIN__` and `__MSYS__`. It uses the MSYS POSIX runtime. This backend is
distinct from GNU/Linux in Ubuntu WSL and from MinGW/w64devkit. The scripts
record the selected compiler and backend rather than reporting WSL success.
There is no automatic fallback when the default WSL backend is unavailable.

MSYS host C executables and command/log receipts are written under
`local-artifacts/m9/native-abi/msys` and `local-artifacts/m9/native-time/msys`.
The POSIX tool directory is added only to the script process's PATH for its
host C stage, then restored before Windows target, Python and Cargo tools.
Global environment, emulator profiles and WSL services are unchanged.

The ABI script still builds its Android ARM64 ELF fixtures using the Windows
NDK LLVM compiler. For the MSYS backend it compiles the errno oracle against
the actual Switch/newlib headers with the Windows devkitA64 compiler, then runs
the existing ARM64 CPU oracle. Host C execution and ARM64 CPU simulation are
separate evidence; neither substitutes for an NRO or physical Switch test.

`-SkipRustTests` on the ABI script explicitly omits the final `as-native` Cargo
tests when another task owns that compiler slot. Its receipt then reports
`passed_C_and_ARM64_oracle_Rust_skipped`, never an overall Rust-test pass. The
default continues to run the Rust loader tests.

The [MSYS host receipt](../reports/native-posix-msys-host-20261024.json) records
all five ABI C executables and the time contracts passing with unchanged
fixtures, plus the ARM64 bridge/errno oracle. The first ABI probe exposed a real
host conversion gap: the Rust filesystem callback returns Android numeric
errors such as `-38`, whereas MSYS host `ENOSYS` is 88. The existing symbolic
errno mapping now also applies under `__CYGWIN__`; the fixture retains Android
`-38` and its host-errno assertion. Seventy-three symbolic conversion pairs
passed, and the Switch errno oracle object compiled before/after the conditional
change is byte-identical. Device semantics and the ordinary Linux identity path
are unchanged. WSL was unavailable during this alternate-backend verification;
its tests are not marked passed by this receipt.
