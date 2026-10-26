# Audio fixtures

`tone.opus` contains a generated 440 Hz tone, 0.12 seconds, 48 kHz stereo.
It was encoded with FFmpeg's libopus encoder and contains no recorded music.
The fixture is shared with Cranamp's Apache-2.0 test suite.

The WAV, FLAC, AIFF, MP2 and ALAC/M4A files contain the same generated tone from Cranamp. `video.webm` pairs a 16 by 16 black VP9 video with Opus audio; it verifies playback of the audio track from a video container. It was made with:

```sh
ffmpeg -f lavfi -i color=black:s=16x16:d=0.12 -i tone.wav -c:v libvpx-vp9 -c:a libopus -shortest video.webm
```
