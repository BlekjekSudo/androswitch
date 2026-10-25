package org.androswitch.contracts;
import android.util.Pair;
public final class PairCases {
    static final class Item {
        final int n;
        Item(int n) { this.n=n; }
        public int hashCode() { System.gc(); return n; }
        public boolean equals(Object other) { System.gc(); return other instanceof Item && ((Item)other).n==n; }
        public String toString() { System.gc(); return "item"; }
    }
    static Pair<Object,Object> saved;
    static final class OneWay {
        final Object match;
        OneWay(Object match) { this.match=match; }
        public boolean equals(Object other) { return other==match; }
    }
    public static int proof() {
        Object a=new Item(7), b=new Item(9);
        Pair<Object,Object> p=new Pair<Object,Object>(a,b);
        if(p.first!=a || p.second!=b) return 1;
        if(p.hashCode()!=(7^9)) return 2;
        Pair<Object,Object> equal=Pair.create(new Item(7),new Item(9));
        if(!p.equals(equal) || !equal.equals(p)) return 3;
        if(p.equals(null) || p.equals(a) || p.equals(Pair.create(a,null))) return 4;
        if(!Pair.create(null,null).equals(Pair.create(null,null))) return 5;
        if(!"Pair{item item}".equals(p.toString())) return 6;
        if(!"Pair{null null}".equals(Pair.create(null,null).toString())) return 7;
        Pair<String,String> strings=Pair.create("A\ud83d\ude00","test");
        if(strings.hashCode() != (1835364 ^ 3556498)) return 8;
        Pair<Object,Object> argument=Pair.create(new OneWay(a),null);
        Pair<Object,Object> receiver=Pair.create(a,null);
        if(!receiver.equals(argument) || argument.equals(receiver)) return 10;
        saved=p;
        System.gc();
        if(saved.first!=a || saved.second!=b) return 9;
        return 0;
    }
    public static Pair<Object,Object> get() { return saved; }
    public static void clear() { saved=null; }
    // Uses only JVM String; the host never executes android.jar's Pair stubs.
    public static void main(String[] ignored) {
        System.out.println("A\ud83d\ude00".hashCode());
        System.out.println("test".hashCode());
    }
}
