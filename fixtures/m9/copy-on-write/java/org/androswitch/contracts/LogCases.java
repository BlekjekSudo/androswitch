package org.androswitch.contracts;
import android.util.Log;
/** Android Log is unavailable in a desktop JVM; validate the actual file sink. */
public final class LogCases {
    static class Problem extends RuntimeException {
        public String toString() { System.gc();return "callback exception summary"; }
    }
    public static int emit() {
        int total=0;
        total+=Log.v("probe","verbose");
        total+=Log.d("probe","debug",null);
        total+=Log.i("probe","info");
        total+=Log.w("probe","warning",new Problem());
        total+=Log.w("probe",new Problem());
        total+=Log.e("probe","error",new Problem());
        total+=Log.println(Log.WARN,"probe","raw");
        return total;
    }
}
