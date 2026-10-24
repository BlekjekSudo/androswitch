package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
public class ForNameActivity extends Activity {
  public static ClassLoader loader;
  public void onCreate(Bundle state){super.onCreate(state);loader=getClassLoader();}
  public static String run(int which){return ForNameCases.run(which,loader);}
  public static String defaults(int which)throws Exception{return ForNameCases.defaults(which,loader);}
  public static int defaultCount(){return ForNameCases.defaultCount();}
  public static Class<?> lookup(String name,boolean initialize,ClassLoader loader)throws Exception {
    return Class.forName(name,initialize,loader);
  }
  public static Class<?> one(String name)throws Exception{return Class.forName(name);}
}
