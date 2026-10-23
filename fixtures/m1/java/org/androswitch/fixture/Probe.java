package org.androswitch.fixture;

/** Ordinary Java compiled once for both JVM reference and unmodified D8 APK. */
public class Probe {
    private static int state = initial();
    private static int initial() { return 41; }
    public static int next() { return ++state; }
    public static int arithmetic(int seed) {
        int result = seed;
        for (int i = 0; i < 20; i++) result = (i % 3 == 0) ? result * 7 - i : result ^ (i << 2);
        return result;
    }
    public static int arrays(int size) {
        int[] items = new int[size];
        for (int i = 0; i < size; i++) items[i] = i * i - 3;
        int sum = 0;
        for (int item : items) sum += item;
        return sum;
    }
    static class Base { int value; Base(int value) { this.value = value; } int read() { return value; } }
    static class Child extends Base { Child(int value) { super(value); } int read() { return super.read() * 3; } }
    public static int objects(int value) { Base counter = new Child(value); return counter.read() + 2; }
    public static int caught(int divisor) {
        try { return 120 / divisor; } catch (ArithmeticException error) { return -77; }
    }
    public static long wide(long value, int shift) { return (value >>> shift) ^ 0x123456789abcdef0L; }
    public static double floating(double x, float y) { return x * y + 0.125; }
    public static int selection(int key) {
        switch (key) { case -3: return 8; case 7: return 70; case 200: return -4; default: return 19; }
    }
    public static int chain(int seed) { return arithmetic(seed) + arrays(8); }
    public static long unavailable() { return System.currentTimeMillis(); }
    public static Object platformField() { return System.in; }
    public static int monitor() { synchronized (Probe.class) { return 7; } }
    public static native int nativeMissing();
    public static int spin() { while (true) {} }
    public static int recurse(int depth) { return recurse(depth + 1); }

    public static void main(String[] args) {
        System.out.println("arithmetic=" + arithmetic(-123));
        System.out.println("arrays=" + arrays(32));
        System.out.println("objects=" + objects(14));
        System.out.println("caught-zero=" + caught(0));
        System.out.println("caught-value=" + caught(5));
        System.out.println("wide=" + wide(-123456789012345L, 17));
        System.out.println("floating=" + floating(1.75, -2.5f));
        System.out.println("switch-hit=" + selection(200));
        System.out.println("switch-miss=" + selection(4));
        System.out.println("chain=" + chain(3));
        System.out.println("static-first=" + next());
        System.out.println("static-second=" + next());
    }
}
