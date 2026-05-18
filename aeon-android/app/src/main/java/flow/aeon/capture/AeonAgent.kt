package flow.aeon.capture

import android.content.Context
import android.net.wifi.WifiManager
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileInputStream
import java.io.InputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.SocketTimeoutException
import java.net.URL
import java.util.LinkedHashSet

object AeonAgent {
    private const val PREFS = "aeon"
    private const val ENDPOINT = "endpoint"
    private const val DEFAULT_ENDPOINT = "http://127.0.0.1:8080"
    private const val DISCOVERY_PORT = 8091
    private const val DISCOVERY_MESSAGE = "AEON_DISCOVER_V1"
    private const val API_READ_LIMIT = 4 * 1024 * 1024

    fun endpoint(context: Context): String {
        val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(ENDPOINT, DEFAULT_ENDPOINT)
            ?: DEFAULT_ENDPOINT
        return try {
            normalizeEndpoint(saved)
        } catch (_: Exception) {
            saved
        }
    }

    data class EndpointCheck(
        val ok: Boolean,
        val endpoint: String,
        val message: String
    )

    data class PostResult(
        val ok: Boolean,
        val statusCode: Int? = null,
        val error: String? = null
    )

    data class ProbeResult(
        val ok: Boolean,
        val statusCode: Int? = null,
        val error: String? = null,
        val body: String? = null
    )

    data class DeviceSummary(
        val id: String,
        val name: String,
        val kind: String,
        val online: Boolean,
        val isLocal: Boolean,
        val lastSeenMs: Long
    )

    data class ConnectUrl(
        val label: String,
        val url: String,
        val kind: String,
        val remote: Boolean
    )

    data class StatusSnapshot(
        val identityShort: String,
        val devices: List<DeviceSummary>,
        val connectUrls: List<ConnectUrl>
    )

    data class CaptureSummary(
        val cid: String,
        val kind: String,
        val kindLabel: String,
        val title: String,
        val summary: String,
        val sourceLabel: String,
        val capturedAt: Long,
        val size: Long,
        val editable: Boolean
    )

    data class EntryDetail(
        val cid: String,
        val kind: String,
        val kindLabel: String,
        val title: String,
        val summary: String,
        val text: String?,
        val sourceLabel: String,
        val filePath: String?,
        val url: String?,
        val size: Long,
        val mime: String,
        val editable: Boolean,
        val rawUrl: String
    )

    data class CaptureAction(
        val id: String,
        val label: String,
        val description: String,
        val icon: String,
        val kind: String
    )

    data class ProcessSummary(
        val pid: Int,
        val name: String,
        val exe: String,
        val cpuPercent: Double,
        val memoryMb: Long,
        val status: String,
        val kindLabel: String,
        val actions: List<CaptureAction>
    )

    data class ActionResult(
        val ok: Boolean,
        val message: String,
        val cid: String? = null
    )

    enum class SmsDirection {
        Incoming,
        Outgoing
    }

    data class SmsBridgePayload(
        val messageId: String,
        val address: String,
        val body: String,
        val receivedAt: Long,
        val direction: SmsDirection
    )

    fun normalizeEndpoint(endpoint: String): String {
        val trimmed = endpoint.trim().trimEnd('/')
        require(trimmed.isNotEmpty()) { "Endpoint is empty" }

        val withScheme = if (trimmed.contains("://")) {
            trimmed
        } else {
            "http://$trimmed"
        }
        val parsed = URL(withScheme)
        require(parsed.protocol == "http" || parsed.protocol == "https") {
            "Endpoint must start with http:// or https://"
        }
        require(!parsed.host.isNullOrBlank()) { "Endpoint host is empty" }
        return withScheme.trimEnd('/')
    }

    fun setEndpoint(context: Context, endpoint: String): String {
        val normalized = normalizeEndpoint(endpoint)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(ENDPOINT, normalized)
            .apply()
        return normalized
    }

    fun saveAndCheckEndpoint(context: Context, rawEndpoint: String): EndpointCheck {
        val normalized = try {
            setEndpoint(context, rawEndpoint)
        } catch (error: Exception) {
            return EndpointCheck(false, rawEndpoint.trim(), error.message ?: "Invalid endpoint")
        }
        return checkEndpoint(context, normalized)
    }

