package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
public class ObservedActivity extends Activity {
  public void onCreate(Bundle state){LifecycleProbe.activity=this;LifecycleProbe.input=state;LifecycleProbe.record("c<");if(LifecycleProbe.mode!=5){if(LifecycleProbe.mode==8){try{super.onCreate(state);}catch(IllegalStateException failure){LifecycleProbe.record("caughtCreate");}}else super.onCreate(state);}LifecycleProbe.record("c>");}
  public void onStart(){LifecycleProbe.record("s<");if(LifecycleProbe.mode==9){try{super.onStart();}catch(IllegalStateException failure){LifecycleProbe.record("caughtStart");}}else super.onStart();LifecycleProbe.record("s>");}
  public void onResume(){LifecycleProbe.record("r<");super.onResume();LifecycleProbe.record("r>");}
  public void onPause(){LifecycleProbe.record("p<");if(LifecycleProbe.mode!=6)super.onPause();LifecycleProbe.record("p>");}
  public void onStop(){LifecycleProbe.record("t<");super.onStop();LifecycleProbe.record("t>");}
  public void onDestroy(){LifecycleProbe.record("d<");super.onDestroy();LifecycleProbe.record("d>");}
  public void onSaveInstanceState(Bundle state){LifecycleProbe.saved=state;state.putInt("before",11);LifecycleProbe.record("v<");if(LifecycleProbe.mode!=7)super.onSaveInstanceState(state);LifecycleProbe.record("v>");state.putInt("after",22);}
}
