package org.androswitch.contracts;
import android.app.Activity;
import android.content.Context;
import android.content.res.Resources;
import android.os.Bundle;
public class DispatchActivity extends DispatchActivityMiddle {
  public static DispatchActivity instance;
  public static Resources bootResources;
  public static int overrides;
  public static int statics;
  public void onCreate(Bundle state) {
    super.onCreate(state);
    instance=this;
    bootResources=throughInterface(this);
  }
  public static Resources throughInterface(ResourceChild value) { return value.getResources(); }
  public static Resources throughMiddle(DispatchActivityMiddle value) { return value.getResources(); }
  public static Resources throughConcrete(DispatchActivity value) { return value.getResources(); }
  public static Resources throughContext(Context value) { return value.getResources(); }
  public static int marker(ResourceChild value) { return value.marker(); }
  public static int staticIdentity() { return 73; }
  public static Resources throughStatic(DispatchActivity value) { statics++;return value.getResources(); }
  public static ResourceChild makeOverride() { return new ResourceOverride(); }
  public static ResourceChild makeNative() { return new NativeResources(); }
}
interface ResourceContract {
  Resources getResources();
  default int marker() { return 17; }
}
interface ResourceChild extends ResourceContract {}
abstract class DispatchActivityBase extends Activity implements ResourceChild {}
abstract class DispatchActivityMiddle extends DispatchActivityBase {}
class ResourceOverride extends DispatchActivityMiddle {
  public Resources getResources() { DispatchActivity.overrides++;return super.getResources(); }
}
class NativeResources extends DispatchActivityMiddle {
  public native Resources getResources();
}
abstract class AbstractBarrier extends DispatchActivityMiddle {
  public abstract Resources getResources();
}
abstract class AbstractUnbacked implements ResourceChild {}
