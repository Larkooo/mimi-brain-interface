#!/usr/bin/env python3
"""Free TTS via Microsoft Edge — no API key.

Reads UTF-8 text on stdin, writes raw f32-LE PCM @ 48kHz mono to stdout.
Voice picked from MIMI_TTS_VOICE env (default fr-FR-HenriNeural).

Pipeline: edge-tts -> mp3 (temp file) -> ffmpeg -> f32le pcm on stdout.
"""
import asyncio
import os
import sys
import tempfile
import subprocess

import edge_tts


async def main():
    text = sys.stdin.read().strip()
    if not text:
        print("tts_edge: empty stdin", file=sys.stderr)
        sys.exit(2)
    voice = os.environ.get("MIMI_TTS_VOICE", "fr-FR-HenriNeural")
    rate = os.environ.get("MIMI_TTS_RATE", "+0%")

    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as f:
        mp3_path = f.name
    try:
        comm = edge_tts.Communicate(text, voice, rate=rate)
        await comm.save(mp3_path)
        subprocess.run(
            [
                "ffmpeg", "-hide_banner", "-loglevel", "error",
                "-i", mp3_path,
                "-f", "f32le", "-ac", "1", "-ar", "48000", "-",
            ],
            stdout=sys.stdout.buffer,
            check=True,
        )
    finally:
        try:
            os.unlink(mp3_path)
        except OSError:
            pass


if __name__ == "__main__":
    asyncio.run(main())
