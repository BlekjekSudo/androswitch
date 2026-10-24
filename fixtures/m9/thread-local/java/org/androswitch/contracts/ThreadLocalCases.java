package org.androswitch.contracts;
public final class ThreadLocalCases {
    public static final class Payload { public final int value; Payload(int v) { value=v; } }
    public static class Local extends ThreadLocal<Object> {
        final int mode; public int calls,sets;
        Local(int mode) { this.mode=mode; }
        public void set(Object v) { sets++; super.set(v); }
        protected Object initialValue() {
            calls++;
            if(mode==1) return null;
            if(mode==2) { set(new Payload(22)); return new Payload(33); }
            if(mode==3 && calls==1) throw new IllegalStateException("initializer-first");
            if(mode==4) { set(new Payload(44)); throw new IllegalStateException("after-set"); }
            if(mode==5 && calls==1) return get();
            if(mode==6) return nativeBounce(this);
            if(mode==7) { if(super.initialValue()!=null) throw new AssertionError("base initialValue"); }
            if(mode==9) remove();
            return new Payload(10+calls);
        }
    }
    public static class Parent extends ThreadLocal<Object> {
        protected Object initialValue() { return new Payload(41); }
    }
    public static class Child extends Parent {
        protected Object initialValue() { return new Payload(((Payload)super.initialValue()).value+1); }
    }
    public static class Covariant extends ThreadLocal<Payload> {
        protected Payload initialValue() { return new Payload(81); }
    }
    public static class Override extends Local {
        public int gets;
        Override() { super(0); }
        public Object get() { gets++; return super.get(); }
    }
    static ThreadLocal<Object> shared;
    static Local special;
    static Thread worker, firstWorker, seenWorker;
    static Payload saved;
    static Object seen;
    static ThreadLocal<Object>[] many;
    static int result,workerCalls;
    static boolean sameThread,notMain;
    public static native Object nativeBounce(Local local);
    static int number(Object value) { return ((Payload)value).value; }
    public static String scenario(int mode) {
        Local v=new Local(mode);
        if(mode==0) {
            Object a=v.get(); boolean same=a==v.get(); v.set(null);
            boolean nullValue=v.get()==null; int before=v.calls;
            v.remove(); int next=number(v.get());
            return number(a)+":"+(same?1:0)+":"+(nullValue?1:0)+":"+before+":"+next+":"+v.calls;
        }
        if(mode==1) { boolean n=v.get()==null && v.get()==null; int a=v.calls; v.remove(); v.get(); return (n?1:0)+":"+a+":"+v.calls; }
        if(mode==2) return number(v.get())+":"+number(v.get())+":"+v.calls+":"+v.sets;
        if(mode==3) { boolean caught=false; try {v.get();}catch(IllegalStateException okay){caught=true;} return (caught?1:0)+":"+number(v.get())+":"+v.calls; }
        if(mode==4) { boolean caught=false; try {v.get();}catch(IllegalStateException okay){caught=true;} return (caught?1:0)+":"+number(v.get())+":"+v.calls; }
        if(mode==5 || mode==6 || mode==7) return number(v.get())+":"+v.calls;
        if(mode==8) { v.set(null); boolean n=v.get()==null;int before=v.calls;v.remove();return (n?1:0)+":"+before+":"+number(v.get())+":"+v.calls; }
        if(mode==9) return number(v.get())+":"+number(v.get())+":"+v.calls;
        throw new IllegalArgumentException("scenario");
    }
    public static String precedence() {
        Override v=new Override(); Object x=v.get(); v.get();
        return number(x)+":"+v.gets+":"+v.calls+":"+number(new Child().get())+":"+new Covariant().get().value;
    }
    public static Object nativeInner() { ThreadLocal<Object> other=new ThreadLocal<Object>(); other.set(new Payload(88)); System.gc(); return other.get(); }
    public static void setup() {
        shared=new ThreadLocal<Object>(); shared.set(new Payload(101));
        result=workerCalls=0; worker=firstWorker=seenWorker=null; seen=null; saved=null;
        sameThread=notMain=false;
    }
    public static final class Task implements Runnable {
        final int value,mode;
        Task(int value,int mode) { this.value=value;this.mode=mode; }
        public void run() {
            Thread current=Thread.currentThread(); seenWorker=current;
            if(mode==0) { boolean empty=shared.get()==null; shared.set(new Payload(value)); System.gc(); result=number(shared.get())+(empty?1000:0); }
            if(mode==1) { seen=shared.get(); result=seen==null?-1:number(seen); shared.set(new Payload(value)); }
            if(mode==2) { seen=shared.get(); result=seen==null?-1:number(seen); }
            sameThread=current==worker; notMain=current!=firstWorker; workerCalls++;
        }
    }
    public static void start(int value,int mode) { worker=new Thread(new Task(value,mode),"threadlocal-worker"); if(firstWorker==null) firstWorker=Thread.currentThread(); worker.start(); }
    public static Thread taskThread(int value,int mode) { return new Thread(new Task(value,mode),"manual-task"); }
    public static int result() { return result; }
    public static int mainValue() { return number(shared.get()); }
    public static boolean identity() { return sameThread && notMain; }
    public static int value() { return shared.get()==null?-1:number(shared.get()); }
    public static void setValue(int v) { shared.set(new Payload(v)); }
    public static void setNull() { shared.set(null); }
    public static Object valueObject() { return shared.get(); }
    public static Object localObject() { return shared; }
    public static void dropKey() { shared=null; }
    public static void initializeSpecial() { special=new Local(0); special.get(); }
    public static void fill(int count) { for(int i=0;i<count;i++) { ThreadLocal<Object> v=new ThreadLocal<Object>(); v.set(new Payload(i)); if(i==0) shared=v; } }
    @SuppressWarnings("unchecked") public static void setupMany(int count) { many=(ThreadLocal<Object>[])new ThreadLocal<?>[count];for(int i=0;i<count;i++)many[i]=new ThreadLocal<Object>(); }
    public static void setMany(int count) { for(int i=0;i<count;i++)many[i].set(null); }
    public static void removeFirst() { many[0].remove(); }
    public static Object firstMany() { return many[0].get(); }
    public static void unsupportedInheritable() { new InheritableThreadLocal<Object>(); }
}
