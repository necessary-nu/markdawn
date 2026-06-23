package nu.necessary.markdawn.demo

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import nu.necessary.markdawn.BlockStore
import nu.necessary.markdawn.Markdown
import nu.necessary.markdawn.VdomDocument
import nu.necessary.markdawn.VdomParser

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Load whatever grammars were bundled (no-op if their .so isn't present).
        nu.necessary.markdawn.Markdawn.loadLanguage("rust")
        setContent { MaterialTheme { DemoScreen() } }
    }
}

private val SAMPLE = """
    # Markdawn ⟶ Compose

    The **same** `VNode` tree that backs the Preact target, rendered natively —
    with a [link](https://necessary.nu) and inline math ${'$'}e^{i\pi}+1=0${'$'}.

    - a bullet
    - [x] a done task
    - [ ] a pending task

    > A blockquote with an accent bar.

    | left | center | right |
    | :--- | :----: | ----: |
    | a    | b      | c     |

    ```rust
    fn main() {
        println!("hello, markdawn");
    }
    ```
""".trimIndent()

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun DemoScreen() {
    var tab by remember { mutableIntStateOf(0) }
    val titles = listOf("Static", "Editor", "Streaming")
    Scaffold(topBar = { TopAppBar(title = { Text("Markdawn") }) }) { inset ->
        Column(Modifier.padding(inset)) {
            TabRow(selectedTabIndex = tab) {
                titles.forEachIndexed { i, t ->
                    Tab(selected = tab == i, onClick = { tab = i }, text = { Text(t) })
                }
            }
            when (tab) {
                0 -> StaticTab()
                1 -> EditorTab()
                else -> StreamingTab()
            }
        }
    }
}

@Composable
private fun StaticTab() {
    Markdown(
        source = SAMPLE,
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
    )
}

@Composable
private fun EditorTab() {
    val doc = remember { VdomDocument() }
    val store = remember { BlockStore() }
    var text by remember { mutableStateOf(SAMPLE) }

    LaunchedEffect(Unit) { store.apply(doc.setSource(text)) }
    DisposableEffect(Unit) { onDispose { doc.close() } }

    Column(Modifier.fillMaxSize().padding(8.dp)) {
        OutlinedTextField(
            value = text,
            onValueChange = {
                text = it
                // Demo simplicity: full re-parse. A real editor would compute a
                // minimal byte range and call `doc.edit(start, end, repl)` so
                // only the touched block re-renders.
                store.apply(doc.setSource(it))
            },
            modifier = Modifier.fillMaxWidth().weight(1f),
            label = { Text("markdown") },
        )
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        Box(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            store.Render()
        }
    }
}

@Composable
private fun StreamingTab() {
    val store = remember { BlockStore() }
    var run by remember { mutableIntStateOf(0) }

    LaunchedEffect(run) {
        store.clear()
        val parser = VdomParser()
        try {
            for (chunk in SAMPLE.chunked(8)) {
                parser.push(chunk)
                store.apply(parser.takeEvents())
                delay(60)
            }
            parser.finish()
            store.apply(parser.takeEvents())
        } finally {
            parser.close()
        }
    }

    Column(Modifier.fillMaxSize()) {
        Button(onClick = { run++ }, modifier = Modifier.padding(8.dp)) {
            Text("Replay stream")
        }
        Box(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            store.Render(Modifier.padding(8.dp))
        }
    }
}
