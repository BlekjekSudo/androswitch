package org.androswitch.contracts;
/** SDK/JVM construction contracts, independent of any external app class. */
public final class ReflectionNewCases {
  public static class Counts {public static int value;}
  public static class Simple {public int value;public Simple(){System.gc();value=37;}}
  public static class WithoutZero {public WithoutZero(int value){}}
  public static class Parent {public Parent(){}}
  public static class ChildWithoutZero extends Parent {public ChildWithoutZero(String value){}}
  public static abstract class Abstract {public Abstract(){}}
  public interface Contract {}
  public static class Private {private Private(){}}
  public static class PrivateSelf {private PrivateSelf(){}public static PrivateSelf make()throws Exception{return PrivateSelf.class.newInstance();}}
  public static class Package {Package(){}}
  public static class Throwing {public Throwing()throws Exception{throw new Exception("ctor detail");}}
  public static class Lazy {static {Counts.value++;}public Lazy(){Counts.value+=10;}}
  public static class BadInit {static {if(fail())throw new IllegalStateException("init");}public BadInit(){}}
  public static class DeniedInit {static{Counts.value+=100;}private DeniedInit(){}}
  private static boolean fail(){return true;}
  public static String run(int index){
    try{
      switch(index){
        case 0:return Integer.toString(Simple.class.newInstance().value);
        case 1:WithoutZero.class.newInstance();break;
        case 2:ChildWithoutZero.class.newInstance();break;
        case 3:Abstract.class.newInstance();break;
        case 4:Contract.class.newInstance();break;
        case 5:int.class.newInstance();break;
        case 6:String[].class.newInstance();break;
        case 7:Private.class.newInstance();break;
        case 8:return PrivateSelf.make()!=null?"self":"bad";
        case 9:return Package.class.newInstance()!=null?"package":"bad";
        case 10:Throwing.class.newInstance();break;
        case 11:Lazy.class.newInstance();return Integer.toString(Counts.value);
        case 12:BadInit.class.newInstance();break;
        case 13:DeniedInit.class.newInstance();break;
        case 14:return Integer.toString(Counts.value);
        default:throw new IllegalArgumentException("case");
      }return "unexpected";
    }catch(Throwable error){return error.getClass().getName();}
  }
  public static void main(String[] args){for(int index=0;index<15;index++)System.out.println(index+"\t"+run(index));System.out.println("12\t"+run(12));}
}
