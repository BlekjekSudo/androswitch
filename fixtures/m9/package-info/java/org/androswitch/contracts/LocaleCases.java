package org.androswitch.contracts;
import java.util.Locale;
public final class LocaleCases {
  public static String normalize(int index){
    Locale locale;
    switch(index){
      case 0:locale=new Locale("FR","ca","POSIX");break;
      case 1:locale=new Locale("","us");break;
      case 2:locale=new Locale("en","","POSIX");break;
      case 3:locale=new Locale("","","POSIX");break;
      default:locale=new Locale("Ja");break;
    }
    return locale.getLanguage()+"|"+locale.getCountry()+"|"+locale.getVariant()+"|"+locale.toString();
  }
  public static int proof(){
    Locale original=Locale.getDefault();
    if(!"en".equals(original.getLanguage()) || !"US".equals(original.getCountry()))throw new IllegalStateException("declared guest locale");
    System.gc();if(Locale.getDefault()!=original)throw new IllegalStateException("default identity");
    Locale other=new Locale("FR","ca","POSIX");Locale.setDefault(other);System.gc();
    if(Locale.getDefault()!=other || !"fr_CA_POSIX".equals(other.toString()))throw new IllegalStateException("default replacement");
    if(!"".equals(Locale.ROOT.getCountry()) || !"".equals(Locale.ROOT.toString()))throw new IllegalStateException("ROOT");
    try{Locale.setDefault(null);throw new IllegalStateException("null accepted");}catch(NullPointerException expected){}
    if(Locale.getDefault()!=other)throw new IllegalStateException("failed set mutated default");
    Locale.setDefault(original);return 1;
  }
  public static Locale current(){return Locale.getDefault();}
  public static void main(String[] args){for(int index=0;index<5;index++)System.out.println(index+"\t"+normalize(index));}
}
