package org.androswitch.media;
import android.app.Activity;
import android.os.Bundle;
public final class UnavailableActivity extends Activity {
    public void onCreate(Bundle state){super.onCreate(state);getSystemService("unsupported-service-proof");}
}
