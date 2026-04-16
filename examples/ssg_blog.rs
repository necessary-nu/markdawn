//! Minimal end-to-end SSG example: render a markdown post to an HTML file
//! using the Dioxus adapter and `dioxus-ssr`.
//!
//! Run with:
//! ```text
//! cargo run --example ssg_blog --features dioxus
//! ```

use dioxus::prelude::*;
use markdawn::{DioxusOptions, Markdown};

const POST: &str = r#"# Hello, world

This is a tiny blog post written in **markdown**, rendered to a Dioxus
`Element` and then to a static HTML string.

> [!NOTE]
> Try editing this file and re-running.

## Features

- Lists work
- *Emphasis* and `code spans` too
- [Links](https://example.com)

```rust
fn main() {
    println!("hello");
}
```
"#;

#[component]
fn BlogPost() -> Element {
    rsx! {
        article { class: "post",
            header { h1 { "My Static Blog" } }
            Markdown {
                source: POST.to_string(),
                options: DioxusOptions::all(),
            }
            footer { p { "© 2026" } }
        }
    }
}

fn main() {
    let html = dioxus_ssr::render_element(rsx! { BlogPost {} });
    println!("{html}");
}
