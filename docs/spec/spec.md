# GitHub Flavored Markdown Spec

Converted from `spec.txt` into Tracey requirement blocks.

# Introduction

## What is GitHub Flavored Markdown?

GitHub Flavored Markdown, often shortened as GFM, is the dialect of Markdown
that is currently supported for user content on GitHub.com and GitHub
Enterprise.

This formal specification, based on the CommonMark Spec, defines the syntax and
semantics of this dialect.

GFM is a strict superset of CommonMark. All the features which are supported in
GitHub user content and that are not specified on the original CommonMark Spec
are hence known as **extensions**, and highlighted as such.

While GFM supports a wide range of inputs, it's worth noting that GitHub.com
and GitHub Enterprise perform additional post-processing and sanitization after
GFM is converted to HTML to ensure security and consistency of the website.

## What is Markdown?

Markdown is a plain text format for writing structured documents,
based on conventions for indicating formatting in email
and usenet posts.  It was developed by John Gruber (with
help from Aaron Swartz) and released in 2004 in the form of a
[syntax description](http://daringfireball.net/projects/markdown/syntax)
and a Perl script (`Markdown.pl`) for converting Markdown to
HTML.  In the next decade, dozens of implementations were
developed in many languages.  Some extended the original
Markdown syntax with conventions for footnotes, tables, and
other document elements.  Some allowed Markdown documents to be
rendered in formats other than HTML.  Websites like Reddit,
StackOverflow, and GitHub had millions of people using Markdown.
And Markdown started to be used beyond the web, to author books,
articles, slide shows, letters, and lecture notes.

What distinguishes Markdown from many other lightweight markup
syntaxes, which are often easier to write, is its readability.
As Gruber writes:

> The overriding design goal for Markdown's formatting syntax is
> to make it as readable as possible. The idea is that a
> Markdown-formatted document should be publishable as-is, as
> plain text, without looking like it's been marked up with tags
> or formatting instructions.
> (<http://daringfireball.net/projects/markdown/>)

The point can be illustrated by comparing a sample of
[AsciiDoc](http://www.methods.co.nz/asciidoc/) with
an equivalent sample of Markdown.  Here is a sample of
AsciiDoc from the AsciiDoc manual:

```
1. List item one.
+
List item one continued with a second paragraph followed by an
Indented block.
+
.................
$ ls *.sh
$ mv *.sh ~/tmp
.................
+
List item continued with a third paragraph.

2. List item two continued with an open block.
+
--
This paragraph is part of the preceding list item.

a. This list is nested and does not require explicit item
continuation.
+
This paragraph is part of the preceding list item.

b. List item b.

This paragraph belongs to item two of the outer list.
--
```

And here is the equivalent in Markdown:

```
1.  List item one.

    List item one continued with a second paragraph followed by an
    Indented block.

        $ ls *.sh
        $ mv *.sh ~/tmp

    List item continued with a third paragraph.

2.  List item two continued with an open block.

    This paragraph is part of the preceding list item.

    1. This list is nested and does not require explicit item continuation.

       This paragraph is part of the preceding list item.

    2. List item b.

    This paragraph belongs to item two of the outer list.
```

The AsciiDoc version is, arguably, easier to write. You don't need
to worry about indentation.  But the Markdown version is much easier
to read.  The nesting of list items is apparent to the eye in the
source, not just in the processed document.

## Why is a spec needed?

John Gruber's [canonical description of Markdown's
syntax](http://daringfireball.net/projects/markdown/syntax)
does not specify the syntax unambiguously.  Here are some examples of
questions it does not answer:

1.  How much indentation is needed for a sublist?  The spec says that
    continuation paragraphs need to be indented four spaces, but is
    not fully explicit about sublists.  It is natural to think that
    they, too, must be indented four spaces, but `Markdown.pl` does
    not require that.  This is hardly a "corner case," and divergences
    between implementations on this issue often lead to surprises for
    users in real documents. (See [this comment by John
    Gruber](https://web.archive.org/web/20170611172104/http://article.gmane.org/gmane.text.markdown.general/1997).)

2.  Is a blank line needed before a block quote or heading?
    Most implementations do not require the blank line.  However,
    this can lead to unexpected results in hard-wrapped text, and
    also to ambiguities in parsing (note that some implementations
    put the heading inside the blockquote, while others do not).
    (John Gruber has also spoken [in favor of requiring the blank
    lines](https://web.archive.org/web/20170611172104/http://article.gmane.org/gmane.text.markdown.general/2146).)

3.  Is a blank line needed before an indented code block?
    (`Markdown.pl` requires it, but this is not mentioned in the
    documentation, and some implementations do not require it.)

    ``` markdown
    paragraph
        code?
    ```

4.  What is the exact rule for determining when list items get
    wrapped in `<p>` tags?  Can a list be partially "loose" and partially
    "tight"?  What should we do with a list like this?

    ``` markdown
    1. one

    2. two
    3. three
    ```

    Or this?

    ``` markdown
    1.  one
        - a

        - b
    2.  two
    ```

    (There are some relevant comments by John Gruber
    [here](https://web.archive.org/web/20170611172104/http://article.gmane.org/gmane.text.markdown.general/2554).)

5.  Can list markers be indented?  Can ordered list markers be right-aligned?

    ``` markdown
     8. item 1
     9. item 2
    10. item 2a
    ```

6.  Is this one list with a thematic break in its second item,
    or two lists separated by a thematic break?

    ``` markdown
    * a
    * * * * *
    * b
    ```

7.  When list markers change from numbers to bullets, do we have
    two lists or one?  (The Markdown syntax description suggests two,
    but the perl scripts and many other implementations produce one.)

    ``` markdown
    1. fee
    2. fie
    -  foe
    -  fum
    ```

8.  What are the precedence rules for the markers of inline structure?
    For example, is the following a valid link, or does the code span
    take precedence ?

    ``` markdown
    [a backtick (`)](/url) and [another backtick (`)](/url).
    ```

9.  What are the precedence rules for markers of emphasis and strong
    emphasis?  For example, how should the following be parsed?

    ``` markdown
    *foo *bar* baz*
    ```

10. What are the precedence rules between block-level and inline-level
    structure?  For example, how should the following be parsed?

    ``` markdown
    - `a long code span can contain a hyphen like this
      - and it can screw things up`
    ```

11. Can list items include section headings?  (`Markdown.pl` does not
    allow this, but does allow blockquotes to include headings.)

    ``` markdown
    - # Heading
    ```

12. Can list items be empty?

    ``` markdown
    * a
    *
    * b
    ```

13. Can link references be defined inside block quotes or list items?

    ``` markdown
    > Blockquote [foo].
    >
    > [foo]: /url
    ```

14. If there are multiple definitions for the same reference, which takes
    precedence?

    ``` markdown
    [foo]: /url1
    [foo]: /url2

    [foo][]
    ```

In the absence of a spec, early implementers consulted `Markdown.pl`
to resolve these ambiguities.  But `Markdown.pl` was quite buggy, and
gave manifestly bad results in many cases, so it was not a
satisfactory replacement for a spec.

Because there is no unambiguous spec, implementations have diverged
considerably.  As a result, users are often surprised to find that
a document that renders one way on one system (say, a GitHub wiki)
renders differently on another (say, converting to docbook using
pandoc).  To make matters worse, because nothing in Markdown counts
as a "syntax error," the divergence often isn't discovered right away.

## About this document

This document attempts to specify Markdown syntax unambiguously.
It contains many examples with side-by-side Markdown and
HTML.  These are intended to double as conformance tests.  An
accompanying script `spec_tests.py` can be used to run the tests
against any Markdown program:

    python test/spec_tests.py --spec spec.txt --program PROGRAM

Since this document describes how Markdown is to be parsed into
an abstract syntax tree, it would have made sense to use an abstract
representation of the syntax tree instead of HTML.  But HTML is capable
of representing the structural distinctions we need to make, and the
choice of HTML for the tests makes it possible to run the tests against
an implementation without writing an abstract syntax tree renderer.

This document is generated from a text file, `spec.txt`, written
in Markdown with a small extension for the side-by-side tests.
The script `tools/makespec.py` can be used to convert `spec.txt` into
HTML or CommonMark (which can then be converted into other formats).

In the examples, the `→` character is used to represent tabs.

# Preliminaries

## Characters and lines

> gfm[preliminaries.characters-and-lines.valid-commonmark-document]
> Any sequence of [characters] is a valid CommonMark
> document.

> gfm[preliminaries.characters-and-lines.character-definition]
> A [character](@) is a Unicode code point.  Although some
> code points (for example, combining accents) do not correspond to
> characters in an intuitive sense, all code points count as characters
> for purposes of this spec.

> gfm[preliminaries.characters-and-lines.encoding-is-unspecified]
> This spec does not specify an encoding; it thinks of lines as composed
> of [characters] rather than bytes.  A conforming parser may be limited
> to a certain encoding.

> gfm[preliminaries.characters-and-lines.line-definition]
> A [line](@) is a sequence of zero or more [characters]
> other than newline (`U+000A`) or carriage return (`U+000D`),
> followed by a [line ending] or by the end of file.

> gfm[preliminaries.characters-and-lines.line-ending-definition]
> A [line ending](@) is a newline (`U+000A`), a carriage return
> (`U+000D`) not followed by a newline, or a carriage return and a
> following newline.

> gfm[preliminaries.characters-and-lines.blank-line-definition]
> A line containing no characters, or a line containing only spaces
> (`U+0020`) or tabs (`U+0009`), is called a [blank line](@).

> gfm[preliminaries.characters-and-lines.character-class-definitions]
> The following definitions of character classes will be used in this spec:

> gfm[preliminaries.characters-and-lines.whitespace-character-definition]
> A [whitespace character](@) is a space
> (`U+0020`), tab (`U+0009`), newline (`U+000A`), line tabulation (`U+000B`),
> form feed (`U+000C`), or carriage return (`U+000D`).

> gfm[preliminaries.characters-and-lines.whitespace-definition]
> [Whitespace](@) is a sequence of one or more [whitespace
> characters].

> gfm[preliminaries.characters-and-lines.unicode-whitespace-character-definition]
> A [Unicode whitespace character](@) is
> any code point in the Unicode `Zs` general category, or a tab (`U+0009`),
> carriage return (`U+000D`), newline (`U+000A`), or form feed
> (`U+000C`).

> gfm[preliminaries.characters-and-lines.unicode-whitespace-definition]
> [Unicode whitespace](@) is a sequence of one
> or more [Unicode whitespace characters].

> gfm[preliminaries.characters-and-lines.space-definition]
> A [space](@) is `U+0020`.

> gfm[preliminaries.characters-and-lines.non-whitespace-character-definition]
> A [non-whitespace character](@) is any character
> that is not a [whitespace character].

> gfm[preliminaries.characters-and-lines.ascii-punctuation-character-definition]
> An [ASCII punctuation character](@)
> is `!`, `"`, `#`, `$`, `%`, `&`, `'`, `(`, `)`,
> `*`, `+`, `,`, `-`, `.`, `/` (U+0021–2F), 
> `:`, `;`, `<`, `=`, `>`, `?`, `@` (U+003A–0040),
> `[`, `\`, `]`, `^`, `_`, `` ` `` (U+005B–0060), 
> `{`, `|`, `}`, or `~` (U+007B–007E).

> gfm[preliminaries.characters-and-lines.punctuation-character-definition]
> A [punctuation character](@) is an [ASCII
> punctuation character] or anything in
> the general Unicode categories  `Pc`, `Pd`, `Pe`, `Pf`, `Pi`, `Po`, or `Ps`.

## Tabs

> gfm[preliminaries.tabs.tabs-not-expanded-in-lines]
> Tabs in lines are not expanded to [spaces].  However,
> in contexts where whitespace helps to define block structure,
> tabs behave as if they were replaced by spaces with a tab stop
> of 4 characters.

> gfm[preliminaries.tabs.tab-indented-code-block-equivalence]
> Thus, for example, a tab can be used instead of four spaces
> in an indented code block.  (Note, however, that internal
> tabs are passed through as literal tabs, not expanded to
> spaces.)
>
> ```````````````````````````````` example
> →foo→baz→→bim
> .
> <pre><code>foo→baz→→bim
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>   →foo→baz→→bim
> .
> <pre><code>foo→baz→→bim
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>     a→a
>     ὐ→a
> .
> <pre><code>a→a
> ὐ→a
> </code></pre>
> ````````````````````````````````

> gfm[preliminaries.tabs.tab-indented-list-continuation-equivalence]
> In the following example, a continuation paragraph of a list
> item is indented with a tab; this has exactly the same effect
> as indentation with four spaces would:
>
> ```````````````````````````````` example
>   - foo
>
> →bar
> .
> <ul>
> <li>
> <p>foo</p>
> <p>bar</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - foo
>
> →→bar
> .
> <ul>
> <li>
> <p>foo</p>
> <pre><code>  bar
> </code></pre>
> </li>
> </ul>
> ````````````````````````````````

> gfm[preliminaries.tabs.tab-after-block-quote-marker-counts-toward-indent]
> Normally the `>` that begins a block quote may be followed
> optionally by a space, which is not considered part of the
> content.  In the following case `>` is followed by a tab,
> which is treated as if it were expanded into three spaces.
> Since one of these spaces is considered part of the
> delimiter, `foo` is considered to be indented six spaces
> inside the block quote context, so we get an indented
> code block starting with two spaces.
>
> ```````````````````````````````` example
> >→→foo
> .
> <blockquote>
> <pre><code>  foo
> </code></pre>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> -→→foo
> .
> <ul>
> <li>
> <pre><code>  foo
> </code></pre>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
>     foo
> →bar
> .
> <pre><code>foo
> bar
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>  - foo
>    - bar
> → - baz
> .
> <ul>
> <li>foo
> <ul>
> <li>bar
> <ul>
> <li>baz</li>
> </ul>
> </li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> #→Foo
> .
> <h1>Foo</h1>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *→*→*→
> .
> <hr />
> ````````````````````````````````

## Insecure characters

> gfm[preliminaries.insecure-characters.null-character-is-replaced]
> For security reasons, the Unicode character `U+0000` must be replaced
> with the REPLACEMENT CHARACTER (`U+FFFD`).

# Blocks and inlines

> gfm[blocks-and-inlines.document-is-sequence-of-blocks]
> We can think of a document as a sequence of
> [blocks](@)---structural elements like paragraphs, block
> quotations, lists, headings, rules, and code blocks.  Some blocks (like
> block quotes and list items) contain other blocks; others (like
> headings and paragraphs) contain [inline](@) content---text,
> links, emphasized text, images, code spans, and so on.

## Precedence

> gfm[blocks-and-inlines.precedence.block-indicators-take-precedence-over-inline-indicators]
> Indicators of block structure always take precedence over indicators
> of inline structure.  So, for example, the following is a list with
> two items, not a list with one item containing a code span:
>
> ```````````````````````````````` example
> - `one
> - two`
> .
> <ul>
> <li>`one</li>
> <li>two`</li>
> </ul>
> ````````````````````````````````

> gfm[blocks-and-inlines.precedence.two-phase-parsing]
> This means that parsing can proceed in two steps:  first, the block
> structure of the document can be discerned; second, text lines inside
> paragraphs, headings, and other block constructs can be parsed for inline
> structure.  The second step requires information about link reference
> definitions that will be available only at the end of the first
> step.  Note that the first step requires processing lines in sequence,
> but the second can be parallelized, since the inline parsing of
> one block element does not affect the inline parsing of any other.

## Container blocks and leaf blocks

> gfm[blocks-and-inlines.container-and-leaf-blocks-definition]
> We can divide blocks into two types:
> [container blocks](@),
> which can contain other blocks, and [leaf blocks](@),
> which cannot.

# Leaf blocks

> gfm[leaf-blocks.leaf-blocks-make-up-markdown-document]
> This section describes the different kinds of leaf block that make up a
> Markdown document.

## Thematic breaks

> gfm[leaf-blocks.thematic-breaks.structure]
> A [thematic break](@) is a line consisting of indentation followed by a
> sequence of thematic break marker characters.
>
> ```````````````````````````````` example
> ***
> ---
> ___
> .
> <hr />
> <hr />
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.indent-up-to-three-spaces]
> A thematic break may be indented by 0-3 spaces.
>
> ```````````````````````````````` example
>  ***
>   ***
>    ***
> .
> <hr />
> <hr />
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.requires-three-or-more-marker-characters]
> A thematic break uses a sequence of three or more marker characters.
>
> Wrong characters:
>
> ```````````````````````````````` example
> +++
> .
> <p>+++</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ===
> .
> <p>===</p>
> ````````````````````````````````
>
> Not enough characters:
>
> ```````````````````````````````` example
> --
> **
> __
> .
> <p>--
> **
> __</p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.requires-dash-underscore-or-asterisk-markers]
> A thematic break uses matching `-`, `_`, or `*` marker characters.

> gfm[leaf-blocks.thematic-breaks.indent-four-spaces-not-allowed]
> Four spaces is too many:
>
> ```````````````````````````````` example
>     ***
> .
> <pre><code>***
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> Foo
>     ***
> .
> <p>Foo
> ***</p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.marker-sequence-may-be-longer-than-three]
> More than three characters may be used:
>
> ```````````````````````````````` example
> _____________________________________
> .
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.spaces-or-tabs-allowed-between-markers]
> Spaces are allowed between the characters:
>
> ```````````````````````````````` example
>  - - -
> .
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
>  **  * ** * ** * **
> .
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
> -     -      -      -
> .
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.trailing-spaces-allowed]
> Spaces are allowed at the end:
>
> ```````````````````````````````` example
> - - - -    
> .
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.no-other-characters-allowed]
> However, no other characters may occur in the line:
>
> ```````````````````````````````` example
> _ _ _ _ a
>
> a------
>
> ---a---
> .
> <p>_ _ _ _ a</p>
> <p>a------</p>
> <p>---a---</p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.all-marker-characters-must-match]
> It is required that all of the [non-whitespace characters] be the same.
> So, this is not a thematic break:
>
> ```````````````````````````````` example
>  *-*
> .
> <p><em>-</em></p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.need-not-be-surrounded-by-blank-lines]
> Thematic breaks do not need blank lines before or after:
>
> ```````````````````````````````` example
> - foo
> ***
> - bar
> .
> <ul>
> <li>foo</li>
> </ul>
> <hr />
> <ul>
> <li>bar</li>
> </ul>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.can-interrupt-paragraphs]
> Thematic breaks can interrupt a paragraph:
>
> ```````````````````````````````` example
> Foo
> ***
> bar
> .
> <p>Foo</p>
> <hr />
> <p>bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.setext-heading-takes-precedence-over-dash-break]
> If a line of dashes that meets the above conditions for being a
> thematic break could also be interpreted as the underline of a [setext
> heading], the interpretation as a
> [setext heading] takes precedence. Thus, for example,
> this is a setext heading, not a paragraph followed by a thematic break:
>
> ```````````````````````````````` example
> Foo
> ---
> bar
> .
> <h2>Foo</h2>
> <p>bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.thematic-break-takes-precedence-over-list-item]
> When both a thematic break and a list item are possible
> interpretations of a line, the thematic break takes precedence:
>
> ```````````````````````````````` example
> * Foo
> * * *
> * Bar
> .
> <ul>
> <li>Foo</li>
> </ul>
> <hr />
> <ul>
> <li>Bar</li>
> </ul>
> ````````````````````````````````

> gfm[leaf-blocks.thematic-breaks.use-different-bullet-for-break-inside-list-item]
> If you want a thematic break in a list item, use a different bullet:
>
> ```````````````````````````````` example
> - Foo
> - * * *
> .
> <ul>
> <li>Foo</li>
> <li>
> <hr />
> </li>
> </ul>
> ````````````````````````````````

## ATX headings

> gfm[leaf-blocks.atx-headings.structure]
> An [ATX heading](@)
> consists of a string of characters, parsed as inline content, between an
> opening sequence of unescaped `#` characters and an optional
> closing sequence of unescaped `#` characters.

> gfm[leaf-blocks.atx-headings.opening-sequence-length-one-to-six]
> The opening sequence of an ATX heading consists of 1--6 unescaped `#`
> characters.  The heading level is equal to the number of `#`
> characters in the opening sequence.
>
> Simple headings:
>
> ```````````````````````````````` example
> # foo
> ## foo
> ### foo
> #### foo
> ##### foo
> ###### foo
> .
> <h1>foo</h1>
> <h2>foo</h2>
> <h3>foo</h3>
> <h4>foo</h4>
> <h5>foo</h5>
> <h6>foo</h6>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.opening-sequence-length-at-most-six]
> More than six `#` characters is not a heading:
>
> ```````````````````````````````` example
> ####### foo
> .
> <p>####### foo</p>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.opening-sequence-followed-by-space-or-eol]
> At least one space is required between the `#` characters and the
> heading's contents, unless the heading is empty.  Note that many
> implementations currently do not require the space.  However, the
> space was required by the
> [original ATX implementation](http://www.aaronsw.com/2002/atx/atx.py),
> and it helps prevent things like the following from being parsed as
> headings:
>
> ```````````````````````````````` example
> #5 bolt
>
> #hashtag
> .
> <p>#5 bolt</p>
> <p>#hashtag</p>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.escaped-opening-hash-does-not-open-heading]
> This is not a heading, because the first `#` is escaped:
>
> ```````````````````````````````` example
> \## foo
> .
> <p>## foo</p>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.contents-parse-as-inline-content]
> Contents are parsed as inlines:
>
> ```````````````````````````````` example
> # foo *bar* \*baz\*
> .
> <h1>foo <em>bar</em> *baz*</h1>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.trim-leading-and-trailing-whitespace-before-inline-parse]
> Leading and trailing [whitespace] is ignored in parsing inline content:
>
> ```````````````````````````````` example
> #                  foo                     
> .
> <h1>foo</h1>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.opening-indent-up-to-three-spaces]
> One to three spaces indentation are allowed:
>
> ```````````````````````````````` example
>  ### foo
>   ## foo
>    # foo
> .
> <h3>foo</h3>
> <h2>foo</h2>
> <h1>foo</h1>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.opening-indent-four-spaces-not-allowed]
> Four spaces are too much:
>
> ```````````````````````````````` example
>     # foo
> .
> <pre><code># foo
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo
>     # bar
> .
> <p>foo
> # bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.closing-sequence-optional]
> A closing sequence of `#` characters is optional:
>
> ```````````````````````````````` example
> ## foo ##
>   ###   bar    ###
> .
> <h2>foo</h2>
> <h3>bar</h3>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.closing-sequence-length-independent]
> It need not be the same length as the opening sequence:
>
> ```````````````````````````````` example
> # foo ##################################
> ##### foo ##
> .
> <h1>foo</h1>
> <h5>foo</h5>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.closing-sequence-may-be-followed-by-spaces-only]
> Spaces are allowed after the closing sequence:
>
> ```````````````````````````````` example
> ### foo ###     
> .
> <h3>foo</h3>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.non-space-after-closing-hashes-makes-them-content]
> A sequence of `#` characters with anything but [spaces] following it
> is not a closing sequence, but counts as part of the contents of the
> heading:
>
> ```````````````````````````````` example
> ### foo ### b
> .
> <h3>foo ### b</h3>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.closing-sequence-must-be-preceded-by-space]
> The closing sequence must be preceded by a space:
>
> ```````````````````````````````` example
> # foo#
> .
> <h1>foo#</h1>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.escaped-hash-does-not-count-in-closing-sequence]
> Backslash-escaped `#` characters do not count as part
> of the closing sequence:
>
> ```````````````````````````````` example
> ### foo \###
> ## foo #\##
> # foo \#
> .
> <h3>foo ###</h3>
> <h2>foo ###</h2>
> <h1>foo #</h1>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.can-interrupt-paragraphs-without-blank-lines]
> ATX headings need not be separated from surrounding content by blank
> lines, and they can interrupt paragraphs:
>
> ```````````````````````````````` example
> ****
> ## foo
> ****
> .
> <hr />
> <h2>foo</h2>
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
> Foo bar
> # baz
> Bar foo
> .
> <p>Foo bar</p>
> <h1>baz</h1>
> <p>Bar foo</p>
> ````````````````````````````````

> gfm[leaf-blocks.atx-headings.can-be-empty]
> ATX headings can be empty:
>
> ```````````````````````````````` example
> ## 
> #
> ### ###
> .
> <h2></h2>
> <h1></h1>
> <h3></h3>
> ````````````````````````````````

## Setext headings

> gfm[leaf-blocks.setext-headings.structure]
> A [setext heading](@) consists of one or more
> lines of text followed by a [setext heading underline].

> gfm[leaf-blocks.setext-headings.text-lines-require-non-whitespace-content]
> Each line of text in a setext heading contains at least one
> [non-whitespace character].

> gfm[leaf-blocks.setext-headings.text-lines-indent-up-to-three-spaces]
> Setext heading text lines may be indented by no more than 3 spaces.

> gfm[leaf-blocks.setext-headings.text-lines-must-otherwise-parse-as-paragraphs]
> If setext heading text lines were not followed by the setext heading
> underline, they would be interpreted as a paragraph.  They cannot be
> interpretable as a [code fence], [ATX heading][ATX headings],
> [block quote][block quotes], [thematic break][thematic breaks],
> [list item][list items], or [HTML block][HTML blocks].

> gfm[leaf-blocks.setext-headings.underline-uses-equals-or-dashes]
> A [setext heading underline](@) is a sequence of
> `=` characters or a sequence of `-` characters.

> gfm[leaf-blocks.setext-headings.level-determined-by-underline-marker]
> The heading is a level 1 heading if `=` characters are used in
> the [setext heading underline], and a level 2 heading if `-`
> characters are used.
>
> Simple examples:
>
> ```````````````````````````````` example
> Foo *bar*
> =========
>
> Foo *bar*
> ---------
> .
> <h1>Foo <em>bar</em></h1>
> <h2>Foo <em>bar</em></h2>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.contents-parse-preceding-lines-as-inline-content]
> The contents of the heading are the result of parsing the preceding
> lines of text as CommonMark inline content.

> gfm[leaf-blocks.setext-headings.need-not-be-surrounded-by-blank-lines]
> In general, a setext heading need not be preceded or followed by a
> blank line.

> gfm[leaf-blocks.setext-headings.cannot-interrupt-paragraphs]
> A setext heading cannot interrupt a paragraph, so when a setext
> heading comes after a paragraph, a blank line is needed between them.

> gfm[leaf-blocks.setext-headings.content-may-span-multiple-lines]
> The content of the header may span more than one line:
>
> ```````````````````````````````` example
> Foo *bar
> baz*
> ====
> .
> <h1>Foo <em>bar
> baz</em></h1>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.raw-content-concatenates-lines-and-trims-edge-whitespace]
> The contents are the result of parsing the headings's raw
> content as inlines.  The heading's raw content is formed by
> concatenating the lines and removing initial and final
> [whitespace].
>
> ```````````````````````````````` example
>   Foo *bar
> baz*→
> ====
> .
> <h1>Foo <em>bar
> baz</em></h1>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.underline-may-be-any-length]
> The underlining can be any length:
>
> ```````````````````````````````` example
> Foo
> -------------------------
>
> Foo
> =
> .
> <h2>Foo</h2>
> <h1>Foo</h1>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.content-indent-up-to-three-spaces-and-need-not-align]
> The heading content can be indented up to three spaces, and need
> not line up with the underlining:
>
> ```````````````````````````````` example
>    Foo
> ---
>
>   Foo
> -----
>
>   Foo
>   ===
> .
> <h2>Foo</h2>
> <h2>Foo</h2>
> <h1>Foo</h1>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.content-indent-four-spaces-not-allowed]
> Four spaces indent is too much:
>
> ```````````````````````````````` example
>     Foo
>     ---
>
>     Foo
> ---
> .
> <pre><code>Foo
> ---
>
> Foo
> </code></pre>
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.underline-indent-up-to-three-spaces]
> The setext heading underline can be indented up to three spaces, and
> may have trailing spaces:
>
> ```````````````````````````````` example
> Foo
>    ----      
> .
> <h2>Foo</h2>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.underline-indent-four-spaces-not-allowed]
> Four spaces is too much:
>
> ```````````````````````````````` example
> Foo
>     ---
> .
> <p>Foo
> ---</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.setext-heading-underline-cannot-contain-internal-spaces]
> The setext heading underline cannot contain internal spaces:
>
> ```````````````````````````````` example
> Foo
> = =
>
> Foo
> --- -
> .
> <p>Foo
> = =</p>
> <p>Foo</p>
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.trailing-spaces-in-content-do-not-create-line-break]
> Trailing spaces in the content line do not cause a line break:
>
> ```````````````````````````````` example
> Foo  
> -----
> .
> <h2>Foo</h2>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.trailing-backslash-does-not-create-line-break]
> Nor does a backslash at the end:
>
> ```````````````````````````````` example
> Foo\
> ----
> .
> <h2>Foo\</h2>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.block-indicators-take-precedence-over-inline-indicators]
> Since indicators of block structure take precedence over
> indicators of inline structure, the following are setext headings:
>
> ```````````````````````````````` example
> `Foo
> ----
> `
>
> <a title="a lot
> ---
> of dashes"/>
> .
> <h2>`Foo</h2>
> <p>`</p>
> <h2>&lt;a title=&quot;a lot</h2>
> <p>of dashes&quot;/&gt;</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.underline-cannot-be-lazy-continuation-line]
> The setext heading underline cannot be a [lazy continuation
> line] in a list item or block quote:
>
> ```````````````````````````````` example
> > Foo
> ---
> .
> <blockquote>
> <p>Foo</p>
> </blockquote>
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
> > foo
> bar
> ===
> .
> <blockquote>
> <p>foo
> bar
> ===</p>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - Foo
> ---
> .
> <ul>
> <li>Foo</li>
> </ul>
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.blank-line-required-after-paragraph-before-heading]
> A blank line is needed between a paragraph and a following
> setext heading, since otherwise the paragraph becomes part
> of the heading's content:
>
> ```````````````````````````````` example
> Foo
> Bar
> ---
> .
> <h2>Foo
> Bar</h2>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.blank-lines-generally-not-required-around-heading]
> But in general a blank line is not required before or after
> setext headings:
>
> ```````````````````````````````` example
> ---
> Foo
> ---
> Bar
> ---
> Baz
> .
> <hr />
> <h2>Foo</h2>
> <h2>Bar</h2>
> <p>Baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.cannot-be-empty]
> Setext headings cannot be empty:
>
> ```````````````````````````````` example
>
> ====
> .
> <p>====</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.text-lines-must-not-parse-as-other-block-constructs]
> Setext heading text lines must not be interpretable as block
> constructs other than paragraphs.  So, the line of dashes
> in these examples gets interpreted as a thematic break:
>
> ```````````````````````````````` example
> ---
> ---
> .
> <hr />
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
> - foo
> -----
> .
> <ul>
> <li>foo</li>
> </ul>
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
>     foo
> ---
> .
> <pre><code>foo
> </code></pre>
> <hr />
> ````````````````````````````````
>
> ```````````````````````````````` example
> > foo
> -----
> .
> <blockquote>
> <p>foo</p>
> </blockquote>
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.backslash-escapes-allow-literal-block-markers-in-content]
> If you want a heading with `> foo` as its literal text, you can
> use backslash escapes:
>
> ```````````````````````````````` example
> \> foo
> ------
> .
> <h2>&gt; foo</h2>
> ````````````````````````````````
>
> **Compatibility note:**  Most existing Markdown implementations
> do not allow the text of setext headings to span multiple lines.
> But there is no consensus about how to interpret
>
> ``` markdown
> Foo
> bar
> ---
> baz
> ```

