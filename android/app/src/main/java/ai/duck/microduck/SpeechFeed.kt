package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/** What the voice-command toggle is doing right now. */
enum class SpeechState {
    /** The toggle is off; nothing is listening. */
    Off,
    /** A bounded recognition session is running (or about to restart). */
    Listening,
    /** RECORD_AUDIO was refused; the toggle asks again when tapped. */
    Denied,
    /** No recognition service on this device, or it kept failing. */
    Unavailable,
}

/**
 * Spoken Chinese as a command channel.
 *
 * The core already understands words: `POST /heard {"text": "..."}` maps
 * 翻滚 to `roulade`, 坐下 to `sit`, 开心 to `excited`. This class owns the
 * platform side — a `SpeechRecognizer` configured for `zh-CN` — and POSTs the
 * best final transcript to that endpoint. It is a *command* channel, like the
 * skill ring; nothing here reaches the control loop directly.
 *
 * `SpeechRecognizer` is not continuous. While the toggle is on, every final
 * result (or error) is followed by a short cooldown and another
 * `startListening`; partials are ignored so one utterance is one POST. The
 * recognizer and all of its callbacks live on the main Looper — the API
 * requires it — and only the HTTP POST runs on a worker thread (`duck-heard`).
 *
 * Everything is guarded: a device without a recognition service, a denied
 * microphone, a dead service and a core that is not listening yet all end in
 * a state the sheet can show, never in a crash.
 *
 * [state] and [lastHeard] are Compose state so the instrument sheet reads them
 * directly instead of polling.
 */
class SpeechFeed(private val context: Context) {
    /** What the toggle is doing; written only on the main thread. */
    var state by mutableStateOf(SpeechState.Off)
        private set

    /** The last final transcript posted, or null before the first one. */
    var lastHeard by mutableStateOf<String?>(null)
        private set

    /// Set by the shell: a fresh final transcript, so the AI loop can answer
    /// speech now instead of on its next periodic tick. Main thread.
    var onUtterance: ((String) -> Unit)? = null

