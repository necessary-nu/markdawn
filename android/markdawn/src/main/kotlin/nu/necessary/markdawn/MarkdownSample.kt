package nu.necessary.markdawn

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp

internal const val SAMPLE_MARKDOWN = """
# Markdawn on Compose

A paragraph with **bold**, *italic*, ~~struck~~ and `inline code`, plus a
[link](https://necessary.nu) and inline math ${'$'}e^{i\pi}+1=0${'$'}.

- first item
- second item with **emphasis**
- third

> A blockquote, rendered with an accent bar.

| lang | kind |
| ---- | ---- |
| rust | systems |
| kotlin | jvm |

```rust
fn main() {
    println!("hello");
}
```
"""

@Preview(showBackground = true)
@Composable
private fun MarkdownPreview() {
    MaterialTheme {
        Surface {
            Markdown(
                source = SAMPLE_MARKDOWN.trimIndent(),
                modifier = Modifier
                    .verticalScroll(rememberScrollState())
                    .padding(16.dp),
            )
        }
    }
}
