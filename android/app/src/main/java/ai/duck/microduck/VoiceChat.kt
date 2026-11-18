package ai.duck.microduck

import android.content.Context
import android.speech.tts.TextToSpeech
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.URL
import java.util.Locale

/** One line of the chat: the person's sentence, or the duck's answer. */
internal data class ChatLine(val fromDuck: Boolean, val text: String)

/** Everything the voice-chat section shows; the machine below only edits this. */
internal data class ChatState(
    val lines: List<ChatLine> = emptyList(),
    val draft: String = "",
    val sending: Boolean = false,
    val error: String? = null,
)

/** Why a send tap did or did not become a model call. */
internal sealed interface ChatVerdict {
    data class Ready(val prompt: String) : ChatVerdict
    data object Blank : ChatVerdict
    data object NoModel : ChatVerdict
    data object Busy : ChatVerdict
}

/**
 * The voice-chat state machine, pure so the JVM tests can pin the three
 * refusals the UI must honour: empty input, a model that is not configured
 * yet, and a send that is already in flight. Nothing here invents a reply.
 *
 * "Configured" matches the iOS ticket's meaning: the AI switch is on AND the
 * endpoint is a parseable http(s) URL with a host. A half-typed address is
 * not a model.
 */
internal object ChatMachine {
    fun configured(config: AIConfig): Boolean {
        if (!config.enabled) return false
        return usableEndpoint(config.endpoint)
    }

    private fun usableEndpoint(raw: String): Boolean {
        val text = raw.trim()
        if (text.isEmpty()) return false
        return try {
            val url = URL(text)
            (url.protocol == "http" || url.protocol == "https") &&
                !url.host.isNullOrBlank()
        } catch (_: Throwable) {
            false
        }
    }

    fun ready(config: AIConfig, draft: String, sending: Boolean): ChatVerdict {
        if (sending) return ChatVerdict.Busy
        val prompt = draft.trim()
        if (prompt.isEmpty()) return ChatVerdict.Blank
        if (!configured(config)) return ChatVerdict.NoModel
        return ChatVerdict.Ready(prompt)
    }

    /** The person's sentence is on the wire; the draft is spent. */
    fun sent(state: ChatState, prompt: String): ChatState = state.copy(
        lines = state.lines + ChatLine(fromDuck = false, text = prompt),
        draft = "",
        sending = true,
        error = null,
    )

    /** The model's answer, or its error; a failed answer leaves no duck line. */
    fun delivered(state: ChatState, reply: String?, error: String?): ChatState {
        val text = reply?.trim().orEmpty()
        return if (text.isNotEmpty()) {
            state.copy(
                lines = state.lines + ChatLine(fromDuck = true, text = text),
                sending = false,
                error = null,
            )
        } else {
            state.copy(
                sending = false,
                error = error ?: "The model's answer was empty",
            )
        }
    }
}

/**
 * The phone's speech synthesizer for the chat replies. It belongs to the
 * activity (init in onCreate, shutdown in onDestroy), not to the sheet, and a
 * phone with no engine simply stays silent — the bubbles still show the text.
 * The concrete language is chosen by the caller from the shell's language.
 */
internal object ChatVoice {
    @Volatile private var engine: TextToSpeech? = null
    @Volatile private var ready = false
    private var utterance = 0

    fun init(context: Context) {
        if (engine != null) return
        try {
            engine = TextToSpeech(context.applicationContext) { status ->
                ready = status == TextToSpeech.SUCCESS
                if (!ready) {
                    try {
                        engine?.shutdown()
                    } catch (_: Throwable) {
                    }
                    engine = null
                }
            }
        } catch (_: Throwable) {
            engine = null
            ready = false
        }
    }

    /** Speak one reply. No engine, no init, or a failed engine: silence. */
    fun speak(text: String, locale: Locale) {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || !ready) return
        val voice = engine ?: return
        try {
            voice.language = locale
            utterance += 1
            voice.speak(trimmed, TextToSpeech.QUEUE_FLUSH, null, "duck-chat-$utterance")
        } catch (_: Throwable) {
            // A missing language pack or a busy engine is not a crash.
        }
    }

    fun shutdown() {
        ready = false
        try {
            engine?.stop()
            engine?.shutdown()
        } catch (_: Throwable) {
        }
        engine = null
    }
}

