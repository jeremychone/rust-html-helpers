use crate::{Elem, Error, Result};
use scraper::{Html, Selector};

/// Selects HTML elements based on a list of CSS selectors and returns them as a list of `Elem`.
/// The selectors are combined with a comma, effectively performing an "OR" match.
/// Elements are returned in document order.
///
/// # Arguments
///
/// * `html_content` - A string slice containing the HTML content to parse.
/// * `selectors` - An iterator of string-like items, each representing a CSS selector.
///
/// # Returns
///
/// A `Result` containing:
/// - `Ok(Vec<Elem>)`: A vector of `Elem` objects representing the selected elements.
/// - `Err(Error)`: An error if parsing the HTML or the combined selector fails.
pub fn select<S>(html_content: &str, selectors: S) -> Result<Vec<Elem>>
where
	S: IntoIterator,
	S::Item: AsRef<str>,
{
	// -- Build the selectors_str
	let mut selectors_str = String::new();
	for s_ref in selectors {
		let s = s_ref.as_ref().trim();
		if s.is_empty() {
			continue;
		}
		if !selectors_str.is_empty() {
			selectors_str.push(',');
		}
		selectors_str.push_str(s);
	}
	// if empty, just return empty vector
	if selectors_str.is_empty() {
		return Ok(Vec::new());
	}
	// build the scraper seletor
	let css_selector = Selector::parse(&selectors_str).map_err(|err| Error::SelectorParse {
		selector: selectors_str.clone(),
		cause: err.to_string(),
	})?;

	// -- Parse and select
	let html = Html::parse_document(html_content);

	let mut els = Vec::new();
	for element_ref in html.select(&css_selector) {
		els.push(Elem::from_element_ref(element_ref));
	}

	Ok(els)
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	// General test functions use this local `Result<T>` for `Box<dyn Error>`.
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	#[test]
	fn test_selector_select_simple_single_selector() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = r#"
			<!DOCTYPE html>
			<html>
			<head><title>Test</title></head>
			<body>
				<div id="main" class="container">
					<h1>Title</h1>
					<p>First paragraph.</p>
					<p class="highlight">Second paragraph with <span>span text</span>.</p>
					<ul>
						<li>Item 1</li>
						<li>Item 2</li>
					</ul>
				</div>
			</body>
			</html>
		"#;

		// -- Exec
		let els_p = select(html_content, vec!["p"])?;

		// -- Check
		assert_eq!(els_p.len(), 2);

		assert_eq!(els_p[0].tag, "p");
		assert!(els_p[0].attrs.is_none());
		assert_eq!(els_p[0].text.as_deref(), Some("First paragraph."));
		assert_eq!(els_p[0].inner_html.as_deref(), Some("First paragraph."));

		assert_eq!(els_p[1].tag, "p");
		assert_eq!(
			els_p[1]
				.attrs
				.as_ref()
				.ok_or("Should have attrs")?
				.get("class")
				.map(|s| s.as_str()),
			Some("highlight")
		);
		assert_eq!(els_p[1].text.as_deref(), Some("Second paragraph with span text."));
		assert_eq!(
			els_p[1].inner_html.as_deref(),
			Some("Second paragraph with <span>span text</span>.")
		);

		// -- Exec & Check - Span inside p.highlight
		let els_span_in_p = select(html_content, ["p.highlight span"])?;
		assert_eq!(els_span_in_p.len(), 1);
		assert_eq!(els_span_in_p[0].tag, "span");
		assert_eq!(els_span_in_p[0].text.as_deref(), Some("span text"));
		assert_eq!(els_span_in_p[0].inner_html.as_deref(), Some("span text"));

		Ok(())
	}

	#[test]
	fn test_selector_select_multiple_selectors_or_logic() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = r#"
            <h1>Title 1</h1>
            <p>Paragraph 1</p>
            <h2>Title 2</h2>
            <div>Div content</div>
            <p>Paragraph 2</p>
        "#;

		// -- Exec
		// Selects elements matching h1 OR p OR h3. (h3 does not exist)
		// Order should be document order.
		let els = select(html_content, ["h1", "p", "h3"])?;

		// -- Check
		assert_eq!(els.len(), 3, "Should find one h1 and two p tags");
		assert_eq!(els[0].tag, "h1");
		assert_eq!(els[0].text.as_deref(), Some("Title 1"));
		assert_eq!(els[1].tag, "p");
		assert_eq!(els[1].text.as_deref(), Some("Paragraph 1"));
		assert_eq!(els[2].tag, "p");
		assert_eq!(els[2].text.as_deref(), Some("Paragraph 2"));

		// -- Exec & Check - Different order of selectors, same result order
		let els_reordered_selectors = select(html_content, ["p", "h1"])?;
		assert_eq!(els_reordered_selectors.len(), 3);
		assert_eq!(
			els_reordered_selectors[0].tag, "h1",
			"Order is document order, not selector order"
		);
		assert_eq!(els_reordered_selectors[1].tag, "p");
		assert_eq!(els_reordered_selectors[2].tag, "p");

		// -- Exec & Check - Select div and h2
		let els_div_h2 = select(html_content, ["div", "h2"])?;
		assert_eq!(els_div_h2.len(), 2);
		assert_eq!(els_div_h2[0].tag, "h2");
		assert_eq!(els_div_h2[0].text.as_deref(), Some("Title 2"));
		assert_eq!(els_div_h2[1].tag, "div");
		assert_eq!(els_div_h2[1].text.as_deref(), Some("Div content"));

		Ok(())
	}

	#[test]
	fn test_selector_select_by_id_and_class_single_selector() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = r#"
			<div id="unique">ID Content</div>
			<div class="group">Class Content 1</div>
			<span class="group">Class Content 2</span>
		"#;

		// -- Exec & Check - By ID
		let els_id = select(html_content, ["#unique"])?;
		assert_eq!(els_id.len(), 1);
		assert_eq!(els_id[0].tag, "div");
		assert_eq!(
			els_id[0]
				.attrs
				.as_ref()
				.ok_or("Should have attrs")?
				.get("id")
				.map(|s| s.as_str()),
			Some("unique")
		);
		assert_eq!(els_id[0].text.as_deref(), Some("ID Content"));

		// -- Exec & Check - By Class
		let els_class = select(html_content, [".group"])?;
		assert_eq!(els_class.len(), 2);
		assert_eq!(els_class[0].tag, "div");
		assert_eq!(els_class[0].text.as_deref(), Some("Class Content 1"));
		assert_eq!(els_class[1].tag, "span");
		assert_eq!(els_class[1].text.as_deref(), Some("Class Content 2"));

		Ok(())
	}

	#[test]
	fn test_selector_select_empty_selector_single() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = "<p>No divs here</p>";

		// -- Exec & Check - Non-existent tag
		let els_div = select(html_content, ["div"])?;
		assert!(els_div.is_empty());

		// -- Exec & Check - Non-existent class
		let els_class = select(html_content, [".missing"])?;
		assert!(els_class.is_empty());

		// -- Exec & Check - Multiple non-existent selectors
		let els_multiple_missing = select(html_content, ["div.foo", ".bar", "main"])?;
		assert!(els_multiple_missing.is_empty());

		Ok(())
	}

	#[test]
	fn test_selector_select_empty_selector_multiple() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = "<p>Some content</p>";

		// -- Exec
		let res = select(html_content, ["", ""])?;

		// -- Check
		assert!(res.is_empty(), "Elem vector should be empty");

		Ok(())
	}

	#[test]
	fn test_selector_select_empty_selector_mixed() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = "<p>Some content</p><span> other content<span>";

		// -- Exec
		let res = select(html_content, ["", "p", ""])?;

		// -- Check
		assert_eq!(res.len(), 1,);
		let el = res.first().ok_or("Should have one item")?;
		assert_eq!(&el.tag, "p");

		Ok(())
	}

	#[test]
	fn test_selector_select_invalid_selector_syntax() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = "<p>Some content</p>";

		// -- Exec
		let res = select(html_content, ["p", "h1[", "div"]); // Invalid selector syntax

		// -- Check
		let Err(err) = res else {
			panic!("Should have been an error for invalid selector syntax")
		};
		let err_string = err.to_string();
		// scraper's error for "p[" is "Invalid selector: Expected an attribute name, found Eof"
		assert!(err_string.contains("is invalid"));

		Ok(())
	}

	#[test]
	fn test_selector_select_empty_iterator_is_error() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = "<p>Some content</p>";

		// -- Exec
		let res = select(html_content, Vec::<&str>::new())?; // Empty iterator

		// -- Check
		assert!(res.is_empty(), "Elem vector should be empty");

		Ok(())
	}

	#[test]
	fn test_selector_select_attributes_and_inner_html_single_selector() -> Result<()> {
		// -- Setup & Fixtures
		let html_content =
			r#"<a href="https://example.com" title="Test Link" class="external link">Click <b>here</b></a>"#;

		// -- Exec
		let snodes = select(html_content, ["a.link"])?;

		// -- Check
		assert_eq!(snodes.len(), 1);
		let node = &snodes[0];
		assert_eq!(node.tag, "a");
		let attrs = node.attrs.as_ref().ok_or("should have attrs")?;
		assert_eq!(attrs.len(), 3);
		assert_eq!(attrs.get("href").map(|s| s.as_str()), Some("https://example.com"));
		assert_eq!(attrs.get("title").map(|s| s.as_str()), Some("Test Link"));
		assert_eq!(attrs.get("class").map(|s| s.as_str()), Some("external link"));

		assert_eq!(node.text.as_deref(), Some("Click here"));
		assert_eq!(node.inner_html.as_deref(), Some("Click <b>here</b>"));

		Ok(())
	}

	// NOTE: Now, the lib does not trim anymore.
	#[test]
	fn test_selector_select_text_and_inner_html_trimming_single_selector() -> Result<()> {
		// -- Setup & Fixtures
		let html_content = r#"
            <p>  Trimmed text here  </p>
            <div>  <span>  Inner  </span>  </div>
            <pre>
            Untrimmed  
            </pre>
            <button>  </button>
        "#;

		// -- Exec & Check - Paragraph text
		let p_nodes = select(html_content, ["p"])?;
		assert_eq!(p_nodes.len(), 1);
		assert_eq!(p_nodes[0].text.as_deref(), Some("  Trimmed text here  "));
		assert_eq!(p_nodes[0].inner_html.as_deref(), Some("  Trimmed text here  "));

		// -- Exec & Check - Div with span
		let div_nodes = select(html_content, ["div"])?;
		assert_eq!(div_nodes.len(), 1);
		assert_eq!(div_nodes[0].text.as_deref(), Some("    Inner    "));
		assert_eq!(div_nodes[0].inner_html.as_deref(), Some("  <span>  Inner  </span>  "));

		// -- Exec & Check - Pre
		let pre_nodes = select(html_content, ["pre"])?;
		assert_eq!(pre_nodes.len(), 1);
		assert_eq!(
			pre_nodes[0].text.as_deref(),
			Some("            Untrimmed  \n            ")
		);
		assert_eq!(
			pre_nodes[0].inner_html.as_deref(),
			Some("            Untrimmed  \n            ")
		);

		// -- Exec & Check - Empty button
		let button_nodes = select(html_content, ["button"])?;
		assert_eq!(button_nodes.len(), 1);
		assert_eq!(button_nodes[0].text, None);
		assert_eq!(button_nodes[0].inner_html, None);

		Ok(())
	}
}

// endregion: --- Tests
