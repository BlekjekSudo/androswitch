package org.androswitch.contracts;
public final class UncaughtMainActivity extends UncaughtActivity {
    public void onCreate(android.os.Bundle state) {
        super.onCreate(state); UncaughtCases.setup(0); UncaughtCases.throwMain();
    }
}
