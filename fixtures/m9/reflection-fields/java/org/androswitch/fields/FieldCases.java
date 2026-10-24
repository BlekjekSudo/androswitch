package org.androswitch.fields;
import java.lang.reflect.Field;

/** Original SDK/JVM input: reflection operations are ordinary Java calls. */
public final class FieldCases {
  public interface First { int shared = 17; }
  public interface Second { int shared = 29; }
  public static class Base {
    public int inherited = 11;
    public static int inheritedStatic = 23;
    protected int protectedNumber = 37;
  }
  public static class Data extends Base implements First, Second {
    public static final String label = "field-contract";
    public static final int constantNumber = 61;
    public boolean yes = true;
    public byte tiny = -7;
    public char letter = '\uffee';
    public short small = -123;
    public int number = 12345;
    public long wide = 1234567890123L;
    public float real = -2.5f;
    public double precise = 9.25;
    public int zero;
    public Object empty;
    public Object payload = "retained-payload";
    private int hidden = 47;
    private int shared = 3;
    public static int readOwn() throws Exception { return Data.class.getDeclaredField("hidden").getInt(new Data()); }
  }
  public static class Child extends Base { }
  public interface DefaultFace { default int inheritedDefault(){return 1;} }
  public static class DefaultBearing implements DefaultFace { public static int number = 83; }
  public static class Counter { public static int value; }
  public static class Lazy {
    public static int number;
    static { Counter.value++; number = 73; }
  }
  public static class Broken {
    public static int number = fail();
    static int fail() { throw new IllegalStateException("field-init-failed"); }
  }
  public static class HiddenBase { public int same = 5; }
  public static class HiddenChild extends HiddenBase { public int same = 9; }
  public static class ReferenceOnly {
    public static int read(Symbol symbol) { return symbol.absent; }
  }
  public static class Symbol { public int absent; }
  public static Object kept;
  public static int readGc;
  public static Field reentrantField;
  public static class Reentrant {
    private static int number;
    static { reentrantField.setAccessible(true); System.gc(); number = 79; }
  }
  public static class GcTarget {
    public static Object payload;
    static { System.gc(); readGc++; payload = "after-clinit-gc"; }
  }
  public static class NativeCaller {
    private static int number = 89;
    public static native int readNative();
    public static native int failNative();
  }
  public static Field lookup(Class<?> type, String name, boolean inherited) throws Exception {
    return inherited ? type.getField(name) : type.getDeclaredField(name);
  }
  public static int hiddenRead(boolean override) throws Exception {
    Field field = Data.class.getDeclaredField("hidden");
    field.setAccessible(override);return field.getInt(new Data());
  }
  public static int callerRead() throws Exception { return Data.readOwn(); }
  public static Object gcRead(Field field) throws Exception { return field.get(null); }
  public static int collision() throws Exception { return HiddenBase.class.getField("same").getInt(new HiddenChild()); }
  public static String staticCollision() throws Exception {
    String small="",wide="";
    for(Field field:StaticAlias.class.getDeclaredFields()){
      if(!"alpha".equals(field.getName()))throw new IllegalStateException("unpatched alias");
      if(field.getType()==Integer.TYPE)small="int:"+field.get(null);
      if(field.getType()==Long.TYPE)wide="long:"+field.get(null);
    }
    return small+","+wide;
  }
  public static int defaultBearingRead() throws Exception {return DefaultBearing.class.getField("number").getInt(null);}
  public static int reentrantRead() throws Exception {
    Field field=Reentrant.class.getDeclaredField("number");reentrantField=field;
    try{return field.getInt(null);}finally{reentrantField=null;}
  }
  public static String evaluate(int index) throws Exception {
    Data data = new Data();
    switch (index) {
      case 0: return (String)Data.class.getDeclaredField("label").get(null);
      case 1: return "" + Data.class.getDeclaredField("constantNumber").getInt(null);
      case 2: return Data.class.getDeclaredField("label").getDeclaringClass().getName();
      case 3: return Data.class.getDeclaredField("label").getType().getName();
      case 4: return Data.class.getDeclaredField("label").getName();
      case 5: return "" + Data.class.getDeclaredField("label").getModifiers();
      case 6: return "" + Data.class.getField("inherited").getInt(data);
      case 7: return Data.class.getField("inherited").getDeclaringClass().getName();
      case 8: return "" + Data.class.getField("shared").getInt(null);
      case 9: return Data.class.getField("shared").getDeclaringClass().getName();
      case 10: return "" + Data.class.getDeclaredField("yes").getBoolean(data);
      case 11: return "" + Data.class.getDeclaredField("tiny").getByte(data);
      case 12: return "" + Data.class.getDeclaredField("tiny").getShort(data);
      case 13: return "" + Data.class.getDeclaredField("letter").getInt(data);
      case 14: return "" + Data.class.getDeclaredField("small").getLong(data);
      case 15: return "" + Data.class.getDeclaredField("number").getLong(data);
      case 16: return "" + Data.class.getDeclaredField("wide").getLong(data);
      case 17: return "" + (long)Data.class.getDeclaredField("tiny").getFloat(data);
      case 18: return "" + (long)Data.class.getDeclaredField("letter").getDouble(data);
      case 19: return "" + (long)Data.class.getDeclaredField("real").getDouble(data);
      case 20: return "" + (long)Data.class.getDeclaredField("precise").getDouble(data);
      case 21: return "" + Data.class.getDeclaredField("zero").getInt(data);
      case 22: return "" + (Data.class.getDeclaredField("empty").get(data) == null);
      case 23: return (String)Data.class.getDeclaredField("payload").get(data);
      case 24: return Data.class.getDeclaredField("number").get(data).getClass().getName();
      case 25: return Data.class.getDeclaredField("yes").get(data).getClass().getName();
      case 26: return "" + Data.class.getField("constantNumber").getInt(new Object());
      case 27: return "" + hiddenRead(true);
      case 28: return "" + Data.readOwn();
      case 29: return "" + Data.class.getDeclaredField("hidden").isAccessible();
      case 30: { Field f=Data.class.getDeclaredField("hidden");f.setAccessible(true);return ""+f.isAccessible(); }
      case 31: { Field f=Lazy.class.getDeclaredField("number");return ""+Counter.value; }
      case 32: return "" + Lazy.class.getDeclaredField("number").getInt(null);
      case 33: return "" + Lazy.class.getDeclaredField("number").getInt(null) + ":" + Counter.value;
      case 34: return "" + Child.class.getField("inheritedStatic").getInt(null);
      case 35: return "" + org.androswitch.fields.other.ProtectedReader.read(new org.androswitch.fields.other.ProtectedReader());
      case 36: return (String)GcTarget.class.getDeclaredField("payload").get(null);
      case 37: return "" + readGc;
      default: return failures(index);
    }
  }
  private static String failures(int index) throws Exception {
    try {
      switch(index) {
        case 38: Data.class.getDeclaredField("inherited");break;
        case 39: Data.class.getField("hidden");break;
        case 40: Data.class.getDeclaredField(null);break;
        case 41: Data.class.getField(null);break;
        case 42: Data.class.getDeclaredField("number").getInt(null);break;
        case 43: Data.class.getDeclaredField("number").getInt(new Object());break;
        case 44: hiddenRead(false);break;
        case 45: Data.class.getDeclaredField("number").getByte(new Data());break;
        case 46: Data.class.getDeclaredField("payload").getInt(new Data());break;
        case 47: Data.class.getDeclaredField("yes").getInt(new Data());break;
        case 48: new int[0].getClass().getDeclaredField("length");break;
        case 49: Integer.TYPE.getDeclaredField("value");break;
        case 50: ReferenceOnly.class.getDeclaredField("absent");break;
        case 51: org.androswitch.fields.other.ProtectedReader.read(new Base());break;
        case 52: Broken.class.getDeclaredField("number").getInt(null);break;
        case 53: Broken.class.getDeclaredField("number").getInt(null);break;
        default: throw new IllegalArgumentException("case index");
      }
      return "NO_EXCEPTION";
    } catch(Throwable error) { return error.getClass().getName(); }
  }
  public static String widening(int source,int target) throws Exception {
    String[] names={"yes","tiny","letter","small","number","wide","real","precise"};
    Data data=new Data();Field field=Data.class.getDeclaredField(names[source]);
    try {
      switch(target){
        case 0:return ""+(field.getBoolean(data)?1:0);
        case 1:return ""+field.getByte(data);
        case 2:return ""+(int)field.getChar(data);
        case 3:return ""+field.getShort(data);
        case 4:return ""+field.getInt(data);
        case 5:return ""+field.getLong(data);
        case 6:return ""+(long)field.getFloat(data);
        case 7:return ""+(long)field.getDouble(data);
        default:throw new IllegalArgumentException("target");
      }
    }catch(IllegalArgumentException expected){return expected.getClass().getName();}
  }
  public static void main(String[] args) throws Exception {
    for(int i=0;i<54;i++)System.out.println(i+"\t"+evaluate(i));
    for(int i=0;i<8;i++)for(int j=0;j<8;j++)System.out.println("widen:"+i+":"+j+"\t"+widening(i,j));
    System.out.println("collision\t"+collision());
    System.out.println("static-alias\t"+staticCollision());
  }
}
