package org.androswitch.graphics.portrait;

import android.app.Activity;
import android.content.pm.ActivityInfo;
import android.opengl.GLSurfaceView;
import android.os.Bundle;
import android.view.MotionEvent;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

/** Ordinary Android API26 APK: no runtime-specific calls or package profiles. */
public final class PortraitActivity extends Activity {
    static { System.loadLibrary("portraitproof"); }
    private GLSurfaceView surface;
    private static native void prepare(String files);
    private static native int created();
    private static native int changed(int width, int height);
    private static native int draw();
    private static native void touch(int action, float x, float y);
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT);
        prepare(getFilesDir().getAbsolutePath());
        surface = new GLSurfaceView(this);
        surface.setEGLContextClientVersion(2);
        surface.setEGLConfigChooser(8,8,8,8,16,0);
        surface.setRenderer(new GLSurfaceView.Renderer() {
            public void onSurfaceCreated(GL10 gl, EGLConfig config) {
                if (gl == null || config == null || created() != 0)
                    throw new IllegalStateException("portrait EGL context missing");
            }
            public void onSurfaceChanged(GL10 gl, int width, int height) {
                int result = changed(width,height);
                if (result != 0) throw new IllegalStateException("portrait EGL surface mismatch: " + result);
            }
            public void onDrawFrame(GL10 gl) {
                int result = draw();
                if (result != 0) throw new IllegalStateException("portrait GLES verification: " + result);
            }
        });
        setContentView(surface);
    }
    @Override public boolean onTouchEvent(MotionEvent event) {
        touch(event.getActionMasked(),event.getX(),event.getY());
        return true;
    }
    @Override public void onPause() {
        if (surface != null) surface.onPause();
        super.onPause();
    }
    @Override public void onResume() {
        super.onResume();
        if (surface != null) surface.onResume();
    }
}
