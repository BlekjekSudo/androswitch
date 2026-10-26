# Shared upstream numeric API family

The implementation comes from RustyDroid by nadabgfccvd and contributors at
`3761815dc6a2a1427e9fc66b66e8471b78d1b170`. Its original `numerics.rs` now builds
on the shipping std profile. The original Math arms and scalar helpers formerly
inside `intrinsics.rs` move into that same family; reference and product calls
use one implementation. The small isolated core-only VM retains its earlier
limited fallback. No application identity selects behavior.

The family covers the inherited Integer/Long radix conversion and parsing,
Float/Double representation and comparisons, Boolean utilities, Character
classification/code points, and Math/StrictMath operations. Wrapper objects and
caches still belong to the existing common `boxes` heap implementation. Numeric
calls cannot create an independent framework object store.

An exact descriptor catalog serves both dispatch and JNI lookup. Broad inherited
Character name matches cannot grant fabricated overloads. Calls retain arguments
and returned references through the existing shared GC scope. CharSequence code
points read the canonical final String value or invoke actual virtual length
and charAt methods, including DEX overrides and callbacks that invoke GC; they
do not substitute Object.toString. Only the one or two required units are read.

The independent JVM scenarios expose inherited arithmetic and exception edge cases.
Float.min/max now share the original Math signed-zero/NaN helpers. Math.round
splits integer and fractional parts so a representable value just below 0.5
does not become 1 through premature addition rounding. Both profiles execute
these corrections. The original `numerics_319.rs` regression bodies remain
unchanged and also have a shipping-profile wrapper.
Floating parse/valueOf null inputs use NullPointerException; integer null
parsing retains NumberFormatException. Both are measured against the JVM.

The frozen reflection oracle executes 450 public JDK overload/input rows. The
shipping test selects the exact inherited metadata surface: **233 comparisons
across 218 declarations**. Each selected method really executes, including
nullable inputs and char-array mutations. Unselected JDK methods are not
counted as implemented. These are fixed input samples, not a proof of all
Unicode values or every transcendental rounding corner.

The same SDK APK source grows to 296 declared cases, with 170 independent JVM
rows. Twenty-two added cases exercise numeric families, raw UTF-16 builder
edits, actual CharSequence callbacks with GC, signed zeros/NaNs, rounding
boundaries, radix parsing, errors and live boxed values. Source/catalog presence
is not execution or device verification; the execution report identifies exact
binaries and measured results separately.

This does not establish complete Android numeric compatibility or whole-runtime
1:1 parity. Inherited Character category/digit/directionality tables are partial;
Unicode data versions and StrictMath bit-level transcendental results are not
certified against an API26 Android reference. Unicode numeric parsing and Java
hexadecimal floating literals remain explicitly unavailable in the existing
shared parser. Other wrapper/property APIs outside this family's declarations
remain in the complete porting ledger.
