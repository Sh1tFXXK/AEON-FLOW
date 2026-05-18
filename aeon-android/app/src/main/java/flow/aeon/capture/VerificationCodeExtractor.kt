package flow.aeon.capture

object VerificationCodeExtractor {
    fun extract(text: String): String? {
        val runs = Regex("\\d+").findAll(text).map { it.value }.toList()
        if (runs.isEmpty()) {
            return null
        }

        val lower = text.lowercase()
        val hasCodeCue = text.contains("验证码") ||
            lower.contains("verification code") ||
            lower.contains("verify code") ||
            lower.contains("use code") ||
            lower.contains(" code ")

        if (hasCodeCue) {
            return runs.firstOrNull { it.length in 4..8 }
        }

        val sixDigitRuns = runs.filter { it.length == 6 }
        return sixDigitRuns.singleOrNull()
    }
}
