package org.androswitch.contracts;

import android.app.Activity;
import android.content.Context;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.os.Bundle;
import android.view.View;

/** Original PNG/resource/raster contracts. No external application assets. */
public class BitmapActivity extends Activity {
    public static Resources resources;
    public static Bitmap bitmap;
    public static Bitmap density;
    public static Bitmap alpha;
    public static Bitmap[] retained;
    public static int rectResult;
    public static int draws;
    public static int mode;
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        resources = getResources();
        bitmap = BitmapFactory.decodeResource(resources, R.drawable.quadrants);
        density = BitmapFactory.decodeResource(resources, R.drawable.alias);
        alpha = BitmapFactory.decodeResource(resources, R.drawable.palette);
        setContentView(new BitmapView(this));
    }
    public static Bitmap decode(int id) { return BitmapFactory.decodeResource(resources, id); }
    public static Bitmap options() { return BitmapFactory.decodeResource(resources, R.drawable.quadrants, new BitmapFactory.Options()); }
    public static Bitmap nullOptions() { return BitmapFactory.decodeResource(resources, R.drawable.quadrants, null); }
    public static Bitmap nullResources() { return BitmapFactory.decodeResource(null, R.drawable.quadrants); }
    public static int densityId() {return R.drawable.density;}
    public static int futureId() {return R.drawable.versioned;}
    public static int corruptId() {return R.raw.corrupt;}
    public static int unsupportedId() {return R.raw.unsupported;}
    public static int hugeId() {return R.raw.huge;}
    public static int localeId() {return R.drawable.localized;}
    public static int orientationId() {return R.drawable.oriented;}
    public static int largeId() {return R.drawable.large;}
    public static int missingId() {return 0x7f08ffff;}
    public static void retain(int count) {
        retained=new Bitmap[count];
        for(int i=0;i<count;i++) retained[i]=BitmapFactory.decodeResource(resources,R.drawable.large);
    }
    public static void clearRetained() {retained=null;}
    public static void recycle() {bitmap.recycle();bitmap.recycle();}
    public static int recycledPixel() {return bitmap.getPixel(0,0);}
    public static int invalidPixel() {return bitmap.getPixel(-1,0);}
    public static void invalidDraw(Canvas canvas) {canvas.drawBitmap(bitmap,null,new RectF(Float.NaN,0,10,10),null);}
    public static void rects() {
        RectF f=new RectF(1,2,7,10);
        f.left=2;
        Rect i=new Rect(1,2,9,12);
        rectResult=(int)f.width()*1000+(int)f.height()*100+i.width()*10+i.height();
    }
    public static class BitmapView extends View {
        public BitmapView(Context context) {super(context);}
        @Override protected void onDraw(Canvas canvas) {
            super.onDraw(canvas);
            draws++;
            if(mode==1) {canvas.drawBitmap(bitmap,0,0,new Paint());return;}
            if(mode==2) {invalidDraw(canvas);return;}
            if(mode==3) {canvas.drawBitmap(bitmap,new Rect(-1,0,2,2),new Rect(0,0,8,8),null);return;}
            canvas.drawColor(0xff000000);
            canvas.drawBitmap(bitmap,null,new RectF(0,0,getWidth(),getHeight()),null);
            // Explicit source and integer destination; alpha is real palette tRNS.
            canvas.drawBitmap(alpha,new Rect(0,0,1,1),new Rect(0,0,8,8),null);
            // Position overload uses source density; nodpi remains unscaled.
            canvas.drawBitmap(bitmap,12,12,null);
        }
    }
}
