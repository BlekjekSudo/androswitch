package org.androswitch.contracts;
import java.util.*;
import java.util.concurrent.CopyOnWriteArrayList;
public final class CowCases {
    public static final class Node { public final int id; Node(int id){this.id=id;} }
    public static final class Key {
        final int id; int mode,calls;final boolean probe;
        Key(int id,int mode){this.id=id;this.mode=mode;this.probe=mode==6;}
        public boolean equals(Object other){
            calls++;int action=mode;mode=0;
            if(action==1) list.add(new Node(9));
            if(action==2) {list.clear();System.gc();}
            if(action==3) throw new IllegalStateException("equals-failure");
            if(action==4) list.set(0,new Node(19));
            if(action==5) nativeTouch();
            if(action==6) list.set(0,new Node(8));
            if(probe && calls==3) nativeTouch();
            return other instanceof Node && ((Node)other).id==id;
        }
    }
    public static class Derived extends CopyOnWriteArrayList<Object> {
        int adds;
        public boolean add(Object value){adds++;return super.add(value);}
    }
    static CopyOnWriteArrayList<Object> list;
    static Iterator<Object> saved;
    static Iterator<?>[] retained;
    static Key query;
    public static native void nativeTouch();
    public static int id(Object value){return value==null?-1:value instanceof Node?((Node)value).id:((Key)value).id;}
    static String contents(CopyOnWriteArrayList<Object> values){StringBuilder text=new StringBuilder();Iterator<Object> iterator=values.iterator();while(iterator.hasNext())text.append(id(iterator.next())).append(',');return text.toString();}
    public static String scenario(int mode){
        list=new CopyOnWriteArrayList<Object>();list.add(new Node(1));list.add(new Node(2));
        if(mode==0){saved=list.iterator();list.set(0,new Node(3));list.add(null);list.remove(1);StringBuilder s=new StringBuilder();while(saved.hasNext())s.append(id(saved.next())).append(',');return s.toString()+"/"+contents(list);}
        if(mode==1){query=new Key(2,0);int a=list.contains(query)?1:0;int b=list.indexOf(query);int c=list.lastIndexOf(query);boolean removed=list.remove(query);return a+":"+b+":"+c+":"+(removed?1:0)+":"+contents(list);}
        if(mode==2){query=new Key(2,1);boolean removed=list.remove(query);return (removed?1:0)+":"+contents(list);}
        if(mode==3){query=new Key(5,1);boolean added=list.addIfAbsent(query);return (added?1:0)+":"+contents(list);}
        if(mode==4){query=new Key(2,2);boolean contains=list.contains(query);return (contains?1:0)+":"+list.size()+":"+query.calls;}
        if(mode==5){query=new Key(2,3);boolean caught=false;try{list.remove(query);}catch(IllegalStateException expected){caught=true;}list.add(new Node(7));return (caught?1:0)+":"+contents(list);}
        if(mode==6){ListIterator<Object> it=list.listIterator(1);int a=id(it.next()),b=id(it.previous()),c=id(it.previous());int u=0;try{it.remove();}catch(UnsupportedOperationException expected){u++;}try{it.set(null);}catch(UnsupportedOperationException expected){u++;}try{it.add(null);}catch(UnsupportedOperationException expected){u++;}int n=0;try{it.previous();}catch(NoSuchElementException expected){n++;}list.clear();return a+":"+b+":"+c+":"+it.nextIndex()+":"+it.previousIndex()+":"+u+":"+n+":"+(it.hasNext()?1:0);}
        if(mode==7){Object[] source=new Object[]{new Node(4),null};CopyOnWriteArrayList<Object> a=new CopyOnWriteArrayList<Object>(source);source[0]=new Node(99);Object[] copy=a.toArray();copy[0]=new Node(98);CopyOnWriteArrayList<Object> b=new CopyOnWriteArrayList<Object>((Collection<?>)a);a.clear();return contents(b);}
        if(mode==8){Node[] target=new Node[]{new Node(7),new Node(8),new Node(9),new Node(10)};Object[] result=list.toArray(target);boolean same=result==target;Node[] small=list.toArray(new Node[0]);return (same?1:0)+":"+target[0].id+":"+target[1].id+":"+(target[2]==null?1:0)+":"+target[3].id+":"+small.length;}
        if(mode==9){list.add(new Key(3,0));Node[] target=new Node[4];boolean caught=false;try{list.toArray(target);}catch(ArrayStoreException expected){caught=true;}return (caught?1:0)+":"+target[0].id+":"+target[1].id+":"+(target[2]==null?1:0);}
        if(mode==10){list.addAll(list);list.addAll(1,new CopyOnWriteArrayList<Object>(new Object[]{null,new Node(8)}));return contents(list);}
        if(mode==11){Derived d=new Derived();d.add(new Node(4));d.addAll(list);return d.adds+":"+contents(d);}
        if(mode==12){list.add(null);boolean a=list.addIfAbsent(null);boolean b=list.addIfAbsent(new Key(2,0));return (a?1:0)+":"+(b?1:0)+":"+list.size();}
        if(mode==13){query=new Key(2,4);boolean removed=list.remove(query);return (removed?1:0)+":"+contents(list);}
        if(mode==14){query=new Key(2,5);boolean contained=list.contains(query);return (contained?1:0)+":"+contents(list);}
        if(mode==15){query=new Key(2,6);boolean removed=list.remove(query);return (removed?1:0)+":"+contents(list);}
        throw new IllegalArgumentException("scenario");
    }
    public static String errors(){
        list=new CopyOnWriteArrayList<Object>();int total=0;
        try{list.get(-1);}catch(ArrayIndexOutOfBoundsException expected){total++;}
        try{list.set(0,null);}catch(ArrayIndexOutOfBoundsException expected){total++;}
        try{list.add(-1,null);}catch(IndexOutOfBoundsException expected){total++;}
        try{list.remove(0);}catch(ArrayIndexOutOfBoundsException expected){total++;}
        try{list.listIterator(1);}catch(IndexOutOfBoundsException expected){total++;}
        try{list.iterator().next();}catch(NoSuchElementException expected){total++;}
        try{new CopyOnWriteArrayList<Object>((Object[])null);}catch(NullPointerException expected){total++;}
        try{new CopyOnWriteArrayList<Object>((Collection<?>)null);}catch(NullPointerException expected){total++;}
        try{list.toArray((Object[])null);}catch(NullPointerException expected){total++;}
        return total+":"+list.size();
    }
    public static void setupSnapshot(){list=new CopyOnWriteArrayList<Object>();list.add(new Node(101));saved=list.iterator();list.clear();}
    public static int iteratorMessages(){
        ListIterator<Object> empty=new CopyOnWriteArrayList<Object>().listIterator();int count=0;
        try{empty.next();}catch(NoSuchElementException expected){if(expected.getMessage()==null)count++;}
        try{empty.previous();}catch(NoSuchElementException expected){if(expected.getMessage()==null)count++;}
        try{empty.remove();}catch(UnsupportedOperationException expected){if(expected.getMessage()==null)count++;}
        try{empty.set(null);}catch(UnsupportedOperationException expected){if(expected.getMessage()==null)count++;}
        try{empty.add(null);}catch(UnsupportedOperationException expected){if(expected.getMessage()==null)count++;}
        return count;
    }
    public static Object listObject(){return list;}
    public static Object oldValue(){return saved.next();}
    public static int nextSaved(){return id(saved.next());}
    public static void dropList(){list=null;}
    public static void dropIterator(){saved=null;}
    public static void nativeReentry(){list.clear();list.add(new Node(77));System.gc();}
    public static void budgetSetup(int count){list=new CopyOnWriteArrayList<Object>(new Object[count]);}
    public static void budgetGrow(){list.add(null);}
    public static int size(){return list.size();}
    public static void retainSnapshots(){retained=new Iterator<?>[1000];for(int i=0;i<retained.length;i++){retained[i]=list.iterator();list.set(0,new Node(i));}}
    public static void releaseSnapshots(){retained=null;System.gc();}
    public static void lockReentry(){list.add(new Node(55));}
    public static void setSameReference(){list.set(0,list.get(0));}
    public static int invalidIndex(){try{list.set(-1,null);return 0;}catch(ArrayIndexOutOfBoundsException expected){return 1;}}
    public static void rotateGenerations(int count){for(int i=0;i<count;i++)list.set(0,new Node(i));}
    public static int first(){return id(list.get(0));}
    public static void unsupported(){list.subList(0,0);}
    public static int regular(){ArrayList<Object> regular=new ArrayList<Object>();regular.add(new Node(1));Iterator<Object> old=regular.iterator();regular.add(new Node(2));try{old.next();return 0;}catch(ConcurrentModificationException expected){return 1;}}
}
