package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
public class LifecycleProofActivity extends Activity {
  public void onCreate(Bundle state){LifecycleProof.activity=this;LifecycleProof.creationState=state;LifecycleProof.mark("c<");super.onCreate(state);LifecycleProof.mark("c>");}
  public void onStart(){LifecycleProof.mark("s<");super.onStart();LifecycleProof.mark("s>");}
  public void onResume(){
    LifecycleProof.mark("r<");super.onResume();LifecycleProof.mark("r>");
    String expected="c<;C;c>;s<;S;s>;r<;R;r>;";
    if(!LifecycleProof.trace.equals(expected))throw new IllegalStateException("startup observer order: "+LifecycleProof.trace);
    LifecycleProof.startup=true;
    TextView text=new TextView(this);text.setText("LIFECYCLE STARTUP PASS\nC/S/R observer order verified\nExit to record P/T/D\nSave is not observed until a real save callback");text.setTextSize(23);text.setTextColor(0xffffffff);text.setBackgroundColor(0xff203040);setContentView(text);
    LifecycleProof.write();
  }
  public void onPause(){LifecycleProof.mark("p<");super.onPause();LifecycleProof.mark("p>");LifecycleProof.write();}
  public void onStop(){LifecycleProof.mark("t<");super.onStop();LifecycleProof.mark("t>");LifecycleProof.write();}
  public void onDestroy(){
    LifecycleProof.mark("d<");super.onDestroy();LifecycleProof.mark("d>");
    if(!LifecycleProof.trace.endsWith("p<;P;p>;t<;T;t>;d<;D;d>;"))throw new IllegalStateException("return observer order: "+LifecycleProof.trace);
    LifecycleProof.returned=true;LifecycleProof.write();
  }
  public void onSaveInstanceState(Bundle state){LifecycleProof.savingState=state;state.putInt("before",73);LifecycleProof.mark("v<");super.onSaveInstanceState(state);LifecycleProof.mark("v>");if(state.getInt("observer")!=91)throw new IllegalStateException("missing save observer");LifecycleProof.write();}
}
