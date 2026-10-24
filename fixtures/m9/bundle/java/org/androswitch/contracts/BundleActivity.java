package org.androswitch.contracts;
import android.app.Activity;
import android.os.Bundle;
import android.os.BaseBundle;
import android.content.Intent;
public class BundleActivity extends Activity {
  public static Bundle values;
  public static Bundle snapshot;
  public static Intent intent;
  public void onCreate(Bundle state) { super.onCreate(state); values = new Bundle(); intent = new Intent(); }  public static void putBoolean(String key, boolean value) { values.putBoolean(key,value); }
  public static boolean getBoolean(String key) { return values.getBoolean(key); }
  public static boolean defaultBoolean(String key, boolean fallback) { return values.getBoolean(key,fallback); }
  public static void extraBoolean(String key, boolean value) { intent.putExtra(key,value); }
  public static boolean readExtraBoolean(String key, boolean fallback) { return intent.getBooleanExtra(key,fallback); }
  public static void putByte(String key, byte value) { values.putByte(key,value); }
  public static byte getByte(String key) { return values.getByte(key); }
  public static byte defaultByte(String key, byte fallback) { return values.getByte(key,fallback); }
  public static void extraByte(String key, byte value) { intent.putExtra(key,value); }
  public static byte readExtraByte(String key, byte fallback) { return intent.getByteExtra(key,fallback); }
  public static void putChar(String key, char value) { values.putChar(key,value); }
  public static char getChar(String key) { return values.getChar(key); }
  public static char defaultChar(String key, char fallback) { return values.getChar(key,fallback); }
  public static void extraChar(String key, char value) { intent.putExtra(key,value); }
  public static char readExtraChar(String key, char fallback) { return intent.getCharExtra(key,fallback); }
  public static void putShort(String key, short value) { values.putShort(key,value); }
  public static short getShort(String key) { return values.getShort(key); }
  public static short defaultShort(String key, short fallback) { return values.getShort(key,fallback); }
  public static void extraShort(String key, short value) { intent.putExtra(key,value); }
  public static short readExtraShort(String key, short fallback) { return intent.getShortExtra(key,fallback); }
  public static void putInt(String key, int value) { values.putInt(key,value); }
  public static int getInt(String key) { return values.getInt(key); }
  public static int defaultInt(String key, int fallback) { return values.getInt(key,fallback); }
  public static void extraInt(String key, int value) { intent.putExtra(key,value); }
  public static int readExtraInt(String key, int fallback) { return intent.getIntExtra(key,fallback); }
  public static void putLong(String key, long value) { values.putLong(key,value); }
  public static long getLong(String key) { return values.getLong(key); }
  public static long defaultLong(String key, long fallback) { return values.getLong(key,fallback); }
  public static void extraLong(String key, long value) { intent.putExtra(key,value); }
  public static long readExtraLong(String key, long fallback) { return intent.getLongExtra(key,fallback); }
  public static void putFloat(String key, float value) { values.putFloat(key,value); }
  public static float getFloat(String key) { return values.getFloat(key); }
  public static float defaultFloat(String key, float fallback) { return values.getFloat(key,fallback); }
  public static void extraFloat(String key, float value) { intent.putExtra(key,value); }
  public static float readExtraFloat(String key, float fallback) { return intent.getFloatExtra(key,fallback); }
  public static void putDouble(String key, double value) { values.putDouble(key,value); }
  public static double getDouble(String key) { return values.getDouble(key); }
  public static double defaultDouble(String key, double fallback) { return values.getDouble(key,fallback); }
  public static void extraDouble(String key, double value) { intent.putExtra(key,value); }
  public static double readExtraDouble(String key, double fallback) { return intent.getDoubleExtra(key,fallback); }
  public static void putString(String key, String value) { values.putString(key,value); }
  public static String getString(String key) { return values.getString(key); }
  public static String defaultString(String key, String fallback) { return values.getString(key,fallback); }
  public static void extraString(String key, String value) { intent.putExtra(key,value); }
  public static String readExtraString(String key) { return intent.getStringExtra(key); }
  public static long baseLong(String key, long fallback) { BaseBundle base=values; return base.getLong(key,fallback); }
  public static void takeSnapshot() { snapshot=intent.getExtras(); }
  public static long snapshotLong(String key, long fallback) { return snapshot.getLong(key,fallback); }
  public static String snapshotString(String key) { return snapshot.getString(key); }
  public static int count() { return values.size(); }
  public static boolean empty() { return values.isEmpty(); }
  public static boolean contains(String key) { return values.containsKey(key); }
  public static void remove(String key) { values.remove(key); }
  public static void clear() { values.clear(); }
}