package org.androswitch.contracts;
public final class ForNameOracle {
  public static void main(String[] args)throws Exception{
    if(args.length!=0){for(int i=0;i<4;i++)System.out.println(i+"\t"+ForNameCases.defaults(i,ForNameOracle.class.getClassLoader()));return;}
    for(int i=0;i<34;i++)System.out.println(i+"\t"+ForNameCases.run(i,ForNameOracle.class.getClassLoader()));
  }
}
