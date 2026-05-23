package flow.aeon.capture

import android.Manifest
import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.net.Uri
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import java.util.LinkedHashSet

class SmsWatcherService : Service() {
    private var observer: ContentObserver? = null
    private val capturedRows = LinkedHashSet<String>()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (checkSelfPermission(Manifest.permission.READ_SMS) != PackageManager.PERMISSION_GRANTED) {
            Log.w("AEON", "SMS bridge cannot start without READ_SMS")
            stopSelf()
            return START_NOT_STICKY
        }

        Thread {
            AeonAgent.hello(this@SmsWatcherService)
            syncRecentSms(limit = 20)
        }.start()

        if (observer == null) {
            observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
                override fun onChange(selfChange: Boolean, uri: Uri?) {
                    Thread {
                        if (uri == null) {
                            syncRecentSms(limit = 5)
                        } else {
                            syncSmsUri(uri)
                        }
                    }.start()
                }
            }
            contentResolver.registerContentObserver(SMS_URI, true, observer!!)
        }
        return START_STICKY
    }

    override fun onDestroy() {
        observer?.let { contentResolver.unregisterContentObserver(it) }
        observer = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun syncSmsUri(uri: Uri) {
        val records = readSms(uri, limit = 1)
        if (records.isEmpty()) {
            syncRecentSms(limit = 5)
            return
        }
        records.forEach(::capture)
    }

    private fun syncRecentSms(limit: Int) {
        readSms(SMS_URI, limit).forEach(::capture)
    }

    private fun capture(record: SmsBridge.SmsRecord) {
        if (!capturedRows.add(record.rowId)) {
            return
        }
        val result = AeonAgent.captureSmsResult(this, SmsBridge.toPayload(record))
        if (!result.ok) {
            Log.w("AEON", "SMS bridge capture failed: ${result.message}")
        }
        if (capturedRows.size > 500) {
            val keep = capturedRows.toList().takeLast(250)
            capturedRows.clear()
            capturedRows.addAll(keep)
        }
    }

    private fun readSms(uri: Uri, limit: Int): List<SmsBridge.SmsRecord> {
        val records = ArrayList<SmsBridge.SmsRecord>()
        val cursor = try {
            contentResolver.query(uri, SMS_COLUMNS, null, null, "date DESC")
        } catch (error: SecurityException) {
            Log.w("AEON", "SMS bridge lost READ_SMS permission", error)
            null
        } ?: return records

        cursor.use {
            val idIndex = it.getColumnIndex("_id")
            val addressIndex = it.getColumnIndex("address")
            val bodyIndex = it.getColumnIndex("body")
            val dateIndex = it.getColumnIndex("date")
            val typeIndex = it.getColumnIndex("type")
            if (idIndex < 0 || bodyIndex < 0) {
                return records
            }

            while (records.size < limit && it.moveToNext()) {
                val body = it.getString(bodyIndex).orEmpty()
                if (body.isBlank()) {
                    continue
                }
                records += SmsBridge.SmsRecord(
                    rowId = it.getString(idIndex).orEmpty(),
                    address = if (addressIndex >= 0) it.getString(addressIndex).orEmpty() else "",
                    body = body,
                    date = if (dateIndex >= 0) it.getLong(dateIndex) else System.currentTimeMillis(),
                    type = if (typeIndex >= 0) it.getInt(typeIndex) else SmsBridge.TYPE_INBOX
                )
            }
        }
        return records
    }

    companion object {
        private val SMS_URI: Uri = Uri.parse("content://sms")
        private val SMS_COLUMNS = arrayOf("_id", "address", "body", "date", "type")
    }
}
