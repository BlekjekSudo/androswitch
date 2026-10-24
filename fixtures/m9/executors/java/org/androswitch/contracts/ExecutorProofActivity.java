package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.widget.TextView;
import java.io.*;
import java.util.concurrent.*;

/** Actual custom ThreadFactory + FIFO tasks, shared storage, main Handler. */
public class ExecutorProofActivity extends Activity {
  private Handler main;
  private TextView label;
  private ExecutorService executor;
  private String results="";
  public void onCreate(Bundle state){
    super.onCreate(state);main=new Handler();label=new TextView(this);
    label.setText("Executor: waiting for dedicated worker");setContentView(label);
    new File("/sdcard/Download").mkdirs();
    final ThreadFactory defaults=Executors.defaultThreadFactory();
    ThreadFactory factory=new ThreadFactory(){public Thread newThread(Runnable target){Thread worker=defaults.newThread(target);worker.setName("SDK executor proof");return worker;}};
    ThreadPoolExecutor pool=new ThreadPoolExecutor(1,1,60,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),factory);
    pool.allowCoreThreadTimeOut(true);executor=Executors.unconfigurableExecutorService(pool);
    for(int index=1;index<=3;index++){
      final int number=index;
      executor.execute(new Runnable(){public void run(){
        Thread actual=Thread.currentThread();
        results+="task="+number+" thread="+actual.getId()+" name="+actual.getName()+" noLooper="+(Looper.myLooper()==null?"true":"false")+"\n";
        if(number==3){
          try{FileOutputStream out=new FileOutputStream("/sdcard/Download/androswitch-executor-proof.txt");out.write((results+"EXECUTOR_PROOF_PASS\n").getBytes());out.close();}
          catch(IOException error){throw new IllegalStateException(error);}
          executor.shutdown();
          main.post(new Runnable(){public void run(){label.setText("Executor proof PASS: FIFO worker tasks, VFS saved");}});
        }
      }});
    }
  }
}
