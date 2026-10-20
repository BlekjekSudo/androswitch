package org.rustydroid.nativeworkbench;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
public final class DetailsActivity extends Activity {
 @Override public void onCreate(Bundle state) {super.onCreate(state);TextView text=new TextView(this);text.setText("Native proof "+MainActivity.LANGUAGE);text.setTextSize(26);setContentView(text);}
}
