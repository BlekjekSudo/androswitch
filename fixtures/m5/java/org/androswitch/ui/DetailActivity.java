package org.androswitch.ui;
import android.app.Activity;
import android.os.Bundle;
import android.widget.*;
import android.view.View;
public final class DetailActivity extends Activity implements View.OnClickListener {
    @Override public void onCreate(Bundle state){super.onCreate(state);LinearLayout root=new LinearLayout(this);root.setOrientation(1);TextView text=new TextView(this);text.setText("Second activity / count "+getIntent().getIntExtra("count",-1));root.addView(text);Button back=new Button(this);back.setText("Return");back.setOnClickListener(this);root.addView(back);setContentView(root);}
    public void onClick(View view){finish();}
}
