package org.androswitch.timeunits;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
import java.io.FileOutputStream;

/** Normal Android SDK application; no runtime-specific classes or hooks. */
public final class TimeUnitActivity extends Activity {
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        String proof = TimeUnitCases.proofRows();
        try {
            FileOutputStream output = new FileOutputStream("/sdcard/Download/androswitch-timeunit-proof.txt");
            output.write(proof.getBytes("UTF-8"));
            output.close();
        } catch (Exception failure) { throw new RuntimeException("TimeUnit proof write failed", failure); }
        TextView label = new TextView(this);
        label.setText("TimeUnit proof PASS: 7 units, conversion boundaries, Enum identity\n" + proof);
        setContentView(label);
    }
}
