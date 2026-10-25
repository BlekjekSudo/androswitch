package org.androswitch.contracts;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
public final class ConcurrentMapCases {
    static ConcurrentHashMap<Object,Object> active;
    static final class Key {
        final int n;
        int action;
        Key(int n) { this.n=n; }
        public int hashCode() { return n; }
        public boolean equals(Object other) {
            int command=action; action=0;
            if(command==1) active.clear();
            if(command==2) active.put("side","retained");
            System.gc();
            if(command==3) throw new IllegalStateException("equals callback");
            return other instanceof Key && ((Key)other).n==n;
        }
    }
    static final class Child extends ConcurrentHashMap<Object,Object> {
        int value=71;
        Child() { super(3); }
    }
    public static int probe(int test) {
        ConcurrentHashMap<Object,Object> m=new ConcurrentHashMap<Object,Object>();
        Object a=new Key(1), b=new Key(2);
        switch(test) {
        case 0: return m.isEmpty() && m.size()==0 ? 1:0;
        case 1: return m.put("k",a)==null && m.get("k")==a ? 1:0;
        case 2: m.put("k",a); return m.put("k",b)==a && m.get("k")==b ? 1:0;
        case 3: return m.putIfAbsent("k",a)==null && m.putIfAbsent("k",b)==a && m.get("k")==a ? 1:0;
        case 4: return m.remove("missing")==null ? 1:0;
        case 5: m.put("k",a); return m.remove("k")==a && m.size()==0 ? 1:0;
        case 6: m.put("k","value"); return !m.remove("k","bad") && m.remove("k",new String("value")) && m.isEmpty() ? 1:0;
        case 7: m.put("k",a); return m.replace("k",b)==a && m.get("k")==b && m.replace("absent",a)==null ? 1:0;
        case 8: m.put("k",a); return !m.replace("k",b,a) && m.replace("k",a,b) && m.get("k")==b ? 1:0;
        case 9: m.put("k","value"); return m.containsValue(new String("value")) && m.containsKey(new String("k")) && !m.containsKey("none") ? 1:0;
        case 10: try {m.get(null);return 0;}catch(NullPointerException e){return 1;}
        case 11: try {m.put("k",null);return 0;}catch(NullPointerException e){return m.isEmpty()?1:0;}
        case 12: m.put("k",a); return !m.remove("k",null) && m.get("k")==a ? 1:0;
        case 13: try {new ConcurrentHashMap<Object,Object>(-1);return 0;}catch(IllegalArgumentException e){return 1;}
        case 14: try {new ConcurrentHashMap<Object,Object>(0,Float.NaN);return 0;}catch(IllegalArgumentException e){return 1;}
        case 15: try {new ConcurrentHashMap<Object,Object>(0,1.0f,0);return 0;}catch(IllegalArgumentException e){return 1;}
        case 16: m.put(a,b); return m.get(new Key(1))==b ? 1:0;
        case 17: {
            active=m; m.put(a,b); Key incoming=new Key(1); incoming.action=1;
            Object result=m.get(incoming); active=null;
            // JDK may linearize before or after the reentrant clear.
            return (result==null || result==b) && m.isEmpty() ? 1:0;
        }
        case 18: {
            active=m; m.put(a,b); Key incoming=new Key(1); incoming.action=2;
            Object result=m.put(incoming,"changed"); active=null;
            return result==b && m.size()==2 && "changed".equals(m.get(a)) && "retained".equals(m.get("side")) ? 1:0;
        }
        case 19: m.put(a,b); m.clear(); return m.get(a)==null && m.size()==0 ? 1:0;
        case 20: { Map<Object,Object> map=m; map.put("k",a); return map.get("k")==a ? 1:0; }
        case 21: { Child child=new Child(); child.put(a,b); return child.get(new Key(1))==b && child.value==71 ? 1:0; }
        case 22: return new ConcurrentHashMap<Object,Object>(0,Float.POSITIVE_INFINITY,2).isEmpty() ? 1:0;
        case 23: {
            active=m; m.put(a,b); Key incoming=new Key(1); incoming.action=3;
            try {m.get(incoming);return 0;} catch(IllegalStateException expected) {
                m.put("after","ok"); active=null; System.gc();
                return m.get(a)==b && "ok".equals(m.get("after")) ? 1:0;
            }
        }
        default: throw new IllegalArgumentException();
        }
    }
    public static ConcurrentHashMap<Object,Object> make() {
        active=new ConcurrentHashMap<Object,Object>(); active.put(new Key(7),new Key(9)); return active;
    }
    public static void release() {active=null;}
    public static void main(String[] ignored) {
        for(int i=0;i<24;i++) System.out.println(i+"\t"+probe(i));
    }
}
