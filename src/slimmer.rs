use crate::support::rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use crate::{Error, Result};
use html5ever::driver::ParseOpts;
use html5ever::parse_document;
use html5ever::serialize::SerializeOpts;
use html5ever::tendril::TendrilSink;

// region:    --- Constants

/// Tags to remove explicitly, regardless of content (unless within <head>).
const TAGS_TO_REMOVE: &[&str] = &["script", "link", "style", "svg", "base"];

/// Tags that should be removed if they become effectively empty (contain only whitespace/comments).
/// Applies only outside the <head> element.
const REMOVABLE_EMPTY_TAGS: &[&str] = &[
	"div", "span", "p", "i", "b", "em", "strong", "section", "article", "header", "footer", "nav", "aside",
];

/// Keywords to check within the 'property' attribute of <meta> tags to determine if they should be kept.
const META_PROPERTY_KEYWORDS: &[&str] = &["title", "url", "image", "description"];

/// Attribute names allowed on <meta> tags within the <head>.
const ALLOWED_META_ATTRS: &[&str] = &["property", "content"];

/// Attribute names allowed on elements outside the <head>.
const ALLOWED_BODY_ATTRS: &[&str] = &["class", "aria-label", "href", "title", "id"];

// endregion: --- Constants

/// Decodes HTML entities (e.g., `&lt;` becomes `<`).
pub fn decode_html_entities(content: &str) -> String {
	html_escape::decode_html_entities(content).to_string()
}

/// Strips non-content elements from the provided HTML content, preserving essential head tags,
/// and returns the cleaned HTML as a string.
///
/// This function removes:
/// - Non-visible tags like `<script>`, `<link>`, `<style>`, `<svg>`, `<base>`.
/// - HTML comments.
/// - Empty or whitespace-only text nodes.
/// - Specific tags (like `<div>`, `<span>`, `<p>`, etc.) if they become effectively empty after processing children.
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
/// - `Err` if any parsing or serialization errors occur.
pub fn slim(html_content: &str) -> Result<String> {
	let dom = parse_document(RcDom::default(), ParseOpts::default())
		.from_utf8()
		.read_from(&mut html_content.as_bytes())?;

	// Process the document starting from the root, initially not inside <head>
	process_node_recursive(&dom.document, false)?;

	let document: SerializableHandle = dom.document.clone().into();
	let serialize_opts = SerializeOpts {
		// script_enabled: false, // Keep default, irrelevant as scripts are removed
		// traversal_scope: markup5ever::serialize::TraversalScope::IncludeNode, // Default
		// create_missing_html_ns: true, // Keep default
		..Default::default()
	};

	let mut output = Vec::new();
	html5ever::serialize(&mut output, &document, serialize_opts)?;

	let content =
		String::from_utf8(output).map_err(|err| Error::custom(format!("html5ever serialization non utf8. {err}")))?;
	let content = remove_empty_lines(content)?;

	Ok(content)
}

/// Removes empty lines from the given content, returning the cleaned string.
fn remove_empty_lines(content: String) -> Result<String> {
	let lines: Vec<&str> = content.lines().filter(|line| !line.trim().is_empty()).collect();
	Ok(lines.join("\n"))
}

