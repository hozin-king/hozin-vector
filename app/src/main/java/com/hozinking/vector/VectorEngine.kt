package com.hozinking.vector

/** Jembatan JNI ke libvectrace.so (Rust: median-cut + vtracer).
 * trace() mengembalikan JSON {"svg","paths","colors","anchors"} atau
 * null bila gagal — pemanggil menampilkan pesan galat, bukan crash. */
object VectorEngine {
    init {
        System.loadLibrary("vectrace")
    }

    private external fun traceNative(
        rgba: ByteArray,
        width: Int,
        height: Int,
        colors: Int,
        detail: Int,
    ): String?

    fun trace(rgba: ByteArray, width: Int, height: Int, colors: Int, detail: Int): String? =
        try {
            traceNative(rgba, width, height, colors, detail)
        } catch (e: Throwable) {
            null
        }
}
