package flow.aeon.capture

import org.junit.Assert.assertEquals
import org.junit.Test

class SmsBridgeTest {
    @Test
    fun incomingSmsRecordConvertsToBridgePayload() {
        val payload = SmsBridge.toPayload(
            SmsBridge.SmsRecord(
                rowId = "42",
                address = "10086",
                body = "Your code is 476291",
                date = 1771000000000,
                type = SmsBridge.TYPE_INBOX
            )
        )

        assertEquals("sms-42", payload.messageId)
        assertEquals("10086", payload.address)
        assertEquals("Your code is 476291", payload.body)
        assertEquals(1771000000000, payload.receivedAt)
        assertEquals(AeonAgent.SmsDirection.Incoming, payload.direction)
    }

    @Test
    fun sentSmsRecordConvertsToOutgoingBridgePayload() {
        val payload = SmsBridge.toPayload(
            SmsBridge.SmsRecord(
                rowId = "43",
                address = "13800138000",
                body = "see you at 3",
                date = 1771000000100,
                type = SmsBridge.TYPE_SENT
            )
        )

        assertEquals(AeonAgent.SmsDirection.Outgoing, payload.direction)
    }
}