> gfm[leaf-blocks.setext-headings.multiline-heading-has-four-interpretations]
> One can find four different interpretations:
>
> 1. paragraph "Foo", heading "bar", paragraph "baz"
> 2. paragraph "Foo bar", thematic break, paragraph "baz"
> 3. paragraph "Foo bar --- baz"
> 4. heading "Foo bar", paragraph "baz"

> gfm[leaf-blocks.setext-headings.multiline-heading-interprets-as-heading-then-paragraph]
> We find interpretation 4 most natural, and interpretation 4
> increases the expressive power of CommonMark, by allowing
> multiline headings.  Authors who want interpretation 1 can
> put a blank line after the first paragraph:
>
> ```````````````````````````````` example
> Foo
>
> bar
> ---
> baz
> .
> <p>Foo</p>
> <h2>bar</h2>
> <p>baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.blank-lines-around-break-produce-hr]
> Authors who want interpretation 2 can put blank lines around
> the thematic break,
>
> ```````````````````````````````` example
> Foo
> bar
>
> ---
>
> baz
> .
> <p>Foo
> bar</p>
> <hr />
> <p>baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.non-setext-break-marker-produces-hr]
> or use a thematic break that cannot count as a [setext heading
> underline], such as
>
> ```````````````````````````````` example
> Foo
> bar
> * * *
> baz
> .
> <p>Foo
> bar</p>
> <hr />
> <p>baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.setext-headings.backslash-escaped-underline-produces-literal-text]
> Authors who want interpretation 3 can use backslash escapes:
>
> ```````````````````````````````` example
> Foo
> bar
> \---
> baz
> .
> <p>Foo
> bar
> ---
> baz</p>
> ````````````````````````````````

## Indented code blocks

> gfm[leaf-blocks.indented-code-blocks.definition]
> An [indented code block](@) is composed of one or more
> [indented chunks] separated by blank lines.
> An [indented chunk](@) is a sequence of non-blank lines,
> each indented four or more spaces. The contents of the code block are
> the literal contents of the lines, including trailing
> [line endings], minus four spaces of indentation.
> An indented code block has no [info string].

> gfm[leaf-blocks.indented-code-blocks.cannot-interrupt-paragraphs]
> An indented code block cannot interrupt a paragraph, so there must be
> a blank line between a paragraph and a following indented code block.
> (A blank line is not needed, however, between a code block and a following
> paragraph.)
>
> ```````````````````````````````` example
>     a simple
>       indented code block
> .
> <pre><code>a simple
>   indented code block
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.list-item-interpretation-takes-precedence]
> If there is any ambiguity between an interpretation of indentation
> as a code block and as indicating that material belongs to a [list
> item][list items], the list item interpretation takes precedence:
>
> ```````````````````````````````` example
>   - foo
>
>     bar
> .
> <ul>
> <li>
> <p>foo</p>
> <p>bar</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1.  foo
>
>     - bar
> .
> <ol>
> <li>
> <p>foo</p>
> <ul>
> <li>bar</li>
> </ul>
> </li>
> </ol>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.contents-are-literal-text]
> The contents of a code block are literal text, and do not get parsed
> as Markdown:
>
> ```````````````````````````````` example
>     <a/>
>     *hi*
>
>     - one
> .
> <pre><code>&lt;a/&gt;
> *hi*
>
> - one
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.blank-lines-separate-chunks]
> Here we have three chunks separated by blank lines:
>
> ```````````````````````````````` example
>     chunk1
>
>     chunk2
>   
>  
>  
>     chunk3
> .
> <pre><code>chunk1
>
> chunk2
>
>
>
> chunk3
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.indent-beyond-four-is-preserved]
> Any initial spaces beyond four will be included in the content, even
> in interior blank lines:
>
> ```````````````````````````````` example
>     chunk1
>       
>       chunk2
> .
> <pre><code>chunk1
>   
>   chunk2
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.indented-code-block-cannot-interrupt-paragraph]
> An indented code block cannot interrupt a paragraph.  (This
> allows hanging indents and the like.)
>
> ```````````````````````````````` example
> Foo
>     bar
>
> .
> <p>Foo
> bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.less-than-four-spaces-ends-block]
> However, any non-blank line with fewer than four leading spaces ends
> the code block immediately.  So a paragraph may occur immediately
> after indented code:
>
> ```````````````````````````````` example
>     foo
> bar
> .
> <pre><code>foo
> </code></pre>
> <p>bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.may-occur-adjacent-to-other-blocks]
> And indented code can occur immediately before and after other kinds of
> blocks:
>
> ```````````````````````````````` example
> # Heading
>     foo
> Heading
> ------
>     foo
> ----
> .
> <h1>Heading</h1>
> <pre><code>foo
> </code></pre>
> <h2>Heading</h2>
> <pre><code>foo
> </code></pre>
> <hr />
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.first-line-can-indented-more-than-four-spaces]
> The first line can be indented more than four spaces:
>
> ```````````````````````````````` example
>         foo
>     bar
> .
> <pre><code>    foo
> bar
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.surrounding-blank-lines-are-excluded]
> Blank lines preceding or following an indented code block
> are not included in it:
>
> ```````````````````````````````` example
>
>     
>     foo
>     
>
> .
> <pre><code>foo
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.indented-code-blocks.trailing-spaces-are-preserved]
> Trailing spaces are included in the code block's content:
>
> ```````````````````````````````` example
>     foo  
> .
> <pre><code>foo  
> </code></pre>
> ````````````````````````````````

## Fenced code blocks

> gfm[leaf-blocks.fenced-code-blocks.code-fence-uses-backticks-or-tildes]
> A [code fence](@) is a sequence of at least three consecutive
> backtick characters (`` ` ``) or tildes (`~`).

> gfm[leaf-blocks.fenced-code-blocks.fence-marker-cannot-mix-backticks-and-tildes]
> Tildes and backticks cannot be mixed in a code fence.

> gfm[leaf-blocks.fenced-code-blocks.opening-fence-indent-up-to-three-spaces]
> A [fenced code block](@) begins with a code fence indented no more
> than three spaces.

> gfm[leaf-blocks.fenced-code-blocks.opening-fence-may-include-info-string]
> The line with the opening code fence may optionally contain some text
> following the code fence; this is trimmed of leading and trailing
> whitespace and called the [info string](@).

> gfm[leaf-blocks.fenced-code-blocks.backtick-info-string-cannot-contain-backticks]
> If the [info string] comes after a backtick fence, it may not contain
> any backtick characters.

> gfm[leaf-blocks.fenced-code-blocks.content-continues-until-matching-closing-fence]
> The content of the code block consists of all subsequent lines, until
> a closing [code fence] of the same type as the code block
> began with (backticks or tildes), and with at least as many backticks
> or tildes as the opening code fence.

> gfm[leaf-blocks.fenced-code-blocks.content-removes-up-to-opening-indent]
> If the leading code fence is indented N spaces, then up to N spaces
> of indentation are removed from each line of the content.

> gfm[leaf-blocks.fenced-code-blocks.short-indented-content-lines-remove-all-indent]
> If a content line is indented less than the opening fence indent,
> all of the content line indentation is removed.

> gfm[leaf-blocks.fenced-code-blocks.content-lines-without-indent-are-preserved]
> If a content line is not indented, it is preserved unchanged.

> gfm[leaf-blocks.fenced-code-blocks.closing-fence-indent-up-to-three-spaces]
> The closing code fence may be indented up to three spaces, and may be
> followed only by spaces, which are ignored.

> gfm[leaf-blocks.fenced-code-blocks.unclosed-block-continues-to-end-of-containing-block]
> If the end of the containing block (or document) is reached and no
> closing code fence has been found, the code block contains all of the
> lines after the opening code fence until the end of the containing
> block (or document).

> gfm[leaf-blocks.fenced-code-blocks.can-interrupt-paragraphs]
> A fenced code block may interrupt a paragraph, and does not require
> a blank line either before or after.

> gfm[leaf-blocks.fenced-code-blocks.content-is-literal-text]
> The content of a code fence is treated as literal text, not parsed
> as inlines.

> gfm[leaf-blocks.fenced-code-blocks.simple-backtick-and-tilde-fences]
> Here is a simple example with backticks:
>
> ```````````````````````````````` example
> ```
> <
>  >
> ```
> .
> <pre><code>&lt;
>  &gt;
> </code></pre>
> ````````````````````````````````
>
> With tildes:
>
> ```````````````````````````````` example
> ~~~
> <
>  >
> ~~~
> .
> <pre><code>&lt;
>  &gt;
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.fewer-than-three-backticks-do-not-form-fence]
> Fewer than three backticks is not enough:
>
> ```````````````````````````````` example
> ``
> foo
> ``
> .
> <p><code>foo</code></p>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.closing-fence-must-use-same-marker-type]
> The closing code fence must use the same character as the opening
> fence:
>
> ```````````````````````````````` example
> ```
> aaa
> ~~~
> ```
> .
> <pre><code>aaa
> ~~~
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ~~~
> aaa
> ```
> ~~~
> .
> <pre><code>aaa
> ```
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.closing-fence-length-at-least-opening-length]
> The closing code fence must be at least as long as the opening fence:
>
> ```````````````````````````````` example
> ````
> aaa
> ```
> ``````
> .
> <pre><code>aaa
> ```
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ~~~~
> aaa
> ~~~
> ~~~~
> .
> <pre><code>aaa
> ~~~
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.unclosed-block-closes-at-end-of-containing-block]
> Unclosed code blocks are closed by the end of the document
> (or the enclosing [block quote][block quotes] or [list item][list items]):
>
> ```````````````````````````````` example
> ```
> .
> <pre><code></code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> `````
>
> ```
> aaa
> .
> <pre><code>
> ```
> aaa
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> > ```
> > aaa
>
> bbb
> .
> <blockquote>
> <pre><code>aaa
> </code></pre>
> </blockquote>
> <p>bbb</p>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.content-may-consist-only-of-empty-lines]
> A code block can have all empty lines as its content:
>
> ```````````````````````````````` example
> ```
>
>   
> ```
> .
> <pre><code>
>   
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.content-may-be-empty]
> A code block can be empty:
>
> ```````````````````````````````` example
> ```
> ```
> .
> <pre><code></code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.fences-may-be-indented-and-content-deindents-to-match]
> Fences can be indented.  If the opening fence is indented,
> content lines will have equivalent opening indentation removed,
> if present:
>
> ```````````````````````````````` example
>  ```
>  aaa
> aaa
> ```
> .
> <pre><code>aaa
> aaa
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>   ```
> aaa
>   aaa
> aaa
>   ```
> .
> <pre><code>aaa
> aaa
> aaa
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>    ```
>    aaa
>     aaa
>   aaa
>    ```
> .
> <pre><code>aaa
>  aaa
> aaa
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.four-space-indented-fence-starts-indented-code-block]
> Four spaces indentation produces an indented code block:
>
> ```````````````````````````````` example
>     ```
>     aaa
>     ```
> .
> <pre><code>```
> aaa
> ```
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.closing-fence-indent-need-not-match-opening-indent]
> Closing fences may be indented by 0-3 spaces, and their indentation
> need not match that of the opening fence:
>
> ```````````````````````````````` example
> ```
> aaa
>   ```
> .
> <pre><code>aaa
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>    ```
> aaa
>   ```
> .
> <pre><code>aaa
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.closing-fence-indented-four-spaces-does-not-close]
> This is not a closing fence, because it is indented 4 spaces:
>
> ```````````````````````````````` example
> ```
> aaa
>     ```
> .
> <pre><code>aaa
>     ```
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.fence-lines-cannot-contain-internal-spaces]
> Code fences (opening and closing) cannot contain internal spaces:
>
> ```````````````````````````````` example
> ``` ```
> aaa
> .
> <p><code> </code>
> aaa</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ~~~~~~
> aaa
> ~~~ ~~
> .
> <pre><code>aaa
> ~~~ ~~
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.can-be-followed-directly-by-paragraphs]
> Fenced code blocks can interrupt paragraphs, and can be followed
> directly by paragraphs, without a blank line between:
>
> ```````````````````````````````` example
> foo
> ```
> bar
> ```
> baz
> .
> <p>foo</p>
> <pre><code>bar
> </code></pre>
> <p>baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.other-blocks-can-appear-adjacent-without-blank-lines]
> Other blocks can also occur before and after fenced code blocks
> without an intervening blank line:
>
> ```````````````````````````````` example
> foo
> ---
> ~~~
> bar
> ~~~
> # baz
> .
> <h2>foo</h2>
> <pre><code>bar
> </code></pre>
> <h1>baz</h1>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.info-string-may-be-provided-after-opening-fence]
> An [info string] can be provided after the opening code fence.

> gfm[leaf-blocks.fenced-code-blocks.info-string-treatment-is-not-mandated]
> This spec does not mandate any particular treatment of the info string.

> gfm[leaf-blocks.fenced-code-blocks.first-info-string-word-typically-identifies-language]
> The first word of the info string is typically used to specify
> the language of the code block. In HTML output, the language is
> normally indicated by adding a class to the `code` element consisting
> of `language-` followed by the language name.
>
> ```````````````````````````````` example
> ```ruby
> def foo(x)
>   return 3
> end
> ```
> .
> <pre><code class="language-ruby">def foo(x)
>   return 3
> end
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ~~~~    ruby startline=3 $%@#$
> def foo(x)
>   return 3
> end
> ~~~~~~~
> .
> <pre><code class="language-ruby">def foo(x)
>   return 3
> end
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ````;
> ````
> .
> <pre><code class="language-;"></code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.backtick-info-strings-cannot-contain-backticks]
> [Info strings] for backtick code blocks cannot contain backticks:
>
> ```````````````````````````````` example
> ``` aa ```
> foo
> .
> <p><code>aa</code>
> foo</p>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.tilde-info-strings-may-contain-backticks-and-tildes]
> [Info strings] for tilde code blocks can contain backticks and tildes:
>
> ```````````````````````````````` example
> ~~~ aa ``` ~~~
> foo
> ~~~
> .
> <pre><code class="language-aa">foo
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.fenced-code-blocks.closing-fences-cannot-have-info-strings]
> Closing code fences cannot have [info strings]:
>
> ```````````````````````````````` example
> ```
> ``` aaa
> ```
> .
> <pre><code>``` aaa
> </code></pre>
> ````````````````````````````````

## HTML blocks

> gfm[leaf-blocks.html-blocks.definition]
> An [HTML block](@) is a group of lines that is treated
> as raw HTML (and will not be escaped in HTML output).

> gfm[leaf-blocks.html-blocks.has-seven-types]
> There are seven kinds of [HTML block], each defined by start and end
> conditions.

> gfm[leaf-blocks.html-blocks.starts-after-up-to-three-spaces-of-indentation]
> An HTML block begins with a line that meets a [start condition](@)
> after up to three spaces of optional indentation.

> gfm[leaf-blocks.html-blocks.ends-at-matching-end-condition-or-container-end]
> An HTML block ends with the first subsequent line that meets a
> matching [end condition](@), or the last line of the document, or the
> last line of the [container block](#container-blocks) containing the
> current HTML block, if no line is encountered that meets the
> [end condition].

> gfm[leaf-blocks.html-blocks.single-line-block-when-start-and-end-both-match]
> If the first line meets both the [start condition] and the
> [end condition], the block contains just that line.

> gfm[leaf-blocks.html-blocks.type-1-script-pre-style-start-and-end-conditions]
> Type 1 HTML blocks start with a line that begins with the string
> `<script`, `<pre`, or `<style` (case-insensitive), followed by
> whitespace, the string `>`, or the end of the line. They end with a
> line containing an end tag `</script>`, `</pre>`, or `</style>`
> (case-insensitive; it need not match the start tag).

> gfm[leaf-blocks.html-blocks.type-2-comment-start-and-end-conditions]
> Type 2 HTML blocks start with a line that begins with the string
> `<!--` and end with a line containing the string `-->`.

> gfm[leaf-blocks.html-blocks.type-3-processing-instruction-start-and-end-conditions]
> Type 3 HTML blocks start with a line that begins with the string `<?`
> and end with a line containing the string `?>`.

> gfm[leaf-blocks.html-blocks.type-4-declaration-start-and-end-conditions]
> Type 4 HTML blocks start with a line that begins with the string `<!`
> followed by an uppercase ASCII letter and end with a line containing
> the character `>`.

> gfm[leaf-blocks.html-blocks.type-5-cdata-start-and-end-conditions]
> Type 5 HTML blocks start with a line that begins with the string
> `<![CDATA[` and end with a line containing the string `]]>`.

> gfm[leaf-blocks.html-blocks.type-6-block-tag-start-and-blank-line-end]
> Type 6 HTML blocks start with a line that begins with the string `<`
> or `</` followed by one of the strings (case-insensitive) `address`,
> `article`, `aside`, `base`, `basefont`, `blockquote`, `body`,
> `caption`, `center`, `col`, `colgroup`, `dd`, `details`, `dialog`,
> `dir`, `div`, `dl`, `dt`, `fieldset`, `figcaption`, `figure`,
> `footer`, `form`, `frame`, `frameset`, `h1`, `h2`, `h3`, `h4`,
> `h5`, `h6`, `head`, `header`, `hr`, `html`, `iframe`, `legend`,
> `li`, `link`, `main`, `menu`, `menuitem`, `nav`, `noframes`, `ol`,
> `optgroup`, `option`, `p`, `param`, `section`, `summary`, `table`,
> `tbody`, `td`, `tfoot`, `th`, `thead`, `title`, `tr`, `track`, or
> `ul`, followed by [whitespace], the end of the line, the string `>`,
> or the string `/>`. They end when the line is followed by a
> [blank line].

> gfm[leaf-blocks.html-blocks.type-7-complete-tag-start-and-blank-line-end]
> Type 7 HTML blocks start with a line that begins with a complete
> [open tag] (with any [tag name] other than `script`, `style`, or
> `pre`) or a complete [closing tag], followed only by [whitespace] or
> the end of the line. They end when the line is followed by a
> [blank line].

> gfm[leaf-blocks.html-blocks.inner-html-does-not-change-active-html-block-type]
> HTML blocks continue until they are closed by their appropriate
> [end condition], or the last line of the document or other [container
> block](#container-blocks).  This means any HTML **within an HTML
> block** that might otherwise be recognised as a start condition will
> be ignored by the parser and passed through as-is, without changing
> the parser's state.

> gfm[leaf-blocks.html-blocks.inner-start-conditions-do-not-affect-parser-state]
> For instance, `<pre>` within a HTML block started by `<table>` will not affect
> the parser state; as the HTML block was started in by start condition 6, it
> will end at any blank line. This can be surprising:
>
> ```````````````````````````````` example
> <table><tr><td>
> <pre>
> **Hello**,
>
> _world_.
> </pre>
> </td></tr></table>
> .
> <table><tr><td>
> <pre>
> **Hello**,
> <p><em>world</em>.
> </pre></p>
> </td></tr></table>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.blank-line-ends-type-6-block-and-normal-parsing-resumes]
> In this case, the HTML block is terminated by the newline — the `**Hello**`
> text remains verbatim — and regular parsing resumes, with a paragraph,
> emphasised `world` and inline and block HTML following.

> gfm[leaf-blocks.html-blocks.types-1-through-6-can-interrupt-paragraphs]
> All types of [HTML blocks] except type 7 may interrupt
> a paragraph.

> gfm[leaf-blocks.html-blocks.type-7-cannot-interrupt-paragraphs]
> Blocks of type 7 may not interrupt a paragraph.

> gfm[leaf-blocks.html-blocks.type-6-examples]
> Some simple examples follow.  Here are some basic HTML blocks
> of type 6:
>
> ```````````````````````````````` example
> <table>
>   <tr>
>     <td>
>            hi
>     </td>
>   </tr>
> </table>
>
> okay.
> .
> <table>
>   <tr>
>     <td>
>            hi
>     </td>
>   </tr>
> </table>
> <p>okay.</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
>  <div>
>   *hello*
>          <foo><a>
> .
>  <div>
>   *hello*
>          <foo><a>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-may-start-with-closing-tag]
> A block can also start with a closing tag:
>
> ```````````````````````````````` example
> </div>
> *foo*
> .
> </div>
> *foo*
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.blank-line-separates-two-html-blocks-with-markdown-between]
> Here we have two HTML blocks with a Markdown paragraph between them:
>
> ```````````````````````````````` example
> <DIV CLASS="foo">
>
> *Markdown*
>
> </DIV>
> .
> <DIV CLASS="foo">
> <p><em>Markdown</em></p>
> </DIV>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-opening-tag-may-be-partial-when-split-at-whitespace]
> The tag on the first line can be partial, as long
> as it is split where there would be whitespace:
>
> ```````````````````````````````` example
> <div id="foo"
>   class="bar">
> </div>
> .
> <div id="foo"
>   class="bar">
> </div>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <div id="foo" class="bar
>   baz">
> </div>
> .
> <div id="foo" class="bar
>   baz">
> </div>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-open-tag-need-not-be-closed]
> An open tag need not be closed:
>
> ```````````````````````````````` example
> <div>
> *foo*
>
> *bar*
> .
> <div>
> *foo*
> <p><em>bar</em></p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-partial-tag-need-not-be-completed]
> A partial tag need not even be completed (garbage
> in, garbage out):
>
> ```````````````````````````````` example
> <div id="foo"
> *hi*
> .
> <div id="foo"
> *hi*
> ````````````````````````````````
>
> ```````````````````````````````` example
> <div class
> foo
> .
> <div class
> foo
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-valid]
> The initial tag doesn't even need to be a valid
> tag, as long as it starts like one:
>
> ```````````````````````````````` example
> <div *???-&&&-<---
> *foo*
> .
> <div *???-&&&-<---
> *foo*
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-initial-tag-need-not-be-alone-on-line]
> In type 6 blocks, the initial tag need not be on a line by
> itself:
>
> ```````````````````````````````` example
> <div><a href="bar">*foo*</a></div>
> .
> <div><a href="bar">*foo*</a></div>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <table><tr><td>
> foo
> </td></tr></table>
> .
> <table><tr><td>
> foo
> </td></tr></table>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-continues-through-apparent-markdown-until-blank-line]
> Everything until the next blank line or end of document
> gets included in the HTML block.  So, in the following
> example, what looks like a Markdown code block
> is actually part of the HTML block, which continues until a blank
> line or the end of the document is reached:
>
> ```````````````````````````````` example
> <div></div>
> ``` c
> int x = 33;
> ```
> .
> <div></div>
> ``` c
> int x = 33;
> ```
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.non-type-6-tag-must-be-complete-and-alone-on-first-line]
> To start an [HTML block] with a tag that is *not* in the
> list of block-level tags in (6), you must put the tag by
> itself on the first line (and it must be complete):
>
> ```````````````````````````````` example
> <a href="foo">
> *bar*
> </a>
> .
> <a href="foo">
> *bar*
> </a>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-7-tag-name-may-be-anything]
> In type 7 blocks, the [tag name] can be anything:
>
> ```````````````````````````````` example
> <Warning>
> *bar*
> </Warning>
> .
> <Warning>
> *bar*
> </Warning>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <i class="foo">
> *bar*
> </i>
> .
> <i class="foo">
> *bar*
> </i>
> ````````````````````````````````
>
> ```````````````````````````````` example
> </ins>
> *bar*
> .
> </ins>
> *bar*
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.block-vs-inline-tag-behavior]
> These rules are designed to allow us to work with tags that
> can function as either block-level or inline-level tags.
> The `<del>` tag is a nice example.  We can surround content with
> `<del>` tags in three different ways.  In this case, we get a raw
> HTML block, because the `<del>` tag is on a line by itself:
>
> ```````````````````````````````` example
> <del>
> *foo*
> </del>
> .
> <del>
> *foo*
> </del>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.blank-line-after-opening-tag-allows-following-markdown]
> In this case, we get a raw HTML block that just includes
> the `<del>` tag (because it ends with the following blank
> line).  So the contents get interpreted as CommonMark:
>
> ```````````````````````````````` example
> <del>
>
> *foo*
>
> </del>
> .
> <del>
> <p><em>foo</em></p>
> </del>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.inline-html-when-tag-is-not-alone-on-line]
> Finally, in this case, the `<del>` tags are interpreted
> as [raw HTML] *inside* the CommonMark paragraph.  (Because
> the tag is not on a line by itself, we get inline HTML
> rather than an [HTML block].)
>
> ```````````````````````````````` example
> <del>*foo*</del>
> .
> <p><del><em>foo</em></del></p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.types-1-through-5-end-at-matching-end-marker]
> HTML tags designed to contain literal content
> (`script`, `style`, `pre`), comments, processing instructions,
> and declarations are treated somewhat differently.
> Instead of ending at the first blank line, these blocks
> end at the first line containing a corresponding end tag.
>
> gfm[leaf-blocks.html-blocks.types-1-through-5-may-contain-blank-lines]
> As a result, these blocks can contain blank lines:

> gfm[leaf-blocks.html-blocks.type-1-pre-example]
> A pre tag (type 1):
>
> ```````````````````````````````` example
> <pre language="haskell"><code>
> import Text.HTML.TagSoup
>
> main :: IO ()
> main = print $ parseTags tags
> </code></pre>
> okay
> .
> <pre language="haskell"><code>
> import Text.HTML.TagSoup
>
> main :: IO ()
> main = print $ parseTags tags
> </code></pre>
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-1-script-example]
> A script tag (type 1):
>
> ```````````````````````````````` example
> <script type="text/javascript">
> // JavaScript example
>
> document.getElementById("demo").innerHTML = "Hello JavaScript!";
> </script>
> okay
> .
> <script type="text/javascript">
> // JavaScript example
>
> document.getElementById("demo").innerHTML = "Hello JavaScript!";
> </script>
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-1-style-example]
> A style tag (type 1):
>
> ```````````````````````````````` example
> <style
>   type="text/css">
> h1 {color:red;}
>
> p {color:blue;}
> </style>
> okay
> .
> <style
>   type="text/css">
> h1 {color:red;}
>
> p {color:blue;}
> </style>
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.types-1-through-5-without-end-marker-close-at-container-end]
> If there is no matching end tag, the block will end at the
> end of the document (or the enclosing [block quote][block quotes]
> or [list item][list items]):
>
> ```````````````````````````````` example
> <style
>   type="text/css">
>
> foo
> .
> <style
>   type="text/css">
>
> foo
> ````````````````````````````````
>
> ```````````````````````````````` example
> > <div>
> > foo
>
> bar
> .
> <blockquote>
> <div>
> foo
> </blockquote>
> <p>bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - <div>
> - foo
> .
> <ul>
> <li>
> <div>
> </li>
> <li>foo</li>
> </ul>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.end-marker-may-occur-on-same-line-as-start]
> The end tag can occur on the same line as the start tag:
>
> ```````````````````````````````` example
> <style>p{color:red;}</style>
> *foo*
> .
> <style>p{color:red;}</style>
> <p><em>foo</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <!-- foo -->*bar*
> *baz*
> .
> <!-- foo -->*bar*
> <p><em>baz</em></p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.trailing-text-after-end-marker-stays-inside-block]
> Note that anything on the last line after the
> end tag will be included in the [HTML block]:
>
> ```````````````````````````````` example
> <script>
> foo
> </script>1. *bar*
> .
> <script>
> foo
> </script>1. *bar*
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-2-comment-example]
> A comment (type 2):
>
> ```````````````````````````````` example
> <!-- Foo
>
> bar
>    baz -->
> okay
> .
> <!-- Foo
>
> bar
>    baz -->
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-3-processing-instruction-example]
> A processing instruction (type 3):
>
> ```````````````````````````````` example
> <?php
>
>   echo '>';
>
> ?>
> okay
> .
> <?php
>
>   echo '>';
>
> ?>
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-4-declaration-example]
> A declaration (type 4):
>
> ```````````````````````````````` example
> <!DOCTYPE html>
> .
> <!DOCTYPE html>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-5-cdata-example]
> CDATA (type 5):
>
> ```````````````````````````````` example
> <![CDATA[
> function matchwo(a,b)
> {
>   if (a < b && a < 0) then {
>     return 1;
>
>   } else {
>
>     return 0;
>   }
> }
> ]]>
> okay
> .
> <![CDATA[
> function matchwo(a,b)
> {
>   if (a < b && a < 0) then {
>     return 1;
>
>   } else {
>
>     return 0;
>   }
> }
> ]]>
> <p>okay</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.opening-tag-indent-up-to-three-spaces]
> The opening tag can be indented 1-3 spaces, but not 4:
>
> ```````````````````````````````` example
>   <!-- foo -->
>
>     <!-- foo -->
> .
>   <!-- foo -->
> <pre><code>&lt;!-- foo --&gt;
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>   <div>
>
>     <div>
> .
>   <div>
> <pre><code>&lt;div&gt;
> </code></pre>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.types-1-through-6-need-not-be-preceded-by-blank-line]
> An HTML block of types 1--6 can interrupt a paragraph, and need not be
> preceded by a blank line.
>
> ```````````````````````````````` example
> Foo
> <div>
> bar
> </div>
> .
> <p>Foo</p>
> <div>
> bar
> </div>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-6-and-7-usually-require-following-blank-line]
> However, a following blank line is needed, except at the end of
> a document, and except for blocks of types 1--5, [above][HTML
> block]:
>
> ```````````````````````````````` example
> <div>
> bar
> </div>
> *foo*
> .
> <div>
> bar
> </div>
> *foo*
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.type-7-example-cannot-interrupt-paragraph]
> HTML blocks of type 7 cannot interrupt a paragraph:
>
> ```````````````````````````````` example
> Foo
> <a href="bar">
> baz
> .
> <p>Foo
> <a href="bar">
> baz</p>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.differs-from-original-markdown-rule]
> This rule differs from John Gruber's original Markdown syntax
> specification, which says:

> gfm[leaf-blocks.html-blocks.original-markdown-rule-quote]
> > The only restrictions are that block-level HTML elements —
> > e.g. `<div>`, `<table>`, `<pre>`, `<p>`, etc. — must be separated from
> > surrounding content by blank lines, and the start and end tags of the
> > block should not be indented with tabs or spaces.

> gfm[leaf-blocks.html-blocks.original-markdown-rule-is-more-restrictive-in-some-ways]
> In some ways Gruber's rule is more restrictive than the one given
> here:
>
> - It requires that an HTML block be preceded by a blank line.
> - It does not allow the start tag to be indented.
> - It requires a matching end tag, which it also does not allow to
>   be indented.

