#!/usr/bin/env python3
"""Generates the two default notification chimes shipped in `static/sounds/`.

These are the out-of-the-box sounds Seraphim plays when a task needs attention
and when a task finishes; an operator can override either with a custom upload in
Settings. They are committed as plain 16-bit mono WAV (universally playable in the
browser, and reproducible from this script). Run from `frontend/`:

    python3 scripts/gen-sounds.py

Keep the output short and gentle: these fire unattended, so they should be
noticeable without being jarring.
"""

import math
import struct
import wave
from pathlib import Path

SAMPLE_RATE = 44_100
OUT_DIR = Path(__file__).resolve().parent.parent / "static" / "sounds"


def tone(freq: float, seconds: float, amplitude: float = 0.32) -> list[float]:
    """A single bell-like note: a fundamental plus a soft octave harmonic under a
    fast-attack, exponential-decay envelope (so it rings rather than clicks)."""
    samples: list[float] = []
    total = int(seconds * SAMPLE_RATE)
    attack = int(0.005 * SAMPLE_RATE)  # 5ms attack avoids a start click
    for i in range(total):
        t = i / SAMPLE_RATE
        envelope = math.exp(-3.2 * t)  # natural decay
        if i < attack:
            envelope *= i / attack
        wave_value = math.sin(2 * math.pi * freq * t)
        wave_value += 0.25 * math.sin(2 * math.pi * freq * 2 * t)  # octave shimmer
        samples.append(amplitude * envelope * wave_value)
    return samples


def sequence(notes: list[tuple[float, float, float]]) -> list[float]:
    """Plays notes one after another, each `(freq, start, duration)` in seconds, so
    overlapping ring-outs blend. Returns the mixed mono buffer."""
    end = max(start + duration for _, start, duration in notes)
    buffer = [0.0] * int((end + 0.3) * SAMPLE_RATE)
    for freq, start, duration in notes:
        offset = int(start * SAMPLE_RATE)
        for i, value in enumerate(tone(freq, duration)):
            if offset + i < len(buffer):
                buffer[offset + i] += value
    return buffer


def write_wav(path: Path, samples: list[float]) -> None:
    peak = max((abs(s) for s in samples), default=1.0) or 1.0
    # Leave a little headroom so the mix never clips.
    scale = min(1.0, 0.9 / peak)
    frames = b"".join(
        struct.pack("<h", int(max(-1.0, min(1.0, s * scale)) * 32_767)) for s in samples
    )
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(SAMPLE_RATE)
        out.writeframes(frames)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    # Attention: a rising two-tone "di-dum" that reads as "look here".
    attention = sequence([(659.25, 0.0, 0.45), (987.77, 0.13, 0.5)])
    write_wav(OUT_DIR / "attention.wav", attention)

    # Completion: a quick ascending major arpeggio (C-E-G-C) that reads as "done".
    completion = sequence(
        [
            (523.25, 0.0, 0.35),
            (659.25, 0.09, 0.35),
            (783.99, 0.18, 0.4),
            (1046.50, 0.27, 0.6),
        ]
    )
    write_wav(OUT_DIR / "completion.wav", completion)

    for name in ("attention.wav", "completion.wav"):
        size = (OUT_DIR / name).stat().st_size
        print(f"wrote {OUT_DIR / name} ({size} bytes)")


if __name__ == "__main__":
    main()
