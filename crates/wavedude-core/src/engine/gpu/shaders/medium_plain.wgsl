// Per cell: (c2, d) as floats.
@group(0) @binding(5) var<storage, read> medium: array<vec2<f32>>;

fn medium_at(i: u32) -> vec2<f32> {
	return medium[i];
}