> gfm[leaf-blocks.html-blocks.most-implementations-do-not-follow-all-original-restrictions]
> Most Markdown implementations (including some of Gruber's own) do not
> respect all of these restrictions.

> gfm[leaf-blocks.html-blocks.original-markdown-rule-is-more-liberal-about-blank-lines]
> There is one respect, however, in which Gruber's rule is more liberal
> than the one given here, since it allows blank lines to occur inside
> an HTML block.  There are two reasons for disallowing them here.
> First, it removes the need to parse balanced tags, which is
> expensive and can require backtracking from the end of the document
> if no matching end tag is found. Second, it provides a very simple
> and flexible way of including Markdown content inside HTML tags:
> simply separate the Markdown from the HTML using blank lines:
>
> Compare:
>
> ```````````````````````````````` example
> <div>
>
> *Emphasized* text.
>
> </div>
> .
> <div>
> <p><em>Emphasized</em> text.</p>
> </div>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <div>
> *Emphasized* text.
> </div>
> .
> <div>
> *Emphasized* text.
> </div>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.markdown-1-attribute-convention-is-more-complex]
> Some Markdown implementations have adopted a convention of
> interpreting content inside tags as text if the open tag has
> the attribute `markdown=1`.  The rule given above seems a simpler and
> more elegant way of achieving the same expressive power, which is also
> much simpler to parse.

> gfm[leaf-blocks.html-blocks.pasting-html-with-blank-lines-usually-still-works]
> The main potential drawback is that one can no longer paste HTML
> blocks into Markdown documents with 100% reliability.  However,
> *in most cases* this will work fine, because the blank lines in
> HTML are usually followed by HTML block tags.  For example:
>
> ```````````````````````````````` example
> <table>
>
> <tr>
>
> <td>
> Hi
> </td>
>
> </tr>
>
> </table>
> .
> <table>
> <tr>
> <td>
> Hi
> </td>
> </tr>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.indented-inner-tags-with-blank-lines-can-become-code-blocks]
> There are problems, however, if the inner tags are indented
> *and* separated by spaces, as then they will be interpreted as
> an indented code block:
>
> ```````````````````````````````` example
> <table>
>
>   <tr>
>
>     <td>
>       Hi
>     </td>
>
>   </tr>
>
> </table>
> .
> <table>
>   <tr>
> <pre><code>&lt;td&gt;
>   Hi
> &lt;/td&gt;
> </code></pre>
>   </tr>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.html-blocks.blank-lines-can-usually-be-removed-except-in-pre-blocks]
> Fortunately, blank lines are usually not necessary and can be
> deleted.  The exception is inside `<pre>` tags, but as described
> [above][HTML blocks], raw HTML blocks starting with `<pre>`
> *can* contain blank lines.

## Link reference definitions

> gfm[leaf-blocks.link-reference-definitions.structure]
> A [link reference definition](@) consists of a [link label], a colon,
> a [link destination], and an optional [link title].

> gfm[leaf-blocks.link-reference-definitions.label-indent-up-to-three-spaces]
> The [link label] in a link reference definition may be indented up to
> three spaces.

> gfm[leaf-blocks.link-reference-definitions.label-followed-by-colon]
> The [link label] in a link reference definition is followed by a
> colon (`:`).

> gfm[leaf-blocks.link-reference-definitions.destination-allows-leading-whitespace]
> The [link destination] in a link reference definition may be preceded
> by optional [whitespace], including up to one [line ending].

> gfm[leaf-blocks.link-reference-definitions.title-allows-leading-whitespace]
> The optional [link title] in a link reference definition may be
> preceded by optional [whitespace], including up to one [line ending].

> gfm[leaf-blocks.link-reference-definitions.title-separated-from-destination-by-whitespace]
> If a [link title] is present, it is separated from the
> [link destination] by [whitespace].

> gfm[leaf-blocks.link-reference-definitions.no-non-whitespace-after-definition]
> No further [non-whitespace characters] may occur on the line.

> gfm[leaf-blocks.link-reference-definitions.defines-label-for-reference-links-and-images]
> A [link reference definition]
> does not correspond to a structural element of a document.  Instead, it
> defines a label which can be used in [reference links]
> and reference-style [images] elsewhere in the document.
>
> ```````````````````````````````` example
> [foo]: /url "title"
>
> [foo]
> .
> <p><a href="/url" title="title">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
>    [foo]: 
>       /url  
>            'the title'  
>
> [foo]
> .
> <p><a href="/url" title="the title">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [Foo*bar\]]:my_(url) 'title (with parens)'
>
> [Foo*bar\]]
> .
> <p><a href="my_(url)" title="title (with parens)">Foo*bar]</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [Foo bar]:
> <my url>
> 'title'
>
> [Foo bar]
> .
> <p><a href="my%20url" title="title">Foo bar</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.may-appear-before-or-after-uses]
> [Link reference definitions] can come either before or after the
> links that use them.

> gfm[leaf-blocks.link-reference-definitions.title-may-extend-over-multiple-lines]
> The title may extend over multiple lines:
>
> ```````````````````````````````` example
> [foo]: /url '
> title
> line1
> line2
> '
>
> [foo]
> .
> <p><a href="/url" title="
> title
> line1
> line2
> ">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.it-may-not-contain-blank-line]
> However, it may not contain a [blank line]:
>
> ```````````````````````````````` example
> [foo]: /url 'title
>
> with blank line'
>
> [foo]
> .
> <p>[foo]: /url 'title</p>
> <p>with blank line'</p>
> <p>[foo]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.title-may-be-omitted]
> The title may be omitted:
>
> ```````````````````````````````` example
> [foo]:
> /url
>
> [foo]
> .
> <p><a href="/url">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.link-destination-may-not-omitted]
> The link destination may not be omitted:
>
> ```````````````````````````````` example
> [foo]:
>
> [foo]
> .
> <p>[foo]:</p>
> <p>[foo]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.empty-destination-may-use-angle-brackets]
>  However, an empty link destination may be specified using
>  angle brackets:
>
> ```````````````````````````````` example
> [foo]: <>
>
> [foo]
> .
> <p><a href="">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.title-must-be-separated-by-whitespace]
> The title must be separated from the link destination by
> whitespace:
>
> ```````````````````````````````` example
> [foo]: <bar>(baz)
>
> [foo]
> .
> <p>[foo]: <bar>(baz)</p>
> <p>[foo]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.destination-and-title-allow-escapes]
> Both title and destination can contain backslash escapes
> and literal backslashes:
>
> ```````````````````````````````` example
> [foo]: /url\bar\*baz "foo\"bar\baz"
>
> [foo]
> .
> <p><a href="/url%5Cbar*baz" title="foo&quot;bar\baz">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.use-may-precede-definition]
> A link can come before its corresponding definition:
>
> ```````````````````````````````` example
> [foo]
>
> [foo]: url
> .
> <p><a href="url">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.first-matching-definition-takes-precedence]
> If there are several matching definitions, the first one takes
> precedence:
>
> ```````````````````````````````` example
> [foo]
>
> [foo]: first
> [foo]: second
> .
> <p><a href="first">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.label-matching-is-case-insensitive]
> As noted in the section on [Links], matching of labels is
> case-insensitive (see [matches]).
>
> ```````````````````````````````` example
> [FOO]: /url
>
> [Foo]
> .
> <p><a href="/url">Foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [ΑΓΩ]: /φου
>
> [αγω]
> .
> <p><a href="/%CF%86%CE%BF%CF%85">αγω</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.unused-definitions-have-no-visible-output]
> Here is a link reference definition with no corresponding link.
> It contributes nothing to the document.
>
> ```````````````````````````````` example
> [foo]: /url
> .
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.multiline-label-form-does-not-define-reference]
> Here is another one:
>
> ```````````````````````````````` example
> [
> foo
> ]: /url
> bar
> .
> <p>bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.trailing-non-whitespace-invalidates-definition]
> This is not a link reference definition, because there are
> [non-whitespace characters] after the title:
>
> ```````````````````````````````` example
> [foo]: /url "title" ok
> .
> <p>[foo]: /url &quot;title&quot; ok</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.following-line-does-not-become-title]
> This is a link reference definition, but it has no title:
>
> ```````````````````````````````` example
> [foo]: /url
> "title" ok
> .
> <p>&quot;title&quot; ok</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.indent-four-spaces-prevents-definition]
> This is not a link reference definition, because it is indented
> four spaces:
>
> ```````````````````````````````` example
>     [foo]: /url "title"
>
> [foo]
> .
> <pre><code>[foo]: /url &quot;title&quot;
> </code></pre>
> <p>[foo]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.code-block-content-does-not-define-reference]
> This is not a link reference definition, because it occurs inside
> a code block:
>
> ```````````````````````````````` example
> ```
> [foo]: /url
> ```
>
> [foo]
> .
> <pre><code>[foo]: /url
> </code></pre>
> <p>[foo]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.cannot-interrupt-paragraphs]
> A [link reference definition] cannot interrupt a paragraph.
>
> ```````````````````````````````` example
> Foo
> [bar]: /baz
>
> [bar]
> .
> <p>Foo
> [bar]: /baz</p>
> <p>[bar]</p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.may-follow-other-blocks-without-blank-line]
> However, it can directly follow other block elements, such as headings
> and thematic breaks, and it need not be followed by a blank line.
>
> ```````````````````````````````` example
> # [Foo]
> [foo]: /url
> > bar
> .
> <h1><a href="/url">Foo</a></h1>
> <blockquote>
> <p>bar</p>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo]: /url
> bar
> ===
> [foo]
> .
> <h1>bar</h1>
> <p><a href="/url">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo]: /url
> ===
> [foo]
> .
> <p>===
> <a href="/url">foo</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.multiple-definitions-may-appear-consecutively]
> Several [link reference definitions]
> can occur one after another, without intervening blank lines.
>
> ```````````````````````````````` example
> [foo]: /foo-url "foo"
> [bar]: /bar-url
>   "bar"
> [baz]: /baz-url
>
> [foo],
> [bar],
> [baz]
> .
> <p><a href="/foo-url" title="foo">foo</a>,
> <a href="/bar-url" title="bar">bar</a>,
> <a href="/baz-url">baz</a></p>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.may-occur-inside-block-containers]
> [Link reference definitions] can occur
> inside block containers, like lists and block quotations.  They
> affect the entire document, not just the container in which they
> are defined:
>
> ```````````````````````````````` example
> [foo]
>
> > [foo]: /url
> .
> <p><a href="/url">foo</a></p>
> <blockquote>
> </blockquote>
> ````````````````````````````````

> gfm[leaf-blocks.link-reference-definitions.scope-is-the-entire-document]
> Link reference definitions affect the entire document, not just the
> container in which they are defined.

> gfm[leaf-blocks.link-reference-definitions.definition-status-is-independent-of-use]
> Whether something is a [link reference definition] is
> independent of whether the link reference it defines is
> used in the document.  Thus, for example, the following
> document contains just a link reference definition, and
> no visible content:
>
> ```````````````````````````````` example
> [foo]: /url
> .
> ````````````````````````````````

## Paragraphs

> gfm[leaf-blocks.paragraphs.structure]
> A sequence of non-blank lines that cannot be interpreted as other
> kinds of blocks forms a [paragraph](@).

> gfm[leaf-blocks.paragraphs.contents-parse-as-inlines]
> The contents of the paragraph are the result of parsing the
> paragraph's raw content as inlines.

> gfm[leaf-blocks.paragraphs.raw-content-concatenates-lines-and-trims-edge-whitespace]
> The paragraph's raw content is formed by concatenating the lines and
> removing initial and final [whitespace].

> gfm[leaf-blocks.paragraphs.blank-line-separates-paragraphs]
> A simple example with two paragraphs:
>
> ```````````````````````````````` example
> aaa
>
> bbb
> .
> <p>aaa</p>
> <p>bbb</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.may-span-multiple-lines-without-blank-lines]
> Paragraphs can contain multiple lines, but no blank lines:
>
> ```````````````````````````````` example
> aaa
> bbb
>
> ccc
> ddd
> .
> <p>aaa
> bbb</p>
> <p>ccc
> ddd</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.multiple-blank-lines-between-paragraphs-have-no-effect]
> Multiple blank lines between paragraph have no effect:
>
> ```````````````````````````````` example
> aaa
>
>
> bbb
> .
> <p>aaa</p>
> <p>bbb</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.leading-spaces-are-ignored]
> Leading spaces are skipped:
>
> ```````````````````````````````` example
>   aaa
>  bbb
> .
> <p>aaa
> bbb</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.lines-after-first-may-be-indented-arbitrarily]
> Lines after the first may be indented any amount, since indented
> code blocks cannot interrupt paragraphs.
>
> ```````````````````````````````` example
> aaa
>              bbb
>                                        ccc
> .
> <p>aaa
> bbb
> ccc</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.first-line-indent-up-to-three-spaces]
> However, the first line may be indented at most three spaces,
> or an indented code block will be triggered:
>
> ```````````````````````````````` example
>    aaa
> bbb
> .
> <p>aaa
> bbb</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
>     aaa
> bbb
> .
> <pre><code>aaa
> </code></pre>
> <p>bbb</p>
> ````````````````````````````````

> gfm[leaf-blocks.paragraphs.trailing-spaces-are-stripped-before-inline-parsing]
> Final spaces are stripped before inline parsing, so a paragraph
> that ends with two or more spaces will not end with a [hard line
> break]:
>
> ```````````````````````````````` example
> aaa     
> bbb     
> .
> <p>aaa<br />
> bbb</p>
> ````````````````````````````````

## Blank lines

> gfm[leaf-blocks.blank-lines.blank-lines-definition]
> [Blank lines] between block-level elements are ignored,
> except for the role they play in determining whether a [list]
> is [tight] or [loose].

> gfm[leaf-blocks.blank-lines.blank-lines-at-beginning-end-document-are-also-ignored]
> Blank lines at the beginning and end of the document are also ignored.
>
> ```````````````````````````````` example
>   
>
> aaa
>   
>
> # aaa
>
>   
> .
> <p>aaa</p>
> <h1>aaa</h1>
> ````````````````````````````````

> gfm[leaf-blocks.blank-lines.extension-wrapper]
> <div class="extension">

## Tables (extension)

> gfm[leaf-blocks.tables-extension.gfm-enables-table-extension]
> GFM enables the `table` extension, where an additional leaf block type is
> available.

> gfm[leaf-blocks.tables-extension.table-definition]
> A [table](@) is an arrangement of data with rows and columns, consisting of a
> single header row, a [delimiter row] separating the header from the data, and
> zero or more data rows.

> gfm[leaf-blocks.tables-extension.rows-consist-of-pipe-separated-cells]
> Each row consists of cells containing arbitrary text, in which [inlines] are
> parsed, separated by pipes (`|`).

> gfm[leaf-blocks.tables-extension.leading-and-trailing-pipes-are-recommended-for-clarity]
> A leading and trailing pipe is recommended for clarity of reading, and if
> there's otherwise parsing ambiguity.

> gfm[leaf-blocks.tables-extension.spaces-around-cell-content-are-trimmed]
> Spaces between pipes and cell content are trimmed.

> gfm[leaf-blocks.tables-extension.block-level-elements-cannot-appear-in-tables]
> Block-level elements cannot be inserted in a table.

> gfm[leaf-blocks.tables-extension.delimiter-row-structure]
> The [delimiter row](@) consists of cells whose only content are hyphens (`-`),
> and optionally, a leading or trailing colon (`:`), or both, to indicate left,
> right, or center alignment respectively.
>
> ```````````````````````````````` example table
> | foo | bar |
> | --- | --- |
> | baz | bim |
> .
> <table>
> <thead>
> <tr>
> <th>foo</th>
> <th>bar</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td>baz</td>
> <td>bim</td>
> </tr>
> </tbody>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.column-widths-and-edge-pipes-need-not-be-consistent]
> Cells in one column don't need to match length, though it's easier to read if
> they are. Likewise, use of leading and trailing pipes may be inconsistent:
>
> ```````````````````````````````` example table
> | abc | defghi |
> :-: | -----------:
> bar | baz
> .
> <table>
> <thead>
> <tr>
> <th align="center">abc</th>
> <th align="right">defghi</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td align="center">bar</td>
> <td align="right">baz</td>
> </tr>
> </tbody>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.escaped-pipes-are-literal-cell-content]
> Include a pipe in a cell's content by escaping it, including inside other
> inline spans:
>
> ```````````````````````````````` example table
> | f\|oo  |
> | ------ |
> | b `\|` az |
> | b **\|** im |
> .
> <table>
> <thead>
> <tr>
> <th>f|oo</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td>b <code>|</code> az</td>
> </tr>
> <tr>
> <td>b <strong>|</strong> im</td>
> </tr>
> </tbody>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.table-ends-at-blank-line-or-other-block]
> The table is broken at the first empty line, or beginning of another
> block-level structure:
>
> ```````````````````````````````` example table
> | abc | def |
> | --- | --- |
> | bar | baz |
> > bar
> .
> <table>
> <thead>
> <tr>
> <th>abc</th>
> <th>def</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td>bar</td>
> <td>baz</td>
> </tr>
> </tbody>
> </table>
> <blockquote>
> <p>bar</p>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example table
> | abc | def |
> | --- | --- |
> | bar | baz |
> bar
>
> bar
> .
> <table>
> <thead>
> <tr>
> <th>abc</th>
> <th>def</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td>bar</td>
> <td>baz</td>
> </tr>
> <tr>
> <td>bar</td>
> <td></td>
> </tr>
> </tbody>
> </table>
> <p>bar</p>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.header-and-delimiter-row-must-have-matching-cell-count]
> The header row must match the [delimiter row] in the number of cells.  If not,
> a table will not be recognized:
>
> ```````````````````````````````` example table
> | abc | def |
> | --- |
> | bar |
> .
> <p>| abc | def |
> | --- |
> | bar |</p>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.body-rows-may-have-fewer-or-more-cells-than-header]
> The remainder of the table's rows may vary in the number of cells.  If there
> are a number of cells fewer than the number of cells in the header row, empty
> cells are inserted.  If there are greater, the excess is ignored:
>
> ```````````````````````````````` example table
> | abc | def |
> | --- | --- |
> | bar |
> | bar | baz | boo |
> .
> <table>
> <thead>
> <tr>
> <th>abc</th>
> <th>def</th>
> </tr>
> </thead>
> <tbody>
> <tr>
> <td>bar</td>
> <td></td>
> </tr>
> <tr>
> <td>bar</td>
> <td>baz</td>
> </tr>
> </tbody>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.no-tbody-generated-when-there-are-no-body-rows]
> If there are no rows in the body, no `<tbody>` is generated in HTML output:
>
> ```````````````````````````````` example table
> | abc | def |
> | --- | --- |
> .
> <table>
> <thead>
> <tr>
> <th>abc</th>
> <th>def</th>
> </tr>
> </thead>
> </table>
> ````````````````````````````````

> gfm[leaf-blocks.tables-extension.extension-wrapper]
> </div>

# Container blocks

> gfm[container-blocks.definition]
> A [container block](#container-blocks) is a block that has other
> blocks as its contents.

> gfm[container-blocks.basic-kinds-are-block-quotes-and-list-items]
> There are two basic kinds of container blocks:
> [block quotes] and [list items].

> gfm[container-blocks.lists-are-meta-containers-for-list-items]
> [Lists] are meta-containers for [list items].

> gfm[container-blocks.syntax-is-defined-recursively]
> We define the syntax for container blocks recursively.  The general
> form of the definition is:

> gfm[container-blocks.general-recursive-definition-form]
> > If X is a sequence of blocks, then the result of
> > transforming X in such-and-such a way is a container of type Y
> > with these blocks as its content.

> gfm[container-blocks.generated-from-contents-rather-than-parse-recipe]
> So, we explain what counts as a block quote or list item by explaining
> how these can be *generated* from their contents. This should suffice
> to define the syntax, although it does not give a recipe for *parsing*
> these constructions.  (A recipe is provided below in the section entitled
> [A parsing strategy](#appendix-a-parsing-strategy).)

## Block quotes

> gfm[container-blocks.block-quotes.marker-structure]
> A [block quote marker](@)
> consists of 0-3 spaces of initial indent, plus (a) the character `>` together
> with a following space, or (b) a single character `>` not followed by a space.

> gfm[container-blocks.block-quotes.basic-case]
> If a string of lines *Ls* constitute a sequence of blocks *Bs*, then
> the result of prepending a [block quote marker] to the beginning of
> each line in *Ls* is a [block quote](#block-quotes) containing *Bs*.

> gfm[container-blocks.block-quotes.laziness]
> If a string of lines *Ls* constitute a [block quote](#block-quotes)
> with contents *Bs*, then the result of deleting the initial [block
> quote marker] from one or more lines in which the next
> [non-whitespace character] after the [block quote marker] is
> [paragraph continuation text] is a block quote with *Bs* as its
> content.

> gfm[container-blocks.block-quotes.paragraph-continuation-text-definition]
> [Paragraph continuation text](@) is text that will be parsed as part
> of the content of a paragraph, but does not occur at the beginning of
> the paragraph.

> gfm[container-blocks.block-quotes.consecutiveness]
> A document cannot contain two [block quotes] in a row unless there is
> a [blank line] between them.

> gfm[container-blocks.block-quotes.only-defined-cases-count]
> Nothing else counts as a [block quote](#block-quotes).

> gfm[container-blocks.block-quotes.example]
> Here is a simple example:
>
> ```````````````````````````````` example
> > # Foo
> > bar
> > baz
> .
> <blockquote>
> <h1>Foo</h1>
> <p>bar
> baz</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.space-after-marker-is-optional]
> The spaces after the `>` characters can be omitted:
>
> ```````````````````````````````` example
> ># Foo
> >bar
> > baz
> .
> <blockquote>
> <h1>Foo</h1>
> <p>bar
> baz</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.marker-indent-up-to-three-spaces]
> The `>` characters can be indented 1-3 spaces:
>
> ```````````````````````````````` example
>    > # Foo
>    > bar
>  > baz
> .
> <blockquote>
> <h1>Foo</h1>
> <p>bar
> baz</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.marker-indent-four-spaces-starts-code-block]
> Four spaces gives us a code block:
>
> ```````````````````````````````` example
>     > # Foo
>     > bar
>     > baz
> .
> <pre><code>&gt; # Foo
> &gt; bar
> &gt; baz
> </code></pre>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.laziness-allows-omitted-continuation-markers]
> The Laziness clause allows us to omit the `>` before
> [paragraph continuation text]:
>
> ```````````````````````````````` example
> > # Foo
> > bar
> baz
> .
> <blockquote>
> <h1>Foo</h1>
> <p>bar
> baz</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.may-mix-lazy-and-non-lazy-continuation-lines]
> A block quote can contain some lazy and some non-lazy
> continuation lines:
>
> ```````````````````````````````` example
> > bar
> baz
> > foo
> .
> <blockquote>
> <p>bar
> baz
> foo</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.laziness-applies-only-to-paragraph-continuation-lines]
> Laziness only applies to lines that would have been continuations of
> paragraphs had they been prepended with [block quote markers].
> For example, the `> ` cannot be omitted in the second line of
>
> ``` markdown
> > foo
> > ---
> ```

> gfm[container-blocks.block-quotes.omitting-marker-before-setext-underline-changes-meaning]
> without changing the meaning:
>
> ```````````````````````````````` example
> > foo
> ---
> .
> <blockquote>
> <p>foo</p>
> </blockquote>
> <hr />
> ````````````````````````````````

> gfm[container-blocks.block-quotes.omitting-marker-before-list-item-ends-block-quote]
> Similarly, if we omit the `> ` in the second line of
>
> ``` markdown
> > - foo
> > - bar
> ```

> gfm[container-blocks.block-quotes.list-example-block-quote-ends-after-first-line]
> then the block quote ends after the first line:
>
> ```````````````````````````````` example
> > - foo
> - bar
> .
> <blockquote>
> <ul>
> <li>foo</li>
> </ul>
> </blockquote>
> <ul>
> <li>bar</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.laziness-does-not-apply-to-code-block-lines]
> For the same reason, we can't omit the `> ` in front of
> subsequent lines of an indented or fenced code block:
>
> ```````````````````````````````` example
> >     foo
>     bar
> .
> <blockquote>
> <pre><code>foo
> </code></pre>
> </blockquote>
> <pre><code>bar
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> > ```
> foo
> ```
> .
> <blockquote>
> <pre><code></code></pre>
> </blockquote>
> <p>foo</p>
> <pre><code></code></pre>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.lazy-continuation-example]
> Note that in the following case, we have a [lazy
> continuation line]:
>
> ```````````````````````````````` example
> > foo
>     - bar
> .
> <blockquote>
> <p>foo
> - bar</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.lazy-continuation-list-example-explanation]
> To see why, note that in
>
> ```markdown
> > foo
> >     - bar
> ```

> gfm[container-blocks.block-quotes.indented-dash-remains-paragraph-continuation-text]
> the `- bar` is indented too far to start a list, and can't
> be an indented code block because indented code blocks cannot
> interrupt paragraphs, so it is [paragraph continuation text].

> gfm[container-blocks.block-quotes.can-be-empty]
> A block quote can be empty:
>
> ```````````````````````````````` example
> >
> .
> <blockquote>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> >
> >  
> > 
> .
> <blockquote>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.can-have-initial-or-final-blank-lines]
> A block quote can have initial or final blank lines:
>
> ```````````````````````````````` example
> >
> > foo
> >  
> .
> <blockquote>
> <p>foo</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.blank-line-separates-adjacent-block-quotes]
> A blank line always separates block quotes:
>
> ```````````````````````````````` example
> > foo
>
> > bar
> .
> <blockquote>
> <p>foo</p>
> </blockquote>
> <blockquote>
> <p>bar</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.consecutiveness-rationale]
> (Most current Markdown implementations, including John Gruber's
> original `Markdown.pl`, will parse this example as a single block quote
> with two paragraphs.  But it seems better to allow the author to decide
> whether two block quotes or one are wanted.)

> gfm[container-blocks.block-quotes.adjacent-lines-form-single-block-quote]
> Consecutiveness means that if we put these block quotes together,
> we get a single block quote:
>
> ```````````````````````````````` example
> > foo
> > bar
> .
> <blockquote>
> <p>foo
> bar</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.blank-quoted-line-creates-two-paragraphs]
> To get a block quote with two paragraphs, use:
>
> ```````````````````````````````` example
> > foo
> >
> > bar
> .
> <blockquote>
> <p>foo</p>
> <p>bar</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.can-interrupt-paragraphs]
> Block quotes can interrupt paragraphs:
>
> ```````````````````````````````` example
> foo
> > bar
> .
> <p>foo</p>
> <blockquote>
> <p>bar</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.blank-lines-generally-not-required-around-block-quotes]
> In general, blank lines are not needed before or after block
> quotes:
>
> ```````````````````````````````` example
> > aaa
> ***
> > bbb
> .
> <blockquote>
> <p>aaa</p>
> </blockquote>
> <hr />
> <blockquote>
> <p>bbb</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.blank-line-required-before-following-paragraph]
> However, because of laziness, a blank line is needed between
> a block quote and a following paragraph:
>
> ```````````````````````````````` example
> > bar
> baz
> .
> <blockquote>
> <p>bar
> baz</p>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> > bar
>
> baz
> .
> <blockquote>
> <p>bar</p>
> </blockquote>
> <p>baz</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> > bar
> >
> baz
> .
> <blockquote>
> <p>bar</p>
> </blockquote>
> <p>baz</p>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.nested-block-quotes-may-omit-continuation-markers]
> It is a consequence of the Laziness rule that any number
> of initial `>`s may be omitted on a continuation line of a
> nested block quote:
>
> ```````````````````````````````` example
> > > > foo
> bar
> .
> <blockquote>
> <blockquote>
> <blockquote>
> <p>foo
> bar</p>
> </blockquote>
> </blockquote>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> >>> foo
> > bar
> >>baz
> .
> <blockquote>
> <blockquote>
> <blockquote>
> <p>foo
> bar
> baz</p>
> </blockquote>
> </blockquote>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.block-quotes.indented-code-needs-five-spaces-after-marker]
> When including an indented code block in a block quote,
> remember that the [block quote marker] includes
> both the `>` and a following space.  So *five spaces* are needed after
> the `>`:
>
> ```````````````````````````````` example
> >     code
>
> >    not code
> .
> <blockquote>
> <pre><code>code
> </code></pre>
> </blockquote>
> <blockquote>
> <p>not code</p>
> </blockquote>
> ````````````````````````````````

## List items

> gfm[container-blocks.list-items.marker-definition]
> A [list marker](@) is a
> [bullet list marker] or an [ordered list marker].

> gfm[container-blocks.list-items.bullet-marker-definition]
> A [bullet list marker](@)
> is a `-`, `+`, or `*` character.

> gfm[container-blocks.list-items.ordered-marker-definition]
> An [ordered list marker](@)
> is a sequence of 1--9 arabic digits (`0-9`), followed by either a
> `.` character or a `)` character.  (The reason for the length
> limit is that with 10 digits we start seeing integer overflows
> in some browsers.)

> gfm[container-blocks.list-items.basic-case]
> If a sequence of lines *Ls* constitute a sequence of blocks *Bs*
> starting with a [non-whitespace character], and *M* is a list marker
> of width *W* followed by 1 <= *N* <= 4 spaces, then the result of
> prepending *M* and the following spaces to the first line of *Ls*, and
> indenting subsequent lines of *Ls* by *W + N* spaces, is a list item
> with *Bs* as its contents.

> gfm[container-blocks.list-items.type-is-determined-by-marker]
> The type of the list item (bullet or ordered) is determined by the
> type of its list marker.

> gfm[container-blocks.list-items.ordered-item-start-number-comes-from-marker]
> If the list item is ordered, then it is also assigned a start number,
> based on the ordered list marker.

> gfm[container-blocks.list-items.interrupting-first-item-cannot-begin-with-blank-line]
> When the first list item in a [list] interrupts a paragraph, the
> lines *Ls* must not begin with a blank line.

> gfm[container-blocks.list-items.interrupting-ordered-list-must-start-at-one]
> When the first list item in a [list] interrupts a paragraph and the
> list item is ordered, the start number must be 1.

> gfm[container-blocks.list-items.thematic-break-line-is-not-a-list-item]
> If any line is a [thematic break][thematic breaks] then that line is
> not a list item.

> gfm[container-blocks.list-items.basic-case-example-source-blocks]
> For example, let *Ls* be the lines
>
> ```````````````````````````````` example
> A paragraph
> with two lines.
>
>     indented code
>
> > A block quote.
> .
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.list-items.basic-case-example-ordered-item]
> And let *M* be the marker `1.`, and *N* = 2.  Then rule #1 says
> that the following is an ordered list item with start number 1,
> and the same contents as *Ls*:
>
> ```````````````````````````````` example
> 1.  A paragraph
>     with two lines.
>
>         indented code
>
>     > A block quote.
> .
> <ol>
> <li>
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.required-indentation-is-marker-width-plus-padding]
> The most important thing to notice is that the position of
> the text after the list marker determines how much indentation
> is needed in subsequent blocks in the list item.  If the list
> marker takes up two spaces, and there are three spaces between
> the list marker and the next [non-whitespace character], then blocks
> must be indented five spaces in order to fall under the list
> item.