/// Recursively processes the DOM tree, removing unwanted nodes and attributes.
/// Returns Ok(true) if the node should be kept, Ok(false) if it should be removed.
fn process_node_recursive(handle: &Handle, is_in_head_context: bool) -> Result<bool> {
	let should_keep = match &handle.data {
		NodeData::Element { name, .. } => {
			let tag_local_name_str = name.local.as_ref();
			let current_node_is_head = tag_local_name_str == "head";
			// Determine context for children: true if current node is <head> or if parent was already in <head>
			let child_context_is_in_head = is_in_head_context || current_node_is_head;

			let mut keep_current_node: bool;

			// --- Determine if the current node itself should be kept (initial decision) ---
			if is_in_head_context {
				// Rules for nodes *directly* within <head> context
				if tag_local_name_str == "title" {
					keep_current_node = true; // Keep <title>
				} else if tag_local_name_str == "meta" {
					keep_current_node = should_keep_meta(handle); // Keep specific <meta> tags
				} else {
					keep_current_node = false; // Remove other tags within <head> context
				}
			} else {
				// Rules for nodes *outside* <head> context OR the <head> tag itself
				// Compare tag name string directly using the constant list
				if TAGS_TO_REMOVE.contains(&tag_local_name_str) {
					keep_current_node = false; // Remove explicitly listed tags
				} else {
					// Keep <head>, <body>, <html>, and other tags by default unless explicitly removed or emptied later.
					keep_current_node = true;
				}
			}

			// --- Process Children Recursively ---
			if keep_current_node {
				let mut indices_to_remove = Vec::new();
				let children_handles = handle.children.borrow().clone(); // Clone Vec<Rc<Node>> for iteration

				for (index, child) in children_handles.iter().enumerate() {
					// Recurse and check if the child should be kept
					if !process_node_recursive(child, child_context_is_in_head)? {
						indices_to_remove.push(index);
					}
				}

				// Remove children marked for removal after iteration
				if !indices_to_remove.is_empty() {
					let mut children_mut = handle
						.children
						.try_borrow_mut()
						.map_err(|err| Error::custom(format!("Node children already borrowed mutably: {err}")))?;
					for &index in indices_to_remove.iter().rev() {
						// index must be valid as we iterated over the original length
						if index < children_mut.len() {
							children_mut.remove(index);
						} else {
							// This case should ideally not happen if indexing is correct
							eprintln!("Warning: Attempted to remove child at invalid index {}", index);
						}
					}
				}

				// --- Filter Attributes of the current node (if kept) ---
				// Pass the context where the node *lives* (is_in_head_context || current_node_is_head)
				filter_attributes(handle, child_context_is_in_head)?;

				// --- Re-evaluate if the current node should be kept (post-processing) ---

				// Remove <head> if it became empty after processing children/attributes,
				// or remove specific tags if they are effectively empty (only applies outside <head>)
				if (current_node_is_head && handle.children.borrow().is_empty())
					|| (!child_context_is_in_head // Check applies outside <head>
    && REMOVABLE_EMPTY_TAGS.contains(&tag_local_name_str) // Compare string directly
    && is_effectively_empty(handle))
				{
					keep_current_node = false;
				}
			}
			// Return the final decision
			keep_current_node
		}
		NodeData::Comment { .. } => false, // Remove comments
		NodeData::Text { contents } => !contents.borrow().trim().is_empty(), // Keep non-empty text
		NodeData::Document => {
			// Process children of the document root, always keep the document node itself
			let mut indices_to_remove = Vec::new();
			let children_handles = handle.children.borrow().clone();
			for (index, child) in children_handles.iter().enumerate() {
				if !process_node_recursive(child, false)? {
					// Start children with is_in_head_context = false
					indices_to_remove.push(index);
				}
			}
			if !indices_to_remove.is_empty() {
				let mut children_mut = handle
					.children
					.try_borrow_mut()
					.map_err(|err| Error::custom(format!("Doc children already borrowed mutably: {err}")))?;
				for &index in indices_to_remove.iter().rev() {
					if index < children_mut.len() {
						children_mut.remove(index);
					}
				}
			}
			true // Keep the document node
		}
		NodeData::Doctype { .. } => true,                // Keep Doctype
		NodeData::ProcessingInstruction { .. } => false, // Remove PIs
	};
	Ok(should_keep)
}

/// Checks if a node contains only whitespace text nodes or comments.
fn is_effectively_empty(handle: &Handle) -> bool {
	handle.children.borrow().iter().all(|child| match &child.data {
		NodeData::Text { contents } => contents.borrow().trim().is_empty(),
		NodeData::Comment { .. } => true, // Comments are ignored/removed elsewhere, treat as empty component
		// Any other node type (Element, Doctype, PI) means it's not effectively empty
		_ => false,
	})
}

