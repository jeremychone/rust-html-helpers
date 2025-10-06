use serde::Serialize;
use std::collections::HashMap;

/// Represents a simplified HTML element, suitable for serialization.
#[derive(Debug, Serialize)]
pub struct Elem {
	pub tag: String,
	pub attrs: Option<HashMap<String, String>>,
	pub text: Option<String>,
	pub inner_html: Option<String>,
}

impl Elem {
	/// Creates a new `Elem` from a `scraper::ElementRef`.
	pub(crate) fn from_element_ref(el_ref: scraper::ElementRef) -> Self {
		let el = el_ref.value();
		let tag = el.name().to_string();

		let attrs = if el.attrs().next().is_some() {
			let attrs = el.attrs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
			Some(attrs)
		} else {
			None
		};

		let full_text = el_ref.text().collect::<String>();
		let text = if full_text.trim().is_empty() {
			None
		} else {
			Some(full_text.to_string())
		};

		let html_content = el_ref.inner_html();
		let inner_html = if html_content.trim().is_empty() {
			None
		} else {
			Some(html_content.to_string())
		};

		Elem {
			tag,
			attrs,
			text,
			inner_html,
		}
	}
}
