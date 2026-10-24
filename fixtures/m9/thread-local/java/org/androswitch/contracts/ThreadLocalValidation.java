package org.androswitch.contracts;
public final class ThreadLocalValidation {
    public static void main(String[] args) throws Exception {
        for(int mode=0;mode<=9;mode++) if(mode!=6) System.out.println("scenario"+mode+"\t"+ThreadLocalCases.scenario(mode));
        System.out.println("precedence\t"+ThreadLocalCases.precedence());
        ThreadLocalCases.setup(); ThreadLocalCases.start(202,0); ThreadLocalCases.worker.join();
        System.out.println("worker\t"+ThreadLocalCases.result()+":"+ThreadLocalCases.mainValue()+":"+ThreadLocalCases.identity());
        ThreadLocalCases.start(303,2); ThreadLocalCases.worker.join();
        System.out.println("newWorker\t"+ThreadLocalCases.result()+":"+ThreadLocalCases.mainValue());
    }
}
