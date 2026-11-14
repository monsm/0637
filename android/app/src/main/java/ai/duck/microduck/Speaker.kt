package ai.duck.microduck

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import java.util.Locale

/**
 * The system voice speaking for the duck — NOT the duck's own synth.
 *
 * The duck's own voice is the core's formant synth (`Voice`), the 1:1
 * channel: the app plays what the physical duck plays. This is the phone's
 * speech synthesizer reading the diary's real facts (or the paragraph the
 * person's own model wrote from them) out loud — a possession-mode channel,
 * and the sheet that offers it says so. Nothing here reaches the control
 * loop; the beak opens through the same client intent a chorus writes on the
 * robot (`POST /mouth`), which the core clamps shut the moment the duck is
 * asleep.
 *
 * The language comes from the caller, captured on the main thread at tap
 * time: [Copy.language] is Compose state and must not be read from this
 * class, whose engine answers init and progress on its own threads. Progress
 * callbacks carry an utterance id and only the CURRENT one is honoured — a
 * cancelled utterance's late onDone must not close the beak under the
 * utterance that replaced it, and a restart (speak over speak) must not flap
 * the mouth closed then open.
 */
class Speaker(
    context: Context,
    private val onSpeakingChanged: (Boolean) -> Unit,
) {
    @Volatile private var tts: TextToSpeech? = null
    @Volatile private var ready = false
    /** The (text, locale) that arrived before the engine finished coming up. */
    @Volatile private var pending: Pair<String, Locale>? = null
    @Volatile private var currentUtterance = ""
    private val main = Handler(Looper.getMainLooper())

    /** Main-thread only: whether an utterance is sounding right now. */
    var speaking = false
        private set

    init {
        tts = TextToSpeech(context.applicationContext) { status ->
            ready = status == TextToSpeech.SUCCESS
            if (!ready) {
                try {
                    tts?.shutdown()
                } catch (_: Throwable) {
                }
                tts = null
            }
            val held = pending
            pending = null
            if (ready && held != null) speak(held.first, held.second)
        }
        tts?.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(utteranceId: String?) = notify(utteranceId, true)
            override fun onDone(utteranceId: String?) = notify(utteranceId, false)
            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String?) = notify(utteranceId, false)
            override fun onError(utteranceId: String?, errorCode: Int) = notify(utteranceId, false)
        })
    }

    /** Speak one text, or hold it (with its locale) until the engine is up.
     *  Cancels what is mid-flight without dropping the flag: a restart keeps
     *  the mouth open instead of closing it for one frame. */
    fun speak(text: String, locale: Locale) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return
        val engine = tts
        if (engine == null || !ready) {
            pending = trimmed to locale
            return
        }
        // Cancel without emitting: the old utterance's callbacks are dead by
        // id, and `speaking` stays true through the hand-over.
        try {
            engine.stop()
        } catch (_: Throwable) {
        }
        val id = "duck-speak-$utteranceCount"
        utteranceCount += 1
        currentUtterance = id
        try {
            engine.language = locale
        } catch (_: Throwable) {
        }
        engine.speak(trimmed, TextToSpeech.QUEUE_ADD, null, id)
        setSpeaking(true)
    }

    /** The button's stop: drop the flag (and the beak) immediately. */
    fun stop() {
        currentUtterance = ""
        try {
            tts?.stop()
        } catch (_: Throwable) {
        }
        setSpeaking(false)
    }

    fun shutdown() {
        currentUtterance = ""
        try {
            tts?.stop()
            tts?.shutdown()
        } catch (_: Throwable) {
        }
        tts = null
        ready = false
        setSpeaking(false)
    }

    private fun notify(utteranceId: String?, value: Boolean) {
        // A stale utterance's events must not fight the one that replaced it.
        if (utteranceId == currentUtterance) setSpeaking(value)
    }

    private fun setSpeaking(value: Boolean) {
        main.post {
            if (speaking != value) {
                speaking = value
                onSpeakingChanged(value)
            }
        }
    }

    private companion object {
        var utteranceCount = 0
    }
}
