package org.androswitch.contracts;
import java.util.Timer;
import java.util.TimerTask;
/** The exact same compiled source runs under the host JVM and DEX VM. */
public class TimerValidation {
  public static int run(int test) {
    Timer timer=new Timer(true);
    TimerTask task=new TimerTask() { public void run() {} };
    try {
      if(test==0) new Timer((String)null);
      else if(test==1) timer.schedule(task,-1);
      else if(test==2) timer.schedule(task,100000,0);
      else if(test==3) timer.scheduleAtFixedRate(task,100000,-1);
      else if(test==4) { timer.schedule(task,100000);timer.schedule(task,100000); }
      else if(test==5) { task.cancel();timer.schedule(task,100000); }
      else if(test==6) { timer.cancel();timer.schedule(task,100000); }
      else if(test==7) return task.cancel() ? 11:10;
      else if(test==8) { timer.schedule(task,100000);return task.cancel() ? 11:10; }
      else if(test==9) { timer.schedule(task,100000);timer.cancel();return task.cancel() ? 11:10; }
      else if(test==10) { timer.schedule(task,100000);task.cancel();return timer.purge()*10+timer.purge(); }
      else if(test==11) { timer.schedule(task,100000);task.cancel();return task.cancel() ? 11:10; }
      return 0;
    } catch(NullPointerException error) { return 1; }
      catch(IllegalArgumentException error) { return 2; }
      catch(IllegalStateException error) { return 3; }
      finally { timer.cancel(); }
  }
  public static void main(String[] args) {
    for(int test=0;test<12;test++) System.out.println(test+":"+run(test));
  }
}
