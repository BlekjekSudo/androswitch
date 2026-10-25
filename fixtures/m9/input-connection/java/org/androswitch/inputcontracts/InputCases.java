package org.androswitch.inputcontracts;
import android.content.Context;
import android.content.ContextWrapper;
import android.opengl.GLSurfaceView;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;

/** Original normal SDK inputs. No proprietary implementation or native hook. */
public final class InputCases {
  public static Context context;
  public static Object kept;
  public static class TrackingText extends SpannableStringBuilder {
    public int replaced;
    public boolean fail;
    @Override
    public SpannableStringBuilder replace(int a, int b, CharSequence source, int x, int y) {
      replaced++;
      System.gc();
      if (fail)
        throw new IllegalStateException("replace callback failed");
      return super.replace(a, b, source, x, y);
    }
  }
  public static class TrackingConnection extends BaseInputConnection {
    public final TrackingText text = new TrackingText();
    public int gets, begins, ends;
    public TrackingConnection(View view, boolean full) {
      super(view, full);
      Selection.setSelection(text, 0);
    }
    @Override
    public Editable getEditable() {
      gets++;
      System.gc();
      return text;
    }
    @Override
    public boolean beginBatchEdit() {
      begins++;
      return false;
    }
    @Override
    public boolean endBatchEdit() {
      ends++;
      return false;
    }
  }
  public static class NoEditable extends BaseInputConnection {
    public NoEditable(View view) {
      super(view, true);
    }
    @Override
    public Editable getEditable() {
      return null;
    }
  }
  public static class CountingContext extends ContextWrapper {
    public int services;
    public CountingContext(Context base) {
      super(base);
    }
    @Override
    public Object getSystemService(String name) {
      services++;
      System.gc();
      if ("input_method".equals(name))
        return null;
      throw new IllegalStateException("unexpected service");
    }
  }
  public static class ThrowingContext extends ContextWrapper {
    public ThrowingContext(Context base) {
      super(base);
    }
    @Override
    public Object getSystemService(String name) {
      throw new IllegalStateException("service callback failed");
    }
  }
  public static BaseInputConnection make(boolean full) {
    return new BaseInputConnection(new View(context), full);
  }
  public static String snapshot(BaseInputConnection input) {
    Editable text = input.getEditable();
    return text.toString() + "|" + Selection.getSelectionStart(text) + "|"
        + Selection.getSelectionEnd(text);
  }
  public static String run(int index) {
    BaseInputConnection input = make(true);
    if (index == 0)
      return "" + (input.getEditable() == input.getEditable());
    if (index == 17)
      return input.beginBatchEdit() + ":" + input.endBatchEdit() + ":"
          + (input.getHandler() == null);
    if (index == 19) {
      BaseInputConnection absent = new NoEditable(new View(context));
      return absent.commitText("x", 1) + ":" + absent.setSelection(0, 0) + ":"
          + absent.deleteSurroundingText(1, 1) + ":" + (absent.getTextBeforeCursor(1, 0) == null);
    }
    if (index == 20)
      return input.getTextBeforeCursor(-1, 0).toString();
    if (index == 15 || index == 16) {
      input.commitText("A\ud83d\ude00B", 1);
      if (index == 15) {
        input.setSelection(3, 3);
        return input.getTextBeforeCursor(2, 0).toString();
      }
      Editable text = input.getEditable();
      return text.length() + ":" + (int) text.charAt(1) + ":" + (int) text.charAt(2);
    }
    if (index == 24) {
      SpannableStringBuilder text = new SpannableStringBuilder("wxyz", 1, 3);
      return text.toString() + ":" + Selection.getSelectionStart(text);
    }
    input.commitText("abcdef", 1);
    switch (index) {
      case 1:
        return snapshot(input);
      case 2:
        input.setSelection(2, 4);
        input.commitText("XY", 1);
        return snapshot(input);
      case 3:
        input.setSelection(4, 2);
        input.commitText("XY", 1);
        return snapshot(input);
      case 4:
        input.setSelection(2, 2);
        input.commitText("Q", 0);
        return snapshot(input);
      case 5:
        input.setSelection(2, 2);
        input.commitText("Q", -1);
        return snapshot(input);
      case 6:
        input.setSelection(3, 3);
        input.deleteSurroundingText(2, 1);
        return snapshot(input);
      case 7:
        input.setSelection(2, 4);
        input.deleteSurroundingText(10, 10);
        return snapshot(input);
      case 8:
        input.setSelection(-1, 99);
        return snapshot(input);
      case 9:
        input.setSelection(2, 4);
        return input.getTextBeforeCursor(99, 0).toString();
      case 10:
        input.setSelection(2, 4);
        return input.getTextAfterCursor(99, 0).toString();
      case 11:
        input.setSelection(2, 4);
        return input.getSelectedText(0).toString();
      case 12:
        input.setSelection(4, 2);
        return input.getSelectedText(0).toString();
      case 13:
        return "" + (input.getSelectedText(0) == null);
      case 14:
        input.getEditable().clear();
        return snapshot(input);
      case 18:
        return input.finishComposingText() + ":" + snapshot(input);
      case 21:
        Selection.removeSelection(input.getEditable());
        return input.getTextAfterCursor(2, 0).toString();
      case 22:
        return input.getEditable().subSequence(1, 3).toString();
      case 23: {
        char[] chars = new char[2];
        input.getEditable().getChars(1, 3, chars, 0);
        return (int) chars[0] + ":" + (int) chars[1];
      }
      case 25:
        input.getEditable().append(';');
        return snapshot(input);
      case 26:
        try {
          input.getEditable().delete(2, 99);
          return "NO_EXCEPTION";
        } catch (IndexOutOfBoundsException expected) {
          return expected.getClass().getName();
        }
      default:
        throw new IllegalArgumentException("case index");
    }
  }
  public static TrackingConnection tracking(boolean full) {
    return new TrackingConnection(new View(context), full);
  }
  public static BaseInputConnection overrideContext() {
    CountingContext alternate = new CountingContext(context);
    View view = new View(alternate);
    BaseInputConnection input = new BaseInputConnection(view, true);
    if (view.getContext() != alternate || alternate.services != 1)
      throw new IllegalStateException("context identity and virtual service callback");
    return input;
  }
  public static BaseInputConnection throwingContext() {
    return new BaseInputConnection(new View(new ThrowingContext(context)), true);
  }
  public static BaseInputConnection glTarget() {
    return new BaseInputConnection(new GLSurfaceView(context), true);
  }
  public static int callbackProof() {
    TrackingConnection input = tracking(true);
    input.commitText("callback", 1);
    if (input.text.replaced != 1 || input.begins != 1 || input.ends != 1 || input.gets < 1)
      throw new IllegalStateException("replace/batch callbacks");
    if (!"callback".equals(input.text.toString()))
      throw new IllegalStateException("actual replacement data");
    return input.text.replaced;
  }
  public static Object[] retention() {
    CountingContext owned = new CountingContext(context);
    View view = new View(owned);
    BaseInputConnection input = new BaseInputConnection(view, true);
    return new Object[] {input, view, owned};
  }
  public static BaseInputConnection nullTarget() {
    return new BaseInputConnection(null, true);
  }
  public static class OddSequence implements CharSequence {
    public int lengths, chars;
    public int length() {
      lengths++;
      System.gc();
      return 1;
    }
    public char charAt(int index) {
      chars++;
      System.gc();
      if (index != 0)
        throw new IndexOutOfBoundsException();
      return '\ud800';
    }
    public CharSequence subSequence(int a, int b) {
      throw new UnsupportedOperationException();
    }
    public String toString() {
      return "WRONG_COERCION";
    }
  }
  public static Object[] unpaired() {
    BaseInputConnection input = make(true);
    OddSequence source = new OddSequence();
    input.commitText(source, 1);
    return new Object[] {input, source};
  }
}
