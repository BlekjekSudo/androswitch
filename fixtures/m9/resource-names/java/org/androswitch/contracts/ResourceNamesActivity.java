package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.content.res.Resources;

/** Resource ownership must follow ARSC even when only the manifest is renamed. */
public class ResourceNamesActivity extends Activity {
  public static Resources resources;
  public static String manifest;
  public void onCreate(Bundle state){super.onCreate(state);resources=getResources();manifest=getPackageName();}
  public static String manifest(){return manifest;}
  public static int id(int which){
    switch(which){
      case 0:return R.string.greeting;
      case 1:return R.string.greeting_alias;
      case 2:return R.integer.answer;
      case 3:return R.bool.enabled;
      case 4:return R.id.anchor;
      case 5:return R.style.ScopeStyle;
      case 6:return R.layout.screen;
      case 7:return R.string.future_only;
      case 9:return R.integer.greeting;
      case 10:return R.string.with_dot;
      default:return R.string.localized;
    }
  }
  public static String owner(int id){return resources.getResourcePackageName(id);}
  public static String type(int id){return resources.getResourceTypeName(id);}
  public static String entry(int id){return resources.getResourceEntryName(id);}
  public static String full(int id){return resources.getResourceName(id);}
  public static int identifier(String name,String type,String owner){return resources.getIdentifier(name,type,owner);}
  public static boolean missing(int id){try{resources.getResourceName(id);return false;}catch(Resources.NotFoundException error){return true;}}
  public static boolean missingRuntime(int id){try{resources.getResourceName(id);return false;}catch(RuntimeException error){return true;}}
}
