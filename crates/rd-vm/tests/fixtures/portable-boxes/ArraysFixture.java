package org.androswitch.vm;

import java.util.Arrays;
import java.util.Comparator;

public final class ArraysFixture {
    static Object[] exposed;
    static boolean drop;
    static boolean detach;
    static void collect() {
        if (drop) {
            if (detach) Arrays.fill(exposed, null);
            System.gc();
        }
    }
    static final class Item {
        final int n;
        Item(int n) { this.n = n; }
        public boolean equals(Object other) { collect(); return other instanceof Item && ((Item) other).n == n; }
        public int hashCode() { collect(); return n; }
        public String toString() { collect(); return "item"; }
    }
    static final class Order implements Comparator<Object> {
        public int compare(Object a, Object b) {
            collect();
            return ((Item) a).n - ((Item) b).n;
        }
    }
    public static int observe(int id) {
        drop = false;
        detach = false;
        switch (id) {
            case 0: { int[] a = {3, 1, 2}; Arrays.fill(a, 1, 3, 8); return Arrays.hashCode(a); }
            case 1: { byte[] a = {1, 2, 3}; Arrays.fill(a, (byte) -7); return Arrays.hashCode(a); }
            case 2: { short[] a = {1, 2}; Arrays.fill(a, (short) -300); return Arrays.hashCode(a); }
            case 3: { char[] a = {'a', 'b'}; Arrays.fill(a, '\ufffe'); return Arrays.hashCode(a); }
            case 4: { boolean[] a = {true, false}; Arrays.fill(a, true); return Arrays.hashCode(a); }
            case 5: { long[] a = {Long.MIN_VALUE, 3}; Arrays.fill(a, -9L); return Arrays.hashCode(a); }
            case 6: { float[] a = {Float.NaN, -0.0f, 1f}; return Arrays.hashCode(a); }
            case 7: { double[] a = {Double.NaN, -0.0, 1.0}; return Arrays.hashCode(a); }
            case 8: { String[] a = {"a", "b", "c"}; Arrays.fill(a, 0, 2, "d"); return Arrays.hashCode(a); }
            case 9: return Arrays.hashCode(Arrays.copyOf(new int[] {1, 2}, 4));
            case 10: return Arrays.hashCode(Arrays.copyOfRange(new int[] {1, 2, 3}, 1, 5));
            case 11: { int[] a = {5, 2, -7, 2}; Arrays.sort(a); return Arrays.hashCode(a); }
            case 12: { long[] a = {5, Long.MIN_VALUE, 7}; Arrays.sort(a); return Arrays.hashCode(a); }
            case 13: { float[] a = {Float.NaN, 0f, -0f, -3f}; Arrays.sort(a); return Arrays.hashCode(a); }
            case 14: { double[] a = {Double.NaN, 0, -0.0, -3}; Arrays.sort(a); return Arrays.hashCode(a); }
            case 15: { String[] a = {"z", "a", "b"}; Arrays.sort(a); return Arrays.hashCode(a); }
            case 16: return Arrays.binarySearch(new int[] {1, 3, 9}, 4);
            case 17: return Arrays.equals(new double[] {Double.NaN, -0.0}, new double[] {Double.NaN, -0.0}) ? 1 : 0;
            case 18: return Arrays.toString(new int[] {1, -2, 3}).hashCode();
            case 19: return Arrays.deepToString(new Object[] {new int[] {1, 2}, "ok", null}).hashCode();
            case 20: try { Arrays.fill(new int[2], 2, 1, 0); return 0; } catch (IllegalArgumentException e) { return 1; }
            case 21: try { Arrays.fill(new int[2], -1, 1, 0); return 0; } catch (ArrayIndexOutOfBoundsException e) { return 1; }
            case 22: try { Arrays.fill(new int[2], 0, 3, 0); return 0; } catch (ArrayIndexOutOfBoundsException e) { return 1; }
            case 23: try { Object[] a = new String[2]; Arrays.fill(a, new Item(1)); return 0; } catch (ArrayStoreException e) { return 1; }
            case 24: try { Arrays.copyOf(new Object[] {new Item(1)}, 1, String[].class); return 0; } catch (ArrayStoreException e) { return 1; }
            case 25: {
                exposed = new Object[] {new Item(3), new Item(1), new Item(2)};
                drop = true;
                Arrays.sort(exposed, new Order());
                drop = false;
                return ((Item) exposed[0]).n * 100 + ((Item) exposed[1]).n * 10 + ((Item) exposed[2]).n;
            }
            case 26: {
                exposed = new Object[] {new Item(3), new Item(1), new Item(2)};
                drop = true;
                return Arrays.hashCode(exposed);
            }
            case 27: {
                exposed = new Object[] {new Item(3), new Item(1), new Item(2)};
                drop = true;
                return Arrays.toString(exposed).hashCode();
            }
            case 28: return Arrays.equals(new Object[] {Float.valueOf(Float.NaN)}, new Object[] {Float.valueOf(Float.NaN)}) ? 1 : 0;
            case 29: return Arrays.equals(new Object[] {Float.valueOf(-0f)}, new Object[] {Float.valueOf(0f)}) ? 1 : 0;
            case 30: return Arrays.equals(new Object[] {Double.valueOf(Double.NaN)}, new Object[] {Double.valueOf(Double.NaN)}) ? 1 : 0;
            case 31: return Arrays.equals(new Object[] {Double.valueOf(-0.0)}, new Object[] {Double.valueOf(0.0)}) ? 1 : 0;
            default: throw new IllegalArgumentException();
        }
    }
    public static void main(String[] args) {
        for (int i = 0; i < 32; ++i) System.out.println(i + "\t" + observe(i));
    }
    // This preserves the inherited RustyDroid snapshot contract, separately
    // from the JVM corpus: mutating a sort input inside compare is unspecified.
    public static int hashDetached() {
        exposed = new Object[] {new Item(3), new Item(1), new Item(2)};
        drop = true;
        detach = true;
        return Arrays.hashCode(exposed);
    }
}
