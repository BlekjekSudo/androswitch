package org.androswitch.kui
import android.app.Activity
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
class MainActivity : Activity(), View.OnClickListener {
    private lateinit var label: TextView
    private var count = 0
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        val root = LinearLayout(this)
        root.setOrientation(1)
        label = TextView(this)
        label.setText("Kotlin ready")
        root.addView(label, -1, 48)
        val button = Button(this)
        button.setText("Kotlin callback")
        button.setOnClickListener(this)
        root.addView(button, -1, 48)
        setContentView(root)
    }
    override fun onClick(view: View) { count++; label.setText("Kotlin tapped " + count) }
}
