# Optics

These projects put the program's newer parts, the Fresnel lens, the diffuser, the interference filter, the two
gratings, the three meta surfaces and the two probabilistic detectors, on textbook optics problems. Every project
names the problem and its designed numbers in its description; the probes sit where the result is read. The files are
WaveDude Next projects (`.wdproj`, format version 4) and open in both programs.

Wavelengths at the default frequency scale: frequency 50 is 32 px, 80 is 20 px, 100 is 16 px, 150 is 10.7 px. A
plane wave is a composite source, a column of emitters 8 px apart (4 px for the 16 px wave of `02`, so that both
colours there have the same amplitude: a line of emitters radiates in proportion to its density times the
wavelength). The brightness of each project suits the program's units (an intensity of 1 at the palette's top);
WaveDude Next shows the pictures darker.

| File | What it shows | Wikipedia |
|---|---|---|
| `01-fresnel-lens-lighthouse.wdproj` | A point source at the focus (600 px) of a Fresnel lens, diameter 400, radii 600 and -600, 3 zones of equal depth on a 24 px plate, index 1.5. The line probe Beam, 450 px behind the lens, reads a 400 px wide collimated band. | Fresnel lens |
| `02-grating-spectrometer.wdproj` | A plane wave of 32 and 16 px through a line grating of period 96 px (mirror dots of 48 px) and a lens of focal length 600. The line probe Spectrum reads the zero order and the first orders at 99 and 197 px from the axis (designed 101 and 212). | Diffraction grating |
| `03-dichroic-filter.wdproj` | A quarter-wave stack for 32 px, 8 pairs of 2.00 at 4.00 px and 1.50 at 5.33 px plus one 2.00, on a glass plate tilted 20 degrees. The 32 px beam reflects at 40 degrees, the 20 px beam passes; line probes Reflected and Transmitted. | Dichroic filter |
| `04-flat-lens.wdproj` | A plane wave of 32 px through a meta lens of focal length 300, 400 px wide (ridges of index 2.00, 32 px high, cells of 4 px). The point probe Focus reads 1.48 at 300 px, the line probe Focal plane a spot 31 px wide at half maximum (designed 24). | Metalens |
| `05-beam-steering.wdproj` | The same wave through a meta prism of 20 degrees (zones of 93.6 px). The line probe Downstream, 500 px behind, finds the beam 194 px above the axis (designed 182). | Metalens |
| `06-bessel-beam.wdproj` | A plane wave of 16 px through a meta axicon of 10 degrees, 360 px wide (cells of 2 px, ridges 16 px high). Point probes Near, Middle and Far on the axis at 200, 500 and 800 px; the line probe Across reads a core 21 px wide at half maximum with the rings of a Bessel beam. | Axicon, Bessel beam |
| `07-rayleigh-criterion.wdproj` | Two point sources 78 px apart imaged 1:1 by a lens of focal length 200 and diameter 100 (object and image at 400 px). The imaging detector Sensor builds a histogram of two peaks with a dip to a third of their height; the line probe Image reads the same integrated intensity smoothly. | Angular resolution |
| `08-shot-noise.wdproj` | A point source and two counting detectors 400 px away: Faint counts (sensitivity 30) expects 2.6 detections a step and jumps between 0 and 9, Bright counts (300) expects 26 and scatters by 5. The point probe Expectation and the line probe Fluence read the smooth intensity. | Shot noise |
| `09-ground-glass.wdproj` | A point source of 16 px through a diffuser (ripples of 24 px, amplitude 6 px, spread 8 px) gives speckle; a second source below a gently absorbing wall gives the smooth far field for comparison. Line probes Speckle and Smooth. | Speckle pattern |
| `10-blazed-grating.wdproj` | A plane wave of 16 px at 45 degrees on a mirror sawtooth grating of 64 px pitch and 12 px amplitude (blaze 20.6 degrees). The third order leaves along the normal and carries the light (0.11 against 0.018 in the zero order); line probes Blazed order and Zero order. | Blazed grating |

## How the numbers were chosen

### Fresnel lens lighthouse

A symmetric lens of index 1.5 has a focal length equal to its radius, so radii of 600 and -600 put the focus 600 px
before the lens and the lamp there. Equal depths cut the 34.3 px sag of a 400 px lens into three zones of 11.4 px,
which a 24 px plate holds; the zones are 232, 48 and 36 px wide. At 16 px the outer zones are two wavelengths wide
and diffract into fans, so the wave has frequency 150 (10.7 px; the 4th-order stencil's phase velocity is off by
0.02 % there) and the zones are 3 to 22 wavelengths. The beam still shows the zones' diffraction, a band of
0.0002 to 0.0006 with ripples, and the mount (absorption 255) keeps the light outside the lens out of the picture.
The collimation is visible but not clean; a Fresnel lens wants zones far wider than the wavelength.

### Diffraction grating spectrometer

