package org.androswitch.vm;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

/** Same API 26-compatible source for a real JVM oracle and unmodified DEX. */
public final class AtomicFixture {
    public static final class Child extends AtomicBoolean {
        public int value = 43;
        Child(boolean value) { super(value); }
        public String toString() { return "DEX_OVERRIDE"; }
        public String baseText() { return super.toString(); }
    }
    public static final class IntChild extends AtomicInteger {
        public int value = 57;
        IntChild(int n) { super(n); }
        public int intValue() { return 307; }
        public String toString() { return "DEX_INT_OVERRIDE"; }
        public String baseText() { return super.toString(); }
    }
    public static final class LongChild extends AtomicLong {
        LongChild(long n) { super(n); }
        public String toString() { return "DEX_LONG_OVERRIDE"; }
        public String baseText() { return super.toString(); }
    }
    static AtomicBoolean shared;
    static boolean firstCaller() { return shared.compareAndSet(false, true); }
    static boolean secondCaller() { return shared.compareAndSet(false, true); }
    public static long run(int which) {
        AtomicBoolean a = new AtomicBoolean();
        switch (which) {
            case 0: return a.get() ? 1 : 0;
            case 1: return new AtomicBoolean(true).get() ? 1 : 0;
            case 2: a.set(true); return a.get() ? 1 : 0;
            case 3: a.set(true); a.lazySet(false); return a.get() ? 1 : 0;
            case 4: {
                boolean before = a.getAndSet(true);
                return (before ? 2 : 0) + (a.get() ? 1 : 0);
            }
            case 5: {
                a.set(true);
                boolean before = a.getAndSet(false);
                return (before ? 2 : 0) + (a.get() ? 1 : 0);
            }
            case 6: {
                boolean success = a.compareAndSet(false, true);
                return (success ? 2 : 0) + (a.get() ? 1 : 0);
            }
            case 7: {
                boolean success = a.compareAndSet(true, true);
                return (success ? 2 : 0) + (a.get() ? 1 : 0);
            }
            case 8:
                while (!a.weakCompareAndSet(false, true)) {}
                return a.get() ? 1 : 0;
            case 9: {
                AtomicBoolean b = new AtomicBoolean(true);
                a.set(false);
                return (a.get() ? 2 : 0) + (b.get() ? 1 : 0);
            }
            case 10: {
                Child b = new Child(false);
                b.set(true);
                return 2 * b.value + (b.get() ? 1 : 0);
            }
            case 11:
                shared = a;
                return (firstCaller() ? 2 : 0) + (secondCaller() ? 1 : 0);
            case 12:
                a.set(true);
                return (a.compareAndSet(false, false) ? 2 : 0) + (a.get() ? 1 : 0);
            default: throw new IllegalArgumentException();
        }
    }
    public static String text(int which) {
        switch (which) {
            case 0: return new AtomicBoolean().toString();
            case 1: return new AtomicBoolean(true).toString();
            case 2: return ((Object)new AtomicBoolean(true)).toString();
            case 3: return ((AtomicBoolean)new Child(false)).toString();
            case 4: return new Child(true).baseText();
            case 5: return new AtomicInteger(-123).toString();
            case 6: return ((Object)new AtomicLong(Long.MIN_VALUE)).toString();
            case 7: return ((AtomicInteger)new IntChild(9)).toString();
            case 8: return new IntChild(9).baseText();
            case 9: return ((AtomicLong)new LongChild(12)).toString();
            case 10: return new LongChild(12).baseText();
            default: throw new IllegalArgumentException();
        }
    }
    public static long integer(int which) {
        AtomicInteger a = new AtomicInteger(7);
        switch (which) {
            case 0: return new AtomicInteger().get();
            case 1: a.set(Integer.MAX_VALUE); return a.incrementAndGet();
            case 2: a.set(Integer.MIN_VALUE); return a.decrementAndGet();
            case 3: return a.getAndIncrement() * 100L + a.get();
            case 4: return a.getAndDecrement() * 100L + a.get();
            case 5: return a.getAndAdd(-15) * 100L + a.get();
            case 6: return a.addAndGet(Integer.MAX_VALUE);
            case 7: a.lazySet(-9); return a.getAndSet(11) * 100L + a.get();
            case 8: return (a.compareAndSet(7, 9) ? 100 : 0) + a.get();
            case 9: return (a.compareAndSet(6, 9) ? 100 : 0) + a.get();
            case 10: while (!a.weakCompareAndSet(7, 8)) {} return a.get();
            case 11: return ((Number)new AtomicInteger(-129)).byteValue();
            case 12: return ((Number)new AtomicInteger(65535)).shortValue();
            case 13: return ((Number)new AtomicInteger(-123456)).longValue();
            case 14: return (long)new AtomicInteger(16777217).floatValue();
            case 15: return (long)new AtomicInteger(Integer.MAX_VALUE).doubleValue();
            case 16: IntChild b = new IntChild(3); b.addAndGet(2); return b.value * 100L + b.get();
            case 17: return ((Number)new IntChild(3)).byteValue();
            case 18: System.gc(); return a.get();
            case 19: return new AtomicInteger(4).get() * 100L + a.get();
            default: throw new IllegalArgumentException();
        }
    }
    public static long longer(int which) {
        AtomicLong a = new AtomicLong(7);
        switch (which) {
            case 0: return new AtomicLong().get();
            case 1: a.set(Long.MAX_VALUE); return a.incrementAndGet();
            case 2: a.set(Long.MIN_VALUE); return a.decrementAndGet();
            case 3: return a.getAndIncrement() * 100L + a.get();
            case 4: return a.getAndDecrement() * 100L + a.get();
            case 5: return a.getAndAdd(-15) * 100L + a.get();
            case 6: return new AtomicLong(Long.MAX_VALUE).addAndGet(7);
            case 7: a.lazySet(-9); return a.getAndSet(11) * 100L + a.get();
            case 8: return (a.compareAndSet(7, Long.MIN_VALUE) ? 100 : 0) + a.get();
            case 9: return (a.compareAndSet(6, 9) ? 100 : 0) + a.get();
            case 10: while (!a.weakCompareAndSet(7, 8)) {} return a.get();
            case 11: return ((Number)new AtomicLong(4294967295L)).intValue();
            case 12: return ((Number)new AtomicLong(Long.MAX_VALUE)).byteValue();
            case 13: return ((Number)new AtomicLong(Long.MAX_VALUE)).shortValue();
            case 14: return (long)new AtomicLong(16777217).floatValue();
            case 15: return (long)new AtomicLong(9007199254740993L).doubleValue();
            case 16: LongChild b = new LongChild(3); b.addAndGet(2); return b.get();
            case 17: System.gc(); return a.get();
            default: throw new IllegalArgumentException();
        }
    }
    public static AtomicBoolean make(boolean value) { return new AtomicBoolean(value); }
    public static boolean swap(AtomicBoolean value, boolean expect, boolean update) {
        return value.compareAndSet(expect, update);
    }
    public static void main(String[] args) {
        for (int i = 0; i < 13; i++) System.out.println("run\t" + i + "\t" + run(i));
        for (int i = 0; i < 20; i++) System.out.println("integer\t" + i + "\t" + integer(i));
        for (int i = 0; i < 18; i++) System.out.println("longer\t" + i + "\t" + longer(i));
        for (int i = 0; i < 11; i++) System.out.println("text\t" + i + "\t" + text(i));
    }
}
