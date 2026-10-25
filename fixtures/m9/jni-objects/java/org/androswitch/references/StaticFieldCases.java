package org.androswitch.references;
public final class StaticFieldCases {
  public static int initialized;
  public static class Parent {
    public static boolean z=true;
    public static byte b=-113;
    public static char c=65530;
    public static short s=-23456;
    public static int i=-17;
    public static long j=0x123456789abcdefL;
    public static float f=1.25f;
    public static double d=-3.5;
    public static Object reference="initial";
    public static String text;
    private static int hidden=73;
    static{initialized++;}
  }
  public static class Child extends Parent {static{initialized+=10;}}
  public static class InstanceShadow extends Parent {public int i;}
  public static int initialized(){return initialized;}
  public static int changed(){
    System.gc();
    return Parent.z && Parent.b==9 && Parent.c==65 && Parent.s==-7 && Parent.i==42
      && Parent.j==-0x123456789L && Parent.f==-2.5f && Parent.d==6.25
      && "retained".equals(Parent.reference)?1:0;
  }
  public static void release(){Parent.reference=null;Parent.text=null;}
}
