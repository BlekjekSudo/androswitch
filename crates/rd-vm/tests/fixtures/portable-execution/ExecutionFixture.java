package org.androswitch.vm.execution;

// Ordinary Java 8 source; the native contract is supplied by the host test
// provider. The independent run() rows execute unchanged on a reference JVM.
public final class ExecutionFixture {
    public static native Object nativeLocal(int mode);
    public static native int initHook();
    public static native int initFailure();

    public static void collect() { System.gc(); }
    public static void collectAndThrow() {
        System.gc();
        throw new IllegalStateException("callback failure");
    }

    public static final class Contended {
        public static int value = 7;
        static { value = initHook() + 35; }
        public static int read() { return value; }
    }
    public static final class NativeFailure {
        public static int value = initFailure();
        public static int read() { return value; }
    }
    public static final class Recursive {
        public static int visits;
        public static int value = initialize();
        private static int initialize() { visits++; return read() + 7; }
        public static int read() { return value; }
    }
    public static class Parent {
        public static int parentValue = 31;
    }
    public static final class Child extends Parent {
        public static int childValue = parentValue + 11;
        public static int read() { return childValue; }
    }
    public static class InitializingParent {
        public static int value = initHook() + 35;
        public static int read() { return value; }
    }
    public static final class ContendingChild extends InitializingParent {
        public static int childValue = 99;
        public static int readChild() { return childValue; }
    }
    public static final class Broken {
        public static int value = broken();
        private static int broken() { throw new RuntimeException("initialization failure"); }
        public static int read() { return value; }
    }
    public static int run(int test) {
        switch (test) {
            case 0: return Recursive.read() * 100 + Recursive.visits;
            case 1:
                try { return Broken.read(); }
                catch (ExceptionInInitializerError expected) { return 1; }
            case 2:
                try { return Broken.read(); }
                catch (NoClassDefFoundError expected) { return 2; }
            case 3: return Child.read();
            default: throw new IllegalArgumentException();
        }
    }
    public static void main(String[] args) {
        for (int test = 0; test < 4; test++)
            System.out.println(test + "\t" + run(test));
    }
}
