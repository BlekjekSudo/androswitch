package org.androswitch.conformance;
import java.io.*;
final class Files {
  static String read(File f) throws IOException {
    if (!f.exists())
      return "";
    long length = f.length();
    if (length < 0 || length >= 262144)
      throw new IOException("report input too large");
    byte[] bytes = new byte[(int) length + 1];
    FileInputStream in = new FileInputStream(f);
    int n = 0, r;
    try {
      while (n < bytes.length && (r = in.read(bytes, n, bytes.length - n)) > 0) n += r;
      if (n == bytes.length)
        throw new IOException("report input changed while reading");
    } finally {
      in.close();
    }
    StringBuilder b = new StringBuilder();
    for (int i = 0; i < n; i++) {
      int v = bytes[i] & 255;
      if (v > 127)
        throw new IOException("suite journal must be escaped ASCII");
      b.append((char) v);
    }
    return b.toString();
  }
  static void write(File f, String text) throws IOException {
    FileOutputStream o = new FileOutputStream(f);
    try {
      o.write(text.getBytes("UTF-8"));
    } finally {
      o.close();
    }
  }
  static int number(File f, int fallback) throws IOException {
    String s = clean(read(f));
    return s.length() == 0 ? fallback : Integer.parseInt(s);
  }
  static String slice(String s, int from, int to) {
    StringBuilder b = new StringBuilder();
    for (int i = from; i < to; i++) b.append(s.charAt(i));
    return b.toString();
  }
  static String clean(String s) {
    int from = 0, to = s.length();
    while (from < to && s.charAt(from) <= 32) from++;
    while (to > from && s.charAt(to - 1) <= 32) to--;
    return slice(s, from, to);
  }
  static boolean prefix(String s, String prefix) {
    if (s.length() < prefix.length())
      return false;
    for (int i = 0; i < prefix.length(); i++)
      if (s.charAt(i) != prefix.charAt(i))
        return false;
    return true;
  }
  static int find(String s, char c, int start) {
    for (int i = start; i < s.length(); i++)
      if (s.charAt(i) == c)
        return i;
    return -1;
  }
  static int findText(String s, String value) {
    for (int i = 0; i + value.length() <= s.length(); i++) {
      boolean same = true;
      for (int j = 0; j < value.length(); j++)
        if (s.charAt(i + j) != value.charAt(j)) {
          same = false;
          break;
        }
      if (same)
        return i;
    }
    return -1;
  }
  static String resultStatus(String json) {
    String key = "\"status\":\"";
    int at = findText(json, key);
    if (at < 0)
      return "ERROR";
    int begin = at + key.length(), end = find(json, '"', begin);
    return end < 0 ? "ERROR" : slice(json, begin, end);
  }
  static String field(String json, String name) {
    String key = "\"" + name + "\":\"";
    int at = findText(json, key);
    if (at < 0)
      return "";
    int from = at + key.length();
    for (int end = from; end < json.length(); end++) {
      char c = json.charAt(end);
      if (c == '\\')
        end++;
      else if (c == '"')
        return slice(json, from, end);
    }
    return "";
  }
  static long positiveLong(String s) {
    long value = 0;
    for (int i = 0; i < s.length(); i++) {
      int digit = s.charAt(i) - '0';
      if (digit < 0 || digit > 9 || value > (Long.MAX_VALUE - digit) / 10)
        throw new NumberFormatException("invalid nonnegative integer");
      value = value * 10 + digit;
    }
    return value;
  }
  static String quote(String s) {
    if (s == null)
      return "null";
    StringBuilder b = new StringBuilder("\"");
    for (int i = 0; i < s.length(); i++) {
      char c = s.charAt(i);
      if (c == '"' || c == '\\')
        b.append('\\').append(c);
      else if (c == '\n')
        b.append("\\n");
      else if (c == '\r')
        b.append("\\r");
      else if (c == '\t')
        b.append("\\t");
      else if (c < 32 || c > 126) {
        b.append("\\u");
        String digits = "0123456789abcdef";
        for (int n = 12; n >= 0; n -= 4) b.append(digits.charAt((c >>> n) & 15));
      } else
        b.append(c);
    }
    return b.append('"').toString();
  }
}
