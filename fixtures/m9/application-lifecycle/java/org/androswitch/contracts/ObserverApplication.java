package org.androswitch.contracts;
import android.app.Application;
public class ObserverApplication extends Application {
  public void onCreate(){
    super.onCreate();LifecycleProbe.app=this;
    if(LifecycleProbe.mode==3){registerActivityLifecycleCallbacks(null);return;}
    LifecycleProbe.a=new LifecycleProbe.Observer("A");LifecycleProbe.b=new LifecycleProbe.Observer("B");
    registerActivityLifecycleCallbacks(LifecycleProbe.a);
    if(LifecycleProbe.mode==2)registerActivityLifecycleCallbacks(LifecycleProbe.a);
    registerActivityLifecycleCallbacks(LifecycleProbe.b);
  }
}
