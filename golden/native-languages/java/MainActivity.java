package org.rustydroid.nativeworkbench;
import android.app.*;
import android.content.*;
import android.content.pm.*;
import android.graphics.Color;
import android.os.Bundle;
import android.view.View;
import android.widget.*;
import java.util.Arrays;
public final class MainActivity extends Activity implements View.OnClickListener {
    public static final String LANGUAGE="@LANG@";
    static { System.loadLibrary("rdcalc"); }
    public static native int digest(int a,int b);
    public static native int nativeFlavor();
    private EditText left,right; private TextView result;
    private Button compute,notifyButton,shortcut,details,finishButton;
    private SharedPreferences preferences;
    private Button button(String text,int id,LinearLayout root) {
        Button b=new Button(this);b.setId(id);b.setText(text);b.setOnClickListener(this);
        root.addView(b,new LinearLayout.LayoutParams(-1,-2));return b;
    }
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);preferences=getSharedPreferences("proof",0);
        LinearLayout root=new LinearLayout(this);root.setOrientation(1);root.setPadding(24,24,24,24);root.setBackgroundColor(Color.WHITE);
        TextView heading=new TextView(this);heading.setText(LANGUAGE+" native workbench");heading.setTextSize(24);heading.setTextColor(Color.BLACK);root.addView(heading);
        left=new EditText(this);left.setId(R.id.left);left.setInputType(2);left.setText("7");root.addView(left,new LinearLayout.LayoutParams(-1,-2));
        right=new EditText(this);right.setId(R.id.right);right.setInputType(2);right.setText("5");root.addView(right,new LinearLayout.LayoutParams(-1,-2));
        compute=button("Compute "+LANGUAGE,R.id.compute,root);
        result=new TextView(this);result.setId(R.id.result);result.setTextColor(Color.BLACK);result.setTextSize(20);
        int boot=digest(7,5);result.setText("Result: "+preferences.getInt("last",boot));root.addView(result);
        notifyButton=button("Notify "+LANGUAGE,R.id.notify,root);
        shortcut=button("Pin shortcut "+LANGUAGE,R.id.shortcut,root);
        details=button("Open details "+LANGUAGE,R.id.details,root);
        finishButton=button("Finish "+LANGUAGE,R.id.finish,root);
        setContentView(root);System.out.println("Native "+LANGUAGE+" flavor="+nativeFlavor()+" boot="+boot);
    }
    public void onClick(View v) {
        if(v==compute) {
            int a=Integer.parseInt(left.getText().toString());int b=Integer.parseInt(right.getText().toString());
            int n=digest(a,b);preferences.edit().putInt("last",n).apply();result.setText("Result: "+n);System.out.println("Native result="+n);
        } else if(v==notifyButton) postNotification();
        else if(v==shortcut) postShortcut();
        else if(v==details) startActivity(new Intent(this,DetailsActivity.class));
        else if(v==finishButton) finish();
    }
    private void postNotification() {
        NotificationManager nm=(NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        nm.createNotificationChannel(new NotificationChannel("proof","Native proof",3));
        Intent intent=new Intent(this,DetailsActivity.class);
        PendingIntent pi=PendingIntent.getActivity(this,1,intent,0x0c000000);
        Notification n=new Notification.Builder(this,"proof").setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle(LANGUAGE+" executed").setContentText(result.getText()).setContentIntent(pi).build();
        nm.notify(17,n);
    }
    private void postShortcut() {
        Intent intent=new Intent(this,DetailsActivity.class);intent.setAction(Intent.ACTION_VIEW);
        ShortcutInfo info=new ShortcutInfo.Builder(this,"native-proof").setShortLabel(LANGUAGE+" proof").setIntent(intent).build();
        ShortcutManager sm=(ShortcutManager)getSystemService(SHORTCUT_SERVICE);sm.setDynamicShortcuts(Arrays.asList(info));
    }
}
