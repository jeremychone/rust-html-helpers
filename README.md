# html-helpers

**A collection of high-level utilities for cleaning, transforming, and converting HTML content.**

> ⚠️ Very early release – currently supports only HTML slimming.

## Example

```rust
let content: String = /* full HTML page */;

let slim_content = html_helpers::slim(&content)?;
```
