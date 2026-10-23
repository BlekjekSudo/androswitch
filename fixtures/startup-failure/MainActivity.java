package org.androswitch.startup;

import android.app.Activity;

/** Initialization must fail before any Activity lifecycle callback runs. */
public class MainActivity extends Activity {
    static { System.loadLibrary("load-failure"); }
}
