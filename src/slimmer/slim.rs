use crate::error::{Error, Result};
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, node::Node};

use super::support::{
	BLOCK_LEVEL_TAGS, REMOVABLE_EMPTY_TAGS, TAGS_TO_REMOVE, VOID_ELEMENTS, filter_and_write_attributes,
	is_string_effectively_empty, remove_empty_lines, should_keep_meta,
};

/// Decodes HTML entities (e.g., `&lt;` becomes `<`).
/// Re-exporting from the original slimmer or using html-escape directly.
pub fn decode_html_entities(content: &str) -> String {
	html_escape::decode_html_entities(content).to_string()
}

/// Strips non-content elements from the provided HTML content using the `scraper` crate,
/// preserving essential head tags, and returns the cleaned HTML as a string.
///
/// This function aims to replicate the behavior of `slimmer::slim` using `scraper`.
/// It removes:
/// - Non-visible tags like `<script>`, `<link>`, `<style>`, `<svg>`, `<base>`.
/// - HTML comments.
/// - Empty or whitespace-only text nodes.
/// - Specific tags (like `<div>`, `<span>`, `<p>`, etc.) if they become effectively empty *after* processing children.
/// - Attributes except for specific allowlists (`class`, `aria-label`, `href` outside head; `property`, `content` for relevant meta tags in head).
///
/// It preserves:
/// - `<title>` tag within `<head>`.
/// - `<meta>` tags within `<head>` if their `property` attribute matches keywords in `META_PROPERTY_KEYWORDS`.
/// - Essential body content.
///
/// # Arguments
///
/// * `html_content` - A string slice containing the HTML content to be processed.
///
/// # Returns
///
/// A `Result<String>` which is:
/// - `Ok(String)` containing the cleaned HTML content.
/// - `Err` if any errors occur during processing.
pub fn slim(html_content: &str) -> Result<String> {
	let html = Html::parse_document(html_content);
	let mut output = String::new();

	process_node_recursive_with_indent(html.tree.root(), false, 0, 0, &mut output)?;

	// Final cleanup of empty lines
	let content = remove_empty_lines(output)?;

	Ok(content)
}

/// Strips non-content elements from the provided HTML content, then formats it with
/// the given indentation (number of spaces per nesting level).
///
/// For indentation, block‑level tags get a newline and indent, inline tags stay inline.
/// Pass `0` for flat (no indentation) output.
pub fn slim_with_indent(html_content: &str, indent_spaces: usize) -> Result<String> {
	let html = Html::parse_document(html_content);
	let mut output = String::new();

	process_node_recursive_with_indent(html.tree.root(), false, indent_spaces, 0, &mut output)?;

	let content = remove_empty_lines(output)?;

	Ok(content)
}

/// Recursively processes a node using `scraper`, writing allowed content to the output string,
/// with optional indentation.
fn process_node_recursive_with_indent(
	node: NodeRef<Node>,
	is_in_head_context: bool,
	indent_spaces: usize,
	depth: usize,
	output: &mut String,
) -> Result<()> {
	match node.value() {
		Node::Document => {
			// Process children of the document (Doctype, root element <html>)
			for child in node.children() {
				process_node_recursive_with_indent(child, false, indent_spaces, depth, output)?;
			}
		}

		Node::Doctype(doctype) => {
			// Serialize Doctype manually
			output.push_str("<!DOCTYPE ");
			output.push_str(&doctype.name);
			let has_public = !doctype.public_id.is_empty();
			let has_system = !doctype.system_id.is_empty();

			if has_public {
				output.push_str(" PUBLIC \"");
				output.push_str(&doctype.public_id);
				output.push('"');
			}

			if has_system {
				if !has_public {
					// Add SYSTEM keyword only if no PUBLIC id
					output.push_str(" SYSTEM");
				}
				output.push(' '); // Always add space before system id string if it exists
				output.push('"');
				output.push_str(&doctype.system_id);
				output.push('"');
			}
			output.push('>');

			// After doctype, start a new line for formatted output.
			if indent_spaces > 0 {
				output.push('\n');
			}
		}

		Node::Comment(_) => { /* Skip comments */ }

		Node::Text(text) => {
			let text_content = text.trim();
			if !text_content.is_empty() {
				// Use the raw text provided by scraper, assuming it's decoded.
				output.push_str(text);
			}
		}

		Node::Element(element) => {
			let tag_name = element.name();
			let current_node_is_head = tag_name == "head";
			// Determine context for children: true if current node is <head> or if parent was already in <head>
			let child_context_is_in_head = is_in_head_context || current_node_is_head;

			let el_ref = ElementRef::wrap(node).ok_or_else(|| Error::custom("Failed to wrap node as ElementRef"))?;

			// --- 1. Decide if this element should be skipped entirely (before processing children) ---

			// Skip tags explicitly marked for removal (outside head context)
			// Note: script/style/link/base removal handled separately for clarity.
			if !child_context_is_in_head && TAGS_TO_REMOVE.contains(&tag_name) {
				return Ok(());
			}
			// Skip specific non-content tags always
			if matches!(tag_name, "script" | "style" | "link" | "base" | "svg") {
				return Ok(());
			}

			// Skip elements within <head> context unless they are <title> or allowed <meta>
			if is_in_head_context {
				if tag_name == "title" {
					// Keep title
				} else if tag_name == "meta" {
					if !should_keep_meta(el_ref) {
						return Ok(()); // Remove disallowed meta tag
					}
					// Keep allowed meta
				} else {
					return Ok(()); // Remove other tags inside head context
				}
			}

			// -- Indentation setup
			let is_formatting = indent_spaces > 0;
			let is_block = is_formatting && BLOCK_LEVEL_TAGS.contains(&tag_name);
			// Title is a block‑level tag for formatting purposes but not in the standard list
			let is_block = is_block || (is_formatting && tag_name == "title");
			// For void elements, we must not emit a closing tag (when formatting).
			let is_void = is_formatting && VOID_ELEMENTS.contains(&tag_name);
			let child_depth = if is_block { depth + 1 } else { depth };

			// --- 1b. Emit newline/indent before opening tag (block-level)
			if is_block {
		if output.is_empty() || !output.ends_with('\n') {
					output.push('\n');
				}
				for _ in 0..(depth * indent_spaces) {
					output.push(' ');
				}
			}

			// --- 2. Process Children Recursively into a temporary buffer ---
			let mut children_output = String::new();
			for child in node.children() {
				process_node_recursive_with_indent(
					child,
					child_context_is_in_head,
					indent_spaces,
					child_depth,
					&mut children_output,
				)?;
			}

			// --- 3. Decide whether to keep the current node based on its content *after* processing children ---
			let is_empty_after_processing = is_string_effectively_empty(&children_output);

			// Check if it's a tag eligible for removal when empty (outside head)
			let is_removable_tag_when_empty = !child_context_is_in_head && REMOVABLE_EMPTY_TAGS.contains(&tag_name);

			// Check if it's the <head> tag itself and it's now empty
			let is_empty_head_tag = current_node_is_head && is_empty_after_processing;

			let should_remove_node = (is_removable_tag_when_empty && is_empty_after_processing) || is_empty_head_tag;

			// --- 4. Construct Output if Node is Kept ---
			if !should_remove_node {
				// Build start tag
				output.push('<');
				output.push_str(tag_name);
				filter_and_write_attributes(el_ref, child_context_is_in_head, output)?;
				output.push('>');

				// Append children's content
				output.push_str(&children_output);

				// -- Indent before closing tag (block-level, non‑void, children non‑empty)
			// Only if children output contains newlines (nested block elements)
			if is_block && !is_void && children_output.contains('\n') {
					output.push('\n');
					for _ in 0..(depth * indent_spaces) {
						output.push(' ');
					}
				}

				// Build end tag
				if !is_void {
					output.push_str("</");
					output.push_str(tag_name);
					output.push('>');
				}
			}
		}

		Node::Fragment => {
			// Should not happen with parse_document, but handle defensively
			for child in node.children() {
				process_node_recursive_with_indent(child, false, indent_spaces, depth, output)?;
			}
		}

		Node::ProcessingInstruction(_) => { /* Skip PIs */ }
	}
	Ok(())
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	// Result type alias for tests
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	// Copied and adapted tests from slimmer.rs
	// Renamed slim -> slim2 and test_slimmer_... -> test_slimmer2_...

	#[test]
	fn test_slimmer2_slim_basic() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
	<meta property="og:title" content="Test Title">
	<meta property="og:url" content="http://example.com">
	<meta property="og:image" content="http://example.com/img.png">
	<meta property="og:description" content="Test Description">
	<meta name="keywords" content="test, html"> <!-- Should be removed -->
    <title>Simple HTML Page</title>
	<style> body{ color: red } </style>
	<link rel="stylesheet" href="style.css">
	<script> console.log("hi"); </script>
	<base href="/"> <!-- Should be removed -->
</head>
<body class="main-body" aria-label="Page body">
	<svg><path d="M0 0 L 10 10"></path></svg> <!-- Should be removed -->
	<div>
		<span></span> <!-- Should be removed (effectively empty after processing) -->
		<p> <!-- Effectively empty after processing --> </p>
		<b>  </b> <!-- Effectively empty after processing -->
		<i><!-- comment --></i> <!-- Effectively empty after processing -->
	</div> <!-- Should be removed (effectively empty after children removed) -->
	<section>Content Inside</section> <!-- Should be kept -->
	<article>  </article> <!-- Should be removed (empty after processing) -->
    <h1 funky-attribute="removeme">Hello, World!</h1> <!-- funky-attribute removed -->
    <p>This is a simple HTML page.</p>
	<a href="https://example.org" class="link-style" extra="gone">Link</a> <!-- href and class kept -->
	<!-- Some Comment -->
</body>
</html>
		"#;

		// Expected output should now match slimmer.rs more closely regarding empty element removal.
		// let expected_head_content = r#"<head><meta content="Test Title" property="og:title"><meta content="http://example.com" property="og:url"><meta content="http://example.com/img.png" property="og:image"><meta content="Test Description" property="og:description"><title>Simple HTML Page</title></head>"#;
		let expected_body_content = r#"<body aria-label="Page body" class="main-body"><section>Content Inside</section><h1>Hello, World!</h1><p>This is a simple HTML page.</p><a class="link-style" href="https://example.org">Link</a></body>"#;
		// Note attribute order might differ slightly between scraper/html5ever & string building, but content should match.

		// -- Exec
		let html = slim(fx_html)?;
		// println!(
		// 	"\n---\nSlimmed HTML (Scraper - Basic + Post-Empty Removal):\n{}\n---\n",
		// 	html
		// );

		// -- Check Head Content (More precise check possible now)
		// Need flexible attribute order check for head
		assert!(html.contains("<head>"));
		assert!(html.contains("</head>"));
		assert!(html.contains(r#"<meta content="Test Title" property="og:title">"#));
		assert!(html.contains(r#"<meta content="http://example.com" property="og:url">"#));
		assert!(html.contains(r#"<meta content="http://example.com/img.png" property="og:image">"#));
		assert!(html.contains(r#"<meta content="Test Description" property="og:description">"#));
		assert!(html.contains(r#"<title>Simple HTML Page</title>"#));

		assert!(
			!html.contains("<meta charset") && !html.contains("<meta name"),
			"Should remove disallowed meta tags"
		);
		assert!(
			!html.contains("<style") && !html.contains("<link") && !html.contains("<script") && !html.contains("<base"),
			"Should remove style, link, script, base"
		);

		// -- Check Body Content (More precise check)
		// Allow for attribute order variations in body tag
		assert!(
			html.contains("<body")
				&& html.contains(r#"class="main-body""#)
				&& html.contains(r#"aria-label="Page body""#)
				&& html.contains(">")
		);
		assert!(html.contains(r#"</body>"#));
		assert!(html.contains(expected_body_content)); // Check the exact sequence for the rest

		// Check removals (should now match slimmer.rs)
		assert!(!html.contains("<svg>"), "Should remove svg");
		assert!(!html.contains("<span>"), "Should remove empty span");
		assert!(!html.contains("<p> </p>"), "Should remove empty p tag");
		assert!(!html.contains("<b>"), "Should remove empty b");
		assert!(!html.contains("<i>"), "Should remove empty i");
		assert!(!html.contains("<div>"), "Should remove outer empty div");
		assert!(!html.contains("<article>"), "Should remove empty article");
		assert!(!html.contains("funky-attribute"), "Should remove funky-attribute");
		assert!(!html.contains("extra=\"gone\""), "Should remove extra anchor attribute");
		assert!(!html.contains("<!--"), "Should remove comments");

		Ok(())
	}

	#[test]
	fn test_slimmer2_slim_empty_head_removed() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
		<!DOCTYPE html>
		<html>
		<head>
			<meta charset="utf-8">
			<link rel="icon" href="favicon.ico">
		</head>
		<body>
			<p>Content</p>
		</body>
		</html>
		"#;

		// -- Exec
		let html = slim(fx_html)?;
		// println!("\n---\nSlimmed HTML (Scraper - Empty Head Removed):\n{}\n---\n", html);

		// -- Check
		// The <head> tag itself should now be removed as it becomes empty after processing children.
		assert!(
			!html.contains("<head>"),
			"Empty <head> tag should be removed after processing. Got: {}",
			html
		);
		assert!(html.contains("<body><p>Content</p></body>"), "Body should remain");

		Ok(())
	}

	#[test]
	fn test_slimmer2_slim_keeps_head_if_title_present() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
		<!DOCTYPE html>
		<html>
		<head>
			<title>Only Title</title>
			<script></script>
		</head>
		<body>
			<p>Content</p>
		</body>
		</html>
		"#;

		// -- Exec
		let html = slim(fx_html)?;
		// println!("\n---\nSlimmed HTML (Scraper - Head with Title Kept):\n{}\n---\n", html);

		// -- Check
		// Head should remain as title is kept.
		assert!(
			html.contains("<head><title>Only Title</title></head>"),
			"<head> with only title should remain"
		);
		assert!(!html.contains("<script>"), "Script should be removed");
		assert!(html.contains("<body><p>Content</p></body>"), "Body should remain");

		Ok(())
	}

	#[test]
	fn test_slimmer2_slim_nested_empty_removal() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
		<!DOCTYPE html>
		<html>
		<body>
			<div> <!-- Will become empty after children removed -->
				<p>  </p> <!-- empty p -->
				<div> <!-- Inner div, will become empty -->
					<span><!-- comment --></span> <!-- empty span -->
				</div>
			</div>
			<section>
				<h1>Title</h1> <!-- Keep H1 -->
				<div> </div> <!-- Remove empty div -->
			</section>
		</body>
		</html>
		"#;
		// Expected: Outer div removed, inner div removed, p removed, span removed. Section and H1 remain.
		// This behaviour should now match html5ever version.
		let expected_body = r#"<body><section><h1>Title</h1></section></body>"#;

		// -- Exec
		let html = slim(fx_html)?;
		// println!("\n---\nSlimmed HTML (Scraper - Nested Empty Removed):\n{}\n---\n", html);

		// -- Check
		assert!(
			html.contains(expected_body),
			"Should remove nested empty elements correctly after processing. Expected: '{}', Got: '{}'",
			expected_body,
			html
		);
		assert!(!html.contains("<p>"), "Empty <p> should be removed");
		assert!(!html.contains("<span>"), "Empty <span> should be removed");
		assert!(
			!html.contains("<div>"),
			"All empty <div> tags should be removed (inner and outer)"
		);
		assert!(html.contains("<section>"), "Section should remain");
		assert!(html.contains("<h1>"), "H1 should remain");

		Ok(())
	}

	#[test]
	fn test_slimmer2_slim_keep_empty_but_not_removable() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
		<!DOCTYPE html>
		<html>
		<body>
			<main></main> <!-- Should keep 'main' even if empty -->
			<table><tr><td></td></tr></table> <!-- Should keep table structure even if cells empty -->
		</body>
		</html>
		"#;
		let expected_body_fragment1 = "<main></main>";

		// -- Exec
		let html = slim(fx_html)?;

		// -- Check
		assert!(html.contains(expected_body_fragment1), "Should keep empty <main>");
		// Be flexible with tbody insertion
		assert!(
			html.contains("<table>") && html.contains("<tr>") && html.contains("<td>") && html.contains("</table>"),
			"Should keep empty table structure. Got: {}",
			html
		);

		Ok(())
	}
}

// endregion: --- Tests

#[cfg(test)]
mod test {
	use super::*;
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	#[test]
	fn test_slimmer2_slim_with_indent_formatted_2spaces() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
		<meta property="og:title" content="Test Title">
		<meta property="og:url" content="http://example.com">
		<meta property="og:image" content="http://example.com/img.png">
		<meta property="og:description" content="Test Description">
		<meta name="keywords" content="test, html">
    <title>Simple HTML Page</title>
		<style> body{ color: red } </style>
		<link rel="stylesheet" href="style.css">
		<script> console.log("hi"); </script>
		<base href="/">
</head>
<body class="main-body" aria-label="Page body">
		<svg><path d="M0 0 L 10 10"></path></svg>
		<div>
			<span></span>
			<p> </p>
			<b>  </b>
			<i><!-- comment --></i>
		</div>
		<section>Content Inside</section>
		<article>  </article>
    <h1 funky-attribute="removeme">Hello, World!</h1>
    <p>This is a simple HTML page.</p>
		<a href="https://example.org" class="link-style" extra="gone">Link</a>
</body>
</html>
			"#;

		// -- Exec
		let formatted = slim_with_indent(fx_html, 2)?;
		let flat = slim(fx_html)?; // should be the same content without formatting

		println!("{formatted}");

		// -- Check: formatted output contains the expected indentation structure
		assert!(
			formatted.lines().count() > flat.lines().count(),
			"Formatted output should have more lines"
		);

		// Verify doctype on its own line, followed by <html> with no indent
		assert!(formatted.starts_with("<!DOCTYPE html>\n<html"));

		// Verify head is indented once (2 spaces)
		assert!(formatted.contains("\n  <head>"));

		// Verify title inside head is indented twice (4 spaces)
		assert!(formatted.contains("\n    <title>Simple HTML Page</title>"));

		// Verify meta inside head (void element, no closing tag) indented twice
		assert!(formatted.contains("\n    <meta content=\"Test Title\" property=\"og:title\">"));

		// Verify body indented once
		assert!(formatted.contains("\n  <body aria-label=\"Page body\" class=\"main-body\">"));

		// Verify section inside body (depth 2)
		assert!(formatted.contains("\n    <section>Content Inside</section>"));

		// Verify h1 inside body (depth 2) and removal of funky-attribute
		assert!(formatted.contains("\n    <h1>Hello, World!</h1>"));

		// Verify link inside body (depth 2, void not, so closing tag present)
		assert!(formatted.contains("\n    <a class=\"link-style\" href=\"https://example.org\">Link</a>"));

		// Make sure svg, empty div, etc removed
		assert!(!formatted.contains("<svg>"));
		assert!(!formatted.contains("<div>"));
		assert!(!formatted.contains("funky-attribute"));

		Ok(())
	}
}
