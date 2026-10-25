package org.androswitch.conformance;
public final class PureCases {
  public static final class Fields {
    public static final int number = 61;
    public String label = "field";
    private int secret = 47;
    public short small = 12;
    public String hello(String value) {
      return "hi " + value;
    }
  }
  public static String run(int which) throws Exception {
    switch (which) {
      case 0: {
        return Integer.toString(Integer.MAX_VALUE + 1);
      }
      case 1: {
        int n = -17;
        return (n / 3) + ":" + (n % 3);
      }
      case 2: {
        int n = -1;
        return (n >>> 1) + ":" + (1 << 33);
      }
      case 3: {
        long n = Long.MAX_VALUE;
        return Long.toString(n + 1);
      }
      case 4: {
        int n = 0;
        for (int i = 0; i < 100; i++) n += i;
        return Integer.toString(n);
      }
      case 5: {
        int n = 7;
        switch (n) {
          case 1:
            return "one";
          case 7:
            return "seven";
          default:
            return "other";
        }
      }
      case 6: {
        return Double.toString(1.25 + 2.5);
      }
      case 7: {
        return Boolean.toString(Double.NaN != Double.NaN);
      }
      case 8: {
        return Boolean.toString(1.0 / -0.0 == Double.NEGATIVE_INFINITY);
      }
      case 9: {
        double d = -3.75;
        return Integer.toString((int) d);
      }
      case 10: {
        try {
          int[] a = new int[1];
          return Integer.toString(a[2]);
        } catch (ArrayIndexOutOfBoundsException e) {
          return "bounds";
        }
      }
      case 11: {
        try {
          int z = 0;
          return Integer.toString(7 / z);
        } catch (ArithmeticException e) {
          return "arithmetic";
        }
      }
      case 12: {
        return Integer.toString("A\uD83D\uDE00Z".length());
      }
      case 13: {
        return "abcd".substring(1, 3);
      }
      case 14: {
        return Integer.toString("\u20ac".charAt(0));
      }
      case 15: {
        return Boolean.toString("abc".equals(new String("abc")));
      }
      case 16: {
        return new StringBuilder().append('x').append(',').append(true).append(42).toString();
      }
      case 17: {
        return new StringBuffer().append("one").append(2).toString();
      }
      case 18: {
        byte[] b = "A\u20ac".getBytes("UTF-8");
        return b.length + ":" + (b[1] & 255);
      }
      case 19: {
        byte[] b = "ABC".getBytes("US-ASCII");
        return b.length + ":" + b[2];
      }
      case 20: {
        return String.format(java.util.Locale.ROOT, "%s:%04d:%%", "x", 7);
      }
      case 21: {
        return "banana".indexOf("na") + ":"
            + "banana".lastIndexOf("na");
      }
      case 22: {
        return "a-b-c".replace('-', '+');
      }
      case 23: {
        return "android".startsWith("and") + ":"
            + "android".endsWith("oid");
      }
      case 24: {
        return new StringBuilder().append((String) null).toString();
      }
      case 25: {
        String s = "\uD83D\uDE00";
        return Integer.toString(s.codePointAt(0));
      }
      case 26: {
        java.util.ArrayList<String> x = new java.util.ArrayList<String>();
        x.add("a");
        x.add("b");
        x.remove(0);
        return x.get(0) + ":" + x.size();
      }
      case 27: {
        java.util.ArrayList<String> x = new java.util.ArrayList<String>();
        x.add(null);
        return Boolean.toString(x.contains(null));
      }
      case 28: {
        java.util.ArrayList<Integer> x = new java.util.ArrayList<Integer>();
        x.add(3);
        x.add(5);
        int n = 0;
        for (Integer v : x) n += v;
        return Integer.toString(n);
      }
      case 29: {
        java.util.HashMap<String, Integer> m = new java.util.HashMap<String, Integer>();
        m.put("x", 3);
        m.put("x", 7);
        return m.size() + ":" + m.get("x");
      }
      case 30: {
        java.util.HashSet<String> s = new java.util.HashSet<String>();
        s.add("x");
        s.add("x");
        return Integer.toString(s.size());
      }
      case 31: {
        java.util.concurrent.CopyOnWriteArrayList<String> x =
            new java.util.concurrent.CopyOnWriteArrayList<String>();
        x.add("a");
        java.util.Iterator<String> i = x.iterator();
        x.add("b");
        return i.next() + ":" + i.hasNext() + ":" + x.size();
      }
      case 32: {
        String[] a = {"a"};
        java.util.concurrent.CopyOnWriteArrayList<String> x =
            new java.util.concurrent.CopyOnWriteArrayList<String>(a);
        a[0] = "b";
        return x.get(0);
      }
      case 33: {
        java.util.concurrent.CopyOnWriteArrayList<String> x =
            new java.util.concurrent.CopyOnWriteArrayList<String>();
        x.add("a");
        java.util.Iterator<String> i = x.iterator();
        i.next();
        try {
          i.remove();
          return "bad";
        } catch (UnsupportedOperationException e) {
          return "unsupported";
        }
      }
      case 34: {
        return Integer.valueOf(200).equals(Integer.valueOf(200)) + ":"
            + Long.valueOf(7).equals(Integer.valueOf(7));
      }
      case 35: {
        int[] a = {1, 2, 3, 4};
        System.arraycopy(a, 0, a, 1, 3);
        return a[0] + ":" + a[1] + ":" + a[2] + ":" + a[3];
      }
      case 36: {
        int[] a = {1, 2};
        int[] b = a.clone();
        b[0] = 9;
        return a[0] + ":" + b[0];
      }
      case 37: {
        java.util.AbstractCollection<String> c = new java.util.AbstractCollection<String>() {
          public int size() {
            return 1;
          }
          public java.util.Iterator<String> iterator() {
            return java.util.Arrays.asList("x").iterator();
          }
        };
        return c.isEmpty() + ":" + c.contains("x");
      }
      case 38: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[0];
        return t.name() + ":" + t.ordinal();
      }
      case 39: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[1];
        return t.name() + ":" + t.ordinal();
      }
      case 40: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[2];
        return t.name() + ":" + t.ordinal();
      }
      case 41: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[3];
        return t.name() + ":" + t.ordinal();
      }
      case 42: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[4];
        return t.name() + ":" + t.ordinal();
      }
      case 43: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[5];
        return t.name() + ":" + t.ordinal();
      }
      case 44: {
        java.util.concurrent.TimeUnit t = java.util.concurrent.TimeUnit.values()[6];
        return t.name() + ":" + t.ordinal();
      }
      case 45: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.NANOSECONDS));
      }
      case 46: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.NANOSECONDS));
      }
      case 47: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.NANOSECONDS));
      }
      case 48: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MICROSECONDS));
      }
      case 49: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MICROSECONDS));
      }
      case 50: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.MICROSECONDS));
      }
      case 51: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MILLISECONDS));
      }
      case 52: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MILLISECONDS));
      }
      case 53: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.MILLISECONDS));
      }
      case 54: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.SECONDS));
      }
      case 55: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.SECONDS));
      }
      case 56: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.SECONDS));
      }
      case 57: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MINUTES));
      }
      case 58: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.MINUTES));
      }
      case 59: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.MINUTES));
      }
      case 60: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.HOURS));
      }
      case 61: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.HOURS));
      }
      case 62: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.HOURS));
      }
      case 63: {
        return Long.toString(java.util.concurrent.TimeUnit.NANOSECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.DAYS));
      }
      case 64: {
        return Long.toString(java.util.concurrent.TimeUnit.SECONDS.convert(
            123456789L, java.util.concurrent.TimeUnit.DAYS));
      }
      case 65: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.convert(
            123456789L, java.util.concurrent.TimeUnit.DAYS));
      }
      case 66: {
        return Long.toString(java.util.concurrent.TimeUnit.DAYS.toNanos(Long.MAX_VALUE));
      }
      case 67: {
        try {
          throw new IllegalStateException("detail");
        } catch (IllegalStateException e) {
          return e.getMessage();
        }
      }
      case 68: {
        int n = 1;
        try {
          n = 2;
        } finally {
          n += 3;
        }
        return Integer.toString(n);
      }
      case 69: {
        try {
          Object o = null;
          return o.toString();
        } catch (NullPointerException e) {
          return "null";
        }
      }
      case 70: {
        try {
          Object x = "x";
          Integer n = (Integer) x;
          return n.toString();
        } catch (ClassCastException e) {
          return "cast";
        }
      }
      case 71: {
        return Boolean.toString(new RuntimeException().getMessage() == null);
      }
      case 72: {
        return PureCases.class.getName();
      }
      case 73: {
        return Boolean.toString(Runnable.class.isInterface());
      }
      case 74: {
        return Number.class.isAssignableFrom(Integer.class) + ":"
            + String.class.isAssignableFrom(Integer.class);
      }
      case 75: {
        return Integer.toString(Fields.class.getDeclaredField("number").getInt(null));
      }
      case 76: {
        return (String) Fields.class.getDeclaredField("label").get(new Fields());
      }
      case 77: {
        java.lang.reflect.Field f = Fields.class.getDeclaredField("secret");
        f.setAccessible(true);
        return Integer.toString(f.getInt(new Fields()));
      }
      case 78: {
        try {
          Fields.class.getDeclaredField("missing");
          return "bad";
        } catch (NoSuchFieldException e) {
          return "missing";
        }
      }
      case 79: {
        return Long.toString(Fields.class.getDeclaredField("small").getLong(new Fields()));
      }
      case 80: {
        return (String) Fields.class.getDeclaredMethod("hello", String.class)
            .invoke(new Fields(), "you");
      }
      case 81: {
        ThreadLocal<Integer> t = new ThreadLocal<Integer>() {
          protected Integer initialValue() {
            return 7;
          }
        };
        return Integer.toString(t.get());
      }
      case 82: {
        ThreadLocal<Integer> t = new ThreadLocal<Integer>() {
          protected Integer initialValue() {
            return 7;
          }
        };
        t.set(null);
        return Boolean.toString(t.get() == null);
      }
      case 83: {
        ThreadLocal<Integer> t = new ThreadLocal<Integer>() {
          protected Integer initialValue() {
            return 7;
          }
        };
        t.set(9);
        t.remove();
        return Integer.toString(t.get());
      }
      case 84: {
        Object lock = new Object();
        synchronized (lock) {
          synchronized (lock) {
            return "reentrant";
          }
        }
      }
      case 85: {
        java.util.concurrent.atomic.AtomicBoolean a =
            new java.util.concurrent.atomic.AtomicBoolean(false);
        return a.compareAndSet(false, true) + ":" + a.get();
      }
      case 86: {
        return Integer.toString(Math.abs(-3));
      }
      case 87: {
        return Integer.toString(Math.abs(-1));
      }
      case 88: {
        return Integer.toString(Math.abs(0));
      }
      case 89: {
        return Integer.toString(Math.abs(1));
      }
      case 90: {
        return Integer.toString(Math.abs(3));
      }
      case 91: {
        return Integer.toHexString(0);
      }
      case 92: {
        return Integer.toHexString(1);
      }
      case 93: {
        return Integer.toHexString(127);
      }
      case 94: {
        return Integer.toHexString(128);
      }
      case 95: {
        return Integer.toHexString(255);
      }
      case 96: {
        return Integer.toHexString(32767);
      }
      default:
        throw new IllegalArgumentException("unknown pure case");
    }
  }
  public static void main(String[] args) throws Exception {
    for (int i = 0; i < 97; i++) System.out.println(i + "\t" + run(i));
  }
}
