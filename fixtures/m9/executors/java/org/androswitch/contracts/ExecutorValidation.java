package org.androswitch.contracts;
import java.util.concurrent.*;
/** Same source runs on the actual host JVM and as unmodified DEX. */
public class ExecutorValidation {
  public static int run(int test){
    ThreadPoolExecutor pool=null;
    try{
      if(test==0)pool=new ThreadPoolExecutor(-1,1,1,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());
      else if(test==1)pool=new ThreadPoolExecutor(0,0,1,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());
      else if(test==2)pool=new ThreadPoolExecutor(2,1,1,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());
      else if(test==3)pool=new ThreadPoolExecutor(1,1,-1,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());
      else if(test==4){pool=new ThreadPoolExecutor(1,1,0,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());pool.allowCoreThreadTimeOut(true);}
      else if(test==5)return Executors.defaultThreadFactory().newThread(null).isAlive()?0:1;
      else if(test==6){Thread thread=new Thread();thread.start();thread.start();}
      else if(test==7)new Thread().setName(null);
      else if(test==8)new LinkedBlockingQueue<Object>().offer(null);
      else if(test==9){pool=new ThreadPoolExecutor(1,1,60,TimeUnit.SECONDS,new LinkedBlockingQueue<Runnable>(),Executors.defaultThreadFactory());pool.shutdown();pool.execute(new Runnable(){public void run(){}});}
      else if(test==10)Executors.unconfigurableExecutorService(null);
      else if(test==11){LinkedBlockingQueue<String> queue=new LinkedBlockingQueue<String>();queue.offer("first");queue.offer("second");return queue.poll().equals("first")&&queue.poll().equals("second")&&queue.isEmpty()?1:0;}
      return 0;
    }catch(NullPointerException error){return 1;}
     catch(IllegalThreadStateException error){return 3;}
     catch(IllegalArgumentException error){return 2;}
     catch(RejectedExecutionException error){return 4;}
     finally{if(pool!=null)pool.shutdown();}
  }
  public static void main(String[] args){for(int test=0;test<12;test++)System.out.println(test+":"+run(test));}
}
