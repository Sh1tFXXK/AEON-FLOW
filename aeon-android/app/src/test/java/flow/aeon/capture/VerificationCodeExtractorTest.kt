package flow.aeon.capture

import org.junit.Assert.assertEquals
import org.junit.Test

class VerificationCodeExtractorTest {
    @Test
    fun extractsChineseAndEnglishVerificationCodes() {
        assertEquals(
            "476291",
            VerificationCodeExtractor.extract("您的验证码是 476291，5分钟内有效")
        )
        assertEquals(
            "123456",
            VerificationCodeExtractor.extract("verification code: 123456")
        )
        assertEquals(
            "9384",
            VerificationCodeExtractor.extract("Use code 9384 to finish login")
        )
    }

    @Test
    fun ignoresNonCodeNumbers() {
        assertEquals(null, VerificationCodeExtractor.extract("订单 20260518 已发货"))
        assertEquals(null, VerificationCodeExtractor.extract("call me at 13800138000"))
    }
}
