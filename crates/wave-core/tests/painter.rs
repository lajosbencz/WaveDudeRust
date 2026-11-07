//! The "Parts and painter" guarantees of docs/spec/70-guarantees.md, ported from the original's
//! library_check where it states them.

use wave_core::model::normalize::Normalize;
use wave_core::model::*;
use wave_core::scene::compose::{SceneKey, compose_scene};
use wave_core::scene::gate_design::{GateDesign, ports};
use wave_core::scene::modes::ModeSolver;
use wave_core::scene::painter::paint_part;
use wave_core::scene::pixels::ScenePixels;
use wave_core::scene::shapes::{BLOCK_ABSORPTION, Vec2, ellipse_vertex_count, flatten, zone_at};

const W: u32 = 200;
const H: u32 = 120;

fn sim() -> Simulation {
	Simulation::default()
}

/// `part` as the model hands it to the painter.
fn normalized(part: &Part) -> Part {
	let mut part = part.clone();
	part.normalize();
	part
}

fn paint_on(mut pixels: ScenePixels, parts: &[Part]) -> ScenePixels {
	for part in parts {
		paint_part(&mut pixels, &normalized(part), &sim());
	}
	pixels
}

/// The part-frame vertices of `kind` once normalised.
fn vertices(kind: &PartKind) -> Option<Vec<Vec2>> {
	wave_core::scene::shapes::vertices(&normalized(&Part::new(kind.clone(), 0, 0)).kind)
}

fn paint(parts: &[Part]) -> ScenePixels {
	paint_on(ScenePixels::blank(W, H, 1.0), parts)
}

fn red(p: &ScenePixels, x: i32, y: i32) -> u8 {
	p.red(x as u32, y as u32)
}

fn blue(p: &ScenePixels, x: i32, y: i32) -> u8 {
	p.blue(x as u32, y as u32)
}

/// Painted area in pixels of a part of index `n` over index 1: an edge pixel's permittivity `n^2` is
/// the area-weighted mean, so its share is `(n_eff^2 - 1) / (n^2 - 1)`.
fn area(p: &ScenePixels, n: f64) -> f64 {
	p.rgba.chunks(4).map(|px| ((px[0] as f64 / 100.0).powi(2) - 1.0) / (n * n - 1.0)).sum()
}

fn count_row(p: &ScenePixels, y: i32, f: impl Fn(u8) -> bool) -> usize {
	(0..p.width as i32).filter(|&x| f(red(p, x, y))).count()
}

fn rows_with(p: &ScenePixels, f: impl Fn(u8) -> bool) -> usize {
	(0..p.height as i32).filter(|&y| (0..p.width as i32).any(|x| f(red(p, x, y)))).count()
}

/// An isosceles triangle with base 40 whose base, at x' = -h/2, is a vertical edge cutting `share` of its
/// pixel column (x = 99 for a share below 1, the part at (110, 60)).
fn base_edge(share: f64) -> Part {
	let height = 2.0 * (10.0 + share);
	let angle = (height / 20.0).atan().to_degrees();
	Part::new(
		PartKind::Triangle(Triangle { base: 40, kind: TriangleType::Isosceles, angle_a: angle, ..Triangle::default() }),
		110,
		60,
	)
}

/// The red level of the effective medium of `share` of index `n` beside index 1.
fn effective_red(share: f64, n: f64) -> u8 {
	((share * n * n + (1.0 - share)).sqrt() * 100.0).round_ties_even() as u8
}

fn lens_part(lens: Lens) -> Part {
	Part::new(PartKind::Lens(lens), 100, 60)
}

fn biconvex() -> Lens {
	Lens { diameter: 100, r1: 300.0, r2: -300.0, thickness: 40, ..Lens::default() }
}

fn with_angle(mut part: Part, angle: f64) -> Part {
	part.base.angle = angle;
	part
}

fn png(pixels: &ScenePixels) -> Vec<u8> {
	let img = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba.clone()).unwrap();
	let mut bytes = Vec::new();
	img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
	bytes
}

#[test]
fn composition_and_disabled_parts() {
	let mut image = ScenePixels::blank(W, H, 1.0);
	image.set_rgba(5, 5, [100, 0, 77, 255]);
	image.set_rgba(100, 60, [100, 160, 33, 255]);
	let mut project = Project::default();
	project.scene.width = W;
	project.scene.height = H;
	assert_eq!(compose_scene(&project), None, "no image, no part: no pixels");
	project.scene.mode = SceneMode::Image;
	project.scene.image_data = Some(png(&image));
	assert_eq!(compose_scene(&project).as_ref(), Some(&image), "no part: the image as is");
	let mut lens = lens_part(biconvex());
	lens.base.enabled = false;
	project.parts.push(lens);
	assert_eq!(compose_scene(&project).as_ref(), Some(&image), "a disabled part leaves the image untouched");
	let key = SceneKey::of(&project);
	project.parts[0].base.enabled = true;
	assert_ne!(SceneKey::of(&project), key);
	let painted = compose_scene(&project).unwrap();
	assert_eq!(SceneKey::of(&project), SceneKey::of(&project.clone()));
	assert_eq!((red(&painted, 100, 60), painted.green(100, 60), blue(&painted, 100, 60)), (150, 160, 33));
	assert_eq!(blue(&painted, 5, 5), 77, "green and blue untouched under the lens");
	assert_eq!(ScenePixels::from_image_bytes(project.scene.image_data.as_ref().unwrap()).unwrap(), image);
}

#[test]
fn blank_canvas_takes_the_background_index() {
	let mut project = Project::default();
	project.scene.width = 40;
	project.scene.height = 30;
	project.scene.refractive_index = 1.33;
	project.parts.push(Part::new(
		PartKind::Rectangle(Rectangle { length: 4, width: 4, ..Rectangle::default() }),
		20,
		15,
	));
	let p = compose_scene(&project).unwrap();
	assert_eq!((p.width, p.height), (40, 30));
	assert_eq!(p.rgba_at(0, 0), [133, 0, 0, 255]);
	assert_eq!(p.red(20, 15), 150);
}