> gfm[container-blocks.list-items.examples-of-required-indentation]
> Here are some examples showing how far content must be indented to be
> put under the list item:
>
> ```````````````````````````````` example
> - one
>
>  two
> .
> <ul>
> <li>one</li>
> </ul>
> <p>two</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - one
>
>   two
> .
> <ul>
> <li>
> <p>one</p>
> <p>two</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
>  -    one
>
>      two
> .
> <ul>
> <li>one</li>
> </ul>
> <pre><code> two
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
>  -    one
>
>       two
> .
> <ul>
> <li>
> <p>one</p>
> <p>two</p>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.required-indentation-is-relative-not-column-based]
> It is tempting to think of this in terms of columns:  the continuation
> blocks must be indented at least to the column of the first
> [non-whitespace character] after the list marker. However, that is not quite right.
> The spaces after the list marker determine how much relative indentation
> is needed.  Which column this indentation reaches will depend on
> how the list item is embedded in other constructions, as shown by
> this example:
>
> ```````````````````````````````` example
>    > > 1.  one
> >>
> >>     two
> .
> <blockquote>
> <blockquote>
> <ol>
> <li>
> <p>one</p>
> <p>two</p>
> </li>
> </ol>
> </blockquote>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.list-items.same-column-can-still-be-inside-item-in-nested-context]
> Here `two` occurs in the same column as the list marker `1.`,
> but is actually contained in the list item, because there is
> sufficient indentation after the last containing blockquote marker.

> gfm[container-blocks.list-items.far-right-text-may-still-be-outside-item]
> The converse is also possible.  In the following example, the word `two`
> occurs far to the right of the initial text of the list item, `one`, but
> it is not considered part of the list item, because it is not indented
> far enough past the blockquote marker:
>
> ```````````````````````````````` example
> >>- one
> >>
>   >  > two
> .
> <blockquote>
> <blockquote>
> <ul>
> <li>one</li>
> </ul>
> <p>two</p>
> </blockquote>
> </blockquote>
> ````````````````````````````````

> gfm[container-blocks.list-items.at-least-one-space-required-after-marker]
> Note that at least one space is needed between the list marker and
> any following content, so these are not list items:
>
> ```````````````````````````````` example
> -one
>
> 2.two
> .
> <p>-one</p>
> <p>2.two</p>
> ````````````````````````````````

> gfm[container-blocks.list-items.item-blocks-may-be-separated-by-multiple-blank-lines]
> A list item may contain blocks that are separated by more than
> one blank line.
>
> ```````````````````````````````` example
> - foo
>
>
>   bar
> .
> <ul>
> <li>
> <p>foo</p>
> <p>bar</p>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.may-contain-any-kind-of-block]
> A list item may contain any kind of block:
>
> ```````````````````````````````` example
> 1.  foo
>
>     ```
>     bar
>     ```
>
>     baz
>
>     > bam
> .
> <ol>
> <li>
> <p>foo</p>
> <pre><code>bar
> </code></pre>
> <p>baz</p>
> <blockquote>
> <p>bam</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.indented-code-inside-item-preserves-empty-lines]
> A list item that contains an indented code block will preserve
> empty lines within the code block verbatim.
>
> ```````````````````````````````` example
> - Foo
>
>       bar
>
>
>       baz
> .
> <ul>
> <li>
> <p>Foo</p>
> <pre><code>bar
>
>
> baz
> </code></pre>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.ordered-start-number-is-at-most-nine-digits]
> Note that ordered list start numbers must be nine digits or less:
>
> ```````````````````````````````` example
> 123456789. ok
> .
> <ol start="123456789">
> <li>ok</li>
> </ol>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1234567890. not ok
> .
> <p>1234567890. not ok</p>
> ````````````````````````````````

> gfm[container-blocks.list-items.ordered-start-number-may-have-leading-zeroes]
> A start number may begin with 0s:
>
> ```````````````````````````````` example
> 0. ok
> .
> <ol start="0">
> <li>ok</li>
> </ol>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 003. ok
> .
> <ol start="3">
> <li>ok</li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.ordered-start-number-cannot-be-negative]
> A start number may not be negative:
>
> ```````````````````````````````` example
> -1. not ok
> .
> <p>-1. not ok</p>
> ````````````````````````````````
>
> gfm[container-blocks.list-items.item-starting-with-indented-code]
> If a sequence of lines *Ls* constitute a sequence of blocks *Bs*
> starting with an indented code block, and *M* is a list marker of
> width *W* followed by one space, then the result of prepending *M*
> and the following space to the first line of *Ls*, and indenting
> subsequent lines of *Ls* by *W + 1* spaces, is a list item with *Bs*
> as its contents.

> gfm[container-blocks.list-items.empty-lines-inside-list-item-need-not-be-indented]
> If a line in a list item is empty, then it need not be indented.

> gfm[container-blocks.list-items.indented-code-must-be-four-spaces-beyond-content-indent]
> An indented code block will have to be indented four spaces beyond
> the edge of the region where text will be included in the list item.
> In the following case that is 6 spaces:
>
> ```````````````````````````````` example
> - foo
>
>       bar
> .
> <ul>
> <li>
> <p>foo</p>
> <pre><code>bar
> </code></pre>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.indented-code-example-with-wider-marker]
> And in this case it is 11 spaces:
>
> ```````````````````````````````` example
>   10.  foo
>
>            bar
> .
> <ol start="10">
> <li>
> <p>foo</p>
> <pre><code>bar
> </code></pre>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.indented-code-start-uses-one-space-after-marker]
> If the *first* block in the list item is an indented code block,
> then by rule #2, the contents must be indented *one* space after the
> list marker:
>
> ```````````````````````````````` example
>     indented code
>
> paragraph
>
>     more code
> .
> <pre><code>indented code
> </code></pre>
> <p>paragraph</p>
> <pre><code>more code
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1.     indented code
>
>    paragraph
>
>        more code
> .
> <ol>
> <li>
> <pre><code>indented code
> </code></pre>
> <p>paragraph</p>
> <pre><code>more code
> </code></pre>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.extra-indent-counts-as-code-content]
> Note that an additional space indent is interpreted as space
> inside the code block:
>
> ```````````````````````````````` example
> 1.      indented code
>
>    paragraph
>
>        more code
> .
> <ol>
> <li>
> <pre><code> indented code
> </code></pre>
> <p>paragraph</p>
> <pre><code>more code
> </code></pre>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.basic-rules-apply-only-to-nonblank-or-code-start]
> Note that rules #1 and #2 only apply to two cases:  (a) cases
> in which the lines to be included in a list item begin with a
> [non-whitespace character], and (b) cases in which
> they begin with an indented code
> block.  In a case like the following, where the first block begins with
> a three-space indent, the rules do not allow us to form a list item by
> indenting the whole thing and prepending a list marker:
>
> ```````````````````````````````` example
>    foo
>
> bar
> .
> <p>foo</p>
> <p>bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> -    foo
>
>   bar
> .
> <ul>
> <li>foo</li>
> </ul>
> <p>bar</p>
> ````````````````````````````````

> gfm[container-blocks.list-items.leading-indent-up-to-three-spaces-can-be-normalized]
> This is not a significant restriction, because when a block begins
> with 1-3 spaces indent, the indentation can always be removed without
> a change in interpretation, allowing rule #1 to be applied.  So, in
> the above case:
>
> ```````````````````````````````` example
> -  foo
>
>    bar
> .
> <ul>
> <li>
> <p>foo</p>
> <p>bar</p>
> </li>
> </ul>
> ````````````````````````````````
>
> gfm[container-blocks.list-items.item-starting-with-blank-line]
> If a sequence of lines *Ls* starting with a single [blank line]
> constitute a (possibly empty) sequence of blocks *Bs*, not separated
> from each other by more than one blank line, and *M* is a list marker
> of width *W*, then the result of prepending *M* to the first line of
> *Ls*, and indenting subsequent lines of *Ls* by *W + 1* spaces, is a
> list item with *Bs* as its contents.

> gfm[container-blocks.list-items.items-may-start-with-one-blank-line-and-still-be-nonempty]
> Here are some list items that start with a blank line but are not empty:
>
> ```````````````````````````````` example
> -
>   foo
> -
>   ```
>   bar
>   ```
> -
>       baz
> .
> <ul>
> <li>foo</li>
> <li>
> <pre><code>bar
> </code></pre>
> </li>
> <li>
> <pre><code>baz
> </code></pre>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.blank-line-start-makes-post-marker-spacing-irrelevant]
> When the list item starts with a blank line, the number of spaces
> following the list marker doesn't change the required indentation:
>
> ```````````````````````````````` example
> -   
>   foo
> .
> <ul>
> <li>foo</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.may-begin-with-at-most-one-blank-line]
> A list item can begin with at most one blank line.
> In the following example, `foo` is not part of the list
> item:
>
> ```````````````````````````````` example
> -
>
>   foo
> .
> <ul>
> <li></li>
> </ul>
> <p>foo</p>
> ````````````````````````````````

> gfm[container-blocks.list-items.empty-bullet-item]
> Here is an empty bullet list item:
>
> ```````````````````````````````` example
> - foo
> -
> - bar
> .
> <ul>
> <li>foo</li>
> <li></li>
> <li>bar</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.empty-item-does-not-require-post-marker-spaces]
> It does not matter whether there are spaces following the [list marker]:
>
> ```````````````````````````````` example
> - foo
> -   
> - bar
> .
> <ul>
> <li>foo</li>
> <li></li>
> <li>bar</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.empty-ordered-item]
> Here is an empty ordered list item:
>
> ```````````````````````````````` example
> 1. foo
> 2.
> 3. bar
> .
> <ol>
> <li>foo</li>
> <li></li>
> <li>bar</li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.list-may-start-or-end-with-empty-item]
> A list may start or end with an empty list item:
>
> ```````````````````````````````` example
> *
> .
> <ul>
> <li></li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.empty-item-cannot-interrupt-paragraph]
> However, an empty list item cannot interrupt a paragraph:
>
> ```````````````````````````````` example
> foo
> *
>
> foo
> 1.
> .
> <p>foo
> *</p>
> <p>foo
> 1.</p>
> ````````````````````````````````
>
> 4.  **Indentation.**  If a sequence of lines *Ls* constitutes a list item
>     according to rule #1, #2, or #3, then the result of indenting each line
>     of *Ls* by 1-3 spaces (the same for each line) also constitutes a
>     list item with the same contents and attributes.  If a line is
>     empty, then it need not be indented.
>
> Indented one space:
>
> ```````````````````````````````` example
>  1.  A paragraph
>      with two lines.
>
>          indented code
>
>      > A block quote.
> .
> <ol>
> <li>
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````
>
> Indented two spaces:
>
> ```````````````````````````````` example
>   1.  A paragraph
>       with two lines.
>
>           indented code
>
>       > A block quote.
> .
> <ol>
> <li>
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````
>
> Indented three spaces:
>
> ```````````````````````````````` example
>    1.  A paragraph
>        with two lines.
>
>            indented code
>
>        > A block quote.
> .
> <ol>
> <li>
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.indenting-whole-item-four-spaces-produces-code-block]
> Four spaces indent gives a code block:
>
> ```````````````````````````````` example
>     1.  A paragraph
>         with two lines.
>
>             indented code
>
>         > A block quote.
> .
> <pre><code>1.  A paragraph
>     with two lines.
>
>         indented code
>
>     &gt; A block quote.
> </code></pre>
> ````````````````````````````````
>
> 5.  **Laziness.**  If a string of lines *Ls* constitute a [list
>     item](#list-items) with contents *Bs*, then the result of deleting
>     some or all of the indentation from one or more lines in which the
>     next [non-whitespace character] after the indentation is
>     [paragraph continuation text] is a
>     list item with the same contents and attributes.  The unindented
>     lines are called
>     [lazy continuation line](@)s.

> gfm[container-blocks.list-items.lazy-continuation-example]
> Here is an example with [lazy continuation lines]:
>
> ```````````````````````````````` example
>   1.  A paragraph
> with two lines.
>
>           indented code
>
>       > A block quote.
> .
> <ol>
> <li>
> <p>A paragraph
> with two lines.</p>
> <pre><code>indented code
> </code></pre>
> <blockquote>
> <p>A block quote.</p>
> </blockquote>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.lazy-continuation-may-delete-only-part-of-indent]
> Indentation can be partially deleted:
>
> ```````````````````````````````` example
>   1.  A paragraph
>     with two lines.
> .
> <ol>
> <li>A paragraph
> with two lines.</li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.laziness-works-inside-nested-structures]
> These examples show how laziness can work in nested structures:
>
> ```````````````````````````````` example
> > 1. > Blockquote
> continued here.
> .
> <blockquote>
> <ol>
> <li>
> <blockquote>
> <p>Blockquote
> continued here.</p>
> </blockquote>
> </li>
> </ol>
> </blockquote>
> ````````````````````````````````
>
> ```````````````````````````````` example
> > 1. > Blockquote
> > continued here.
> .
> <blockquote>
> <ol>
> <li>
> <blockquote>
> <p>Blockquote
> continued here.</p>
> </blockquote>
> </li>
> </ol>
> </blockquote>
> ````````````````````````````````
>
> 6.  **That's all.** Nothing that is not counted as a list item by rules
>     #1--5 counts as a [list item](#list-items).

> gfm[container-blocks.list-items.sublists-follow-general-indentation-rules]
> The rules for sublists follow from the general rules
> [above][List items].  A sublist must be indented the same number
> of spaces a paragraph would need to be in order to be included
> in the list item.

> gfm[container-blocks.list-items.sublist-example-two-space-indent]
> So, in this case we need two spaces indent:
>
> ```````````````````````````````` example
> - foo
>   - bar
>     - baz
>       - boo
> .
> <ul>
> <li>foo
> <ul>
> <li>bar
> <ul>
> <li>baz
> <ul>
> <li>boo</li>
> </ul>
> </li>
> </ul>
> </li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.sublist-example-one-space-is-not-enough]
> One is not enough:
>
> ```````````````````````````````` example
> - foo
>  - bar
>   - baz
>    - boo
> .
> <ul>
> <li>foo</li>
> <li>bar</li>
> <li>baz</li>
> <li>boo</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.sublist-example-indent-increases-with-wider-marker]
> Here we need four, because the list marker is wider:
>
> ```````````````````````````````` example
> 10) foo
>     - bar
> .
> <ol start="10">
> <li>foo
> <ul>
> <li>bar</li>
> </ul>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.sublist-example-three-spaces-not-enough-for-wider-marker]
> Three is not enough:
>
> ```````````````````````````````` example
> 10) foo
>    - bar
> .
> <ol start="10">
> <li>foo</li>
> </ol>
> <ul>
> <li>bar</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.list-items.list-may-be-first-block-in-item]
> A list may be the first block in a list item:
>
> ```````````````````````````````` example
> - - foo
> .
> <ul>
> <li>
> <ul>
> <li>foo</li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1. - 2. foo
> .
> <ol>
> <li>
> <ul>
> <li>
> <ol start="2">
> <li>foo</li>
> </ol>
> </li>
> </ul>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.list-items.item-may-contain-heading]
> A list item can contain a heading:
>
> ```````````````````````````````` example
> - # Foo
> - Bar
>   ---
>   baz
> .
> <ul>
> <li>
> <h1>Foo</h1>
> </li>
> <li>
> <h2>Bar</h2>
> baz</li>
> </ul>
> ````````````````````````````````

### Motivation

> gfm[container-blocks.list-items.motivation.original-markdown-spec-quoted]
> John Gruber's Markdown spec says the following about list items:
>
> 1. "List markers typically start at the left margin, but may be indented
>    by up to three spaces. List markers must be followed by one or more
>    spaces or a tab."
>
> 2. "To make lists look nice, you can wrap items with hanging indents....
>    But if you don't want to, you don't have to."
>
> 3. "List items may consist of multiple paragraphs. Each subsequent
>    paragraph in a list item must be indented by either 4 spaces or one
>    tab."
>
> 4. "It looks nice if you indent every line of the subsequent paragraphs,
>    but here again, Markdown will allow you to be lazy."
>
> 5. "To put a blockquote within a list item, the blockquote's `>`
>    delimiters need to be indented."
>
> 6. "To put a code block within a list item, the code block needs to be
>    indented twice — 8 spaces or two tabs."

> gfm[container-blocks.list-items.motivation.original-rule-implies-four-space-rule]
> These rules specify that a paragraph under a list item must be indented
> four spaces (presumably, from the left margin, rather than the start of
> the list marker, but this is not said), and that code under a list item
> must be indented eight spaces instead of the usual four.  They also say
> that a block quote must be indented, but not by how much; however, the
> example given has four spaces indentation.  Although nothing is said
> about other kinds of block-level content, it is certainly reasonable to
> infer that *all* block elements under a list item, including other
> lists, must be indented four spaces.  This principle has been called the
> *four-space rule*.

> gfm[container-blocks.list-items.motivation.markdown-pl-diverged-from-four-space-rule]
> The four-space rule is clear and principled, and if the reference
> implementation `Markdown.pl` had followed it, it probably would have
> become the standard.  However, `Markdown.pl` allowed paragraphs and
> sublists to start with only two spaces indentation, at least on the
> outer level.  Worse, its behavior was inconsistent: a sublist of an
> outer-level list needed two spaces indentation, but a sublist of this
> sublist needed three spaces.  It is not surprising, then, that different
> implementations of Markdown have developed very different rules for
> determining what comes under a list item.  (Pandoc and python-Markdown,
> for example, stuck with Gruber's syntax description and the four-space
> rule, while discount, redcarpet, marked, PHP Markdown, and others
> followed `Markdown.pl`'s behavior more closely.)

> gfm[container-blocks.list-items.motivation.no-rule-avoids-breaking-all-existing-documents]
> Unfortunately, given the divergences between implementations, there
> is no way to give a spec for list items that will be guaranteed not
> to break any existing documents.  However, the spec given here should
> correctly handle lists formatted with either the four-space rule or
> the more forgiving `Markdown.pl` behavior, provided they are laid out
> in a way that is natural for a human to read.

> gfm[container-blocks.list-items.motivation.marker-width-determines-indentation]
> The strategy here is to let the width and indentation of the list marker
> determine the indentation necessary for blocks to fall under the list
> item, rather than having a fixed and arbitrary number.  The writer can
> think of the body of the list item as a unit which gets indented to the
> right enough to fit the list marker (and any indentation on the list
> marker).  (The laziness rule, #5, then allows continuation lines to be
> unindented if needed.)

> gfm[container-blocks.list-items.motivation.fixed-margin-indentation-is-unintuitive]
> This rule is superior, we claim, to any rule requiring a fixed level of
> indentation from the margin.  The four-space rule is clear but
> unnatural. It is quite unintuitive that
>
> ``` markdown
> - foo
>
>   bar
>
>   - baz
> ```

> gfm[container-blocks.list-items.motivation.four-space-rule-splits-list]
> should be parsed as two lists with an intervening paragraph,
>
> ``` html
> <ul>
> <li>foo</li>
> </ul>
> <p>bar</p>
> <ul>
> <li>baz</li>
> </ul>
> ```

> gfm[container-blocks.list-items.motivation.preferred-parse-is-single-list]
> as the four-space rule demands, rather than a single list,
>
> ``` html
> <ul>
> <li>
> <p>foo</p>
> <p>bar</p>
> <ul>
> <li>baz</li>
> </ul>
> </li>
> </ul>
> ```

> gfm[container-blocks.list-items.motivation.four-space-rule-is-arbitrary]
> The choice of four spaces is arbitrary.  It can be learned, but it is
> not likely to be guessed, and it trips up beginners regularly.

> gfm[container-blocks.list-items.motivation.two-space-rule-allows-underindented-content]
> Would it help to adopt a two-space rule?  The problem is that such
> a rule, together with the rule allowing 1--3 spaces indentation of the
> initial list marker, allows text that is indented *less than* the
> original list marker to be included in the list item. For example,
> `Markdown.pl` parses
>
> ``` markdown
>    - one
>
>   two
> ```

> gfm[container-blocks.list-items.motivation.two-space-rule-example-single-list-item]
> as a single list item, with `two` a continuation paragraph:
>
> ``` html
> <ul>
> <li>
> <p>one</p>
> <p>two</p>
> </li>
> </ul>
> ```

> gfm[container-blocks.list-items.motivation.two-space-rule-example-inside-block-quote]
> and similarly
>
> ``` markdown
> >   - one
> >
> >  two
> ```

> gfm[container-blocks.list-items.motivation.two-space-rule-block-quote-html]
> as
>
> ``` html
> <blockquote>
> <ul>
> <li>
> <p>one</p>
> <p>two</p>
> </li>
> </ul>
> </blockquote>
> ```

> gfm[container-blocks.list-items.motivation.two-space-rule-is-extremely-unintuitive]
> This is extremely unintuitive.

> gfm[container-blocks.list-items.motivation.fixed-indent-from-marker-is-an-alternative]
> Rather than requiring a fixed indent from the margin, we could require
> a fixed indent (say, two spaces, or even one space) from the list marker (which
> may itself be indented).  This proposal would remove the last anomaly
> discussed.  Unlike the spec presented above, it would count the following
> as a list item with a subparagraph, even though the paragraph `bar`
> is not indented as far as the first paragraph `foo`:
>
> ``` markdown
>  10. foo
>
>    bar  
> ```

> gfm[container-blocks.list-items.motivation.fixed-marker-indent-breaks-code-patterns]
> Arguably this text does read like a list item with `bar` as a subparagraph,
> which may count in favor of the proposal.  However, on this proposal indented
> code would have to be indented six spaces after the list marker.  And this
> would break a lot of existing Markdown, which has the pattern:
>
> ``` markdown
> 1.  foo
>
>         indented code
> ```

> gfm[container-blocks.list-items.motivation.spec-preserves-eight-space-code-pattern]
> where the code is indented eight spaces.  The spec above, by contrast, will
> parse this text as expected, since the code block's indentation is measured
> from the beginning of `foo`.

> gfm[container-blocks.list-items.motivation.indented-code-start-needs-special-treatment]
> The one case that needs special treatment is a list item that *starts*
> with indented code.  How much indentation is required in that case, since
> we don't have a "first paragraph" to measure from?  Rule #2 simply stipulates
> that in such cases, we require one space indentation from the list marker
> (and then the normal four spaces for the indented code).  This will match the
> four-space rule in cases where the list marker plus its initial indentation
> takes four spaces (a common case), but diverge in other cases.

> gfm[container-blocks.list-items.motivation.extension-wrapper]
> <div class="extension">

## Task list items (extension)

> gfm[container-blocks.task-list-items-extension.gfm-enables-task-list-extension]
> GFM enables the `tasklist` extension, where an additional processing step is
> performed on [list items].

> gfm[container-blocks.task-list-items-extension.definition]
> A [task list item](@) is a [list item][list items] where the first block in it
> is a paragraph which begins with a [task list item marker] and at least one
> whitespace character before any other content.

> gfm[container-blocks.task-list-items-extension.marker-structure]
> A [task list item marker](@) consists of an optional number of spaces, a left
> bracket (`[`), either a whitespace character or the letter `x` in either
> lowercase or uppercase, and then a right bracket (`]`).

> gfm[container-blocks.task-list-items-extension.rendered-marker-becomes-semantic-checkbox]
> When rendered, the [task list item marker] is replaced with a semantic checkbox element;
> in an HTML output, this would be an `<input type="checkbox">` element.

> gfm[container-blocks.task-list-items-extension.whitespace-marker-is-unchecked]
> If the character between the brackets is a whitespace character, the checkbox
> is unchecked.  Otherwise, the checkbox is checked.

> gfm[container-blocks.task-list-items-extension.checkbox-interaction-is-unspecified]
> This spec does not define how the checkbox elements are interacted with: in practice,
> implementors are free to render the checkboxes as disabled or inmutable elements,
> or they may dynamically handle dynamic interactions (i.e. checking, unchecking) in
> the final rendered document.
>
> ```````````````````````````````` example disabled
> - [ ] foo
> - [x] bar
> .
> <ul>
> <li><input disabled="" type="checkbox"> foo</li>
> <li><input checked="" disabled="" type="checkbox"> bar</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.task-list-items-extension.task-lists-may-be-arbitrarily-nested]
> Task lists can be arbitrarily nested:
>
> ```````````````````````````````` example disabled
> - [x] foo
>   - [ ] bar
>   - [x] baz
> - [ ] bim
> .
> <ul>
> <li><input checked="" disabled="" type="checkbox"> foo
> <ul>
> <li><input disabled="" type="checkbox"> bar</li>
> <li><input checked="" disabled="" type="checkbox"> baz</li>
> </ul>
> </li>
> <li><input disabled="" type="checkbox"> bim</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.task-list-items-extension.extension-wrapper]
> </div>

## Lists

> gfm[container-blocks.lists.definition]
> A [list](@) is a sequence of one or more
> list items [of the same type].

> gfm[container-blocks.lists.items-may-be-separated-by-blank-lines]
> The list items may be separated by any number of blank lines.

> gfm[container-blocks.lists.same-type-definition]
> Two list items are [of the same type](@)
> if they begin with a [list marker] of the same type.

> gfm[container-blocks.lists.same-marker-type-definition]
> Two list markers are of the same type if (a) they are bullet list
> markers using the same character (`-`, `+`, or `*`) or (b) they are
> ordered list numbers with the same delimiter (either `.` or `)`).

> gfm[container-blocks.lists.ordered-and-bullet-list-definition]
> A list is an [ordered list](@)
> if its constituent list items begin with
> [ordered list markers], and a
> [bullet list](@) if its constituent list
> items begin with [bullet list markers].

> gfm[container-blocks.lists.start-number-definition]
> The [start number](@)
> of an [ordered list] is determined by the list number of
> its initial list item.  The numbers of subsequent list items are
> disregarded.

> gfm[container-blocks.lists.loose-list-definition]
> A list is [loose](@) if any of its constituent
> list items are separated by blank lines, or if any of its constituent
> list items directly contain two block-level elements with a blank line
> between them.

> gfm[container-blocks.lists.tight-list-definition]
> Otherwise a list is [tight](@).

> gfm[container-blocks.lists.loose-vs-tight-html-difference]
> In HTML output, paragraphs in a loose list are wrapped in `<p>`
> tags, while paragraphs in a tight list are not.

> gfm[container-blocks.lists.changing-marker-type-or-delimiter-starts-new-list]
> Changing the bullet or ordered list delimiter starts a new list:
>
> ```````````````````````````````` example
> - foo
> - bar
> + baz
> .
> <ul>
> <li>foo</li>
> <li>bar</li>
> </ul>
> <ul>
> <li>baz</li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1. foo
> 2. bar
> 3) baz
> .
> <ol>
> <li>foo</li>
> <li>bar</li>
> </ol>
> <ol start="3">
> <li>baz</li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.lists.list-can-interrupt-paragraph-without-blank-line]
> In CommonMark, a list can interrupt a paragraph. That is,
> no blank line is needed to separate a paragraph from a following
> list:
>
> ```````````````````````````````` example
> Foo
> - bar
> - baz
> .
> <p>Foo</p>
> <ul>
> <li>bar</li>
> <li>baz</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.markdown-pl-disallows-this-to-avoid-hard-wrapped-numeral-lists]
> `Markdown.pl` does not allow this, through fear of triggering a list
> via a numeral in a hard-wrapped line:
>
> ``` markdown
> The number of windows in my house is
> 14.  The number of doors is 6.
> ```

> gfm[container-blocks.lists.markdown-pl-still-allows-block-quotes-to-interrupt-paragraphs]
> Oddly, though, `Markdown.pl` *does* allow a blockquote to
> interrupt a paragraph, even though the same considerations might
> apply.

> gfm[container-blocks.lists.commonmark-allows-list-interruption]
> In CommonMark, we do allow lists to interrupt paragraphs, for
> two reasons.  First, it is natural and not uncommon for people
> to start lists without blank lines:
>
> ``` markdown
> I need to buy
> - new shoes
> - a coat
> - a plane ticket
> ```

> gfm[container-blocks.lists.second-reason-is-principle-of-uniformity]
> Second, we are attracted to a

> gfm[container-blocks.lists.principle-of-uniformity-definition]
> > [principle of uniformity](@):
> > if a chunk of text has a certain
> > meaning, it will continue to have the same meaning when put into a
> > container block (such as a list item or blockquote).

> gfm[container-blocks.lists.uniformity-preserves-nested-list-meaning]
> (Indeed, the spec for [list items] and [block quotes] presupposes
> this principle.) This principle implies that if
>
> ``` markdown
>   * I need to buy
>     - new shoes
>     - a coat
>     - a plane ticket
> ```

> gfm[container-blocks.lists.nested-example-is-list-item-with-sublist]
> is a list item containing a paragraph followed by a nested sublist,
> as all Markdown implementations agree it is (though the paragraph
> may be rendered without `<p>` tags, since the list is "tight"),
> then
>
> ``` markdown
> I need to buy
> - new shoes
> - a coat
> - a plane ticket
> ```

> gfm[container-blocks.lists.corresponding-standalone-example-should-parse-the-same-way]
> by itself should be a paragraph followed by a nested sublist.

> gfm[container-blocks.lists.uniformity-supports-list-interruption]
> Since it is well established Markdown practice to allow lists to
> interrupt paragraphs inside list items, the [principle of
> uniformity] requires us to allow this outside list items as
> well.  ([reStructuredText](http://docutils.sourceforge.net/rst.html)
> takes a different approach, requiring blank lines before lists
> even inside other list items.)

> gfm[container-blocks.lists.only-lists-starting-with-one-may-interrupt-paragraphs]
> In order to solve the problem of unwanted lists in paragraphs with
> hard-wrapped numerals, we allow only lists starting with `1` to
> interrupt paragraphs.  Thus,
>
> ```````````````````````````````` example
> The number of windows in my house is
> 14.  The number of doors is 6.
> .
> <p>The number of windows in my house is
> 14.  The number of doors is 6.</p>
> ````````````````````````````````

> gfm[container-blocks.lists.starting-with-one-can-still-cause-unintended-list-capture]
> We may still get an unintended result in cases like
>
> ```````````````````````````````` example
> The number of windows in my house is
> 1.  The number of doors is 6.
> .
> <p>The number of windows in my house is</p>
> <ol>
> <li>The number of doors is 6.</li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.lists.starting-with-one-should-prevent-most-spurious-captures]
> but this rule should prevent most spurious list captures.

