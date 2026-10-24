package org.androswitch.contracts;
import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.util.AndroidException;
public final class PackageMetadataProbe {
  public static Context context;
  public static PackageInfo last;
  public static PackageInfo query(String name,int flags)throws PackageManager.NameNotFoundException{
    last=context.getPackageManager().getPackageInfo(name,flags);return last;
  }
  public static void clear(){last=null;}
  public static String source(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.sourceDir;}
  public static long installTime(String name)throws PackageManager.NameNotFoundException{return query(name,0).firstInstallTime;}
  public static Object signatures(String name)throws PackageManager.NameNotFoundException{return query(name,0).signatures;}
  public static int uid(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.uid;}
  public static int flags(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.flags;}
  public static int catchSuperclass(String name){try{query(name,0);return 0;}catch(AndroidException expected){return 1;}}
}