#[test]
fn biconvex_lens() {
	let p = paint(&[lens_part(biconvex())]);
	assert_eq!(count_row(&p, 60, |r| r == 150), 40, "40 px on the axis at red 150");
	assert_eq!(rows_with(&p, |r| r > 100), 100, "100 rows");
	let edge = count_row(&p, 60 + 49, |r| r >= 125);
	assert!((30..=33).contains(&edge), "edge thickness with sag 4 per side: {edge}");
	assert_eq!((red(&p, 80, 60), red(&p, 79, 60), red(&p, 119, 60), red(&p, 120, 60)), (150, 100, 150, 100));
	let p = paint(&[with_angle(lens_part(biconvex()), 90.0)]);
	let column = (0..H as i32).filter(|&y| red(&p, 100, y) == 150).count();
	let columns = (0..W as i32).filter(|&x| (0..H as i32).any(|y| red(&p, x, y) > 100)).count();
	assert_eq!((column, columns), (40, 100), "turned 90 deg: 40 px along y, 100 wide");
}

#[test]
fn straight_edge_takes_the_area_weighted_permittivity() {
	for share in [0.05, 0.1, 0.25, 0.3, 0.4, 0.5, 0.6, 0.75, 0.9, 0.95] {
		let p = paint(&[base_edge(share)]);
		let triangle = vertices(&base_edge(share).kind).unwrap();
		assert!((triangle[0].x + 10.0 + share).abs() < 1e-9, "the base at x 110 - 10 - {share}");
		for y in 50..70 {
			assert_eq!(
				(red(&p, 98, y), red(&p, 99, y), red(&p, 100, y)),
				(100, effective_red(share, 1.5), 150),
				"share {share}, row {y}"
			);
		}
	}
}

#[test]
fn sub_pixel_interface_positions_paint_distinct_levels() {
	let at = |share: f64| red(&paint(&[base_edge(share)]), 99, 60);
	assert_ne!(at(0.25), at(0.75), "a quarter and three quarters of the pixel");
	assert_eq!((at(0.25), at(0.75)), (effective_red(0.25, 1.5), effective_red(0.75, 1.5)));
	let levels: Vec<u8> = (1..20).map(|k| at(k as f64 * 0.05)).collect();
	assert!(levels.windows(2).all(|w| w[0] < w[1]), "no staircase: every 0.05 px a level higher {levels:?}");
}

#[test]
fn coverage_of_a_straight_edge() {
	use wave_core::scene::painter::coverage;
	assert_eq!([coverage(0.0, 1.0, 0.0), coverage(0.25, 0.0, 1.0), coverage(-0.25, 1.0, 0.0)], [0.5, 0.75, 0.25]);
	assert_eq!([coverage(0.5, 1.0, 0.0), coverage(-0.5, 0.0, -1.0)], [1.0, 0.0]);
	let d = std::f64::consts::FRAC_1_SQRT_2;
	assert!((coverage(0.0, d, d) - 0.5).abs() < 1e-12);
	assert!((coverage(0.2, d, -d) - (1.0 - (d - 0.2).powi(2))).abs() < 1e-12, "45 deg: a triangle cut off a corner");
	assert_eq!(coverage(0.71, d, d), 1.0);
	// a 30 deg edge against the exact area of the pixel's samples on a fine grid
	let (nx, ny) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
	for e in [-0.6, -0.4, -0.1, 0.0, 0.3, 0.55] {
		let n = 400;
		let inside = (0..n * n)
			.filter(|i| {
				let (x, y) = ((i % n) as f64 + 0.5, (i / n) as f64 + 0.5);
				nx * (x / n as f64 - 0.5) + ny * (y / n as f64 - 0.5) < e
			})
			.count() as f64
			/ (n * n) as f64;
		assert!((coverage(e, nx, ny) - inside).abs() < 2e-3, "e {e}: {} vs {inside}", coverage(e, nx, ny));
	}
}

fn coated(depth: f64, profile: CoatingProfile, front: bool, back: bool, sides: bool) -> Coating {
	Coating { depth, profile, front, back, sides }
}

#[test]
fn lens_coatings() {
	let lens = |coating| lens_part(Lens { coating, ..biconvex() });
	let p = paint(&[lens(coated(5.0, CoatingProfile::Layer, true, true, true))]);
	assert_eq!(
		[red(&p, 100, 60), red(&p, 100, 107), red(&p, 100, 13), red(&p, 82, 60), red(&p, 86, 60)],
		[150, 122, 122, 122, 150]
	);
	let p = paint(&[lens(coated(5.0, CoatingProfile::Graded, true, true, true))]);
	let ramp: Vec<u8> = (80..88).map(|x| red(&p, x, 60)).collect();
	assert!(ramp[0] <= 103 && ramp[2] == 125 && ramp[5] == 150 && ramp[7] == 150, "{ramp:?}");
	assert!(ramp.windows(2).all(|w| w[0] <= w[1]), "{ramp:?}");
	assert_eq!(
		(red(&p, 117, 60), red(&p, 100, 107)),
		(125, 125),
		"graded band identical on the second surface and the rim"
	);
	let p = paint(&[lens(coated(5.0, CoatingProfile::Layer, false, true, true))]);
	assert_eq!([red(&p, 82, 60), red(&p, 117, 60), red(&p, 100, 107), red(&p, 100, 13)], [150, 122, 122, 122]);
	let p = paint(&[lens(coated(5.0, CoatingProfile::Layer, true, true, false))]);
	assert_eq!(
		[red(&p, 82, 60), red(&p, 117, 60), red(&p, 100, 107), red(&p, 100, 13)],
		[122, 122, 150, 150],
		"bare sides"
	);
}

