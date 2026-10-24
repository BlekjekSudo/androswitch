package org.androswitch.contracts;
import java.io.FileOutputStream;
public final class CowProofActivity extends CowActivity {
    public void onCreate(android.os.Bundle state){
        super.onCreate(state);
        String snapshot=CowCases.scenario(0);String equality=CowCases.scenario(2);String cursor=CowCases.scenario(6);
        CowCases.setupSnapshot();CowCases.dropList();System.gc();int retained=CowCases.nextSaved();CowCases.dropIterator();System.gc();
        if(!snapshot.equals("1,2,/3,-1,")||!equality.equals("1:1,9,")||!cursor.equals("2:2:1:0:-1:3:1:1")||retained!=101||CowCases.iteratorMessages()!=5)throw new IllegalStateException("cow-proof-contract");
        try{FileOutputStream out=new FileOutputStream("/sdcard/Download/androswitch-cow-proof.txt");out.write(("COW_SNAPSHOT_PROOF_PASS\nsnapshot="+snapshot+"\nreentrantRemove="+equality+"\niterator="+cursor+"\ngcRetained="+retained+"\n").getBytes("UTF-8"));out.close();}catch(java.io.IOException failure){throw new IllegalStateException("cow-proof-io");}
        android.widget.TextView view=new android.widget.TextView(this);view.setText("COW proof PASS: snapshots, callback mutation, GC; VFS saved");setContentView(view);
    }
}
