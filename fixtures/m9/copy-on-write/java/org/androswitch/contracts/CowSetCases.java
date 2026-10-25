package org.androswitch.contracts;
import java.util.*;
import java.util.concurrent.*;

/** COW set exercises the same snapshot/mutation engine as the list corpus. */
public final class CowSetCases {
    public static class Key {
        final int id; int mode, calls;
        Key(int id) { this.id = id; }
        Key(int id, int mode) { this.id = id; this.mode = mode; }
        public boolean equals(Object other) {
            calls++; int action=mode; mode=0;
            if(action==1) set.add(new Key(99));
            if(action==2) { set.clear(); System.gc(); }
            if(action==3) throw new IllegalStateException("set-equals");
            if(action==4) System.gc();
            return other instanceof Key && ((Key)other).id==id;
        }
        public int hashCode() { return id; }
    }
    public static final class Derived extends CopyOnWriteArraySet<Object> {
        int adds;
        public boolean add(Object value) { adds++; return super.add(value); }
    }
    static CopyOnWriteArraySet<Object> set;
    static Iterator<Object> saved;
    static int id(Object value) { return value==null ? -1 : ((Key)value).id; }
    static String contents(Iterator<Object> it) {
        StringBuilder out=new StringBuilder();
        while(it.hasNext()) out.append(id(it.next())).append(',');
        return out.toString();
    }
    static CopyOnWriteArrayList<Object> input(Object[] values) {
        return new CopyOnWriteArrayList<Object>(values);
    }
    static void setup() {
        set=new CopyOnWriteArraySet<Object>();
        set.add(new Key(1));set.add(new Key(2));
    }
    public static String scenario(int mode) {
        setup();
        if(mode==0) { set.clear(); return set.size()+":"+(set.isEmpty()?1:0); }
        if(mode==1) { boolean a=set.add(new Key(3)),b=set.add(new Key(3)); return (a?1:0)+":"+(b?1:0)+":"+contents(set.iterator()); }
        if(mode==2) { boolean a=set.add(null),b=set.add(null),c=set.contains(null),d=set.remove(null); return (a?1:0)+":"+(b?1:0)+":"+(c?1:0)+":"+(d?1:0)+":"+set.size(); }
        if(mode==3) { set=new CopyOnWriteArraySet<Object>(input(new Object[]{new Key(1),null,new Key(1),new Key(2),null})); return contents(set.iterator()); }
        if(mode==4) { Iterator<Object> it=set.iterator();set.remove(new Key(1));set.add(new Key(3)); return contents(it)+"/"+contents(set.iterator()); }
        if(mode==5) { int n=0;Iterator<Object> it=set.iterator();try{it.remove();}catch(UnsupportedOperationException e){n++;}it.next();it.next();try{it.next();}catch(NoSuchElementException e){n++;}return n+":"+set.size(); }
        if(mode==6) { Key[] target=new Key[]{new Key(7),new Key(8),new Key(9),new Key(10)};Object[] result=set.toArray(target);return (result==target?1:0)+":"+id(target[0])+":"+id(target[1])+":"+(target[2]==null?1:0)+":"+id(target[3]); }
        if(mode==7) { Key[] result=set.toArray(new Key[0]);return result.length+":"+id(result[0])+":"+id(result[1]); }
        if(mode==8) { int n=0;try{set.toArray(new String[3]);}catch(ArrayStoreException e){n++;}return ""+n; }
        if(mode==9) { int n=0;try{new CopyOnWriteArraySet<Object>((Collection<?>)null);}catch(NullPointerException e){n++;}return ""+n; }
        if(mode==10) { Derived d=new Derived();d.add(new Key(7));d.add(new Key(7));return d.adds+":"+d.size()+":"+(d instanceof AbstractSet?1:0); }
        if(mode==11) { Key query=new Key(5,1);boolean added=set.add(query);return (added?1:0)+":"+contents(set.iterator()); }
        if(mode==12) { Key query=new Key(2,2);boolean removed=set.remove(query);return (removed?1:0)+":"+set.size()+":"+query.calls; }
        if(mode==13) { Key query=new Key(2,2);boolean found=set.contains(query);return (found?1:0)+":"+set.size()+":"+query.calls; }
        if(mode==14) { int n=0;try{set.remove(new Key(2,3));}catch(IllegalStateException e){n++;}set.add(new Key(4));return n+":"+contents(set.iterator()); }
        if(mode==15) { Key first=new Key(4);set=new CopyOnWriteArraySet<Object>(input(new Object[]{first,new Key(4,4),null,new Key(5,4)}));return contents(set.iterator())+":"+(set.toArray()[0]==first?1:0); }
        if(mode==16) { CopyOnWriteArrayList<Object> source=input(new Object[]{new Key(6),new Key(6),new Key(7)});set=new CopyOnWriteArraySet<Object>(source);source.clear();return source.size()+":"+contents(set.iterator()); }
        if(mode==17) { saved=set.iterator();set=null;System.gc();return contents(saved); }
        if(mode==18) { Iterator<Object> it=set.iterator();set.clear();System.gc();return contents(it)+":"+set.size(); }
        if(mode==19) { int n=0;try{set.toArray((Object[])null);}catch(NullPointerException e){n++;}return n+":"+set.size(); }
        throw new IllegalArgumentException("set scenario");
    }
    public static Object setObject() { return set; }
    public static void main(String[] args) {
        for(int i=0;i<20;i++) System.out.println("scenario"+i+"\t"+scenario(i));
    }
}