#[test]
fn shape_faces() {
	let glass = Material { refractive_index: 2.25, ..Material::default() };
	let slab = |coating, angle| {
		with_angle(
			Part::new(
				PartKind::Rectangle(Rectangle {
					material: glass.clone(),
					coating,
					length: 40,
					width: 60,
					linked: false,
				}),
				100,
				60,
			),
			angle,
		)
	};
	let p = paint(&[slab(coated(4.0, CoatingProfile::Layer, false, true, false), 0.0)]);
	assert_eq!(
		[red(&p, 82, 60), red(&p, 117, 60), red(&p, 100, 32), red(&p, 100, 87)],
		[225, 150, 225, 225],
		"only the back coated"
	);
	let p = paint(&[slab(coated(4.0, CoatingProfile::Layer, true, false, true), 0.0)]);
	assert_eq!(
		[red(&p, 82, 60), red(&p, 117, 60), red(&p, 100, 32), red(&p, 100, 87)],
		[150, 225, 150, 150],
		"front and sides coated, back bare"
	);
	let p = paint(&[slab(coated(4.0, CoatingProfile::Layer, true, false, true), 90.0)]);
	assert_eq!(
		[red(&p, 100, 78), red(&p, 100, 42), red(&p, 72, 60), red(&p, 127, 60)],
		[150, 225, 150, 150],
		"the faces turn with the part"
	);
	let disc = Part::new(
		PartKind::Ellipse(Ellipse {
			material: glass,
			coating: coated(4.0, CoatingProfile::Layer, true, true, false),
			length: 60,
			width: 60,
			linked: true,
		}),
		100,
		60,
	);
	let p = paint(&[disc]);
	assert_eq!(
		[red(&p, 72, 60), red(&p, 127, 60), red(&p, 100, 32), red(&p, 100, 87)],
		[150, 150, 225, 225],
		"a circle with bare sides: the band at the leftmost and rightmost points only"
	);
}

#[test]
fn mirror_lens_by_the_centre_test() {
	let mut lens = biconvex();
	lens.material.mirror = true;
	lens.material.absorption = 200;
	lens.coating.depth = 5.0;
	let p = paint(&[lens_part(lens)]);
	assert_eq!(count_row(&p, 60, |r| r == 0), 40);
	assert!(p.rgba.chunks(4).all(|px| (px[0] == 0 || px[0] == 100) && px[2] == 0), "red 0 or untouched, no blue");
}

#[test]
fn fitted_lens_fills_its_diameter() {
	let p = paint(&[lens_part(Lens { diameter: 100, thickness: 20, r1: 50.0, r2: -50.0, ..Lens::default() })]);
	let rows = rows_with(&p, |r| r > 100);
	assert!((98..=100).contains(&rows), "{rows}");
	assert_eq!(count_row(&p, 60, |r| r == 150), 20);
}

fn shape(kind: PartKind) -> Part {
	Part::new(kind, 100, 60)
}

#[test]
fn polygon_shapes() {
	let p = paint(&[shape(PartKind::Polygon(Polygon { sides: 6, side: 30, ..Polygon::default() }))]);
	let expected = 3.0 * 3f64.sqrt() / 2.0 * 900.0;
	assert!((area(&p, 1.5) - expected).abs() < 0.01 * expected);
	assert_eq!(
		[red(&p, 124, 60), red(&p, 126, 60), red(&p, 100, 74), red(&p, 100, 90)],
		[150, 100, 150, 100],
		"hexagon: flat faces at +-x' 26 px out, vertices at +-y' 30 px"
	);
	let hex = vertices(&PartKind::Polygon(Polygon { sides: 6, side: 30, ..Polygon::default() })).unwrap();
	assert!(hex.iter().any(|v| (v.x.abs() < 1e-9) && (v.y + 30.0).abs() < 1e-9));
	assert!(hex.iter().any(|v| (v.x.abs() < 1e-9) && (v.y - 30.0).abs() < 1e-9));
	assert_eq!((p.green(100, 60), p.rgba_at(100, 60)[3], red(&p, 0, 0)), (0, 255, 100));

	let p = paint(&[shape(PartKind::Rectangle(Rectangle { length: 60, width: 20, ..Rectangle::default() }))]);
	assert!((area(&p, 1.5) - 1200.0).abs() < 2.0);
	assert_eq!(
		[
			red(&p, 70, 60),
			red(&p, 69, 60),
			red(&p, 129, 60),
			red(&p, 130, 60),
			red(&p, 100, 50),
			red(&p, 100, 49),
			red(&p, 100, 69),
			red(&p, 100, 70)
		],
		[150, 100, 150, 100, 150, 100, 150, 100],
		"rectangle 60 x 20 at x 70..129, y 50..69"
	);

	let p = paint(&[shape(PartKind::Ellipse(Ellipse { length: 60, width: 40, ..Ellipse::default() }))]);
	let expected = std::f64::consts::PI * 600.0;
	assert!((area(&p, 1.5) - expected).abs() < 0.01 * expected);
	assert_eq!([red(&p, 128, 60), red(&p, 131, 60), red(&p, 100, 78), red(&p, 100, 81)], [150, 100, 150, 100]);
	let circle = vertices(&PartKind::Ellipse(Ellipse { length: 60, width: 60, ..Ellipse::default() })).unwrap();
	assert_eq!((circle.len(), ellipse_vertex_count(60, 60)), (63, 63));
	assert!(circle.iter().all(|v| (v.length() - 30.0).abs() < 1e-9), "circle of 60: 63 vertices all 30 px out");
	assert_eq!((ellipse_vertex_count(4, 4), ellipse_vertex_count(2000, 2000)), (24, 180));
}

