package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.os.Looper;
import java.util.Timer;
import java.util.TimerTask;

/** Standard SDK-only contracts. Host tests control time/worker delivery. */
public class TimerActivity extends Activity {
  public static Timer timer;
  public static Probe task;
  public static int count;
  public static long scheduled;
  public static long actual;
  public static boolean cancelled;
  public static boolean workerLooperAbsent;
  public void onCreate(Bundle state) { super.onCreate(state); }
  public static void create(int kind) {
    if (kind == 0) timer = new Timer();
    else if (kind == 1) timer = new Timer(true);
    else if (kind == 2) timer = new Timer("contract worker");
    else timer = new Timer("contract daemon",true);
  }
  public static void nullName() { timer = new Timer((String)null); }
  public static void makeTask(int mode) { task = new Probe(mode); }
  public static TimerTask reference() { return task; }
  public static Timer owner() { return timer; }
  public static void forgetTask() { task = null; }
  public static void forgetTimer() { timer = null; }
  public static void schedule(long delay, long period, int mode) {
    if (mode == 0) timer.schedule(task,delay);
    else if (mode == 1) timer.schedule(task,delay,period);
    else timer.scheduleAtFixedRate(task,delay,period);
  }
  public static boolean cancelTask() { return task.cancel(); }
  public static void cancelTimer() { timer.cancel(); }
  public static int purge() { return timer.purge(); }
  public static int count() { return count; }
  public static long scheduled() { return scheduled; }
  public static long actual() { return actual; }
  public static boolean cancelled() { return cancelled; }
  public static boolean workerLooperAbsent() { return workerLooperAbsent; }
  public static long wall() { return System.currentTimeMillis(); }
  public static long monotonic() { return android.os.SystemClock.elapsedRealtime(); }
  public static void handler() { new android.os.Handler(); }
  public static boolean noLooper() { return Looper.myLooper()==null; }
  public static native void nativeHop();
  public static void date() { timer.schedule(task,(java.util.Date)null); }
  public static class Probe extends TimerTask {
    final int mode;
    public Probe(int mode) { this.mode=mode; }
    public void run() {
      count++;
      scheduled=scheduledExecutionTime();
      actual=System.currentTimeMillis();
      workerLooperAbsent=Looper.myLooper()==null;
      if(mode==1) cancelled=cancel();
      else if(mode==2) timer.cancel();
      else if(mode==3) { int zero=count-count; count=count/zero; }
      else if(mode==4) {
        // Reentrant scheduling must not encounter a borrowed queue.
        timer.schedule(new Probe(2),0);
      } else if(mode==5) System.gc();
      else if(mode==6) nativeHop();
    }
  }
}
