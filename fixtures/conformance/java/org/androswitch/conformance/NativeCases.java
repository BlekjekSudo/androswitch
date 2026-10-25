package org.androswitch.conformance;
public final class NativeCases {
  private static boolean loaded;
  private static synchronized void load() {
    if (!loaded) {
      System.loadLibrary("asconformance");
      loaded = true;
    }
  }
  private static native String sharedFile(String path);
  private static native long integer(int mode);
  private static native double mixed(double a, float b, int c, long d);
  private static native String echo(String input);
  private static native int[] array(int[] input);
  private static native void raise();
  private static native int callback(int value);
  private static native String facts(int mode);
  private static native String graphics(int mode);
  private static native String vulkan(int mode);
  private static int reenter(int value) {
    Object[] roots = new Object[] {new Object(), "callback"};
    System.gc();
    if (roots.length != 2)
      throw new AssertionError();
    return value * 7;
  }
  public static String run(int n) throws Exception {
    load();
    switch (n) {
      case 0:
        AndroidCases.check(integer(0) == 8, "64bit native pointer");
        return "8";
      case 1:
        AndroidCases.check(mixed(1.25, 2.5f, 3, 4L) == 10.75, "mixed JNI ABI");
        return "10.75";
      case 2:
        String x = "A\u20acZ";
        AndroidCases.check(x.equals(echo(x)), "UTF8 JNI string");
        return x;
      case 3:
        int[] a = {1, -2, 3};
        int[] b = array(a);
        AndroidCases.check(a[0] == 2 && a[1] == -4 && b[2] == 6, "array copyback");
        return "2,-4,6";
      case 4:
        try {
          raise();
          throw new AssertionError("missing native exception");
        } catch (IllegalStateException e) {
          AndroidCases.check(
              "native-exception".equals(e.getMessage()), "actual native exception message");
          return e.getMessage();
        }
      case 5:
        AndroidCases.check(callback(37) == 259, "native callback across GC");
        return "259";
      case 6:
        AndroidCases.check(integer(1) == 1, "JNI local frame");
        return "local frame";
      case 7:
        AndroidCases.check(integer(2) == 21, "native attach/callback/detach");
        return "21";
      case 8:
        java.io.File f = new java.io.File(RunBook.activity.getFilesDir(), "jni-shared.bin");
        java.io.FileOutputStream o = new java.io.FileOutputStream(f);
        o.write(47);
        o.close();
        String result = sharedFile(f.getAbsolutePath());
        java.io.FileInputStream in = new java.io.FileInputStream(f);
        int actual = in.read();
        in.close();
        AndroidCases.check(actual == 83, "native write visible to Java");
        return result;
      default:
        if (n < 18)
          return facts(n - 9);
        if (n < 27)
          return graphics(n - 18);
        return vulkan(n - 27);
    }
  }
}