    private val handler = Handler(Looper.getMainLooper())
    private val poster = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "duck-heard").apply {
            priority = Thread.NORM_PRIORITY
            isDaemon = true
        }
    }

    @Volatile private var enabled = false
    private var recognizer: SpeechRecognizer? = null
    private var listening = false
    // Consecutive failures that happened before the service was ready. A
    // silent room's ERROR_NO_MATCH is normal and is not counted; a service
    // that never gets ready backs off and eventually gives up.
    private var failures = 0
    private var lastPosted: String? = null
    private var lastPostedAt = 0L

    private val restart = Runnable {
        if (enabled) {
            listening = false
            listen()
        }
    }

    // A session normally ends in onResults or onError. If a service never
    // answers, count it as a failure and try again rather than leaving the
    // toggle stuck in "listening" for the rest of the app's life. A session
    // that is in fact still live makes the next startListening return
    // ERROR_RECOGNIZER_BUSY, which the listener retries quietly.
    private val watchdog = Runnable {
        if (!enabled || !listening) return@Runnable
        listening = false
        failures += 1
        if (failures >= MAX_FAILURES) {
            stopWith(SpeechState.Unavailable)
        } else {
            scheduleRestart()
        }
    }

    private val listener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {
            // The service answered; earlier failures no longer predict the next.
            failures = 0
        }

        override fun onBeginningOfSpeech() = Unit

        override fun onRmsChanged(rmsdB: Float) = Unit

        override fun onBufferReceived(buffer: ByteArray?) = Unit

        override fun onEndOfSpeech() = Unit

        override fun onPartialResults(partialResults: Bundle?) {
            // Ignored on purpose: partials would POST the same utterance many
            // times. Only onResults, the final best guess, is sent.
        }

        override fun onEvent(eventType: Int, params: Bundle?) = Unit

        override fun onError(error: Int) {
            listening = false
            handler.removeCallbacks(watchdog)
            if (!enabled) return
            when (error) {
                SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> {
                    stopWith(SpeechState.Denied)
                    return
                }
                SpeechRecognizer.ERROR_CLIENT -> {
                    failures += 1
                    if (failures >= MAX_FAILURES) {
                        stopWith(SpeechState.Unavailable)
                        return
                    }
                }
                // A quiet room is not a fault: retry at the base cadence.
                SpeechRecognizer.ERROR_NO_MATCH,
                SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
                SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> Unit
                else -> failures += 1
            }
            scheduleRestart()
        }

        override fun onResults(results: Bundle?) {
            listening = false
            handler.removeCallbacks(watchdog)
            failures = 0
            val text = results
                ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                ?.firstOrNull()
                ?.trim()
                .orEmpty()
            if (text.isNotEmpty()) {
                lastHeard = text
                post(text)
                onUtterance?.invoke(text)
            }
            scheduleRestart()
        }
    }

    /**
     * Turn the channel on. Asks for nothing by itself: the shell checks and
     * requests RECORD_AUDIO first, then calls this. Safe to call twice.
     */
    fun start() {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            handler.post { start() }
            return
        }
        if (enabled) return
        if (!SpeechRecognizer.isRecognitionAvailable(context)) {
            state = SpeechState.Unavailable
            return
        }
        if (ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.RECORD_AUDIO,
            ) != PackageManager.PERMISSION_GRANTED
        ) {
            state = SpeechState.Denied
            return
        }
        val created = try {
            SpeechRecognizer.createSpeechRecognizer(context)
        } catch (_: Throwable) {
            null
        }
        if (created == null) {
            state = SpeechState.Unavailable
            return
        }
        created.setRecognitionListener(listener)
        recognizer = created
        enabled = true
        listening = false
        failures = 0
        state = SpeechState.Listening
        listen()
    }

    /** Turn the channel off and release the recognizer. */
    fun stop() {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            handler.post { stop() }
            return
        }
        stopWith(SpeechState.Off)
    }

    /** The permission prompt was refused; show it without a recognizer. */
    fun markDenied() {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            handler.post { markDenied() }
            return
        }
        stopWith(SpeechState.Denied)
    }

    private fun listen() {
        if (!enabled) return
        val recognizer = this.recognizer ?: return
        if (listening) return
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(
                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
            )
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Copy.language.tag)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_PREFERENCE, Copy.language.tag)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
        }
        listening = true
        try {
            recognizer.startListening(intent)
            handler.removeCallbacks(watchdog)
            handler.postDelayed(watchdog, SESSION_TIMEOUT_MS)
        } catch (_: Throwable) {
            listening = false
            failures += 1
            if (failures >= MAX_FAILURES) {
                stopWith(SpeechState.Unavailable)
            } else {
                scheduleRestart()
            }
        }
    }

    /** One more bounded session, after a cooldown that grows with failures. */
    private fun scheduleRestart() {
        if (!enabled) return
        handler.removeCallbacks(restart)
        val backoff = BASE_RESTART_MS shl failures.coerceAtMost(3)
        handler.postDelayed(restart, backoff.coerceAtMost(MAX_RESTART_MS))
    }

    private fun stopWith(next: SpeechState) {
        enabled = false
        listening = false
        failures = 0
        handler.removeCallbacks(restart)
        handler.removeCallbacks(watchdog)
        val recognizer = this.recognizer
        this.recognizer = null
        if (recognizer != null) {
            try {
                recognizer.cancel()
            } catch (_: Throwable) {
                // A recognizer that refuses to cancel must not crash the shell.
            }
            // cancel() and destroy() have thrown IllegalStateException on some
            // builds; either failing still leaves the instance released from
            // this class, which is the important part.
            try {
                recognizer.destroy()
            } catch (_: Throwable) {
                // Same for destroy: the state below is the user-visible truth.
            }
        }
        state = next
    }

    /**
     * Send one final transcript to the core. The same text within a short
     * window is dropped: some recognizers deliver a final result more than
     * once for one utterance.
     */
    private fun post(text: String) {
        val now = System.currentTimeMillis()
        if (text == lastPosted && now - lastPostedAt < DEBOUNCE_MS) return
        lastPosted = text
        lastPostedAt = now
        val body = """{"text":"${escape(text)}"}"""
        poster.execute {
            try {
                val connection = URL("http://127.0.0.1:8765/heard")
                    .openConnection() as HttpURLConnection
                connection.requestMethod = "POST"
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.connectTimeout = 2000
                connection.readTimeout = 2000
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                connection.responseCode
                connection.disconnect()
            } catch (_: Throwable) {
                // The core may not be listening yet; the next utterance retries.
            }
        }
    }

    /** Minimal JSON string escaping; transcripts are usually plain Chinese. */
    private fun escape(value: String): String = buildString(value.length + 8) {
        for (character in value) {
            when (character) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> if (character.code < 0x20) {
                    append("\\u%04x".format(character.code))
                } else {
                    append(character)
                }
            }
        }
    }

    private companion object {
        const val BASE_RESTART_MS = 800L
        const val MAX_RESTART_MS = 4000L
        const val MAX_FAILURES = 6
        const val SESSION_TIMEOUT_MS = 15_000L
        const val DEBOUNCE_MS = 2500L
    }
}
