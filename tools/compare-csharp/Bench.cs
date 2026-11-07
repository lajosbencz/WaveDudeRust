// "bench [w h] [steps]": a step's cost on both C# engines, the scene and pattern of `wave-cli bench`
// (rim 32 outside, open edges, one source at the centre, both planes captured as halves every 2 steps,
// pipelined).

using System.Diagnostics;
using WaveDudeNext.Engine;
using WaveDudeNext.Model;
using WaveDudeNext.Runs;

static class Bench
{
	const int Warmup = 20;

	public static int Run(string[] args)
	{
		int w = args.Length > 1 ? int.Parse(args[1]) : 1920;
		int h = args.Length > 2 ? int.Parse(args[2]) : 1080;
		int steps = args.Length > 3 ? int.Parse(args[3]) : 200;
		SceneData scene = SceneData.Empty(w, h, -32, 1.0, openEdges: true);
		Console.WriteLine($"bench {w}x{h}, rim 32, {steps} steps after {Warmup}");
		foreach (bool fourth in new[] { false, true })
		{
			var options = new EngineOptions(1.0, 1.0, true, fourth, 10.0, 1.0);
			Time(new WaveSimulator(scene, options, Sources(w, h)), "CPU", fourth, steps);
			GpuWaveSimulator? gpu = GpuWaveSimulator.TryCreate(scene, options, Sources(w, h), out string problems);
			if (gpu == null)
			{
				Console.WriteLine($"  GPU skipped: {problems}");
				continue;
			}
			using (gpu)
			{
				Time(gpu, "GPU", fourth, steps);
			}
		}
		return 0;
	}

	static List<Emitter> Sources(int w, int h) => [Source(w / 2, h / 2)];

	static Emitter Source(int x, int y) => new()
	{
		X = x, Y = y, Level = 100, Amplitude = 1.0, PhaseRadians = 0, Mode = SourceMode.Continuous, PulseShape = PulseShape.Gaussian,
		OnWidthCycles = 10, OffWidthCycles = 10, OffWidthBase = 10, RandomizeOffWidthAmount = 0, DetachAfterStep = -1, RngSeed = 1,
	};

	static void Time(IWaveEngine engine, string name, bool fourth, int steps)
	{
		for (int i = 0; i < Warmup; i++)
		{
			engine.AdvanceOneStep();
		}
		engine.PrepareFrame(true, false);
		var clock = Stopwatch.StartNew();
		for (int i = 0; i < steps; i++)
		{
			engine.AdvanceOneStep();
		}
		engine.PrepareFrame(true, false);
		double pure = clock.Elapsed.TotalMilliseconds / steps;

		FrameShape full = FrameShape.Full(engine.Width, engine.Height);
		var snapshots = new[] { engine.CreateSnapshot(full, true), engine.CreateSnapshot(full, true), engine.CreateSnapshot(full, true) };
		int next = 0;
		clock.Restart();
		for (int i = 0; i < steps; i++)
		{
			engine.AdvanceOneStep();
			if (i % 2 == 0)
			{
				FrameSnapshot s = snapshots[next];
				next = (next + 1) % snapshots.Length;
				engine.AwaitFrame(s);
				engine.CaptureFrame(s, true);
			}
		}
		foreach (FrameSnapshot s in snapshots)
		{
			engine.AwaitFrame(s);
		}
		double captured = clock.Elapsed.TotalMilliseconds / steps;
		string stencil = fourth ? "4th order" : "9-point";
		Console.WriteLine($"  {name} {stencil}: {pure:0.000} ms a step, {captured:0.000} ms a step with captures every 2");
	}
}