> gfm[container-blocks.lists.any-number-of-blank-lines-may-separate-items]
> There can be any number of blank lines between items:
>
> ```````````````````````````````` example
> - foo
>
> - bar
>
>
> - baz
> .
> <ul>
> <li>
> <p>foo</p>
> </li>
> <li>
> <p>bar</p>
> </li>
> <li>
> <p>baz</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - foo
>   - bar
>     - baz
>
>
>       bim
> .
> <ul>
> <li>foo
> <ul>
> <li>bar
> <ul>
> <li>
> <p>baz</p>
> <p>bim</p>
> </li>
> </ul>
> </li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-html-comment-separates-lists]
> To separate consecutive lists of the same type, or to separate a
> list from an indented code block that would otherwise be parsed
> as a subparagraph of the final list item, you can insert a blank HTML
> comment:
>
> ```````````````````````````````` example
> - foo
> - bar
>
> <!-- -->
>
> - baz
> - bim
> .
> <ul>
> <li>foo</li>
> <li>bar</li>
> </ul>
> <!-- -->
> <ul>
> <li>baz</li>
> <li>bim</li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> -   foo
>
>     notcode
>
> -   foo
>
> <!-- -->
>
>     code
> .
> <ul>
> <li>
> <p>foo</p>
> <p>notcode</p>
> </li>
> <li>
> <p>foo</p>
> </li>
> </ul>
> <!-- -->
> <pre><code>code
> </code></pre>
> ````````````````````````````````

> gfm[container-blocks.lists.items-need-not-use-uniform-indentation]
> List items need not be indented to the same level.  The following
> list items will be treated as items at the same list level,
> since none is indented enough to belong to the previous list
> item:
>
> ```````````````````````````````` example
> - a
>  - b
>   - c
>    - d
>   - e
>  - f
> - g
> .
> <ul>
> <li>a</li>
> <li>b</li>
> <li>c</li>
> <li>d</li>
> <li>e</li>
> <li>f</li>
> <li>g</li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 1. a
>
>   2. b
>
>    3. c
> .
> <ol>
> <li>
> <p>a</p>
> </li>
> <li>
> <p>b</p>
> </li>
> <li>
> <p>c</p>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.lists.more-than-three-spaces-do-not-start-item]
> Note, however, that list items may not be indented more than
> three spaces.  Here `- e` is treated as a paragraph continuation
> line, because it is indented more than three spaces:
>
> ```````````````````````````````` example
> - a
>  - b
>   - c
>    - d
>     - e
> .
> <ul>
> <li>a</li>
> <li>b</li>
> <li>c</li>
> <li>d
> - e</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.four-space-indented-marker-is-code-block]
> And here, `3. c` is treated as in indented code block,
> because it is indented four spaces and preceded by a
> blank line.
>
> ```````````````````````````````` example
> 1. a
>
>   2. b
>
>     3. c
> .
> <ol>
> <li>
> <p>a</p>
> </li>
> <li>
> <p>b</p>
> </li>
> </ol>
> <pre><code>3. c
> </code></pre>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-line-between-items-makes-list-loose]
> This is a loose list, because there is a blank line between
> two of the list items:
>
> ```````````````````````````````` example
> - a
> - b
>
> - c
> .
> <ul>
> <li>
> <p>a</p>
> </li>
> <li>
> <p>b</p>
> </li>
> <li>
> <p>c</p>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-line-before-empty-item-still-makes-list-loose]
> So is this, with a empty second item:
>
> ```````````````````````````````` example
> * a
> *
>
> * c
> .
> <ul>
> <li>
> <p>a</p>
> </li>
> <li></li>
> <li>
> <p>c</p>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-line-between-item-blocks-makes-list-loose]
> These are loose lists, even though there is no space between the items,
> because one of the items directly contains two block-level elements
> with a blank line between them:
>
> ```````````````````````````````` example
> - a
> - b
>
>   c
> - d
> .
> <ul>
> <li>
> <p>a</p>
> </li>
> <li>
> <p>b</p>
> <p>c</p>
> </li>
> <li>
> <p>d</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - a
> - b
>
>   [ref]: /url
> - d
> .
> <ul>
> <li>
> <p>a</p>
> </li>
> <li>
> <p>b</p>
> </li>
> <li>
> <p>d</p>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-lines-inside-code-block-do-not-make-list-loose]
> This is a tight list, because the blank lines are in a code block:
>
> ```````````````````````````````` example
> - a
> - ```
>   b
>
>
>   ```
> - c
> .
> <ul>
> <li>a</li>
> <li>
> <pre><code>b
>
>
> </code></pre>
> </li>
> <li>c</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.loose-sublist-does-not-necessarily-make-outer-list-loose]
> This is a tight list, because the blank line is between two
> paragraphs of a sublist.  So the sublist is loose while
> the outer list is tight:
>
> ```````````````````````````````` example
> - a
>   - b
>
>     c
> - d
> .
> <ul>
> <li>a
> <ul>
> <li>
> <p>b</p>
> <p>c</p>
> </li>
> </ul>
> </li>
> <li>d</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-line-inside-block-quote-does-not-make-list-loose]
> This is a tight list, because the blank line is inside the
> block quote:
>
> ```````````````````````````````` example
> * a
>   > b
>   >
> * c
> .
> <ul>
> <li>a
> <blockquote>
> <p>b</p>
> </blockquote>
> </li>
> <li>c</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.consecutive-block-elements-without-blank-line-keep-list-tight]
> This list is tight, because the consecutive block elements
> are not separated by blank lines:
>
> ```````````````````````````````` example
> - a
>   > b
>   ```
>   c
>   ```
> - d
> .
> <ul>
> <li>a
> <blockquote>
> <p>b</p>
> </blockquote>
> <pre><code>c
> </code></pre>
> </li>
> <li>d</li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.single-paragraph-list-is-tight]
> A single-paragraph list is tight:
>
> ```````````````````````````````` example
> - a
> .
> <ul>
> <li>a</li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - a
>   - b
> .
> <ul>
> <li>a
> <ul>
> <li>b</li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````

> gfm[container-blocks.lists.blank-line-between-block-elements-in-item-makes-list-loose]
> This list is loose, because of the blank line between the
> two block elements in the list item:
>
> ```````````````````````````````` example
> 1. ```
>    foo
>    ```
>
>    bar
> .
> <ol>
> <li>
> <pre><code>foo
> </code></pre>
> <p>bar</p>
> </li>
> </ol>
> ````````````````````````````````

> gfm[container-blocks.lists.outer-list-may-be-loose-while-inner-list-is-tight]
> Here the outer list is loose, the inner list tight:
>
> ```````````````````````````````` example
> * foo
>   * bar
>
>   baz
> .
> <ul>
> <li>
> <p>foo</p>
> <ul>
> <li>bar</li>
> </ul>
> <p>baz</p>
> </li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> - a
>   - b
>   - c
>
> - d
>   - e
>   - f
> .
> <ul>
> <li>
> <p>a</p>
> <ul>
> <li>b</li>
> <li>c</li>
> </ul>
> </li>
> <li>
> <p>d</p>
> <ul>
> <li>e</li>
> <li>f</li>
> </ul>
> </li>
> </ul>
> ````````````````````````````````

# Inlines

> gfm[inlines.inline-parsing-is-sequential]
> Inlines are parsed sequentially from the beginning of the character
> stream to the end (left to right, in left-to-right languages).
> Thus, for example, in
>
> ```````````````````````````````` example
> `hi`lo`
> .
> <p><code>hi</code>lo`</p>
> ````````````````````````````````

> gfm[inlines.earlier-inline-match-takes-precedence-in-sequential-parse]
> `hi` is parsed as code, leaving the backtick at the end as a literal
> backtick.

## Backslash escapes

> gfm[inlines.backslash-escapes.any-ascii-punctuation-may-be-escaped]
> Any ASCII punctuation character may be backslash-escaped:
>
> ```````````````````````````````` example
> \!\"\#\$\%\&\'\(\)\*\+\,\-\.\/\:\;\<\=\>\?\@\[\\\]\^\_\`\{\|\}\~
> .
> <p>!&quot;#$%&amp;'()*+,-./:;&lt;=&gt;?@[\]^_`{|}~</p>
> ````````````````````````````````

> gfm[inlines.backslash-escapes.backslashes-before-other-characters-are-literal]
> Backslashes before other characters are treated as literal
> backslashes:
>
> ```````````````````````````````` example
> \→\A\a\ \3\φ\«
> .
> <p>\→\A\a\ \3\φ\«</p>
> ````````````````````````````````

> gfm[inlines.backslash-escapes.escaped-characters-lose-special-meaning]
> Escaped characters are treated as regular characters and do
> not have their usual Markdown meanings:
>
> ```````````````````````````````` example
> \*not emphasized*
> \<br/> not a tag
> \[not a link](/foo)
> \`not code`
> 1\. not a list
> \* not a list
> \# not a heading
> \[foo]: /url "not a reference"
> \&ouml; not a character entity
> .
> <p>*not emphasized*
> &lt;br/&gt; not a tag
> [not a link](/foo)
> `not code`
> 1. not a list
> * not a list
> # not a heading
> [foo]: /url &quot;not a reference&quot;
> &amp;ouml; not a character entity</p>
> ````````````````````````````````

> gfm[inlines.backslash-escapes.escaped-backslash-does-not-escape-following-character]
> If a backslash is itself escaped, the following character is not:
>
> ```````````````````````````````` example
> \\*emphasis*
> .
> <p>\<em>emphasis</em></p>
> ````````````````````````````````

> gfm[inlines.backslash-escapes.backslash-at-end-line-is-hard-line-break]
> A backslash at the end of the line is a [hard line break]:
>
> ```````````````````````````````` example
> foo\
> bar
> .
> <p>foo<br />
> bar</p>
> ````````````````````````````````

> gfm[inlines.backslash-escapes.do-not-work-in-code-autolinks-or-raw-html]
> Backslash escapes do not work in code blocks, code spans, autolinks, or
> raw HTML:
>
> ```````````````````````````````` example
> `` \[\` ``
> .
> <p><code>\[\`</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
>     \[\]
> .
> <pre><code>\[\]
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ~~~
> \[\]
> ~~~
> .
> <pre><code>\[\]
> </code></pre>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <http://example.com?find=\*>
> .
> <p><a href="http://example.com?find=%5C*">http://example.com?find=\*</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <a href="/bar\/)">
> .
> <a href="/bar\/)">
> ````````````````````````````````

> gfm[inlines.backslash-escapes.work-in-urls-titles-references-and-info-strings]
> But they work in all other contexts, including URLs and link titles,
> link references, and [info strings] in [fenced code blocks]:
>
> ```````````````````````````````` example
> [foo](/bar\* "ti\*tle")
> .
> <p><a href="/bar*" title="ti*tle">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo]
>
> [foo]: /bar\* "ti\*tle"
> .
> <p><a href="/bar*" title="ti*tle">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ``` foo\+bar
> foo
> ```
> .
> <pre><code class="language-foo+bar">foo
> </code></pre>
> ````````````````````````````````

## Entity and numeric character references

> gfm[inlines.entity-and-numeric-character-references.references-produce-unicode-characters]
> Valid HTML entity references and numeric character references can be
> used in place of the corresponding Unicode character.

> gfm[inlines.entity-and-numeric-character-references.references-are-not-recognized-in-code]
> Entity and character references are not recognized in code blocks and
> code spans.

> gfm[inlines.entity-and-numeric-character-references.references-cannot-replace-structural-markers]
> Entity and character references cannot stand in place of special
> characters that define structural elements in CommonMark.

> gfm[inlines.entity-and-numeric-character-references.parsers-need-not-preserve-source-form]
> Conforming CommonMark parsers need not store information about
> whether a particular character was represented in the source
> using a Unicode character or an entity reference.

> gfm[inlines.entity-and-numeric-character-references.entity-references-definition]
> [Entity references](@) consist of `&` + any of the valid
> HTML5 entity names + `;`. The
> document <https://html.spec.whatwg.org/multipage/entities.json>
> is used as an authoritative source for the valid entity
> references and their corresponding code points.
>
> ```````````````````````````````` example
> &nbsp; &amp; &copy; &AElig; &Dcaron;
> &frac34; &HilbertSpace; &DifferentialD;
> &ClockwiseContourIntegral; &ngE;
> .
> <p>  &amp; © Æ Ď
> ¾ ℋ ⅆ
> ∲ ≧̸</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.decimal-numeric-reference-definition]
> [Decimal numeric character
> references](@)
> consist of `&#` + a string of 1--7 arabic digits + `;`. A
> numeric character reference is parsed as the corresponding
> Unicode character. Invalid Unicode code points will be replaced by
> the REPLACEMENT CHARACTER (`U+FFFD`).  For security reasons,
> the code point `U+0000` will also be replaced by `U+FFFD`.
>
> ```````````````````````````````` example
> &#35; &#1234; &#992; &#0;
> .
> <p># Ӓ Ϡ �</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.hex-reference-definition]
> [Hexadecimal numeric character
> references](@) consist of `&#` +
> either `X` or `x` + a string of 1-6 hexadecimal digits + `;`.
> They too are parsed as the corresponding Unicode character (this
> time specified with a hexadecimal numeral instead of decimal).
>
> ```````````````````````````````` example
> &#X22; &#XD06; &#xcab;
> .
> <p>&quot; ആ ಫ</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.invalid-references-are-literal-text]
> Here are some nonentities:
>
> ```````````````````````````````` example
> &nbsp &x; &#; &#x;
> &#987654321;
> &#abcdef0;
> &ThisIsNotDefined; &hi?;
> .
> <p>&amp;nbsp &amp;x; &amp;#; &amp;#x;
> &amp;#987654321;
> &amp;#abcdef0;
> &amp;ThisIsNotDefined; &amp;hi?;</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.named-entities-require-semicolon]
> Although HTML5 does accept some entity references
> without a trailing semicolon (such as `&copy`), these are not
> recognized here, because it makes the grammar too ambiguous:
>
> ```````````````````````````````` example
> &copy
> .
> <p>&amp;copy</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.unknown-named-entities-are-literal]
> Strings that are not on the list of HTML5 named entities are not
> recognized as entity references either:
>
> ```````````````````````````````` example
> &MadeUpEntity;
> .
> <p>&amp;MadeUpEntity;</p>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.references-are-recognized-outside-code]
> Entity and numeric character references are recognized in any
> context besides code spans or code blocks, including
> URLs, [link titles], and [fenced code block][] [info strings]:
>
> ```````````````````````````````` example
> <a href="&ouml;&ouml;.html">
> .
> <a href="&ouml;&ouml;.html">
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo](/f&ouml;&ouml; "f&ouml;&ouml;")
> .
> <p><a href="/f%C3%B6%C3%B6" title="föö">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo]
>
> [foo]: /f&ouml;&ouml; "f&ouml;&ouml;"
> .
> <p><a href="/f%C3%B6%C3%B6" title="föö">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ``` f&ouml;&ouml;
> foo
> ```
> .
> <pre><code class="language-föö">foo
> </code></pre>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.references-are-literal-text-in-code]
> Entity and numeric character references are treated as literal
> text in code spans and code blocks:
>
> ```````````````````````````````` example
> `f&ouml;&ouml;`
> .
> <p><code>f&amp;ouml;&amp;ouml;</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
>     f&ouml;f&ouml;
> .
> <pre><code>f&amp;ouml;f&amp;ouml;
> </code></pre>
> ````````````````````````````````

> gfm[inlines.entity-and-numeric-character-references.references-cannot-replace-structure-symbols]
> Entity and numeric character references cannot be used
> in place of symbols indicating structure in CommonMark
> documents.
>
> ```````````````````````````````` example
> &#42;foo&#42;
> *foo*
> .
> <p>*foo*
> <em>foo</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> &#42; foo
>
> * foo
> .
> <p>* foo</p>
> <ul>
> <li>foo</li>
> </ul>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo&#10;&#10;bar
> .
> <p>foo
>
> bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> &#9;foo
> .
> <p>→foo</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [a](url &quot;tit&quot;)
> .
> <p>[a](url &quot;tit&quot;)</p>
> ````````````````````````````````

## Code spans

> gfm[inlines.code-spans.backtick-string-definition]
> A [backtick string](@)
> is a string of one or more backtick characters (`` ` ``) that is neither
> preceded nor followed by a backtick.

> gfm[inlines.code-spans.definition]
> A [code span](@) begins with a backtick string and ends with a
> backtick string of equal length.

> gfm[inlines.code-spans.line-endings-become-spaces]
> In a code span, [line endings] are converted to [spaces].

> gfm[inlines.code-spans.single-edge-space-is-trimmed]
> If the resulting code span content both begins and ends with a
> [space] character, but does not consist entirely of [space]
> characters, a single [space] character is removed from the front and
> back.

> gfm[inlines.code-spans.example]
> This is a simple code span:
>
> ```````````````````````````````` example
> `foo`
> .
> <p><code>foo</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.longer-delimiters-allow-literal-backticks]
> Here two backticks are used, because the code contains a backtick.
> This example also illustrates stripping of a single leading and
> trailing space:
>
> ```````````````````````````````` example
> `` foo ` bar ``
> .
> <p><code>foo ` bar</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.edge-space-trimming-allows-literal-backticks-at-edges]
> This example shows the motivation for stripping leading and trailing
> spaces:
>
> ```````````````````````````````` example
> ` `` `
> .
> <p><code>``</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.only-one-space-is-trimmed]
> Note that only *one* space is stripped:
>
> ```````````````````````````````` example
> `  ``  `
> .
> <p><code> `` </code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.edge-space-trimming-requires-space-on-both-sides]
> The stripping only happens if the space is on both
> sides of the string:
>
> ```````````````````````````````` example
> ` a`
> .
> <p><code> a</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.only-ascii-space-is-trimmed]
> Only [spaces], and not [unicode whitespace] in general, are
> stripped in this way:
>
> ```````````````````````````````` example
> ` b `
> .
> <p><code> b </code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.all-space-content-is-not-trimmed]
> No stripping occurs if the code span contains only spaces:
>
> ```````````````````````````````` example
> ` `
> `  `
> .
> <p><code> </code>
> <code>  </code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.line-endings-conversion-example]
> [Line endings] are treated like spaces:
>
> ```````````````````````````````` example
> ``
> foo
> bar  
> baz
> ``
> .
> <p><code>foo bar   baz</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ``
> foo 
> ``
> .
> <p><code>foo </code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.interior-spaces-are-preserved]
> Interior spaces are not collapsed:
>
> ```````````````````````````````` example
> `foo   bar 
> baz`
> .
> <p><code>foo   bar  baz</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.browser-rendering-may-collapse-spaces]
> Note that browsers will typically collapse consecutive spaces
> when rendering `<code>` elements, so it is recommended that
> the following CSS be used:

> gfm[inlines.code-spans.code-white-space-pre-wrap]
>     code{white-space: pre-wrap;}

> gfm[inlines.code-spans.backslashes-are-literal]
> Note that backslash escapes do not work in code spans. All backslashes
> are treated literally:
>
> ```````````````````````````````` example
> `foo\`bar`
> .
> <p><code>foo\</code>bar`</p>
> ````````````````````````````````

> gfm[inlines.code-spans.backslash-escapes-are-never-needed]
> Backslash escapes are never needed, because one can always choose a
> string of *n* backtick characters as delimiters, where the code does
> not contain any strings of exactly *n* backtick characters.
>
> ```````````````````````````````` example
> ``foo`bar``
> .
> <p><code>foo`bar</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ` foo `` bar `
> .
> <p><code>foo `` bar</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.code-spans-take-precedence-over-emphasis-and-links]
> Code span backticks have higher precedence than any other inline
> constructs except HTML tags and autolinks.  Thus, for example, this is
> not parsed as emphasized text, since the second `*` is part of a code
> span:
>
> ```````````````````````````````` example
> *foo`*`
> .
> <p>*foo<code>*</code></p>
> ````````````````````````````````

> gfm[inlines.code-spans.code-span-prevents-link-parse]
> And this is not parsed as a link:
>
> ```````````````````````````````` example
> [not a `link](/foo`)
> .
> <p>[not a <code>link](/foo</code>)</p>
> ````````````````````````````````

> gfm[inlines.code-spans.code-spans-html-tags-and-autolinks-share-precedence]
> Code spans, HTML tags, and autolinks have the same precedence.
> Thus, this is code:
>
> ```````````````````````````````` example
> `<a href="`">`
> .
> <p><code>&lt;a href=&quot;</code>&quot;&gt;`</p>
> ````````````````````````````````

> gfm[inlines.code-spans.html-tag-wins-when-tag-parses-first]
> But this is an HTML tag:
>
> ```````````````````````````````` example
> <a href="`">`
> .
> <p><a href="`">`</p>
> ````````````````````````````````

> gfm[inlines.code-spans.code-span-wins-in-ambiguous-autolink-like-text]
> And this is code:
>
> ```````````````````````````````` example
> `<http://foo.bar.`baz>`
> .
> <p><code>&lt;http://foo.bar.</code>baz&gt;`</p>
> ````````````````````````````````

> gfm[inlines.code-spans.autolink-wins-when-autolink-parses-first]
> But this is an autolink:
>
> ```````````````````````````````` example
> <http://foo.bar.`baz>`
> .
> <p><a href="http://foo.bar.%60baz">http://foo.bar.`baz</a>`</p>
> ````````````````````````````````

> gfm[inlines.code-spans.unclosed-backtick-string-is-literal]
> When a backtick string is not closed by a matching backtick string,
> we just have literal backticks:
>
> ```````````````````````````````` example
> ```foo``
> .
> <p>```foo``</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> `foo
> .
> <p>`foo</p>
> ````````````````````````````````

> gfm[inlines.code-spans.opening-and-closing-backtick-strings-must-have-equal-length]
> The following case also illustrates the need for opening and
> closing backtick strings to be equal in length:
>
> ```````````````````````````````` example
> `foo``bar``
> .
> <p>`foo<code>bar</code></p>
> ````````````````````````````````

## Emphasis and strong emphasis

> gfm[inlines.emphasis-and-strong-emphasis.original-markdown-description-quoted]
> John Gruber's original [Markdown syntax
> description](http://daringfireball.net/projects/markdown/syntax#em) says:

> gfm[inlines.emphasis-and-strong-emphasis.original-markdown-description]
> > Markdown treats asterisks (`*`) and underscores (`_`) as indicators of
> > emphasis. Text wrapped with one `*` or `_` will be wrapped with an HTML
> > `<em>` tag; double `*`'s or `_`'s will be wrapped with an HTML `<strong>`
> > tag.

> gfm[inlines.emphasis-and-strong-emphasis.original-rules-leave-nesting-underdetermined]
> This is enough for most users, but these rules leave much undecided,
> especially when it comes to nested emphasis.  The original
> `Markdown.pl` test suite makes it clear that triple `***` and
> `___` delimiters can be used for strong emphasis, and most
> implementations have also allowed the following patterns:
>
> ``` markdown
> ***strong emph***
> ***strong** in emph*
> ***emph* in strong**
> **in strong *emph***
> *in emph **strong***
> ```

> gfm[inlines.emphasis-and-strong-emphasis.less-common-but-useful-emphasis-patterns]
> The following patterns are less widely supported, but the intent
> is clear and they are useful (especially in contexts like bibliography
> entries):
>
> ``` markdown
> *emph *with emph* in it*
> **strong **with strong** in it**
> ```

> gfm[inlines.emphasis-and-strong-emphasis.many-implementations-restrict-intraword-underscore]
> Many implementations have also restricted intraword emphasis to
> the `*` forms, to avoid unwanted emphasis in words containing
> internal underscores.  (It is best practice to put these in code
> spans, but users often do not.)
>
> ``` markdown
> internal emphasis: foo*bar*baz
> no emphasis: foo_bar_baz
> ```

> gfm[inlines.emphasis-and-strong-emphasis.rules-capture-patterns-without-backtracking]
> The rules given below capture all of these patterns, while allowing
> for efficient parsing strategies that do not backtrack.

> gfm[inlines.emphasis-and-strong-emphasis.delimiter-run-definition]
> First, some definitions.  A [delimiter run](@) is either
> a sequence of one or more `*` characters that is not preceded or
> followed by a non-backslash-escaped `*` character, or a sequence
> of one or more `_` characters that is not preceded or followed by
> a non-backslash-escaped `_` character.

> gfm[inlines.emphasis-and-strong-emphasis.left-flanking-delimiter-run-definition]
> A [left-flanking delimiter run](@) is
> a [delimiter run] that is (1) not followed by [Unicode whitespace],
> and either (2a) not followed by a [punctuation character], or
> (2b) followed by a [punctuation character] and
> preceded by [Unicode whitespace] or a [punctuation character].
> For purposes of this definition, the beginning and the end of
> the line count as Unicode whitespace.

> gfm[inlines.emphasis-and-strong-emphasis.right-flanking-delimiter-run-definition]
> A [right-flanking delimiter run](@) is
> a [delimiter run] that is (1) not preceded by [Unicode whitespace],
> and either (2a) not preceded by a [punctuation character], or
> (2b) preceded by a [punctuation character] and
> followed by [Unicode whitespace] or a [punctuation character].
> For purposes of this definition, the beginning and the end of
> the line count as Unicode whitespace.

> gfm[inlines.emphasis-and-strong-emphasis.delimiter-run-example]
> Here are some examples of delimiter runs.
>
>   - left-flanking but not right-flanking:
>
>     ```
>     ***abc
>       _abc
>     **"abc"
>      _"abc"
>     ```
>
>   - right-flanking but not left-flanking:
>
>     ```
>      abc***
>      abc_
>     "abc"**
>     "abc"_
>     ```
>
>   - Both left and right-flanking:
>
>     ```
>      abc***def
>     "abc"_"def"
>     ```
>
>   - Neither left nor right-flanking:
>
>     ```
>     abc *** def
>     a _ b
>     ```

> gfm[inlines.emphasis-and-strong-emphasis.left-right-flanking-terminology-note]
> (The idea of distinguishing left-flanking and right-flanking
> delimiter runs based on the character before and the character
> after comes from Roopesh Chander's
> [vfmd](http://www.vfmd.org/vfmd-spec/specification/#procedure-for-identifying-emphasis-tags).
> vfmd uses the terminology "emphasis indicator string" instead of "delimiter
> run," and its rules for distinguishing left- and right-flanking runs
> are a bit more complex than the ones given here.)

> gfm[inlines.emphasis-and-strong-emphasis.single-asterisk-can-open-emphasis]
> A single `*` character [can open emphasis](@) iff it is part of a
> [left-flanking delimiter run].

> gfm[inlines.emphasis-and-strong-emphasis.single-underscore-can-open-emphasis]
> A single `_` character [can open emphasis] iff it is part of a
> [left-flanking delimiter run] and either (a) not part of a
> [right-flanking delimiter run] or (b) part of a
> [right-flanking delimiter run] preceded by punctuation.

> gfm[inlines.emphasis-and-strong-emphasis.single-asterisk-can-close-emphasis]
> A single `*` character [can close emphasis](@) iff it is part of a
> [right-flanking delimiter run].

> gfm[inlines.emphasis-and-strong-emphasis.single-underscore-can-close-emphasis]
> A single `_` character [can close emphasis] iff it is part of a
> [right-flanking delimiter run] and either (a) not part of a
> [left-flanking delimiter run] or (b) part of a
> [left-flanking delimiter run] followed by punctuation.

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-can-open-strong-emphasis]
> A double `**` [can open strong emphasis](@) iff it is part of a
> [left-flanking delimiter run].

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-can-open-strong-emphasis]
> A double `__` [can open strong emphasis] iff it is part of a
> [left-flanking delimiter run] and either (a) not part of a
> [right-flanking delimiter run] or (b) part of a
> [right-flanking delimiter run] preceded by punctuation.

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-can-close-strong-emphasis]
> A double `**` [can close strong emphasis](@) iff it is part of a
> [right-flanking delimiter run].

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-can-close-strong-emphasis]
> A double `__` [can close strong emphasis] iff it is part of a
> [right-flanking delimiter run] and either (a) not part of a
> [left-flanking delimiter run] or (b) part of a
> [left-flanking delimiter run] followed by punctuation.

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-requires-matching-delimiters]
> Emphasis begins with a delimiter that [can open emphasis] and ends
> with a delimiter that [can close emphasis], using the same character
> (`_` or `*`) as the opening delimiter. The opening and closing
> delimiters must belong to separate [delimiter runs]. If one of the
> delimiters can both open and close emphasis, then the sum of the
> lengths of the delimiter runs containing the opening and closing
> delimiters must not be a multiple of 3 unless both lengths are
> multiples of 3.

> gfm[inlines.emphasis-and-strong-emphasis.strong-emphasis-requires-matching-delimiters]
> Strong emphasis begins with a delimiter that [can open strong
> emphasis] and ends with a delimiter that [can close strong
> emphasis], using the same character (`_` or `*`) as the opening
> delimiter. The opening and closing delimiters must belong to separate
> [delimiter runs]. If one of the delimiters can both open and close
> strong emphasis, then the sum of the lengths of the delimiter runs
> containing the opening and closing delimiters must not be a multiple
> of 3 unless both lengths are multiples of 3.

> gfm[inlines.emphasis-and-strong-emphasis.literal-asterisk-cannot-edge-asterisk-emphasis]
> A literal `*` character cannot occur at the beginning or end of
> `*`-delimited emphasis or `**`-delimited strong emphasis, unless it
> is backslash-escaped.

> gfm[inlines.emphasis-and-strong-emphasis.literal-underscore-cannot-edge-underscore-delimiters]
> A literal `_` character cannot occur at the beginning or end of
> `_`-delimited emphasis or `__`-delimited strong emphasis, unless it
> is backslash-escaped.

> gfm[inlines.emphasis-and-strong-emphasis.ambiguity-resolution-principles]
> Where rules 1--12 above are compatible with multiple parsings,
> the following principles resolve ambiguity:
>
> 13. The number of nestings should be minimized. Thus, for example,
>     an interpretation `<strong>...</strong>` is always preferred to
>     `<em><em>...</em></em>`.
>
> 14. An interpretation `<em><strong>...</strong></em>` is always
>     preferred to `<strong><em>...</em></strong>`.
>
> 15. When two potential emphasis or strong emphasis spans overlap,
>     so that the second begins before the first ends and ends after
>     the first ends, the first takes precedence. Thus, for example,
>     `*foo _bar* baz_` is parsed as `<em>foo _bar</em> baz_` rather
>     than `*foo <em>bar* baz</em>`.
>
> 16. When there are two potential emphasis or strong emphasis spans
>     with the same closing delimiter, the shorter one (the one that
>     opens later) takes precedence. Thus, for example,
>     `**foo **bar baz**` is parsed as `**foo <strong>bar baz</strong>`
>     rather than `<strong>foo **bar baz</strong>`.
>
> 17. Inline code spans, links, images, and HTML tags group more tightly
>     than emphasis.  So, when there is a choice between an interpretation
>     that contains one of these elements and one that does not, the
>     former always wins.  Thus, for example, `*[foo*](bar)` is
>     parsed as `*<a href="bar">foo*</a>` rather than as
>     `<em>[foo</em>](bar)`.

