package org.androswitch.contracts;
import android.app.Application;
import android.app.Activity;
import android.os.Bundle;
public final class LifecycleProbe {
  public static int mode,createdCallbacks,startedCallbacks;
  public static String log="",createdOrder="",startedOrder="";
  public static Application app,detached;
  public static Activity activity;
  public static Bundle input,saved,observedSaved;
  public static Observer a,b,c;
  public static void record(String value){log+=value+";";}
  public static void configure(int value){mode=value;}
  public static String transcript(){return createdOrder+"|"+startedOrder;}
  public static class Observer implements Application.ActivityLifecycleCallbacks {
    public String id;
    public int payload=123;
    public Observer(String value){id=value;}
    public void onActivityCreated(Activity receiver,Bundle state){
      if(payload!=123||receiver!=activity||state!=input)throw new IllegalStateException("created identity");
      record(id+"C");createdOrder+=id;createdCallbacks++;
      if(mode==1&&this==a){snapshotMutation();collect();}
      if(mode==2&&this==a)app.unregisterActivityLifecycleCallbacks(this);
      if((mode==4||mode==8)&&this==a)throw new IllegalStateException("observer failed");
    }
    public void onActivityStarted(Activity receiver){record(id+"S");startedOrder+=id;startedCallbacks++;if(mode==9&&this==a)throw new IllegalStateException("started observer failed");}
    public void onActivityResumed(Activity receiver){record(id+"R");}
    public void onActivityPaused(Activity receiver){record(id+"P");}
    public void onActivityStopped(Activity receiver){record(id+"T");}
    public void onActivityDestroyed(Activity receiver){record(id+"D");}
    public void onActivitySaveInstanceState(Activity receiver,Bundle state){
      if(receiver!=activity||state!=saved||state.getInt("before")!=11)throw new IllegalStateException("save identity");
      observedSaved=state;state.putInt("observer",payload);record(id+"V");
    }
  }
  public static abstract class MissingObserver implements Application.ActivityLifecycleCallbacks { }
  static void snapshotMutation(){capture(app,b);app.unregisterActivityLifecycleCallbacks(b);b=null;c=new Observer("C");app.registerActivityLifecycleCallbacks(c);}
  public static class EqualObserver extends Observer {
    public EqualObserver(String value){super(value);}
    public boolean equals(Object other){return other instanceof Observer&&id.equals(((Observer)other).id);}
  }
  public static class MutatingObserver extends Observer {
    public MutatingObserver(){super("M");}
    public boolean equals(Object other){app.registerActivityLifecycleCallbacks(this);return false;}
  }
  public static void detached(){detached=new Application();detached.registerActivityLifecycleCallbacks(new Observer("W"));}
  public static void clearDetached(){detached=null;}
  public static native void collect();
  public static native void capture(Object app,Object callback);
  public static void weakRegistration(){Application other=new Application();Observer callback=new Observer("W");other.registerActivityLifecycleCallbacks(callback);capture(other,callback);}
}
