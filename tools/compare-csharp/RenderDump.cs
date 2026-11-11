// What `--render` adds to the dump: Next's own pictures of the planes, the brightness it would fit and its probe graph layouts.

using System.Globalization;
using System.Text;
using WaveDudeNext.Export;
using WaveDudeNext.Model;
using WaveDudeNext.Probes;
using WaveDudeNext.Rendering;

/// <summary>The planes as the preview stores and colours them: the field as halves, the intensity as halves of its square root.</summary>
sealed class RenderDump
{
	private readonly Project _project;
	private readonly string _dir;
	private readonly int _w, _h;
	private readonly ColorLut _fieldLut, _intensityLut;
	private readonly StringBuilder _fit = new();

	public RenderDump(Project project, string dir, int w, int h)
	{
		_project = project;
		_dir = dir;
		_w = w;
		_h = h;
		// the catalog of WaveDudeNext.UI without the user's folder, which a dump must not depend on
		List<ColorScheme> builtIn = ColorSchemeStore.LoadBuiltIn(null);
		_fieldLut = Lut(builtIn, project.Display.Field);
		_intensityLut = Lut(builtIn, project.Display.Intensity);
	}

	private ColorLut Lut(List<ColorScheme> builtIn, DisplayLayer layer)
	{
		ColorScheme? scheme = ColorSchemeStore.Find(_project.ColorSchemes, layer.ColorScheme) ?? ColorSchemeStore.Find(builtIn, layer.ColorScheme);
		return scheme == null ? ColorLut.Grey : ColorLut.Build(scheme, layer.BlackLevel);
	}

	public void Dump(int step, float[] field, float[] intensity)
	{
		Half[] fieldHalves = Array.ConvertAll(field, v => (Half)v);
		Half[] rootHalves = Array.ConvertAll(intensity, v => (Half)MathF.Sqrt(v));
		Paint($"field_{step}.png", fieldHalves, _fieldLut, SampleKind.Field, _project.Display.Field.Brightness);
		Paint($"intensity_{step}.png", rootHalves, _intensityLut, SampleKind.IntensityRoot, _project.Display.Intensity.Brightness);
		string Fit(double? f) => f?.ToString("R", CultureInfo.InvariantCulture) ?? "-";
		_fit.Append(step).Append(' ').Append(Fit(BrightnessFit.For(fieldHalves, SampleKind.Field)))
			.Append(' ').Append(Fit(BrightnessFit.For(rootHalves, SampleKind.IntensityRoot))).Append('\n');
	}

	private void Paint(string name, Half[] halves, ColorLut lut, SampleKind kind, double brightness)
	{
		int[] pixels = new int[_w * _h];
		FrameRenderer.Render(halves, _w, _h, lut, kind, (float)brightness, pixels);
		using FileStream png = File.Create(Path.Combine(_dir, name));
		PngEncoder.Write(png, pixels, _w, _h);
	}

	/// <summary>fit.txt, and graph.txt with the series and profile of every probe beside it (see the README).</summary>
	public void Finish(IReadOnlyList<ProbeSpec> probes, ProbeTrace[] traces, int steps, int frame)
	{
		File.WriteAllText(Path.Combine(_dir, "fit.txt"), _fit.ToString());
		var graph = new StringBuilder();
		for (int k = 0; k < probes.Count; k++)
		{
			Probe probe = probes[k].Origin!;
			ProbeGraphData data = ProbeGraphData.At(traces[k], steps - 1, frame);
			BoxRect box = ProbeGraph.BoxInCanvas(probe, _w, _h);
			// the range of what the layout draws: a line probe shows nothing before its start step
			bool started = probe.StartAfter <= 0 || data.StepsSampled >= probe.StartAfter;
			ProbeGraph.ComputeRanges(probe, probe.ShowsProfile && !started ? default : data, steps, out _, out _, out double yMin, out double yMax);
			ProbeGraphLayout layout = ProbeGraph.Layout(probe, data, steps, box.Width, box.Height, ProbeGraphStyle.Estimate(11f), CultureInfo.InvariantCulture);
			string Ticks(IReadOnlyList<GraphTick> ticks) => string.Join(' ', ticks.Select(t => $"{t.Label}@{t.Position.ToString("F2", CultureInfo.InvariantCulture)}"));
			float[] series = data.Values is null ? [] : data.Values[..data.Count];
			float[] profile = data.Profile ?? [];
			WriteFloats(Path.Combine(_dir, $"series_{k}.f32"), series);
			WriteFloats(Path.Combine(_dir, $"profile_{k}.f32"), profile);
			graph.Append(string.Join('\t', k, series.Length, profile.Length, data.ProfileLength.ToString("R", CultureInfo.InvariantCulture), data.StepsSampled,
				yMin.ToString("R", CultureInfo.InvariantCulture), yMax.ToString("R", CultureInfo.InvariantCulture), Ticks(layout.YTicks), Ticks(layout.XTicks))).Append('\n');
		}
		File.WriteAllText(Path.Combine(_dir, "graph.txt"), graph.ToString());
	}

	// little-endian hosts only, which every .NET 10 target is
	public static void WriteFloats(string path, float[] values) =>
		File.WriteAllBytes(path, System.Runtime.InteropServices.MemoryMarshal.AsBytes(values.AsSpan()).ToArray());
}
