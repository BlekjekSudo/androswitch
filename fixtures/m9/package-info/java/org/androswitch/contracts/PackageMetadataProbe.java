package org.androswitch.contracts;
import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.ProviderInfo;
import android.content.pm.ApplicationInfo;
import android.util.AndroidException;
public final class PackageMetadataProbe {
  public static Context context;
  public static PackageInfo last;
  public static ProviderInfo lastProvider;
  public static ApplicationInfo lastApplication;
  public static boolean feature(String name){return context.getPackageManager().hasSystemFeature(name);}
  public static boolean featureVersion(String name,int version){return context.getPackageManager().hasSystemFeature(name,version);}
  public static ApplicationInfo application(String name,int flags)throws PackageManager.NameNotFoundException{
    lastApplication=context.getPackageManager().getApplicationInfo(name,flags);return lastApplication;
  }
  public static ProviderInfo provider(String authority,int flags){
    lastProvider=context.getPackageManager().resolveContentProvider(authority,flags);return lastProvider;
  }
  public static PackageInfo query(String name,int flags)throws PackageManager.NameNotFoundException{
    last=context.getPackageManager().getPackageInfo(name,flags);return last;
  }
  public static void clear(){last=null;lastProvider=null;lastApplication=null;}
  public static String source(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.sourceDir;}
  public static long installTime(String name)throws PackageManager.NameNotFoundException{return query(name,0).firstInstallTime;}
  public static Object signatures(String name)throws PackageManager.NameNotFoundException{return query(name,0).signatures;}
  public static int uid(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.uid;}
  public static int flags(String name)throws PackageManager.NameNotFoundException{return query(name,0).applicationInfo.flags;}
  public static int catchSuperclass(String name){try{query(name,0);return 0;}catch(AndroidException expected){return 1;}}
}
