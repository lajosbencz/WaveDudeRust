// The WaveDude web demo: an example or a dropped project, played on the wasm engine package.
import init, { Simulation, View, requestGpu } from "./pkg/wavedude_wasm.js";

// Time one stepToward call aims for, so the page stays responsive.
const STEP_SLICE_MS = 12;
const MAX_STEPS_PER_CALL = 1 << 20;
// Longest wall time one tick advances playback by, so a stalled page does not jump ahead.
const MAX_ELAPSED_MS = 100;
const GAP_PX = 4;
// Height of the layer name above each picture, with room for the edge line under it.
const CAPTION_PX = 18;

const $ = (id) => document.getElementById(id);
const ui = {
	example: $("example"), file: $("file"), tier: $("tier"), gpu: $("gpu"), status: $("status"),
	stage: $("stage"), play: $("play"), seek: $("seek"), position: $("position"),
};

/** The edge line's width in device pixels on a screen of `ratio`: whole, at least one. */
const hairline = (ratio) => Math.max(1, Math.round(ratio));
const nextFrame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const message = (error) => error?.message ?? String(error);

/** The page's state; the player owns the playback state. `opening` is the project being opened, if any. */
const app = {
	catalog: null, gpu: undefined, player: null, project: null, opening: null, ticket: 0,
	status: { text: "Loading ...", error: false }, dropping: false,
};

/**
 * [read, write, current?]: `render` writes a value only when it differs from the last one written or, for a
 * control the user changes, from `current()`, the control's own value. A read must not call the simulation,
 * which a stepToward in flight borrows.
 */
const bindings = [
	[() => app.status.text, (v) => { ui.status.textContent = v; }],
	[() => app.status.error, (v) => ui.status.classList.toggle("error", v)],
	[() => app.dropping, (v) => document.body.classList.toggle("dropping", v)],
	[() => !app.gpu, (v) => {
		ui.gpu.disabled = v;
		ui.gpu.parentElement.title = v ? "WebGPU is not available in this browser, so the CPU engine runs." : "";
	}],
	[() => (app.opening ?? app.project)?.example?.file ?? "", (v) => { ui.example.value = v; }, () => ui.example.value],
	[() => app.project?.example?.description ?? "", (v) => { ui.example.title = v; }],
	[() => !app.player, (v) => { ui.play.disabled = ui.seek.disabled = v; }],
	[() => (app.player?.playing ? "Pause" : "Play"), (v) => { ui.play.textContent = v; }],
	[() => (app.player?.frameCount ?? 1) - 1, (v) => { ui.seek.max = v; }],
	// The slider is the user's while held.
	[() => String(app.player?.scrubbing ? ui.seek.value : app.player?.target ?? 0), (v) => { ui.seek.value = v; },
		() => ui.seek.value],
	[() => (app.player ? `${app.player.target} / ${app.player.frameCount - 1}` : ""), (v) => { ui.position.textContent = v; }],
	[() => (app.player ? `step ${app.player.step}` : ""), (v) => { ui.position.title = v; }],
	[() => Boolean(app.player?.pending), (v) => ui.position.classList.toggle("pending", v)],
];
const rendered = bindings.map(() => undefined);
let renderQueued = false;

function render() {
	renderQueued = false;
	bindings.forEach(([read, write, current], i) => {
		const value = read();
		if (value !== (current ? current() : rendered[i])) write((rendered[i] = value));
	});
}

/** Renders on the next animation frame, once however many changes come before it. */
function invalidate() {
	if (renderQueued) return;
	renderQueued = true;
	requestAnimationFrame(render);
}

function setStatus(text, error = false) {
	app.status = { text, error };
	invalidate();
}

/**
 * Steps toward the wanted frame and draws what is stored, until stopped. One per opened project.
 * Only its loop calls the simulation, one call at a time: a stepToward in flight borrows it mutably, so the
 * immutable properties are read once here.
 */