    fun checkEndpoint(context: Context, endpoint: String = endpoint(context)): EndpointCheck {
        val normalized = try {
            normalizeEndpoint(endpoint)
        } catch (error: Exception) {
            return EndpointCheck(false, endpoint, error.message ?: "Invalid endpoint")
        }
        val probe = probeEndpoint(normalized)
        if (!probe.ok) {
            val detail = probe.statusCode?.let { "HTTP $it" } ?: probe.error ?: "Request failed"
            return EndpointCheck(false, normalized, "Cannot connect: $detail")
        }

        val hello = postBytesResult(
            context,
            "$normalized/api/devices/hello",
            "application/json; charset=utf-8",
            helloPayload(context)
        )
        return if (hello.ok) {
            EndpointCheck(true, normalized, "Connected to AEON: $normalized")
        } else {
            val detail = hello.statusCode?.let { "HTTP $it" } ?: hello.error ?: "Request failed"
            EndpointCheck(false, normalized, "AEON replied, but hello failed: $detail")
        }
    }

    fun discoverEndpoint(context: Context, manual: String? = null): EndpointCheck {
        val errors = mutableListOf<String>()
        val candidates = LinkedHashSet<String>()
        candidates += discoverLanEndpoints(context)
        candidates += endpointCandidates(context, manual)
        for (candidate in candidates) {
            val checked = checkEndpoint(context, candidate)
            if (checked.ok) {
                setEndpoint(context, checked.endpoint)
                return checked.copy(message = "Auto-connected to AEON: ${checked.endpoint}")
            }
            errors += "${checked.endpoint}: ${checked.message.removePrefix("Cannot connect: ")}"
        }
        val summary = errors.take(5).joinToString("\n")
        return EndpointCheck(
            false,
            manual?.trim().orEmpty(),
            "No AEON endpoint found.\n$summary"
        )
    }

    fun hello(context: Context): Boolean {
        return postBytes(
            context,
            endpoint(context) + "/api/devices/hello",
            "application/json; charset=utf-8",
            helloPayload(context)
        )
    }

    fun captureText(context: Context, text: String, title: String? = null): Boolean =
        captureTextResult(context, text, title).ok

    fun captureSms(context: Context, payload: SmsBridgePayload): Boolean =
        captureSmsResult(context, payload).ok

    fun captureSmsResult(context: Context, payload: SmsBridgePayload): ActionResult {
        ensureEndpoint(context)?.let { return it }

        val body = JSONObject()
            .put("message_id", payload.messageId)
            .put("address", payload.address)
            .put("body", payload.body)
            .put("received_at", payload.receivedAt)
            .put("direction", payload.direction.name)
            .toString()
            .toByteArray(Charsets.UTF_8)

        val result = postBytesWithBody(
            context,
            endpoint(context) + "/api/bridge/sms",
            "application/json; charset=utf-8",
            body
        )
        return actionResultFromBody(result, "SMS captured")
    }

    fun captureTextResult(context: Context, text: String, title: String? = null): ActionResult {
        ensureEndpoint(context)?.let { return it }

        val payload = JSONObject()
            .put("text", text)
            .put("title", title ?: text.lineSequence().firstOrNull()?.take(60))
            .put("source", "Android")
            .toString()
            .toByteArray(Charsets.UTF_8)

        val result = postBytesResult(
            context,
            endpoint(context) + "/api/capture/text",
            "application/json; charset=utf-8",
            payload
        )
        return if (result.ok) {
            ActionResult(true, "Captured to AEON")
        } else {
            ActionResult(false, result.failureMessage("Text capture failed"))
        }
    }

    fun captureUri(context: Context, uri: Uri): Boolean =
        captureUriResult(context, uri).ok

    fun captureUriResult(context: Context, uri: Uri): ActionResult {
        ensureEndpoint(context)?.let { return it }

        val resolver = context.contentResolver
        val mime = resolver.getType(uri) ?: "application/octet-stream"
        val name = fileName(context, uri) ?: "android-share"

        val input = try {
            openSharedInput(context, uri)
        } catch (error: Throwable) {
            Log.e("AEON", "Cannot open shared uri: $uri", error)
            null
        }
        if (input == null) {
            Log.e("AEON", "Cannot open shared uri: $uri")
            return ActionResult(false, "Cannot read shared file")
        }

        val result = input.use {
            postMultipartStream(context, endpoint(context) + "/api/capture/drop", name, mime, it)
        }
        return if (result.ok) {
            ActionResult(true, "Captured to AEON")
        } else {
            ActionResult(false, result.failureMessage("Upload failed"))
        }
    }

