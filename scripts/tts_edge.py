#!/usr/bin/env python3
"""Free TTS via Microsoft Edge — no API key.

Reads UTF-8 text on stdin, writes raw f32-LE PCM @ 48kHz mono to stdout.
Voice picked from MIMI_TTS_VOICE env (default fr-FR-HenriNeural).

Pipeline: edge-tts CLI -> mp3 (temp file) -> ffmpeg -> f32le pcm on stdout.
Uses the `edge-tts` binary on PATH (pipx-installed) so this script works
with the system Python — no venv / no `import edge_tts` required.
"""
import os
import shutil
import subprocess
import sys
import tempfile


def main() -> int:
    text = sys.stdin.read().strip()
    if not text:
        print("tts_edge: empty stdin", file=sys.stderr)
        return 2
    voice = os.environ.get("MIMI_TTS_VOICE", "fr-FR-HenriNeural")
    rate = os.environ.get("MIMI_TTS_RATE", "+0%")
    edge_bin = os.environ.get("MIMI_TTS_EDGE_BIN") or shutil.which("edge-tts")
    if not edge_bin:
        print("tts_edge: edge-tts binary not on PATH", file=sys.stderr)
        return 3

    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as f:
        mp3_path = f.name
    try:
        subprocess.run(
            [edge_bin, "--voice", voice, "--rate", rate,
             "--text", text, "--write-media", mp3_path],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        subprocess.run(
            ["ffmpeg", "-hide_banner", "-loglevel", "error",
             "-i", mp3_path,
             "-f", "f32le", "-ac", "1", "-ar", "48000", "-"],
            stdout=sys.stdout.buffer,
            check=True,
        )
    finally:
        try:
            os.unlink(mp3_path)
        except OSError:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
