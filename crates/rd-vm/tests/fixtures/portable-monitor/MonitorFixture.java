package org.androswitch.vm;
public class MonitorFixture {
  public static int holds(Object object) { return Thread.holdsLock(object) ? 1 : 0; }
  public static int withLock(Object object) { synchronized(object) { return holds(object); } }
  public static int recursive(Object object, int depth) {
    synchronized(object) { return depth == 0 ? holds(object) : holds(object) + recursive(object, depth-1); }
  }
  public static synchronized int staticSynchronized() { return holds(MonitorFixture.class); }
  public synchronized int instanceSynchronized() { return holds(this); }
  public static synchronized native long staticNative(int behavior);
  public synchronized native long instanceNative(int behavior);
  public static void unsupportedWait(Object object) throws InterruptedException { object.wait(1); }
  public static void unsupportedNotify(Object object) { object.notify(); }
  public static int run(int test) {
    Object object = new Object();
    switch(test) {
      case 0: {
        int value=holds(object);
        synchronized(object) { value=value*10+holds(object); }
        return value*10+holds(object);
      }
      case 1: {
        int value=0;
        synchronized(object) {
          value=holds(object);
          synchronized(object) { value=value*10+holds(object); }
          value=value*10+holds(object);
        }
        return value*10+holds(object);
      }
      case 2: synchronized(MonitorFixture.class) { return holds(MonitorFixture.class)*10+holds(object); }
      case 3: { int[] array=new int[1]; synchronized(array) { return holds(array); } }
      case 4: {
        int value=0;
        try { synchronized(object) { value=holds(object); int zero=0; value=100/zero; } }
        catch(ArithmeticException expected) { value=value*10+1; }
        return value*10+holds(object);
      }
      case 5: try { synchronized((Object)null) { return -1; } } catch(NullPointerException expected) { return 7; }
      case 6: try { return holds(null); } catch(NullPointerException expected) { return 9; }
      case 7: return staticSynchronized();
      case 8: return new MonitorFixture().instanceSynchronized();
      case 9: return recursive(object,10);
      case 10: {
        Object second=new Object();
        synchronized(object) { synchronized(second) { System.gc(); return holds(object)*10+holds(second); } }
      }
      case 11: return withLock(object)*10+holds(object);
      default: throw new IllegalArgumentException();
    }
  }
  public static void main(String[] args) { for(int i=0;i<12;i++) System.out.println(i+"\t"+run(i)); }
}
