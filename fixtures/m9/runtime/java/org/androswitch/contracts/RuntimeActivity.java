package org.androswitch.contracts;
public class RuntimeActivity extends android.app.Activity {
    public void onCreate(android.os.Bundle state) {
        super.onCreate(state);
        android.widget.TextView text = new android.widget.TextView(this);
        text.setText("Runtime contract fixture");
        setContentView(text);
    }
}
