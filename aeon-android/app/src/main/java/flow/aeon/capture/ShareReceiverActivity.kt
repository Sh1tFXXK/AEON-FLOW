package flow.aeon.capture

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.widget.Toast

class ShareReceiverActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Thread {
            val result = try {
                when (intent.action) {
                    Intent.ACTION_SEND -> captureSingle(intent)
                    Intent.ACTION_SEND_MULTIPLE -> captureMultiple(intent)
                    else -> AeonAgent.ActionResult(false, "Unsupported share action")
                }
            } catch (error: Throwable) {
                Log.e("AEON", "Share capture failed", error)
                AeonAgent.ActionResult(false, error.message ?: error.javaClass.simpleName)
            }
            Log.i("AEON", "Share capture result ok=${result.ok} action=${intent.action} message=${result.message}")
            runOnUiThread {
                Toast.makeText(
                    this,
                    result.message,
                    Toast.LENGTH_SHORT
                ).show()
                finish()
            }
        }.start()
    }

    private fun captureSingle(intent: Intent): AeonAgent.ActionResult {
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        if (text != null) {
            return AeonAgent.captureTextResult(this, text)
        }

        val uri = streamUri(intent)
        if (uri != null) {
            return AeonAgent.captureUriResult(this, uri)
        }

        return AeonAgent.ActionResult(false, "No shared content found")
    }

    private fun captureMultiple(intent: Intent): AeonAgent.ActionResult {
        val uris = streamUris(intent) ?: return AeonAgent.ActionResult(false, "No shared files found")
        var okCount = 0
        var lastError = "No files captured"
        for (uri in uris) {
            val result = AeonAgent.captureUriResult(this, uri)
            if (result.ok) {
                okCount += 1
            } else {
                lastError = result.message
            }
        }
        return if (okCount > 0) {
            AeonAgent.ActionResult(true, "Captured $okCount/${uris.size} to AEON")
        } else {
            AeonAgent.ActionResult(false, lastError)
        }
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