    fun status(context: Context): StatusSnapshot? {
        val body = getResult(endpoint(context) + "/api/status").body ?: return null
        val json = JSONObject(body)
        return StatusSnapshot(
            identityShort = json.optString("identity_short"),
            devices = json.optJSONArray("devices").orEmpty().mapObjects { item ->
                DeviceSummary(
                    id = item.optString("id"),
                    name = item.optString("name", "Device"),
                    kind = item.optString("kind", "device"),
                    online = item.optBoolean("online"),
                    isLocal = item.optBoolean("is_local"),
                    lastSeenMs = item.optLong("last_seen_ms")
                )
            },
            connectUrls = json.optJSONArray("connect_urls").orEmpty().mapObjects { item ->
                ConnectUrl(
                    label = item.optString("label"),
                    url = item.optString("url"),
                    kind = item.optString("kind"),
                    remote = item.optBoolean("remote")
                )
            }
        )
    }

    fun entries(context: Context): List<CaptureSummary> {
        val body = getResult(endpoint(context) + "/api/entries").body ?: return emptyList()
        return JSONArray(body).mapObjects { item ->
            CaptureSummary(
                cid = item.optString("cid"),
                kind = item.optString("kind"),
                kindLabel = item.optString("kind_label", item.optString("kind")),
                title = item.optString("title", "Untitled"),
                summary = item.optNullableString("summary").orEmpty(),
                sourceLabel = item.optString("source_label"),
                capturedAt = item.optLong("captured_at"),
                size = item.optLong("size"),
                editable = item.optBoolean("editable")
            )
        }
    }

    fun entry(context: Context, cid: String): EntryDetail? {
        val body = getResult(endpoint(context) + "/api/entry/$cid").body ?: return null
        val item = JSONObject(body)
        return EntryDetail(
            cid = item.optString("cid"),
            kind = item.optString("kind"),
            kindLabel = item.optString("kind_label", item.optString("kind")),
            title = item.optString("title", "Untitled"),
            summary = item.optNullableString("summary").orEmpty(),
            text = item.optNullableString("text"),
            sourceLabel = item.optString("source_label"),
            filePath = item.optNullableString("file_path"),
            url = item.optNullableString("url"),
            size = item.optLong("size"),
            mime = item.optString("mime"),
            editable = item.optBoolean("editable"),
            rawUrl = item.optString("raw_url")
        )
    }

    fun editEntry(context: Context, cid: String, title: String, text: String): ActionResult {
        val payload = JSONObject()
            .put("title", title)
            .put("text", text)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val result = postBytesResult(
            context,
            endpoint(context) + "/api/entry/$cid/edit",
            "application/json; charset=utf-8",
            payload
        )
        if (!result.ok) {
            return ActionResult(false, result.error ?: result.statusCode?.let { "HTTP $it" } ?: "Edit failed")
        }
        return ActionResult(true, "Saved as new version")
    }

    fun processes(context: Context): List<ProcessSummary> {
        val body = getResult(endpoint(context) + "/api/processes").body ?: return emptyList()
        return JSONArray(body).mapObjects { item ->
            ProcessSummary(
                pid = item.optInt("pid"),
                name = item.optString("name", "process"),
                exe = item.optString("exe"),
                cpuPercent = item.optDouble("cpu_percent"),
                memoryMb = item.optLong("memory_mb"),
                status = item.optString("status"),
                kindLabel = processKindLabel(item.opt("kind")),
                actions = item.optJSONArray("capture_options").orEmpty().mapObjects { action ->
                    CaptureAction(
                        id = action.optString("id"),
                        label = action.optString("label"),
                        description = action.optString("description"),
                        icon = action.optString("icon"),
                        kind = action.optString("kind")
                    )
                }
            )
        }
    }

    fun captureProcess(context: Context, pid: Int, optionId: String): ActionResult {
        val payload = JSONObject()
            .put("pid", pid)
            .put("option_id", optionId)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val result = postBytesWithBody(
            context,
            endpoint(context) + "/api/capture-process",
            "application/json; charset=utf-8",
            payload
        )
        val json = result.body?.let { runCatching { JSONObject(it) }.getOrNull() }
        if (json != null) {
            return ActionResult(
                ok = json.optBoolean("ok"),
                message = json.optString("message", json.optString("error", "Done")),
                cid = json.optString("cid").takeIf { it.isNotBlank() }
            )
        }
        return ActionResult(false, result.error ?: result.statusCode?.let { "HTTP $it" } ?: "Request failed")
    }

