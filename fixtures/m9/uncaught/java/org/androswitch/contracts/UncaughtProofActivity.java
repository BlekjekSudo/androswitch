package org.androswitch.contracts;
public final class UncaughtProofActivity extends UncaughtActivity {
    public void onCreate(android.os.Bundle state) {
        super.onCreate(state);
        android.widget.TextView text=new android.widget.TextView(this);
        text.setText("SDK uncaught worker proof: waiting for handler/VFS"); setContentView(text);
        UncaughtCases.setup(1); UncaughtCases.proof=true; UncaughtCases.start(0,0);
    }
}
