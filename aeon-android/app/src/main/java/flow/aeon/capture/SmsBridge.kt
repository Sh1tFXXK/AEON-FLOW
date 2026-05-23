package flow.aeon.capture

object SmsBridge {
    const val TYPE_INBOX = 1
    const val TYPE_SENT = 2

    data class SmsRecord(
        val rowId: String,
        val address: String,
        val body: String,
        val date: Long,
        val type: Int
    )

    fun toPayload(record: SmsRecord): AeonAgent.SmsBridgePayload =
        AeonAgent.SmsBridgePayload(
            messageId = "sms-${record.rowId}",
            address = record.address,
            body = record.body,
            receivedAt = record.date,
            direction = directionForType(record.type)
        )

    fun directionForType(type: Int): AeonAgent.SmsDirection =
        if (type == TYPE_SENT) AeonAgent.SmsDirection.Outgoing else AeonAgent.SmsDirection.Incoming
}