> gfm[inlines.emphasis-and-strong-emphasis.rules-illustrated-by-examples]
> These rules can be illustrated through a series of examples.
>
> Rule 1:
>
> ```````````````````````````````` example
> *foo bar*
> .
> <p><em>foo bar</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-when-followed-by-whitespace]
> This is not emphasis, because the opening `*` is followed by
> whitespace, and hence not part of a [left-flanking delimiter run]:
>
> ```````````````````````````````` example
> a * foo bar*
> .
> <p>a * foo bar*</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.asterisk-cannot-open-after-alphanumeric-before-punctuation]
> This is not emphasis, because the opening `*` is preceded
> by an alphanumeric and followed by punctuation, and hence
> not part of a [left-flanking delimiter run]:
>
> ```````````````````````````````` example
> a*"foo"*
> .
> <p>a*&quot;foo&quot;*</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.unicode-nonbreaking-space-counts-as-whitespace]
> Unicode nonbreaking spaces count as whitespace, too:
>
> ```````````````````````````````` example
> * a *
> .
> <p>* a *</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-permitted]
> Intraword emphasis with `*` is permitted:
>
> ```````````````````````````````` example
> foo*bar*
> .
> <p>foo<em>bar</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 5*6*78
> .
> <p>5<em>6</em>78</p>
> ````````````````````````````````
>
> Rule 2:
>
> ```````````````````````````````` example
> _foo bar_
> .
> <p><em>foo bar</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-whitespace]
> This is not emphasis, because the opening `_` is followed by
> whitespace:
>
> ```````````````````````````````` example
> _ foo bar_
> .
> <p>_ foo bar_</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.underscore-cannot-open-after-alphanumeric-before-punctuation]
> This is not emphasis, because the opening `_` is preceded
> by an alphanumeric and followed by punctuation:
>
> ```````````````````````````````` example
> a_"foo"_
> .
> <p>a_&quot;foo&quot;_</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-is-not-allowed-inside-words]
> Emphasis with `_` is not allowed inside words:
>
> ```````````````````````````````` example
> foo_bar_
> .
> <p>foo_bar_</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 5_6_78
> .
> <p>5_6_78</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> пристаням_стремятся_
> .
> <p>пристаням_стремятся_</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.right-flanking-underscore-does-not-emphasize]
> Here `_` does not generate emphasis, because the first delimiter run
> is right-flanking and the second left-flanking:
>
> ```````````````````````````````` example
> aa_"bb"_cc
> .
> <p>aa_&quot;bb&quot;_cc</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.underscore-can-open-when-punctuation-precedes]
> This is emphasis, even though the opening delimiter is
> both left- and right-flanking, because it is preceded by
> punctuation:
>
> ```````````````````````````````` example
> foo-_(bar)_
> .
> <p>foo-<em>(bar)</em></p>
> ````````````````````````````````
>
> Rule 3:

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-delimiters-must-match]
> This is not emphasis, because the closing delimiter does
> not match the opening delimiter:
>
> ```````````````````````````````` example
> _foo*
> .
> <p>_foo*</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-whitespace]
> This is not emphasis, because the closing `*` is preceded by
> whitespace:
>
> ```````````````````````````````` example
> *foo bar *
> .
> <p>*foo bar *</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-closing]
> A newline also counts as whitespace:
>
> ```````````````````````````````` example
> *foo bar
> *
> .
> <p>*foo bar
> *</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.asterisk-cannot-close-after-punctuation-before-alphanumeric]
> This is not emphasis, because the second `*` is
> preceded by punctuation and followed by an alphanumeric
> (hence it is not part of a [right-flanking delimiter run]:
>
> ```````````````````````````````` example
> *(*foo)
> .
> <p>*(*foo)</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.asterisk-closing-restriction-example]
> The point of this restriction is more easily appreciated
> with this example:
>
> ```````````````````````````````` example
> *(*foo*)*
> .
> <p><em>(<em>foo</em>)</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-allowed]
> Intraword emphasis with `*` is allowed:
>
> ```````````````````````````````` example
> *foo*bar
> .
> <p><em>foo</em>bar</p>
> ````````````````````````````````
>
> Rule 4:

> gfm[inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-whitespace]
> This is not emphasis, because the closing `_` is preceded by
> whitespace:
>
> ```````````````````````````````` example
> _foo bar _
> .
> <p>_foo bar _</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.underscore-cannot-close-after-punctuation-before-alphanumeric]
> This is not emphasis, because the second `_` is
> preceded by punctuation and followed by an alphanumeric:
>
> ```````````````````````````````` example
> _(_foo)
> .
> <p>_(_foo)</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.is-emphasis-within-emphasis]
> This is emphasis within emphasis:
>
> ```````````````````````````````` example
> _(_foo_)_
> .
> <p><em>(<em>foo</em>)</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-emphasis-is-disallowed]
> Intraword emphasis is disallowed for `_`:
>
> ```````````````````````````````` example
> _foo_bar
> .
> <p>_foo_bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _пристаням_стремятся
> .
> <p>_пристаням_стремятся</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _foo_bar_baz_
> .
> <p><em>foo_bar_baz</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.underscore-can-close-when-punctuation-follows]
> This is emphasis, even though the closing delimiter is
> both left- and right-flanking, because it is followed by
> punctuation:
>
> ```````````````````````````````` example
> _(bar)_.
> .
> <p><em>(bar)</em>.</p>
> ````````````````````````````````
>
> Rule 5:
>
> ```````````````````````````````` example
> **foo bar**
> .
> <p><strong>foo bar</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-whitespace]
> This is not strong emphasis, because the opening delimiter is
> followed by whitespace:
>
> ```````````````````````````````` example
> ** foo bar**
> .
> <p>** foo bar**</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-open-after-alphanumeric-before-punctuation]
> This is not strong emphasis, because the opening `**` is preceded
> by an alphanumeric and followed by punctuation, and hence
> not part of a [left-flanking delimiter run]:
>
> ```````````````````````````````` example
> a**"foo"**
> .
> <p>a**&quot;foo&quot;**</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-permitted]
> Intraword strong emphasis with `**` is permitted:
>
> ```````````````````````````````` example
> foo**bar**
> .
> <p>foo<strong>bar</strong></p>
> ````````````````````````````````
>
> Rule 6:
>
> ```````````````````````````````` example
> __foo bar__
> .
> <p><strong>foo bar</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-whitespace]
> This is not strong emphasis, because the opening delimiter is
> followed by whitespace:
>
> ```````````````````````````````` example
> __ foo bar__
> .
> <p>__ foo bar__</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.newline-counts-as-whitespace-for-double-underscore]
> A newline counts as whitespace:
>
> ```````````````````````````````` example
> __
> foo bar__
> .
> <p>__
> foo bar__</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-cannot-open-after-alphanumeric-before-punctuation]
> This is not strong emphasis, because the opening `__` is preceded
> by an alphanumeric and followed by punctuation:
>
> ```````````````````````````````` example
> a__"foo"__
> .
> <p>a__&quot;foo&quot;__</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-strong-emphasis-is-forbidden]
> Intraword strong emphasis is forbidden with `__`:
>
> ```````````````````````````````` example
> foo__bar__
> .
> <p>foo__bar__</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> 5__6__78
> .
> <p>5__6__78</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> пристаням__стремятся__
> .
> <p>пристаням__стремятся__</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo, __bar__, baz__
> .
> <p><strong>foo, bar, baz</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-can-open-when-punctuation-precedes]
> This is strong emphasis, even though the opening delimiter is
> both left- and right-flanking, because it is preceded by
> punctuation:
>
> ```````````````````````````````` example
> foo-__(bar)__
> .
> <p>foo-<strong>(bar)</strong></p>
> ````````````````````````````````
>
> Rule 7:

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-whitespace]
> This is not strong emphasis, because the closing delimiter is preceded
> by whitespace:
>
> ```````````````````````````````` example
> **foo bar **
> .
> <p>**foo bar **</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-closing-cannot-fall-back-to-single-asterisk-rule]
> (Nor can it be interpreted as an emphasized `*foo bar *`, because of
> Rule 11.)

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-cannot-close-after-punctuation-before-alphanumeric]
> This is not strong emphasis, because the second `**` is
> preceded by punctuation and followed by an alphanumeric:
>
> ```````````````````````````````` example
> **(**foo)
> .
> <p>**(**foo)</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-asterisk-closing-restriction-examples]
> The point of this restriction is more easily appreciated
> with these examples:
>
> ```````````````````````````````` example
> *(**foo**)*
> .
> <p><em>(<strong>foo</strong>)</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **Gomphocarpus (*Gomphocarpus physocarpus*, syn.
> *Asclepias physocarpa*)**
> .
> <p><strong>Gomphocarpus (<em>Gomphocarpus physocarpus</em>, syn.
> <em>Asclepias physocarpa</em>)</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo "*bar*" foo**
> .
> <p><strong>foo &quot;<em>bar</em>&quot; foo</strong></p>
> ````````````````````````````````
>
> Intraword emphasis:
>
> ```````````````````````````````` example
> **foo**bar
> .
> <p><strong>foo</strong>bar</p>
> ````````````````````````````````
>
> Rule 8:

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-whitespace]
> This is not strong emphasis, because the closing delimiter is
> preceded by whitespace:
>
> ```````````````````````````````` example
> __foo bar __
> .
> <p>__foo bar __</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-cannot-close-after-punctuation-before-alphanumeric]
> This is not strong emphasis, because the second `__` is
> preceded by punctuation and followed by an alphanumeric:
>
> ```````````````````````````````` example
> __(__foo)
> .
> <p>__(__foo)</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-closing-restriction-example]
> The point of this restriction is more easily appreciated
> with this example:
>
> ```````````````````````````````` example
> _(__foo__)_
> .
> <p><em>(<strong>foo</strong>)</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.intraword-double-underscore-closing-is-forbidden]
> Intraword strong emphasis is forbidden with `__`:
>
> ```````````````````````````````` example
> __foo__bar
> .
> <p>__foo__bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __пристаням__стремятся
> .
> <p>__пристаням__стремятся</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo__bar__baz__
> .
> <p><strong>foo__bar__baz</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.double-underscore-can-close-when-punctuation-follows]
> This is strong emphasis, even though the closing delimiter is
> both left- and right-flanking, because it is followed by
> punctuation:
>
> ```````````````````````````````` example
> __(bar)__.
> .
> <p><strong>(bar)</strong>.</p>
> ````````````````````````````````
>
> Rule 9:

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-may-contain-any-nonempty-inlines]
> Any nonempty sequence of inline elements can be the contents of an
> emphasized span.
>
> ```````````````````````````````` example
> *foo [bar](/url)*
> .
> <p><em>foo <a href="/url">bar</a></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo
> bar*
> .
> <p><em>foo
> bar</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-emphasis]
> In particular, emphasis and strong emphasis can be nested
> inside emphasis:
>
> ```````````````````````````````` example
> _foo __bar__ baz_
> .
> <p><em>foo <strong>bar</strong> baz</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _foo _bar_ baz_
> .
> <p><em>foo <em>bar</em> baz</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo_ bar_
> .
> <p><em><em>foo</em> bar</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo *bar**
> .
> <p><em>foo <em>bar</em></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo **bar** baz*
> .
> <p><em>foo <strong>bar</strong> baz</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo**bar**baz*
> .
> <p><em>foo<strong>bar</strong>baz</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-alternative]
> Note that in the preceding case, the interpretation
>
> ``` markdown
> <p><em>foo</em><em>bar<em></em>baz</em></p>
> ```

> gfm[inlines.emphasis-and-strong-emphasis.multiple-of-three-condition-explanation]
> is precluded by the condition that a delimiter that
> can both open and close (like the `*` after `foo`)
> cannot form emphasis if the sum of the lengths of
> the delimiter runs containing the opening and
> closing delimiters is a multiple of 3 unless
> both lengths are multiples of 3.

> gfm[inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-prevents-consecutive-emphasis]
> For the same reason, we don't get two consecutive
> emphasis sections in this example:
>
> ```````````````````````````````` example
> *foo**bar*
> .
> <p><em>foo**bar</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.multiple-of-three-rule-produces-nested-strong]
> The same condition ensures that the following
> cases are all strong emphasis nested inside
> emphasis, even when the interior spaces are
> omitted:
>
> ```````````````````````````````` example
> ***foo** bar*
> .
> <p><em><strong>foo</strong> bar</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo **bar***
> .
> <p><em>foo <strong>bar</strong></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo**bar***
> .
> <p><em>foo<strong>bar</strong></em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.both-multiples-of-three-allow-emphasis-match]
> When the lengths of the interior closing and opening
> delimiter runs are *both* multiples of 3, though,
> they can match to create emphasis:
>
> ```````````````````````````````` example
> foo***bar***baz
> .
> <p>foo<em><strong>bar</strong></em>baz</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo******bar*********baz
> .
> <p>foo<strong>bar</strong>***baz</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-allows-indefinite-nesting]
> Indefinite levels of nesting are possible:
>
> ```````````````````````````````` example
> *foo **bar *baz* bim** bop*
> .
> <p><em>foo <strong>bar <em>baz</em> bim</strong> bop</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo [*bar*](/url)*
> .
> <p><em>foo <a href="/url"><em>bar</em></a></em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.empty-asterisk-emphasis-is-not-allowed]
> There can be no empty emphasis or strong emphasis:
>
> ```````````````````````````````` example
> ** is not an empty emphasis
> .
> <p>** is not an empty emphasis</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **** is not an empty strong emphasis
> .
> <p>**** is not an empty strong emphasis</p>
> ````````````````````````````````
>
> Rule 10:

> gfm[inlines.emphasis-and-strong-emphasis.strong-emphasis-may-contain-any-nonempty-inlines]
> Any nonempty sequence of inline elements can be the contents of an
> strongly emphasized span.
>
> ```````````````````````````````` example
> **foo [bar](/url)**
> .
> <p><strong>foo <a href="/url">bar</a></strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo
> bar**
> .
> <p><strong>foo
> bar</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.emphasis-and-strong-may-nest-inside-strong]
> In particular, emphasis and strong emphasis can be nested
> inside strong emphasis:
>
> ```````````````````````````````` example
> __foo _bar_ baz__
> .
> <p><strong>foo <em>bar</em> baz</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo __bar__ baz__
> .
> <p><strong>foo bar baz</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ____foo__ bar__
> .
> <p><strong>foo bar</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo **bar****
> .
> <p><strong>foo bar</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo *bar* baz**
> .
> <p><strong>foo <em>bar</em> baz</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo*bar*baz**
> .
> <p><strong>foo<em>bar</em>baz</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ***foo* bar**
> .
> <p><strong><em>foo</em> bar</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo *bar***
> .
> <p><strong>foo <em>bar</em></strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.strong-emphasis-allows-indefinite-nesting]
> Indefinite levels of nesting are possible:
>
> ```````````````````````````````` example
> **foo *bar **baz**
> bim* bop**
> .
> <p><strong>foo <em>bar <strong>baz</strong>
> bim</em> bop</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo [*bar*](/url)**
> .
> <p><strong>foo <a href="/url"><em>bar</em></a></strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.empty-underscore-emphasis-is-not-allowed]
> There can be no empty emphasis or strong emphasis:
>
> ```````````````````````````````` example
> __ is not an empty emphasis
> .
> <p>__ is not an empty emphasis</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ____ is not an empty strong emphasis
> .
> <p>____ is not an empty strong emphasis</p>
> ````````````````````````````````
>
> Rule 11:
>
> ```````````````````````````````` example
> foo ***
> .
> <p>foo ***</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo *\**
> .
> <p>foo <em>*</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo *_*
> .
> <p>foo <em>_</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo *****
> .
> <p>foo *****</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo **\***
> .
> <p>foo <strong>*</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo **_**
> .
> <p>foo <strong>_</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.rule-11-puts-excess-asterisks-outside-emphasis]
> Note that when delimiters do not match evenly, Rule 11 determines
> that the excess literal `*` characters will appear outside of the
> emphasis, rather than inside it:
>
> ```````````````````````````````` example
> **foo*
> .
> <p>*<em>foo</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo**
> .
> <p><em>foo</em>*</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ***foo**
> .
> <p>*<strong>foo</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ****foo*
> .
> <p>***<em>foo</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **foo***
> .
> <p><strong>foo</strong>*</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo****
> .
> <p><em>foo</em>***</p>
> ````````````````````````````````
>
> Rule 12:
>
> ```````````````````````````````` example
> foo ___
> .
> <p>foo ___</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo _\__
> .
> <p>foo <em>_</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo _*_
> .
> <p>foo <em>*</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo _____
> .
> <p>foo _____</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo __\___
> .
> <p>foo <strong>_</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo __*__
> .
> <p>foo <strong>*</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo_
> .
> <p>_<em>foo</em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.rule-12-puts-excess-underscores-outside-emphasis]
> Note that when delimiters do not match evenly, Rule 12 determines
> that the excess literal `_` characters will appear outside of the
> emphasis, rather than inside it:
>
> ```````````````````````````````` example
> _foo__
> .
> <p><em>foo</em>_</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ___foo__
> .
> <p>_<strong>foo</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ____foo_
> .
> <p>___<em>foo</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo___
> .
> <p><strong>foo</strong>_</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _foo____
> .
> <p><em>foo</em>___</p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.rule-13-requires-different-directly-nested-delimiters]
> Rule 13 implies that if you want emphasis nested directly inside
> emphasis, you must use different delimiters:
>
> ```````````````````````````````` example
> **foo**
> .
> <p><strong>foo</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *_foo_*
> .
> <p><em><em>foo</em></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __foo__
> .
> <p><strong>foo</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _*foo*_
> .
> <p><em><em>foo</em></em></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.strong-can-nest-inside-strong-with-same-delimiter]
> However, strong emphasis within strong emphasis is possible without
> switching delimiters:
>
> ```````````````````````````````` example
> ****foo****
> .
> <p><strong>foo</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ____foo____
> .
> <p><strong>foo</strong></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.rule-13-applies-to-long-delimiter-runs]
> Rule 13 can be applied to arbitrarily long sequences of
> delimiters:
>
> ```````````````````````````````` example
> ******foo******
> .
> <p><strong>foo</strong></p>
> ````````````````````````````````
>
> Rule 14:
>
> ```````````````````````````````` example
> ***foo***
> .
> <p><em><strong>foo</strong></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _____foo_____
> .
> <p><em><strong>foo</strong></em></p>
> ````````````````````````````````
>
> Rule 15:
>
> ```````````````````````````````` example
> *foo _bar* baz_
> .
> <p><em>foo _bar</em> baz_</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo __bar *baz bim__ bam*
> .
> <p><em>foo <strong>bar *baz bim</strong> bam</em></p>
> ````````````````````````````````
>
> Rule 16:
>
> ```````````````````````````````` example
> **foo **bar baz**
> .
> <p>**foo <strong>bar baz</strong></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo *bar baz*
> .
> <p>*foo <em>bar baz</em></p>
> ````````````````````````````````
>
> Rule 17:
>
> ```````````````````````````````` example
> *[bar*](/url)
> .
> <p>*<a href="/url">bar*</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _foo [bar_](/url)
> .
> <p>_foo <a href="/url">bar_</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *<img src="foo" title="*"/>
> .
> <p>*<img src="foo" title="*"/></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **<a href="**">
> .
> <p>**<a href="**"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __<a href="__">
> .
> <p>__<a href="__"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *a `*`*
> .
> <p><em>a <code>*</code></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> _a `_`_
> .
> <p><em>a <code>_</code></em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> **a<http://foo.bar/?q=**>
> .
> <p>**a<a href="http://foo.bar/?q=**">http://foo.bar/?q=**</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> __a<http://foo.bar/?q=__>
> .
> <p>__a<a href="http://foo.bar/?q=__">http://foo.bar/?q=__</a></p>
> ````````````````````````````````

> gfm[inlines.emphasis-and-strong-emphasis.extension-wrapper]
> <div class="extension">

## Strikethrough (extension)

> gfm[inlines.strikethrough-extension.gfm-enables-strikethrough-extension]
> GFM enables the `strikethrough` extension, where an additional emphasis type is
> available.

> gfm[inlines.strikethrough-extension.strikethrough-uses-double-tilde-delimiters]
> Strikethrough text is any text wrapped in two tildes (`~`).
>
> ```````````````````````````````` example strikethrough
> ~~Hi~~ Hello, world!
> .
> <p><del>Hi</del> Hello, world!</p>
> ````````````````````````````````

> gfm[inlines.strikethrough-extension.new-paragraph-ends-strikethrough-parse]
> As with regular emphasis delimiters, a new paragraph will cause strikethrough
> parsing to cease:
>
> ```````````````````````````````` example strikethrough
> This ~~has a
>
> new paragraph~~.
> .
> <p>This ~~has a</p>
> <p>new paragraph~~.</p>
> ````````````````````````````````

> gfm[inlines.strikethrough-extension.extension-wrapper]
> </div>

## Links

> gfm[inlines.links.definition]
> A link contains [link text] (the visible text), a [link destination]
> (the URI that is the link destination), and optionally a [link title].

> gfm[inlines.links.inline-and-reference-links-are-the-two-basic-kinds]
> There are two basic kinds of links in Markdown. In [inline links] the
> destination and title are given immediately after the link text. In
> [reference links] the destination and title are defined elsewhere in
> the document.

> gfm[inlines.links.link-text-definition]
> A [link text](@) consists of a sequence of zero or more
> inline elements enclosed by square brackets (`[` and `]`).

> gfm[inlines.links.link-text-cannot-contain-links]
> Links may not contain other links, at any level of nesting. If
> multiple otherwise valid link definitions appear nested inside each
> other, the inner-most definition is used.

> gfm[inlines.links.link-text-allows-only-escaped-or-balanced-brackets]
> Brackets are allowed in the [link text] only if (a) they are
> backslash-escaped or (b) they appear as a matched pair of brackets,
> with an open bracket `[`, a sequence of zero or more inlines, and a
> close bracket `]`.

> gfm[inlines.links.code-autolink-and-html-bind-tighter-than-link-text]
> Backtick [code spans], [autolinks], and raw [HTML tags] bind more tightly
> than the brackets in link text.

> gfm[inlines.links.link-text-brackets-bind-more-tightly-than-emphasis]
> The brackets in link text bind more tightly than markers for
> [emphasis and strong emphasis].

> gfm[inlines.links.link-destination-definition]
> A [link destination](@) consists of either
>
> - a sequence of zero or more characters between an opening `<` and a
>   closing `>` that contains no line breaks or unescaped
>   `<` or `>` characters, or
>
> - a nonempty sequence of characters that does not start with
>   `<`, does not include ASCII space or control characters, and
>   includes parentheses only if (a) they are backslash-escaped or
>   (b) they are part of a balanced pair of unescaped parentheses.

> gfm[inlines.links.implementations-may-limit-parenthesis-nesting-in-destinations]
> Implementations may impose limits on parentheses nesting to avoid
> performance issues, but at least three levels of nesting should be
> supported.

> gfm[inlines.links.link-title-definition]
> A [link title](@)  consists of either
>
> - a sequence of zero or more characters between straight double-quote
>   characters (`"`), including a `"` character only if it is
>   backslash-escaped, or
>
> - a sequence of zero or more characters between straight single-quote
>   characters (`'`), including a `'` character only if it is
>   backslash-escaped, or
>
> - a sequence of zero or more characters between matching parentheses
>   (`(...)`), including a `(` or `)` character only if it is
>   backslash-escaped.

> gfm[inlines.links.link-titles-may-span-lines-but-not-blank-lines]
> Although [link titles] may span multiple lines, they may not contain
> a [blank line].

> gfm[inlines.links.inline-link-definition]
> An [inline link](@) consists of a [link text] followed immediately
> by a left parenthesis `(`, optional [whitespace], an optional
> [link destination], an optional [link title] separated from the link
> destination by [whitespace], optional [whitespace], and a right
> parenthesis `)`.

> gfm[inlines.links.inline-link-text-comes-from-link-text-inlines]
> The link's text consists of the inlines contained in the [link text]
> excluding the enclosing square brackets.

> gfm[inlines.links.inline-link-uri-comes-from-destination-with-delimiters-removed]
> The link's URI consists of the link destination, excluding enclosing
> `<...>` if present, with backslash-escapes in effect.

> gfm[inlines.links.inline-link-title-comes-from-title-with-delimiters-removed]
> The link's title consists of the link title, excluding its enclosing
> delimiters, with backslash-escapes in effect.

> gfm[inlines.links.inline-link-example]
> Here is a simple inline link:
>
> ```````````````````````````````` example
> [link](/uri "title")
> .
> <p><a href="/uri" title="title">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.title-may-be-omitted]
> The title may be omitted:
>
> ```````````````````````````````` example
> [link](/uri)
> .
> <p><a href="/uri">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-and-title-may-both-be-omitted]
> Both the title and the destination may be omitted:
>
> ```````````````````````````````` example
> [link]()
> .
> <p><a href="">link</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link](<>)
> .
> <p><a href="">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-may-contain-spaces-only-in-angle-brackets]
> The destination can only contain spaces if it is
> enclosed in pointy brackets:
>
> ```````````````````````````````` example
> [link](/my uri)
> .
> <p>[link](/my uri)</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link](</my uri>)
> .
> <p><a href="/my%20uri">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-cannot-contain-line-breaks]
> The destination cannot contain line breaks,
> even if enclosed in pointy brackets:
>
> ```````````````````````````````` example
> [link](foo
> bar)
> .
> <p>[link](foo
> bar)</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link](<foo
> bar>)
> .
> <p>[link](<foo
> bar>)</p>
> ````````````````````````````````

> gfm[inlines.links.destination-may-contain-right-parenthesis-in-angle-brackets]
> The destination can contain `)` if it is enclosed
> in pointy brackets:
>
> ```````````````````````````````` example
> [a](<b)c>)
> .
> <p><a href="b)c">a</a></p>
> ````````````````````````````````

> gfm[inlines.links.angle-brackets-around-destination-must-be-unescaped]
> Pointy brackets that enclose links must be unescaped:
>
> ```````````````````````````````` example
> [link](<foo\>)
> .
> <p>[link](&lt;foo&gt;)</p>
> ````````````````````````````````

> gfm[inlines.links.unmatched-angle-brackets-do-not-form-destination]
> These are not links, because the opening pointy bracket
> is not matched properly:
>
> ```````````````````````````````` example
> [a](<b)c
> [a](<b)c>
> [a](<b>c)
> .
> <p>[a](&lt;b)c
> [a](&lt;b)c&gt;
> [a](<b>c)</p>
> ````````````````````````````````

> gfm[inlines.links.parentheses-in-destination-may-be-escaped]
> Parentheses inside the link destination may be escaped:
>
> ```````````````````````````````` example
> [link](\(foo\))
> .
> <p><a href="(foo)">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.balanced-parentheses-are-allowed-in-destination]
> Any number of parentheses are allowed without escaping, as long as they are
> balanced:
>
> ```````````````````````````````` example
> [link](foo(and(bar)))
> .
> <p><a href="foo(and(bar))">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.unbalanced-parentheses-require-escaping-or-angle-brackets]
> However, if you have unbalanced parentheses, you need to escape or use the
> `<...>` form:
>
> ```````````````````````````````` example
> [link](foo\(and\(bar\))
> .
> <p><a href="foo(and(bar)">link</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link](<foo(and(bar)>)
> .
> <p><a href="foo(and(bar)">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-allows-normal-backslash-escaping]
> Parentheses and other symbols can also be escaped, as usual
> in Markdown:
>
> ```````````````````````````````` example
> [link](foo\)\:)
> .
> <p><a href="foo):">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-may-include-fragments-and-queries]
> A link can contain fragment identifiers and queries:
>
> ```````````````````````````````` example
> [link](#fragment)
>
> [link](http://example.com#fragment)
>
> [link](http://example.com?foo=3#frag)
> .
> <p><a href="#fragment">link</a></p>
> <p><a href="http://example.com#fragment">link</a></p>
> <p><a href="http://example.com?foo=3#frag">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.backslash-before-non-escapable-character-is-literal-in-destination]
> Note that a backslash before a non-escapable character is
> just a backslash:
>
> ```````````````````````````````` example
> [link](foo\bar)
> .
> <p><a href="foo%5Cbar">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.destination-preserves-url-escaping-and-allows-entity-references]
> URL-escaping should be left alone inside the destination, as all
> URL-escaped characters are also valid URL characters. Entity and
> numerical character references in the destination will be parsed
> into the corresponding Unicode code points, as usual.  These may
> be optionally URL-escaped when written as HTML, but this spec
> does not enforce any particular policy for rendering URLs in
> HTML or other formats.  Renderers may make different decisions
> about how to escape or normalize URLs in the output.
>
> ```````````````````````````````` example
> [link](foo%20b&auml;)
> .
> <p><a href="foo%20b%C3%A4">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.title-cannot-appear-without-destination]
> Note that, because titles can often be parsed as destinations,
> if you try to omit the destination and keep the title, you'll
> get unexpected results:
>
> ```````````````````````````````` example
> [link]("title")
> .
> <p><a href="%22title%22">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.titles-may-use-single-quotes-double-quotes-or-parentheses]
> Titles may be in single quotes, double quotes, or parentheses:
>
> ```````````````````````````````` example
> [link](/url "title")
> [link](/url 'title')
> [link](/url (title))
> .
> <p><a href="/url" title="title">link</a>
> <a href="/url" title="title">link</a>
> <a href="/url" title="title">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.titles-allow-backslash-escapes-and-entity-references]
> Backslash escapes and entity and numeric character references
> may be used in titles:
>
> ```````````````````````````````` example
> [link](/url "title \"&quot;")
> .
> <p><a href="/url" title="title &quot;&quot;">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.title-must-be-separated-from-destination-by-whitespace]
> Titles must be separated from the link using a [whitespace].
> Other [Unicode whitespace] like non-breaking space doesn't work.
>
> ```````````````````````````````` example
> [link](/url "title")
> .
> <p><a href="/url%C2%A0%22title%22">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.nested-balanced-quotes-in-titles-require-escaping]
> Nested balanced quotes are not allowed without escaping:
>
> ```````````````````````````````` example
> [link](/url "title "and" title")
> .
> <p>[link](/url &quot;title &quot;and&quot; title&quot;)</p>
> ````````````````````````````````

> gfm[inlines.links.different-title-delimiter-can-avoid-escaping]
> But it is easy to work around this by using a different quote type:
>
> ```````````````````````````````` example
> [link](/url 'title "and" title')
> .
> <p><a href="/url" title="title &quot;and&quot; title">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.markdown-pl-title-behavior]
> (Note:  `Markdown.pl` did allow double quotes inside a double-quoted
> title, and its test suite included a test demonstrating this.
> But it is hard to see a good rationale for the extra complexity this
> brings, since there are already many ways---backslash escaping,
> entity and numeric character references, or using a different
> quote type for the enclosing title---to write titles containing
> double quotes.  `Markdown.pl`'s handling of titles has a number
> of other strange features.  For example, it allows single-quoted
> titles in inline links, but not reference links.  And, in
> reference links but not inline links, it allows a title to begin
> with `"` and end with `)`.  `Markdown.pl` 1.0.1 even allows
> titles with no closing quotation mark, though 1.0.2b8 does not.
> It seems preferable to adopt a simple, rational rule that works
> the same way in inline links and link reference definitions.)

