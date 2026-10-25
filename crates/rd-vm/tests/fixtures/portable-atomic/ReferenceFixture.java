package org.androswitch.vm;
import java.util.concurrent.atomic.AtomicReference;

public final class ReferenceFixture {
    static final class Token {
        final int n;
        Token(int n) { this.n=n; }
        public boolean equals(Object ignored) { return true; }
        public String toString() { System.gc(); return "token"; }
    }
    static final class Child extends AtomicReference<Object> {
        public int value=19;
        Child(Object value) { super(value); }
        public String toString() { return "override"; }
        String base() { return super.toString(); }
    }
    public static int run(int test) {
        Token a=new Token(7), b=new Token(7);
        AtomicReference<Token> r=new AtomicReference<Token>();
        switch(test) {
        case 0: return r.get()==null ? 1 : 0;
        case 1: return new AtomicReference<Token>(a).get()==a ? 1 : 0;
        case 2: r.set(a); r.lazySet(b); return r.get()==b ? 1 : 0;
        case 3: r.set(a); return (r.getAndSet(b)==a && r.get()==b) ? 1 : 0;
        case 4: return (r.compareAndSet(null,a) && r.get()==a) ? 1 : 0;
        case 5: r.set(a); return (!r.compareAndSet(b,null) && r.get()==a) ? 1 : 0;
        case 6: while(!r.weakCompareAndSet(null,a)) {} return r.get()==a ? 1 : 0;
        case 7: r.set(a); System.gc(); return r.get().n;
        case 8: r.set(a); return (r.getAndSet(null)==a && r.get()==null) ? 1 : 0;
        default: throw new IllegalArgumentException();
        }
    }
    public static String text(int test) {
        switch(test) {
        case 0: return new AtomicReference<Object>().toString();
        case 1: return new AtomicReference<Object>("string").toString();
        case 2: return new AtomicReference<Object>(new Token(5)).toString();
        case 3: return new Child(new Token(1)).toString();
        case 4: return new Child(new Token(1)).base();
        default: throw new IllegalArgumentException();
        }
    }
    public static AtomicReference<Object> make() { return new AtomicReference<Object>(new Token(31)); }
    public static void main(String[] ignored) {
        for(int i=0;i<9;i++) System.out.println("run\t"+i+"\t"+run(i));
        for(int i=0;i<5;i++) System.out.println("text\t"+i+"\t"+text(i));
    }
}
