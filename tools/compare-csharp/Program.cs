// Runs a canvas-mode .wdproj through the C# WaveDudeNext CPU engine and dumps what `wave-cli compare` reads.
// Usage: compare-csharp <project.wdproj> <out-dir> --steps N [--every K] [--probe-steps]
//        compare-csharp bench [w h] [steps]

using System.Globalization;
using System.Text;
using WaveDudeNext.Engine;
using WaveDudeNext.Export;
using WaveDudeNext.Model;
using WaveDudeNext.Parts;
using WaveDudeNext.Probes;
using WaveDudeNext.Runs;

CultureInfo.DefaultThreadCurrentCulture = CultureInfo.InvariantCulture;
CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;

if (args.Length > 0 && args[0] == "bench")
{
	return Bench.Run(args);
}
if (args.Length < 2)
{
	Console.Error.WriteLine("usage: compare-csharp <project.wdproj> <out-dir> --steps N [--every K] [--probe-steps]");
	return 2;
}
string projectPath = args[0];
string outDir = args[1];
int steps = 600;
int every = 100;
bool probeSteps = false;
for (int i = 2; i < args.Length; i++)
{
	switch (args[i])
	{
		case "--steps": steps = int.Parse(args[++i]); break;
		case "--every": every = int.Parse(args[++i]); break;
		case "--probe-steps": probeSteps = true; break;
		default: Console.Error.WriteLine($"unknown argument {args[i]}"); return 2;
	}
}
Directory.CreateDirectory(outDir);

Project project = Project.Load(projectPath);
if (project.Scene.Mode != SceneMode.Canvas)
{
	Console.Error.WriteLine("only canvas-mode projects: the painter would need the scene image");
	return 1;
}
project.Simulation.Steps = steps;
project.Normalize();
Scene sceneModel = project.Scene;
ScenePixels? pixels = PartPainter.Paint(null, sceneModel.Width, sceneModel.Height, sceneModel.RefractiveIndex, project.Parts);
RunSpec spec = RunSpec.Create(project, pixels, preferGpu: false, trackIntensity: true);
// RunSpec.BuildScene is internal; the same call
SceneData scene = pixels is { } p
	? SceneData.FromBgra(p.Bgra, p.Width, p.Height, p.StrideBytes, false, spec.DampingEdge, spec.Edges == EdgeMode.Open)
	: SceneData.Empty(spec.Width, spec.Height, spec.DampingEdge, spec.RefractiveIndex, spec.Edges == EdgeMode.Open);

int cw = scene.CanvasWidth, ch = scene.CanvasHeight, rim = scene.Rim;
byte[] bgra = pixels?.Bgra ?? Blank(cw, ch, sceneModel.RefractiveIndex);
int rowBytes = pixels?.StrideBytes ?? cw * 4;
int[] packed = new int[cw * ch];
for (int y = 0; y < ch; y++)
{
	Buffer.BlockCopy(bgra, y * rowBytes, packed, y * cw * 4, cw * 4);
}
using (FileStream png = File.Create(Path.Combine(outDir, "scene.png")))
{
	PngEncoder.Write(png, packed, cw, ch);
}

var engine = new WaveSimulator(scene, spec.Engine, spec.Emitters);
File.WriteAllText(Path.Combine(outDir, "scene.txt"), string.Join('\n',
	$"width {cw}", $"height {ch}", $"rim {rim}", $"edges {spec.Edges}", $"stride {engine.Stride}",
	$"stencil {(spec.Engine.FourthOrderLaplacian ? "FourthOrder" : "NinePoint")}",
	$"substeps {SubSteps(scene, spec.Engine.FourthOrderLaplacian)}", $"emitters {spec.Emitters.Length}", $"steps {steps}", $"every {every}", ""));

ProbeSampler? sampler = spec.Probes.Length > 0 ? ProbeSampler.Build(spec.Probes, cw, ch, rim) : null;
var csv = new StringBuilder("step");
foreach (ProbeSpec probe in spec.Probes)
{
	csv.Append(';').Append(probe.Name);
}
csv.Append('\n');

int stride = engine.Stride;
for (int step = 1; step <= steps; step++)
{
	engine.AdvanceOneStep();
	bool dump = step % every == 0;
	if (sampler != null && (probeSteps || dump))
	{
		csv.Append(step);
		for (int k = 0; k < sampler.Count; k++)
		{
			float[] data = (sampler.UseIntensity[k] ? engine.IntensityData : engine.FieldData)!;
			int[] cells = sampler.Pixels[k];
			double sum = 0;
			foreach (int c in cells)
			{
				sum += data[c];
			}
			csv.Append(';').Append((sum / cells.Length).ToString("R"));
		}
		csv.Append('\n');
	}
	if (dump)
	{
		WritePlane(Path.Combine(outDir, $"field_{step}.f32"), engine.FieldData, stride, rim, cw, ch);
		WritePlane(Path.Combine(outDir, $"intensity_{step}.f32"), engine.IntensityData!, stride, rim, cw, ch);
		Console.Error.Write($"\rstep {step}/{steps}");
	}
}
Console.Error.WriteLine();
if (sampler != null)
{
	File.WriteAllText(Path.Combine(outDir, "probes.csv"), csv.ToString());
}
return 0;

static void WritePlane(string path, float[] padded, int stride, int rim, int w, int h)
{
	float[] canvas = new float[w * h];
	for (int y = 0; y < h; y++)
	{
		Array.Copy(padded, (y + rim + 1) * stride + rim + 1, canvas, y * w, w);
	}
	// little-endian hosts only, which every .NET 10 target is
	File.WriteAllBytes(path, System.Runtime.InteropServices.MemoryMarshal.AsBytes(canvas.AsSpan()).ToArray());
}

static byte[] Blank(int w, int h, double index)
{
	byte red = (byte)Math.Clamp((int)Math.Round(index * 100.0), 0, 255);
	byte[] b = new byte[w * h * 4];
	for (int i = 0; i < b.Length; i += 4)
	{
		b[i + 2] = red;
		b[i + 3] = 255;
	}
	return b;
}

// the engine keeps its sub-step count private; read it by reflection for the record
static int SubSteps(SceneData scene, bool fourth)
{
	var probe = new WaveSimulator(scene, new EngineOptions(1, 1, false, fourth, 10, 1));
	return (int?)typeof(WaveSimulator).GetField("_subSteps", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)?.GetValue(probe) ?? -1;
}
