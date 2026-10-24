package org.androswitch.contracts;
import java.io.FileOutputStream;
public final class RuntimeProofActivity extends android.app.Activity {
    public void onCreate(android.os.Bundle state) {
        super.onCreate(state);
        Runtime runtime = Runtime.getRuntime();
        int first = runtime.availableProcessors();
        System.gc();
        boolean singleton = runtime == Runtime.getRuntime();
        int second = Runtime.getRuntime().availableProcessors();
        if (!singleton || first <= 0 || second <= 0)
            throw new IllegalStateException("runtime-proof-capability");
        try {
            FileOutputStream output = new FileOutputStream("/sdcard/Download/androswitch-runtime-proof.txt");
            String report = "RUNTIME_PROOF_PASS\nsingleton=" + singleton + "\nfirstAvailableProcessors=" + first + "\navailableProcessors=" + second + "\n";
            output.write(report.getBytes("UTF-8"));
            output.close();
        } catch (java.io.IOException failure) {
            throw new IllegalStateException("runtime-proof-io");
        }
        android.widget.TextView text = new android.widget.TextView(this);
        text.setText("Runtime proof PASS: " + second + " available processors; VFS saved");
        setContentView(text);
    }
}
