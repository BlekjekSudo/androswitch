package org.androswitch.contracts;
import android.app.Activity;
import android.app.Service;
import android.os.Bundle;
import android.os.IBinder;
import android.content.*;
import android.content.pm.ResolveInfo;
import android.widget.TextView;
import java.util.List;
/** Independent API26 offline receiver/service contracts, reused under two packages. */
public final class CommunicationActivity extends Activity {
    static CommunicationActivity activity;
    static BroadcastReceiver receiver;
    public static int count, connections;
    public static String events="";
    public static final class LocalService extends Service {
        public IBinder onBind(Intent intent) { return null; }
    }
    public static final class Connection implements ServiceConnection {
        public void onServiceConnected(ComponentName name,IBinder binder) { connections++; }
        public void onServiceDisconnected(ComponentName name) { connections++; }
    }
    public static class Defaults {public int integer;public Object reference;}
    public static final class ChildDefaults extends Defaults {}
    public static final class ServiceShape extends android.content.pm.ServiceInfo {}
    public static int defaults() {ChildDefaults d=new ChildDefaults();return(d.integer==0?1:0)+(d.reference==null?2:0);}
    public static Object unboundApplication(android.content.pm.ServiceInfo info) {return info.applicationInfo;}
    public static int unboundInherited(ServiceShape info) {return info.flags;}
    public static int initializedZero(android.content.pm.ServiceInfo info) {return info.flags;}
    public static final class Receiver extends BroadcastReceiver {
        public void onReceive(Context context,Intent intent) {
            if(context!=activity || isOrderedBroadcast() || isInitialStickyBroadcast())
                throw new IllegalStateException("invalid broadcast dispatch context");
            System.gc();
            String value=intent.getStringExtra("value");
            events=events+value+":";
            count++;
            if("reenter".equals(value))send("nested",true);
        }
    }
    public void onCreate(Bundle state) {
        super.onCreate(state); activity=this;
        TextView view=new TextView(this);view.setText("Offline communication contracts");setContentView(view);
        setup();
    }
    public static void setup() {
        receiver=new Receiver();
        IntentFilter filter=new IntentFilter("contract.PING");filter.addAction("contract.MANIFEST");filter.addCategory("contract.CATEGORY");
        activity.registerReceiver(receiver,filter);
        activity.registerReceiver(receiver,new IntentFilter(filter),null,null);
    }
    public static void send(String value,boolean categoryMatches) {
        Intent intent=new Intent("contract.PING").setPackage(activity.getPackageName()).addCategory(categoryMatches?"contract.CATEGORY":"contract.OTHER");
        intent.putExtra("value",value);
        activity.sendBroadcast(intent,null);
        intent.putExtra("value","mutated-after-send");
    }
    public static Intent rejectedCandidate(int mode) {
        Intent intent=new Intent(mode==1?"contract.MANIFEST":"contract.PING").putExtra("value","original");
        if(mode==1)intent.setPackage(activity.getPackageName());
        if(mode==2)intent.setPackage("net.absent.external.receiver");
        return intent;
    }
    public static void dispatch(Intent intent) {activity.sendBroadcast(intent);}
    public static void remove() { activity.unregisterReceiver(receiver);receiver=null; }
    public static int removeAgain() {
        try { activity.unregisterReceiver(new Receiver());return 0; }
        catch(IllegalArgumentException expected) { return 1; }
    }
    public static boolean stickyQuery() {return activity.registerReceiver(null,new IntentFilter("contract.ABSENT"))==null;}
    public static String discover(String packageName) {
        Intent intent=new Intent("contract.SERVICE").setPackage(packageName);
        List<ResolveInfo> services=activity.getPackageManager().queryIntentServices(intent,0);
        if(services.isEmpty())return "absent";
        ResolveInfo first=services.get(0);
        return services.size()+":"+first.serviceInfo.packageName+":"+first.serviceInfo.name;
    }
    public static boolean managerIdentity() {return activity.getPackageManager()==activity.getPackageManager();}
    public static String serviceProcess() {
        ResolveInfo info=activity.getPackageManager().queryIntentServices(new Intent("contract.SERVICE").setPackage(activity.getPackageName()),0).get(0);
        return info.serviceInfo.processName;
    }
    public static int bindAbsent(String packageName) {
        Connection connection=new Connection();
        if(activity.bindService(new Intent("contract.ABSENT").setPackage(packageName),connection,Context.BIND_AUTO_CREATE))return -1;
        activity.unbindService(connection);
        try {activity.unbindService(connection);return -2;}
        catch(IllegalArgumentException expected) {return connections;}
    }
    public static int logPolicy() {
        return (android.util.Log.isLoggable("arbitrary",android.util.Log.DEBUG)?1:0)
            +(android.util.Log.isLoggable("renamed",android.util.Log.INFO)?2:0);
    }
}
