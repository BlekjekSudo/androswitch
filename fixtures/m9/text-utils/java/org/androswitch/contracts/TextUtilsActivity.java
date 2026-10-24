package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
public class TextUtilsActivity extends Activity {
    @Override public void onCreate(Bundle state){
        super.onCreate(state);
        TextFixture.run(10);
        TextView view=new TextView(this);
        view.setText("TextUtils CharSequence contracts");
        setContentView(view);
    }
}
