package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.os.Looper;
import java.util.concurrent.*;

/** Ordinary SDK contracts; host controls delivery, never the task implementation. */
public class ExecutorActivity extends Activity {
  public static ThreadPoolExecutor pool;
  public static ExecutorService service;
  public static LinkedBlockingQueue<Runnable> queue;
  public static ThreadFactory factory;
  public static Thread saved;
  public static Thread seen;
  public static String seenName;
  public static Runnable task;
  public static int count,order,factories;
  public static boolean noLooper;
  public void onCreate(Bundle state) {super.onCreate(state);}
  public static class Factory implements ThreadFactory {
    final int mode;
    final ThreadFactory defaults=Executors.defaultThreadFactory();
    Factory(int mode){this.mode=mode;}
    public Thread newThread(Runnable worker){
      factories++;
      if(mode==1)return null;
      if(mode==2)return new Thread(new Job(8,0));
      if(mode==3)return new Thread(worker){public void run(){super.run();}};
      if(mode==4)throw new IllegalStateException("factory failed");
      saved=defaults.newThread(worker);
      saved.setName("contract pool worker");
      return saved;
    }
  }
  public static void create(int mode,long keepAlive,boolean timeout){
    queue=new LinkedBlockingQueue<Runnable>();
    factory=mode<0 ? Executors.defaultThreadFactory():new Factory(mode);
    pool=new ThreadPoolExecutor(1,1,keepAlive,TimeUnit.SECONDS,queue,factory);
    pool.allowCoreThreadTimeOut(timeout);
    service=Executors.unconfigurableExecutorService(pool);
  }
  public static void unsupportedPool(){
    new ThreadPoolExecutor(2,2,60,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());
  }
  public static void enqueue(int id,int mode){task=new Job(id,mode);service.execute(task);}
  public static void shutdown(){service.shutdown();}
  public static boolean shutdownFlag(){return service.isShutdown();}
  public static boolean terminated(){return service.isTerminated();}
  public static void shutdownNow(){service.shutdownNow();}
  public static int queueSize(){return queue.size();}
  public static long completed(){return pool.getCompletedTaskCount();}
  public static int poolSize(){return pool.getPoolSize();}
  public static int count(){return count;}
  public static int order(){return order;}
  public static int factoryCount(){return factories;}
  public static Thread saved(){return saved;}
  public static Runnable task(){return task;}
  public static ThreadPoolExecutor owner(){return pool;}
  public static void forget(){pool=null;service=null;queue=null;factory=null;saved=null;task=null;}
  public static String name(){return seenName;}
  public static Thread seen(){return seen;}
  public static boolean noLooper(){return noLooper;}
  public static void thread(int mode){
    if(mode==0)saved=new Thread(new Job(7,6),"standalone worker");
    else saved=new Thread("subclass worker"){public void run(){count+=10;seen=Thread.currentThread();seenName=seen.getName();}};
  }
  public static void start(){saved.start();}
  public static void runInline(){saved.run();}
  public static boolean alive(){return saved.isAlive();}
  public static void handler(){new android.os.Handler();}
  public static native void nativeHop();
  public static class Job implements Runnable {
    final int id,mode;
    Job(int id,int mode){this.id=id;this.mode=mode;}
    public void run(){
      count++;order=order*10+id;noLooper=Looper.myLooper()==null;
      if(mode==1){service.execute(new Job(9,0));service.shutdown();}
      else if(mode==2){int zero=count-count;count=count/zero;}
      else if(mode==3)System.gc();
      else if(mode==4)nativeHop();
      else if(mode==6){seen=Thread.currentThread();seenName=seen.getName();}
    }
  }
}
