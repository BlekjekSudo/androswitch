package org.androswitch.fields;
import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
import java.io.FileOutputStream;
import java.lang.reflect.Field;
/** Launchable first-party proof, with no native test hooks. */
public class FieldActivity extends Activity {
  public void onCreate(Bundle state) {
    super.onCreate(state);
    try {
      Field label=FieldCases.Data.class.getDeclaredField("label");
      Field number=FieldCases.Data.class.getDeclaredField("constantNumber");
      Field inherited=FieldCases.Data.class.getField("inherited");
      if(!"field-contract".equals(label.get(null))||number.getInt(null)!=61)throw new IllegalStateException("static values");
      if(label.getDeclaringClass()!=FieldCases.Data.class||label.getType()!=String.class||label.getModifiers()!=25)throw new IllegalStateException("field metadata");
      if(inherited.getInt(new FieldCases.Data())!=11||inherited.getDeclaringClass()!=FieldCases.Base.class)throw new IllegalStateException("instance inheritance");
      Field secret=FieldCases.Data.class.getDeclaredField("hidden");secret.setAccessible(true);
      if(secret.getInt(new FieldCases.Data())!=47)throw new IllegalStateException("access override");
      try{FieldCases.Data.class.getDeclaredField("missing");throw new IllegalStateException("fabricated field");}catch(NoSuchFieldException expected){}
      String proof="status=PASS\npackage="+getPackageName()+"\nstatic=field-contract:61\ninstance=11\nmetadata=owner,type,modifiers\naccess=override\nmissing=NoSuchFieldException\n";
      FileOutputStream output=new FileOutputStream("/sdcard/Download/androswitch-field-reflection-proof.txt");output.write(proof.getBytes("UTF-8"));output.close();
      TextView view=new TextView(this);view.setText("FIELD REFLECTION PASS");view.setTextSize(24);view.setTextColor(0xffffffff);view.setBackgroundColor(0xff284050);setContentView(view);
    }catch(Exception failure){throw new IllegalStateException(failure.getMessage());}
  }
}
