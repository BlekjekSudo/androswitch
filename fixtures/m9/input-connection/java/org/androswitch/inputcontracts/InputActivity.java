package org.androswitch.inputcontracts;
import android.app.Activity;
import android.os.Bundle;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.InputConnection;
import android.widget.TextView;
import java.io.FileOutputStream;
public class InputActivity extends Activity {
  @Override
  public void onCreate(Bundle state) {
    super.onCreate(state);
    InputCases.context = this;
    try {
      View target = new View(this);
      if (target.getContext() != this)
        throw new IllegalStateException("real View context");
      BaseInputConnection input = new BaseInputConnection(target, true);
      if (!(input instanceof InputConnection)
          || !Class.forName("android.view.inputmethod.InputConnection").isInterface())
        throw new IllegalStateException("actual InputConnection type");
      if (input.getEditable() != input.getEditable())
        throw new IllegalStateException("editable identity");
      input.commitText("abcdef", 1);
      input.setSelection(2, 4);
      input.commitText("XY", 1);
      if (!"abXYef|4|4".equals(InputCases.snapshot(input)))
        throw new IllegalStateException("local editor state");
      if (!"abXY".equals(input.getTextBeforeCursor(99, 0).toString())
          || !"ef".equals(input.getTextAfterCursor(99, 0).toString()))
        throw new IllegalStateException("cursor queries");
      if (InputCases.callbackProof() != 1)
        throw new IllegalStateException("DEX callbacks");
      InputCases.overrideContext();
      InputCases.glTarget();
      System.gc();
      String proof = "status=PASS\npackage=" + getPackageName()
          + ("\ntext=abXYef\nselection=4:4\nreplace_callback=1\ncontext=actual,virtual\nime_"
             + "delivery=unavailable\n");
      FileOutputStream output =
          new FileOutputStream("/sdcard/Download/androswitch-input-connection-proof.txt");
      output.write(proof.getBytes("UTF-8"));
      output.close();
      TextView view = new TextView(this);
      view.setText("INPUT CONNECTION PASS");
      view.setTextSize(24);
      view.setTextColor(0xffffffff);
      view.setBackgroundColor(0xff253e48);
      setContentView(view);
    } catch (Exception failure) {
      throw new IllegalStateException(failure.getMessage());
    }
  }
}
