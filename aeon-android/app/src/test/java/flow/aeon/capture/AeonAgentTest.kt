package flow.aeon.capture

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AeonAgentTest {
    @Test
    fun normalizeEndpointAddsHttpScheme() {
        assertEquals(
            "http://192.168.1.44:8080",
            AeonAgent.normalizeEndpoint("192.168.1.44:8080")
        )
    }

    @Test
    fun normalizeEndpointTrimsTrailingSlashAndWhitespace() {
        assertEquals(
            "https://aeon.example.com",
            AeonAgent.normalizeEndpoint("  https://aeon.example.com/  ")
        )
    }

    @Test
    fun normalizeEndpointRejectsEmptyInput() {
        assertThrows(IllegalArgumentException::class.java) {
            AeonAgent.normalizeEndpoint("   ")
        }
    }
}
