package org.androswitch.contracts;
import java.io.FileOutputStream;
import android.os.Handler;
public final class ThreadLocalProofActivity extends ThreadLocalActivity {
    final ThreadLocal<Object> local=new ThreadLocal<Object>();
    Thread main,worker;
    android.widget.TextView text;
    public void onCreate(android.os.Bundle state) {
        super.onCreate(state); main=Thread.currentThread(); local.set(new ThreadLocalCases.Payload(101));
        text=new android.widget.TextView(this); text.setText("ThreadLocal proof: waiting for actual worker");setContentView(text);
        final Handler ui=new Handler();
        worker=new Thread(new Runnable() { public void run() {
            boolean empty=local.get()==null; local.set(null); boolean storedNull=local.get()==null;
            local.remove(); local.set(new ThreadLocalCases.Payload(202)); System.gc();
            int value=((ThreadLocalCases.Payload)local.get()).value;
            boolean identity=Thread.currentThread()==worker && Thread.currentThread()!=main;
            if(!empty || !storedNull || value!=202 || !identity) throw new IllegalStateException("threadlocal-worker-proof");
            ui.post(new Runnable() { public void run() {
                if(Thread.currentThread()!=main || ((ThreadLocalCases.Payload)local.get()).value!=101) throw new IllegalStateException("threadlocal-main-proof");
                try {
                    FileOutputStream out=new FileOutputStream("/sdcard/Download/androswitch-threadlocal-proof.txt");
                    out.write("THREAD_LOCAL_PROOF_PASS\nmain=101\nworker=202\nseparate=true\nstoredNull=true\nactualThreadIdentity=true\n".getBytes("UTF-8"));out.close();
                }catch(java.io.IOException e){throw new IllegalStateException("threadlocal-proof-io");}
                text.setText("ThreadLocal proof PASS: main 101, worker 202; VFS saved");
            }});
        }},"threadlocal-sdk-worker"); worker.start();
    }
}
