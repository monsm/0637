package ai.duck.microduck

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.net.Uri
import android.provider.MediaStore
import androidx.core.content.FileProvider
import java.io.File

/**
 * The Director photo, off the GL thread's hands: composite the read-back duck
 * over the camera still, save it to the gallery, or hand the file to the
 * system chooser. No frame here enters the duck's senses.
 */
fun compositePhoto(room: Bitmap?, duck: Bitmap): Bitmap {
    val width = duck.width
    val height = duck.height
    if (width <= 0 || height <= 0) return duck
    val out = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(out)
    if (room != null) {
        // Aspect-fill the room into the frame the user saw, then the duck at
        // the same framing. The viewfinder is a screen-space composite; so is
        // this.
        val scale = maxOf(width.toFloat() / room.width, height.toFloat() / room.height)
        val dw = room.width * scale
        val dh = room.height * scale
        val dst = RectF((width - dw) / 2f, (height - dh) / 2f, (width + dw) / 2f, (height + dh) / 2f)
        canvas.drawBitmap(room, null, dst, Paint(Paint.FILTER_BITMAP_FLAG))
    } else {
        // In Director the GL frame is transparent when no camera image came
        // back. Paint the studio behind it instead of saving an empty PNG.
        canvas.drawColor(android.graphics.Color.rgb(14, 19, 26))
        val paint = Paint()
        paint.shader = android.graphics.LinearGradient(
            0f, 0f, 0f, height.toFloat(),
            android.graphics.Color.rgb(14, 19, 26),
            android.graphics.Color.rgb(23, 28, 36),
            android.graphics.Shader.TileMode.CLAMP,
        )
        canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), paint)
    }
    canvas.drawBitmap(duck, 0f, 0f, null)
    return out
}

/** The PNG file the shell keeps in cache and shares. */
fun photoFile(context: Context, bitmap: Bitmap): File {
    val directory = File(context.cacheDir, "photos").apply { mkdirs() }
    val file = File(directory, "MicroDuck-${System.currentTimeMillis()}.png")
    file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    return file
}

/**
 * Save into the system gallery. API 29+ scoped storage needs no permission
 * and MediaStore owns the file; a failure returns false rather than claiming
 * a save that did not happen.
 */
fun savePhoto(context: Context, bitmap: Bitmap): Boolean {
    return try {
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, "MicroDuck-${System.currentTimeMillis()}.png")
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/MicroDuck")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
            ?: return false
        resolver.openOutputStream(uri)?.use { stream ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream)
        } ?: return false
        values.clear()
        values.put(MediaStore.Images.Media.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
        true
    } catch (_: Throwable) {
        false
    }
}

/** Hand an already-kept postcard file to the chooser. */
fun sharePostcard(context: Context, file: File) {
    val uri: Uri = FileProvider.getUriForFile(
        context,
        "${context.packageName}.fileprovider",
        file,
    )
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "image/png"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(
        Intent.createChooser(send, Copy.share)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    )
}

/** Hand the PNG to whatever app the user picks. */
fun sharePhoto(context: Context, bitmap: Bitmap) {
    val file = photoFile(context, bitmap)
    val uri: Uri = FileProvider.getUriForFile(
        context,
        "${context.packageName}.fileprovider",
        file,
    )
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "image/png"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(
        Intent.createChooser(send, Copy.share)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    )
}
