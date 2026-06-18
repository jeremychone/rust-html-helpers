// region:    --- Types

/// Options for the `slim` function (indentation, etc.).
#[derive(Clone, Copy, Debug, Default)]
pub struct SlimOptions {
	/// Whether to use tabs instead of spaces for indentation.
	pub indent_with_tabs: bool,
	/// Number of spaces per indentation level, or `None` for flat output.
	pub indent: Option<u8>,
}

// endregion: --- Types

// region:    --- Constructors & Fluid API

impl SlimOptions {
	/// Set the number of spaces for indentation (enables formatting).
	pub fn with_indent(mut self, spaces: u8) -> Self {
		self.indent = Some(spaces);
		self
	}

	/// Use tabs instead of spaces for indentation.
	pub fn with_indent_with_tabs(mut self, tabs: bool) -> Self {
		self.indent_with_tabs = tabs;
		self
	}
}

// endregion: --- Constructors & Fluid API

// region:    --- Tests

#[cfg(test)]
mod tests {
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_slim_options_default() -> TestResult<()> {
		// -- Setup & Fixtures
		// -- Exec
		let opts = SlimOptions::default();

		// -- Check
		assert!(!opts.indent_with_tabs, "indent_with_tabs should default to false");
		assert!(opts.indent.is_none(), "indent should default to None");

		Ok(())
	}

	#[test]
	fn test_slim_options_with_indent() -> TestResult<()> {
		// -- Setup & Fixtures
		// -- Exec
		let opts = SlimOptions::default().with_indent(4);

		// -- Check
		assert_eq!(opts.indent, Some(4));
		assert!(!opts.indent_with_tabs);

		Ok(())
	}

	#[test]
	fn test_slim_options_with_indent_with_tabs() -> TestResult<()> {
		// -- Setup & Fixtures
		// -- Exec
		let opts = SlimOptions::default().with_indent_with_tabs(true);

		// -- Check
		assert!(opts.indent_with_tabs);
		assert!(opts.indent.is_none());

		Ok(())
	}

	#[test]
	fn test_slim_options_combined() -> TestResult<()> {
		// -- Setup & Fixtures
		// -- Exec
		let opts = SlimOptions::default().with_indent(2).with_indent_with_tabs(true);

		// -- Check
		assert_eq!(opts.indent, Some(2));
		assert!(opts.indent_with_tabs);

		Ok(())
	}
}

// endregion: --- Tests
