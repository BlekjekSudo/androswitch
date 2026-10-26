import java.lang.reflect.*;
import java.io.*;
import java.util.*;
public final class StaticOracle {
  static String desc(Class<?> c) {
    if(c.isArray()) return c.getName().replace('.','/');
    if(!c.isPrimitive()) return "L"+c.getName().replace('.','/')+";";
    if(c==int.class)return "I"; if(c==long.class)return "J";
    if(c==float.class)return "F"; if(c==double.class)return "D";
    if(c==boolean.class)return "Z"; if(c==char.class)return "C";
    if(c==byte.class)return "B"; if(c==short.class)return "S"; return "V";
  }
  static String sig(Method m) {
    String s="("; for(Class<?> c:m.getParameterTypes())s+=desc(c);
    return s+")"+desc(m.getReturnType());
  }
  static String units(String s) {
    StringBuilder b=new StringBuilder();
    for(int i=0;i<s.length();i++) { if(i!=0)b.append('.'); b.append(Integer.toHexString(s.charAt(i))); }
    return b.toString();
  }
  static String encode(Object v,Class<?> type) {
    if(v==null)return "n";
    if(type==String.class || type==CharSequence.class)return "s:"+units((String)v);
    if(type==char[].class)return "a:"+units(new String((char[])v));
    if(!type.isPrimitive()) {
      Class<?> primitive=v instanceof Boolean?boolean.class:v instanceof Character?char.class:
          v instanceof Byte?byte.class:v instanceof Short?short.class:v instanceof Integer?int.class:
          v instanceof Long?long.class:v instanceof Float?float.class:v instanceof Double?double.class:null;
      return primitive==null?"unclaimed-object":("o:"+desc(type)+":"+encode(v,primitive));
    }
    if(type==boolean.class)return "i:"+(((Boolean)v)?1:0);
    if(type==char.class)return "i:"+(int)((Character)v);
    if(type==float.class)return "f:"+Integer.toHexString(Float.floatToRawIntBits((Float)v));
    if(type==double.class)return "d:"+Long.toHexString(Double.doubleToRawLongBits((Double)v));
    if(type==long.class)return "j:"+v;
    return "i:"+v;
  }
  static Object[] args(Method m) {
    Class<?>[] p=m.getParameterTypes(); Object[] a=new Object[p.length];
    String c=m.getDeclaringClass().getSimpleName(), n=m.getName();
    for(int i=0;i<p.length;i++) {
      Class<?> t=p[i];
      if(t==int.class)a[i]=i==0?-7:3;
      else if(t==long.class)a[i]=i==0?-7L:3L;
      else if(t==float.class)a[i]=i==0?-0.0f:0.0f;
      else if(t==double.class)a[i]=i==0?-0.0:0.0;
      else if(t==boolean.class)a[i]=i==0;
      else if(t==char.class)a[i]=i==0?'A':'a';
      else if(t==String.class)a[i]=c.equals("Boolean")?"TrUe":(c.equals("Float")||c.equals("Double"))?"1.25":"42";
      else if(t==CharSequence.class)a[i]="a\uD83D\uDE42z";
      else if(t==char[].class)a[i]=new char[]{'x','x','x','x'};
      else return null;
    }
    if(c.equals("Integer")||c.equals("Long")) {
      if((n.equals("toString")||n.equals("toUnsignedString"))&&p.length==2)a[1]=16;
      if(p.length==2&&p[0]==String.class&&p[1]==int.class){a[0]="2a";a[1]=16;}
    }
    if(c.equals("Character")) {
      if(p.length>0&&(p[0]==int.class))a[0]=65;
      if(n.equals("digit")){a[0]=p[0]==char.class?(Object)'F':(Object)70;a[1]=16;}
      if(n.equals("charCount"))a[0]=0x1f642;
      if(n.equals("isSurrogatePair")||n.equals("toCodePoint")){a[0]='\uD83D';a[1]='\uDE42';}
      if(n.equals("codePointAt")||n.equals("codePointBefore"))a[1]=n.equals("codePointAt")?1:3;
      if(n.equals("toChars")&&p.length==3){a[0]=0x1f642;a[2]=1;}
    }
    if(n.equals("round")&&p.length==1) {
      if(p[0]==float.class)a[0]=-1.5f; else if(p[0]==double.class)a[0]=-1.5;
    }
    return a;
  }
  public static void main(String[] args)throws Exception {
    Class<?>[] classes={Integer.class,Long.class,Float.class,Double.class,Boolean.class,Character.class,Math.class,StrictMath.class};
    PrintWriter out=new PrintWriter(new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8"));
    for(Class<?> c:classes) {
      List<Method> methods=new ArrayList<Method>(Arrays.asList(c.getDeclaredMethods()));
      Collections.sort(methods,new Comparator<Method>(){public int compare(Method a,Method b){return (a.getName()+sig(a)).compareTo(b.getName()+sig(b));}});
      for(Method m:methods) {
        if(!Modifier.isPublic(m.getModifiers())||!Modifier.isStatic(m.getModifiers())||m.isSynthetic())continue;
        Object[] original=args(m); if(original==null)continue;
        boolean refs=false; for(Class<?> p:m.getParameterTypes())refs|=!p.isPrimitive();
        for(int scenario=0;scenario<(refs?2:1);scenario++) {
          Object[] a=args(m); Class<?>[] p=m.getParameterTypes();
          if(scenario==1)for(int i=0;i<a.length;i++)if(!p[i].isPrimitive())a[i]=null;
          String inputs="";for(int i=0;i<a.length;i++){if(i!=0)inputs+=",";inputs+=encode(a[i],p[i]);}
          String result;
          try {result=encode(m.invoke(null,a),m.getReturnType());}
          catch(InvocationTargetException e){result="e:"+desc(e.getCause().getClass());}
          for(Object v:a)if(v instanceof char[])result+="|"+encode(v,char[].class);
          out.println(desc(c)+"\t"+m.getName()+"\t"+sig(m)+"\t"+inputs+"\t"+result);
        }
      }
    }
    out.close();
  }
}
