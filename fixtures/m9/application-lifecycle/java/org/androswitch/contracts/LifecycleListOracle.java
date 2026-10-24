package org.androswitch.contracts;
import java.util.ArrayList;
/** Plain JDK ArrayList snapshot/mutation oracle, not an Android stub. */
public final class LifecycleListOracle {
  static ArrayList<String> callbacks;
  static String dispatch(int mode,boolean created){
    String out="";
    for(Object raw:callbacks.toArray()){
      String id=(String)raw;out+=id;
      if(created&&id.equals("A")){
        if(mode==1){callbacks.remove("B");callbacks.add("C");}
        if(mode==2)callbacks.remove("A");
      }
    }
    return out;
  }
  public static void main(String[] args){
    for(int mode=0;mode<3;mode++){
      callbacks=new ArrayList<String>();callbacks.add("A");if(mode==2)callbacks.add("A");callbacks.add("B");
      System.out.println(mode+"\t"+dispatch(mode,true)+"|"+dispatch(mode,false));
    }
  }
}
