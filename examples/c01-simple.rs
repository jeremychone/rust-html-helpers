fn main() -> Result<(), Box<dyn std::error::Error>> {
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

	let slim = html_helpers::slim(fx_html)?;

	println!("Slim:\n\n{slim}");

	Ok(())
}