/// Checks if a `<meta>` tag handle should be kept based on its `property` attribute.
fn should_keep_meta(handle: &Handle) -> bool {
	if let NodeData::Element { ref attrs, .. } = handle.data {
		// Borrow attributes immutably
		let attributes = attrs.borrow();
		for attr in attributes.iter() {
			// Check if the attribute name is 'property'
			if attr.name.local.as_ref() == "property" {
				let value = attr.value.to_lowercase();
				// Check if the property value contains any of the relevant keywords
				if META_PROPERTY_KEYWORDS.iter().any(|&keyword| value.contains(keyword)) {
					return true; // Keep this meta tag
				}
			}
		}
	}
	false // Do not keep if not meta or property doesn't match
}

/// Filters attributes of an element node based on whether it's inside the `<head>` section context.
fn filter_attributes(handle: &Handle, is_in_head_context: bool) -> Result<()> {
	if let NodeData::Element {
		ref name, ref attrs, ..
	} = handle.data
	{
		// Borrow attributes mutably to retain specific ones
		let mut attributes = attrs
			.try_borrow_mut()
			.map_err(|err| Error::custom(format!("Attrs already borrowed mutably for <{}>: {}", name.local, err)))?;

		let tag_local_name_str = name.local.as_ref();

		if is_in_head_context {
			if tag_local_name_str == "meta" {
				// For <meta> tags inside <head>, keep attributes from the allowed list
				attributes.retain(|attr| ALLOWED_META_ATTRS.contains(&attr.name.local.as_ref()));
			} else if tag_local_name_str == "title" {
				// For <title> tags, remove all attributes
				attributes.clear();
			} else {
				// For other unexpected tags potentially kept inside head, clear attributes just in case
				attributes.clear();
			}
		} else {
			// For elements outside <head>, keep attributes from the allowed list
			attributes.retain(|attr| ALLOWED_BODY_ATTRS.contains(&attr.name.local.as_ref()));
		}
	}
	Ok(())
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	// Result type alias for tests
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	#[test]
	fn test_slimmer_slim_basic() -> TestResult<()> {
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
		<span></span> <!-- Should be removed (effectively empty) -->
		<p> <!-- Effectively empty --> </p>
		<b>  </b> <!-- Effectively empty -->
		<i><!-- comment --></i> <!-- Effectively empty -->
	</div> <!-- Should be removed (effectively empty after children removed) -->
	<section>Content Inside</section> <!-- Should be kept -->
	<article>  </article> <!-- Should be removed -->
    <h1 funky-attribute="removeme">Hello, World!</h1> <!-- funky-attribute removed -->
    <p>This is a simple HTML page.</p>
	<a href="https://example.org" class="link-style" extra="gone">Link</a> <!-- href and class kept -->
	<!-- Some Comment -->
</body>
</html>
		"#;

		let expected_head_content = r#"<head><meta property="og:title" content="Test Title"><meta property="og:url" content="http://example.com"><meta property="og:image" content="http://example.com/img.png"><meta property="og:description" content="Test Description"><title>Simple HTML Page</title></head>"#;
		// Note: The outer <div>, inner <span>, <p>, <b>, <i> and <article> are now removed because they become empty.
		let expected_body_content = r#"<body class="main-body" aria-label="Page body"><section>Content Inside</section><h1>Hello, World!</h1><p>This is a simple HTML page.</p><a href="https://example.org" class="link-style">Link</a></body>"#;

		// -- Exec
		let html = slim(fx_html)?;
		println!("\n---\nSlimmed HTML (Basic + Empty Removal):\n{}\n---\n", html);

		// -- Check Head Content
		assert!(
			html.contains(expected_head_content),
			"Should contain cleaned head content"
		);
		assert!(html.contains("<title>Simple HTML Page</title>"), "Should keep title");
		assert!(html.contains(r#"meta property="og:title""#), "Should keep meta title");
		assert!(html.contains(r#"meta property="og:url""#), "Should keep meta url");
		assert!(html.contains(r#"meta property="og:image""#), "Should keep meta image");
		assert!(
			html.contains(r#"meta property="og:description""#),
			"Should keep meta description"
		);
		assert!(!html.contains("<meta charset"), "Should remove meta charset");
		assert!(!html.contains("<meta name"), "Should remove meta name tags");
		assert!(!html.contains("<style>"), "Should remove style");
		assert!(!html.contains("<link"), "Should remove link");
		assert!(!html.contains("<script"), "Should remove script from head");
		assert!(!html.contains("<base"), "Should remove base");

		// -- Check Body Content
		assert!(
			html.contains(expected_body_content),
			"Should contain cleaned body content (with empty elements removed)"
		);
		assert!(!html.contains("<svg>"), "Should remove svg");
		assert!(!html.contains("<span>"), "Should remove empty span");
		assert!(!html.contains("<p> </p>"), "Should remove empty p");
		assert!(!html.contains("<b>"), "Should remove empty b");
		assert!(!html.contains("<i>"), "Should remove empty i");
		assert!(!html.contains("<div>"), "Should remove outer empty div");
		assert!(!html.contains("<article>"), "Should remove empty article");
		assert!(
			html.contains("<section>Content Inside</section>"),
			"Should keep non-empty section"
		);
		assert!(html.contains("<h1>Hello, World!</h1>"), "Should keep h1");
		assert!(!html.contains("funky-attribute"), "Should remove funky-attribute");
		assert!(
			html.contains(r#"<body class="main-body" aria-label="Page body">"#),
			"Should keep body attributes"
		);
		assert!(
			html.contains(r#"<a href="https://example.org" class="link-style">Link</a>"#),
			"Should keep allowed anchor attributes"
		);
		assert!(!html.contains("extra=\"gone\""), "Should remove extra anchor attribute");
		assert!(!html.contains("<!--"), "Should remove comments");

		Ok(())
	}

	#[test]
	fn test_slimmer_slim_empty_head_removed() -> TestResult<()> {
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
		println!("\n---\nSlimmed HTML (Empty Head):\n{}\n---\n", html);

		// -- Check
		assert!(
			!html.contains("<head>"),
			"Empty <head> tag should be removed after processing"
		);
		assert!(html.contains("<body><p>Content</p></body>"), "Body should remain");

		Ok(())
	}

	#[test]
	fn test_slimmer_slim_keeps_head_if_title_present() -> TestResult<()> {
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
		println!("\n---\nSlimmed HTML (Head with Title):\n{}\n---\n", html);

		// -- Check
		assert!(
			html.contains("<head><title>Only Title</title></head>"),
			"<head> with only title should remain"
		);
		assert!(!html.contains("<script>"), "Script should be removed");
		assert!(html.contains("<body><p>Content</p></body>"), "Body should remain");

		Ok(())
	}

	#[test]
	fn test_slimmer_slim_nested_empty_removal() -> TestResult<()> {
		// -- Setup & Fixtures
		let fx_html = r#"
		<!DOCTYPE html>
		<html>
		<body>
			<div>
				<p>  </p> <!-- empty p -->
				<div> <!-- Inner div -->
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
		let expected_body = r#"<body><section><h1>Title</h1></section></body>"#;

		// -- Exec
		let html = slim(fx_html)?;
		println!("\n---\nSlimmed HTML (Nested Empty):\n{}\n---\n", html);

		// -- Check
		assert!(
			html.contains(expected_body),
			"Should remove nested empty elements correctly"
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
	fn test_slimmer_slim_keep_empty_but_not_removable() -> TestResult<()> {
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
		// let expected_body = r#"<body><main></main><table><tbody><tr><td></td></tr></tbody></table></body>"#;
		// // Note: tbody is often inserted by parser

		// -- Exec
		let html = slim(fx_html)?;
		println!("\n---\nSlimmed HTML (Keep Non-Removable Empty):\n{}\n---\n", html);

		// -- Check
		// Need a flexible check because the parser might add tbody
		assert!(html.contains("<main>"), "Should keep empty <main>");
		assert!(html.contains("<table>"), "Should keep empty <table>");
		assert!(html.contains("<tr>"), "Should keep empty <tr>");
		assert!(html.contains("<td>"), "Should keep empty <td>");

		Ok(())
	}
}

// endregion: --- Tests
