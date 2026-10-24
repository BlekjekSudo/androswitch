package org.androswitch.contracts;
import android.app.Application;
import android.app.Activity;
import android.os.Bundle;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
/** Device proof: real callbacks only, with no native-only test hooks. */
public final class LifecycleProof extends Application implements Application.ActivityLifecycleCallbacks {
  public static String trace="";
  public static Activity activity;
  public static Bundle creationState,savingState;
  public static boolean startup,returned,saved;
  public void onCreate(){super.onCreate();registerActivityLifecycleCallbacks(this);}
  public static void mark(String value){trace+=value+";";}
  public static void write(){
    String transcript="startup="+(startup?"PASS":"PENDING")+"\nreturn="+(returned?"PASS":"PENDING")+"\nsave="+(saved?"observed":"not_observed")+"\ntrace="+trace+"\n";
    try{
      File directory=new File("/sdcard/Download");directory.mkdirs();
      FileOutputStream output=new FileOutputStream("/sdcard/Download/androswitch-lifecycle-proof.txt");
      output.write(transcript.getBytes("UTF-8"));output.close();
    }catch(IOException failure){throw new IllegalStateException(failure.toString());}
  }
  static void same(Activity receiver){if(receiver!=activity)throw new IllegalStateException("observer Activity identity");}
  public void onActivityCreated(Activity receiver,Bundle state){same(receiver);if(state!=creationState)throw new IllegalStateException("creation Bundle identity");mark("C");}
  public void onActivityStarted(Activity receiver){same(receiver);mark("S");}
  public void onActivityResumed(Activity receiver){same(receiver);mark("R");}
  public void onActivityPaused(Activity receiver){same(receiver);mark("P");}
  public void onActivityStopped(Activity receiver){same(receiver);mark("T");}
  public void onActivityDestroyed(Activity receiver){same(receiver);mark("D");}
  public void onActivitySaveInstanceState(Activity receiver,Bundle state){same(receiver);if(state!=savingState||state.getInt("before")!=73)throw new IllegalStateException("save Bundle identity");state.putInt("observer",91);saved=true;mark("V");}
}
