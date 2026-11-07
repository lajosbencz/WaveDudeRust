# 0009 Core answers to the core map's open questions

Status: accepted, 2026-11-06 (the user's answers)

1. CPU and GPU engines agree within a documented tolerance (0003), not bit
   for bit.
2. The logic gate layout and the waveguide mode solver are derived from the
   original's code into formulas in the spec and reimplemented from those.
3. The fibre cladding's absorption hump and the zone plate mount's absorption
   are documented as smooth profiles in the spec and implemented from it.
4. What an export burns in (probe boxes, markers, labels) follows the UI spec.
5. MP4 through openh264 (0005).
6. The written `.wdproj` DOCTYPE declares the same elements, attributes and
   enumerations as the original's; not byte for byte.
7. Scene image: the embedded bytes are the truth, the path is informative;
   undecodable bytes are an error naming the file.
8. A rectangle or ellipse loaded with Linked=true and Length != Width takes
   Length = Width, whatever the attribute order.
9. A Wave Dude 2D JSON scheme with colours and no stops gets evenly spaced
   stops.
10. Lossy exports (JPEG, MPEG-1, MP4) must reach at least the original's
    quality: PSNR >= 40 dB against the rendered frame at JPEG quality 90 and
    the default video bitrate, and never below the original's measured PSNR
    for the same settings.
11. Probe CSV: shortest round-trip float formatting, `\n` line endings,
    semicolons, decimal point, a header row.
