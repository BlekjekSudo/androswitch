package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.widget.TextView;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.Timer;
import java.util.TimerTask;

/** Unmodified Android APIs only: real worker writes VFS evidence and posts UI. */
public class TimerProofActivity extends Activity {
  private Timer timer;
  private Handler main;
  private TextView label;
  private int count;
  private long first;
  private String results="";
  public void onCreate(Bundle state) {
    super.onCreate(state);
    label=new TextView(this);
    label.setText("Timer: waiting for dedicated worker");
    setContentView(label);
    main=new Handler();
    File directory=new File("/sdcard/Download");
    directory.mkdirs();
    timer=new Timer("SDK Timer proof",true);
    timer.scheduleAtFixedRate(new TimerTask() {
      public void run() {
        final long now=System.currentTimeMillis();
        if(count==0) first=now;
        count++;
        results += "task="+count+" scheduled="+scheduledExecutionTime()+" actual="+now+" noLooper="+(Looper.myLooper()==null ? "true" : "false")+"\n";
        if(count==3) {
          results += "TIMER_PROOF_PASS elapsed="+(now-first)+"\n";
          try {
            FileOutputStream out=new FileOutputStream("/sdcard/Download/androswitch-timer-proof.txt");
            out.write(results.getBytes());
            out.close();
          } catch(IOException error) { throw new IllegalStateException(error); }
          timer.cancel();
          main.post(new Runnable() {
            public void run() { label.setText("Timer proof PASS: 3 worker tasks, VFS saved"); }
          });
        }
      }
    },100,80);
  }
}