#[test]
fn triangles() {
	let tri = |kind, a, b| Triangle { base: 60, kind, angle_a: a, angle_b: b, ..Triangle::default() };
	let v = vertices(&PartKind::Triangle(tri(TriangleType::Equilateral, 0.0, 0.0))).unwrap();
	assert!(
		(v[2].x - v[0].x - 60.0 * 3f64.sqrt() / 2.0).abs() < 1e-9 && v[2].y.abs() < 1e-9 && v[2].x > 0.0,
		"height 52, apex on +x'"
	);
	let v = vertices(&PartKind::Triangle(tri(TriangleType::RightIsosceles, 0.0, 0.0))).unwrap();
	assert!((v[2].x - v[0].x - 30.0).abs() < 1e-9);
	let v = vertices(&PartKind::Triangle(tri(TriangleType::ThirtySixtyNinety, 0.0, 0.0))).unwrap();
	assert!(
		(v[2].y - v[0].y).abs() < 1e-9 && (v[2].x - v[0].x - 60.0 * (std::f64::consts::PI / 6.0).tan()).abs() < 1e-9,
		"30-60-90 height 34.6"
	);
	assert_eq!(tri(TriangleType::Isosceles, 70.0, 10.0).effective_angles(), (70.0, 70.0));
	assert_eq!(
		tri(TriangleType::Custom, 100.0, 100.0).effective_angles(),
		(100.0, 79.0),
		"custom 100 + 100 becomes 100 + 79"
	);
	let p = paint(&[shape(PartKind::Triangle(tri(TriangleType::Equilateral, 60.0, 60.0)))]);
	assert!((area(&p, 1.5) - 1558.8).abs() < 0.01 * 1558.8);
	assert_eq!([red(&p, 75, 60), red(&p, 73, 60), red(&p, 120, 60), red(&p, 127, 60)], [150, 100, 150, 100]);
}

#[test]
fn outline_and_grin_and_bragg() {
	let mut outline = Outline::default();
	let p = paint(&[shape(PartKind::Outline(outline.clone()))]);
	assert!(p.rgba.chunks(4).all(|px| px[0] == 100), "an outline without points paints nothing");
	outline.points = vec![Point::new(-10, -5), Point::new(10, -5), Point::new(10, 5), Point::new(-10, 5)];
	let p = paint(&[shape(PartKind::Outline(outline))]);
	assert!((area(&p, 1.5) - 200.0).abs() < 2.0 && red(&p, 100, 60) == 150 && red(&p, 100, 66) == 100);

	let p = paint(&[shape(PartKind::GrinLens(GrinLens::default()))]);
	assert_eq!(
		[red(&p, 100, 60), red(&p, 100, 85), red(&p, 100, 108), red(&p, 100, 110), red(&p, 149, 60), red(&p, 150, 60)],
		[180, 172, 152, 100, 180, 100],
		"GRIN 100 x 100: 1.8 on the axis to 1.5 at the edge, 1 outside"
	);

	let p = paint(&[shape(PartKind::BraggReflector(BraggReflector {
		pairs: 3,
		thickness1: 10,
		thickness2: 6,
		width: 20,
		..BraggReflector::default()
	}))]);
	let layers: String = (75..125)
		.map(|x| match red(&p, x, 60) {
			150 => '1',
			200 => '2',
			100 => '.',
			_ => '?',
		})
		.collect();
	assert_eq!(layers, format!(".{}.", "1111111111222222".repeat(3)));
	assert_eq!((red(&p, 100, 69), red(&p, 100, 70)), (150, 100));
}

fn fibre(x: i32, nodes: Vec<FiberNode>, f: impl FnOnce(&mut Fiber)) -> Part {
	let mut fiber = Fiber { nodes, ..Fiber::default() };
	f(&mut fiber);
	Part::new(PartKind::Fiber(fiber), x, 60)
}

fn straight(half: i32) -> Vec<FiberNode> {
	vec![
		FiberNode { x: -half, out_x: 67, ..FiberNode::default() },
		FiberNode { x: half, in_x: -67, ..FiberNode::default() },
	]
}

#[test]
fn fibre_core_cladding_and_ends() {
	assert_eq!(flatten(&Fiber::default().nodes).len(), 2, "a straight path is one chord");
	let p = paint(&[fibre(80, straight(100), |_| {})]);
	assert_eq!(
		[red(&p, 80, 60), red(&p, 80, 69), red(&p, 80, 72), red(&p, 80, 79), red(&p, 80, 81), red(&p, 0, 60)],
		[150, 150, 145, 145, 100, 150]
	);
	assert_eq!(
		[red(&p, 179, 60), red(&p, 180, 60), red(&p, 179, 78), red(&p, 180, 78), red(&p, 185, 60)],
		[150, 100, 145, 100, 100],
		"a flat end is exact"
	);
	let p = paint(&[fibre(80, straight(100), |f| f.coating.depth = 3.0)]);
	assert_eq!([red(&p, 177, 60), red(&p, 176, 60), red(&p, 80, 78), red(&p, 80, 76)], [122, 150, 120, 145]);

	let short = |f: fn(&mut Fiber)| paint(&[fibre(100, straight(60), f)]);
	let p = short(|f| f.coating.depth = 3.0);
	assert_eq!(
		[red(&p, 42, 60), red(&p, 157, 60), red(&p, 100, 78)],
		[122, 122, 120],
		"3 px coating on both ends and the tube"
	);
	let p = short(|f| {
		f.coating.depth = 3.0;
		f.coating.front = false;
	});
	assert_eq!([red(&p, 42, 60), red(&p, 157, 60), red(&p, 100, 78)], [150, 122, 120], "bare start face");
	let p = short(|f| {
		f.coating.depth = 3.0;
		f.coating.back = false;
		f.coating.sides = false;
	});
	assert_eq!([red(&p, 42, 60), red(&p, 157, 60), red(&p, 100, 78)], [122, 150, 145], "bare end face and tube");
	let p = short(|f| {
		f.coating = coated(10.0, CoatingProfile::Graded, false, true, false);
	});
	assert_eq!((red(&p, 145, 60), red(&p, 155, 60), red(&p, 42, 60)), (150, 121, 150));
	assert!(red(&p, 159, 60) <= 103);

	let p = paint(&[fibre(80, straight(100), |f| f.ends = FiberEnds::Round)]);
	assert_eq!(
		[red(&p, 185, 60), red(&p, 189, 60), red(&p, 190, 60), red(&p, 199, 60), red(&p, 195, 75)],
		[150, 150, 145, 145, 100],
		"round ends"
	);
}

