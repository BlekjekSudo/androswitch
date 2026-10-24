package org.androswitch.contracts;
public final class RuntimeCases {
    public static Runtime expose() { return Runtime.getRuntime(); }
    public static int processors() { return Runtime.getRuntime().availableProcessors(); }
    public static String run(int test) {
        switch (test) {
            case 0: return "same=" + (Runtime.getRuntime() == Runtime.getRuntime());
            case 1: {
                Runtime before = Runtime.getRuntime();
                System.gc();
                return "afterGC=" + (before == Runtime.getRuntime());
            }
            case 2: return "positive=" + (processors() > 0);
            case 3: return "class=" + Runtime.getRuntime().getClass().getName();
            default: throw new IllegalArgumentException();
        }
    }
    public static void main(String[] args) {
        for (int test=0; test<4; test++) System.out.println(test + "\t" + run(test));
    }
}