sin(theta) = wavelength / period gives 19.5 degrees for 32 px and 9.6 degrees for 16 px at a period of 96 px. The
dots are half the period (48 px), so the second orders vanish and the first orders take the light the zero order
leaves. A 288 px beam lights three periods; its orders would need thousands of pixels to separate in free space
(the far field begins at width squared over wavelength), so a lens of focal length 600 brings every order to a spot
in its focal plane, 600 tan(theta) from the axis, as the camera lens of a spectrometer does. The lens is 440 px
across so that the 19.5 degree order from the beam's edge still enters it; the measured spots lie 5 % inside the
designed positions since the thick lens (90 px) focuses a little shorter than its radii say.

### Dichroic interference filter

A quarter-wave layer is the wavelength over four times the index: 4.00 px at 2.00 and 5.33 px at 1.50 for 32 px.
Eight pairs and a final high-index layer reflect 98 % at the design wavelength at normal incidence
(R = ((1 - (nH/nL)^(2N) nH^2 / ns) / (1 + ...))^2 with ns = 1.5); the stopband spans 29 to 35 px, and 20 px lies
far outside it. At 20 degrees the layers' optical thickness falls by 1.5 to 2.6 % (cos of the refracted angle), a
shift of the stopband well below its width. The two colours arrive as separate beams, the 32 px one above, so one
picture shows the upper beam turning away at 40 degrees and the lower beam passing; a few percent of the 32 px light
leaks through (0.009 against 0.089 for the passed beam, which is itself the weaker source). The filter's angle is
negative so the reflection leaves upward, where the probe sits 260 px along the reflected path.

### Flat lens, beam steering, Bessel beam

The meta surfaces take their design wavelength from the design frequency and their height from a full turn of
phase, wavelength over (index - 1): 32 px for ridges of index 2.00 at 32 px, 16 px at 16 px. The lens is f/0.75 and
the spot a wavelength times the focal length over the width, 24 px; the measured 31 px at half maximum includes
the lens's finite zones. The prism's zone is wavelength over sin 20 degrees, 93.6 px, so 4 px cells give 23 cells
a zone; the beam lands within 12 px of 500 tan 20 degrees. The axicon turns each half of a 360 px wave by 10
degrees, so the halves overlap over 180 / tan 10 = 1021 px, and the core is about a wavelength over twice sin 10
degrees, 46 px between its first zeros (21 px at half maximum). The axicon runs at 16 px with 2 px cells, since
at 32 px a 10 degree cone makes a 92 px core, no narrower than the picture.

### Rayleigh criterion

The lens images 1:1 from 400 px to 400 px (twice its focal length). The sources are 1.22 x 16 x 400 / 100 = 78 px
apart, the Rayleigh spacing of a circular aperture of diameter 100 at an image distance of 400. The two sources
differ by 10 frequency levels so their interference beats with a period of 160 steps and averages out of the
integrated intensity. The 2D aperture is a slit, whose first zero lies at 1.0 rather than 1.22 wavelengths times
the f-number, so the peaks are a little better than barely resolved: the dip falls to a third of the peaks. The
mount is two absorbing blocks that stop the light passing around the lens; the detector's sensitivity of 20 gives
some 3500 counts over the run in 4 px bins, enough for the two peaks to stand out of the Poisson noise. The line probe Image reads from the first step; the imaging detector Sensor starts at step 800, when the wave reaches the image plane, so its histogram holds no empty frames.

### Shot noise

The intensity at the detectors is 9e-4 per pixel; a line of 100 px at a sensitivity of 30 expects 2.7 detections a
step, so the counts are small integers with a spread of 1.6, the square root of the mean; the second detector at
300 expects 27 and scatters by 5, a fifth of its mean. Both start after 600 steps, when the wave has settled.

### Ground glass

The diffuser's relief adds thirteen ripples about 24 px with a spread of 8 px; at 6 px amplitude the face is
about 20 px high, which at index 1.5 is 10 px of path, more than half a wavelength of 16 px, so the transmitted
phase is scrambled and the far field is speckle. The second path has no plate. A wall of absorption 255 reflects
like a mirror (a damping of 1 - 255/255 per step stops the wave dead), so the wall here absorbs 16 rising over
50 px on either side of a 40 px core: the wave loses a few percent per pixel, leaks 1 % in intensity to the other
half and reflects little, the lower probe reading a flat 0.0003 with a small rise next to the wall.

### Blazed grating

With 4 wavelengths per period the orders at 45 degrees incidence lie at sin(theta) = 0.707 - m / 4: 45, 27, 12 and
-2.5 degrees for m = 0 to 3. A sawtooth of 12 px amplitude on 64 px has a blaze angle of atan(24 / 64) = 20.6
degrees, whose facets reflect the 45 degree wave to 2 x 20.6 - 45 = -3.9 degrees, within 1.4 degrees of the third
order, which therefore leaves along the normal with six times the zero order's intensity. The beam comes from the
lower right so that the blazed order leaves straight down, away from the incident beam, and the source sits 450 px
from the grating along its 45 degree path. The second order (12 degrees) still shows as fringes of 46 px in the
blazed beam. Finer pitches (3 and 1.5 wavelengths) were tried and blaze less cleanly.

## Figures

The `figures` folder holds the intensity at the last frame of every project, with the glowing edges, the probe
markers and their graphs (`wavedude-cli export --still --markers --graphs --edges`), and the painted scenes of the
Fresnel lens, the spectrometer, the filter and the blazed grating, all reduced to 640 px wide.
