package flow.aeon.capture

import android.app.Activity
import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
        }
        val title = TextView(this).apply {
            text = "AEON Capture"
            textSize = 22f
        }
        val endpoint = EditText(this).apply {
            hint = "http://192.168.1.8:8080"
            setText(AeonAgent.endpoint(this@MainActivity))
            isSingleLine = true
        }
        val save = Button(this).apply {
            text = "Save endpoint"
            setOnClickListener {
                AeonAgent.setEndpoint(this@MainActivity, endpoint.text.toString())
                announceDevice()
            }
        }
        val watcher = Button(this).apply {
            text = "Start photo watcher"
            setOnClickListener {
                requestPhotoPermission()
                startService(Intent(this@MainActivity, PhotoWatcherService::class.java))
            }
        }

        layout.addView(title)
        layout.addView(endpoint)
        layout.addView(save)
        layout.addView(watcher)
        setContentView(layout)
        announceDevice()
    }

    private fun announceDevice() {
        Thread {
            AeonAgent.hello(this@MainActivity)
        }.start()
    }

    private fun requestPhotoPermission() {
        val permission = if (Build.VERSION.SDK_INT >= 33) {
            Manifest.permission.READ_MEDIA_IMAGES
        } else {
            Manifest.permission.READ_EXTERNAL_STORAGE
        }
        requestPermissions(arrayOf(permission), 1001)
    }
}
