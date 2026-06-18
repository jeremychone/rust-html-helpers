# html-helpers Public API

[html-helpers](https://crates.io/crates/html-helpers) provides high-level utilities for cleaning, transforming, and converting HTML content.

## Functions

### `html_helpers::slim`

```rust
pub fn slim(html_content: &str) -> Result<String>
```

Slims an HTML page by removing non-content elements (scripts, styles, comments, empty tags) and filtering attributes, preserving only essential head tags and body content.

- Removes `<script>`, `<link>`, `<style>`, `<svg>`, `<base>`, HTML comments, empty whitespace text nodes, and specific tags (e.g., `<div>`, `<span>`, `<p>`) that become effectively empty after processing children.
- Drops empty `<head>` elements. Keeps `<title>` and certain `<meta>` tags whose `property` attribute contains "title", "url", "image", or "description".
- Filters attributes: outside `<head>` keeps `class`, `aria-label`, `href`, `title`, `id`; inside `<head>` keeps only `property`/`content` on meta tags.

Returns the cleaned HTML as a `String`.

### `html_helpers::select`

```rust
pub fn select<S>(html_content: &str, selectors: S) -> Result<Vec<Elem>>
where
    S: IntoIterator,
    S::Item: AsRef<str>,
```

Selects HTML elements matching a list of CSS selectors (combined with OR). Returns a `Vec<Elem>` in document order.

- Selectors are joined by commas.
- Empty selector strings are silently ignored.
- Returns an empty vector when no valid selectors remain.
- Under the hood uses [`scraper`](https://crates.io/crates/scraper).

### `html_helpers::decode_html_entities`

```rust
pub fn decode_html_entities(content: &str) -> String
```

Decodes HTML entities (e.g., `&lt;` → `<`). Convenient when you need to unescape attribute values or text after `slim` or `select`.

## Types

### `Elem`

```rust
pub struct Elem {
    pub tag: String,
    pub attrs: Option<HashMap<String, String>>,
    pub text: Option<String>,
    pub inner_html: Option<String>,
}
```

Represents a simplified HTML element suitable for serialization.

- `tag`: tag name in lowercase.
- `attrs`: key-value attribute map, or `None` when no attributes are present.
- `text`: visible text content of the element (collected from all descendants), or `None` if effectively empty.
- `inner_html`: raw inner HTML of the element, or `None` if effectively empty.

### `Error`

```rust
pub enum Error {
    Custom(String),
    SelectorParse { selector: String, cause: String },
}
```

- `Custom`: generic error (e.g., internal processing).
- `SelectorParse`: invalid CSS selector syntax.

### `Result<T>`

Type alias: `pub type Result<T> = Result<T, Error>;`

## Example

```rust
use html_helpers::{slim, select, Elem};

let html = r#"
<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body class="page">
    <p>Hello, World!</p>
    <a href="https://example.com" class="link">Example</a>
</body>
</html>
"#;

// Slim away non-content elements and attributes
let cleaned = slim(html)?;

// Select elements matching CSS selectors from the cleaned HTML
let elements: Vec<Elem> = select(&cleaned, ["p", "a.link"])?;
assert_eq!(elements.len(), 2);
assert_eq!(elements[0].tag, "p");
assert_eq!(elements[0].text.as_deref(), Some("Hello, World!"));
assert_eq!(elements[1].tag, "a");
assert_eq!(
    elements[1].attrs.as_ref().unwrap().get("href").map(String::as_str),
    Some("https://example.com")
);
```
