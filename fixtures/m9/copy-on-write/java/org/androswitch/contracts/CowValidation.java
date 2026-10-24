package org.androswitch.contracts;
public final class CowValidation {
    public static void main(String[] args){for(int i=0;i<=13;i++)System.out.println("scenario"+i+"\t"+CowCases.scenario(i));System.out.println("errors\t"+CowCases.errors());System.out.println("iteratorMessages\t"+CowCases.iteratorMessages());}
}
