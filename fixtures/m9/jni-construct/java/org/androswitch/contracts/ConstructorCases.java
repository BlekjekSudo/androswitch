package org.androswitch.contracts;
public class ConstructorCases {
  static { System.loadLibrary("constructproof"); }
  public static native Payload create(int form,int mode,Object identity,int[] array);
  public static native int nested();
  public static native int exceptionProbe(int form);
  public static native int threadIdentity();
  public static native int attachedThread();
  public static native int contextLoader();
  public static void collect(){System.gc();}
  public static int run(int form,int mode){
    Object identity=new Object();int[] array={3,5};
    Payload result=create(form,mode,identity,array);
    if(result==null||result.identity!=identity||result.array!=array)throw new AssertionError("constructor references");
    System.gc();return result.checksum();
  }
  public static void main(String[] args){
    System.out.println("threads\tidentity\t"+threadIdentity());
    System.out.println("threads\tattached\t"+attachedThread());
    System.out.println("threads\tcontext\t"+contextLoader());
    System.out.println("loader\tinheritance\t"+LoaderCases.inheritance());
    for(int form=0;form<3;form++){
      System.out.println("value\t"+form+"\t"+run(form,0));
      System.out.println("reentry\t"+form+"\t"+run(form,2));
      try{run(form,1);throw new AssertionError("missing constructor exception");}
      catch(IllegalStateException expected){System.out.println("exception\t"+form+"\t"+expected.getMessage());}
      System.out.println("nativeCatch\t"+form+"\t"+exceptionProbe(form));
    }
    System.out.println("init\tvalue\t"+new Initialized().value());
    System.out.println("init\tagain\t"+new Initialized().value());
    try{new BadInitializer();throw new AssertionError("missing init failure");}
    catch(ExceptionInInitializerError expected){System.out.println("init\tfailure\t"+expected.getClass().getName());}
    try{new BadInitializer();throw new AssertionError("missing persistent init failure");}
    catch(NoClassDefFoundError expected){System.out.println("init\tpoisoned\t"+expected.getClass().getName());}
  }
}
class Payload {
  public Object identity;public int[] array;public int result;
  Payload(int i,boolean z,byte b,char c,short s,long j,float f,double d,Object identity,int[] array,
      double d1,double d2,double d3,double d4,double d5,double d6,double d7,double d8,int mode){
    if(mode==1)throw new IllegalStateException("constructor failure");
    this.identity=identity;this.array=array;
    if(mode==2){ConstructorCases.collect();result=ConstructorCases.nested();ConstructorCases.collect();}
    result+=i+(z?1:0)+b+c+s+(int)j+(int)(f*8)+(int)(d*8)
      +(int)(d1+2*d2+3*d3+4*d4+5*d5+6*d6+7*d7+8*d8)+array[0]+array[1];
  }
  public int checksum(){return result;}
}
class Leaf {
  int result;Leaf(int value){ConstructorCases.collect();result=value;}
  public int value(){return result;}
}
abstract class AbstractPayload { public AbstractPayload(){} }
interface ConstructorInterface {}
class ConstructorParent { public ConstructorParent(int value){} }
class ConstructorChild extends ConstructorParent { public ConstructorChild(){super(1);} }
class InitializationState { static int count; }
class Initialized {
  static { InitializationState.count++; }
  int result;Initialized(){result=InitializationState.count;}
  public int value(){return result;}
}
class BadInitializer {
  static int value=explode();
  static int explode(){throw new IllegalStateException("initializer failure");}
  public BadInitializer(){}
}
class LookupThread extends Thread {
  static int calls;
  public void run(){calls++;}
  public static Thread currentThread(){calls++;return null;}
}
class InheritedLookupThread extends Thread {}
class NativeLookupThread extends Thread { public native void run(); }
abstract class AbstractLookupThread extends Thread { public abstract void run(); }
class LoaderCases {
  public static int inheritance(){
    Thread parent=Thread.currentThread();ClassLoader saved=parent.getContextClassLoader();
    Thread first=new Thread();
    parent.setContextClassLoader(null);Thread second=new Thread();parent.setContextClassLoader(saved);
    if(first.getContextClassLoader()!=saved||second.getContextClassLoader()!=null
        ||parent.getContextClassLoader()!=saved)throw new AssertionError("context inheritance snapshot");
    return 107;
  }
}
class LoaderParentThread extends Thread {
  static Thread child;static int reads;
  public ClassLoader getContextClassLoader(){reads++;return super.getContextClassLoader();}
  public void run(){child=new Thread();}
}