/**
 * Voice chat on the AI companion page: type one sentence (or reuse the last
 * words the duck heard), the person's own model answers as plain text, and
 * this phone reads the answer aloud. The core never talks to the network and
 * the audio never leaves the phone.
 */
@Composable
internal fun VoiceChatSection(config: AIConfig) {
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf(ChatState()) }
    val configured = ChatMachine.configured(config)
    val send: () -> Unit = {
        when (val verdict = ChatMachine.ready(config, state.draft, state.sending)) {
            is ChatVerdict.Ready -> {
                state = ChatMachine.sent(state, verdict.prompt)
                // Copy.language is Compose state; read it here, on the main
                // thread, and hand both the prompt language and the TTS
                // locale down explicitly.
                val chinese = Copy.language == Copy.Language.Chinese
                scope.launch {
                    val (reply, error) = withContext(Dispatchers.IO) {
                        AIBrain.chat(
                            config = config,
                            stateJson = fetchRawState() ?: "",
                            heard = fetchLastHeard() ?: "",
                            message = verdict.prompt,
                            chinese = chinese,
                        )
                    }
                    state = ChatMachine.delivered(state, reply, error)
                    if (reply != null) {
                        ChatVoice.speak(
                            reply,
                            if (chinese) Locale.SIMPLIFIED_CHINESE else Locale.US,
                        )
                    }
                }
            }
            ChatVerdict.NoModel, ChatVerdict.Blank, ChatVerdict.Busy -> Unit
        }
    }
    Column(
        Modifier.fillMaxWidth().testTag("chat-section"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                Copy.chatTitle,
                color = Color.White,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f).testTag("chat-title"),
            )
            ButtonChip(Copy.heard, "chat-heard") {
                scope.launch {
                    val heard = withContext(Dispatchers.IO) { fetchLastHeard() }
                        ?.trim().orEmpty()
                    if (heard.isNotEmpty()) state = state.copy(draft = heard)
                }
            }
        }
        Text(
            Copy.chatLocalVoice,
            color = Color.White.copy(alpha = 0.5f),
            style = MaterialTheme.typography.labelSmall,
        )
        state.lines.takeLast(6).forEachIndexed { index, line ->
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = if (line.fromDuck) Arrangement.Start else Arrangement.End,
            ) {
                Text(
                    line.text,
                    color = Color.White,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier
                        .background(
                            if (line.fromDuck) Color.White.copy(alpha = 0.10f)
                            else DuckTheme.AccentAction.copy(alpha = 0.30f),
                            RoundedCornerShape(10.dp),
                        )
                        .padding(horizontal = 10.dp, vertical = 8.dp)
                        .testTag("chat-line-$index"),
                )
            }
        }
        if (state.sending) {
            Text(
                Copy.chatThinking,
                color = Color.White.copy(alpha = 0.6f),
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.testTag("chat-thinking"),
            )
        }
        if (state.error != null) {
            Text(
                state.error.orEmpty(),
                color = DuckTheme.StateWarn,
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.testTag("chat-error"),
            )
        }
        OutlinedTextField(
            value = state.draft,
            onValueChange = { state = state.copy(draft = it.take(500)) },
            placeholder = { Text(Copy.chatHint) },
            enabled = configured && !state.sending,
            singleLine = true,
            modifier = Modifier.fillMaxWidth().testTag("chat-input"),
        )
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            ChatSendChip(
                label = Copy.chatSend,
                enabled = configured && !state.sending && state.draft.isNotBlank(),
                testTag = "chat-send",
                onClick = send,
            )
            if (!configured) {
                Text(
                    Copy.chatNoModel,
                    color = DuckTheme.StateWarn,
                    style = MaterialTheme.typography.labelSmall,
                    modifier = Modifier.testTag("chat-no-model"),
                )
            }
        }
    }
}

/** The send chip with an honest disabled face; the click stays named for
 *  TalkBack so the control is never silently gone. */
@Composable
private fun ChatSendChip(
    label: String,
    enabled: Boolean,
    testTag: String,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .heightIn(min = 48.dp)
            .background(
                Color.White.copy(alpha = if (enabled) 0.10f else 0.05f),
                RoundedCornerShape(10.dp),
            )
            .clickable(enabled = enabled, onClickLabel = label) { onClick() }
            .testTag(testTag)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = Color.White.copy(alpha = if (enabled) 1f else 0.4f),
            style = MaterialTheme.typography.labelLarge,
        )
    }
}