#[test]
fn fibre_absorption() {
	let p = paint(&[fibre(100, straight(60), |f| {
		f.absorption = 200;
		f.absorption_ramp = 5;
	})]);
	assert_eq!(
		[
			blue(&p, 100, 69),
			blue(&p, 100, 72),
			blue(&p, 100, 74),
			blue(&p, 100, 75),
			blue(&p, 100, 78),
			blue(&p, 100, 80)
		],
		[0, 50, 162, 162, 18, 0],
		"the cladding's hump"
	);
	assert_eq!(
		[blue(&p, 158, 75), blue(&p, 162, 75), blue(&p, 158, 60)],
		[162, 0, 0],
		"stops at the end face; the core stays clear"
	);
	let p = paint(&[fibre(100, straight(60), |f| {
		f.absorption = 200;
		f.absorption_ramp = 5;
		f.cladding_thickness = 0;
	})]);
	assert!(p.rgba.chunks(4).all(|px| px[2] == 0), "a bare core absorbs nothing");
	// Example 23's single-mode fibre: R_c 3, R_o 34, band 10, hump 14..34 with the peak at 24
	let p = paint_on(
		ScenePixels::blank(W, 200, 1.0),
		&[Part::new(
			PartKind::Fiber(Fiber {
				core_diameter: 6,
				cladding_thickness: 31,
				absorption: 252,
				nodes: straight(60),
				..Fiber::default()
			}),
			100,
			100,
		)],
	);
	// pixel row y has r = y + 0.5 - 100
	let at = |r: f64| blue(&p, 100, (100.0 + r - 0.5) as i32);
	assert_eq!(
		[at(13.5), at(16.5), at(19.5), at(23.5), at(24.5), at(28.5), at(33.5), at(34.5)],
		[0, 16, 76, 227, 227, 76, 1, 0]
	);
}

#[test]
fn bent_fibre_follows_the_curve() {
	let nodes = vec![
		FiberNode { x: -60, y: 40, out_y: -80, ..FiberNode::default() },
		FiberNode { x: 60, y: 40, in_y: -80, ..FiberNode::default() },
	];
	let path = flatten(&nodes);
	assert!(path.len() > 20 && path.iter().all(|v| v.x.abs() <= 60.0 && v.y >= -20.0001 && v.y <= 40.0));
	assert!(path.iter().map(|v| (v.y + 20.0).abs()).fold(f64::INFINITY, f64::min) < 0.5);
	assert_eq!((path[0], path[path.len() - 1]), (Vec2::new(-60.0, 40.0), Vec2::new(60.0, 40.0)));
	let p = paint(&[fibre(80, nodes, |_| {})]);
	assert_eq!(
		[red(&p, 80, 35), red(&p, 80, 25), red(&p, 80, 15), red(&p, 80, 100)],
		[150, 145, 100, 100],
		"painted along the curve"
	);
	assert_eq!(
		[red(&p, 20, 99), red(&p, 20, 100), red(&p, 140, 99), red(&p, 140, 100)],
		[150, 100, 150, 100],
		"flat ends across the handles"
	);
}

#[test]
fn later_part_paints_over_an_earlier() {
	let square = |half: i32, mirror: bool, n: f64| {
		let points =
			[(-half, -half), (half, -half), (half, half), (-half, half)].map(|(x, y)| Point::new(x, y)).to_vec();
		Part::new(
			PartKind::Outline(Outline {
				material: Material { mirror, refractive_index: n, ..Material::default() },
				points,
				..Outline::default()
			}),
			32,
			24,
		)
	};
	let p = paint_on(ScenePixels::blank(64, 48, 1.0), &[square(16, true, 1.5), square(12, false, 1.0)]);
	assert_eq!(
		[red(&p, 32, 24), red(&p, 32, 10), red(&p, 32, 6)],
		[100, 0, 100],
		"the mirror square hollowed to a 4 px ring"
	);
}

