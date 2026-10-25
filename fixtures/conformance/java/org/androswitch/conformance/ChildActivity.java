package org.androswitch.conformance;
import android.app.Activity;
import android.os.*;
import android.widget.TextView;
public final class ChildActivity extends Activity {
  public void onCreate(Bundle b) {
    super.onCreate(b);
    TextView t = new TextView(this);
    t.setText("Lifecycle test: automatic return");
    setContentView(t);
  }
  public void onResume() {
    super.onResume();
    new Handler().postDelayed(new Runnable() {
      public void run() {
        finish();
      }
    }, 250);
  }
}
