package nu.necessary.markdawn.demo

import android.app.Application
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.svg.SvgDecoder

/**
 * Registers an [ImageLoader] with Coil's [SvgDecoder] as the process singleton,
 * so the library's `AsyncImage` can render the font-outlined SVG that markdawn
 * emits for math (and, later, diagrams).
 */
class DemoApp : Application(), SingletonImageLoader.Factory {
    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .components { add(SvgDecoder.Factory()) }
            .build()
}
