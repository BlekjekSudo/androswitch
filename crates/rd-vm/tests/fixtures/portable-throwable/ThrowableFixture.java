package Lorg.androswitch.vm;
public class ThrowableFixture {
  public static int constructors;
  public static class Shadow extends IllegalStateException {
    public String message="shadow";
    public String detailMessage="shadow detail";
    public Shadow(String text) { super(text); constructors++; }
  }
  public static class MessageOverride extends Shadow {
    public MessageOverride(String text) { super(text); }
    public String getMessage() { String base=super.getMessage();return base==null?"override-null":base=="base"?"override-message":"override-other"; }
  }
  public static class LocalizedOverride extends MessageOverride {
    public LocalizedOverride(String text) { super(text); }
    public String getLocalizedMessage() { return super.getLocalizedMessage()=="override-message"?"localized-message":"localized-other"; }
    public String toString() { constructors+=1000;return super.toString(); }
  }
  public static class Middle extends IllegalArgumentException { public Middle(String text) { super(text); } }
  public static class Child extends Middle {
    public Child(String text) { super(text); }
    public String getMessage() { return super.getMessage()=="base"?"child-message":"child-other"; }
  }
  public static class MiddleOverride extends IllegalArgumentException {
    public MiddleOverride(String text) { super(text); }
    public String getMessage() { return super.getMessage()=="base"?"middle-message":"middle-other"; }
  }
  public static class ChildOverride extends MiddleOverride {
    public ChildOverride(String text) { super(text); }
    public String getMessage() { return super.getMessage()=="middle-message"?"child-over-middle":"child-other"; }
  }
  public static Throwable make(int kind) {
    switch(kind) {
      case 0: return new Throwable();
      case 1: return new Throwable((String)null);
      case 2: return new Throwable("");
      case 3: return new Throwable("detail");
      case 4: return new Exception();
      case 5: return new RuntimeException("runtime");
      case 6: return new IllegalStateException("timer task failed");
      case 7: return new Error("error");
      case 8: return new Shadow("base");
      case 9: return new MessageOverride("base");
      case 10: return new LocalizedOverride("base");
      case 11: return new Child("base");
      case 12: return new ChildOverride("base");
      case 13: return new ArrayIndexOutOfBoundsException("index");
      case 14: return new java.io.IOException("io");
      case 15: return new NumberFormatException("number");
      case 16: return new AssertionError();
      default: throw new IllegalArgumentException();
    }
  }
  public static String probe(int kind,int api) {
    Throwable value=make(kind);
    switch(api) { case 0:return value.getMessage();case 1:return value.getLocalizedMessage();default:return value.toString(); }
  }
  public static Throwable shadow(String message) { return new Shadow(message); }
  public static void throwShadow(String message) { throw new Shadow(message); }
  public static void fail() { throw new IllegalStateException("timer task failed"); }
  public static void rethrowGenerated() { try { int zero=0;int value=1/zero; } catch(ArithmeticException expected) { throw expected; } }
  public static String generatedMessage() { try { int zero=0;int value=1/zero;return null; } catch(ArithmeticException expected) { return expected.getMessage(); } }
  public static native void nativeFailure();
  public static String caughtNative() { try { nativeFailure();return null; } catch(IllegalStateException expected) { return expected.getMessage(); } }
  public static Throwable causeConstructor() { return new Exception((Throwable)null); }
  public static Throwable pairConstructor() { return new Exception("cause",null); }
  public static Throwable cause() { return make(6).getCause(); }
  public static StackTraceElement[] stack() { return make(6).getStackTrace(); }
  public static void tick() {}
  public static void main(String[] args) {
    for(int kind=0;kind<17;kind++) for(int api=0;api<3;api++) {
      String value=probe(kind,api);
      System.out.println(kind+"\t"+api+"\t"+(value==null?"<null>":value));
    }
  }
}