#[test]
fn absorption_profiles() {
	let square = |f: fn(&mut Rectangle)| {
		let mut r = Rectangle { length: 100, width: 100, ..Rectangle::default() };
		r.material.refractive_index = 1.0;
		f(&mut r);
		Part::new(PartKind::Rectangle(r), 100, 60)
	};
	let p = paint(&[square(|_| {})]);
	assert!(p.rgba.chunks(4).all(|px| px[2] == 0), "no absorption, no blue");
	let part = square(|r| {
		r.material.absorption = 200;
		r.material.absorption_ramp = 20;
	});
	let p = paint(std::slice::from_ref(&part));
	let e = |d: f64| (200.0 * (d / 20.0) * (d / 20.0)).round_ties_even() as u8;
	assert_eq!(
		[
			blue(&p, 49, 60),
			blue(&p, 50, 60),
			blue(&p, 59, 60),
			blue(&p, 69, 60),
			blue(&p, 70, 60),
			blue(&p, 100, 60),
			blue(&p, 149, 60)
		],
		[0, e(0.5), e(9.5), e(19.5), 200, 200, e(0.5)]
	);
	assert!(p.rgba.chunks(4).all(|px| px[0] == 100 && px[1] == 0), "index 1 and green untouched");
	let mut image = ScenePixels::blank(W, H, 1.0);
	image.set_rgba(51, 60, [100, 0, 230, 255]);
	image.set_rgba(100, 60, [100, 0, 10, 255]);
	image.set_rgba(5, 5, [100, 0, 77, 255]);
	let p = paint_on(image, std::slice::from_ref(&part));
	assert_eq!([blue(&p, 51, 60), blue(&p, 100, 60), blue(&p, 5, 5)], [230, 200, 77], "the larger blue wins");
	let p = paint(&[square(|r| {
		r.material.absorption = 200;
		r.material.absorption_ramp = 20;
		r.length = 10;
	})]);
	assert_eq!(p.rgba.chunks(4).map(|px| px[2]).max(), Some(200), "a thin part reaches full strength in its middle");
	let p = paint(&[square(|r| {
		r.material.absorption = 200;
		r.material.absorption_ramp = 20;
		r.material.mirror = true;
	})]);
	assert!(p.rgba.chunks(4).all(|px| px[2] == 0), "a mirror ignores its absorption");
	let p = paint(&[square(|r| {
		r.material.absorption = 900;
		r.material.absorption_ramp = 0;
	})]);
	assert_eq!(
		[blue(&p, 50, 60), blue(&p, 51, 60), blue(&p, 100, 60)],
		[64, 255, 255],
		"absorption clamped to 255, ramp to at least 1"
	);

	// spec 20's example: Example 18's "Absorber 1"
	let absorber = Rectangle {
		material: Material { absorption: 255, absorption_ramp: 77, refractive_index: 1.0, ..Material::default() },
		length: 62,
		width: 172,
		..Rectangle::default()
	};
	let p = paint_on(ScenePixels::blank(200, 400, 1.0), &[Part::new(PartKind::Rectangle(absorber), 90, 161)]);
	assert_eq!((blue(&p, 110, 161), blue(&p, 90, 161)), (30, 255));
}

fn gate_part(kind: GateKind, f: impl FnOnce(&mut LogicGate)) -> (Part, std::sync::Arc<GateDesign>) {
	let mut gate = LogicGate { kind, ..LogicGate::default() };
	f(&mut gate);
	gate.normalize();
	let design = GateDesign::for_gate(&gate, &sim()).unwrap();
	let width = design.length as i32 + 400;
	let height = 2 * design.port_spacing as i32 + 200;
	(Part::new(PartKind::LogicGate(gate), width / 2, height / 2), design)
}

fn gate_canvas(design: &GateDesign) -> ScenePixels {
	ScenePixels::blank(design.length as u32 + 400, 2 * design.port_spacing as u32 + 200, 1.0)
}

fn gate_ports(part: &Part) -> [Point; 4] {
	let PartKind::LogicGate(g) = &part.kind else { unreachable!() };
	ports(&part.base, g, &sim()).unwrap()
}

#[test]
fn logic_gate_layout_and_absorbers() {
	let (part, d) = gate_part(GateKind::Xor, |_| {});
	let p = paint_on(gate_canvas(&d), std::slice::from_ref(&part));
	let pt = gate_ports(&part);
	let (gx, gy) = (part.base.x, part.base.y);
	let w = d.core_width as i32;
	let (start, end) = d.phase_section();
	let phase_x = gx + ((start + end) / 2.0) as i32;
	assert_eq!(
		[
			red(&p, pt[0].x + 2, pt[0].y),
			red(&p, pt[0].x + 2, pt[0].y - w / 2 - 3),
			red(&p, phase_x, pt[0].y),
			red(&p, phase_x, pt[1].y),
			red(&p, pt[2].x, pt[2].y),
			red(&p, gx, 2)
		],
		[255, 145, 245, 255, 255, 100],
		"cores 2.55 in cladding 1.45, the phase section 2.45 on arm A only"
	);
	let last = gx + d.length as i32 / 2 - 1;
	let first = gx - d.length as i32 / 2 + 1;
	assert_eq!(
		[
			red(&p, last, pt[2].y),
			red(&p, last, pt[2].y - w / 2 - 3),
			red(&p, first, pt[0].y),
			red(&p, last - 6 - 1, pt[2].y)
		],
		[160, 120, 160, 255],
		"AR bands at both ends"
	);
	let rim_y = pt[2].y - (d.half_width() - d.port_spacing as f64 / 2.0) as i32 + 1;
	assert_eq!(
		[blue(&p, pt[0].x + 2, pt[0].y), blue(&p, gx, gy), blue(&p, pt[2].x, pt[2].y), blue(&p, first, pt[0].y)],
		[0, 0, 0, 0],
		"no absorption along the cores, the coupler, the outputs, the inputs"
	);
	assert!(blue(&p, pt[2].x, rim_y) > 50, "the cladding's rim absorbs");
	let x = d.coupler_start() as i32 + gx + 5;
	for y in gy - d.coupled as i32 / 2 - w / 2..gy + d.coupled as i32 / 2 + w / 2 {
		assert_eq!(blue(&p, x, y), 0, "coupler row {y}");
	}

	let (upper_off, _) = gate_part(GateKind::Xor, |g| g.upper_output_active = false);
	let p = paint_on(gate_canvas(&d), &[upper_off]);
	let half = blue(&p, last - d.ramp_len as i32 / 2, pt[2].y);
	assert!(
		blue(&p, last, pt[2].y) > 50
			&& blue(&p, last, pt[3].y) == 0
			&& blue(&p, pt[2].x, pt[2].y) == 0
			&& (6..30).contains(&half)
	);

	let (closed, _) = gate_part(GateKind::Xor, |g| {
		g.upper_output_active = false;
		g.lower_output_active = false;
	});
	let p = paint_on(gate_canvas(&d), &[closed]);
	for port in [pt[2], pt[3]] {
		for y in port.y - w / 2..port.y + w / 2 {
			assert!(blue(&p, last, y) >= 60, "both outputs off: the core at the end absorbs ({y})");
		}
	}
}