> gfm[inlines.links.whitespace-is-allowed-inside-inline-link-delimiters]
> [Whitespace] is allowed around the destination and title:
>
> ```````````````````````````````` example
> [link](   /uri
>   "title"  )
> .
> <p><a href="/uri" title="title">link</a></p>
> ````````````````````````````````

> gfm[inlines.links.no-whitespace-allowed-between-link-text-and-opening-parenthesis]
> But it is not allowed between the link text and the
> following parenthesis:
>
> ```````````````````````````````` example
> [link] (/uri)
> .
> <p>[link] (/uri)</p>
> ````````````````````````````````

> gfm[inlines.links.link-text-may-contain-balanced-brackets-but-not-unbalanced-ones]
> The link text may contain balanced brackets, but not unbalanced ones,
> unless they are escaped:
>
> ```````````````````````````````` example
> [link [foo [bar]]](/uri)
> .
> <p><a href="/uri">link [foo [bar]]</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link] bar](/uri)
> .
> <p>[link] bar](/uri)</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link [bar](/uri)
> .
> <p>[link <a href="/uri">bar</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link \[bar](/uri)
> .
> <p><a href="/uri">link [bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.link-text-may-contain-inline-content]
> The link text may contain inline content:
>
> ```````````````````````````````` example
> [link *foo **bar** `#`*](/uri)
> .
> <p><a href="/uri">link <em>foo <strong>bar</strong> <code>#</code></em></a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [![moon](moon.jpg)](/uri)
> .
> <p><a href="/uri"><img src="moon.jpg" alt="moon" /></a></p>
> ````````````````````````````````

> gfm[inlines.links.links-may-not-contain-other-links-at-any-level-nesting]
> However, links may not contain other links, at any level of nesting.
>
> ```````````````````````````````` example
> [foo [bar](/uri)](/uri)
> .
> <p>[foo <a href="/uri">bar</a>](/uri)</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo *[bar [baz](/uri)](/uri)*](/uri)
> .
> <p>[foo <em>[bar <a href="/uri">baz</a>](/uri)</em>](/uri)</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![[[foo](uri1)](uri2)](uri3)
> .
> <p><img src="uri3" alt="[foo](uri2)" /></p>
> ````````````````````````````````

> gfm[inlines.links.cases-illustrate-precedence-link-text-grouping-over-emphasis-grouping]
> These cases illustrate the precedence of link text grouping over
> emphasis grouping:
>
> ```````````````````````````````` example
> *[foo*](/uri)
> .
> <p>*<a href="/uri">foo*</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo *bar](baz*)
> .
> <p><a href="baz*">foo *bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.brackets-aren-t-part-links-do-not-take-precedence]
> Note that brackets that *aren't* part of links do not take
> precedence:
>
> ```````````````````````````````` example
> *foo [bar* baz]
> .
> <p><em>foo [bar</em> baz]</p>
> ````````````````````````````````

> gfm[inlines.links.cases-illustrate-precedence-html-tags-code-spans-autolinks-over-link]
> These cases illustrate the precedence of HTML tags, code spans,
> and autolinks over link grouping:
>
> ```````````````````````````````` example
> [foo <bar attr="](baz)">
> .
> <p>[foo <bar attr="](baz)"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo`](/uri)`
> .
> <p>[foo<code>](/uri)</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo<http://example.com/?search=](uri)>
> .
> <p>[foo<a href="http://example.com/?search=%5D(uri)">http://example.com/?search=](uri)</a></p>
> ````````````````````````````````

> gfm[inlines.links.reference-links-have-three-kinds]
> There are three kinds of [reference link](@)s:
> [full](#full-reference-link), [collapsed](#collapsed-reference-link),
> and [shortcut](#shortcut-reference-link).

> gfm[inlines.links.full-reference-link-definition]
> A [full reference link](@)
> consists of a [link text] immediately followed by a [link label]
> that [matches] a [link reference definition] elsewhere in the document.

> gfm[inlines.links.link-label-definition]
> A [link label](@)  begins with a left bracket (`[`) and ends
> with the first right bracket (`]`) that is not backslash-escaped.

> gfm[inlines.links.link-label-must-contain-non-whitespace]
> Between these brackets there must be at least one [non-whitespace character].

> gfm[inlines.links.link-label-cannot-contain-unescaped-brackets]
> Unescaped square bracket characters are not allowed inside the
> opening and closing square brackets of [link labels].

> gfm[inlines.links.link-label-length-is-at-most-999-characters]
> A link label can have at most 999 characters inside the square
> brackets.

> gfm[inlines.links.label-match-definition]
> One label [matches](@)
> another just in case their normalized forms are equal.  To normalize a
> label, strip off the opening and closing brackets,
> perform the *Unicode case fold*, strip leading and trailing
> [whitespace] and collapse consecutive internal
> [whitespace] to a single space.

> gfm[inlines.links.first-matching-reference-definition-is-used]
> If there are multiple matching reference link definitions, the one
> that comes first in the document is used.

> gfm[inlines.links.reference-link-text-comes-from-first-label-inlines]
> The contents of the first link label are parsed as inlines, which are
> used as the link's text.  The link's URI and title are provided by the
> matching [link reference definition].

> gfm[inlines.links.full-reference-link-example]
> Here is a simple example:
>
> ```````````````````````````````` example
> [foo][bar]
>
> [bar]: /url "title"
> .
> <p><a href="/url" title="title">foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.reference-link-text-follows-inline-link-text-rules]
> The rules for the [link text] are the same as with
> [inline links].  Thus:

> gfm[inlines.links.reference-link-text-allows-balanced-or-escaped-brackets]
> The link text may contain balanced brackets, but not unbalanced ones,
> unless they are escaped:
>
> ```````````````````````````````` example
> [link [foo [bar]]][ref]
>
> [ref]: /uri
> .
> <p><a href="/uri">link [foo [bar]]</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [link \[bar][ref]
>
> [ref]: /uri
> .
> <p><a href="/uri">link [bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.reference-link-text-may-contain-inline-content]
> The link text may contain inline content:
>
> ```````````````````````````````` example
> [link *foo **bar** `#`*][ref]
>
> [ref]: /uri
> .
> <p><a href="/uri">link <em>foo <strong>bar</strong> <code>#</code></em></a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [![moon](moon.jpg)][ref]
>
> [ref]: /uri
> .
> <p><a href="/uri"><img src="moon.jpg" alt="moon" /></a></p>
> ````````````````````````````````

> gfm[inlines.links.reference-link-text-cannot-contain-links]
> However, links may not contain other links, at any level of nesting.
>
> ```````````````````````````````` example
> [foo [bar](/uri)][ref]
>
> [ref]: /uri
> .
> <p>[foo <a href="/uri">bar</a>]<a href="/uri">ref</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo *bar [baz][ref]*][ref]
>
> [ref]: /uri
> .
> <p>[foo <em>bar <a href="/uri">baz</a></em>]<a href="/uri">ref</a></p>
> ````````````````````````````````

> gfm[inlines.links.nested-reference-link-examples-produce-shortcuts]
> (In the examples above, we have two [shortcut reference links]
> instead of one [full reference link].)

> gfm[inlines.links.reference-link-text-grouping-takes-precedence-over-emphasis]
> The following cases illustrate the precedence of link text grouping over
> emphasis grouping:
>
> ```````````````````````````````` example
> *[foo*][ref]
>
> [ref]: /uri
> .
> <p>*<a href="/uri">foo*</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo *bar][ref]
>
> [ref]: /uri
> .
> <p><a href="/uri">foo *bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.reference-links-lose-to-html-tags-code-spans-and-autolinks]
> These cases illustrate the precedence of HTML tags, code spans,
> and autolinks over link grouping:
>
> ```````````````````````````````` example
> [foo <bar attr="][ref]">
>
> [ref]: /uri
> .
> <p>[foo <bar attr="][ref]"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo`][ref]`
>
> [ref]: /uri
> .
> <p>[foo<code>][ref]</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo<http://example.com/?search=][ref]>
>
> [ref]: /uri
> .
> <p>[foo<a href="http://example.com/?search=%5D%5Bref%5D">http://example.com/?search=][ref]</a></p>
> ````````````````````````````````
>
> Matching is case-insensitive:
>
> ```````````````````````````````` example
> [foo][BaR]
>
> [bar]: /url "title"
> .
> <p><a href="/url" title="title">foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.label-matching-uses-unicode-case-fold]
> Unicode case fold is used:
>
> ```````````````````````````````` example
> [Толпой][Толпой] is a Russian word.
>
> [ТОЛПОЙ]: /url
> .
> <p><a href="/url">Толпой</a> is a Russian word.</p>
> ````````````````````````````````

> gfm[inlines.links.label-matching-collapses-consecutive-internal-whitespace]
> Consecutive internal [whitespace] is treated as one space for
> purposes of determining matching:
>
> ```````````````````````````````` example
> [Foo
>   bar]: /url
>
> [Baz][Foo bar]
> .
> <p><a href="/url">Baz</a></p>
> ````````````````````````````````

> gfm[inlines.links.no-whitespace-allowed-between-link-text-and-link-label]
> No [whitespace] is allowed between the [link text] and the
> [link label]:
>
> ```````````````````````````````` example
> [foo] [bar]
>
> [bar]: /url "title"
> .
> <p>[foo] <a href="/url" title="title">bar</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo]
> [bar]
>
> [bar]: /url "title"
> .
> <p>[foo]
> <a href="/url" title="title">bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.no-space-before-link-label-departs-from-original-markdown]
> This is a departure from John Gruber's original Markdown syntax
> description, which explicitly allows whitespace between the link
> text and the link label.  It brings reference links in line with
> [inline links], which (according to both original Markdown and
> this spec) cannot have whitespace after the link text.  More
> importantly, it prevents inadvertent capture of consecutive
> [shortcut reference links]. If whitespace is allowed between the
> link text and the link label, then in the following we will have
> a single reference link, not two shortcut reference links, as
> intended:
>
> ``` markdown
> [foo]
> [bar]
>
> [foo]: /url1
> [bar]: /url2
> ```

> gfm[inlines.links.shortcut-reference-links-make-space-before-label-too-dangerous]
> (Note that [shortcut reference links] were introduced by Gruber
> himself in a beta version of `Markdown.pl`, but never included
> in the official syntax description.  Without shortcut reference
> links, it is harmless to allow space between the link text and
> link label; but once shortcut references are introduced, it is
> too dangerous to allow this, as it frequently leads to
> unintended results.)

> gfm[inlines.links.first-reference-definition-wins-example]
> When there are multiple matching [link reference definitions],
> the first is used:
>
> ```````````````````````````````` example
> [foo]: /url1
>
> [foo]: /url2
>
> [bar][foo]
> .
> <p><a href="/url1">bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.matching-uses-normalized-strings-not-parsed-inline-content]
> Note that matching is performed on normalized strings, not parsed
> inline content.  So the following does not match, even though the
> labels define equivalent inline content:
>
> ```````````````````````````````` example
> [bar][foo\!]
>
> [foo!]: /url
> .
> <p>[bar][foo!]</p>
> ````````````````````````````````

> gfm[inlines.links.link-labels-cannot-contain-unescaped-brackets]
> [Link labels] cannot contain brackets, unless they are
> backslash-escaped:
>
> ```````````````````````````````` example
> [foo][ref[]
>
> [ref[]: /uri
> .
> <p>[foo][ref[]</p>
> <p>[ref[]: /uri</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo][ref[bar]]
>
> [ref[bar]]: /uri
> .
> <p>[foo][ref[bar]]</p>
> <p>[ref[bar]]: /uri</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [[[foo]]]
>
> [[[foo]]]: /url
> .
> <p>[[[foo]]]</p>
> <p>[[[foo]]]: /url</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo][ref\[]
>
> [ref\[]: /uri
> .
> <p><a href="/uri">foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.trailing-bracket-is-not-escaped-by-backslash-before-backslash]
> Note that in this example `]` is not backslash-escaped:
>
> ```````````````````````````````` example
> [bar\\]: /uri
>
> [bar\\]
> .
> <p><a href="/uri">bar\</a></p>
> ````````````````````````````````

> gfm[inlines.links.link-label-must-contain-at-least-one-non-whitespace-character]
> A [link label] must contain at least one [non-whitespace character]:
>
> ```````````````````````````````` example
> []
>
> []: /uri
> .
> <p>[]</p>
> <p>[]: /uri</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [
>  ]
>
> [
>  ]: /uri
> .
> <p>[
> ]</p>
> <p>[
> ]: /uri</p>
> ````````````````````````````````

> gfm[inlines.links.collapsed-reference-link-definition]
> A [collapsed reference link](@)
> consists of a [link label] that [matches] a
> [link reference definition] elsewhere in the
> document, followed by the string `[]`.

> gfm[inlines.links.collapsed-reference-link-text-comes-from-first-label-inlines]
> The contents of the first link label are parsed as inlines,
> which are used as the link's text. The link's URI and title are
> provided by the matching reference link definition.

> gfm[inlines.links.collapsed-reference-link-is-equivalent-to-full-link-with-same-label]
> `[foo][]` is equivalent to `[foo][foo]`.
>
> ```````````````````````````````` example
> [foo][]
>
> [foo]: /url "title"
> .
> <p><a href="/url" title="title">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [*foo* bar][]
>
> [*foo* bar]: /url "title"
> .
> <p><a href="/url" title="title"><em>foo</em> bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.collapsed-reference-link-labels-are-case-insensitive]
> The link labels are case-insensitive:
>
> ```````````````````````````````` example
> [Foo][]
>
> [foo]: /url "title"
> .
> <p><a href="/url" title="title">Foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.no-whitespace-allowed-between-collapsed-reference-brackets]
> As with full reference links, [whitespace] is not
> allowed between the two sets of brackets:
>
> ```````````````````````````````` example
> [foo] 
> []
>
> [foo]: /url "title"
> .
> <p><a href="/url" title="title">foo</a>
> []</p>
> ````````````````````````````````

> gfm[inlines.links.shortcut-reference-link-definition]
> A [shortcut reference link](@)
> consists of a [link label] that [matches] a
> [link reference definition] elsewhere in the
> document and is not followed by `[]` or a link label.

> gfm[inlines.links.shortcut-reference-link-text-comes-from-label-inlines]
> The contents of the first link label are parsed as inlines,
> which are used as the link's text. The link's URI and title
> are provided by the matching link reference definition.

> gfm[inlines.links.shortcut-reference-link-is-equivalent-to-collapsed-reference-link]
> `[foo]` is equivalent to `[foo][]`.
>
> ```````````````````````````````` example
> [foo]
>
> [foo]: /url "title"
> .
> <p><a href="/url" title="title">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [*foo* bar]
>
> [*foo* bar]: /url "title"
> .
> <p><a href="/url" title="title"><em>foo</em> bar</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [[*foo* bar]]
>
> [*foo* bar]: /url "title"
> .
> <p>[<a href="/url" title="title"><em>foo</em> bar</a>]</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [[bar [foo]
>
> [foo]: /url
> .
> <p>[[bar <a href="/url">foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.shortcut-reference-link-labels-are-case-insensitive]
> The link labels are case-insensitive:
>
> ```````````````````````````````` example
> [Foo]
>
> [foo]: /url "title"
> .
> <p><a href="/url" title="title">Foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.space-after-shortcut-reference-link-is-preserved]
> A space after the link text should be preserved:
>
> ```````````````````````````````` example
> [foo] bar
>
> [foo]: /url
> .
> <p><a href="/url">foo</a> bar</p>
> ````````````````````````````````

> gfm[inlines.links.escaping-opening-bracket-prevents-reference-link]
> If you just want bracketed text, you can backslash-escape the
> opening bracket to avoid links:
>
> ```````````````````````````````` example
> \[foo]
>
> [foo]: /url "title"
> .
> <p>[foo]</p>
> ````````````````````````````````

> gfm[inlines.links.link-label-ends-at-first-unescaped-closing-bracket]
> Note that this is a link, because a link label ends with the first
> following closing bracket:
>
> ```````````````````````````````` example
> [foo*]: /url
>
> *[foo*]
> .
> <p>*<a href="/url">foo*</a></p>
> ````````````````````````````````

> gfm[inlines.links.full-and-collapsed-references-take-precedence-over-shortcuts]
> Full and compact references take precedence over shortcut
> references:
>
> ```````````````````````````````` example
> [foo][bar]
>
> [foo]: /url1
> [bar]: /url2
> .
> <p><a href="/url2">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo][]
>
> [foo]: /url1
> .
> <p><a href="/url1">foo</a></p>
> ````````````````````````````````

> gfm[inlines.links.inline-links-take-precedence-over-reference-links]
> Inline links also take precedence:
>
> ```````````````````````````````` example
> [foo]()
>
> [foo]: /url1
> .
> <p><a href="">foo</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> [foo](not a link)
>
> [foo]: /url1
> .
> <p><a href="/url1">foo</a>(not a link)</p>
> ````````````````````````````````

> gfm[inlines.links.reference-link-may-parse-later-brackets-as-separate-text]
> In the following case `[bar][baz]` is parsed as a reference,
> `[foo]` as normal text:
>
> ```````````````````````````````` example
> [foo][bar][baz]
>
> [baz]: /url
> .
> <p>[foo]<a href="/url">bar</a></p>
> ````````````````````````````````

> gfm[inlines.links.defined-middle-label-makes-earlier-reference-parse]
> Here, though, `[foo][bar]` is parsed as a reference, since
> `[bar]` is defined:
>
> ```````````````````````````````` example
> [foo][bar][baz]
>
> [baz]: /url1
> [bar]: /url2
> .
> <p><a href="/url2">foo</a><a href="/url1">baz</a></p>
> ````````````````````````````````

> gfm[inlines.links.shortcut-reference-does-not-parse-when-followed-by-link-label]
> Here `[foo]` is not parsed as a shortcut reference, because it
> is followed by a link label (even though `[bar]` is not defined):
>
> ```````````````````````````````` example
> [foo][bar][baz]
>
> [baz]: /url1
> [foo]: /url2
> .
> <p>[foo]<a href="/url1">bar</a></p>
> ````````````````````````````````

## Images

> gfm[inlines.images.definition]
> Syntax for images is like the syntax for links, with one
> difference. Instead of [link text], we have an
> [image description](@).

> gfm[inlines.images.image-description-follows-link-text-rules-with-two-differences]
> The rules for [image description] are the same as for [link text],
> except that (a) an image description starts with `![` rather than
> `[` and (b) an image description may contain links.

> gfm[inlines.images.image-description-contains-inline-elements]
> An image description has inline elements as its contents.

> gfm[inlines.images.html-rendering-typically-uses-plain-description-as-alt]
> When an image is rendered to HTML, the image description is
> standardly used as the image's `alt` attribute.
>
> ```````````````````````````````` example
> ![foo](/url "title")
> .
> <p><img src="/url" alt="foo" title="title" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo *bar*]
>
> [foo *bar*]: train.jpg "train & tracks"
> .
> <p><img src="train.jpg" alt="foo bar" title="train &amp; tracks" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo ![bar](/url)](/url2)
> .
> <p><img src="/url2" alt="foo bar" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo [bar](/url)](/url2)
> .
> <p><img src="/url2" alt="foo bar" /></p>
> ````````````````````````````````

