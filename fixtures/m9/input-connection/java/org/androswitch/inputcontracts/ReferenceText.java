package org.androswitch.inputcontracts;
/**
 * Source-derived API26 POINT-selection model using independent JDK UTF16 text.
 * This is not execution of Android's framework or a reference Android device.
 */
public final class ReferenceText {
  StringBuilder text = new StringBuilder();
  int start = 0, end = 0;
  int point(int value, int a, int b, int inserted) {
    if (value < 0 || value < a)
      return value;
    if (a == b || value >= b)
      return value - (b - a) + inserted;
    if (value == a || inserted == 0)
      return a;
    return a + (int) ((long) (value - a) * inserted / (b - a));
  }
  void replace(int a, int b, String value) {
    int x = point(start, a, b, value.length()), y = point(end, a, b, value.length());
    text.replace(a, b, value);
    start = x;
    end = y;
  }
  void select(int a, int b) {
    if (a >= 0 && b >= 0 && a <= text.length() && b <= text.length()) {
      start = a;
      end = b;
    }
  }
  void commit(String value, int cursor) {
    int a = Math.max(0, Math.min(start, end)), b = Math.max(0, Math.max(start, end));
    int next = cursor > 0 ? cursor + b - 1 : cursor + a;
    next = Math.max(0, Math.min(next, text.length()));
    start = end = next;
    replace(a, b, value);
  }
  void delete(int before, int after) {
    int a = Math.min(start, end), b = Math.max(start, end), removed = 0;
    if (before > 0) {
      int from = Math.max(0, a - before);
      replace(from, a, "");
      removed = a - from;
    }
    if (after > 0) {
      b -= removed;
      replace(b, Math.min(text.length(), b + after), "");
    }
  }
  String snapshot() {
    return text.toString() + "|" + start + "|" + end;
  }
  static String run(int index) {
    ReferenceText value = new ReferenceText();
    if (index == 0)
      return "true";
    if (index == 17)
      return "false:false:true";
    if (index == 19)
      return "true:false:false:true";
    if (index == 20)
      return "";
    if (index == 15)
      return "\ud83d\ude00";
    if (index == 16) {
      String text = "A\ud83d\ude00B";
      return text.length() + ":" + (int) text.charAt(1) + ":" + (int) text.charAt(2);
    }
    if (index == 24)
      return "wxyz".substring(1, 3) + ":-1";
    value.commit("abcdef", 1);
    switch (index) {
      case 1:
        return value.snapshot();
      case 2:
        value.select(2, 4);
        value.commit("XY", 1);
        return value.snapshot();
      case 3:
        value.select(4, 2);
        value.commit("XY", 1);
        return value.snapshot();
      case 4:
        value.select(2, 2);
        value.commit("Q", 0);
        return value.snapshot();
      case 5:
        value.select(2, 2);
        value.commit("Q", -1);
        return value.snapshot();
      case 6:
        value.select(3, 3);
        value.delete(2, 1);
        return value.snapshot();
      case 7:
        value.select(2, 4);
        value.delete(10, 10);
        return value.snapshot();
      case 8:
        value.select(-1, 99);
        return value.snapshot();
      case 9:
        return value.text.substring(0, 2);
      case 10:
        return value.text.substring(4);
      case 11:
      case 12:
        return value.text.substring(2, 4);
      case 13:
        return "true";
      case 14:
        value.replace(0, value.text.length(), "");
        return value.snapshot();
      case 18:
        return "true:" + value.snapshot();
      case 21:
        return value.text.substring(0, 2);
      case 22:
        return value.text.subSequence(1, 3).toString();
      case 23:
        return (int) value.text.charAt(1) + ":" + (int) value.text.charAt(2);
      case 25:
        value.replace(6, 6, ";");
        return value.snapshot();
      case 26:
        return "java.lang.IndexOutOfBoundsException";
      default:
        throw new IllegalArgumentException("case index");
    }
  }
  public static void main(String[] args) {
    for (int i = 0; i < 27; i++) System.out.println(i + "\t" + run(i));
  }
}