    fun captureProcesses(context: Context): ActionResult {
        val result = postBytesWithBody(
            context,
            endpoint(context) + "/api/capture/processes",
            "application/json; charset=utf-8",
            "{}".toByteArray(Charsets.UTF_8)
        )
        return actionResultFromBody(result, "Process inventory captured")
    }

    fun captureAll(context: Context): ActionResult {
        val result = postBytesWithBody(
            context,
            endpoint(context) + "/api/capture/all",
            "application/json; charset=utf-8",
            "{}".toByteArray(Charsets.UTF_8)
        )
        return actionResultFromBody(result, "Full state captured")
    }

    private fun helloPayload(context: Context): ByteArray =
        JSONObject()
            .put("id", deviceId(context))
            .put("name", deviceName())
            .put("kind", "android")
            .toString()
            .toByteArray(Charsets.UTF_8)

    private fun postBytes(context: Context, url: String, contentType: String, data: ByteArray): Boolean =
        postBytesResult(context, url, contentType, data).ok

    private fun postBytesResult(context: Context, url: String, contentType: String, data: ByteArray): PostResult {
        val result = postBytesWithBody(context, url, contentType, data)
        return PostResult(result.ok, result.statusCode, result.error ?: result.body)
    }

    private fun postBytesWithBody(context: Context, url: String, contentType: String, data: ByteArray): ProbeResult {
        var connection: HttpURLConnection? = null
        return try {
            connection = URL(url).openConnection() as HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 3000
            connection.readTimeout = 5000
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", contentType)
            applyAeonHeaders(context, connection)
            connection.outputStream.use { it.write(data) }
            val status = connection.responseCode
            val body = if (status in 200..299) {
                connection.inputStream.bufferedReader().use { it.readText().take(2000) }
            } else {
                connection.errorStream?.bufferedReader()?.use { it.readText().take(2000) }
            }
            ProbeResult(status in 200..299, statusCode = status, body = body)
        } catch (error: Exception) {
            ProbeResult(false, error = error.message ?: error.javaClass.simpleName)
        } finally {
            connection?.disconnect()
        }
    }

    private fun actionResultFromBody(result: ProbeResult, fallback: String): ActionResult {
        val json = result.body?.let { runCatching { JSONObject(it) }.getOrNull() }
        if (json != null) {
            return ActionResult(
                ok = json.optBoolean("ok", result.ok),
                message = json.optString("message", json.optString("error", fallback)),
                cid = json.optString("cid").takeIf { it.isNotBlank() }
                    ?: json.optJSONArray("captured")?.optString(0)?.takeIf { it.isNotBlank() }
            )
        }
        return ActionResult(false, result.error ?: result.statusCode?.let { "HTTP $it" } ?: "Request failed")
    }

    private fun getResult(url: String): ProbeResult {
        var connection: HttpURLConnection? = null
        return try {
            connection = URL(url).openConnection() as HttpURLConnection
            connection.requestMethod = "GET"
            connection.connectTimeout = 1200
            connection.readTimeout = 2000
            val status = connection.responseCode
            val body = if (status in 200..299) {
                connection.inputStream.bufferedReader().use { it.readTextLimited(API_READ_LIMIT) }
            } else {
                connection.errorStream?.bufferedReader()?.use { it.readTextLimited(4096) }
            }
            ProbeResult(status in 200..299, statusCode = status, body = body)
        } catch (error: Exception) {
            ProbeResult(false, error = error.message ?: error.javaClass.simpleName)
        } finally {
            connection?.disconnect()
        }
    }

    private fun probeEndpoint(endpoint: String): ProbeResult {
        val relay = getResult("$endpoint/api/relay/status")
        if (relay.ok && relay.body?.contains("aeon-relay") == true) {
            return relay
        }
        val desktop = getResult("$endpoint/api/status")
        if (desktop.ok) {
            return desktop
        }
        return if (relay.error != null || relay.statusCode != null) relay else desktop
    }

    private fun endpointCandidates(context: Context, manual: String?): List<String> {
        val candidates = LinkedHashSet<String>()
        fun add(raw: String?) {
            if (raw.isNullOrBlank()) return
            try {
                candidates += normalizeEndpoint(raw)
            } catch (_: Exception) {
            }
        }

        add(manual)
        endpoint(context).takeUnless { isLocalDevEndpoint(it) }?.let(::add)

        subnetPrefix(context)?.let { prefix ->
            add("http://${prefix}.44:8080")
            add("http://${prefix}.1:8080")
            for (host in listOf(2, 10, 20, 30, 40, 50, 100, 101, 107, 200, 254)) {
                add("http://${prefix}.$host:8080")
            }
            add("http://${prefix}.44:8090")
            add("http://${prefix}.1:8090")
            for (host in listOf(2, 10, 20, 30, 40, 50, 100, 101, 107, 200, 254)) {
                add("http://${prefix}.$host:8090")
            }
        }

        add(DEFAULT_ENDPOINT)
        add("http://127.0.0.1:8080")
        add("http://10.0.2.2:8080")
        add("http://127.0.0.1:8090")
        add("http://10.0.2.2:8090")

        return candidates.toList()
    }

