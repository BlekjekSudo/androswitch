package org.androswitch.vm;
import java.util.concurrent.atomic.AtomicBoolean;

/** Same API 26-compatible source for a real JVM oracle and unmodified DEX. */
public final class AtomicFixture {
    public static final class Child extends AtomicBoolean {
        public int value = 43;
        Child(boolean value) { super(value); }
        public String toString() { return "DEX_OVERRIDE"; }
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
            default: throw new IllegalArgumentException();
        }
    }
    public static AtomicBoolean make(boolean value) { return new AtomicBoolean(value); }
    public static boolean swap(AtomicBoolean value, boolean expect, boolean update) {
        return value.compareAndSet(expect, update);
    }
    public static void main(String[] args) {
        for (int i = 0; i < 13; i++) System.out.println("run\t" + i + "\t" + run(i));
        for (int i = 0; i < 5; i++) System.out.println("text\t" + i + "\t" + text(i));
    }
}
