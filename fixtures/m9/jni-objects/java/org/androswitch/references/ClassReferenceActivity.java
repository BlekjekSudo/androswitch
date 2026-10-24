package org.androswitch.references;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
public final class ClassReferenceActivity extends Activity {
    public void onCreate(Bundle saved) {
        super.onCreate(saved);
        TextView text = new TextView(this);
        Class<?> own = ClassReferenceCases.own();
        if (ClassReferenceCases.echo(own) != own || ClassReferenceCases.echoObject(own) != own)
            throw new AssertionError("Class object identity");
        text.setText("Class reference contract: " + own.getName());
        setContentView(text);
    }
}
