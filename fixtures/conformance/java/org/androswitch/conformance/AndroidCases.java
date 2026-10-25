package org.androswitch.conformance;
import android.app.Activity;
public final class AndroidCases {
  public static void check(boolean value, String detail) {
    if (!value)
      throw new IllegalStateException(detail);
  }
  public static String run(int which, Activity a) throws Exception {
    switch (which) {
      case 0: {
        int n = Runtime.getRuntime().availableProcessors();
        check(n > 0, "positive processor count");
        return "allowedProcessors=" + n;
      }
      case 1: {
        Runtime r = Runtime.getRuntime();
        System.gc();
        check(r == Runtime.getRuntime(), "singleton survives GC");
        return "same Runtime";
      }
      case 2: {
        String v = System.getProperty("androswitch.host.cpu.allowed_mask");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 3: {
        String v = System.getProperty("androswitch.host.cpu.clock_hz");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 4: {
        String v = System.getProperty("androswitch.host.gpu.clock_hz");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 5: {
        String v = System.getProperty("androswitch.host.memory.clock_hz");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 6: {
        String v = System.getProperty("androswitch.host.tick.frequency_hz");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 7: {
        String v = System.getProperty("androswitch.host.os.name");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 8: {
        String v = System.getProperty("androswitch.host.os.version");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 9: {
        String v = System.getProperty("androswitch.host.renderer.backend");
        if (v == null)
          return "UNSUPPORTED:host fact unavailable on this backend";
        return v;
      }
      case 10: {
        String v = android.os.Build.MODEL;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 11: {
        String v = android.os.Build.MANUFACTURER;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 12: {
        String v = android.os.Build.BRAND;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 13: {
        String v = android.os.Build.DEVICE;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 14: {
        String v = android.os.Build.PRODUCT;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 15: {
        String v = android.os.Build.BOARD;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 16: {
        String v = android.os.Build.HARDWARE;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 17: {
        String v = android.os.Build.ID;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 18: {
        String v = android.os.Build.DISPLAY;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 19: {
        String v = android.os.Build.FINGERPRINT;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 20: {
        String v = android.os.Build.TYPE;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 21: {
        String v = android.os.Build.TAGS;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 22: {
        String v = android.os.Build.USER;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 23: {
        String v = android.os.Build.HOST;
        check(v != null && !v.isEmpty(), "nonempty guest profile field");
        return v;
      }
      case 24: {
        return "buildTime=" + android.os.Build.TIME;
      }
      case 25: {
        String[] v = android.os.Build.SUPPORTED_ABIS;
        check(v != null && v.length > 0, "ABI list");
        return v[0] + " count=" + v.length;
      }
      case 26: {
        String v = android.os.Build.VERSION.RELEASE;
        check(v != null, "version field");
        return v;
      }
      case 27: {
        String v = android.os.Build.VERSION.INCREMENTAL;
        check(v != null, "version field");
        return v;
      }
      case 28: {
        String v = android.os.Build.VERSION.CODENAME;
        check(v != null, "version field");
        return v;
      }
      case 29: {
        String v = android.os.Build.VERSION.BASE_OS;
        check(v != null, "version field");
        return v;
      }
      case 30: {
        String v = android.os.Build.VERSION.SECURITY_PATCH;
        check(v != null, "version field");
        return v;
      }
      case 31: {
        check(android.os.Build.VERSION.SDK_INT >= 21, "API floor");
        return "guestAPI=" + android.os.Build.VERSION.SDK_INT;
      }
      case 32: {
        return "preview=" + android.os.Build.VERSION.PREVIEW_SDK_INT;
      }
      case 33: {
        long n = System.currentTimeMillis();
        check(n > 0, "Unix epoch clock");
        return Long.toString(n);
      }
      case 34: {
        long x = System.nanoTime(), y = System.nanoTime();
        check(y - x >= 0, "monotonic nano clock");
        return Long.toString(y - x);
      }
      case 35: {
        long x = android.os.SystemClock.elapsedRealtime(),
             y = android.os.SystemClock.elapsedRealtime();
        check(x >= 0 && y >= x, "elapsed clock");
        return Long.toString(y);
      }
      case 36: {
        long x = android.os.SystemClock.uptimeMillis();
        check(x >= 0, "uptime");
        return Long.toString(x);
      }
      case 37: {
        android.util.DisplayMetrics m = a.getResources().getDisplayMetrics();
        check(m.widthPixels > 0 && m.heightPixels > 0 && m.density > 0, "real display dimensions");
        return m.widthPixels + "x" + m.heightPixels + " density=" + m.density;
      }
      case 38: {
        android.content.res.Configuration c = a.getResources().getConfiguration();
        return "orientation=" + c.orientation + " density=" + c.densityDpi
            + " dp=" + c.screenWidthDp + "x" + c.screenHeightDp;
      }
      case 39: {
        return "rotation=" + a.getWindowManager().getDefaultDisplay().getRotation();
      }
      case 40: {
        return "requested=" + a.getRequestedOrientation();
      }
      case 41: {
        a.setRequestedOrientation(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE);
        check(a.getRequestedOrientation() == 0, "orientation request state");
        return "landscape requested; physical rotation is separate";
      }
      case 42: {
        a.setRequestedOrientation(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT);
        check(a.getRequestedOrientation() == 1, "orientation request state");
        return "portrait requested; physical rotation is separate";
      }
      case 43: {
        check(
            a.getWindow() != null && a.getWindow().getDecorView() != null, "window/decor identity");
        return "window and decor";
      }
      case 44: {
        a.getWindow().addFlags(128);
        a.getWindow().clearFlags(128);
        return "read/write window flags";
      }
      case 45: {
        check(a.getPackageName() != null, "package identity");
        return a.getPackageName();
      }
      case 46: {
        android.content.pm.PackageInfo p =
            a.getPackageManager().getPackageInfo(a.getPackageName(), 0);
        check(p.packageName.equals(a.getPackageName()), "manifest package");
        return p.versionName + "/" + p.versionCode;
      }
      case 47: {
        java.io.FileInputStream in = new java.io.FileInputStream(a.getApplicationInfo().sourceDir);
        int x = in.read(), y = in.read();
        in.close();
        check(x == 80 && y == 75, "real copied APK ZIP header");
        return "PK";
      }
      case 48: {
        try {
          a.getPackageManager().getPackageInfo("org.androswitch.nonexistent.fixture", 0);
          throw new IllegalStateException("fabricated package");
        } catch (android.content.pm.PackageManager.NameNotFoundException ok) {
          return "NameNotFoundException";
        }
      }
      case 49: {
        java.io.InputStream in = a.getAssets().open("probe.txt");
        check(in.read() == 65 && in.read() == 83, "actual asset bytes");
        in.close();
        return "AS asset";
      }
      case 50: {
        java.io.File f = new java.io.File(a.getFilesDir(), "io.bin");
        java.io.FileOutputStream o = new java.io.FileOutputStream(f);
        o.write(new byte[] {3, 7, 11});
        o.close();
        java.io.FileInputStream i = new java.io.FileInputStream(f);
        check(i.read() == 3 && i.read() == 7 && i.read() == 11 && i.read() == -1,
            "roundtrip and EOF");
        i.close();
        return "3,7,11,EOF";
      }
      case 51: {
        java.io.File f = new java.io.File(a.getFilesDir(), "append.bin");
        java.io.FileOutputStream o = new java.io.FileOutputStream(f);
        o.write(1);
        o.close();
        o = new java.io.FileOutputStream(f, true);
        o.write(2);
        o.close();
        java.io.FileInputStream i = new java.io.FileInputStream(f);
        check(i.read() == 1 && i.read() == 2, "append preserves bytes");
        i.close();
        return "1,2";
      }
      case 52: {
        java.io.File f = new java.io.File(a.getFilesDir(), "child");
        check(f.getName().equals("child"), "basename");
        return f.getParent();
      }
      case 53: {
        java.io.File d = new java.io.File(a.getFilesDir(), "nested/a");
        check(d.isDirectory() || d.mkdirs(), "recursive directory");
        return d.getAbsolutePath();
      }
      case 54: {
        java.io.File f = new java.io.File(a.getFilesDir(), "remove.bin");
        new java.io.FileOutputStream(f).close();
        check(f.delete() && !f.exists(), "delete actual file");
        return "deleted";
      }
      case 55: {
        java.io.File f = new java.io.File(a.getFilesDir(), "seek.bin");
        java.io.RandomAccessFile r = new java.io.RandomAccessFile(f, "rw");
        r.writeInt(0x12345678);
        r.seek(0);
        check(r.readInt() == 0x12345678, "seek and read");
        r.close();
        return "seek/read";
      }
      case 56: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-int", 0);
        check(p.edit().putInt("v", 47).commit(), "persistent commit");
        check(p.getInt("v", 0) == 47, "read committed value");
        return "committed";
      }
      case 57: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-long", 0);
        check(p.edit().putLong("v", 1234567890123L).commit(), "persistent commit");
        check(p.getLong("v", 0) == 1234567890123L, "read committed value");
        return "committed";
      }
      case 58: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-boolean", 0);
        check(p.edit().putBoolean("v", true).commit(), "persistent commit");
        check(p.getBoolean("v", false) == true, "read committed value");
        return "committed";
      }
      case 59: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-float", 0);
        check(p.edit().putFloat("v", 1.25f).commit(), "persistent commit");
        check(p.getFloat("v", 0) == 1.25f, "read committed value");
        return "committed";
      }
      case 60: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-string", 0);
        p.edit().putString("v", "A<&\"Z").commit();
        check(p.getString("v", null).equals("A<&\"Z"), "XML escaping");
        return "escaped string";
      }
      case 61: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-remove", 0);
        p.edit().putInt("v", 1).commit();
        p.edit().remove("v").commit();
        check(!p.contains("v"), "removed key");
        return "removed";
      }
      case 62: {
        android.content.SharedPreferences p = a.getSharedPreferences("suite-clear", 0);
        p.edit().putInt("v", 1).commit();
        p.edit().clear().commit();
        check(!p.contains("v"), "cleared key");
        return "cleared";
      }
      case 63: {
        android.os.Bundle b = new android.os.Bundle();
        b.putInt("k", 47);
        check(b.getInt("k") == 47, "typed roundtrip");
        return "int";
      }
      case 64: {
        android.os.Bundle b = new android.os.Bundle();
        b.putLong("k", 1234567890123L);
        check(b.getLong("k") == 1234567890123L, "typed roundtrip");
        return "long";
      }
      case 65: {
        android.os.Bundle b = new android.os.Bundle();
        b.putBoolean("k", true);
        check(b.getBoolean("k") == true, "typed roundtrip");
        return "boolean";
      }
      case 66: {
        android.os.Bundle b = new android.os.Bundle();
        b.putFloat("k", 1.25f);
        check(b.getFloat("k") == 1.25f, "typed roundtrip");
        return "float";
      }
      case 67: {
        android.os.Bundle b = new android.os.Bundle();
        b.putDouble("k", 3.5);
        check(b.getDouble("k") == 3.5, "typed roundtrip");
        return "double";
      }
      case 68: {
        android.os.Bundle b = new android.os.Bundle();
        b.putByte("k", (byte) -7);
        check(b.getByte("k") == (byte) -7, "typed roundtrip");
        return "byte";
      }
      case 69: {
        android.os.Bundle b = new android.os.Bundle();
        b.putShort("k", (short) 300);
        check(b.getShort("k") == (short) 300, "typed roundtrip");
        return "short";
      }
      case 70: {
        android.os.Bundle b = new android.os.Bundle();
        b.putChar("k", 'x');
        check(b.getChar("k") == 'x', "typed roundtrip");
        return "char";
      }
      case 71: {
        android.os.Bundle b = new android.os.Bundle();
        b.putString("k", "value");
        check(b.getString("k").equals("value"), "string");
        return "value";
      }
      case 72: {
        android.os.Bundle b = new android.os.Bundle();
        check(b.getInt("none", 91) == 91, "missing key default");
        return "91";
      }
      case 73: {
        android.os.Bundle b = new android.os.Bundle(), child = new android.os.Bundle();
        child.putInt("v", 7);
        b.putBundle("child", child);
        System.gc();
        check(b.getBundle("child").getInt("v") == 7, "nested GC reference");
        return "7";
      }
      case 74: {
        android.view.View v = new android.view.View(a);
        check(v.getContext() == a, "actual constructor Context");
        System.gc();
        check(v.getContext() == a, "Context reference survives GC");
        return "actual Context";
      }
      case 75: {
        android.widget.TextView v = new android.widget.TextView(a);
        v.setText("test");
        check(v.getText().toString().equals("test"), "text state");
        return "test";
      }
      case 76: {
        android.widget.Button b = new android.widget.Button(a);
        b.setId(401);
        b.setText("button");
        check(b.getId() == 401 && b.getText().toString().equals("button"), "button identity/text");
        return "401/button";
      }
      case 77: {
        android.view.inputmethod.BaseInputConnection c =
            new android.view.inputmethod.BaseInputConnection(new android.view.View(a), true);
        c.commitText("abcd", 1);
        c.setSelection(1, 3);
        c.commitText("X", 1);
        check(c.getEditable().toString().equals("aXd"), "actual Editable replace");
        return "aXd";
      }
      case 78: {
        android.text.SpannableStringBuilder b =
            new android.text.SpannableStringBuilder("A\uD83D\uDE00Z");
        check(b.length() == 4, "UTF16 units");
        b.delete(1, 3);
        check(b.toString().equals("AZ"), "UTF16 range");
        return "AZ";
      }
      case 79: {
        android.database.sqlite.SQLiteDatabase d =
            a.openOrCreateDatabase("suite-insert.db", 0, null);
        d.execSQL("DROP TABLE IF EXISTS t");
        d.execSQL("CREATE TABLE t(v INTEGER)");
        d.execSQL("INSERT INTO t(v) VALUES (7)");
        android.database.Cursor c = d.rawQuery("SELECT COUNT(*) FROM t", null);
        check(c.moveToFirst() && c.getInt(0) == 1, "SQLite transaction state");
        c.close();
        d.close();
        return "rows=1";
      }
      case 80: {
        android.database.sqlite.SQLiteDatabase d =
            a.openOrCreateDatabase("suite-rollback.db", 0, null);
        d.execSQL("DROP TABLE IF EXISTS t");
        d.execSQL("CREATE TABLE t(v INTEGER)");
        d.beginTransaction();
        d.execSQL("INSERT INTO t(v) VALUES (9)");
        d.endTransaction();
        android.database.Cursor c = d.rawQuery("SELECT COUNT(*) FROM t", null);
        check(c.moveToFirst() && c.getInt(0) == 0, "SQLite transaction state");
        c.close();
        d.close();
        return "rows=0";
      }
      case 81: {
        android.database.sqlite.SQLiteDatabase d =
            a.openOrCreateDatabase("suite-commit.db", 0, null);
        d.execSQL("DROP TABLE IF EXISTS t");
        d.execSQL("CREATE TABLE t(v INTEGER)");
        d.beginTransaction();
        d.execSQL("INSERT INTO t(v) VALUES (9)");
        d.setTransactionSuccessful();
        d.endTransaction();
        android.database.Cursor c = d.rawQuery("SELECT COUNT(*) FROM t", null);
        check(c.moveToFirst() && c.getInt(0) == 1, "SQLite transaction state");
        c.close();
        d.close();
        return "rows=1";
      }
      case 82: {
        android.database.sqlite.SQLiteDatabase d = a.openOrCreateDatabase("suite-bind.db", 0, null);
        d.execSQL("CREATE TABLE IF NOT EXISTS t(v TEXT)");
        android.database.sqlite.SQLiteStatement s =
            d.compileStatement("INSERT INTO t(v) VALUES (?)");
        s.bindString(1, "quote'value");
        s.executeInsert();
        s.close();
        d.close();
        return "bound string";
      }
      case 83: {
        int n = android.media.AudioTrack.getMinBufferSize(48000, 12, 2);
        check(n > 0, "supported PCM format");
        return "minimum=" + n;
      }
      case 84: {
        int n = android.media.AudioTrack.getMinBufferSize(48000, 12, 2);
        android.media.AudioTrack t = new android.media.AudioTrack(3, 48000, 12, 2, n, 1);
        check(t.getState() == 1, "initialized PCM");
        t.play();
        int written = t.write(new byte[1920], 0, 1920);
        t.stop();
        t.release();
        check(written > 0, "PCM submitted");
        return "bytes=" + written;
      }
      case 85: {
        throw new IllegalStateException("runner must schedule lifecycle roundtrip");
      }
      case 86: {
        check(a.getApplication() != null && a.getApplicationContext() != null,
            "attached application");
        return a.getApplication().getClass().getName();
      }
      case 87: {
        check(a.getIntent() != null, "initial Intent");
        return "attached Intent";
      }
      case 88: {
        return "NOT_RUN:asynchronous Handler verification is scheduled separately";
      }
      default:
        throw new IllegalArgumentException("unknown Android case");
    }
  }
}
