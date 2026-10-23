package org.androswitch.fixture;
public final class RustBridge {
    static { System.loadLibrary("rustbridge"); }
    private static native long calculate(int a, long b, double c);
    public static long exercise() { return calculate(-19, 1234567890123L, 7.5); }
}
