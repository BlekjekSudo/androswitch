package org.androswitch.contracts;
/** JVM reference uses genuine start/join. DEX tests deliver bounded workers explicitly. */
public final class UncaughtValidation {
    public static void main(String[] args) throws Exception {
        Thread.UncaughtExceptionHandler previous=Thread.getDefaultUncaughtExceptionHandler();
        try {
            Thread.setDefaultUncaughtExceptionHandler(null);
            System.out.println("defaultNull\t"+UncaughtCases.defaultNull());
            System.out.println("groupFallback\t"+UncaughtCases.groupFallback());
            for(int selection=0;selection<4;selection++) {
                UncaughtCases.setup(0); UncaughtCases.start(selection,0); UncaughtCases.worker.join();
                System.out.println("worker"+selection+"\t"+UncaughtCases.calls+":"+UncaughtCases.tag+":"+UncaughtCases.payload+":"+UncaughtCases.sameThread()+":"+UncaughtCases.sameException()+":"+UncaughtCases.alive);
            }
            UncaughtCases.setup(0); UncaughtCases.start(0,1); UncaughtCases.worker.join();
            System.out.println("caught\t"+UncaughtCases.calls+":"+UncaughtCases.caught);
            System.out.println("terminatedNull\t"+(UncaughtCases.worker.getUncaughtExceptionHandler()==null));
            UncaughtCases.worker.setUncaughtExceptionHandler(UncaughtCases.saved);
            System.out.println("setAfterStop\t"+(UncaughtCases.worker.getUncaughtExceptionHandler()==UncaughtCases.saved));
            UncaughtCases.worker.setUncaughtExceptionHandler(null);
            System.out.println("clearAfterStop\t"+(UncaughtCases.worker.getUncaughtExceptionHandler()==null));
            UncaughtCases.setup(1); UncaughtCases.start(0,0); UncaughtCases.worker.join();
            System.out.println("mutateAndGc\t"+UncaughtCases.calls+":"+UncaughtCases.payload+":"+UncaughtCases.defaultNull());
        } finally { Thread.setDefaultUncaughtExceptionHandler(previous); }
    }
}
