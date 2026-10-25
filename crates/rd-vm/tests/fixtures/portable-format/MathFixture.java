package org.androswitch.vm;

/** Scalar Math/StrictMath oracle; IEEE exceptional values and integer bounds. */
public final class MathFixture {
    public static int ints(int op, int a, int b) {
        switch (op) {
        case 0: return Math.min(a,b);
        case 1: return Math.max(a,b);
        case 2: return StrictMath.min(a,b);
        default: return StrictMath.max(a,b);
        }
    }
    public static long longs(int op, long a, long b) {
        switch (op) {
        case 0: return Math.min(a,b);
        case 1: return Math.max(a,b);
        case 2: return StrictMath.min(a,b);
        default: return StrictMath.max(a,b);
        }
    }
    public static float floats(int op, float a, float b) {
        switch (op) {
        case 0: return Math.min(a,b);
        case 1: return Math.max(a,b);
        case 2: return StrictMath.min(a,b);
        default: return StrictMath.max(a,b);
        }
    }
    public static double doubles(int op, double a, double b) {
        switch (op) {
        case 0: return Math.min(a,b);
        case 1: return Math.max(a,b);
        case 2: return StrictMath.min(a,b);
        default: return StrictMath.max(a,b);
        }
    }
    private static void row(String type, int op, long a, long b, long result) {
        System.out.println(type+"\t"+op+"\t"+a+"\t"+b+"\t"+result);
    }
    public static void main(String[] ignored) {
        int[] iv={Integer.MIN_VALUE,Integer.MAX_VALUE,0,1,-1,7,-7,123};
        long[] jv={Long.MIN_VALUE,Long.MAX_VALUE,0,1,-1,7,-7,123};
        float[] fv={Float.NEGATIVE_INFINITY,-1,-0.0f,0.0f,1,Float.POSITIVE_INFINITY,Float.NaN,Float.MIN_VALUE};
        double[] dv={Double.NEGATIVE_INFINITY,-1,-0.0,0.0,1,Double.POSITIVE_INFINITY,Double.NaN,Double.MIN_VALUE};
        for(int op=0;op<4;op++) {
            for(int a:iv) for(int b:iv) row("ints",op,a,b,ints(op,a,b));
            for(long a:jv) for(long b:jv) row("longs",op,a,b,longs(op,a,b));
            for(float a:fv) for(float b:fv) row("floats",op,Float.floatToRawIntBits(a),Float.floatToRawIntBits(b),Float.floatToRawIntBits(floats(op,a,b)));
            for(double a:dv) for(double b:dv) row("doubles",op,Double.doubleToRawLongBits(a),Double.doubleToRawLongBits(b),Double.doubleToRawLongBits(doubles(op,a,b)));
        }
    }
}
