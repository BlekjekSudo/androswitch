package org.androswitch.ui;
import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.content.Intent;
import android.view.View;
import android.widget.TextView;
import java.io.*;
public final class MainActivity extends Activity implements View.OnClickListener {
    private TextView label;
    private int count;
    private boolean ready;
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        setContentView(R.layout.main);
        label=(TextView)findViewById(R.id.counter);
        findViewById(R.id.add).setOnClickListener(this);
        findViewById(R.id.next).setOnClickListener(this);
        try {
            File save=new File(getFilesDir(),"counter");
            if(save.exists()) {FileInputStream in=new FileInputStream(save);count=in.read();in.close();}
        } catch(IOException e) {throw new RuntimeException(e);}
        update();
        new Handler().postDelayed(new Runnable(){public void run(){ready=true;update();}},1200);
    }
    private void update(){label.setText((ready?"Ready ":"Count ")+count+" / app "+((ProofApplication)getApplication()).created);}
    public void onClick(View view) {
        if(view.getId()==R.id.next){Intent next=new Intent(this,DetailActivity.class);next.putExtra("count",count);startActivity(next);return;}
        count++;
        try {FileOutputStream out=new FileOutputStream(new File(getFilesDir(),"counter"));out.write(count);out.close();} catch(IOException e){throw new RuntimeException(e);}
        update();
    }
}
