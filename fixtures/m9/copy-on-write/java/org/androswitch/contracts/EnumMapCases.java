package org.androswitch.contracts;
import java.util.*;

/** General enum-keyed Map contracts. No application or game identifiers. */
@SuppressWarnings({"unchecked", "rawtypes"})
public final class EnumMapCases {
    enum Key { Z, A { public String toString() { return "override"; } }, Q }
    enum Other { Z }
    static EnumMap<Key,Object> saved;
    static EnumMap<Key,Object> map() {
        EnumMap<Key,Object> m=new EnumMap<Key,Object>(Key.class);
        m.put(Key.Q,"q"); m.put(Key.Z,"z"); m.put(Key.A,null); return m;
    }
    static String contents(EnumMap<Key,Object> m) {
        return m.size()+":"+m.get(Key.Z)+":"+m.get(Key.A)+":"+m.get(Key.Q);
    }
    static final class ValueKey {
        public boolean equals(Object other) { System.gc(); return "z".equals(other); }
    }
    public static String scenario(int n) {
        EnumMap<Key,Object> m=map();
        if(n==0) return contents(m);
        if(n==1) return m.put(Key.Z,"new")+":"+contents(m);
        if(n==2) return m.containsKey(Key.A)+":"+m.containsValue(null)+":"+m.get(Key.A);
        if(n==3) return m.remove(Key.A)+":"+m.containsKey(Key.A)+":"+m.size();
        if(n==4) { m.clear();return m.size()+":"+m.isEmpty(); }
        if(n==5) return m.get(null)+":"+m.containsKey(null)+":"+m.remove(null);
        if(n==6) return m.get(Other.Z)+":"+m.containsKey(Other.Z)+":"+m.remove(Other.Z);
        if(n==7) return m.get("Z")+":"+m.containsKey("Z")+":"+m.remove("Z");
        if(n==8) { try{m.put(null,"bad");}catch(NullPointerException e){return "NPE:"+m.size();}return "bad"; }
        if(n==9) { try{((Map)m).put(Other.Z,"bad");}catch(ClassCastException e){return "CCE:"+m.size();}return "bad"; }
        if(n==10) { EnumMap<Key,Object> copy=new EnumMap<Key,Object>(m);m.clear();return contents(copy); }
        if(n==11) { m.clear();EnumMap<Key,Object> copy=new EnumMap<Key,Object>(m);copy.put(Key.A,"a");return contents(copy); }
        if(n==12) { HashMap<Key,Object> source=new HashMap<Key,Object>();source.put(Key.Q,"q");source.put(Key.Z,"z");return contents(new EnumMap<Key,Object>(source)); }
        if(n==13) { try{new EnumMap(new HashMap());}catch(IllegalArgumentException e){return "IAE";}return "bad"; }
        if(n==14) { try{new EnumMap((Class)String.class);}catch(NullPointerException e){return "NPE";}return "bad"; }
        if(n==15) { try{new EnumMap((Class)null);}catch(NullPointerException e){return "NPE";}return "bad"; }
        if(n==16) { m.putAll(m);return contents(m); }
        if(n==17) { EnumMap<Key,Object> copy=new EnumMap<Key,Object>(Key.class);copy.putAll(m);m.clear();return contents(copy); }
        if(n==18) { EnumMap<Other,Object> source=new EnumMap<Other,Object>(Other.class);m.putAll((Map)source);return contents(m); }
        if(n==19) { EnumMap<Other,Object> source=new EnumMap<Other,Object>(Other.class);source.put(Other.Z,"bad");try{m.putAll((Map)source);}catch(ClassCastException e){return "CCE:"+m.size();}return "bad"; }
        if(n==20) { saved=m;System.gc();return contents(saved); }
        if(n==21) return m.containsValue(new ValueKey())+":"+contents(m);
        if(n==22) return (m instanceof AbstractMap)+":"+(m instanceof Cloneable)+":"+(m instanceof java.io.Serializable);
        if(n==23) { m.put(Key.A,"a");return Key.A.getDeclaringClass().getName()+":"+contents(m); }
        if(n==24) { try{new EnumMap((Map)null);}catch(NullPointerException e){return "NPE";}return "bad"; }
        if(n==25) { try{m.putAll(null);}catch(NullPointerException e){return "NPE:"+m.size();}return "bad"; }
        throw new IllegalArgumentException("enum map scenario");
    }
    public static void main(String[] args) {
        for(int n=0;n<26;n++) System.out.println("scenario"+n+"\t"+scenario(n));
    }
}
