# Gobol Highlight.js plugin

This directory contains a standalone Highlight.js language definition for
Gobol (`.gbl` files).

## Browser

Load Highlight.js and the plugin in that order:

```html
<link rel="stylesheet"
      href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/github.min.css">
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js"></script>
<script src="path/to/gobol.js"></script>
<script>hljs.highlightAll();</script>
```

Use `language-gobol` (or `language-gbl`) on code blocks:

```html
<pre><code class="language-gobol">func main() {
    io::println("Hello, Gobol!");
}</code></pre>
```

The plugin registers itself automatically when loaded after Highlight.js.
For bundlers, import the definition and register it explicitly:

```js
import hljs from "highlight.js/lib/core";
import gobol from "./gobol.js";

hljs.registerLanguage("gobol", gobol);
```