#[test]
fn mode_solver_beta_matches_the_painted_guide() {
	let (part, d) = gate_part(GateKind::Xor, |_| {});
	let p = paint_on(gate_canvas(&d), std::slice::from_ref(&part));
	let pt = gate_ports(&part);
	let x = pt[0].x + 2;
	let gy = part.base.y;
	// arm A's band, y' from -P to 0, padded with 8 rows of index 1 as the solver's own profile
	let mut rows = vec![1.0; 8];
	rows.extend((gy - d.port_spacing as i32..gy).map(|y| red(&p, x, y) as f64 / 100.0));
	rows.extend([1.0; 8]);
	let solver = ModeSolver::for_simulation(&sim());
	let om = solver.omega_term(d.wavelength);
	let mode = solver.beta(&rows, om, false).expect("a guided mode");
	assert!((mode.beta - d.beta).abs() < 1e-9, "{} vs {}", mode.beta, d.beta);
	assert!(mode.beta / (std::f64::consts::TAU / d.wavelength) > d.cladding_index);
}

#[test]
fn ports_track_move_turn_and_frequency() {
	let (part, d) = gate_part(GateKind::Xor, |_| {});
	let before = gate_ports(&part);
	let mut moved = part.clone();
	moved.base.x += 40;
	moved.base.y -= 7;
	assert_eq!(gate_ports(&moved), before.map(|p| Point::new(p.x + 40, p.y - 7)));
	let mut turned = part.clone();
	turned.base.angle = 90.0;
	let t = gate_ports(&turned);
	assert!((t[1].x - t[0].x - d.port_spacing as i32).abs() <= 1 && (t[1].y - t[0].y).abs() <= 1, "{t:?}");
	assert!((t[0].y - part.base.y - 256).abs() <= 1, "the inputs lie below the centre at 90 deg");
	let mut retuned = part.clone();
	let PartKind::LogicGate(g) = &mut retuned.kind else { unreachable!() };
	g.design_frequency = 70.0;
	assert_ne!(gate_ports(&retuned), before);
	let mut scaled = sim();
	scaled.source_frequency_scale = 2.5;
	let PartKind::LogicGate(g) = &part.kind else { unreachable!() };
	assert_ne!(ports(&part.base, g, &scaled).unwrap(), before);
}

#[test]
fn zone_plate() {
	let plate = ZonePlate { diameter: 480, focal_length: 360, ..ZonePlate::default() };
	let lambda = wavelength(plate.design_frequency, 1.0, 2.0);
	let f = plate.focal_length as f64;
	let radius = |n: f64, lambda: f64| (n * lambda * f + (n * lambda / 2.0).powi(2)).sqrt();
	assert_eq!(zone_at(radius(1.0, lambda) - 0.5, lambda, 360), 0);
	assert_eq!(zone_at(radius(1.0, lambda) + 0.5, lambda, 360), 1);
	assert_eq!(zone_at(radius(3.0, lambda) + 0.5, lambda, 360), 3);

	let (cx, cy) = (240, 320);
	let part = Part::new(PartKind::ZonePlate(plate.clone()), cx, cy);
	let p = paint_on(ScenePixels::blank(960, 640, 1.0), std::slice::from_ref(&part));
	let r1 = radius(1.0, lambda);
	let block_y = cy + ((r1 + radius(2.0, lambda)) / 2.0).round() as i32;
	let quarter = blue(&p, cx - 16, block_y);
	assert!(
		blue(&p, cx, block_y) >= BLOCK_ABSORPTION as u8 - 4 && blue(&p, cx, cy) == 0 && blue(&p, cx - 32, block_y) < 10
	);
	assert!((11..60).contains(&quarter) && red(&p, cx, block_y) == 100);
	assert!(blue(&p, cx, cy + r1 as i32 - 2) == 0 && blue(&p, cx, cy + r1 as i32 + 2) > 50);
	assert_eq!(blue(&p, cx, cy + 240 + 30), 0, "no mount: nothing beyond the diameter");
	assert!((cy + 241..640).all(|y| blue(&p, cx, y) == 0) && (0..cy - 241).all(|y| blue(&p, cx, y) == 0));

	let mounted = Part::new(PartKind::ZonePlate(ZonePlate { mount: true, mount_width: 120, ..plate.clone() }), cx, cy);
	let p = paint_on(ScenePixels::blank(960, 640, 1.0), &[mounted]);
	assert!(blue(&p, cx, cy + 270) >= 92 && blue(&p, cx, cy - 270) >= 92 && blue(&p, cx, cy) == 0);

	// twice the frequency scale halves the wavelength: the first block starts at the halved lambda's r_1
	let first_block = |scale: f64| {
		let mut s = sim();
		s.source_frequency_scale = scale;
		let mut p = ScenePixels::blank(960, 640, 1.0);
		paint_part(&mut p, &normalized(&part), &s);
		(cy..640).find(|&y| blue(&p, cx, y) > 0).unwrap()
	};
	let expected = |lambda: f64| (cy as f64 + radius(1.0, lambda) - 0.5).ceil() as i32;
	assert_eq!(wavelength(50.0, 1.0, 4.0), lambda / 2.0);
	assert_eq!(first_block(2.0), expected(lambda));
	assert_eq!(first_block(4.0), expected(lambda / 2.0));
	assert!(first_block(4.0) < first_block(2.0));
}

