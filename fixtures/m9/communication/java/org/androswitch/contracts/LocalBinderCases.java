package org.androswitch.contracts;
import android.os.Binder;
import android.os.IBinder;
import android.os.IInterface;

/** Normal SDK local-interface calls; no remote service or Parcel surrogate. */
public final class LocalBinderCases {
  public interface Echo extends IInterface {String echo(String value);}
  public static final class Endpoint extends Binder implements Echo {
    Endpoint(String descriptor){attachInterface(this,descriptor);}
    public IBinder asBinder(){return this;}
    public String echo(String value){System.gc();return "echo:"+value;}
  }
  public static final class Owner implements IInterface {
    private final IBinder binder;
    Owner(IBinder binder){this.binder=binder;}
    public IBinder asBinder(){return binder;}
  }
  public static Binder kept;
  public static Binder make(){kept=new Endpoint("contract.echo");return kept;}
  public static int proof()throws Exception {
    Binder empty=new Binder();
    if(empty.getInterfaceDescriptor()!=null || empty.queryLocalInterface(null)!=null)throw new IllegalStateException("new Binder descriptor");
    Endpoint endpoint=new Endpoint("contract.echo");kept=endpoint;
    if(!(endpoint instanceof IBinder) || !IBinder.class.isInterface() || !IInterface.class.isInterface())throw new IllegalStateException("Binder types");
    IInterface owner=endpoint.queryLocalInterface(new String("contract.echo"));
    System.gc();
    if(owner!=endpoint || owner.asBinder()!=endpoint || !"echo:value".equals(((Echo)owner).echo("value")))throw new IllegalStateException("actual local dispatch");
    String descriptor=new String("contract.rebound");
    Owner rebound=new Owner(endpoint);endpoint.attachInterface(rebound,descriptor);
    if(endpoint.getInterfaceDescriptor()!=descriptor || endpoint.queryLocalInterface("contract.echo")!=null || endpoint.queryLocalInterface(descriptor)!=rebound)throw new IllegalStateException("rebind identity");
    if(!endpoint.pingBinder() || !endpoint.isBinderAlive())throw new IllegalStateException("live local Binder");
    endpoint.attachInterface(rebound,null);
    if(endpoint.getInterfaceDescriptor()!=null || endpoint.queryLocalInterface(null)!=null)throw new IllegalStateException("null descriptor");
    endpoint.attachInterface(null,"contract.empty");
    if(endpoint.queryLocalInterface("contract.empty")!=null)throw new IllegalStateException("null owner");
    kept=null;return 1;
  }
  public static IInterface query(String name){return kept.queryLocalInterface(name);}
  public static void clear(){kept=null;}
}