    private fun isLocalDevEndpoint(endpoint: String): Boolean =
        try {
            val host = URL(normalizeEndpoint(endpoint)).host
            host == "127.0.0.1" || host == "localhost" || host == "10.0.2.2"
        } catch (_: Exception) {
            false
        }

    private fun discoverLanEndpoints(context: Context): List<String> {
        val endpoints = LinkedHashSet<String>()
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        val lock = wifi?.createMulticastLock("aeon-discovery")?.apply {
            setReferenceCounted(false)
            try {
                acquire()
            } catch (_: Exception) {
            }
        }

        try {
            DatagramSocket().use { socket ->
                socket.broadcast = true
                socket.soTimeout = 700
                val payload = "$DISCOVERY_MESSAGE ${deviceId(context)}".toByteArray(Charsets.UTF_8)
                for (target in discoveryTargets(context)) {
                    try {
                        socket.send(DatagramPacket(payload, payload.size, target, DISCOVERY_PORT))
                    } catch (_: Exception) {
                    }
                }

                val deadline = System.currentTimeMillis() + 2200
                val buffer = ByteArray(4096)
                while (System.currentTimeMillis() < deadline) {
                    val packet = DatagramPacket(buffer, buffer.size)
                    try {
                        socket.receive(packet)
                    } catch (_: SocketTimeoutException) {
                        continue
                    } catch (_: Exception) {
                        break
                    }

                    val text = String(packet.data, packet.offset, packet.length, Charsets.UTF_8)
                    val json = try {
                        JSONObject(text)
                    } catch (_: Exception) {
                        null
                    } ?: continue
                    if (json.optString("kind") != "aeon-discovery") {
                        continue
                    }

                    addEndpoint(endpoints, json.optString("preferred_endpoint"))
                    addEndpoint(endpoints, json.optString("ui_url"))
                    addEndpoint(endpoints, json.optString("relay_url"))
                }
            }
        } finally {
            try {
                if (lock?.isHeld == true) {
                    lock.release()
                }
            } catch (_: Exception) {
            }
        }

        return endpoints.toList()
    }

    private fun discoveryTargets(context: Context): List<InetAddress> {
        val targets = LinkedHashSet<InetAddress>()
        fun add(host: String) {
            try {
                targets += InetAddress.getByName(host)
            } catch (_: Exception) {
            }
        }
        add("255.255.255.255")
        subnetPrefix(context)?.let { prefix -> add("$prefix.255") }
        return targets.toList()
    }

    private fun addEndpoint(endpoints: LinkedHashSet<String>, raw: String?) {
        if (raw.isNullOrBlank()) return
        try {
            endpoints += normalizeEndpoint(raw)
        } catch (_: Exception) {
        }
    }

    private fun subnetPrefix(context: Context): String? {
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
            ?: return null
        val gateway = wifi.dhcpInfo?.gateway ?: return null
        val bytes = byteArrayOf(
            (gateway and 0xff).toByte(),
            (gateway shr 8 and 0xff).toByte(),
            (gateway shr 16 and 0xff).toByte(),
            (gateway shr 24 and 0xff).toByte()
        )
        val host = InetAddress.getByAddress(bytes).hostAddress ?: return null
        val parts = host.split(".")
        if (parts.size != 4) return null
        return parts.take(3).joinToString(".")
    }

