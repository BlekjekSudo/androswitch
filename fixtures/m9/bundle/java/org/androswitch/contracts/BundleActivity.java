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

  public static void putBooleanArray(String key,boolean[] value) {values.putBooleanArray(key,value);}
  public static boolean[] getBooleanArray(String key) {return values.getBooleanArray(key);}
  public static void extraBooleanArray(String key,boolean[] value) {intent.putExtra(key,value);}
  public static boolean[] readExtraBooleanArray(String key) {return intent.getBooleanArrayExtra(key);}
  public static void putByteArray(String key,byte[] value) {values.putByteArray(key,value);}
  public static byte[] getByteArray(String key) {return values.getByteArray(key);}
  public static void extraByteArray(String key,byte[] value) {intent.putExtra(key,value);}
  public static byte[] readExtraByteArray(String key) {return intent.getByteArrayExtra(key);}
  public static void putCharArray(String key,char[] value) {values.putCharArray(key,value);}
  public static char[] getCharArray(String key) {return values.getCharArray(key);}
  public static void extraCharArray(String key,char[] value) {intent.putExtra(key,value);}
  public static char[] readExtraCharArray(String key) {return intent.getCharArrayExtra(key);}
  public static void putShortArray(String key,short[] value) {values.putShortArray(key,value);}
  public static short[] getShortArray(String key) {return values.getShortArray(key);}
  public static void extraShortArray(String key,short[] value) {intent.putExtra(key,value);}
  public static short[] readExtraShortArray(String key) {return intent.getShortArrayExtra(key);}
  public static void putIntArray(String key,int[] value) {values.putIntArray(key,value);}
  public static int[] getIntArray(String key) {return values.getIntArray(key);}
  public static void extraIntArray(String key,int[] value) {intent.putExtra(key,value);}
  public static int[] readExtraIntArray(String key) {return intent.getIntArrayExtra(key);}
  public static void putLongArray(String key,long[] value) {values.putLongArray(key,value);}
  public static long[] getLongArray(String key) {return values.getLongArray(key);}
  public static void extraLongArray(String key,long[] value) {intent.putExtra(key,value);}
  public static long[] readExtraLongArray(String key) {return intent.getLongArrayExtra(key);}
  public static void putFloatArray(String key,float[] value) {values.putFloatArray(key,value);}
  public static float[] getFloatArray(String key) {return values.getFloatArray(key);}
  public static void extraFloatArray(String key,float[] value) {intent.putExtra(key,value);}
  public static float[] readExtraFloatArray(String key) {return intent.getFloatArrayExtra(key);}
  public static void putDoubleArray(String key,double[] value) {values.putDoubleArray(key,value);}
  public static double[] getDoubleArray(String key) {return values.getDoubleArray(key);}
  public static void extraDoubleArray(String key,double[] value) {intent.putExtra(key,value);}
  public static double[] readExtraDoubleArray(String key) {return intent.getDoubleArrayExtra(key);}
  public static void putStringArray(String key,String[] value) {values.putStringArray(key,value);}
  public static String[] getStringArray(String key) {return values.getStringArray(key);}
  public static void extraStringArray(String key,String[] value) {intent.putExtra(key,value);}
  public static String[] readExtraStringArray(String key) {return intent.getStringArrayExtra(key);}
  public static int arrayProof() {
    boolean[] a0=new boolean[]{true,false};
    values.putBooleanArray("Boolean",a0);intent.putExtra("Boolean",a0);
    System.gc();
    if(values.getBooleanArray("Boolean")!=a0 || intent.getBooleanArrayExtra("Boolean")!=a0)throw new IllegalStateException("array identity Boolean");
    byte[] a1=new byte[]{-113,99};
    values.putByteArray("Byte",a1);intent.putExtra("Byte",a1);
    System.gc();
    if(values.getByteArray("Byte")!=a1 || intent.getByteArrayExtra("Byte")!=a1)throw new IllegalStateException("array identity Byte");
    char[] a2=new char[]{'\u263a','Z'};
    values.putCharArray("Char",a2);intent.putExtra("Char",a2);
    System.gc();
    if(values.getCharArray("Char")!=a2 || intent.getCharArrayExtra("Char")!=a2)throw new IllegalStateException("array identity Char");
    short[] a3=new short[]{-23456,12345};
    values.putShortArray("Short",a3);intent.putExtra("Short",a3);
    System.gc();
    if(values.getShortArray("Short")!=a3 || intent.getShortArrayExtra("Short")!=a3)throw new IllegalStateException("array identity Short");
    int[] a4=new int[]{17,-19};
    values.putIntArray("Int",a4);intent.putExtra("Int",a4);
    System.gc();
    if(values.getIntArray("Int")!=a4 || intent.getIntArrayExtra("Int")!=a4)throw new IllegalStateException("array identity Int");
    long[] a5=new long[]{Long.MIN_VALUE,Long.MAX_VALUE};
    values.putLongArray("Long",a5);intent.putExtra("Long",a5);
    System.gc();
    if(values.getLongArray("Long")!=a5 || intent.getLongArrayExtra("Long")!=a5)throw new IllegalStateException("array identity Long");
    float[] a6=new float[]{1.25f,-3.5f};
    values.putFloatArray("Float",a6);intent.putExtra("Float",a6);
    System.gc();
    if(values.getFloatArray("Float")!=a6 || intent.getFloatArrayExtra("Float")!=a6)throw new IllegalStateException("array identity Float");
    double[] a7=new double[]{Double.NaN,-0.0};
    values.putDoubleArray("Double",a7);intent.putExtra("Double",a7);
    System.gc();
    if(values.getDoubleArray("Double")!=a7 || intent.getDoubleArrayExtra("Double")!=a7)throw new IllegalStateException("array identity Double");
    String[] a8=new String[]{"alpha",null};
    values.putStringArray("String",a8);intent.putExtra("String",a8);
    System.gc();
    if(values.getStringArray("String")!=a8 || intent.getStringArrayExtra("String")!=a8)throw new IllegalStateException("array identity String");
    snapshot=intent.getExtras();a8[0]="changed";
    if(!"changed".equals(snapshot.getStringArray("String")[0]))throw new IllegalStateException("shallow array snapshot");
    values.putInt("wrong",42);intent.putExtra("wrong",42);
    if(values.getIntArray("wrong")!=null || intent.getStringArrayExtra("wrong")!=null || intent.getStringArrayExtra("absent")!=null)throw new IllegalStateException("array type defaults");
    values.putStringArray(null,new String[0]);if(values.getStringArray(null).length!=0)throw new IllegalStateException("null key empty array");
    values.putStringArray("nullable",null);if(values.getStringArray("nullable")!=null || !values.containsKey("nullable"))throw new IllegalStateException("explicit null array");
    return 9;
  }
  public static void releaseArrays(){values.clear();intent=new Intent();snapshot=null;}
}
