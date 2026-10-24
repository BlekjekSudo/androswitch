package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.view.View;
import android.view.MotionEvent;
import android.widget.Button;
public class ForNameActivity extends Activity implements View.OnClickListener,View.OnTouchListener {
  public static ClassLoader loader;
  public static int clicks;
  public void onCreate(Bundle state){super.onCreate(state);loader=getClassLoader();Button button=new Button(this);button.setOnClickListener(this);button.setOnTouchListener(this);button.performClick();}
  public void onClick(View view){clicks++;}
  public boolean onTouch(View view,MotionEvent event){return false;}
  public static String run(int which){return ForNameCases.run(which,loader);}
  public static String defaults(int which)throws Exception{return ForNameCases.defaults(which,loader);}
  public static int defaultCount(){return ForNameCases.defaultCount();}
  public static Class<?> lookup(String name,boolean initialize,ClassLoader loader)throws Exception {
    return Class.forName(name,initialize,loader);
  }
  public static Class<?> one(String name)throws Exception{return Class.forName(name);}
}
