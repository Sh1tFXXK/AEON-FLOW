package flow.aeon.capture

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast

class ShareReceiverActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Thread {
            val ok = when (intent.action) {
                Intent.ACTION_SEND -> captureSingle(intent)
                Intent.ACTION_SEND_MULTIPLE -> captureMultiple(intent)
                else -> false
            }
            runOnUiThread {
                Toast.makeText(
                    this,
                    if (ok) "Captured to AEON" else "AEON capture failed",
                    Toast.LENGTH_SHORT
                ).show()
                finish()
            }
        }.start()
    }

    private fun captureSingle(intent: Intent): Boolean {
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        if (text != null) {
            return AeonAgent.captureText(this, text)
        }

        val uri = streamUri(intent)
        if (uri != null) {
            return AeonAgent.captureUri(this, uri)
        }

        return false
    }

    private fun captureMultiple(intent: Intent): Boolean {
        val uris = streamUris(intent) ?: return false
        var ok = false
        for (uri in uris) {
            ok = AeonAgent.captureUri(this, uri) || ok
        }
        return ok
    }

    private fun streamUri(intent: Intent): Uri? {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }
    }

    private fun streamUris(intent: Intent): ArrayList<Uri>? {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
        }
    }
}
