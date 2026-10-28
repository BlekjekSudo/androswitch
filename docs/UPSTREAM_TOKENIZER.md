# Shared StringTokenizer integration

The implementation starts from RustyDroid's `jstring_tokenizer.rs` by
nadabgfccvd and contributors at pinned upstream
`3761815dc6a2a1427e9fc66b66e8471b78d1b170`. The entire inherited family is
selected on the shipping std profile. The three constructors and six instance
declarations have one exact descriptor catalog used by VM and Android/JNI
lookup. Enumeration calls use the same tokenizer owner. No application identity
selects behavior.

The original delimiter-set/cursor algorithm remains in that module. Its former
HostState variant is removed. One Engine-owned typed table stores the real
immutable source and delimiter String references, cursor and return-delimiter
flag. It does not retain a second permanent copy of either input. GC treats
these as owner-dependent edges, revisits old owners in minor collections, and
purges dead owners before arena slots can be reused. Numbers and cursor values
are never conservative roots. Constructors remain tied to their declared class;
DEX subclass initializers and overrides still execute. Enumeration aliases and
nextToken(delimiters) call actual virtual methods, including GC and super calls.

Independent JVM scenarios expose inherited gaps: supplementary delimiter code
points, nullable delimiters, mutation after failed nextToken(null), and cursor
advancement on exhaustion. Both VM profiles execute the corrections. Internally
the cursor uses valid UTF-8 boundaries of the existing String heap; supplementary
characters are kept whole. Full unpaired-surrogate String storage remains an
explicit separate limitation. The
[Java contract](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/StringTokenizer.html)
and the installed OpenJDK source were used to check these cases; they are not
copied implementation bodies or an Android API26 certification.

Three shared GC/state unit contracts cover input lifetime, owner disposal/slot
reuse, old-to-young delimiter edges, scalar values coinciding with object IDs,
and Engine isolation. A shipping test repeats the original returned-delimiter,
Enumeration and delimiter-change scenario, and another checks all nine exact
declarations. The original complete reference suite remains registered. The
central SDK source adds ten scenarios and grows to 309 declared cases with 183
actual JVM rows. One scenario exercises DEX subclass field initialization,
overrides, super calls and callbacks that collect garbage.

Counts describe the declared corpus. The execution report must identify which
checks actually ran on each exact binary. No older 219-case hardware result is
transferred to this change. Source inclusion, host execution and an NRO build do
not establish full upstream 1:1 parity or game first-frame compatibility.
