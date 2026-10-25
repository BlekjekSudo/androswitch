package org.androswitch.conformance;
import android.app.*;
import android.os.Bundle;
public final class Recorder extends Application implements Application.ActivityLifecycleCallbacks {
  public static String events = "";
  public void onCreate() {
    super.onCreate();
    registerActivityLifecycleCallbacks(this);
  }
  private void event(Activity a, String e) {
    if (a instanceof ChildActivity)
      events += e;
  }
  public void onActivityCreated(Activity a, Bundle b) {
    event(a, "C");
  }
  public void onActivityStarted(Activity a) {
    event(a, "S");
  }
  public void onActivityResumed(Activity a) {
    event(a, "R");
  }
  public void onActivityPaused(Activity a) {
    event(a, "P");
  }
  public void onActivityStopped(Activity a) {
    event(a, "T");
  }
  public void onActivityDestroyed(Activity a) {
    event(a, "D");
  }
  public void onActivitySaveInstanceState(Activity a, Bundle b) {
    event(a, "B");
  }
}
