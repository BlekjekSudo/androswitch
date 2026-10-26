package org.androswitch.contracts;
import java.util.ArrayList;
import java.util.HashMap;
/** Independent JVM oracle for the value methods used by lifecycle observers. */
public final class LifecycleValueOracle {
  static class Override extends Object {
    int calls;
    public boolean equals(Object other){calls++;return true;}
    boolean identity(Object other){return super.equals(other);}
  }
  static class Inherited extends Override { }
  static class Plain { }
  static class PlainChild extends Plain { }
  static class ListChild extends ArrayList<Object> { }
  static class ListOverride extends ArrayList<Object> {
    public boolean equals(Object other){return true;}
  }
  public static int evaluate(int index){
    switch(index){
      case 0:return "".endsWith("")?1:0;
      case 1:return "abc".endsWith("")?1:0;
      case 2:return "".endsWith("a")?1:0;
      case 3:return "abc".endsWith("bc")?1:0;
      case 4:return "abc".endsWith("abc")?1:0;
      case 5:return "abc".endsWith("abcd")?1:0;
      case 6:return "abc".endsWith("Bc")?1:0;
      case 7:return "A\uD83D\uDE00".endsWith("\uD83D\uDE00")?1:0;
      case 8:return "\uD83D\uDE00A".endsWith("\uD83D\uDE00")?1:0;
      case 9:return "\u00e9".endsWith("e")?1:0;
      case 10:try{"abc".endsWith(null);return 0;}catch(NullPointerException expected){return 1;}
      case 11:{Object a=new Object();return a.equals(a)?1:0;}
      case 12:return new Object().equals(new Object())?1:0;
      case 13:return new Object().equals(null)?1:0;
      case 14:{Override a=new Override();return a.equals(null)&&a.calls==1?1:0;}
      case 15:{Override a=new Override();return a.identity(null)?1:0;}
      case 16:{Override a=new Override();return a.identity(a)&&a.calls==0?1:0;}
      case 17:{Override a=new Override();return a.identity(new Object())?1:0;}
      case 18:{Object a="ab".concat("c"),b="a".concat("bc");return a.equals(b)?1:0;}
      case 19:{Object a="ab".concat("c");return a.equals(null)?1:0;}
      case 20:{Object a=Integer.valueOf(1000),b=Integer.valueOf(1000);return a.equals(b)?1:0;}
      case 21:{Object a=Integer.valueOf(1000),b=Long.valueOf(1000);return a.equals(b)?1:0;}
      case 22:{Object a=Integer.valueOf(1000);return a.equals(null)?1:0;}
      case 23:{Object a=new PlainChild();return a.equals(a)&&!a.equals(new PlainChild())?1:0;}
      case 24:{Inherited a=new Inherited();Object b=a;return b.equals(null)&&a.calls==1?1:0;}
      case 25:{Inherited a=new Inherited();return a.identity(a)&&a.calls==0?1:0;}
      // Real JVM results are true. Ported collection value methods must match
      // them; unported families must fail explicitly, never identity-fallback.
      case 26:{Object a=new ArrayList<Object>(),b=new ArrayList<Object>();return a.equals(b)?1:0;}
      case 27:{Object a=new HashMap<Object,Object>(),b=new HashMap<Object,Object>();return a.equals(b)?1:0;}
      case 28:{Object a=new ListChild(),b=new ListChild();return a.equals(b)?1:0;}
      case 29:{Object a=new ArrayList<Object>();return a.equals(a)?1:0;}
      case 30:{Object a=new ListOverride();return a.equals(null)?1:0;}
      case 31:{Object a=new int[]{1};return a.equals(a)?1:0;}
      case 32:{Object a=new int[]{1},b=new int[]{1};return a.equals(b)?1:0;}
      case 33:{Object a=new int[]{1};return a.equals(null)?1:0;}
      case 34:{Object a=new Object[]{null};return a.equals(a)?1:0;}
      case 35:{Object a=new String[]{"same"},b=new String[]{"same"};return a.equals(b)?1:0;}
      case 36:{Object a=new int[1][];return a.equals(a)?1:0;}
      default:throw new IllegalArgumentException("unknown case");
    }
  }
  public static void main(String[] args){for(int i=0;i<37;i++)System.out.println(i+"\t"+evaluate(i));}
}
