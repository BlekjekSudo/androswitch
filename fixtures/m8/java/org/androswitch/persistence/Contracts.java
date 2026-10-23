package org.androswitch.persistence;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.Iterator;

/** The same source runs on the JVM and in DEX; no Android-specific oracle. */
public final class Contracts {
    private static Iterator<?> detached() {
        ArrayList<String> values = new ArrayList<String>();
        values.add("survives");
        return values.iterator();
    }
    public static String collections() {
        if (Integer.valueOf(42) != Integer.valueOf(42) || Boolean.valueOf(true) != Boolean.TRUE) throw new IllegalStateException("wrapper cache");
        ArrayList<Object> values = new ArrayList<Object>();
        values.add(Integer.valueOf(1200));
        values.add("alpha");
        HashMap<Object,Object> map = new HashMap<Object,Object>();
        map.put(Integer.valueOf(1200), values);
        Iterator<?> iterator = detached();
        System.gc();
        ArrayList<?> found = (ArrayList<?>)map.get(Integer.valueOf(1200));
        Object[] copied = found.toArray();
        if (copied.length != 2 || !"alpha".equals(copied[1])) throw new IllegalStateException("array copy");
        int bounds = 0;
        try { found.get(-1); } catch (IndexOutOfBoundsException expected) { bounds = 1; }
        return ((Integer)found.get(0)).intValue() + ":" + found.size() + ":" + iterator.next() + ":" + bounds;
    }
    public static int twice(int value) { return value * 2 + 1; }
    public static int fails() { int zero = 0; return 1 / zero; }
    public static String reflection() throws Exception {
        Class<?> type = Class.forName("org.androswitch.persistence.Contracts");
        if (!"java.lang.Object".equals(type.getSuperclass().getName()) || Integer.TYPE.getSuperclass()!=null) throw new IllegalStateException("class hierarchy metadata");
        java.lang.reflect.Method method = type.getDeclaredMethod("twice", new Class<?>[] { Integer.TYPE });
        Object result = method.invoke(null, new Object[] { Integer.valueOf(9) });
        int catches = 0;
        try { type.getDeclaredMethod("absent", new Class<?>[0]); } catch (NoSuchMethodException expected) { catches++; }
        try { method.invoke(null, new Object[] { Long.valueOf(9) }); } catch (IllegalArgumentException expected) { catches++; }
        try { type.getDeclaredMethod("fails", new Class<?>[0]).invoke(null, new Object[0]); } catch (java.lang.reflect.InvocationTargetException expected) { catches++; }
        return type.getName() + ":" + ((Integer)result).intValue() + ":caught:" + catches;
    }
    public static void main(String[] args) throws Exception {
        System.out.println(collections());
        System.out.println(reflection());
    }
}
