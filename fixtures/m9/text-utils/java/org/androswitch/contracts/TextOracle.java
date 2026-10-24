package org.androswitch.contracts;
/** JVM oracle for the API26 null-or-virtual-length expression, no Android stub. */
public class TextOracle {
    public static void main(String[] args){
        for(int kind=0;kind<19;kind++){
            TextCases.reset();
            try{
                CharSequence sequence=TextCases.make(kind);
                int result=TextCases.encode(sequence==null||sequence.length()==0);
                System.out.println(kind+"\tVALUE\t"+result);
            }catch(IllegalStateException failure){
                System.out.println(kind+"\tTHROW\t"+failure.getClass().getName()+"\t"+failure.getMessage()+"\t"+TextCases.calls);
            }
        }
    }
}
