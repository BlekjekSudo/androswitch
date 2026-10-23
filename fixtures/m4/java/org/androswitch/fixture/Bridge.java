package org.androswitch.fixture;

/** Unmodified JNI fixture, run by the JVM oracle and the DEX runtime. */
public final class Bridge {
    static { System.loadLibrary("bridge"); }
    private static native double mixed(int a, long b, float c, double d);
    private static native int registered(int value);
    private static native int nativeReenter(int value);
    private static native String roundtrip(String value);
    private static native int arrays();
    private static native int references();
    private static native void throwing();
    private static native int callbackException();
    private static native int callbackThread(int value);
    private static native int nativeStorage(String file);
    private static native int unsupported();
    private static native double many(int a,int b,int c,int d,int e,int f,int g,int h,int i,double j,double k,double l,double m,double n,double o,double p,double q,double r);
    private static native double nativeFloatCallback();
    private static native double nativeStackCallback();
    public static double stackCallback(int a,int b,int c,int d,int e,int f,int g,int h,int i,double j,double k,double l,double m,double n,double o,double p,double q,double r){return a+b+c+d+e+f+g+h+i+j+k+l+m+n+o+p+q+r;}
    public static double stackCallbackTest(){return nativeStackCallback();}
    public static double manyArguments(){return many(1,2,3,4,5,6,7,8,9,0.25,1.25,2.25,3.25,4.25,5.25,6.25,7.25,8.25);}
    public static double floatCallback(int a,double b,float c,long d){return a+b*c+d;}
    public static double floatCallbackTest(){return nativeFloatCallback();}
    public static double numeric() { return mixed(-7, 1234567890123L, -2.5f, 1.75); }
    public static int registration() { return registered(41); }
    public static int callback(int value) { return value * 9 + 7; }
    public static int reentrant(int value) { return value == 0 ? 2 : nativeReenter(value - 1) + 3; }
    public static int reentry() { return nativeReenter(4); }
    public static int strings() { String value="A\u0000\uD83D\uDE03Z"; String copy=roundtrip(value); return copy.equals(value) ? copy.length() : -1; }
    public static int arrayTest() { return arrays(); }
    public static int referenceTest() { return references(); }
    public static void collect() { System.gc(); }
    public static int raise() { int zero = 0; return 41 / zero; }
    public static int exceptionTest() { try { throwing(); return -1; } catch (IllegalArgumentException expected) { return 73; } }
    public static int callbackExceptionTest() { return callbackException(); }
    public static int threadTest() { return callbackThread(11); }
    public static int missingApiTest() { return unsupported(); }
    public static int storageTest() throws Exception {
        String path = new java.io.File(System.getProperty("user.dir"), "shared.bin").getAbsolutePath();
        java.io.FileOutputStream file = new java.io.FileOutputStream(path);
        file.write(37); file.close();
        int nativeValue = nativeStorage(path);
        java.io.FileInputStream input = new java.io.FileInputStream(path);
        int result = input.read(); input.close();
        return nativeValue * 256 + result;
    }
    private static native int synchronization();
    public static int synchronizationTest() {return synchronization();}
    public static void main(String[] args) throws Exception {
        System.out.println("synchronization="+synchronizationTest());
        System.out.println("numeric=" + numeric());
        System.out.println("registration=" + registration());
        System.out.println("reentry=" + reentry());
        System.out.println("strings=" + strings());
        System.out.println("arrays=" + arrayTest());
        System.out.println("references=" + referenceTest());
        System.out.println("exception=" + exceptionTest());
        System.out.println("callback-exception=" + callbackExceptionTest());
        System.out.println("thread=" + threadTest());
        System.out.println("storage=" + storageTest());
        System.out.println("many=" + manyArguments());
        System.out.println("float-callback=" + floatCallbackTest());
        System.out.println("stack-callback=" + stackCallbackTest());
        System.out.println("rust=" + RustBridge.exercise());
    }
}
