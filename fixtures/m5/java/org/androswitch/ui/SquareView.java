package org.androswitch.ui;
import android.content.Context;
import android.graphics.*;
import android.view.*;
public final class SquareView extends View {
    private final Paint paint=new Paint();
    private int color=0xff2ad6a1;
    public SquareView(Context context){super(context);}
    @Override protected void onDraw(Canvas canvas){super.onDraw(canvas);canvas.drawColor(0xff182532);paint.setColor(color);canvas.drawRect(24,20,124,100,paint);paint.setColor(0xfff2cc70);canvas.drawCircle(180,60,35,paint);paint.setColor(0xffe9f1fa);paint.setTextSize(18);canvas.drawText("Canvas / touch the square",24,138,paint);}
    @Override public boolean onTouchEvent(MotionEvent event){if(event.getAction()==MotionEvent.ACTION_UP){color=0xffe47664;invalidate();}return true;}
}
