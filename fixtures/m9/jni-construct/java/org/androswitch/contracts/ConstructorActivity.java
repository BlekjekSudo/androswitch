package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
import java.io.FileOutputStream;
public class ConstructorActivity extends Activity {
  public void onCreate(Bundle saved){
    super.onCreate(saved);TextView text=new TextView(this);text.setTextSize(18);setContentView(text);
    try{
      String result="";
      int identity=ConstructorCases.threadIdentity();int attached=ConstructorCases.attachedThread();
      if(identity!=89||attached!=97)throw new AssertionError("JNI Thread identity/lifetime mismatch");
      result+="threads identity="+identity+"\nthreads attached="+attached+"\n";
      int context=ConstructorCases.contextLoader();
      if(context!=103)throw new AssertionError("JNI Thread context loader mismatch");
      result+="threads context="+context+"\n";
      for(int form=0;form<3;form++){
        int value=ConstructorCases.run(form,0);int reentry=ConstructorCases.run(form,2);
        // Frozen actual JVM JNI results for this original fixture. The builder
        // independently checks all forms against its newly executed JVM oracle.
        if(value!=-1985187208||reentry!=-1985187171)throw new AssertionError("constructor numeric mismatch");
        result+="value "+form+"="+value+"\n";
        result+="reentry "+form+"="+reentry+"\n";
        try{ConstructorCases.run(form,1);throw new AssertionError("missing constructor exception");}
        catch(IllegalStateException expected){if(!"constructor failure".equals(expected.getMessage()))throw new AssertionError("constructor exception message");result+="exception "+form+"="+expected.getMessage()+"\n";}
        if(ConstructorCases.exceptionProbe(form)!=71)throw new AssertionError("native exception probe");
      }
      result+="JNI_CONSTRUCTOR_PROOF_PASS\n";
      FileOutputStream file=new FileOutputStream("/sdcard/Download/androswitch-jni-constructor-proof.txt");
      file.write(result.getBytes("UTF-8"));file.close();text.setText("JNI constructor proof PASS: NewObject/A/V, GC, reentry, exceptions");
    }catch(Throwable error){text.setText("JNI constructor proof FAIL: "+error.getClass().getName()+": "+error.getMessage());throw new RuntimeException(error);}
  }
}
