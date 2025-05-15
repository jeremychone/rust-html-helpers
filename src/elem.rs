use serde::Serialize;
use std::collections::HashMap;

/// Represents a simplified HTML element, suitable for serialization.
#[derive(Debug, Serialize)]
pub struct Elem {
	pub tag: String,
	pub attrs: HashMap<String, String>,
	pub text: Option<String>,
	pub inner_html: Option<String>,
}

impl Elem {
	/// Creates a new `Elem` from a `scraper::ElementRef`.
	pub(crate) fn from_element_ref(el: scraper::ElementRef) -> Self {
		let tag = el.value().name().to_string();

		let attrs = el.value().attrs().map(|(k, v)| (k.to_string(), v.to_string())).collect();

		let full_text = el.text().collect::<String>();
		let trimmed_text = full_text.trim();
		let text = if trimmed_text.is_empty() {
			None
		} else {
			Some(trimmed_text.to_string())
		};

		let html_content = el.inner_html();
		let trimmed_html = html_content.trim();
		let inner_html = if trimmed_html.is_empty() {
			None
		} else {
			Some(trimmed_html.to_string())
		};

		Elem {
			tag,
			attrs,
			text,
			inner_html,
		}
	}
}
