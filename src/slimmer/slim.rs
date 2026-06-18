use super::SlimOptions;
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
pub fn slim(html_content: &str, options: impl Into<SlimOptions>) -> Result<String> {
	let options = options.into();
	let html = Html::parse_document(html_content);
	let mut output = String::new();

	process_node_stack_based(html.tree.root(), false, &options, 0, &mut output)?;

	// Final cleanup of empty lines
	let content = remove_empty_lines(output)?;

	Ok(content)
}

/// Non‑recursive stack‑based version of the slim processing.
fn process_node_stack_based(
	root_node: NodeRef<Node>,
	is_in_head_context: bool,
	options: &SlimOptions,
	depth: usize,
	output: &mut String,
) -> Result<()> {
	let indent_spaces = options.indent.unwrap_or(0) as usize;
	let use_tabs = options.indent_with_tabs;

	#[derive(Clone)]
	enum FrameState {
		Enter,
		Exit,
	}

	struct Frame<'a> {
		node: NodeRef<'a, Node>,
		is_in_head_context: bool,
		depth: usize,
		state: FrameState,
		children_output: String,
		/// Where this frame's output should be appended.
		/// `Some(idx)` means the frame at the given stack index is the parent
		/// that will collect our output; `None` means append to global output.
		output_target_index: Option<usize>,
	}

	let mut stack: Vec<Frame> = Vec::new();
	stack.push(Frame {
		node: root_node,
		is_in_head_context,
		depth,
		state: FrameState::Enter,
		children_output: String::new(),
		output_target_index: None,
	});

	while let Some(frame) = stack.pop() {
		match frame.state {
			FrameState::Enter => {
				match frame.node.value() {
					Node::Document => {
						// Push children in reverse order (no Exit needed)
						let children: Vec<_> = frame.node.children().collect();
						for child in children.into_iter().rev() {
							stack.push(Frame {
								node: child,
								is_in_head_context: false,
								depth: frame.depth,
								state: FrameState::Enter,
								children_output: String::new(),
								output_target_index: frame.output_target_index,
							});
						}
					}
					Node::Doctype(doctype) => {
						// Serialize Doctype
						let mut s = String::new();
						s.push_str("<!DOCTYPE ");
						s.push_str(&doctype.name);
						let has_public = !doctype.public_id.is_empty();
						let has_system = !doctype.system_id.is_empty();

						if has_public {
							s.push_str(" PUBLIC \"");
							s.push_str(&doctype.public_id);
							s.push('"');
						}

						if has_system {
							if !has_public {
								s.push_str(" SYSTEM");
							}
							s.push(' ');
							s.push('"');
							s.push_str(&doctype.system_id);
							s.push('"');
						}
						s.push('>');

						if indent_spaces > 0 {
							s.push('\n');
						}

						// Append to parent frame or global output
						match frame.output_target_index {
							Some(idx) => {
								stack
									.get_mut(idx)
									.expect("target frame should exist")
									.children_output
									.push_str(&s);
							}
							None => {
								output.push_str(&s);
							}
						}
					}
					Node::Comment(_) => { /* Skip comments */ }
					Node::Text(text) => {
						let text_content = text.trim();
						if !text_content.is_empty() {
							let s = text.to_string();
							match frame.output_target_index {
								Some(idx) => {
									stack
										.get_mut(idx)
										.expect("target frame should exist")
										.children_output
										.push_str(&s);
								}
								None => {
									output.push_str(&s);
								}
							}
						}
					}
					Node::Element(element) => {
						let tag_name = element.name();

						// Handle <html> as transparent container: push children directly, no wrapper.
						if tag_name == "html" {
							let child_context_is_in_head = frame.is_in_head_context;
							let mut children: Vec<_> = frame.node.children().collect();
							children.reverse();
							for child in children {
								stack.push(Frame {
									node: child,
									is_in_head_context: child_context_is_in_head,
									depth: frame.depth,
									state: FrameState::Enter,
									children_output: String::new(),
									output_target_index: frame.output_target_index,
								});
							}
							continue;
						}

						let el_ref = ElementRef::wrap(frame.node)
							.ok_or_else(|| Error::custom("Failed to wrap node as ElementRef"))?;

						let current_node_is_head = tag_name == "head";
						let child_context_is_in_head = frame.is_in_head_context || current_node_is_head;

						// Fast-skip rules
						// Fast-skip rules
						let should_skip = match tag_name {
							_ if !child_context_is_in_head && TAGS_TO_REMOVE.contains(&tag_name) => true,
							"script" | "style" | "link" | "base" | "svg" => true,
							_ if frame.is_in_head_context => {
								!(tag_name == "title" || (tag_name == "meta" && should_keep_meta(el_ref)))
							}
							_ => false,
						};

						if should_skip {
							continue;
						}

						// Push Exit frame for this element
						let exit_idx = stack.len();
						stack.push(Frame {
							node: frame.node,
							is_in_head_context: frame.is_in_head_context,
							depth: frame.depth,
							state: FrameState::Exit,
							children_output: String::new(),
							output_target_index: frame.output_target_index,
						});

						// Compute child depth and push children in reverse order
						let is_formatting = indent_spaces > 0 || use_tabs;
						let is_block = if is_formatting {
							BLOCK_LEVEL_TAGS.contains(&tag_name) || tag_name == "title"
						} else {
							false
						};
						let child_depth = if is_block { frame.depth + 1 } else { frame.depth };

						let mut children: Vec<_> = frame.node.children().collect();
						children.reverse();
						for child in children {
							stack.push(Frame {
								node: child,
								is_in_head_context: child_context_is_in_head,
								depth: child_depth,
								state: FrameState::Enter,
								children_output: String::new(),
								output_target_index: Some(exit_idx),
							});
						}
					}
					Node::Fragment => {
						let children: Vec<_> = frame.node.children().collect();
						for child in children.into_iter().rev() {
							stack.push(Frame {
								node: child,
								is_in_head_context: false,
								depth: frame.depth,
								state: FrameState::Enter,
								children_output: String::new(),
								output_target_index: frame.output_target_index,
							});
						}
					}
					Node::ProcessingInstruction(_) => { /* Skip PIs */ }
				}
			}
			FrameState::Exit => {
				let el_ref =
					ElementRef::wrap(frame.node).ok_or_else(|| Error::custom("Failed to wrap node as ElementRef"))?;
				let tag_name = el_ref.value().name();

				let is_formatting = indent_spaces > 0 || use_tabs;
				let is_block = if is_formatting {
					BLOCK_LEVEL_TAGS.contains(&tag_name) || tag_name == "title"
				} else {
					false
				};
				let is_void = is_formatting && VOID_ELEMENTS.contains(&tag_name);

				let is_empty_after_processing = is_string_effectively_empty(&frame.children_output);
				let is_in_head_for_removal = frame.is_in_head_context || tag_name == "head";
				let is_removable_tag_when_empty = !is_in_head_for_removal && REMOVABLE_EMPTY_TAGS.contains(&tag_name);
				let is_empty_head_tag = tag_name == "head" && is_empty_after_processing;
				let should_remove = (is_removable_tag_when_empty && is_empty_after_processing) || is_empty_head_tag;

				if should_remove {
					continue;
				}

				let mut out = String::new();

				// Indent before opening tag (block‑level)
				if is_block {
					out.push('\n');
					let indent_str = if use_tabs {
						"\t".repeat(frame.depth)
					} else {
						" ".repeat(frame.depth * indent_spaces)
					};
					out.push_str(&indent_str);
				}

				// Start tag with filtered attributes
				out.push('<');
				out.push_str(tag_name);
				// Attribute filter uses the head‑context of the element itself
				let is_in_head_for_attrs = frame.is_in_head_context || tag_name == "head";
				filter_and_write_attributes(el_ref, is_in_head_for_attrs, &mut out)?;
				out.push('>');

				// Append children output
				out.push_str(&frame.children_output);

				// Indent before closing tag if needed
				if is_block && !is_void && frame.children_output.contains('\n') {
					out.push('\n');
					let indent_str = if use_tabs {
						"\t".repeat(frame.depth)
					} else {
						" ".repeat(frame.depth * indent_spaces)
					};
					out.push_str(&indent_str);
				}

				// Closing tag unless void
				if !is_void {
					out.push_str("</");
					out.push_str(tag_name);
					out.push('>');
				}

				// Append to parent frame or global output
				match frame.output_target_index {
					Some(idx) => {
						stack
							.get_mut(idx)
							.expect("target frame should exist")
							.children_output
							.push_str(&out);
					}
					None => {
						output.push_str(&out);
					}
				}
			}
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
		let html = slim(fx_html, SlimOptions::default())?;
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
		let html = slim(fx_html, SlimOptions::default())?;
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
		let html = slim(fx_html, SlimOptions::default())?;
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
		let html = slim(fx_html, SlimOptions::default())?;
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
		let html = slim(fx_html, SlimOptions::default())?;

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
