package org.androswitch.timeunits;

import java.util.concurrent.TimeUnit;

/** Original SDK fixture. All expected values are produced by the actual JVM. */
public final class TimeUnitCases {
    private static final TimeUnit[] ALL = TimeUnit.values();
    private enum Other { ONE }
    private static final long[] PROBES = {
        Long.MIN_VALUE, Long.MIN_VALUE + 1, -86400000000000L, -3600000000000L,
        -1000000001L, -1000L, -999L, -1L, 0L, 1L, 999L, 1000L,
        1000000001L, 3600000000000L, 86400000000000L, Long.MAX_VALUE - 1,
        Long.MAX_VALUE
    };
    // Used only to generate extra boundary inputs, never expected results.
    private static final long[] SCALES = {
        1L, 1000L, 1000000L, 1000000000L, 60000000000L,
        3600000000000L, 86400000000000L
    };

    public static long convertCase(int source, int target, long value) {
        return ALL[target].convert(value, ALL[source]);
    }
    public static long directCase(int source, int target, long value) {
        TimeUnit unit = ALL[source];
        switch (target) {
            case 0: return unit.toNanos(value);
            case 1: return unit.toMicros(value);
            case 2: return unit.toMillis(value);
            case 3: return unit.toSeconds(value);
            case 4: return unit.toMinutes(value);
            case 5: return unit.toHours(value);
            default: return unit.toDays(value);
        }
    }
    public static String name(int index) { return ALL[index].name(); }
    public static int ordinal(int index) { return ALL[index].ordinal(); }
    public static int compare(int first, int second) { return ALL[first].compareTo(ALL[second]); }
    public static int identity(int index) {
        TimeUnit unit = ALL[index];
        return unit == TimeUnit.valueOf(unit.name())
            && unit.getDeclaringClass() == TimeUnit.class
            && unit.name().equals(unit.toString()) ? 1 : 0;
    }
    public static int freshArray() {
        TimeUnit[] first = TimeUnit.values();
        TimeUnit[] second = TimeUnit.values();
        if (first == second || first.length != 7 || second.length != 7) return 0;
        first[0] = TimeUnit.DAYS;
        System.gc();
        return second[0] == TimeUnit.NANOSECONDS && ALL[0] == TimeUnit.NANOSECONDS ? 1 : 0;
    }
    @SuppressWarnings({"rawtypes", "unchecked"})
    public static int error(int kind) {
        try {
            switch (kind) {
                case 0: TimeUnit.valueOf((String)null); break;
                case 1: TimeUnit.valueOf("seconds"); break;
                case 2: TimeUnit.valueOf(" SECONDS"); break;
                case 3: TimeUnit.SECONDS.convert(1L, null); break;
                case 4: ((Comparable)TimeUnit.SECONDS).compareTo(Other.ONE); break;
                default: ((TimeUnit)null).toNanos(1L); break;
            }
        } catch (NullPointerException expected) { return 10; }
        catch (IllegalArgumentException expected) { return 20; }
        catch (ClassCastException expected) { return 30; }
        return -1;
    }
    public static String proofRows() {
        if (TimeUnit.DAYS.toNanos(Long.MAX_VALUE) != Long.MAX_VALUE
            || TimeUnit.DAYS.toNanos(Long.MIN_VALUE) != Long.MIN_VALUE
            || TimeUnit.SECONDS.convert(-999L, TimeUnit.MILLISECONDS) != 0L
            || TimeUnit.SECONDS.convert(-1001L, TimeUnit.MILLISECONDS) != -1L
            || freshArray() != 1) throw new AssertionError("TimeUnit boundary/array contract");
        StringBuilder names = new StringBuilder();
        int digest = 1;
        int count = 0;
        for (int source = 0; source < ALL.length; source++) {
            if (identity(source) != 1 || ordinal(source) != source)
                throw new AssertionError("TimeUnit enum identity");
            if (source != 0) names.append(',');
            names.append(name(source));
            for (int target = 0; target < ALL.length; target++) {
                for (int p = 0; p < PROBES.length; p++) {
                    long result = convertCase(source, target, PROBES[p]);
                    if (result != directCase(source, target, PROBES[p]))
                        throw new AssertionError("TimeUnit conversion variants");
                    digest = digest * 31 + (int)(result ^ (result >>> 32));
                    count++;
                }
            }
        }
        for (int kind = 0; kind < 6; kind++) {
            int expected = kind == 1 || kind == 2 ? 20 : kind == 4 ? 30 : 10;
            if (error(kind) != expected) throw new AssertionError("TimeUnit exception contract");
        }
        return "units=7 names=" + names.toString() + "\nvectors=" + count + " digest="
            + digest + "\nTIME_UNIT_PROOF_PASS\n";
    }

    private static void emit(int source, int target, long value) {
        System.out.println("C\t" + source + "\t" + target + "\t" + value + "\t"
            + convertCase(source, target, value));
        System.out.println("T\t" + source + "\t" + target + "\t" + value + "\t"
            + directCase(source, target, value));
    }
    public static void main(String[] args) {
        if (args.length != 0) { System.out.print(proofRows()); return; }
        for (int source = 0; source < ALL.length; source++) {
            System.out.println("N\t" + source + "\t" + name(source));
            System.out.println("O\t" + source + "\t" + ordinal(source));
            System.out.println("I\t" + source + "\t" + identity(source));
            for (int target = 0; target < ALL.length; target++) {
                System.out.println("P\t" + source + "\t" + target + "\t" + compare(source, target));
                for (long value : PROBES) emit(source, target, value);
                if (source > target) {
                    long ratio = SCALES[source] / SCALES[target];
                    long high = Long.MAX_VALUE / ratio;
                    long low = Long.MIN_VALUE / ratio;
                    emit(source, target, high - 1); emit(source, target, high); emit(source, target, high + 1);
                    emit(source, target, low - 1); emit(source, target, low); emit(source, target, low + 1);
                }
            }
        }
        System.out.println("A\t" + freshArray());
        for (int kind = 0; kind < 6; kind++) System.out.println("E\t" + kind + "\t" + error(kind));
    }
}
