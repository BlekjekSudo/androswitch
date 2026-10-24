package org.androswitch.contracts;
import android.content.SharedPreferences;
import android.opengl.GLSurfaceView;
import javax.microedition.khronos.egl.EGL10;
import javax.microedition.khronos.egl.EGLDisplay;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;
/** Only contracts backed by real existing AndroSwitch callback providers. */
public final class ListenerContracts implements SharedPreferences.OnSharedPreferenceChangeListener,GLSurfaceView.Renderer,GLSurfaceView.EGLConfigChooser {
  public void onSharedPreferenceChanged(SharedPreferences preferences,String key){ }
  public void onSurfaceCreated(GL10 gl,EGLConfig config){ }
  public void onSurfaceChanged(GL10 gl,int width,int height){ }
  public void onDrawFrame(GL10 gl){ }
  public EGLConfig chooseConfig(EGL10 egl,EGLDisplay display){return null;}
}
