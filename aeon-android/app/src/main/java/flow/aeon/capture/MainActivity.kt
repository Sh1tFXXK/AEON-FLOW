package flow.aeon.capture

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast

class MainActivity : Activity() {
    private companion object {
        const val REQUEST_PICK_CAPTURE_FILE = 2001
    }

    private val heartbeatHandler = Handler(Looper.getMainLooper())
    private lateinit var endpoint: EditText
    private lateinit var status: TextView
    private lateinit var content: LinearLayout
    private var activeTab = "stream"

    private val heartbeat = object : Runnable {
        override fun run() {
            Thread { AeonAgent.hello(this@MainActivity) }.start()
            heartbeatHandler.postDelayed(this, 30_000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        buildUi()
        announceDevice()
        showStream()
    }

    override fun onResume() {
        super.onResume()
        heartbeatHandler.removeCallbacks(heartbeat)
        heartbeatHandler.postDelayed(heartbeat, 5_000)
    }

    override fun onPause() {
        heartbeatHandler.removeCallbacks(heartbeat)
        super.onPause()
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_PICK_CAPTURE_FILE || resultCode != RESULT_OK) {
            return
        }
        val uri = data?.data ?: return
        val flags = data.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION
        if (flags != 0) {
            runCatching { contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        }
        setStatus("Capturing shared file...")
        Thread {
            val result = AeonAgent.captureUriResult(this, uri)
            runOnUiThread {
                toast(result.message)
                showStream()
            }
        }.start()
    }

    private fun buildUi() {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 18)
            setBackgroundColor(Color.rgb(246, 247, 245))
        }

        root.addView(TextView(this).apply {
            text = "AEON Capture"
            textSize = 24f
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            setTextColor(Color.rgb(22, 25, 21))
        })

        endpoint = EditText(this).apply {
            hint = "AEON auto discovery, or http://desktop-ip:8080"
            setText(AeonAgent.endpoint(this@MainActivity))
            isSingleLine = true
            inputType = InputType.TYPE_TEXT_VARIATION_URI
        }
        root.addView(endpoint)

        status = TextView(this).apply {
            text = "Connecting..."
            textSize = 13f
            setTextColor(Color.rgb(86, 91, 84))
            setPadding(0, 0, 0, 8)
        }
        root.addView(status)

        root.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(tabButton("Stream", "stream"))
            addView(tabButton("Processes", "processes"))
            addView(tabButton("Devices", "devices"))
            addView(tabButton("Send", "send"))
        })

        val scroll = ScrollView(this)
        content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, 12, 0, 0)
        }
        scroll.addView(content)
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        setContentView(root)
    }

    private fun tabButton(label: String, tab: String): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            setOnClickListener {
                activeTab = tab
                when (tab) {
                    "stream" -> showStream()
                    "processes" -> showProcesses()
                    "devices" -> showDevices()
                    "send" -> showSend()
                }
            }
        }

    private fun showStream() {
        activeTab = "stream"
        setLoading("Loading capture stream...")
        Thread {
            val entries = runCatching { AeonAgent.entries(this) }.getOrElse { emptyList() }
            runOnUiThread {
                content.removeAllViews()
                header("Capture Stream")
                content.addView(actionButton("Refresh") { showStream() })
                if (entries.isEmpty()) {
                    muted("No captures yet")
                    return@runOnUiThread
                }
                entries.take(80).forEach { entry ->
                    card {
                        addView(title(entry.title))
                        addView(mutedView("${entry.kindLabel} · ${entry.sourceLabel} · ${entry.size} B"))
                        if (entry.summary.isNotBlank()) {
                            addView(body(entry.summary.take(220)))
                        }
                        addView(row {
                            addView(smallButton("Open") { showEntry(entry.cid) })
                            if (entry.editable) addView(smallButton("Edit") { showEntry(entry.cid, edit = true) })
                        })
                    }
                }
            }
        }.start()
    }

    private fun showEntry(cid: String, edit: Boolean = false) {
        setLoading("Loading entry...")
        Thread {
            val detail = runCatching { AeonAgent.entry(this, cid) }.getOrNull()
            runOnUiThread {
                content.removeAllViews()
                if (detail == null) {
                    muted("Entry not found")
                    return@runOnUiThread
                }
                header(detail.title)
                muted("${detail.kindLabel} · ${detail.sourceLabel} · ${detail.mime} · ${detail.size} B")
                if (detail.filePath != null) muted("Path: ${detail.filePath}")
                if (detail.url != null) muted("URL: ${detail.url}")

                val text = detail.text ?: detail.summary
                if (edit && detail.editable && detail.text != null) {
                    val editor = EditText(this).apply {
                        setText(detail.text)
                        minLines = 10
                        gravity = Gravity.TOP
                        inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    }
                    content.addView(editor)
                    content.addView(actionButton("Save as new version") {
                        saveEntry(detail.cid, detail.title, editor.text.toString())
                    })
                } else {
                    content.addView(body(text.ifBlank { "(No preview)" }))
                    content.addView(row {
                        if (detail.editable && detail.text != null) {
                            addView(smallButton("Edit") { showEntry(detail.cid, edit = true) })
                        }
                        addView(smallButton("Raw") { openRaw(detail.rawUrl) })
                        addView(smallButton("Back") { showStream() })
                    })
                }
            }
        }.start()
    }

    private fun saveEntry(cid: String, title: String, text: String) {
        setStatus("Saving...")
        Thread {
            val result = AeonAgent.editEntry(this, cid, title, text)
            runOnUiThread {
                toast(result.message)
                showStream()
            }
        }.start()
    }

    private fun showProcesses() {
        activeTab = "processes"
        setLoading("Loading processes...")
        Thread {
            val processes = runCatching { AeonAgent.processes(this) }.getOrElse { emptyList() }
            runOnUiThread {
                content.removeAllViews()
                header("Process Panel")
                content.addView(actionButton("Refresh") { showProcesses() })
                content.addView(actionButton("Capture full PC state") { captureFullPcState() })
                content.addView(actionButton("Capture process inventory") { captureProcessInventory() })
                if (processes.isEmpty()) {
                    muted("No processes found")
                    return@runOnUiThread
                }
                processes.take(120).forEach { process ->
                    card {
                        addView(title(process.name))
                        addView(mutedView("${process.kindLabel} · PID ${process.pid} · CPU ${"%.1f".format(process.cpuPercent)}% · MEM ${process.memoryMb} MB"))
                        if (process.exe.isNotBlank()) addView(mutedView(process.exe))
                        addView(row {
                            process.actions.take(4).forEach { action ->
                                addView(smallButton("${action.icon} ${action.label}") {
                                    runProcessAction(process.pid, action)
                                })
                            }
                        })
                    }
                }
            }
        }.start()
    }

    private fun runProcessAction(pid: Int, action: AeonAgent.CaptureAction) {
        setStatus("Running ${action.label}...")
        Thread {
            val result = AeonAgent.captureProcess(this, pid, action.id)
            runOnUiThread {
                toast(if (result.ok) "${action.label} done ${result.cid?.take(8) ?: ""}" else result.message)
                showStream()
            }
        }.start()
    }

    private fun showDevices() {
        activeTab = "devices"
        setLoading("Loading devices...")
        Thread {
            val snapshot = runCatching { AeonAgent.status(this) }.getOrNull()
            runOnUiThread {
                content.removeAllViews()
                header("Devices")
                content.addView(actionButton("Refresh") { showDevices() })
                if (snapshot == null) {
                    muted("Cannot load devices")
                    return@runOnUiThread
                }
                muted("Identity: ${snapshot.identityShort}")
                snapshot.connectUrls.forEach { url ->
                    card {
                        addView(title(url.label))
                        addView(body(url.url))
                    }
                }
                snapshot.devices.forEach { device ->
                    card {
                        addView(title(device.name))
                        addView(mutedView("${device.kind} · ${device.id} · ${if (device.online) "online" else "offline"}"))
                    }
                }
            }
        }.start()
    }

    private fun showSend() {
        activeTab = "send"
        content.removeAllViews()
        header("Send / Settings")
        content.addView(actionButton("Auto find AEON") { announceDevice() })
        content.addView(actionButton("Save & test endpoint") { testEndpoint() })
        content.addView(actionButton("Capture file / image / any data") { pickCaptureFile() })
        content.addView(actionButton("Start photo watcher") {
            requestPhotoPermission()
            startService(Intent(this, PhotoWatcherService::class.java))
            toast("Photo watcher started")
        })
        val input = EditText(this).apply {
            hint = "Capture text to AEON"
            minLines = 4
            gravity = Gravity.TOP
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        }
        content.addView(input)
        content.addView(actionButton("Capture text") {
            val text = input.text.toString()
            if (text.isBlank()) {
                toast("Text is empty")
                return@actionButton
            }
            Thread {
                val result = AeonAgent.captureTextResult(this, text)
                runOnUiThread {
                    toast(result.message)
                    if (result.ok) input.setText("")
                }
            }.start()
        })
    }

    private fun pickCaptureFile() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("*/*", "image/*", "video/*", "audio/*", "application/*", "text/*"))
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
        startActivityForResult(intent, REQUEST_PICK_CAPTURE_FILE)
    }

    private fun captureProcessInventory() {
        setStatus("Capturing process inventory...")
        Thread {
            val result = AeonAgent.captureProcesses(this)
            runOnUiThread {
                toast(result.message)
                showStream()
            }
        }.start()
    }

    private fun captureFullPcState() {
        setStatus("Capturing full PC state...")
        Thread {
            val result = AeonAgent.captureAll(this)
            runOnUiThread {
                toast(result.message)
                showStream()
            }
        }.start()
    }

    private fun announceDevice() {
        setStatus("Searching AEON...")
        Thread {
            val result = AeonAgent.discoverEndpoint(this, endpoint.text?.toString())
            runOnUiThread {
                if (result.ok) endpoint.setText(result.endpoint)
                setStatus(result.message)
                if (activeTab == "devices") showDevices()
            }
        }.start()
    }

    private fun testEndpoint() {
        setStatus("Testing endpoint...")
        Thread {
            val result = AeonAgent.saveAndCheckEndpoint(this, endpoint.text.toString())
            runOnUiThread {
                endpoint.setText(result.endpoint)
                setStatus(result.message)
            }
        }.start()
    }

    private fun openRaw(rawUrl: String) {
        val base = AeonAgent.endpoint(this)
        val url = if (rawUrl.startsWith("http")) rawUrl else base + rawUrl
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    }

    private fun requestPhotoPermission() {
        val permission = if (Build.VERSION.SDK_INT >= 33) {
            Manifest.permission.READ_MEDIA_IMAGES
        } else {
            Manifest.permission.READ_EXTERNAL_STORAGE
        }
        requestPermissions(arrayOf(permission), 1001)
    }

    private fun setLoading(message: String) {
        content.removeAllViews()
        muted(message)
    }

    private fun setStatus(message: String) {
        status.text = message
    }

    private fun toast(message: String) {
        setStatus(message)
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    private fun header(text: String) {
        content.addView(TextView(this).apply {
            this.text = text
            textSize = 20f
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            setTextColor(Color.rgb(22, 25, 21))
            setPadding(0, 8, 0, 8)
        })
    }

    private fun title(text: String): TextView =
        TextView(this).apply {
            this.text = text
            textSize = 16f
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            setTextColor(Color.rgb(24, 28, 22))
        }

    private fun body(text: String): TextView =
        TextView(this).apply {
            this.text = text
            textSize = 14f
            setTextColor(Color.rgb(38, 42, 35))
            setPadding(0, 6, 0, 6)
        }

    private fun muted(text: String) {
        content.addView(mutedView(text))
    }

    private fun mutedView(text: String): TextView =
        TextView(this).apply {
            this.text = text
            textSize = 12f
            setTextColor(Color.rgb(92, 97, 88))
            setPadding(0, 3, 0, 3)
        }

    private fun actionButton(text: String, action: () -> Unit): Button =
        Button(this).apply {
            this.text = text
            isAllCaps = false
            setOnClickListener { action() }
        }

    private fun smallButton(text: String, action: () -> Unit): Button =
        actionButton(text, action).apply {
            textSize = 12f
        }

    private fun card(build: LinearLayout.() -> Unit) {
        content.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(16, 14, 16, 14)
            setBackgroundColor(Color.WHITE)
            val params = LinearLayout.LayoutParams(-1, -2)
            params.setMargins(0, 0, 0, 12)
            layoutParams = params
            build()
        })
    }

    private fun row(build: LinearLayout.() -> Unit): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 8, 0, 0)
            build()
        }
}
