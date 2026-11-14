package ai.duck.microduck

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * This phone as the official DKPD pad — the sender half of the protocol in
 * `docs/robot/udp-pad.md`.
 *
 * 28 bytes, little-endian, thirty times a second, exactly the packet a
 * laptop or a pad sends the robot: magic "DKPD", version 1, a wrapping seq,
 * the sender's own clock, six axes and a button word. The other phone's
 * core receives it on UDP 4210 through `pad.rs` — and that receiver is only
 * open when both people typed the same pairing code, so this sender is a
 * paired-neighbour tool, never an open door.
 *
 * The stick mapping is the one the receiver undoes: ly is forward (+vx),
 * lx is left (+yaw), raw values — the deadzone belongs to the receiver.
 */
class PadSender(private val input: DuckInput) {
    @Volatile private var socket: DatagramSocket? = null
    @Volatile private var target: InetAddress? = null
    @Volatile private var targetPort: Int = 4210
    @Volatile private var running = false
    private var thread: Thread? = null
    private var seq: Int = 0

    /** Dial one paired neighbour. Returns false when the address is junk. */
    fun start(addr: String): Boolean {
        val host = addr.substringBeforeLast(':', addr)
        val port = addr.substringAfterLast(':', "8766").toIntOrNull() ?: 8766
        val address = runCatching { InetAddress.getByName(host) }.getOrNull() ?: return false
        stop()
        val sock = runCatching { DatagramSocket() }.getOrNull() ?: return false
        socket = sock
        // The peer announced its LAN socket (8766); the pad listens on 4210.
        val destination = address
        targetPort = 4210
        running = true
        thread = Thread {
            val buffer = ByteBuffer.allocate(28).order(ByteOrder.LITTLE_ENDIAN)
            while (running) {
                buffer.clear()
                buffer.put(byteArrayOf('D'.code.toByte(), 'K'.code.toByte(), 'P'.code.toByte(), 'D'.code.toByte()))
                buffer.put(1) // version
                buffer.put(0) // flags
                buffer.putInt(seq)
                buffer.putInt((System.currentTimeMillis() and 0xFFFFFFFFL).toInt())
                // lx, ly, rx, ry, lt, rt as i16.
                buffer.putShort((-input.wz.coerceIn(-1.0, 1.0) * 32767).toInt().toShort())
                buffer.putShort((input.vx.coerceIn(-0.4, 0.4) / 0.4 * 32767).toInt().toShort())
                buffer.putShort(0)
                buffer.putShort(0)
                buffer.putShort(0)
                buffer.putShort(0)
                buffer.putShort(0) // buttons: the receiver keeps the verbs the shells own
                seq += 1
                val packet = DatagramPacket(buffer.array(), 28, destination, targetPort)
                runCatching { sock.send(packet) }
                try {
                    Thread.sleep(33)
                } catch (_: InterruptedException) {
                    break
                }
            }
        }.apply { isDaemon = true }
        thread?.start()
        return true
    }

    fun stop() {
        running = false
        thread?.interrupt()
        thread = null
        try {
            socket?.close()
        } catch (_: Throwable) {
        }
        socket = null
        target = null
    }

    val active: Boolean get() = running
}
