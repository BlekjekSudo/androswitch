package org.androswitch.graphics;

import android.app.Activity;
import android.app.ActivityManager;
import android.opengl.GLSurfaceView;
import android.os.Bundle;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.MotionEvent;
import android.view.View;
import android.widget.FrameLayout;
import javax.microedition.khronos.egl.EGL10;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.egl.EGLDisplay;
import javax.microedition.khronos.opengles.GL10;

/** Independent Android API fixture; no native code or runtime-specific APIs. */
public final class SurfaceActivity extends Activity {
    public static GLSurfaceView surface;
    private static String events = "";
    private static int requestNext;
    private static boolean askThread;
    private static boolean askGl;
    private static int initialOrientation = Integer.MIN_VALUE;
    private static boolean softwareMode;
    public static void configureInitialOrientation(int requested, boolean software) {
        initialOrientation = requested; softwareMode = software;
    }
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        if (initialOrientation != Integer.MIN_VALUE) setRequestedOrientation(initialOrientation);
        DisplayMetrics metrics = new DisplayMetrics();
        getWindowManager().getDefaultDisplay().getMetrics(metrics);
        int version = ((ActivityManager)getSystemService("activity"))
            .getDeviceConfigurationInfo().reqGlEsVersion;
        events += "M" + metrics.widthPixels + ":" + metrics.heightPixels + ":"
            + metrics.density + ":" + version + ";";
        if (softwareMode) {
            FrameLayout root = new FrameLayout(this);
            root.setBackgroundColor(0xff336699);
            setContentView(root);
            return;
        }
        surface = new GLSurfaceView(this);
        surface.setEGLContextClientVersion(2);
        surface.setEGLConfigChooser(new Chooser());
        surface.setRenderer(new Renderer());
        surface.setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
        surface.queueEvent(new Runnable() {
            public void run() {
                if (Looper.myLooper() != null) throw new IllegalStateException("render looper");
                events += "Q;";
            }
        });
        setContentView(surface);
    }
    public static String status() { return events; }
    public static void requestDuringNextFrame() { requestNext = 1; }
    public static void queryThreadDuringNextFrame() { askThread = true; }
    public static void queryGlDuringNextFrame() { askGl = true; }
    @Override public boolean onTouchEvent(MotionEvent event) {
        events += "T" + event.getActionMasked() + ":" + event.getPointerCount() + ":"
            + event.getX(0) + ":" + event.getY(0) + ";";
        return true;
    }
    public static void consumeTouches() {
        surface.setOnTouchListener(new View.OnTouchListener() {
            public boolean onTouch(View view, MotionEvent event) { events += "V;"; return true; }
        });
    }
    private static final class Chooser implements GLSurfaceView.EGLConfigChooser {
        public EGLConfig chooseConfig(EGL10 egl, EGLDisplay display) {
            int[] attributes = {EGL10.EGL_RED_SIZE,8,EGL10.EGL_GREEN_SIZE,8,
                EGL10.EGL_BLUE_SIZE,8,EGL10.EGL_DEPTH_SIZE,16,
                EGL10.EGL_RENDERABLE_TYPE,4,EGL10.EGL_NONE};
            int[] count = {0};
            if (!egl.eglChooseConfig(display,attributes,null,0,count))
                throw new IllegalArgumentException("EGL count");
            EGLConfig[] configs = new EGLConfig[count[0]];
            if (!egl.eglChooseConfig(display,attributes,configs,configs.length,count))
                throw new IllegalArgumentException("EGL configs");
            int[] depth = {0};
            if (!egl.eglGetConfigAttrib(display,configs[0],EGL10.EGL_DEPTH_SIZE,depth))
                throw new IllegalArgumentException("EGL depth");
            events += "C" + depth[0] + ";";
            return configs[0];
        }
    }
    private static final class Renderer implements GLSurfaceView.Renderer {
        public void onSurfaceCreated(GL10 gl, EGLConfig config) {
            if (gl == null || config == null) throw new IllegalStateException("missing context");
            if (Looper.myLooper() != null) throw new IllegalStateException("render looper");
            events += "S;";
        }
        public void onSurfaceChanged(GL10 gl, int width, int height) {
            events += "Z" + width + ":" + height + ";";
        }
        public void onDrawFrame(GL10 gl) {
            if (askThread) Thread.currentThread();
            if (askGl) gl.glClear(GL10.GL_COLOR_BUFFER_BIT);
            events += "D;";
            if (requestNext != 0) { requestNext = 0; surface.requestRender(); }
        }
    }
}