> gfm[inlines.images.html-rendering-should-use-plain-string-content-for-alt]
> Though this spec is concerned with parsing, not rendering, it is
> recommended that in rendering to HTML, only the plain string content
> of the [image description] be used.  Note that in
> the above example, the alt attribute's value is `foo bar`, not `foo
> [bar](/url)` or `foo <a href="/url">bar</a>`.  Only the plain string
> content is rendered, without formatting.
>
> ```````````````````````````````` example
> ![foo *bar*][]
>
> [foo *bar*]: train.jpg "train & tracks"
> .
> <p><img src="train.jpg" alt="foo bar" title="train &amp; tracks" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo *bar*][foobar]
>
> [FOOBAR]: train.jpg "train & tracks"
> .
> <p><img src="train.jpg" alt="foo bar" title="train &amp; tracks" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo](train.jpg)
> .
> <p><img src="train.jpg" alt="foo" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> My ![foo bar](/path/to/train.jpg  "title"   )
> .
> <p>My <img src="/path/to/train.jpg" alt="foo bar" title="title" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo](<url>)
> .
> <p><img src="url" alt="foo" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![](/url)
> .
> <p><img src="/url" alt="" /></p>
> ````````````````````````````````
>
> Reference-style:
>
> ```````````````````````````````` example
> ![foo][bar]
>
> [bar]: /url
> .
> <p><img src="/url" alt="foo" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![foo][bar]
>
> [BAR]: /url
> .
> <p><img src="/url" alt="foo" /></p>
> ````````````````````````````````
>
> Collapsed:
>
> ```````````````````````````````` example
> ![foo][]
>
> [foo]: /url "title"
> .
> <p><img src="/url" alt="foo" title="title" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![*foo* bar][]
>
> [*foo* bar]: /url "title"
> .
> <p><img src="/url" alt="foo bar" title="title" /></p>
> ````````````````````````````````

> gfm[inlines.images.reference-image-labels-are-case-insensitive]
> The labels are case-insensitive:
>
> ```````````````````````````````` example
> ![Foo][]
>
> [foo]: /url "title"
> .
> <p><img src="/url" alt="Foo" title="title" /></p>
> ````````````````````````````````

> gfm[inlines.images.no-whitespace-allowed-between-reference-image-brackets]
> As with reference links, [whitespace] is not allowed
> between the two sets of brackets:
>
> ```````````````````````````````` example
> ![foo] 
> []
>
> [foo]: /url "title"
> .
> <p><img src="/url" alt="foo" title="title" />
> []</p>
> ````````````````````````````````
>
> Shortcut:
>
> ```````````````````````````````` example
> ![foo]
>
> [foo]: /url "title"
> .
> <p><img src="/url" alt="foo" title="title" /></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ![*foo* bar]
>
> [*foo* bar]: /url "title"
> .
> <p><img src="/url" alt="foo bar" title="title" /></p>
> ````````````````````````````````

> gfm[inlines.images.reference-image-labels-cannot-contain-unescaped-brackets]
> Note that link labels cannot contain unescaped brackets:
>
> ```````````````````````````````` example
> ![[foo]]
>
> [[foo]]: /url "title"
> .
> <p>![[foo]]</p>
> <p>[[foo]]: /url &quot;title&quot;</p>
> ````````````````````````````````

> gfm[inlines.images.shortcut-image-labels-are-case-insensitive]
> The link labels are case-insensitive:
>
> ```````````````````````````````` example
> ![Foo]
>
> [foo]: /url "title"
> .
> <p><img src="/url" alt="Foo" title="title" /></p>
> ````````````````````````````````

> gfm[inlines.images.escaping-opening-bracket-after-bang-produces-literal-bracketed-text]
> If you just want a literal `!` followed by bracketed text, you can
> backslash-escape the opening `[`:
>
> ```````````````````````````````` example
> !\[foo]
>
> [foo]: /url "title"
> .
> <p>![foo]</p>
> ````````````````````````````````

> gfm[inlines.images.escaping-bang-produces-literal-bang-followed-by-link]
> If you want a link after a literal `!`, backslash-escape the
> `!`:
>
> ```````````````````````````````` example
> \![foo]
>
> [foo]: /url "title"
> .
> <p>!<a href="/url" title="title">foo</a></p>
> ````````````````````````````````

## Autolinks

> gfm[inlines.autolinks.definition]
> [Autolink](@)s are absolute URIs and email addresses inside
> `<` and `>`. They are parsed as links, with the URL or email address
> as the link label.

> gfm[inlines.autolinks.uri-autolink-definition]
> A [URI autolink](@) consists of `<`, followed by an
> [absolute URI] followed by `>`.  It is parsed as
> a link to the URI, with the URI as the link's label.

> gfm[inlines.autolinks.absolute-uri-definition]
> An [absolute URI](@),
> for these purposes, consists of a [scheme] followed by a colon (`:`)
> followed by zero or more characters other than ASCII
> [whitespace] and control characters, `<`, and `>`.  If
> the URI includes these characters, they must be percent-encoded
> (e.g. `%20` for a space).

> gfm[inlines.autolinks.scheme-definition]
> For purposes of this spec, a [scheme](@) is any sequence
> of 2--32 characters beginning with an ASCII letter and followed
> by any combination of ASCII letters, digits, or the symbols plus
> ("+"), period ("."), or hyphen ("-").

> gfm[inlines.autolinks.valid-uri-autolink-example]
> Here are some valid autolinks:
>
> ```````````````````````````````` example
> <http://foo.bar.baz>
> .
> <p><a href="http://foo.bar.baz">http://foo.bar.baz</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <http://foo.bar.baz/test?q=hello&id=22&boolean>
> .
> <p><a href="http://foo.bar.baz/test?q=hello&amp;id=22&amp;boolean">http://foo.bar.baz/test?q=hello&amp;id=22&amp;boolean</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <irc://foo.bar:2233/baz>
> .
> <p><a href="irc://foo.bar:2233/baz">irc://foo.bar:2233/baz</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks.autolink-schemes-are-case-insensitive]
> Uppercase is also fine:
>
> ```````````````````````````````` example
> <MAILTO:FOO@BAR.BAZ>
> .
> <p><a href="MAILTO:FOO@BAR.BAZ">MAILTO:FOO@BAR.BAZ</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks.spec-accepts-some-absolute-uris-that-are-not-valid-uris]
> Note that many strings that count as [absolute URIs] for
> purposes of this spec are not valid URIs, because their
> schemes are not registered or because of other problems
> with their syntax:
>
> ```````````````````````````````` example
> <a+b+c:d>
> .
> <p><a href="a+b+c:d">a+b+c:d</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <made-up-scheme://foo,bar>
> .
> <p><a href="made-up-scheme://foo,bar">made-up-scheme://foo,bar</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <http://../>
> .
> <p><a href="http://../">http://../</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <localhost:5001/foo>
> .
> <p><a href="localhost:5001/foo">localhost:5001/foo</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks.autolinks-cannot-contain-spaces]
> Spaces are not allowed in autolinks:
>
> ```````````````````````````````` example
> <http://foo.bar/baz bim>
> .
> <p>&lt;http://foo.bar/baz bim&gt;</p>
> ````````````````````````````````

> gfm[inlines.autolinks.backslash-escapes-do-not-work-inside-autolinks]
> Backslash-escapes do not work inside autolinks:
>
> ```````````````````````````````` example
> <http://example.com/\[\>
> .
> <p><a href="http://example.com/%5C%5B%5C">http://example.com/\[\</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks.email-autolink-definition]
> An [email autolink](@)
> consists of `<`, followed by an [email address],
> followed by `>`.  The link's label is the email address,
> and the URL is `mailto:` followed by the email address.

> gfm[inlines.autolinks.email-address-definition]
> An [email address](@),
> for these purposes, is anything that matches
> the [non-normative regex from the HTML5
> spec](https://html.spec.whatwg.org/multipage/forms.html#e-mail-state-(type=email)):

> gfm[inlines.autolinks.email-address-regex]
>     /^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?
>     (?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/

> gfm[inlines.autolinks.valid-email-autolink-example]
> Examples of email autolinks:
>
> ```````````````````````````````` example
> <foo@bar.example.com>
> .
> <p><a href="mailto:foo@bar.example.com">foo@bar.example.com</a></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <foo+special@Bar.baz-bar0.com>
> .
> <p><a href="mailto:foo+special@Bar.baz-bar0.com">foo+special@Bar.baz-bar0.com</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks.backslash-escapes-do-not-work-inside-email-autolinks]
> Backslash-escapes do not work inside email autolinks:
>
> ```````````````````````````````` example
> <foo\+@bar.example.com>
> .
> <p>&lt;foo+@bar.example.com&gt;</p>
> ````````````````````````````````

> gfm[inlines.autolinks.invalid-autolink-example]
> These are not autolinks:
>
> ```````````````````````````````` example
> <>
> .
> <p>&lt;&gt;</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> < http://foo.bar >
> .
> <p>&lt; http://foo.bar &gt;</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <m:abc>
> .
> <p>&lt;m:abc&gt;</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <foo.bar.baz>
> .
> <p>&lt;foo.bar.baz&gt;</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> http://example.com
> .
> <p>http://example.com</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo@bar.example.com
> .
> <p>foo@bar.example.com</p>
> ````````````````````````````````

> gfm[inlines.autolinks.extension-wrapper]
> <div class="extension">

## Autolinks (extension)

> gfm[inlines.autolinks-extension.gfm-enables-extended-autolinks]
> GFM enables the `autolink` extension, where autolinks will be recognised in a
> greater number of conditions.

> gfm[inlines.autolinks-extension.definition]
> [Autolink]s can also be constructed without requiring the use of `<` and to `>`
> to delimit them, although they will be recognized under a smaller set of
> circumstances.  All such recognized autolinks can only come at the beginning of
> a line, after whitespace, or any of the delimiting characters `*`, `_`, `~`,
> and `(`.

> gfm[inlines.autolinks-extension.extended-www-autolink-definition]
> An [extended www autolink](@) will be recognized
> when the text `www.` is found followed by a [valid domain].
> A [valid domain](@) consists of segments
> of alphanumeric characters, underscores (`_`) and hyphens (`-`)
> separated by periods (`.`).
> There must be at least one period,
> and no underscores may be present in the last two segments of the domain.

> gfm[inlines.autolinks-extension.extended-www-autolinks-insert-http-scheme]
> The scheme `http` will be inserted automatically:
>
> ```````````````````````````````` example autolink
> www.commonmark.org
> .
> <p><a href="http://www.commonmark.org">www.commonmark.org</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.extended-www-autolinks-may-have-path-after-domain]
> After a [valid domain], zero or more non-space non-`<` characters may follow:
>
> ```````````````````````````````` example autolink
> Visit www.commonmark.org/help for more information.
> .
> <p>Visit <a href="http://www.commonmark.org/help">www.commonmark.org/help</a> for more information.</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.extended-autolink-path-validation]
> We then apply [extended autolink path validation](@) as follows:

> gfm[inlines.autolinks-extension.trailing-punctuation-is-excluded-from-autolinks]
> Trailing punctuation (specifically, `?`, `!`, `.`, `,`, `:`, `*`, `_`, and `~`)
> will not be considered part of the autolink, though they may be included in the
> interior of the link:
>
> ```````````````````````````````` example autolink
> Visit www.commonmark.org.
>
> Visit www.commonmark.org/a.b.
> .
> <p>Visit <a href="http://www.commonmark.org">www.commonmark.org</a>.</p>
> <p>Visit <a href="http://www.commonmark.org/a.b">www.commonmark.org/a.b</a>.</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.unmatched-trailing-right-parentheses-are-excluded]
> When an autolink ends in `)`, we scan the entire autolink for the total number
> of parentheses.  If there is a greater number of closing parentheses than
> opening ones, we don't consider the unmatched trailing parentheses part of the
> autolink, in order to facilitate including an autolink inside a parenthesis:
>
> ```````````````````````````````` example autolink
> www.google.com/search?q=Markup+(business)
>
> www.google.com/search?q=Markup+(business)))
>
> (www.google.com/search?q=Markup+(business))
>
> (www.google.com/search?q=Markup+(business)
> .
> <p><a href="http://www.google.com/search?q=Markup+(business)">www.google.com/search?q=Markup+(business)</a></p>
> <p><a href="http://www.google.com/search?q=Markup+(business)">www.google.com/search?q=Markup+(business)</a>))</p>
> <p>(<a href="http://www.google.com/search?q=Markup+(business)">www.google.com/search?q=Markup+(business)</a>)</p>
> <p>(<a href="http://www.google.com/search?q=Markup+(business)">www.google.com/search?q=Markup+(business)</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.trailing-parenthesis-check-applies-only-at-link-end]
> This check is only done when the link ends in a closing parentheses `)`, so if
> the only parentheses are in the interior of the autolink, no special rules are
> applied:
>
> ```````````````````````````````` example autolink
> www.google.com/search?q=(business))+ok
> .
> <p><a href="http://www.google.com/search?q=(business))+ok">www.google.com/search?q=(business))+ok</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.trailing-semicolon-is-excluded-when-it-looks-like-entity]
> If an autolink ends in a semicolon (`;`), we check to see if it appears to
> resemble an [entity reference][entity references]; if the preceding text is `&`
> followed by one or more alphanumeric characters.  If so, it is excluded from
> the autolink:
>
> ```````````````````````````````` example autolink
> www.google.com/search?q=commonmark&hl=en
>
> www.google.com/search?q=commonmark&hl;
> .
> <p><a href="http://www.google.com/search?q=commonmark&amp;hl=en">www.google.com/search?q=commonmark&amp;hl=en</a></p>
> <p><a href="http://www.google.com/search?q=commonmark">www.google.com/search?q=commonmark</a>&amp;hl;</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.left-angle-bracket-ends-autolink]
> `<` immediately ends an autolink.
>
> ```````````````````````````````` example autolink
> www.commonmark.org/he<lp
> .
> <p><a href="http://www.commonmark.org/he">www.commonmark.org/he</a>&lt;lp</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.extended-url-autolink-definition]
> An [extended url autolink](@) will be recognised when one of the schemes
> `http://`, `https://`, or `ftp://`, followed by a [valid domain], then zero or
> more non-space non-`<` characters according to
> [extended autolink path validation]:
>
> ```````````````````````````````` example autolink
> http://commonmark.org
>
> (Visit https://encrypted.google.com/search?q=Markup+(business))
>
> Anonymous FTP is available at ftp://foo.bar.baz.
> .
> <p><a href="http://commonmark.org">http://commonmark.org</a></p>
> <p>(Visit <a href="https://encrypted.google.com/search?q=Markup+(business)">https://encrypted.google.com/search?q=Markup+(business)</a>)</p>
> <p>Anonymous FTP is available at <a href="ftp://foo.bar.baz">ftp://foo.bar.baz</a>.</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.extended-email-autolink-definition]
> An [extended email autolink](@) will be recognised when an email address is
> recognised within any text node.  Email addresses are recognised according to
> the following rules:

> gfm[inlines.autolinks-extension.extended-email-autolink-rules]
> * One ore more characters which are alphanumeric, or `.`, `-`, `_`, or `+`.
> * An `@` symbol.
> * One or more characters which are alphanumeric, or `-` or `_`,
>   separated by periods (`.`).
>   There must be at least one period.
>   The last character must not be one of `-` or `_`.

> gfm[inlines.autolinks-extension.extended-email-autolinks-insert-mailto-scheme]
> The scheme `mailto:` will automatically be added to the generated link:
>
> ```````````````````````````````` example autolink
> foo@bar.baz
> .
> <p><a href="mailto:foo@bar.baz">foo@bar.baz</a></p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.plus-is-allowed-before-but-not-after-at-sign]
> `+` can occur before the `@`, but not after.
>
> ```````````````````````````````` example autolink
> hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.
> .
> <p>hello@mail+xyz.example isn't valid, but <a href="mailto:hello+xyz@mail.example">hello+xyz@mail.example</a> is.</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.dot-dash-and-underscore-rules-around-at-sign]
> `.`, `-`, and `_` can occur on both sides of the `@`, but only `.` may occur at
> the end of the email address, in which case it will not be considered part of
> the address:
>
> ```````````````````````````````` example autolink
> a.b-c_d@a.b
>
> a.b-c_d@a.b.
>
> a.b-c_d@a.b-
>
> a.b-c_d@a.b_
> .
> <p><a href="mailto:a.b-c_d@a.b">a.b-c_d@a.b</a></p>
> <p><a href="mailto:a.b-c_d@a.b">a.b-c_d@a.b</a>.</p>
> <p>a.b-c_d@a.b-</p>
> <p>a.b-c_d@a.b_</p>
> ````````````````````````````````

> gfm[inlines.autolinks-extension.extension-wrapper]
> </div>

## Raw HTML

> gfm[inlines.raw-html.html-like-tag-text-is-parsed-as-raw-html]
> Text between `<` and `>` that looks like an HTML tag is parsed as a
> raw HTML tag and will be rendered in HTML without escaping.

> gfm[inlines.raw-html.custom-tag-and-attribute-names-are-allowed]
> Tag and attribute names are not limited to current HTML tags,
> so custom tags (and even, say, DocBook tags) may be used.

> gfm[inlines.raw-html.tag-grammar-follows]
> Here is the grammar for tags:

> gfm[inlines.raw-html.tag-name-definition]
> A [tag name](@) consists of an ASCII letter
> followed by zero or more ASCII letters, digits, or
> hyphens (`-`).

> gfm[inlines.raw-html.attribute-definition]
> An [attribute](@) consists of [whitespace],
> an [attribute name], and an optional
> [attribute value specification].

> gfm[inlines.raw-html.attribute-name-definition]
> An [attribute name](@)
> consists of an ASCII letter, `_`, or `:`, followed by zero or more ASCII
> letters, digits, `_`, `.`, `:`, or `-`.  (Note:  This is the XML
> specification restricted to ASCII.  HTML5 is laxer.)

> gfm[inlines.raw-html.attribute-value-specification-definition]
> An [attribute value specification](@)
> consists of optional [whitespace],
> a `=` character, optional [whitespace], and an [attribute
> value].

> gfm[inlines.raw-html.attribute-value-definition]
> An [attribute value](@)
> consists of an [unquoted attribute value],
> a [single-quoted attribute value], or a [double-quoted attribute value].

> gfm[inlines.raw-html.unquoted-attribute-value-definition]
> An [unquoted attribute value](@)
> is a nonempty string of characters not
> including [whitespace], `"`, `'`, `=`, `<`, `>`, or `` ` ``.

> gfm[inlines.raw-html.single-quoted-attribute-value-definition]
> A [single-quoted attribute value](@)
> consists of `'`, zero or more
> characters not including `'`, and a final `'`.

> gfm[inlines.raw-html.double-quoted-attribute-value-definition]
> A [double-quoted attribute value](@)
> consists of `"`, zero or more
> characters not including `"`, and a final `"`.

> gfm[inlines.raw-html.open-tag-definition]
> An [open tag](@) consists of a `<` character, a [tag name],
> zero or more [attributes], optional [whitespace], an optional `/`
> character, and a `>` character.

> gfm[inlines.raw-html.closing-tag-definition]
> A [closing tag](@) consists of the string `</`, a
> [tag name], optional [whitespace], and the character `>`.

> gfm[inlines.raw-html.html-comment-definition]
> An [HTML comment](@) consists of `<!-->`, `<!--->`, or  `<!--`, a string of
> characters not including the string `-->`, and `-->` (see the
> [HTML spec](https://html.spec.whatwg.org/multipage/parsing.html#markup-declaration-open-state)).

> gfm[inlines.raw-html.processing-instruction-definition]
> A [processing instruction](@)
> consists of the string `<?`, a string
> of characters not including the string `?>`, and the string
> `?>`.

> gfm[inlines.raw-html.declaration-definition]
> A [declaration](@) consists of the
> string `<!`, a name consisting of one or more uppercase ASCII letters,
> [whitespace], a string of characters not including the
> character `>`, and the character `>`.

> gfm[inlines.raw-html.cdata-section-definition]
> A [CDATA section](@) consists of
> the string `<![CDATA[`, a string of characters not including the string
> `]]>`, and the string `]]>`.

> gfm[inlines.raw-html.html-tag-definition]
> An [HTML tag](@) consists of an [open tag], a [closing tag],
> an [HTML comment], a [processing instruction], a [declaration],
> or a [CDATA section].

> gfm[inlines.raw-html.simple-open-tag-examples]
> Here are some simple open tags:
>
> ```````````````````````````````` example
> <a><bab><c2c>
> .
> <p><a><bab><c2c></p>
> ````````````````````````````````
>
> Empty elements:
>
> ```````````````````````````````` example
> <a/><b2/>
> .
> <p><a/><b2/></p>
> ````````````````````````````````
>
> [Whitespace] is allowed:
>
> ```````````````````````````````` example
> <a  /><b2
> data="foo" >
> .
> <p><a  /><b2
> data="foo" ></p>
> ````````````````````````````````
>
> With attributes:
>
> ```````````````````````````````` example
> <a foo="bar" bam = 'baz <em>"</em>'
> _boolean zoop:33=zoop:33 />
> .
> <p><a foo="bar" bam = 'baz <em>"</em>'
> _boolean zoop:33=zoop:33 /></p>
> ````````````````````````````````

> gfm[inlines.raw-html.custom-tag-names-are-allowed]
> Custom tag names can be used:
>
> ```````````````````````````````` example
> Foo <responsive-image src="foo.jpg" />
> .
> <p>Foo <responsive-image src="foo.jpg" /></p>
> ````````````````````````````````

> gfm[inlines.raw-html.illegal-tag-and-attribute-syntax-is-not-parsed-as-html]
> Illegal tag names, not parsed as HTML:
>
> ```````````````````````````````` example
> <33> <__>
> .
> <p>&lt;33&gt; &lt;__&gt;</p>
> ````````````````````````````````
>
> Illegal attribute names:
>
> ```````````````````````````````` example
> <a h*#ref="hi">
> .
> <p>&lt;a h*#ref=&quot;hi&quot;&gt;</p>
> ````````````````````````````````
>
> Illegal attribute values:
>
> ```````````````````````````````` example
> <a href="hi'> <a href=hi'>
> .
> <p>&lt;a href=&quot;hi'&gt; &lt;a href=hi'&gt;</p>
> ````````````````````````````````
>
> Illegal [whitespace]:
>
> ```````````````````````````````` example
> < a><
> foo><bar/ >
> <foo bar=baz
> bim!bop />
> .
> <p>&lt; a&gt;&lt;
> foo&gt;&lt;bar/ &gt;
> &lt;foo bar=baz
> bim!bop /&gt;</p>
> ````````````````````````````````
>
> Missing [whitespace]:
>
> ```````````````````````````````` example
> <a href='bar'title=title>
> .
> <p>&lt;a href='bar'title=title&gt;</p>
> ````````````````````````````````
>
> Closing tags:
>
> ```````````````````````````````` example
> </a></foo >
> .
> <p></a></foo ></p>
> ````````````````````````````````

> gfm[inlines.raw-html.closing-tags-cannot-have-attributes]
> Illegal attributes in closing tag:
>
> ```````````````````````````````` example
> </a href="foo">
> .
> <p>&lt;/a href=&quot;foo&quot;&gt;</p>
> ````````````````````````````````
>
> Comments:
>
> ```````````````````````````````` example
> foo <!-- this is a --
> comment - with hyphens -->
> .
> <p>foo <!-- this is a --
> comment - with hyphens --></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo <!--> foo -->
>
> foo <!---> foo -->
> .
> <p>foo <!--> foo --&gt;</p>
> <p>foo <!---> foo --&gt;</p>
> ````````````````````````````````
>
> Processing instructions:
>
> ```````````````````````````````` example
> foo <?php echo $a; ?>
> .
> <p>foo <?php echo $a; ?></p>
> ````````````````````````````````
>
> Declarations:
>
> ```````````````````````````````` example
> foo <!ELEMENT br EMPTY>
> .
> <p>foo <!ELEMENT br EMPTY></p>
> ````````````````````````````````
>
> CDATA sections:
>
> ```````````````````````````````` example
> foo <![CDATA[>&<]]>
> .
> <p>foo <![CDATA[>&<]]></p>
> ````````````````````````````````

> gfm[inlines.raw-html.entity-and-numeric-references-are-preserved-in-html-attributes]
> Entity and numeric character references are preserved in HTML
> attributes:
>
> ```````````````````````````````` example
> foo <a href="&ouml;">
> .
> <p>foo <a href="&ouml;"></p>
> ````````````````````````````````

> gfm[inlines.raw-html.backslash-escapes-do-not-work-in-html-attributes]
> Backslash escapes do not work in HTML attributes:
>
> ```````````````````````````````` example
> foo <a href="\*">
> .
> <p>foo <a href="\*"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <a href="\"">
> .
> <p>&lt;a href=&quot;&quot;&quot;&gt;</p>
> ````````````````````````````````

> gfm[inlines.raw-html.extension-wrapper]
> <div class="extension">

## Disallowed Raw HTML (extension)

> gfm[inlines.disallowed-raw-html-extension.gfm-enables-tagfilter-extension]
> GFM enables the `tagfilter` extension, where the following HTML tags will be
> filtered when rendering HTML output:

> gfm[inlines.disallowed-raw-html-extension.disallowed-tags]
> * `<title>`
> * `<textarea>`
> * `<style>`
> * `<xmp>`
> * `<iframe>`
> * `<noembed>`
> * `<noframes>`
> * `<script>`
> * `<plaintext>`

> gfm[inlines.disallowed-raw-html-extension.filtering-replaces-leading-angle-bracket]
> Filtering is done by replacing the leading `<` with the entity `&lt;`.  These
> tags are chosen in particular as they change how HTML is interpreted in a way
> unique to them (i.e. nested HTML is interpreted differently), and this is
> usually undesireable in the context of other rendered Markdown content.

> gfm[inlines.disallowed-raw-html-extension.disallowed-tags-change-html-parsing]
> These tags are chosen in particular as they change how HTML is interpreted in a way
> unique to them (i.e. nested HTML is interpreted differently), and this is
> usually undesireable in the context of other rendered Markdown content.

> gfm[inlines.disallowed-raw-html-extension.all-other-html-tags-are-left-untouched]
> All other HTML tags are left untouched.
>
> ```````````````````````````````` example tagfilter
> <strong> <title> <style> <em>
>
> <blockquote>
>   <xmp> is disallowed.  <XMP> is also disallowed.
> </blockquote>
> .
> <p><strong> &lt;title> &lt;style> <em></p>
> <blockquote>
>   &lt;xmp> is disallowed.  &lt;XMP> is also disallowed.
> </blockquote>
> ````````````````````````````````

> gfm[inlines.disallowed-raw-html-extension.extension-wrapper]
> </div>

## Hard line breaks

> gfm[inlines.hard-line-breaks.two-trailing-spaces-produce-hard-line-break]
> A line break (not in a code span or HTML tag) that is preceded
> by two or more spaces and does not occur at the end of a block
> is parsed as a [hard line break](@) (rendered
> in HTML as a `<br />` tag):
>
> ```````````````````````````````` example
> foo  
> baz
> .
> <p>foo<br />
> baz</p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.backslash-before-line-ending-also-produces-hard-line-break]
> For a more visible alternative, a backslash before the
> [line ending] may be used instead of two spaces:
>
> ```````````````````````````````` example
> foo\
> baz
> .
> <p>foo<br />
> baz</p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.more-than-two-trailing-spaces-also-work]
> More than two spaces can be used:
>
> ```````````````````````````````` example
> foo       
> baz
> .
> <p>foo<br />
> baz</p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.leading-spaces-on-next-line-are-ignored]
> Leading spaces at the beginning of the next line are ignored:
>
> ```````````````````````````````` example
> foo  
>      bar
> .
> <p>foo<br />
> bar</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo\
>      bar
> .
> <p>foo<br />
> bar</p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.hard-line-breaks-can-occur-inside-inline-constructs]
> Line breaks can occur inside emphasis, links, and other constructs
> that allow inline content:
>
> ```````````````````````````````` example
> *foo  
> bar*
> .
> <p><em>foo<br />
> bar</em></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> *foo\
> bar*
> .
> <p><em>foo<br />
> bar</em></p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.hard-line-breaks-do-not-occur-inside-code-spans-or-html-tags]
> Line breaks do not occur inside code spans
>
> ```````````````````````````````` example
> `code  
> span`
> .
> <p><code>code   span</code></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> `code\
> span`
> .
> <p><code>code\ span</code></p>
> ````````````````````````````````
>
> or HTML tags:
>
> ```````````````````````````````` example
> <a href="foo  
> bar">
> .
> <p><a href="foo  
> bar"></p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> <a href="foo\
> bar">
> .
> <p><a href="foo\
> bar"></p>
> ````````````````````````````````

> gfm[inlines.hard-line-breaks.hard-line-break-syntax-does-not-work-at-end-of-block]
> Hard line breaks are for separating inline content within a block.
> Neither syntax for hard line breaks works at the end of a paragraph or
> other block element:
>
> ```````````````````````````````` example
> foo\
> .
> <p>foo\</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> foo  
> .
> <p>foo</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ### foo\
> .
> <h3>foo\</h3>
> ````````````````````````````````
>
> ```````````````````````````````` example
> ### foo  
> .
> <h3>foo</h3>
> ````````````````````````````````

## Soft line breaks

> gfm[inlines.soft-line-breaks.regular-line-break-produces-softbreak]
> A regular line break (not in a code span or HTML tag) that is not
> preceded by two or more spaces or a backslash is parsed as a
> [softbreak](@).  (A softbreak may be rendered in HTML either as a
> [line ending] or as a space. The result will be the same in
> browsers. In the examples here, a [line ending] will be used.)
>
> ```````````````````````````````` example
> foo
> baz
> .
> <p>foo
> baz</p>
> ````````````````````````````````

> gfm[inlines.soft-line-breaks.edge-spaces-around-softbreak-are-removed]
> Spaces at the end of the line and beginning of the next line are
> removed:
>
> ```````````````````````````````` example
> foo 
>  baz
> .
> <p>foo
> baz</p>
> ````````````````````````````````

> gfm[inlines.soft-line-breaks.softbreak-may-render-as-line-ending-or-space]
> A conforming parser may render a soft line break in HTML either as a
> line break or as a space.

> gfm[inlines.soft-line-breaks.renderers-may-optionally-promote-softbreaks]
> A renderer may also provide an option to render soft line breaks
> as hard line breaks.

## Textual content

> gfm[inlines.textual-content.uninterpreted-characters-are-parsed-as-text]
> Any characters not given an interpretation by the above rules will
> be parsed as plain textual content.
>
> ```````````````````````````````` example
> hello $.;'there
> .
> <p>hello $.;'there</p>
> ````````````````````````````````
>
> ```````````````````````````````` example
> Foo χρῆν
> .
> <p>Foo χρῆν</p>
> ````````````````````````````````

> gfm[inlines.textual-content.internal-spaces-are-preserved]
> Internal spaces are preserved verbatim:
>
> ```````````````````````````````` example
> Multiple     spaces
> .
> <p>Multiple     spaces</p>
> ````````````````````````````````

> gfm[inlines.textual-content.end-tests-marker]
> <!-- END TESTS -->

# Appendix: A parsing strategy

> gfm[appendix-a-parsing-strategy.reference-implementations-use-this-parsing-strategy]
> In this appendix we describe some features of the parsing strategy
> used in the CommonMark reference implementations.

## Overview

> gfm[appendix-a-parsing-strategy.overview.parsing-has-two-phases]
> Parsing has two phases:
>
> 1. In the first phase, lines of input are consumed and the block
> structure of the document---its division into paragraphs, block quotes,
> list items, and so on---is constructed.  Text is assigned to these
> blocks but not parsed. Link reference definitions are parsed and a
> map of links is constructed.
>
> 2. In the second phase, the raw text contents of paragraphs and headings
> are parsed into sequences of Markdown inline elements (strings,
> code spans, links, emphasis, and so on), using the map of link
> references constructed in phase 1.

> gfm[appendix-a-parsing-strategy.overview.document-is-represented-as-tree-of-blocks]
> At each point in processing, the document is represented as a tree of
> **blocks**.  The root of the tree is a `document` block.  The `document`
> may have any number of other blocks as **children**.  These children
> may, in turn, have other blocks as children.  The last child of a block
> is normally considered **open**, meaning that subsequent lines of input
> can alter its contents.  (Blocks that are not open are **closed**.)
> Here, for example, is a possible document tree, with the open blocks
> marked by arrows:
>
> ``` tree
> -> document
>   -> block_quote
>        paragraph
>          "Lorem ipsum dolor\nsit amet."
>     -> list (type=bullet tight=true bullet_char=-)
>          list_item
>            paragraph
>              "Qui *quodsi iracundia*"
>       -> list_item
>         -> paragraph
>              "aliquando id"
> ```

## Phase 1: block structure

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.each-line-may-close-open-or-append]
> Each line that is processed has an effect on this tree.  The line is
> analyzed and, depending on its contents, the document may be altered
> in one or more of the following ways:
>
> 1. One or more open blocks may be closed.
> 2. One or more new blocks may be created as children of the
>    last open block.
> 3. Text may be added to the last (deepest) open block remaining
>    on the tree.

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.lines-can-be-discarded-after-incorporation]
> Once a line has been incorporated into the tree in this way,
> it can be discarded, so input can be read in a stream.

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.phase-1-line-processing-procedure]
> For each line, we follow this procedure:
>
> 1. First we iterate through the open blocks, starting with the
> root document, and descending through last children down to the last
> open block.  Each block imposes a condition that the line must satisfy
> if the block is to remain open.  For example, a block quote requires a
> `>` character.  A paragraph requires a non-blank line.
> In this phase we may match all or just some of the open
> blocks.  But we cannot close unmatched blocks yet, because we may have a
> [lazy continuation line].
>
> 2.  Next, after consuming the continuation markers for existing
> blocks, we look for new block starts (e.g. `>` for a block quote).
> If we encounter a new block start, we close any blocks unmatched
> in step 1 before creating the new block as a child of the last
> matched block.
>
> 3.  Finally, we look at the remainder of the line (after block
> markers like `>`, list markers, and indentation have been consumed).
> This is text that can be incorporated into the last open
> block (a paragraph, code block, heading, or raw HTML).

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.setext-underlines-convert-paragraphs]
> Setext headings are formed when we see a line of a paragraph
> that is a [setext heading underline].

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.reference-definitions-are-detected-at-paragraph-close]
> Reference link definitions are detected when a paragraph is closed;
> the accumulated text lines are parsed to see if they begin with
> one or more reference link definitions.  Any remainder becomes a
> normal paragraph.

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.four-line-example-shows-tree-evolution]
> We can see how this works by considering how the tree above is
> generated by four lines of Markdown:
>
> ``` markdown
> > Lorem ipsum dolor
> sit amet.
> > - Qui *quodsi iracundia*
> > - aliquando id
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.initial-tree-is-just-document]
> At the outset, our document model is just
>
> ``` tree
> -> document
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.first-example-line]
> The first line of our text,
>
> ``` markdown
> > Lorem ipsum dolor
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.first-line-opens-block-quote-and-paragraph]
> causes a `block_quote` block to be created as a child of our
> open `document` block, and a `paragraph` block as a child of
> the `block_quote`.  Then the text is added to the last open
> block, the `paragraph`:
>
> ``` tree
> -> document
>   -> block_quote
>     -> paragraph
>          "Lorem ipsum dolor"
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.second-example-line]
> The next line,
>
> ``` markdown
> sit amet.
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.second-line-is-lazy-continuation]
> is a "lazy continuation" of the open `paragraph`, so it gets added
> to the paragraph's text:
>
> ``` tree
> -> document
>   -> block_quote
>     -> paragraph
>          "Lorem ipsum dolor\nsit amet."
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.third-example-line]
> The third line,
>
> ``` markdown
> > - Qui *quodsi iracundia*
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.third-line-closes-paragraph-and-opens-list]
> causes the `paragraph` block to be closed, and a new `list` block
> opened as a child of the `block_quote`.  A `list_item` is also
> added as a child of the `list`, and a `paragraph` as a child of
> the `list_item`.  The text is then added to the new `paragraph`:
>
> ``` tree
> -> document
>   -> block_quote
>        paragraph
>          "Lorem ipsum dolor\nsit amet."
>     -> list (type=bullet tight=true bullet_char=-)
>       -> list_item
>         -> paragraph
>              "Qui *quodsi iracundia*"
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.fourth-example-line]
> The fourth line,
>
> ``` markdown
> > - aliquando id
> ```

> gfm[appendix-a-parsing-strategy.phase-1-block-structure.fourth-line-opens-second-list-item]
> causes the `list_item` (and its child the `paragraph`) to be closed,
> and a new `list_item` opened up as child of the `list`.  A `paragraph`
> is added as a child of the new `list_item`, to contain the text.
> We thus obtain the final tree:
>
> ``` tree
> -> document
>   -> block_quote
>        paragraph
>          "Lorem ipsum dolor\nsit amet."
>     -> list (type=bullet tight=true bullet_char=-)
>          list_item
>            paragraph
>              "Qui *quodsi iracundia*"
>       -> list_item
>         -> paragraph
>              "aliquando id"
> ```

## Phase 2: inline structure

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.open-blocks-close-at-end-of-input]
> Once all of the input has been parsed, all open blocks are closed.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.phase-2-parses-inline-content]
> We then "walk the tree," visiting every node, and parse raw
> string contents of paragraphs and headings as inlines.  At this
> point we have seen all the link reference definitions, so we can
> resolve reference links as we go.
>
> ``` tree
> document
>   block_quote
>     paragraph
>       str "Lorem ipsum dolor"
>       softbreak
>       str "sit amet."
>     list (type=bullet tight=true bullet_char=-)
>       list_item
>         paragraph
>           str "Qui "
>           emph
>             str "quodsi iracundia"
>       list_item
>         paragraph
>           str "aliquando id"
> ```

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.example-produces-softbreak-and-emphasis]
> Notice how the [line ending] in the first paragraph has
> been parsed as a `softbreak`, and the asterisks in the first list item
> have become an `emph`.

### An algorithm for parsing nested emphasis and links

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.trickiest-inline-case]
> By far the trickiest part of inline parsing is handling emphasis,
> strong emphasis, links, and images.  This is done using the following
> algorithm.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.push-delimiters-to-stack]
> When we're parsing inlines and we hit either
>
> - a run of `*` or `_` characters, or
> - a `[` or `![`

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.stack-entries-point-to-text-nodes]
> we insert a text node with these symbols as its literal content, and we
> add a pointer to this text node to the [delimiter stack](@).

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.delimiter-stack-definition]
> The [delimiter stack] is a doubly linked list.  Each
> element contains a pointer to a text node, plus information about
>
> - the type of delimiter (`[`, `![`, `*`, `_`)
> - the number of delimiters,
> - whether the delimiter is "active" (all are active to start), and
> - whether the delimiter is a potential opener, a potential closer,
>   or both (which depends on what sort of characters precede
>   and follow the delimiters).

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.closing-bracket-triggers-link-search]
> When we hit a `]` character, we call the *look for link or image*
> procedure (see below).

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.end-of-input-triggers-emphasis-processing]
> When we hit the end of the input, we call the *process emphasis*
> procedure (see below), with `stack_bottom` = NULL.

#### *look for link or image*

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-scan-backward]
> Starting at the top of the delimiter stack, we look backwards
> through the stack for an opening `[` or `![` delimiter.
>
> - If we don't find one, we return a literal text node `]`.
>
> - If we do find one, but it's not *active*, we remove the inactive
>   delimiter from the stack, and return a literal text node `]`.
>
> - If we find one and it's active, then we parse ahead to see if
>   we have an inline link/image, reference link/image, compact reference
>   link/image, or shortcut reference link/image.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-failure-case]
>   + If we don't, then we remove the opening delimiter from the
>     delimiter stack and return a literal text node `]`.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-success-case]
>   + If we do, then

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-builds-node-children]
>     * We return a link or image node whose children are the inlines
>       after the text node pointed to by the opening delimiter.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-runs-emphasis-processing]
>     * We run *process emphasis* on these inlines, with the `[` opener
>       as `stack_bottom`.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-removes-opener]
>     * We remove the opening delimiter.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.look-for-link-or-image-deactivates-earlier-openers]
>     * If we have a link (and not an image), we also set all
>       `[` delimiters before the opening delimiter to *inactive*.  (This
>       will prevent us from getting links within links.)

#### *process emphasis*

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-stack-bottom-limits-scan]
> Parameter `stack_bottom` sets a lower bound to how far we
> descend in the [delimiter stack].  If it is NULL, we can
> go all the way to the bottom.  Otherwise, we stop before
> visiting `stack_bottom`.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-initial-current-position]
> Let `current_position` point to the element on the [delimiter stack]
> just above `stack_bottom` (or the first element if `stack_bottom`
> is NULL).

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-tracks-openers-bottom]
> We keep track of the `openers_bottom` for each delimiter
> type (`*`, `_`) and each length of the closing delimiter run
> (modulo 3).  Initialize this to `stack_bottom`.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-loop]
> Then we repeat the following until we run out of potential
> closers:
>
> - Move `current_position` forward in the delimiter stack (if needed)
>   until we find the first potential closer with delimiter `*` or `_`.
>   (This will be the potential closer closest
>   to the beginning of the input -- the first one in parse order.)
>
> - Now, look back in the stack (staying above `stack_bottom` and
>   the `openers_bottom` for this delimiter type) for the
>   first matching potential opener ("matching" means same delimiter).
>
> - If one is found:

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-chooses-node-kind]
>   + Figure out whether we have emphasis or strong emphasis:
>     if both closer and opener spans have length >= 2, we have
>     strong, otherwise regular.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-inserts-node]
>   + Insert an emph or strong emph node accordingly, after
>     the text node corresponding to the opener.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-interior-delimiters]
>   + Remove any delimiters between the opener and closer from
>     the delimiter stack.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-consumes-delimiters]
>   + Remove 1 (for regular emph) or 2 (for strong emph) delimiters
>     from the opening and closing text nodes.  If they become empty
>     as a result, remove them and remove the corresponding element
>     of the delimiter stack.  If the closing node is removed, reset
>     `current_position` to the next element in the stack.
>
> - If none is found:

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-updates-openers-bottom]
>   + Set `openers_bottom` to the element before `current_position`.
>     (We know that there are no openers for this kind of closer up to and
>     including this point, so this puts a lower bound on future searches.)

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-removes-inactive-closer]
>   + If the closer at `current_position` is not a potential opener,
>     remove it from the delimiter stack (since we know it can't
>     be a closer either).

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-advances]
>   + Advance `current_position` to the next element in the stack.

> gfm[appendix-a-parsing-strategy.phase-2-inline-structure.an-algorithm-for-parsing-nested-emphasis-and-links.process-emphasis-cleans-up-stack]
> After we're done, we remove all delimiters above `stack_bottom` from the
> delimiter stack.