/// A compact picture of a painting: a hash of the bytes and 25 x 25 cells of 8 x 8 pixels, each
/// the mean red change (`.` none, else a digit of |mean - 100| / 16, `-` for a lower index) and
/// a second grid for blue.
fn summary(p: &ScenePixels) -> String {
	let hash = p.rgba.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3));
	let mut out = format!("fnv {hash:016x}\n");
	for channel in [0usize, 2] {
		for cy in 0..25u32 {
			for cx in 0..25u32 {
				let mut sum = 0.0;
				for y in cy * 8..cy * 8 + 8 {
					for x in cx * 8..cx * 8 + 8 {
						sum += p.rgba_at(x, y)[channel] as f64;
					}
				}
				let mean = sum / 64.0 - if channel == 0 { 100.0 } else { 0.0 };
				out.push(match mean {
					m if m.abs() < 0.5 => '.',
					m if m < 0.0 => '-',
					m => char::from_digit(((m / 16.0) as u32).min(9), 10).unwrap(),
				});
			}
			out.push('\n');
		}
		out.push('\n');
	}
	out
}

fn kinds() -> Vec<(&'static str, PartKind)> {
	let glass = |n: f64| Material { refractive_index: n, ..Material::default() };
	vec![
		(
			"lens",
			PartKind::Lens(Lens {
				diameter: 120,
				r1: 150.0,
				r2: -150.0,
				thickness: 40,
				coating: coated(4.0, CoatingProfile::Graded, true, true, true),
				..Lens::default()
			}),
		),
		(
			"polygon",
			PartKind::Polygon(Polygon {
				sides: 5,
				side: 60,
				material: Material { absorption: 100, absorption_ramp: 10, ..glass(2.0) },
				..Polygon::default()
			}),
		),
		(
			"rectangle",
			PartKind::Rectangle(Rectangle {
				length: 80,
				width: 40,
				coating: coated(3.0, CoatingProfile::Layer, true, false, true),
				..Rectangle::default()
			}),
		),
		("ellipse", PartKind::Ellipse(Ellipse { length: 100, width: 50, material: glass(1.8), ..Ellipse::default() })),
		(
			"triangle",
			PartKind::Triangle(Triangle {
				base: 80,
				kind: TriangleType::Custom,
				angle_a: 50.0,
				angle_b: 70.0,
				..Triangle::default()
			}),
		),
		(
			"outline",
			PartKind::Outline(Outline {
				points: [(-60, -30), (40, -50), (70, 20), (0, 60), (-40, 20)].map(|(x, y)| Point::new(x, y)).to_vec(),
				material: Material { mirror: true, ..Material::default() },
				..Outline::default()
			}),
		),
		(
			"fiber",
			PartKind::Fiber(Fiber {
				core_diameter: 10,
				cladding_thickness: 12,
				absorption: 120,
				absorption_ramp: 4,
				nodes: vec![
					FiberNode { x: -80, y: 30, out_x: 40, out_y: -60, ..FiberNode::default() },
					FiberNode { x: 80, y: -20, in_x: -60, in_y: 40, ..FiberNode::default() },
				],
				..Fiber::default()
			}),
		),
		(
			"bragg",
			PartKind::BraggReflector(BraggReflector {
				pairs: 4,
				thickness1: 7,
				thickness2: 5,
				width: 120,
				..BraggReflector::default()
			}),
		),
		(
			"grin",
			PartKind::GrinLens(GrinLens {
				length: 80,
				diameter: 60,
				coating: coated(3.0, CoatingProfile::Layer, true, true, false),
				..GrinLens::default()
			}),
		),
		(
			"gate",
			PartKind::LogicGate(LogicGate {
				design_frequency: 160.0,
				upper_output_active: false,
				..LogicGate::default()
			}),
		),
		(
			"zone_plate",
			PartKind::ZonePlate(ZonePlate {
				diameter: 180,
				focal_length: 100,
				thickness: 16,
				coating: 6,
				design_frequency: 120.0,
				mount: true,
				mount_width: 8,
				..ZonePlate::default()
			}),
		),
	]
}

#[test]
fn snapshots_of_each_kind() {
	for (name, kind) in kinds() {
		let mut text = String::new();
		for angle in [0.0, 17.0, 45.0, 90.0] {
			let mut part = Part::new(kind.clone(), 100, 100);
			part.base.angle = angle;
			let p = paint_on(ScenePixels::blank(200, 200, 1.0), &[part]);
			text.push_str(&format!("angle {angle}\n{}", summary(&p)));
		}
		insta::assert_snapshot!(name, text);
	}
}

#[test]
#[ignore = "timing; run with --ignored --nocapture in release"]
fn timing_full_hd() {
	let parts = [
		(
			"outline 400 x 330",
			PartKind::Outline(Outline {
				points: [(-200, -165), (120, -165), (200, -60), (170, 165), (-60, 140), (-200, 90)]
					.map(|(x, y)| Point::new(x, y))
					.to_vec(),
				..Outline::default()
			}),
		),
		(
			"lens 400 px",
			PartKind::Lens(Lens { diameter: 400, r1: 600.0, r2: -600.0, thickness: 80, ..Lens::default() }),
		),
		("hexagon", PartKind::Polygon(Polygon::default())),
		(
			"S-bent fibre",
			PartKind::Fiber(Fiber {
				nodes: vec![
					FiberNode { x: -900, y: -300, out_x: 600, ..FiberNode::default() },
					FiberNode { x: 900, y: 300, in_x: -600, ..FiberNode::default() },
				],
				..Fiber::default()
			}),
		),
	];
	let blank = ScenePixels::blank(1920, 1080, 1.0);
	for (name, kind) in parts {
		let part = normalized(&Part::new(kind, 960, 540));
		let mut times: Vec<f64> = (0..7)
			.map(|_| {
				let mut p = blank.clone();
				let t = std::time::Instant::now();
				paint_part(&mut p, &part, &sim());
				t.elapsed().as_secs_f64() * 1e3
			})
			.collect();
		times.sort_by(f64::total_cmp);
		eprintln!("{name}: median {:.2} ms (min {:.2})", times[3], times[0]);
	}
}
