package org.androswitch.contracts;

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.widget.TextView;

/** Two real VM workers suspend in native code, then resume out of global LIFO order. */
public final class GcOwnersActivity extends Activity {
    private static native void prepare(String directory);
    private static native int barrier(int owner);
    private static native int releaseB(int resultA);
    private static native int finish(int resultB);
    private static native void abortWaits();
    private Handler main;
    private TextView label;
    private boolean nativeReady;

    private static final class Payload {
        final int value;
        Payload(int value) { this.value = value; }
    }

    // Neither local payload is passed to JNI, stored in a field, or captured
    // by a Runnable. Only its live DEX register frame can retain it across GC.
    private static int bodyA() {
        Payload local = new Payload(101);
        if (barrier(0) != 0) throw new IllegalStateException("A native barrier failed");
        System.gc();
        if (local.value != 101) throw new IllegalStateException("A local was lost");
        return local.value;
    }
    private static int bodyB() {
        Payload local = new Payload(202);
        if (barrier(1) != 0) throw new IllegalStateException("B native barrier failed");
        System.gc();
        if (local.value != 202) throw new IllegalStateException("B suspended local was lost");
        return local.value;
    }

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        main = new Handler();
        label = new TextView(this);
        label.setText("GC owners: waiting for two native barriers");
        setContentView(label);
        System.loadLibrary("gcownersproof");
        prepare(getFilesDir().getAbsolutePath());
        nativeReady = true;
        Thread a = new Thread(new Runnable() {
            @Override public void run() {
                int result = bodyA();
                // This JNI call is reached only AFTER bodyA's interpreter
                // frame has unwound, while bodyB remains native-suspended.
                if (releaseB(result) != 0) throw new IllegalStateException("A release failed");
            }
        });
        Thread b = new Thread(new Runnable() {
            @Override public void run() {
                int result = bodyB();
                if (finish(result) != 0) throw new IllegalStateException("Native GC proof failed");
                main.post(new Runnable() {
                    @Override public void run() {
                        label.setText("GC owners PASS: two workers, native suspension, local roots kept");
                    }
                });
            }
        });
        a.setName("GC owner A");
        b.setName("GC owner B");
        a.start();
        b.start();
    }

    @Override public void onPause() {
        // Session stop can always release both native waits before worker
        // joins. Cancellation never publishes the successful proof report.
        if (nativeReady) abortWaits();
        super.onPause();
    }
}
