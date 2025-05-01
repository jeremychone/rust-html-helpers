use derive_more::{Display, From};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Display, From)]
#[display("{self:?}")]
pub enum Error {
	#[from(String, &String, &str)]
	Custom(String),

	// -- Externals
	#[from]
	Io(std::io::Error), // as example
	// Note: Consider adding specific errors for HTML parsing/serialization if needed
	// #[from]
	// HtmlParseError(html5ever::driver::ParseError), // Example, if you need to expose it
}

// region:    --- Custom

impl Error {
	/// Creates a custom error from any type that implements `std::error::Error`.
	pub fn custom_from_err(err: impl std::error::Error) -> Self {
		Self::Custom(err.to_string())
	}

	/// Creates a custom error from any type that can be converted into a String.
	pub fn custom(val: impl Into<String>) -> Self {
		Self::Custom(val.into())
	}
}

// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate
