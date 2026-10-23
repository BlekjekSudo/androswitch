package org.androswitch.media;
import android.app.Activity;
import android.os.Bundle;
import android.content.res.AssetManager;
import android.media.AudioTrack;
import android.media.AudioManager;
import android.media.AudioFormat;
import android.widget.TextView;
import java.io.InputStream;
public final class MainActivity extends Activity {
    static {System.loadLibrary("mixed-media");}
    private static native int nativeMedia(AssetManager assets);
    private static native int queuedAudio();
    public void onCreate(Bundle bundle){
        super.onCreate(bundle);
        int assetCount=0;
        try {InputStream stream=getAssets().open("stream.bin");byte[] chunk=new byte[137];int n;while((n=stream.read(chunk))>0)assetCount+=n;stream.close();}catch(Exception e){assetCount=-1;}
        AudioTrack track=new AudioTrack(AudioManager.STREAM_MUSIC,48000,AudioFormat.CHANNEL_OUT_STEREO,AudioFormat.ENCODING_PCM_16BIT,9600,AudioTrack.MODE_STREAM);
        short[] samples=new short[4800];for(int i=0;i<samples.length;i++)samples[i]=(short)(i%96<48?1500:-1500);
        track.play();int written=track.write(samples,0,samples.length);track.stop();track.release();
        int nativeResult=nativeMedia(getAssets());
        int queued=queuedAudio();
        TextView label=new TextView(this);label.setText("Mixed assets "+assetCount+" / Java PCM "+written+" / native "+nativeResult+" / SL "+queued);setContentView(label);
    }
}
