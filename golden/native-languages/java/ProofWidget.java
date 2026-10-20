package org.rustydroid.nativeworkbench;
import android.appwidget.*;import android.content.*;import android.widget.RemoteViews;
public final class ProofWidget extends AppWidgetProvider {
 @Override public void onUpdate(Context context,AppWidgetManager manager,int[] ids) {
  for(int id:ids) {RemoteViews views=new RemoteViews(context.getPackageName(),R.layout.widget);views.setTextViewText(R.id.widget_text,MainActivity.LANGUAGE+" native widget");manager.updateAppWidget(id,views);}
 }
}
