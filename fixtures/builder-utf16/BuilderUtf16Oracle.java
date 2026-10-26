// Original AndroSwitch contract fixture, MIT OR Apache-2.0.
public final class BuilderUtf16Oracle {
  public static String run(int which) {
    StringBuilder b = new StringBuilder();
    char high = '\uD83D', low = '\uDE42';
    switch (which) {
      case 0: b.append(high); return b.length()+":"+(int)b.charAt(0)+":"+b.codePointAt(0);
      case 1: b.append(high).append(low); return b.length()+":"+b.codePointAt(0);
      case 2: b.append("ab").insert(1,high).insert(2,low); return b.length()+":"+b.codePointAt(1)+":"+(int)b.charAt(3);
      case 3: b.append("ab"); b.setCharAt(0,high); b.setCharAt(1,low); return ""+b.codePointAt(0);
      case 4: b.append(low).append(high).reverse(); return ""+b.codePointAt(0);
      case 5: b.appendCodePoint(high).appendCodePoint(low); return ""+b.codePointAt(0);
      case 6: b.appendCodePoint(0x1F642); b.setLength(1); b.append(low); return ""+b.codePointAt(0);
      case 7: b.append(high); b.setLength(3); char[] c=new char[3]; b.getChars(0,3,c,0); return (int)c[0]+":"+(int)c[1]+":"+(int)c[2];
      case 8: b.append(high).append('x').deleteCharAt(1).append(low); return ""+b.codePointAt(0);
      case 9: b.append(high).append('x').append(low).replace(1,2,""); return ""+b.codePointAt(0);
      case 10: b.append('a').appendCodePoint(0x1F642).append('b').reverse(); return (int)b.charAt(0)+":"+b.codePointAt(1)+":"+(int)b.charAt(3);
      default: throw new IllegalArgumentException();
    }
  }
  public static void main(String[] args) {
    for(int i=0;i<11;i++) System.out.println(i+"\t"+run(i));
  }
}
