//! The unit a number field is measured in, by the field's `(type, member)` names.

/// A physical unit; its text is the app string `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
	Pixel,
	Cell,
	Degree,
	Step,
	Cycle,
	Percent,
	PixelPerStep,
	FramePerSecond,
}

impl Unit {
	pub(super) fn key(self) -> &'static str {
		match self {
			Self::Pixel => "UnitPixel",
			Self::Cell => "UnitCell",
			Self::Degree => "UnitDegree",
			Self::Step => "UnitStep",
			Self::Cycle => "UnitCycle",
			Self::Percent => "UnitPercent",
			Self::PixelPerStep => "UnitPixelPerStep",
			Self::FramePerSecond => "UnitFramePerSecond",
		}
	}
}

/// The unit of `member` of `ty` (the names the labels and hints are keyed by); none for a dimensionless field.
pub(super) fn of(ty: &str, member: &str) -> Option<Unit> {
	use Unit::*;
	Some(match (ty, member) {
		("Scene", "Width" | "Height" | "DampingEdge") => Pixel,
		("Simulation", "Steps" | "VisualizeEvery") => Step,
		("Simulation", "WaveSpeed") => PixelPerStep,
		("Display", "ExportFrom" | "ExportTo") => Step,
		("Video", "Fps") => FramePerSecond,
		("Source", "X" | "Y") => Cell,
		("Source", "Amplitude") => Percent,
		("Source", "Phase") => Degree,
		("Source", "PulseWidth" | "OffWidth") => Cycle,
		("Source", "DetachAfter") => Step,
		("SourceArray", "SpacingX" | "SpacingY" | "RandomPosition") => Pixel,
		("SourceArray", "Angle" | "RandomPhase") => Degree,
		("SourceArray", "RandomOffWidth") => Cycle,
		("Part", "X" | "Y") => Pixel,
		("Part", "Angle") => Degree,
		("Point", "X" | "Y") => Pixel,
		("Coating", "Depth") => Pixel,
		("Material", "AbsorptionRamp") => Pixel,
		("Rectangle" | "Ellipse", "Length" | "Width") => Pixel,
		("Polygon", "Side") => Pixel,
		("Triangle", "Base") => Pixel,
		("Triangle", "AngleA" | "AngleB") => Degree,
		("Lens", "Diameter" | "R1" | "R2" | "Thickness") => Pixel,
		("Diffuser", "Length" | "Width" | "CenterWavelength" | "Amplitude" | "Sigma") => Pixel,
		("InterferenceFilter", "Length" | "Width") => Pixel,
		("FilterLayer", "Thickness") => Pixel,
		("SimpleGrating", "ColumnSpacing" | "RowSpacing" | "Diameter") => Pixel,
		("FacettedGrating", "Length" | "Width" | "Pitch" | "Amplitude") => Pixel,
		("BraggReflector", "Thickness1" | "Thickness2" | "Width") => Pixel,
		("GrinLens", "Length" | "Diameter") => Pixel,
		("MetaSurface", "Length" | "Width" | "Pitch" | "Height") => Pixel,
		("MetaLens", "FocalLength") => Pixel,
		("MetaPrism", "Deflection") | ("MetaAxicon", "ConeAngle") => Degree,
		("ZonePlate", "FocalLength" | "Diameter" | "Thickness" | "Coating" | "MountWidth") => Pixel,
		("Fiber", "CoreDiameter" | "CladdingThickness" | "AbsorptionRamp") => Pixel,
		("LogicGate", "Coating" | "PortSpacing") => Pixel,
		("Probe", "Ax" | "Ay" | "Bx" | "By" | "BoxX" | "BoxY" | "BoxWidth" | "BoxHeight" | "BinSize") => Pixel,
		("Probe", "Window" | "StartAfter") => Step,
		_ => return None,
	})
}