class Player {
	constructor(sim) {
		this.sim = sim;
		this.frameCount = sim.frameCount;
		this.fps = sim.fps;
		this.width = sim.canvasWidth;
		this.height = sim.canvasHeight;
		this.playing = false;
		this.scrubbing = false;
		this.stopped = false;
		this.target = 0;
		this.position = 0;
		this.step = 0;
		this.pending = true;
		this.drawn = null;
		this.budget = 16;
		this.wake = null;
		this.changed = false;
		this.views = sim.layers.map((layer) => {
			const picture = document.createElement("div");
			picture.className = "picture";
			const canvas = document.createElement("canvas");
			picture.append(canvas);
			const cell = document.createElement("figure");
			const caption = document.createElement("figcaption");
			caption.textContent = layer;
			caption.style.height = `${CAPTION_PX}px`;
			cell.append(caption, picture);
			ui.stage.append(cell);
			return { layer, cell, picture, canvas, view: new View(canvas), box: "" };
		});
		this.resizes = new ResizeObserver(() => this.#layout());
		this.resizes.observe(ui.stage);
		this.#watchRatio();
		this.done = this.#run();
	}

	/** Lays out again when the device pixel ratio changes, as on a move to another screen. */
	#watchRatio() {
		matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`).addEventListener("change", () => {
			if (this.stopped) return;
			this.#layout();
			this.#watchRatio();
		}, { once: true });
	}

	/**
	 * Stacks the layers or sets them side by side, whichever shows them larger, and fits them to the stage with room
	 * for the edge line around them and their names above them.
	 */
	#layout() {
		const n = this.views.length;
		const ratio = window.devicePixelRatio || 1;
		const edges = (2 * hairline(ratio)) / ratio;
		const [w, h] = [ui.stage.clientWidth - edges, ui.stage.clientHeight - edges];
		const fit = (cellW, cellH) => Math.max(Math.min(cellW / this.width, cellH / this.height), 0);
		const stacked = fit(w, (h - GAP_PX * (n - 1)) / n - CAPTION_PX);
		const besides = fit((w - GAP_PX * (n - 1)) / n, h - CAPTION_PX);
		const scale = Math.max(stacked, besides);
		ui.stage.style.flexDirection = stacked >= besides ? "column" : "row";
		for (const { picture } of this.views) {
			picture.style.width = `${Math.max(1, Math.floor(this.width * scale))}px`;
			picture.style.height = `${Math.max(1, Math.floor(this.height * scale))}px`;
		}
		this.drawn = null;
		this.poke();
	}

	seekTo(frame) {
		this.target = this.position = frame;
		this.poke();
	}

	/** While the slider is held, playback holds. */
	scrub(on) {
		this.scrubbing = on;
		this.poke();
	}

	play(on) {
		if (on && this.target >= this.frameCount - 1) this.seekTo(0);
		this.playing = on;
		this.poke();
	}

	/** Something changed: the loop works again, or once more if it is mid-slice, and the page renders. */
	poke() {
		this.changed = true;
		this.wake?.();
		invalidate();
	}

	/** Ends the loop, waits out a call in flight, and frees the simulation and the views. */
	async stop() {
		this.stopped = true;
		this.poke();
		await this.done;
		this.resizes.disconnect();
		this.sim.free();
		for (const { cell, view } of this.views) {
			view.free();
			cell.remove();
		}
	}

	async #run() {
		let last = performance.now();
		try {
			while (!this.stopped) {
				const now = await nextFrame();
				const elapsed = Math.min(Math.max(now - last, 0), MAX_ELAPSED_MS);
				last = now;
				this.changed = false;
				const more = await this.#tick(elapsed / 1000);
				if (!more && !this.changed && !this.stopped) {
					await new Promise((resolve) => { this.wake = resolve; });
					this.wake = null;
					last = performance.now();
				}
			}
		} catch (error) {
			setStatus(message(error), true);
			this.play(false);
		}
	}

	/** One slice of work, `seconds` after the last; false when nothing is left to do until something changes. */
	async #tick(seconds) {
		const target = this.target;
		const stored = await this.#stepSlice(target);
		const advancing = this.playing && !this.scrubbing;
		if (stored && advancing && target === this.target) {
			this.position = Math.min(this.position + seconds * this.fps, this.frameCount - 1);
			const next = Math.floor(this.position);
			if (next !== target) this.target = next;
			else if (next === this.frameCount - 1) this.playing = false;
		}
		this.#show(stored ? target : this.sim.latestBefore(target));
		this.step = this.sim.stepOf(this.target);
		this.pending = !stored;
		invalidate();
		return !stored || (this.playing && !this.scrubbing);
	}

	/** True when `target` is stored; else steps toward it, and sizes the next slice by the time this one took. */
	async #stepSlice(target) {
		const started = performance.now();
		const stored = await this.sim.stepToward(target, this.budget);
		if (!stored) {
			const spent = Math.max(performance.now() - started, 0.1);
			const scale = Math.min(Math.max(STEP_SLICE_MS / spent, 0.25), 2);
			this.budget = Math.min(Math.max(Math.round(this.budget * scale), 1), MAX_STEPS_PER_CALL);
		}
		return stored;
	}

	/** Draws `frame` on each canvas, which reaches past its picture by the edge line's device pixels. */
	#show(frame) {
		if (frame === undefined || frame === this.drawn) return;
		const ratio = window.devicePixelRatio || 1;
		for (const entry of this.views) {
			const { layer, picture, canvas, view } = entry;
			if (!this.sim.draw(view, frame, layer, picture.clientWidth, picture.clientHeight, ratio)) return;
			// The canvas's CSS box follows the bitmap draw sized.
			const box = `${canvas.width} ${canvas.height} ${ratio}`;
			if (box === entry.box) continue;
			entry.box = box;
			const offset = `${-hairline(ratio) / ratio}px`;
			Object.assign(canvas.style, {
				left: offset, top: offset, width: `${canvas.width / ratio}px`, height: `${canvas.height / ratio}px`,
			});
		}
		this.drawn = frame;
	}
}

/**
 * Opens `text` with the chosen quality and engine; the project on show stays until the new one is ready.
 * `example` is the catalog entry, or undefined for a file.
 */
async function openProject(text, name, example) {
	const ticket = ++app.ticket;
	app.opening = { example };
	setStatus(`Opening ${name} ...`);
	try {
		const sim = ui.gpu.checked
			? await Simulation.openOn(app.gpu, text, ui.tier.value)
			: await Simulation.open(text, ui.tier.value);
		const old = app.player;
		if (ticket === app.ticket) {
			app.player = null;
			await old?.stop();
		}
		if (ticket !== app.ticket) return sim.free();
		app.opening = null;
		const engine = sim.engine === "gpu" ? "GPU" : sim.gpuFellBack ? "CPU (the GPU engine failed)" : "CPU";
		setStatus(`${name}: ${sim.canvasWidth} x ${sim.canvasHeight} cells, ${sim.frameCount} frames at ${sim.fps} fps, ${engine}`);
		app.project = { text, name, example };
		app.player = new Player(sim);
	} catch (error) {
		if (ticket !== app.ticket) return;
		app.opening = null;
		setStatus(`${name}: ${message(error)}`, true);
	}
}

async function openExample(example) {
	const response = await fetch(`examples/${example.file}`);
	if (!response.ok) return setStatus(`${example.file}: ${response.status} ${response.statusText}`, true);
	await openProject(await response.text(), example.title, example);
}

async function openFile(file) {
	await openProject(await file.text(), file.name);
}

/** Fills the example and quality lists from the catalog; they do not change afterwards. */
function fillLists(catalog) {
	for (const topic of catalog.topics) {
		const group = document.createElement("optgroup");
		group.label = topic.title;
		for (const example of catalog.examples.filter((e) => e.topic === topic.id)) {
			const option = new Option(example.title, example.file);
			option.title = example.description;
			group.append(option);
		}
		ui.example.append(group);
	}
	for (const tier of catalog.tiers) ui.tier.append(new Option(tier.title, tier.id, false, tier.id === "Middle"));
}

async function main() {
	await init();
	app.catalog = await (await fetch("examples/catalog.json")).json();
	fillLists(app.catalog);
	if (navigator.gpu) app.gpu = await requestGpu();
	ui.gpu.checked = Boolean(app.gpu);
	setStatus(`Pick an example or drop a .wdproj file.${app.gpu ? "" : " WebGPU is not available, so the CPU engine runs."}`);

	const fail = (error) => setStatus(message(error), true);
	const reopen = () => app.project && openProject(app.project.text, app.project.name, app.project.example);
	const setDropping = (on) => { app.dropping = on; invalidate(); };
	ui.example.addEventListener("change", () => {
		const example = app.catalog.examples.find((e) => e.file === ui.example.value);
		if (example) openExample(example).catch(fail);
	});
	ui.file.addEventListener("change", () => {
		const [file] = ui.file.files;
		ui.file.value = "";
		if (file) openFile(file).catch(fail);
	});
	ui.tier.addEventListener("change", reopen);
	ui.gpu.addEventListener("change", reopen);
	ui.play.addEventListener("click", () => app.player?.play(!app.player.playing));
	ui.seek.addEventListener("input", () => app.player?.seekTo(Number(ui.seek.value)));
	ui.seek.addEventListener("pointerdown", () => app.player?.scrub(true));
	for (const type of ["pointerup", "pointercancel"]) window.addEventListener(type, () => app.player?.scrub(false));
	document.addEventListener("dragover", (event) => {
		event.preventDefault();
		setDropping(true);
	});
	document.addEventListener("dragleave", (event) => {
		if (!event.relatedTarget) setDropping(false);
	});
	document.addEventListener("drop", (event) => {
		event.preventDefault();
		setDropping(false);
		const [file] = event.dataTransfer.files;
		if (file) openFile(file).catch(fail);
	});
}

main().catch((error) => setStatus(`The demo could not start: ${message(error)}`, true));
