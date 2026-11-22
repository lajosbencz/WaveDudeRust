fn main() {
	waveduderust_icon_subset::subset_icon_fonts(&[
		"MaterialSymbolsRounded_Filled-Regular.ttf",
		"MaterialSymbolsRounded-Regular.ttf",
	])
	.expect("subsetting the icon fonts");
}
