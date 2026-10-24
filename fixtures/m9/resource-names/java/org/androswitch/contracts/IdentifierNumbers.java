package org.androswitch.contracts;

/** Host-only Java numeric oracle; resource lookup itself runs through Android DEX. */
public final class IdentifierNumbers {
  private static void emit(String input) {
    StringBuilder units=new StringBuilder();
    for(int i=0;i<input.length();i++) {
      if(i!=0) units.append(',');
      units.append(Integer.toHexString(input.charAt(i)));
    }
    try {
      System.out.println(units+"\tVALUE\t"+Integer.parseInt(input));
    } catch(NumberFormatException failure) {
      System.out.println(units+"\tINVALID\t0");
    }
  }
  public static void main(String[] args) {
    String[] common={"0","1","+123","-1","-0","+0","2147483647","-2147483648",
      "2147483648","-2147483649","0002147483647","0x7f010000"," 123","123 ",
      "+","-","","1_000","++1","--1","\u00b2","\ud835\udfd9","12\u0663"};
    for(String input:common) emit(input);
    // All BMP decimal-digit blocks from ICU58 / Unicode9. These existed in
    // API26; current JVM results are not used to admit newer digit blocks.
    int[] starts={0x0030,0x0660,0x06f0,0x07c0,0x0966,0x09e6,0x0a66,0x0ae6,
      0x0b66,0x0be6,0x0c66,0x0ce6,0x0d66,0x0de6,0x0e50,0x0ed0,0x0f20,
      0x1040,0x1090,0x17e0,0x1810,0x1946,0x19d0,0x1a80,0x1a90,0x1b50,
      0x1bb0,0x1c40,0x1c50,0xa620,0xa8d0,0xa900,0xa9d0,0xa9f0,0xaa50,0xabf0,0xff10};
    for(int start:starts) {
      emit(new String(new char[]{(char)(start+1),(char)(start+2),(char)(start+3)}));
      StringBuilder digits=new StringBuilder();
      for(int i=0;i<10;i++) digits.append((char)(start+i));
      emit(digits.toString());
    }
  }
}
