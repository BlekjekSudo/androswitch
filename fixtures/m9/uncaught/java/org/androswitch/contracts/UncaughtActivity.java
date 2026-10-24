package org.androswitch.contracts;
public class UncaughtActivity extends android.app.Activity {
    public void onCreate(android.os.Bundle state) { super.onCreate(state); UncaughtCases.activity=this; }
}
