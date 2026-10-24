package org.androswitch.contracts;
import java.io.FileOutputStream;
import java.util.Timer;
import java.util.TimerTask;
import android.app.Activity;
import android.opengl.GLSurfaceView;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

public final class UncaughtCases {
    static RuntimeException failure;
    static Thread worker, observed, executing;
    static Throwable observedError;
    static Handler saved, chosen;
    static Timer timer;
    static GLSurfaceView view;
    static Activity activity;
    static int calls, tag, payload, caught, getterCalls, reentered, zero;
    static boolean alive, proof;
    public static native int nativeReenter();
    public static final class Payload { int value = 271; }
    public static final class Handler implements Thread.UncaughtExceptionHandler {
        final int code, mode;
        final Payload data = new Payload();
        Handler(int code, int mode) { this.code=code; this.mode=mode; }
        public void uncaughtException(Thread thread, Throwable error) {
            calls++; tag=code; observed=thread; executing=Thread.currentThread();
            observedError=error; alive=thread.isAlive();
            if(mode==1) {
                Thread.setDefaultUncaughtExceptionHandler(null);
                thread.setUncaughtExceptionHandler(null); saved=null; chosen=null;
            }
            if(mode==3) Thread.setDefaultUncaughtExceptionHandler(new Handler(99,0));
            if(mode==4) reentered=nativeReenter();
            System.gc();
            payload=data.value;
            if(mode==2) throw new IllegalStateException("handler-failure");
            if(proof) {
                if(thread!=executing || error!=failure || !alive || payload!=271)
                    throw new IllegalStateException("uncaught-proof-mismatch");
                try {
                    FileOutputStream out=new FileOutputStream("/sdcard/Download/androswitch-uncaught-proof.txt");
                    out.write("UNCAUGHT_HANDLER_PROOF_PASS\nsameThread=true\nsameThrowable=true\nalive=true\npayload=271\n".getBytes("UTF-8"));
                    out.close();
                } catch(java.io.IOException bad) { throw new IllegalStateException("uncaught-proof-io"); }
            }
        }
    }
    public static final class GetterThread extends Thread {
        GetterThread(Runnable task) { super(task,"getter-worker"); }
        public Thread.UncaughtExceptionHandler getUncaughtExceptionHandler() {
            getterCalls++;
            System.gc();
            return chosen;
        }
    }
    public static final class Task implements Runnable {
        final int mode;
        Task(int mode) { this.mode=mode; }
        public void run() {
            if(mode==1) { try { throw failure; } catch(RuntimeException okay) { caught++; } return; }
            if(mode==2) { System.getSecurityManager(); return; }
            if(mode==3) { caught=1/zero; return; }
            if(mode==4) throw new RuntimeException("local-only-failure");
            throw failure;
        }
    }
    public static void setup(int mode) {
        Thread.setDefaultUncaughtExceptionHandler(null);
        failure=new RuntimeException("original-failure");
        saved=new Handler(10,mode); chosen=new Handler(30,mode);
        calls=tag=payload=caught=getterCalls=reentered=0;
        observed=executing=worker=null; observedError=null; alive=false; proof=false;
        Thread.setDefaultUncaughtExceptionHandler(saved);
    }
    public static void start(int selection,int mode) {
        worker=selection==3 ? new GetterThread(new Task(mode)) : new Thread(new Task(mode),"uncaught-worker");
        if(selection==1) worker.setUncaughtExceptionHandler(new Handler(20,saved.mode));
        if(selection==2) {
            worker.setUncaughtExceptionHandler(new Handler(20,0));
            worker.setUncaughtExceptionHandler(null);
        }
        worker.start();
    }
    public static void startTimer() {
        timer=new Timer("uncaught-timer");
        timer.schedule(new TimerTask() { public void run() { throw failure; } },0);
    }
    public static GLSurfaceView prepareGl() {
        view=new GLSurfaceView(activity);
        view.setEGLContextClientVersion(2);
        view.setRenderer(new GLSurfaceView.Renderer() {
            public void onSurfaceCreated(GL10 gl,EGLConfig config) {}
            public void onSurfaceChanged(GL10 gl,int w,int h) {}
            public void onDrawFrame(GL10 gl) { throw failure; }
        });
        view.queueEvent(new Task(0));
        return view;
    }
    public static void throwMain() { throw failure; }
    public static void caughtMain() { try { throw failure; } catch(RuntimeException okay) { caught++; } }
    public static int reenter() { System.gc(); return Thread.currentThread()==observed ? 813 : -1; }
    public static int calls() { return calls; }
    public static int tag() { return tag; }
    public static int payload() { return payload; }
    public static int caught() { return caught; }
    public static int getterCalls() { return getterCalls; }
    public static int reentered() { return reentered; }
    public static boolean sameThread() { return observed!=null && observed==executing; }
    public static boolean sameException() { return observedError==failure; }
    public static boolean aliveInHandler() { return alive; }
    public static Thread worker() { return worker; }
    public static Throwable failure() { return failure; }
    public static Thread.UncaughtExceptionHandler currentDefault() { return Thread.getDefaultUncaughtExceptionHandler(); }
    public static Thread.UncaughtExceptionHandler workerHandler() { return worker.getUncaughtExceptionHandler(); }
    public static boolean setAfterStop() { worker.setUncaughtExceptionHandler(saved); return worker.getUncaughtExceptionHandler()==saved; }
    public static boolean clearAfterStop() { worker.setUncaughtExceptionHandler(null); return worker.getUncaughtExceptionHandler()==null; }
    public static void clearDefault() { Thread.setDefaultUncaughtExceptionHandler(null); }
    public static void dropSavedHandlers() { saved=null; chosen=null; }
    public static void explicitGroup() { Thread thread=Thread.currentThread(); thread.getUncaughtExceptionHandler().uncaughtException(thread,failure); }
    public static boolean defaultNull() { return Thread.getDefaultUncaughtExceptionHandler()==null; }
    public static boolean groupFallback() { return Thread.currentThread().getUncaughtExceptionHandler()!=null && Thread.currentThread().getUncaughtExceptionHandler()!=Thread.getDefaultUncaughtExceptionHandler(); }
    public static void dropOriginalAndHandlerStatics() { failure=null; saved=null; chosen=null; observed=null; executing=null; observedError=null; worker=null; }
}
