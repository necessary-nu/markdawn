package nu.necessary.markdawn

import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier

/**
 * Ordered, keyed store of top-level blocks — the `BlockStore` analog from
 * `web/preact.js`. Apply upsert/delete batches from [VdomParser] / [VdomDocument];
 * because each block is rendered under `key(id)`, Compose recomposes only the
 * blocks whose vtree actually changed, leaving siblings (and their state /
 * scroll position) untouched.
 */
@Stable
class BlockStore {
    private val order = mutableStateListOf<Long>()
    private val blocks = mutableStateMapOf<Long, VNode>()

    fun apply(events: List<VdomEvent>) {
        for (ev in events) when (ev) {
            is Upsert -> {
                if (ev.id !in blocks) order.add(ev.id)
                blocks[ev.id] = ev.vtree
            }
            is Delete -> {
                blocks.remove(ev.id)
                order.remove(ev.id)
            }
        }
    }

    fun clear() {
        order.clear()
        blocks.clear()
    }

    @Composable
    fun Render(modifier: Modifier = Modifier) {
        Column(modifier) {
            for (id in order) {
                key(id) {
                    blocks[id]?.let { VNodeView(it) }
                }
            }
        }
    }
}

/** One-shot static render of a markdown string. */
@Composable
fun Markdown(
    source: String,
    modifier: Modifier = Modifier,
    options: VdomOptions = VdomOptions(),
) {
    val trees = remember(source, options) { Markdawn.parseToVdom(source, options) }
    Column(modifier) {
        trees.forEach { VNodeView(it) }
    }
}
