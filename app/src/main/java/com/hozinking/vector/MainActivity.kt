package com.hozinking.vector

import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.MediaStore
import android.view.View
import android.widget.Toast
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.FileProvider
import com.caverock.androidsvg.SVG
import com.hozinking.vector.databinding.ActivityMainBinding
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors
import kotlin.math.max

class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding
    private val worker = Executors.newSingleThreadExecutor()

    private var sourceBitmap: Bitmap? = null
    private var svgText: String? = null
    private var svgName = "hozin-vector.svg"

    private val pickPhoto = registerForActivityResult(
        ActivityResultContracts.PickVisualMedia()
    ) { uri: Uri? ->
        if (uri == null) return@registerForActivityResult
        loadPhoto(uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        binding.btnPick.setOnClickListener {
            pickPhoto.launch(
                PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
            )
        }
        binding.sliderColors.addOnChangeListener { _, value, _ ->
            binding.lblColors.text = getString(R.string.colors_label, value.toInt())
        }
        binding.sliderDetail.addOnChangeListener { _, value, _ ->
            binding.lblDetail.text = getString(R.string.detail_label, value.toInt())
        }
        binding.btnProcess.setOnClickListener { process() }
        binding.btnCopy.setOnClickListener { copySvg() }
        binding.btnSave.setOnClickListener { saveSvg() }
        binding.btnShare.setOnClickListener { shareSvg() }
        setResultVisible(false)
    }

    private fun loadPhoto(uri: Uri) {
        worker.execute {
            try {
                val bmp = contentResolver.openInputStream(uri)?.use {
                    BitmapFactory.decodeStream(it)
                } ?: throw IllegalStateException("foto tidak bisa dibaca")
                // Batasi sisi panjang 1024 px — cukup untuk trace logo, hemat waktu.
                val scaled = downscale(bmp, 1024)
                runOnUiThread {
                    sourceBitmap = scaled
                    binding.imgSource.setImageBitmap(scaled)
                    binding.imgSource.visibility = View.VISIBLE
                    binding.btnProcess.isEnabled = true
                    binding.txtStatus.text = getString(R.string.status_ready)
                    setResultVisible(false)
                }
            } catch (e: Exception) {
                runOnUiThread {
                    Toast.makeText(this, "Gagal memuat foto: ${e.message}", Toast.LENGTH_LONG).show()
                }
            }
        }
    }

    private fun process() {
        val bmp = sourceBitmap ?: return
        val colors = binding.sliderColors.value.toInt()
        val detail = binding.sliderDetail.value.toInt()
        binding.btnProcess.isEnabled = false
        binding.progress.visibility = View.VISIBLE
        binding.txtStatus.text = getString(R.string.status_processing)
        setResultVisible(false)
        worker.execute {
            try {
                val rgba = bitmapToRgba(bmp)
                val json = VectorEngine.trace(rgba, bmp.width, bmp.height, colors, detail)
                    ?: throw IllegalStateException("mesin trace gagal")
                val obj = JSONObject(json)
                val svg = obj.getString("svg")
                val paths = obj.getInt("paths")
                val colorCount = obj.getInt("colors")
                val anchors = obj.getInt("anchors")
                val preview = renderPreview(svg)
                runOnUiThread {
                    svgText = svg
                    svgName = "hozin-vector-${timestamp()}.svg"
                    binding.progress.visibility = View.GONE
                    binding.btnProcess.isEnabled = true
                    if (preview != null) {
                        binding.imgPreview.setImageBitmap(preview)
                    }
                    binding.txtStats.text = getString(
                        R.string.stats_format, colorCount, anchors, paths,
                        formatKb(svg.toByteArray(Charsets.UTF_8).size),
                    )
                    binding.txtStatus.text = getString(R.string.status_done)
                    setResultVisible(true)
                }
            } catch (e: Exception) {
                runOnUiThread {
                    binding.progress.visibility = View.GONE
                    binding.btnProcess.isEnabled = true
                    binding.txtStatus.text = getString(R.string.status_failed, e.message ?: "?")
                }
            }
        }
    }

    private fun copySvg() {
        val svg = svgText ?: return
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText("svg", svg))
        Toast.makeText(this, R.string.copied, Toast.LENGTH_SHORT).show()
    }

    private fun saveSvg() {
        val svg = svgText ?: return
        worker.execute {
            val ok = saveToPictures(svg, svgName)
            runOnUiThread {
                Toast.makeText(
                    this,
                    if (ok) getString(R.string.saved, svgName) else getString(R.string.save_failed),
                    Toast.LENGTH_LONG,
                ).show()
            }
        }
    }

    private fun shareSvg() {
        val svg = svgText ?: return
        worker.execute {
            try {
                val file = File(cacheDir, svgName)
                FileOutputStream(file).use { it.write(svg.toByteArray(Charsets.UTF_8)) }
                val uri = FileProvider.getUriForFile(
                    this, "${packageName}.fileprovider", file
                )
                val intent = Intent(Intent.ACTION_SEND).apply {
                    type = "image/svg+xml"
                    putExtra(Intent.EXTRA_STREAM, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                runOnUiThread {
                    startActivity(Intent.createChooser(intent, getString(R.string.share_title)))
                }
            } catch (e: Exception) {
                runOnUiThread {
                    Toast.makeText(this, "Gagal membagikan: ${e.message}", Toast.LENGTH_LONG).show()
                }
            }
        }
    }

    /** Simpan ke folder Pictures/Svg Tools (MediaStore, API 29+).
     * Di bawah itu simpan ke folder Svg Tools di Pictures milik aplikasi
     * (tanpa izin tambahan). Nama file sudah unik per waktu, jadi tidak
     * pernah menimpa file lama. */
    private fun saveToPictures(svg: String, name: String): Boolean {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.Images.Media.DISPLAY_NAME, name)
                    put(MediaStore.Images.Media.MIME_TYPE, "image/svg+xml")
                    put(
                        MediaStore.Images.Media.RELATIVE_PATH,
                        "${Environment.DIRECTORY_PICTURES}/Svg Tools",
                    )
                }
                val uri = contentResolver.insert(
                    MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values
                ) ?: return false
                contentResolver.openOutputStream(uri)?.use {
                    it.write(svg.toByteArray(Charsets.UTF_8))
                } ?: return false
                true
            } else {
                val dir = File(
                    getExternalFilesDir(Environment.DIRECTORY_PICTURES),
                    "Svg Tools",
                ).apply { mkdirs() }
                File(dir, name).writeText(svg)
                true
            }
        } catch (e: Exception) {
            false
        }
    }

    /** Render SVG ke bitmap untuk preview (tanpa WebView). Gagal render
     * bukan fatal — SVG tetap bisa disalin/disimpan/dibagikan. */
    private fun renderPreview(svg: String): Bitmap? {
        return try {
            val parsed = SVG.getFromString(svg)
            val w = parsed.documentWidth.toInt().coerceAtLeast(1)
            val h = parsed.documentHeight.toInt().coerceAtLeast(1)
            val scale = 1024f / max(w, h)
            val bmp = Bitmap.createBitmap(
                (w * scale).toInt().coerceAtLeast(1),
                (h * scale).toInt().coerceAtLeast(1),
                Bitmap.Config.ARGB_8888,
            )
            val canvas = Canvas(bmp)
            canvas.drawColor(Color.WHITE)
            canvas.scale(scale, scale)
            parsed.renderToCanvas(canvas)
            bmp
        } catch (e: Exception) {
            null
        }
    }

    private fun setResultVisible(visible: Boolean) {
        val v = if (visible) View.VISIBLE else View.GONE
        binding.lblPreview.visibility = v
        binding.imgPreview.visibility = v
        binding.txtStats.visibility = v
        binding.btnCopy.visibility = v
        binding.btnSave.visibility = v
        binding.btnShare.visibility = v
    }

    private fun downscale(bmp: Bitmap, maxSide: Int): Bitmap {
        val longest = max(bmp.width, bmp.height)
        if (longest <= maxSide) return bmp
        val scale = maxSide.toFloat() / longest
        return Bitmap.createScaledBitmap(
            bmp, (bmp.width * scale).toInt(), (bmp.height * scale).toInt(), true
        )
    }

    private fun bitmapToRgba(bmp: Bitmap): ByteArray {
        val argb = IntArray(bmp.width * bmp.height)
        bmp.getPixels(argb, 0, bmp.width, 0, 0, bmp.width, bmp.height)
        val out = ByteArray(argb.size * 4)
        for (i in argb.indices) {
            val p = argb[i]
            out[i * 4] = ((p shr 16) and 0xFF).toByte()
            out[i * 4 + 1] = ((p shr 8) and 0xFF).toByte()
            out[i * 4 + 2] = (p and 0xFF).toByte()
            out[i * 4 + 3] = ((p ushr 24) and 0xFF).toByte()
        }
        return out
    }

    private fun timestamp(): String =
        SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())

    private fun formatKb(bytes: Int): String =
        if (bytes < 1024) "$bytes B" else "%.1f KB".format(bytes / 1024f)

    override fun onDestroy() {
        worker.shutdown()
        super.onDestroy()
    }
}