    private fun postMultipartStream(
        context: Context,
        url: String,
        name: String,
        mime: String,
        input: InputStream
    ): PostResult {
        val boundary = "aeon-${System.currentTimeMillis()}"
        var connection: HttpURLConnection? = null
        return try {
            connection = URL(url).openConnection() as HttpURLConnection
            connection.requestMethod = "POST"
            connection.connectTimeout = 3000
            connection.readTimeout = 120_000
            connection.doOutput = true
            connection.setChunkedStreamingMode(64 * 1024)
            connection.setRequestProperty("Content-Type", "multipart/form-data; boundary=$boundary")
            applyAeonHeaders(context, connection)
            connection.outputStream.use { output ->
                output.write("--$boundary\r\n".toByteArray())
                output.write(
                    "Content-Disposition: form-data; name=\"file\"; filename=\"${name.safeHeader()}\"\r\n"
                        .toByteArray()
                )
                output.write("Content-Type: $mime\r\n\r\n".toByteArray())
                input.copyTo(output, bufferSize = 64 * 1024)
                output.write("\r\n--$boundary--\r\n".toByteArray())
            }
            val status = connection.responseCode
            if (status !in 200..299) {
                val detail = connection.errorStream
                    ?.bufferedReader()
                    ?.use { it.readTextLimited(4096) }
                    .orEmpty()
                Log.e("AEON", "Multipart upload failed HTTP $status: $detail")
                return PostResult(false, statusCode = status, error = detail.ifBlank { "HTTP $status" })
            }
            PostResult(true, statusCode = status)
        } catch (error: OutOfMemoryError) {
            Log.e("AEON", "Multipart upload ran out of memory", error)
            PostResult(false, error = "Android memory exhausted while uploading")
        } catch (error: Exception) {
            Log.e("AEON", "Multipart upload failed", error)
            PostResult(false, error = error.message ?: error.javaClass.simpleName)
        } finally {
            connection?.disconnect()
        }
    }

    private fun ensureEndpoint(context: Context): ActionResult? {
        if (hello(context)) {
            return null
        }
        val discovered = discoverEndpoint(context)
        return if (discovered.ok) {
            null
        } else {
            ActionResult(false, discovered.message)
        }
    }

    private fun PostResult.failureMessage(fallback: String): String {
        val detail = when {
            statusCode != null && !error.isNullOrBlank() -> "HTTP $statusCode: $error"
            statusCode != null -> "HTTP $statusCode"
            !error.isNullOrBlank() -> error
            else -> fallback
        }
        return detail.take(500)
    }

    private fun openSharedInput(context: Context, uri: Uri): InputStream? {
        if (uri.scheme == "file") {
            val path = uri.path ?: return null
            return FileInputStream(File(path))
        }
        return context.contentResolver.openInputStream(uri)
    }

    private fun applyAeonHeaders(context: Context, connection: HttpURLConnection) {
        connection.setRequestProperty("X-AEON-Device-Id", deviceId(context))
        connection.setRequestProperty("X-AEON-Device-Name", deviceName())
        connection.setRequestProperty("X-AEON-Device-Kind", "android")
    }

    private fun deviceId(context: Context): String {
        val raw = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ANDROID_ID
        ) ?: Build.FINGERPRINT ?: "unknown"
        val safe = raw.replace(Regex("[^A-Za-z0-9._-]"), "-").take(40)
        return "android-$safe"
    }

    private fun deviceName(): String {
        val maker = Build.MANUFACTURER.orEmpty().trim()
        val model = Build.MODEL.orEmpty().trim()
        return listOf(maker, model)
            .filter { it.isNotEmpty() }
            .distinctBy { it.lowercase() }
            .joinToString(" ")
            .ifEmpty { "Android device" }
    }

    private fun fileName(context: Context, uri: Uri): String? {
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) {
                return cursor.getString(index)
            }
        }
        return uri.lastPathSegment
    }

    private fun JSONArray?.orEmpty(): JSONArray = this ?: JSONArray()

    private fun <T> JSONArray.mapObjects(block: (JSONObject) -> T): List<T> {
        val result = ArrayList<T>(length())
        for (i in 0 until length()) {
            optJSONObject(i)?.let { result += block(it) }
        }
        return result
    }

    private fun processKindLabel(kind: Any?): String {
        return when (kind) {
            is String -> kind
            is JSONObject -> when {
                kind.has("KnownApp") -> "Known app"
                kind.has("AeonVM") -> "AEON VM"
                else -> "Process"
            }
            else -> "Process"
        }
    }

    private fun String.safeHeader(): String =
        replace("\\", "_").replace("\"", "_").replace("\r", " ").replace("\n", " ")

    private fun JSONObject.optNullableString(key: String): String? =
        if (isNull(key)) null else optString(key).takeIf { it != "null" }

    private fun java.io.Reader.readTextLimited(limit: Int): String {
        val out = StringBuilder()
        val buffer = CharArray(8192)
        while (out.length < limit) {
            val read = read(buffer, 0, minOf(buffer.size, limit - out.length))
            if (read <= 0) break
            out.append(buffer, 0, read)
        }
        return out.toString()
    }
}
