package org.androswitch.contracts;
import android.app.Activity;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.widget.TextView;
import java.io.FileInputStream;
import java.io.FileOutputStream;
public class PackageMetadataActivity extends Activity {
  public void onCreate(Bundle state){
    super.onCreate(state);PackageMetadataProbe.context=this;
    try{
      String name=getPackageName();PackageInfo own=PackageMetadataProbe.query(name,0);
      if(!name.equals(own.packageName)||own.versionCode!=7||!"2.4-proof".equals(own.versionName))throw new IllegalStateException("package/version facts");
      ApplicationInfo app=own.applicationInfo;
      if(!name.equals(app.packageName)||app.minSdkVersion!=26||app.targetSdkVersion!=26)throw new IllegalStateException("application facts");
      if(!getPackageCodePath().equals(app.sourceDir)||!app.sourceDir.equals(app.publicSourceDir))throw new IllegalStateException("mounted APK identity");
      FileInputStream input=new FileInputStream(app.sourceDir);int p=input.read(),k=input.read();input.close();
      if(p!='P'||k!='K')throw new IllegalStateException("source APK unreadable");
      String missing=name+".missing";
      try{PackageMetadataProbe.query(missing,0);throw new IllegalStateException("fabricated package");}catch(PackageManager.NameNotFoundException expected){if(!missing.equals(expected.getMessage()))throw new IllegalStateException("unknown package message");}
      try{PackageMetadataProbe.query(null,0);throw new IllegalStateException("fabricated null package");}catch(PackageManager.NameNotFoundException expected){if(expected.getMessage()!=null)throw new IllegalStateException("null package message");}
      if(PackageMetadataProbe.catchSuperclass(missing)!=1)throw new IllegalStateException("AndroidException ancestry");
      String proof="status=PASS\npackage="+name+"\nversionCode="+own.versionCode+"\nversionName="+own.versionName+"\nsource="+app.sourceDir+"\napk_header=PK\nunknown=NameNotFoundException\nnull=NameNotFoundException\n";
      FileOutputStream output=new FileOutputStream("/sdcard/Download/androswitch-package-info-proof.txt");output.write(proof.getBytes("UTF-8"));output.close();
      TextView text=new TextView(this);text.setText("PACKAGE INFO PASS\n"+name+"\n"+own.versionName+" / "+own.versionCode+"\nInstalled APK header and typed failures verified");text.setTextSize(22);text.setTextColor(0xffffffff);text.setBackgroundColor(0xff203040);setContentView(text);
    }catch(Exception failure){throw new IllegalStateException(failure.getMessage());}
  }
}
