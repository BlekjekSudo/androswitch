package org.androswitch.conformance;
public final class PureCases {
  private static final class ProxyWriter extends java.io.Writer {
    final java.io.StringWriter sink = new java.io.StringWriter();
    public void write(char[] c, int off, int n) { sink.write(c, off, n); }
    public void flush() {}
    public void close() {}
  }
  private static final class ProxyReader extends java.io.Reader {
    private final String text;
    private int pos;
    ProxyReader(String value) { text = value; }
    public int read(char[] c, int off, int n) {
      if (n == 0) return 0;
      if (pos == text.length()) return -1;
      int copied = 0;
      while (copied < n && pos < text.length()) c[off + copied++] = text.charAt(pos++);
      return copied;
    }
    public void close() {}
  }
  private static final class ReadList extends java.util.AbstractList<String> {
    public int size() { return 3; }
    public String get(int index) {
      if (index < 0 || index >= 3) throw new IndexOutOfBoundsException();
      return index == 1 ? "b" : "a";
    }
  }
  private static final class ReadCollection extends java.util.AbstractCollection<String> {
    public int size() { return 3; }
    public java.util.Iterator<String> iterator() {
      return java.util.Arrays.asList("a", "b", "a").iterator();
    }
  }
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
      case 97: {
        java.util.Set<String> s = new java.util.HashSet<String>(java.util.Arrays.asList("a", new String("a"), null));
        return s.size() + ":" + s.contains("a") + ":" + s.contains(null);
      }
      case 98: {
        java.util.Collection<String> src = java.util.Arrays.asList("a", "b");
        return new java.util.ArrayList<String>(src).size() + ":"
            + new java.util.LinkedList<String>(src).size() + ":" + new java.util.Vector<String>(src).size()
            + ":" + new java.util.LinkedHashSet<String>(src).size();
      }
      case 99: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        return l.addAll(l) + ":" + l.toString();
      }
      case 100: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b", "a"));
        boolean kept = l.retainAll(java.util.Collections.singleton("a"));
        boolean removed = l.removeAll(java.util.Collections.singleton("a"));
        return kept + ":" + removed + ":" + l.isEmpty();
      }
      case 101: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        String[] dest = new String[] {"x", "x", "x", "x"};
        return (l.toArray(dest) == dest) + ":" + java.util.Arrays.toString(dest);
      }
      case 102: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b", "c", "d"));
        java.util.List<String> v = l.subList(1, 4);
        java.util.List<String> n = v.subList(1, 2);
        System.gc(); n.clear();
        l.set(1, "z");
        return l.toString() + ":" + v.toString();
      }
      case 103: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        java.util.ListIterator<String> it = l.listIterator(1);
        String before = it.previous(); it.remove(); System.gc();
        return before + ":" + it.nextIndex() + ":" + it.next() + ":" + it.hasNext() + ":" + l;
      }
      case 104: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        java.util.ListIterator<String> it = l.listIterator();
        try { it.remove(); return "missing exception"; }
        catch (IllegalStateException expected) { return it.next(); }
      }
      case 105: {
        ReadList src = new ReadList();
        java.util.Set<String> set = new java.util.HashSet<String>(src);
        return set.size() + ":" + set.containsAll(src) + ":" + src.indexOf("b") + ":" + src.lastIndexOf("a");
      }
      case 106: {
        ReadCollection src = new ReadCollection();
        java.util.List<String> dst = new java.util.ArrayList<String>(src);
        return dst.toString() + ":" + src.contains("b") + ":" + src.isEmpty();
      }
      case 107: {
        java.util.List<String> a = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        java.util.List<String> b = new java.util.LinkedList<String>(a);
        return a.equals(b) + ":" + (a.hashCode() == b.hashCode()) + ":" + a;
      }
      case 108: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("b", "a", "c"));
        String range = java.util.Collections.min(l) + ":" + java.util.Collections.max(l);
        java.util.Collections.sort(l); java.util.Collections.reverse(l);
        return range + ":" + l;
      }
      case 109: {
        java.util.Map<String,String> m = java.util.Collections.singletonMap("a", "b");
        try { m.put("x", "y"); return "mutable"; }
        catch (UnsupportedOperationException expected) { return m.get("a"); }
      }
      case 110: {
        java.util.List<String> l = java.util.Collections.unmodifiableList(new java.util.ArrayList<String>());
        try { l.addAll(java.util.Arrays.asList("a")); return "mutable"; }
        catch (UnsupportedOperationException expected) { return Integer.toString(l.size()); }
      }
      case 111: {
        java.util.List<String> l = new java.util.ArrayList<String>();
        boolean changed = java.util.Collections.addAll(l, "a", "b");
        return changed + ":" + l;
      }
      case 112: {
        java.util.List<String> l = new java.util.ArrayList<String>();
        try { l.addAll(1, java.util.Arrays.asList("a")); return "bad index"; }
        catch (IndexOutOfBoundsException expected) { return Integer.toString(l.size()); }
      }
      case 113: {
        try { new java.util.HashSet<String>((java.util.Collection<String>) null); return "bad null"; }
        catch (NullPointerException expected) { return "null"; }
      }
      case 114: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        try { l.subList(-1, 1); return "bad range"; }
        catch (IndexOutOfBoundsException expected) { return Integer.toString(l.size()); }
      }
      case 115: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "b"));
        l.addAll(1, java.util.Arrays.asList("c", "d"));
        return l.toString();
      }
      case 116: {
        java.util.List<String> l = new java.util.ArrayList<String>(java.util.Arrays.asList("a", "c", "b"));
        java.util.Collections.sort(l, new java.util.Comparator<String>() {
          public int compare(String a, String b) { System.gc(); return b.compareTo(a); }
        });
        return l.toString();
      }
      case 117: {
        return "ab".compareTo("az") + ":" + "a".compareTo("aa") + ":" + "\uD83D\uDE00".compareTo("\uE000");
      }
      case 118: {
        return "AbC".compareToIgnoreCase("abc") + ":" + "aBc".equalsIgnoreCase("ABC");
      }
      case 119: {
        return "AbCd".regionMatches(true, 1, "bC", 0, 2) + ":" + "abcd".startsWith("bc", 1);
      }
      case 120: {
        return "abc".contentEquals(new StringBuffer("abc")) + ":" + "abcd".subSequence(1, 3);
      }
      case 121: {
        return String.join("/", new ReadCollection());
      }
      case 122: {
        return "  abc \n".trim() + ":" + "A\uD83D\uDE00Z".codePointAt(1);
      }
      case 123: {
        java.util.regex.Pattern p = java.util.regex.Pattern.compile("ab", java.util.regex.Pattern.CASE_INSENSITIVE);
        return p.flags() + ":" + p.pattern() + ":" + p.matcher("AB").matches();
      }
      case 124: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("(?<name>[a-z]+)(?=!)").matcher("\uD83D\uDE00ok!");
        return m.find() + ":" + m.group("name") + ":" + m.start() + ":" + m.end();
      }
      case 125: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("(a+)").matcher("aa b");
        m.find(); java.util.regex.MatchResult saved = m.toMatchResult();
        m.reset("b"); System.gc();
        return saved.group(1) + ":" + saved.start(1) + ":" + saved.end(1) + ":" + m.find();
      }
      case 126: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("a").matcher("ab");
        m.usePattern(java.util.regex.Pattern.compile("b")); System.gc();
        return m.find() + ":" + m.group() + ":" + m.regionStart() + ":" + m.regionEnd();
      }
      case 127: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("(a)(b)").matcher("ab-ab");
        return m.replaceAll("$2$1") + ":" + m.replaceFirst("$2$1");
      }
      case 128: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("a+").matcher("aa b a");
        StringBuffer buffer = new StringBuffer();
        while (m.find()) { System.gc(); m.appendReplacement(buffer, "x"); }
        m.appendTail(buffer); return buffer.toString();
      }
      case 129: {
        return java.util.Arrays.toString(java.util.regex.Pattern.compile(",").split("a,b,", -1));
      }
      case 130: {
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("a").matcher("a");
        try { m.group(); return "bad state"; }
        catch (IllegalStateException expected) { return "state"; }
      }
      case 131: {
        try { java.util.regex.Pattern.compile("["); return "bad pattern"; }
        catch (java.util.regex.PatternSyntaxException expected) { return "syntax"; }
      }
      case 132: {
        return "aa-bb".matches("[a-z]+-[a-z]+") + ":" + "1-2".replaceAll("(\\d)", "$1$1");
      }
      case 133: {
        java.util.List<Object> a = new java.util.ArrayList<Object>();
        java.util.List<Object> b = new java.util.ArrayList<Object>();
        a.add(b); b.add(a);
        try { a.toString(); return "missing cycle error"; }
        catch (StackOverflowError expected) { a.clear(); b.clear(); return a.toString(); }
      }
      case 134: {
        java.util.List<Object> a = new java.util.ArrayList<Object>();
        java.util.List<Object> b = new java.util.ArrayList<Object>();
        a.add(b); b.add(a);
        int caught=0;
        try { a.hashCode(); } catch (StackOverflowError expected) { caught++; }
        try { a.equals(b); } catch (StackOverflowError expected) { caught++; }
        a.clear(); b.clear();
        return caught + ":" + a.equals(b) + ":" + a.hashCode();
      }
      case 135: {
        java.io.BufferedReader r = new java.io.BufferedReader(new java.io.StringReader("a\r\nb\rc\n"));
        return r.readLine() + ":" + r.readLine() + ":" + r.readLine() + ":" + r.readLine();
      }
      case 136: {
        java.io.StringReader r = new java.io.StringReader("abcd");
        r.read(); r.mark(20); long n = r.skip(2); r.reset();
        return n + ":" + r.read() + ":" + r.ready();
      }
      case 137: {
        java.io.StringReader r = new java.io.StringReader("text"); r.close();
        try { r.read(); return "not closed"; }
        catch (java.io.IOException expected) { return "closed:" + r.markSupported(); }
      }
      case 138: {
        java.io.StringWriter w = new java.io.StringWriter();
        w.write(0xD83D); w.write(0xDE42); return w.toString();
      }
      case 139: {
        java.io.StringWriter s = new java.io.StringWriter();
        java.io.BufferedWriter w = new java.io.BufferedWriter(s);
        w.write("hello", 1, 3); w.flush(); w.close(); return s.toString();
      }
      case 140: {
        java.io.StringWriter w = new java.io.StringWriter();
        w.append('a').append("bcde", 1, 3).append((CharSequence)null); return w.toString();
      }
      case 141: {
        java.io.ByteArrayOutputStream w = new java.io.ByteArrayOutputStream();
        w.write(new byte[]{0, 65, 66, 0}, 1, 2);
        String first = w.toString("UTF-8"); w.reset(); w.write(67);
        return first + ":" + w.size() + ":" + w.toString("UTF-8");
      }
      case 142: {
        java.io.ByteArrayInputStream r = new java.io.ByteArrayInputStream(new byte[]{10,20,30,40});
        r.read(); r.mark(10); long n = r.skip(2); r.reset();
        return n + ":" + r.read() + ":" + r.available();
      }
      case 143: {
        java.io.ByteArrayOutputStream b = new java.io.ByteArrayOutputStream();
        java.io.OutputStreamWriter w = new java.io.OutputStreamWriter(b, "UTF-8");
        w.write(0xD83D); w.flush(); w.write(0xDE42); w.close();
        return new String(b.toByteArray(), "UTF-8");
      }
      case 144: {
        ProxyWriter w = new ProxyWriter(); w.write('A'); w.append('B');
        w.write("CDE", 1, 2); return w.sink.toString();
      }
      case 145: {
        ProxyReader r = new ProxyReader("abc"); char[] c = new char[2];
        int first = r.read(); int n = r.read(c);
        return first + ":" + n + ":" + new String(c);
      }
      case 146: {
        java.io.ByteArrayOutputStream b = new java.io.ByteArrayOutputStream();
        java.io.OutputStreamWriter w = new java.io.OutputStreamWriter(b, "UTF-8");
        System.gc(); w.write("state"); System.gc(); w.flush();
        return b.toString("UTF-8");
      }
      case 147: {
        ProxyWriter p = new ProxyWriter();
        java.io.BufferedWriter w = new java.io.BufferedWriter(p);
        w.write(0xD83D); w.flush(); p.sink.write(0xDE42); return p.sink.toString();
      }
      default:
        throw new IllegalArgumentException("unknown pure case");
    }
  }
  public static void main(String[] args) throws Exception {
    // Write the oracle as UTF-8 bytes, independently of a Windows console's
    // legacy encoding. A surrogate/Unicode test must preserve its real value.
    java.io.PrintWriter out = args.length == 0 ? new java.io.PrintWriter(System.out) :
        new java.io.PrintWriter(new java.io.OutputStreamWriter(new java.io.FileOutputStream(args[0]), "UTF-8"));
    for (int i = 0; i < 148; i++) out.println(i + "\t" + run(i));
    out.flush(); if (args.length != 0) out.close();
  }
}
