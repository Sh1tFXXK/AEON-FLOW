package flow.aeon.capture

import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.provider.Settings
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL

object AeonAgent {
    private const val PREFS = "aeon"
    private const val ENDPOINT = "endpoint"
    private const val DEFAULT_ENDPOINT = "http://127.0.0.1:8080"

    fun endpoint(context: Context): String =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(ENDPOINT, DEFAULT_ENDPOINT)
            ?: DEFAULT_ENDPOINT

    fun setEndpoint(context: Context, endpoint: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(ENDPOINT, endpoint.trim().trimEnd('/'))
            .apply()
    }

    fun hello(context: Context): Boolean {
        val payload = JSONObject()
            .put("id", deviceId(context))
            .put("name", deviceName())
            .put("kind", "android")
            .toString()
            .toByteArray(Charsets.UTF_8)

        return postBytes(
            endpoint(context) + "/api/devices/hello",
            "application/json; charset=utf-8",
            payload
        )
    }

    fun captureText(context: Context, text: String, title: String? = null): Boolean {
        hello(context)

        val payload = JSONObject()
            .put("text", text)
            .put("title", title ?: text.lineSequence().firstOrNull()?.take(60))
            .put("source", "Android")
            .toString()
            .toByteArray(Charsets.UTF_8)

        return postBytes(
            endpoint(context) + "/api/capture/text",
            "application/json; charset=utf-8",
            payload
        )
    }

    fun captureUri(context: Context, uri: Uri): Boolean {
        hello(context)

        val resolver = context.contentResolver
        val mime = resolver.getType(uri) ?: "application/octet-stream"
        val name = fileName(context, uri) ?: "android-share"
        val data = resolver.openInputStream(uri)?.use { input ->
            input.readBytes()
        } ?: return false

        return postMultipart(endpoint(context) + "/api/capture/drop", name, mime, data)
    }

    private fun postBytes(url: String, contentType: String, data: ByteArray): Boolean {
        val connection = URL(url).openConnection() as HttpURLConnection
        return try {
            connection.requestMethod = "POST"
            connection.connectTimeout = 3000
            connection.readTimeout = 5000
            connection.doOutput = true
            connection.setRequestProperty("Content-Type", contentType)
            connection.outputStream.use { it.write(data) }
            connection.responseCode in 200..299
        } catch (_: Exception) {
            false
        } finally {
            connection.disconnect()
        }
    }

    private fun postMultipart(url: String, name: String, mime: String, data: ByteArray): Boolean {
        val boundary = "aeon-${System.currentTimeMillis()}"
        val body = ByteArrayOutputStream()
        body.write("--$boundary\r\n".toByteArray())
        body.write("Content-Disposition: form-data; name=\"file\"; filename=\"$name\"\r\n".toByteArray())
        body.write("Content-Type: $mime\r\n\r\n".toByteArray())
        body.write(data)
        body.write("\r\n--$boundary--\r\n".toByteArray())

        return postBytes(url, "multipart/form-data; boundary=$boundary", body.toByteArray())
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
}
