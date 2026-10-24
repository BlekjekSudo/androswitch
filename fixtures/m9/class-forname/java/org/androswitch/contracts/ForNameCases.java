package org.androswitch.contracts;

/** Original ordinary Java initialization/linking cases shared with the JVM. */
public final class ForNameCases {
  public static class Counts { public static int events; }
  public static class Parent { static { Counts.events=Counts.events*10+1; } }
  public static class Lazy extends Parent { static { Counts.events=Counts.events*10+2; } }
  public static class ArrayOnly { static { Counts.events=Counts.events*10+3; } }
  public static class Once { static { Counts.events=Counts.events*10+5; } }
  public static class RuntimeFailure { static { if(fail())throw new IllegalStateException("real initializer"); } }
  public static class ErrorFailure { static { if(fail())throw new Error("real error"); } }
  public static class OwnError extends Error { public OwnError(){super("derived error");} }
  public static class DerivedFailure { static { if(fail())throw new OwnError(); } }
  public static class CustomLoader extends ClassLoader { }
  public static class DefaultCounts { public static int events; }
  static int markDefault(){return ++DefaultCounts.events;}
  public interface DefaultContract { int MARK=markDefault(); default int value(){return 7;} }
  public static class DefaultImplementation implements DefaultContract { }
  public static String defaults(int which,ClassLoader loader)throws Exception {
    String binary=name(which==2?"DefaultContract":"DefaultImplementation");
    return found(binary,which!=0,loader)+":"+DefaultCounts.events;
  }
  public static int defaultCount(){return DefaultCounts.events;}
  static boolean fail(){return true;}
  static String name(String simple){return "org.androswitch.contracts.ForNameCases$"+simple;}
  static String found(String name,boolean initialize,ClassLoader loader)throws Exception {
    return Class.forName(name,initialize,loader).getName();
  }
  public static String run(int which,ClassLoader appLoader) {
    try {
      switch(which) {
        case 0:return found(name("Lazy"),false,appLoader)+":"+Counts.events;
        case 1:return found(name("Lazy"),false,appLoader)+":"+Counts.events;
        case 2:return found(name("Lazy"),true,appLoader)+":"+Counts.events;
        case 3:return found(name("Lazy"),true,appLoader)+":"+Counts.events;
        case 4:return Class.forName(name("Once")).getName()+":"+Counts.events;
        case 5:return found("[[L"+name("ArrayOnly")+";",true,appLoader)+":"+Counts.events;
        case 6:return found(name("ArrayOnly"),true,appLoader)+":"+Counts.events;
        case 7:return found("[[I",true,null);
        case 8:return found("java.lang.String",true,null);
        case 9:return Class.forName("java.lang.String",false,appLoader)==Class.forName("java.lang.String",false,null)?"same":"different";
        case 10:return found(name("Lazy"),false,null);
        case 11:return found("[L"+name("Lazy")+";",false,null);
        case 12:return found(name("Missing"),false,appLoader);
        case 13:return found("[L"+name("Missing")+";",true,appLoader);
        case 14:return found("int",true,appLoader);
        case 15:return found("void",false,appLoader);
        case 16:return found("[V",false,appLoader);
        case 17:return found("[Ljava/lang/String;",false,appLoader);
        case 18:return found("java/lang/String",false,appLoader);
        case 19:return found("Ljava.lang.String;",false,appLoader);
        case 20:return found("java..lang.String",false,appLoader);
        case 21:return found("",false,appLoader);
        case 22:return found(null,false,appLoader);
        case 23:return found(name("RuntimeFailure"),false,appLoader);
        case 24:return found(name("RuntimeFailure"),true,appLoader);
        case 25:return found(name("RuntimeFailure"),false,appLoader);
        case 26:return found(name("RuntimeFailure"),true,appLoader);
        case 27:return found(name("ErrorFailure"),true,appLoader);
        case 28:return found(name("ErrorFailure"),true,appLoader);
        case 29:return found(name("DerivedFailure"),true,appLoader);
        case 30:return found(name("DerivedFailure"),true,appLoader);
        case 31:return found("org.androswitch.contracts.LinkedChild",false,appLoader);
        case 32:return found("[Lorg.androswitch.contracts.LinkedChild;",true,appLoader);
        case 33:return found("[Ljava.lang.String;",true,null);
        default:throw new IllegalArgumentException("case");
      }
    } catch(Throwable failure){return failure.getClass().getName();}
  }
}
